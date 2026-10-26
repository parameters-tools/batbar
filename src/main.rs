use std::env;
use std::fs;
use std::io::{self, IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::process;
use std::thread;
use std::time::Duration;

const POWER_SUPPLY_DIR: &str = "/sys/class/power_supply";

const RESET: &str = "\x1b[0m";
const BOLD: &str = "\x1b[1m";
const RED: &str = "\x1b[31m";
const GREEN: &str = "\x1b[32m";
const YELLOW: &str = "\x1b[33m";
const CYAN: &str = "\x1b[36m";
const GREY: &str = "\x1b[90m";

const BAR_WIDTH: usize = 20;
const NAME_W_MIN: usize = 7;

const DIVIDER_EXTRA: usize = 69;

struct Config {
    all: bool,
    json: bool,
    color: bool,
    watch: Option<f64>,
}

struct Battery {
    name: String,
    manufacturer: Option<String>,
    model: Option<String>,
    technology: Option<String>,
    status: String,
    capacity: Option<u8>,
    capacity_level: Option<String>,
    present: bool,
    energy_now: Option<u64>,
    energy_full: Option<u64>,
    energy_full_design: Option<u64>,
    charge_now: Option<u64>,
    charge_full: Option<u64>,
    charge_full_design: Option<u64>,
    voltage_now: Option<u64>,
    power_now: Option<u64>,
    current_now: Option<u64>,
    cycle_count: Option<u32>,
}

struct Supply {
    name: String,
    kind: String,
    online: Option<bool>,
    status: Option<String>,
}

fn read_trim(path: &Path) -> Option<String> {
    fs::read_to_string(path)
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

fn read_u64(path: &Path) -> Option<u64> {
    read_trim(path)?.parse().ok()
}

fn read_i64(path: &Path) -> Option<i64> {
    read_trim(path)?.parse().ok()
}

impl Battery {
    fn load(dir: &Path) -> Option<Battery> {
        if read_trim(&dir.join("type")).as_deref() != Some("Battery") {
            return None;
        }

        let name = read_trim(&dir.join("name"))
            .or_else(|| dir.file_name().map(|n| n.to_string_lossy().into_owned()))?;

        let present = read_trim(&dir.join("present"))
            .map(|s| s != "0")
            .unwrap_or(true);

        Some(Battery {
            name,
            manufacturer: read_trim(&dir.join("manufacturer")),
            model: read_trim(&dir.join("model_name")),
            technology: read_trim(&dir.join("technology")),
            status: read_trim(&dir.join("status")).unwrap_or_else(|| "Unknown".to_string()),
            capacity: read_u64(&dir.join("capacity")).map(|v| v.min(100) as u8),
            capacity_level: read_trim(&dir.join("capacity_level")),
            present,
            energy_now: read_u64(&dir.join("energy_now")),
            energy_full: read_u64(&dir.join("energy_full")),
            energy_full_design: read_u64(&dir.join("energy_full_design")),
            charge_now: read_u64(&dir.join("charge_now")),
            charge_full: read_u64(&dir.join("charge_full")),
            charge_full_design: read_u64(&dir.join("charge_full_design")),
            voltage_now: read_u64(&dir.join("voltage_now")),
            power_now: read_u64(&dir.join("power_now")),
            current_now: read_i64(&dir.join("current_now")).map(|v| v.unsigned_abs()),
            cycle_count: read_u64(&dir.join("cycle_count")).map(|v| v.min(u32::MAX as u64) as u32),
        })
    }

    fn energy_now_uw(&self) -> Option<u64> {
        self.energy_now.or_else(|| {
            let charge = self.charge_now? as u128;
            let voltage = self.voltage_now? as u128;
            Some((charge * voltage / 1_000_000) as u64)
        })
    }

    fn energy_full_uw(&self) -> Option<u64> {
        self.energy_full.or_else(|| {
            let charge = self.charge_full? as u128;
            let voltage = self.voltage_now? as u128;
            Some((charge * voltage / 1_000_000) as u64)
        })
    }

    fn energy_full_design_uw(&self) -> Option<u64> {
        self.energy_full_design.or_else(|| {
            let charge = self.charge_full_design? as u128;
            let voltage = self.voltage_now? as u128;
            Some((charge * voltage / 1_000_000) as u64)
        })
    }

    fn power_uw(&self) -> Option<u64> {
        if let Some(power) = self.power_now.filter(|&power| power > 0) {
            return Some(power);
        }
        let current = self.current_now? as u128;
        let voltage = self.voltage_now? as u128;
        if current == 0 || voltage == 0 {
            return None;
        }
        Some((current * voltage / 1_000_000) as u64)
    }

    fn percent(&self) -> Option<u8> {
        if let Some(capacity) = self.capacity {
            return Some(capacity.min(100));
        }
        let now = self.energy_now_uw()? as f64;
        let full = self.energy_full_uw()? as f64;
        if full <= 0.0 {
            return None;
        }
        Some((now / full * 100.0).round().clamp(0.0, 100.0) as u8)
    }

    fn health_pct(&self) -> Option<u8> {
        let full = self.energy_full_uw()? as f64;
        let design = self.energy_full_design_uw()? as f64;
        if design <= 0.0 {
            return None;
        }
        Some((full / design * 100.0).round().clamp(0.0, 255.0) as u8)
    }

    fn eta(&self) -> Option<(&'static str, u64)> {
        let power = self.power_uw()?;
        if power == 0 {
            return None;
        }
        match self.status.to_ascii_lowercase().as_str() {
            "charging" => {
                let full = self.energy_full_uw()?;
                let now = self.energy_now_uw()?;
                if full <= now {
                    return None;
                }
                Some(("full", eta_seconds((full - now) as f64, power)))
            }
            "discharging" => {
                let now = self.energy_now_uw()?;
                Some(("empty", eta_seconds(now as f64, power)))
            }
            _ => None,
        }
    }
}

fn eta_seconds(energy_uw: f64, power_uw: u64) -> u64 {
    ((energy_uw / power_uw as f64) * 3600.0).round() as u64
}

fn scan_supplies(read_online: bool) -> (Vec<Battery>, Vec<Supply>) {
    let mut batteries = Vec::new();
    let mut others = Vec::new();

    let entries = match fs::read_dir(POWER_SUPPLY_DIR) {
        Ok(entries) => entries,
        Err(_) => return (batteries, others),
    };

    let mut paths: Vec<PathBuf> = entries.filter_map(|e| e.ok()).map(|e| e.path()).collect();
    paths.sort();

    for path in paths {
        let kind = read_trim(&path.join("type")).unwrap_or_default();
        if kind == "Battery" {
            if let Some(battery) = Battery::load(&path) {
                batteries.push(battery);
            }
        } else if !kind.is_empty() {
            others.push(Supply {
                name: read_trim(&path.join("name")).unwrap_or_else(|| {
                    path.file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_default()
                }),
                kind,
                online: if read_online {
                    read_trim(&path.join("online")).map(|s| s == "1")
                } else {
                    None
                },
                status: read_trim(&path.join("status")),
            });
        }
    }

    (batteries, others)
}

fn paint(text: &str, code: &str, color: bool) -> String {
    if color {
        format!("{code}{text}{RESET}")
    } else {
        text.to_string()
    }
}

fn level_color(pct: u8) -> &'static str {
    if pct <= 15 {
        RED
    } else if pct <= 30 {
        YELLOW
    } else {
        GREEN
    }
}

fn state_color(status: &str) -> &'static str {
    match status.to_ascii_lowercase().as_str() {
        "charging" => CYAN,
        "discharging" => YELLOW,
        "full" => GREEN,
        _ => GREY,
    }
}

