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
if command -v cargo-tauri &>/dev/null; then
  TAURI_RUN=(cargo tauri build)
elif npm --prefix frontend exec tauri --version &>/dev/null; then
  TAURI_RUN=(npm --prefix frontend exec tauri build)
else
  echo "==> Installing tauri-cli (one-time, may take a few minutes)"
  cargo install tauri-cli --version "^2" --locked
  TAURI_RUN=(cargo tauri build)
fi
(cd src-tauri && "${TAURI_RUN[@]}" --bundles deb)
cd ..
echo "==> Artifacts in src-tauri/target/release/bundle/deb/"
ls src-tauri/target/release/bundle/deb/*.deb
