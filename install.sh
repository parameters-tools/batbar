#!/usr/bin/env bash
set -e

BOLD="\033[1m"
GREEN="\033[32m"
BLUE="\033[34m"
RED="\033[31m"
RESET="\033[0m"

echo -e "${BLUE}${BOLD}🔋 batbar installer${RESET}"
echo "-------------------------------------"

if command -v cargo >/dev/null 2>&1; then
    echo -e "🚀 Rust toolchain detected. Compiling and installing ${BOLD}batbar${RESET}..."
    cargo install --git https://github.com/parameters-tools/batbar

    echo -e "\n${GREEN}✅ batbar successfully installed!${RESET}"
    echo -e "Binary placed at: ${BOLD}$(command -v batbar || echo "$HOME/.cargo/bin/batbar")${RESET}"
    echo -e "Run ${BOLD}batbar${RESET} to check your battery!"
    exit 0
fi

echo -e "${RED}❌ Rust/Cargo not found.${RESET}"
echo "Please install Rust using: curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh"
echo "Then re-run this script."
exit 1