fn make_bar(pct: u8, color: bool) -> String {
    let filled = ((pct as usize * BAR_WIDTH) / 100).min(BAR_WIDTH);
    let unfilled = BAR_WIDTH - filled;

    let mut bar = String::with_capacity(BAR_WIDTH + 12);
    if color {
        bar.push_str(level_color(pct));
    }
    bar.push('[');
    bar.push_str(&"█".repeat(filled));
    if color {
        bar.push_str(GREY);
    }
    bar.push_str(&"░".repeat(unfilled));
    bar.push(']');
    if color {
        bar.push_str(RESET);
    }
    bar
}

fn fmt_duration(seconds: u64) -> String {
    let minutes = (seconds + 30) / 60;
    let hours = minutes / 60;
    let mins = minutes % 60;
    if hours > 0 {
        format!("{hours}h{mins:02}m")
    } else {
        format!("{mins}m")
    }
}

fn eta_label(battery: &Battery) -> String {
    match battery.eta() {
        Some((kind, seconds)) => format!("{kind} {}", fmt_duration(seconds)),
        None => "—".to_string(),
    }
}

fn health_label(battery: &Battery) -> String {
    battery
        .health_pct()
        .map(|h| format!("{h}%"))
        .unwrap_or_else(|| "—".to_string())
}

