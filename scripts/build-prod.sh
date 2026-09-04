#!/usr/bin/env bash
# scripts/build-prod.sh — production .deb build (Linux).
# Usage: ./scripts/build-prod.sh
# Frontend build + `cargo tauri build --bundles deb` (custom-protocol ON
# automatically — never use plain `cargo build` for prod, the packaged app
# would embed the Vite dev URL and show ERR_CONNECTION_REFUSED).
set -e
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(dirname "$SCRIPT_DIR")"
cd "$ROOT_DIR"

echo "==> Building frontend"
npm --prefix frontend install
npm --prefix frontend run build

echo "==> Building .deb (tauri build)"
TAURI_BIN="$ROOT_DIR/frontend/node_modules/.bin/tauri"
if command -v cargo-tauri &>/dev/null; then
  (cd src-tauri && cargo tauri build --bundles deb)
elif [ -x "$TAURI_BIN" ] && "$TAURI_BIN" --version &>/dev/null; then
  # Same cwd caveat as dev.sh — invoke the binary directly from src-tauri.
  (cd src-tauri && "$TAURI_BIN" build --bundles deb)
else
  echo "==> Installing tauri-cli (one-time, may take a few minutes)"
  cargo install tauri-cli --version "^2" --locked
  (cd src-tauri && cargo tauri build --bundles deb)
fi
cd ..
echo "==> Artifacts in src-tauri/target/release/bundle/deb/"
ls src-tauri/target/release/bundle/deb/*.deb
