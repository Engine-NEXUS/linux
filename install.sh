#!/usr/bin/env bash
# ==============================================================================
# NEXUS Agent — Dev-local install (Linux).
# Fast path: frontend build + cargo build + symlink + .desktop + hotkey + setup.
# For release builds see scripts/build-prod.sh; for end users install-prod.sh.
# ==============================================================================
set -e

CYAN='\033[0;36m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
RED='\033[0;31m'
NC='\033[0m'

echo -e "${CYAN}╔═══════════════════════════════════════════════════════════╗${NC}"
echo -e "${CYAN}║              NEXUS Assistant — Installer                  ║${NC}"
echo -e "${CYAN}╚═══════════════════════════════════════════════════════════╝${NC}"
echo ""

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

# 1. Stop any currently running instance
echo -e "${CYAN}==> [1/5] Stopping previous NEXUS instances...${NC}"
pkill -f "/nexus" || true
sleep 1

# 2. Check tools
echo -e "${CYAN}==> [2/5] Checking build environment...${NC}"
if ! command -v node &> /dev/null; then
  echo -e "${RED}Error: Node.js is not installed. Please install Node.js (v18+).${NC}"
  exit 1
fi
if ! command -v cargo &> /dev/null; then
  echo -e "${RED}Error: Rust / Cargo is not installed. Please install Rust via https://rustup.rs${NC}"
  exit 1
fi

# 3. Check Linux dependencies
if [[ "$OSTYPE" == "linux-gnu"* ]]; then
  if command -v apt-get &> /dev/null; then
    MISSING_PKGS=()
    for pkg in libwebkit2gtk-4.1-dev libsoup-3.0-dev libjavascriptcoregtk-4.1-dev libgtk-3-dev libasound2-dev libssl-dev; do
      if ! dpkg -s "$pkg" &> /dev/null; then
        MISSING_PKGS+=("$pkg")
      fi
    done
    if [ ${#MISSING_PKGS[@]} -gt 0 ]; then
      echo -e "${YELLOW}==> Installing missing system packages: ${MISSING_PKGS[*]}${NC}"
      sudo apt update && sudo apt install -y "${MISSING_PKGS[@]}"
    fi
  fi
fi

# 4. Build frontend
echo -e "${CYAN}==> [3/5] Building UI assets...${NC}"
npm --prefix frontend install
npm --prefix frontend run build

# 5. Build release binary
echo -e "${CYAN}==> [4/5] Compiling NEXUS release binary...${NC}"
cd src-tauri
cargo build --release --features custom-protocol
cd ..

# 6. Create Desktop Launcher & CLI Symlink
echo -e "${CYAN}==> [5/5] Setting up launcher and commands...${NC}"
mkdir -p ~/.local/bin ~/.local/share/applications
ln -sf "$SCRIPT_DIR/src-tauri/target/release/nexus" ~/.local/bin/nexus

if [[ "$OSTYPE" == "linux-gnu"* ]]; then
  cat << DESKTOP_EOF > ~/.local/share/applications/nexus.desktop
[Desktop Entry]
Name=NEXUS
Comment=Floating Desktop AI Assistant
Exec=$SCRIPT_DIR/src-tauri/target/release/nexus
Icon=$SCRIPT_DIR/src-tauri/icons/128x128.png
Terminal=false
Type=Application
Categories=Utility;AudioVideo;Development;
StartupNotify=true
DESKTOP_EOF
  chmod +x ~/.local/share/applications/nexus.desktop
fi

echo ""
echo -e "${GREEN}═════════════════════════════════════════════════════════════${NC}"
echo -e "${GREEN}✓ NEXUS is installed and ready!${NC}"
"$SCRIPT_DIR/scripts/register-hotkey.sh" "$SCRIPT_DIR/src-tauri/target/release/nexus" || true

echo -e "• Binary: ~/.local/bin/nexus"
echo -e "• Global Hotkey: Super+Space (DE keybind → nexus --wake)"
echo -e "• Wake Word: \"NEXUS\""
echo -e "${GREEN}═════════════════════════════════════════════════════════════${NC}"
echo ""
echo -e "${CYAN}Launching NEXUS Setup Wizard...${NC}"
nohup "$SCRIPT_DIR/src-tauri/target/release/nexus" --setup > /dev/null 2>&1 &
echo -e "${GREEN}Done! The Setup Wizard is now open on your screen.${NC}"