fn cycles_label(battery: &Battery) -> String {
    battery
        .cycle_count
        .map(|c| c.to_string())
        .unwrap_or_else(|| "—".to_string())
}

fn inferred_ac(batteries: &[Battery]) -> Option<bool> {
    if batteries.is_empty() {
        return None;
    }
    let any_plugged = batteries.iter().any(|b| {
        matches!(
            b.status.to_ascii_lowercase().as_str(),
            "charging" | "full" | "not charging"
        )
    });
    if any_plugged {
        return Some(true);
    }
    if batteries
        .iter()
        .all(|b| b.status.eq_ignore_ascii_case("discharging"))
    {
        return Some(false);
    }
    None
}

fn ac_state(batteries: &[Battery], others: &[Supply]) -> Option<bool> {
    let mains: Vec<&Supply> = others.iter().filter(|s| s.kind == "Mains").collect();
    if mains.iter().any(|s| s.online.is_some()) {
        return Some(mains.iter().any(|s| s.online == Some(true)));
    }
    inferred_ac(batteries)
}

fn render_text(batteries: &[Battery], others: &[Supply], cfg: &Config) {
    let visible: Vec<&Battery> = batteries.iter().filter(|b| cfg.all || b.present).collect();

    if visible.is_empty() {
        println!("No battery found. Run with --all to list every power supply.");
    } else {
        let name_w = visible
            .iter()
            .map(|b| b.name.len())
            .max()
            .unwrap_or(NAME_W_MIN)
            .max(NAME_W_MIN);

        let header = format!(
            "{:<nw$} {:<12} {:<22} {:>4}  {:<12} {:>6} {:>6}",
            "BATTERY",
            "STATE",
            "CHARGE",
            "LVL",
            "ETA",
            "HEALTH",
            "CYCLES",
            nw = name_w
        );
        let divider = "-".repeat(name_w + DIVIDER_EXTRA);

        println!();
        println!(
            "{}",
            paint("🔋 BATTERY STATUS (Direct sysfs)", BOLD, cfg.color)
        );
        println!("{divider}");
        println!("{}", paint(&header, BOLD, cfg.color));
        println!("{divider}");

        for battery in &visible {
            let pct = battery.percent();
            let bar = make_bar(pct.unwrap_or(0), cfg.color);
            let level = match pct {
                Some(p) => format!("{p:>3}%"),
                None => format!("{:>4}", "—"),
            };
            let state = format!("{:<12}", battery.status);
            let state = paint(&state, state_color(&battery.status), cfg.color);
            let eta = eta_label(battery);
            let health = format!("{:>6}", health_label(battery));
            let cycles = format!("{:>6}", cycles_label(battery));

            println!(
                "{:<nw$} {} {} {}  {:<12} {} {}",
                battery.name,
                state,
                bar,
                level,
                eta,
                health,
                cycles,
                nw = name_w
            );
        }

        println!("{divider}");
    }

    if let Some(online) = ac_state(batteries, others) {
        let (label, code) = if online {
            ("AC: connected", GREEN)
        } else {
            ("AC: disconnected", RED)
        };
        println!("{}", paint(label, code, cfg.color));
    }

    if cfg.all {
        println!();
        println!("{}", paint("POWER SUPPLIES", BOLD, cfg.color));
        println!("{:-<60}", "");
        println!(
            "{}",
            paint(
                &format!("{:<22} {:<12} {:<8} {}", "NAME", "TYPE", "ONLINE", "STATUS"),
                BOLD,
                cfg.color
            )
        );
        println!("{:-<60}", "");
        for supply in others {
            let online = match supply.online {
                Some(true) => "yes",
                Some(false) => "no",
                None => "—",
            };
            println!(
                "{:<22} {:<12} {:<8} {}",
                supply.name,
                supply.kind,
                online,
                supply.status.as_deref().unwrap_or("—")
            );
        }
        println!("{:-<60}", "");

        for battery in &visible {
            let mut details = Vec::new();
            if let Some(maker) = &battery.manufacturer {
                details.push(maker.clone());
            }
            if let Some(model) = &battery.model {
                details.push(model.clone());
            }
            if let Some(tech) = &battery.technology {
                details.push(tech.clone());
            }
            if let Some(level) = &battery.capacity_level {
                details.push(level.clone());
            }
            if let (Some(now), Some(full)) = (battery.energy_now_uw(), battery.energy_full_uw()) {
                details.push(format!(
                    "{:.1} Wh / {:.1} Wh",
                    now as f64 / 1_000_000.0,
                    full as f64 / 1_000_000.0
                ));
            }
            if !details.is_empty() {
                println!("{}: {}", battery.name, details.join(" · "));
            }
        }
    }
    println!();
}

