# 🔋 batbar

> A blazing-fast, zero-dependency battery status visualizer for Linux — reads `sysfs` directly, no `upower`, no `acpi`, no subprocesses.

[![License](https://img.shields.io/badge/license-MIT%2FApache--2.0-blue.svg)](#-license)
[![Rust](https://img.shields.io/badge/rust-1.85%2B-blue.svg)](https://www.rust-lang.org)
[![Platform](https://img.shields.io/badge/platform-linux-lightgrey.svg)](#-compatibility)
[![Dependencies](https://img.shields.io/badge/dependencies-0-brightgreen.svg)](Cargo.toml)
[![Speed](https://img.shields.io/badge/speed-~1.2ms-brightgreen.svg)](#-benchmarks)

`batbar` is a lightweight alternative to GUI battery applets and shell-heavy status scripts. It enumerates `/sys/class/power_supply`, reads the kernel's battery attributes, and renders a clean, color-coded table with a charge bar, time estimate, battery health, and cycle count.

```
🔋 BATTERY STATUS (Direct sysfs)
----------------------------------------------------------------------------
BATTERY STATE        CHARGE                  LVL  ETA          HEALTH CYCLES
----------------------------------------------------------------------------
BAT0    Charging     [███████████░░░░░░░░░]  57%  —              100%    203
----------------------------------------------------------------------------
AC: connected
```

---

## Table of Contents

- [Features](#-features)
- [Install](#-install)
- [Usage](#-usage)
- [JSON output](#-json-output)
- [Live mode](#-live-mode)
- [Benchmarks](#-benchmarks)
- [How it works](#-how-it-works)
- [Why is `--all` slower?](#-why-is---all-slower)
- [Compatibility](#-compatibility)
- [Troubleshooting](#-troubleshooting)
- [Development](#-development)
- [License](#-license)

---

## ✨ Features

- ⚡ **Fast by default** — ~1.2 ms warm run, ~2.2 MB peak RSS, 344 KB binary.
- 📊 **Visual charge bar** — green above 30%, yellow at 16–30%, red at 15% or below.
- ⏱️ **Time estimate** — time to full or empty when the kernel exposes `power_now` or `current_now`.
- 🩺 **Health & cycles** — full-charge capacity vs. design capacity, plus cycle count.
- 🧩 **Multiple batteries** — every battery under `/sys/class/power_supply` is included.
- 🔌 **AC detection** — inferred from battery status, with exact ACPI state under `--all`.
- 🤖 **JSON output** — stable, hand-rolled, no serde dependency.
- 📺 **Live mode** — `--live` redraws in place; `--watch` accepts fractional intervals.
- 🪶 **Zero dependencies** — pure Rust standard library.
- 🎨 **Respects `NO_COLOR`** — colors also auto-disable when stdout is not a TTY.
- 📦 **Tiny** — `opt-level = "z"`, LTO, stripped, `panic = "abort"`.

---

## 🚀 Install

### Build from source

```bash
git clone https://github.com/parameters-tools/batbar.git
cd batbar
cargo build --release
sudo install -m 755 target/release/batbar /usr/local/bin/batbar
```

### Via Cargo

```bash
cargo install --git https://github.com/parameters-tools/batbar
```

### Installer script

The installer requires a Rust toolchain:

```bash
curl -sSL https://raw.githubusercontent.com/parameters-tools/batbar/main/install.sh | bash
```

---

## 📖 Usage

```bash
batbar                 # batteries only
batbar --all           # include adapters, online state, and battery details
batbar --json          # JSON output
batbar --live          # continuous in-place refresh (1s)
batbar --watch 5       # refresh every 5 seconds
batbar --watch 0.25    # sub-second refresh
batbar --no-color      # plain text
```

### Options

| Flag | Description |
| :--- | :--- |
| `-a`, `--all` | Show every power supply and detailed battery info |
| `-j`, `--json` | Machine-readable JSON |
| `-l`, `--live` | Continuous in-place refresh (1s interval) |
| `-w`, `--watch [SECS]` | Refresh interval in seconds (default 1, decimals allowed) |
| `-n`, `--no-color` | Disable ANSI colors |
| `-h`, `--help` | Show help |
| `-V`, `--version` | Show version |

Example `--all` output:

```
🔋 BATTERY STATUS (Direct sysfs)
----------------------------------------------------------------------------
BATTERY STATE        CHARGE                  LVL  ETA          HEALTH CYCLES
----------------------------------------------------------------------------
BAT0    Charging     [██████████░░░░░░░░░░]  51%  —              100%    203
----------------------------------------------------------------------------
AC: connected

POWER SUPPLIES
------------------------------------------------------------
NAME                   TYPE         ONLINE   STATUS
------------------------------------------------------------
ACAD                   Mains        yes      —
ucsi-source-psy-USBC000:001 USB          no       Not charging
------------------------------------------------------------
BAT0: HP · Primary · Li-ion · Normal · 18.2 Wh / 35.7 Wh
```

---

## 🤖 JSON output

```bash
batbar --json
```

```json
{
  "ac_online": true,
  "batteries": [
    {
      "name": "BAT0",
      "manufacturer": "HP",
      "model": "Primary",
      "technology": "Li-ion",
      "status": "Charging",
      "capacity_level": "Normal",
      "present": true,
      "capacity": 57,
      "energy_now_wh": 20.301,
      "energy_full_wh": 35.651,
      "power_w": null,
      "health_percent": 100,
      "cycle_count": 203,
      "eta_seconds": null,
      "eta_kind": null
    }
  ]
}
```

`power_w`, `eta_seconds`, and `eta_kind` are `null` when the driver does not expose power/current. Field names are stable; new fields may be added.

---

## 📺 Live mode

`batbar --live` keeps the current frame on screen and redraws in place instead of clearing the terminal:

```bash
batbar --live          # 1 second
batbar --watch 0.5     # twice per second
batbar --watch 10      # every 10 seconds
```

Press `Ctrl-C` to stop. When stdout is not a terminal (for example, piped to a file), `--watch` prints plain frames rather than ANSI escapes.

Battery percentage itself changes slowly; the bar, ETA, and power figures are the values that move between ticks.

---

## ⚡ Benchmarks

Measured on an HP laptop running Linux over 30–100 warm runs. Your numbers will vary with storage and ACPI firmware.

| Command | Average runtime | Peak RSS | Notes |
| :--- | ---: | ---: | :--- |
| **`batbar`** | **1.16 ms** | **2.2 MB** | Default fast path |
| `batbar --json` | 1.22 ms | 2.2 MB | Same data, JSON |
| `upower -d` | 12.39 ms | — | D-Bus round-trip |
| `batbar --all` | 53.00 ms | 2.2 MB | Queries ACPI adapter `online` |

The `--all` cost is dominated by a single ACPI embedded-controller transaction (~51 ms on this machine), not by `batbar` itself. Battery attributes are cached by the kernel and read in microseconds.

---

## 🛠️ How it works

1. Enumerate `/sys/class/power_supply/*` and keep entries whose `type` is `Battery`.
2. Read `status`, `capacity`, `capacity_level`, `energy_now`, `energy_full`, `energy_full_design`, `charge_now`, `charge_full`, `voltage_now`, `power_now`, `current_now`, and `cycle_count` when present.
3. If a driver reports charge in µAh instead of energy in µWh, derive watt-hours as `charge × voltage`.
4. Normalize to percent, health, power draw, and ETA, then render an aligned table.
5. Treat the values as best-effort: missing files are skipped, not treated as errors.

No `upower`, no `acpi`, no shelling out, no D-Bus.

---

## 🔌 Why is `--all` slower?

Reading an ACPI adapter's `online` attribute can trigger an embedded-controller transaction. On the test laptop, `/sys/class/power_supply/ACAD/online` takes **~51 ms per read**, while every battery attribute takes **~0.02 ms**.

`batbar` therefore infers AC state from the battery `status` for normal runs:

| Battery status | Inferred AC |
| :--- | :--- |
| `Charging`, `Full`, `Not charging` | connected |
| `Discharging` | disconnected |
| anything else | unknown |

`--all` performs the exact ACPI read and also shows every adapter. If you need millisecond-accurate AC state in a loop, prefer the default mode.

---

## 🧭 Compatibility

- **OS:** Linux only (uses `/sys/class/power_supply`).
- **Rust:** 1.85+ (edition 2024).
- **Drivers:** works with the mainline `power_supply` class — ACPI batteries, `axp20x`, `bq27xxx`, `cw2015`, `max170xx`, and others.
- **Desktops/servers without a battery:** exits cleanly with a message; `--all` still lists power supplies.

---

## 🩺 Troubleshooting

| Symptom | Cause / fix |
| :--- | :--- |
| `ETA` shows `—` | The driver does not expose `power_now` or `current_now`. |
| `HEALTH` shows `—` | No `energy_full_design` / `charge_full_design` attribute. |
| `HEALTH` above 100% | Normal on new batteries; capacity can exceed the design value. |
| No output / "No battery found" | Desktop or VM; use `--all` to inspect available supplies. |
| `--all` feels slow | Expected: it performs an ACPI transaction. Use the default mode. |

---

## 🧑‍💻 Development

```bash
cargo fmt --check
cargo clippy --release -- -D warnings
cargo test --release
cargo build --release
```

The project deliberately has **no runtime or dev dependencies**. Tests cover duration formatting, bar geometry, color thresholds, JSON escaping, ETA/health math, and AC inference.

---

## 📄 License

Dual-licensed under either the [MIT License](LICENSE-MIT) or the [Apache License, Version 2.0](LICENSE-APACHE), at your option.

---

<sub>Built with Rust. Not affiliated with any hardware vendor.</sub>
