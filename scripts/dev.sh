#!/usr/bin/env bash
# scripts/dev.sh — one-line dev loop (Linux).
# Usage: ./scripts/dev.sh
# Checks deps, installs frontend modules if missing, execs `cargo tauri dev`
# (custom-protocol OFF so Vite HMR on :5173 keeps working).
set -e
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(dirname "$SCRIPT_DIR")"
cd "$ROOT_DIR"

MISSING=()
for pkg in libwebkit2gtk-4.1-dev libgtk-3-dev libayatana-appindicator3-dev librsvg2-dev libasound2-dev libssl-dev pkg-config; do
  dpkg -s "$pkg" &>/dev/null || MISSING+=("$pkg")
done
if [ ${#MISSING[@]} -gt 0 ]; then
  echo "==> Installing missing system packages: ${MISSING[*]}"
  sudo apt update && sudo apt install -y "${MISSING[@]}"
fi
command -v node &>/dev/null || { echo "Error: Node.js 18+ required."; exit 1; }
command -v cargo &>/dev/null || { echo "Error: Rust/cargo required (https://rustup.rs)."; exit 1; }
[ -d frontend/node_modules ] || { echo "==> npm install"; npm --prefix frontend install; }

echo "==> tauri dev (HMR on http://localhost:5173)"
cd src-tauri
exec cargo tauri dev