fn json_escape(input: &str) -> String {
    let mut out = String::with_capacity(input.len() + 2);
    for ch in input.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

fn json_string(value: &str) -> String {
    format!("\"{}\"", json_escape(value))
}

fn json_opt_str(value: Option<&str>) -> String {
    value.map(json_string).unwrap_or_else(|| "null".to_string())
}

fn json_opt_num<T: std::fmt::Display>(value: Option<T>) -> String {
    value
        .map(|v| v.to_string())
        .unwrap_or_else(|| "null".to_string())
}

fn json_opt_float(value: Option<f64>) -> String {
    value
        .map(|v| format!("{v:.3}"))
        .unwrap_or_else(|| "null".to_string())
}

fn render_json(batteries: &[Battery], others: &[Supply]) -> String {
    let ac = ac_state(batteries, others);
    let mut out = String::new();
    out.push_str("{\n");
    out.push_str(&format!("  \"ac_online\": {},\n", json_opt_num(ac)));
    out.push_str("  \"batteries\": [");

    for (index, battery) in batteries.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        out.push_str("\n    {");
        out.push_str(&format!("\"name\": {}, ", json_string(&battery.name)));
        out.push_str(&format!(
            "\"manufacturer\": {}, ",
            json_opt_str(battery.manufacturer.as_deref())
        ));
        out.push_str(&format!(
            "\"model\": {}, ",
            json_opt_str(battery.model.as_deref())
        ));
        out.push_str(&format!(
            "\"technology\": {}, ",
            json_opt_str(battery.technology.as_deref())
        ));
        out.push_str(&format!("\"status\": {}, ", json_string(&battery.status)));
        out.push_str(&format!(
            "\"capacity_level\": {}, ",
            json_opt_str(battery.capacity_level.as_deref())
        ));
        out.push_str(&format!("\"present\": {}, ", battery.present));
        out.push_str(&format!(
            "\"capacity\": {}, ",
            json_opt_num(battery.percent())
        ));
        out.push_str(&format!(
            "\"energy_now_wh\": {}, ",
            json_opt_float(battery.energy_now_uw().map(|v| v as f64 / 1_000_000.0))
        ));
        out.push_str(&format!(
            "\"energy_full_wh\": {}, ",
            json_opt_float(battery.energy_full_uw().map(|v| v as f64 / 1_000_000.0))
        ));
        out.push_str(&format!(
            "\"power_w\": {}, ",
            json_opt_float(battery.power_uw().map(|v| v as f64 / 1_000_000.0))
        ));
        out.push_str(&format!(
            "\"health_percent\": {}, ",
            json_opt_num(battery.health_pct())
        ));
        out.push_str(&format!(
            "\"cycle_count\": {}, ",
            json_opt_num(battery.cycle_count)
        ));
        let eta = battery.eta();
        out.push_str(&format!(
            "\"eta_seconds\": {}, ",
            json_opt_num(eta.map(|(_, s)| s))
        ));
        out.push_str(&format!(
            "\"eta_kind\": {}",
            json_opt_str(eta.map(|(kind, _)| kind))
        ));
        out.push('}');
    }

    if !batteries.is_empty() {
        out.push_str("\n  ");
    }
    out.push_str("]\n}\n");
    out
}

fn print_help() {
    println!(
        "batbar {version}
Fast battery status from Linux sysfs, with no external processes.

USAGE:
    batbar [OPTIONS]

OPTIONS:
    -a, --all          Show all power supplies and battery details
    -j, --json         Print machine-readable JSON
    -l, --live         Continuously refresh in place (1s interval)
    -w, --watch [SECS] Refresh every SECS seconds (default 1, decimals ok)
    -n, --no-color     Disable ANSI colors
    -h, --help         Show this help
    -V, --version      Show version

The bar is green above 30%, yellow at 16-30%, and red at 15% or below.
Colors follow NO_COLOR and are disabled when stdout is not a terminal.",
        version = env!("CARGO_PKG_VERSION")
    );
}

