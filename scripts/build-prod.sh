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

echo "==> Installing frontend deps"
npm --prefix frontend install
if [ -f scripts/sync-hand-assets.mjs ]; then
  node scripts/sync-hand-assets.mjs || true
fi

echo "==> Syncing sidecars to resources"
for f in atspi_server.py cdp_server.py nlu_server.py ocr_server.py stt_server.py; do
  [ -f "server/$f" ] && cp -f "server/$f" "src-tauri/resources/server/$f" || true
done

echo "==> Building frontend + .deb (tauri build)"
# Run from the repo root: tauri.conf.json's beforeBuildCommand (`npm run build`)
# resolves against the root package.json shim, which src-tauri/ lacks.
TAURI_BIN="$ROOT_DIR/frontend/node_modules/.bin/tauri"
if command -v cargo-tauri &>/dev/null; then
  cargo tauri build --bundles deb
elif [ -x "$TAURI_BIN" ] && "$TAURI_BIN" --version &>/dev/null; then
  "$TAURI_BIN" build --bundles deb
else
  echo "==> Installing tauri-cli (one-time, may take a few minutes)"
  cargo install tauri-cli --version "^2" --locked
  cargo tauri build --bundles deb
fi
echo "==> Artifacts in src-tauri/target/release/bundle/deb/"
ls "$ROOT_DIR/src-tauri/target/release/bundle/deb/"*.deb
