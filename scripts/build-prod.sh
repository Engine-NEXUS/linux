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
cd src-tauri
if ! command -v cargo-tauri &>/dev/null && ! npm --prefix ../frontend exec tauri --version &>/dev/null; then
  echo "==> Installing tauri-cli"
  cargo install tauri-cli --version "^2" --locked
fi
cargo tauri build --bundles deb
cd ..
echo "==> Artifacts in src-tauri/target/release/bundle/deb/"
ls src-tauri/target/release/bundle/deb/*.deb