fn parse_args() -> Config {
    let args: Vec<String> = env::args().skip(1).collect();
    let mut cfg = Config {
        all: false,
        json: false,
        color: io::stdout().is_terminal() && env::var_os("NO_COLOR").is_none(),
        watch: None,
    };

    let mut index = 0;
    while index < args.len() {
        let arg = args[index].as_str();
        match arg {
            "-a" | "--all" => cfg.all = true,
            "-j" | "--json" => cfg.json = true,
            "-n" | "--no-color" => cfg.color = false,
            "-h" | "--help" => {
                print_help();
                process::exit(0);
            }
            "-V" | "--version" => {
                println!("batbar {}", env!("CARGO_PKG_VERSION"));
                process::exit(0);
            }
            "-l" | "--live" => {
                cfg.watch.get_or_insert(1.0);
            }
            "-w" | "--watch" => {
                let next = args.get(index + 1).and_then(|v| v.parse::<f64>().ok());
                if let Some(secs) = next {
                    index += 1;
                    cfg.watch = Some(clamp_interval(secs));
                } else {
                    cfg.watch = Some(1.0);
                }
            }
            other if other.starts_with("--watch=") => match other[8..].parse::<f64>() {
                Ok(secs) => cfg.watch = Some(clamp_interval(secs)),
                Err(_) => {
                    eprintln!("batbar: invalid value for --watch: '{}'", &other[8..]);
                    process::exit(2);
                }
            },
            other => {
                eprintln!("batbar: unknown argument '{other}' (try --help)");
                process::exit(2);
            }
        }
        index += 1;
    }

    cfg
}

fn clamp_interval(seconds: f64) -> f64 {
    if seconds.is_finite() {
        seconds.clamp(0.05, 3600.0)
    } else {
        1.0
    }
}

fn run_once(cfg: &Config) {
    let (batteries, others) = scan_supplies(cfg.all);
    if cfg.json {
        print!("{}", render_json(&batteries, &others));
        return;
    }
    render_text(&batteries, &others, cfg);
}

fn main() {
    let cfg = parse_args();

    match cfg.watch {
        Some(secs) if !cfg.json => {
            let interactive = io::stdout().is_terminal();
            let interval = Duration::from_secs_f64(secs);
            loop {
                if interactive {
                    print!("\x1b[H");
                }
                run_once(&cfg);
                if interactive {
                    print!("\x1b[J");
                }
                let _ = io::stdout().flush();
                thread::sleep(interval);
            }
        }
        _ => run_once(&cfg),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn durations_are_readable() {
        assert_eq!(fmt_duration(0), "0m");
        assert_eq!(fmt_duration(59), "1m");
        assert_eq!(fmt_duration(60), "1m");
        assert_eq!(fmt_duration(3600), "1h00m");
        assert_eq!(fmt_duration(3900), "1h05m");
    }

    #[test]
    fn bar_has_constant_visible_width() {
        for pct in [0u8, 1, 15, 30, 50, 99, 100] {
            let plain = make_bar(pct, false);
            assert_eq!(plain.chars().count(), BAR_WIDTH + 2);
            assert!(plain.starts_with('['));
            assert!(plain.ends_with(']'));
        }
    }

    #[test]
    fn level_thresholds_match_docs() {
        assert_eq!(level_color(15), RED);
        assert_eq!(level_color(16), YELLOW);
        assert_eq!(level_color(30), YELLOW);
        assert_eq!(level_color(31), GREEN);
    }

    #[test]
    fn json_escapes_control_characters() {
        assert_eq!(json_string("a\"b\\c\n"), "\"a\\\"b\\\\c\\n\"");
    }

    fn sample(status: &str) -> Battery {
        Battery {
            name: "BAT0".to_string(),
            manufacturer: None,
            model: None,
            technology: None,
            status: status.to_string(),
            capacity: Some(50),
            capacity_level: None,
            present: true,
            energy_now: Some(20_000_000),
            energy_full: Some(40_000_000),
            energy_full_design: Some(50_000_000),
            charge_now: None,
            charge_full: None,
            charge_full_design: None,
            voltage_now: Some(10_000_000),
            power_now: Some(10_000_000),
            current_now: None,
            cycle_count: Some(100),
        }
    }

    #[test]
    fn estimates_are_derived_from_energy() {
        let discharging = sample("Discharging");
        assert_eq!(discharging.percent(), Some(50));
        assert_eq!(discharging.health_pct(), Some(80));
        assert_eq!(discharging.eta(), Some(("empty", 7200)));

        let charging = sample("Charging");
        assert_eq!(charging.eta(), Some(("full", 7200)));

        let full = sample("Full");
        assert_eq!(full.eta(), None);
    }

    #[test]
    fn ac_state_is_inferred_from_status() {
        assert_eq!(inferred_ac(&[sample("Charging")]), Some(true));
        assert_eq!(inferred_ac(&[sample("Discharging")]), Some(false));
        assert_eq!(inferred_ac(&[sample("Full")]), Some(true));
        assert_eq!(inferred_ac(&[]), None);
    }
}
