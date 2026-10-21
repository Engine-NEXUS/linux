#!/usr/bin/env bash
# ==============================================================================
# NEXUS Agent — Dev-local install & setup (Linux, macOS, Windows/WSL).
# Fast path: system deps + venv + frontend + cargo build + launcher + hotkey + setup.
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
pkill -x nexus 2>/dev/null || pkill -f "/nexus" 2>/dev/null || true
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

# 3. Check Linux dependencies & setup Python sidecar venv
VENV_DIR="${XDG_DATA_HOME:-$HOME/.local/share}/nexus/venv"
if [[ "$OSTYPE" == "linux-gnu"* ]]; then
  if command -v apt-get &> /dev/null; then
    MISSING_PKGS=()
    for pkg in libwebkit2gtk-4.1-dev libsoup-3.0-dev libjavascriptcoregtk-4.1-dev libgtk-3-dev \
               libayatana-appindicator3-dev librsvg2-dev libasound2-dev libssl-dev pkg-config \
               libgtk-layer-shell0 \
               portaudio19-dev python3-venv python3-pip python3-gi gir1.2-atspi-2.0 at-spi2-core; do
      if ! dpkg -s "$pkg" &> /dev/null; then
        MISSING_PKGS+=("$pkg")
      fi
    done
    if [ ${#MISSING_PKGS[@]} -gt 0 ]; then
      echo -e "${YELLOW}==> Installing required system packages: ${MISSING_PKGS[*]}${NC}"
      sudo apt update && sudo apt install -y "${MISSING_PKGS[@]}"
    fi

    # Optional X11 helpers and wl-clipboard
    OPTIONAL_PKGS=()
    for pkg in xdotool wmctrl xclip wl-clipboard; do
      if ! dpkg -s "$pkg" &> /dev/null; then
        OPTIONAL_PKGS+=("$pkg")
      fi
    done
    if [ ${#OPTIONAL_PKGS[@]} -gt 0 ]; then
      echo -e "${YELLOW}==> Installing optional clipboard and X11 helper tools (only used on X11 / for clipboard): ${OPTIONAL_PKGS[*]}${NC}"
      sudo apt install -y "${OPTIONAL_PKGS[@]}" || true
    fi
  fi

  # Setup Python venv under user's data dir (never system pip or sudo pip)
  DATA_DIR="${XDG_DATA_HOME:-$HOME/.local/share}/nexus"
  mkdir -p "$DATA_DIR"
  if [ ! -d "$VENV_DIR" ]; then
    echo -e "${CYAN}==> Creating Python virtual environment in $VENV_DIR...${NC}"
    python3 -m venv --system-site-packages "$VENV_DIR"
  fi
  VENV_PIP="$VENV_DIR/bin/pip"
  if [ -f "server/requirements-linux.txt" ] && [ -x "$VENV_PIP" ]; then
    echo -e "${CYAN}==> Installing Python sidecar dependencies into venv...${NC}"
    "$VENV_PIP" install --upgrade pip --quiet
    "$VENV_PIP" install -r server/requirements-linux.txt
  fi
fi

# 4. Build frontend
echo -e "${CYAN}==> [3/5] Building UI assets...${NC}"
npm --prefix frontend install
if [ -f "scripts/sync-hand-assets.mjs" ]; then
  node scripts/sync-hand-assets.mjs || true
fi
npm --prefix frontend run build

# 5. Build release binary
echo -e "${CYAN}==> [4/5] Compiling NEXUS release binary...${NC}"
cd src-tauri
cargo build --release --features custom-protocol
cd ..

# 6. Create Desktop Launcher & CLI Symlink
echo -e "${CYAN}==> [5/5] Setting up launcher and commands...${NC}"
mkdir -p ~/.local/bin ~/.local/share/applications
TARGET_BIN="$SCRIPT_DIR/src-tauri/target/release/nexus"
ln -sf "$TARGET_BIN" ~/.local/bin/nexus

if [[ "$OSTYPE" == "linux-gnu"* ]]; then
  # Desktop entry: the binary picks the GTK backend itself (it forces
  # GDK_BACKEND=x11 only when an XWayland display is actually available),
  # so nothing is forced here — pointing GTK at a missing X server would
  # stop the app from starting.
  cat << DESKTOP_EOF > ~/.local/share/applications/nexus.desktop
[Desktop Entry]
Name=NEXUS
Comment=Floating Desktop AI Assistant
Exec=env NEXUS_PYTHON=$VENV_DIR/bin/python3 PATH=$VENV_DIR/bin:\$PATH $TARGET_BIN
Icon=$SCRIPT_DIR/src-tauri/icons/128x128.png
Terminal=false
Type=Application
Categories=Utility;AudioVideo;Development;
StartupNotify=true
DESKTOP_EOF
  chmod +x ~/.local/share/applications/nexus.desktop

  # Rewrite stale nexus-handler.desktop if it exists and points to a missing binary
  HANDLER_DESKTOP="$HOME/.local/share/applications/nexus-handler.desktop"
  if [ -f "$HANDLER_DESKTOP" ]; then
    CURR_EXEC="$(grep '^Exec=' "$HANDLER_DESKTOP" | cut -d'=' -f2- | tr -d '"' | awk '{print $1}')"
    if [ ! -f "$CURR_EXEC" ] || [ "$CURR_EXEC" != "$TARGET_BIN" ]; then
      echo -e "${YELLOW}==> Updating stale deep-link handler ($HANDLER_DESKTOP)...${NC}"
      cat << HANDLER_EOF > "$HANDLER_DESKTOP"
[Desktop Entry]
Type=Application
Name=NEXUS
Terminal=false
NoDisplay=true
MimeType=x-scheme-handler/nexus
Exec=env NEXUS_PYTHON=$VENV_DIR/bin/python3 "$TARGET_BIN" %u
HANDLER_EOF
    fi
  fi

  # Rewrite stale autostart NEXUS.desktop if it exists and points to a missing binary
  AUTOSTART_DESKTOP="$HOME/.config/autostart/NEXUS.desktop"
  if [ -f "$AUTOSTART_DESKTOP" ]; then
    CURR_EXEC="$(grep '^Exec=' "$AUTOSTART_DESKTOP" | cut -d'=' -f2- | tr -d '"' | awk '{print $1}')"
    if [ ! -f "$CURR_EXEC" ] || [ "$CURR_EXEC" != "$TARGET_BIN" ]; then
      echo -e "${YELLOW}==> Updating stale autostart entry ($AUTOSTART_DESKTOP)...${NC}"
      cat << AUTOSTART_EOF > "$AUTOSTART_DESKTOP"
[Desktop Entry]
Type=Application
Version=1.0
Name=NEXUS
Comment=NEXUS startup script
Exec=env NEXUS_PYTHON=$VENV_DIR/bin/python3 "$TARGET_BIN"
StartupNotify=false
Terminal=false
AUTOSTART_EOF
    fi
  fi

  update-desktop-database ~/.local/share/applications 2>/dev/null || true

  # Register global summon hotkey
  if [ -x "$SCRIPT_DIR/scripts/register-hotkey.sh" ]; then
    "$SCRIPT_DIR/scripts/register-hotkey.sh" "$TARGET_BIN" || true
  fi
fi

echo ""
echo -e "${GREEN}═════════════════════════════════════════════════════════════${NC}"
echo -e "${GREEN}✓ NEXUS is installed and ready!${NC}"
echo -e "• Binary: ~/.local/bin/nexus"
if [[ "$OSTYPE" == "linux-gnu"* ]]; then
  echo -e "• Global Hotkey: Ctrl+Super+Space (nexus --wake)"
else
  echo -e "• Global Hotkey: Ctrl+Shift+Space"
fi
echo -e "• Wake Word: \"NEXUS\""
echo -e "${GREEN}═════════════════════════════════════════════════════════════${NC}"
echo ""
echo -e "${CYAN}Launching NEXUS Setup Wizard...${NC}"
nohup "$TARGET_BIN" --setup > /dev/null 2>&1 &
echo -e "${GREEN}Done! The Setup Wizard is now open on your screen.${NC}"
