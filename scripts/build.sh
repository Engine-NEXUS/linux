#!/usr/bin/env bash
# scripts/build.sh — Unix (Linux & macOS) build helper.
# Usage:
#   ./scripts/build.sh              # build frontend + tauri (release)
#   ./scripts/build.sh --target aarch64-apple-darwin
#   ./scripts/build.sh --fast        # cargo build --release --features custom-protocol

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(dirname "$SCRIPT_DIR")"
cd "$ROOT_DIR"

FAST_BUILD=false
TARGET=""
BUNDLES=""

while [[ $# -gt 0 ]]; do
  case "$1" in
    --fast)
      FAST_BUILD=true
      shift
      ;;
    --target)
      TARGET="$2"
      shift 2
      ;;
    --bundles)
      BUNDLES="$2"
      shift 2
      ;;
    *)
      echo "Unknown option: $1"
      exit 1
      ;;
  esac
done

echo -e "\033[0;36m==> Installing frontend dependencies\033[0m"
npm --prefix frontend install

echo -e "\033[0;36m==> Building frontend\033[0m"
npm --prefix frontend run build

if [ "$FAST_BUILD" = true ]; then
  echo -e "\033[0;36m==> Compiling release binary (cargo build)\033[0m"
  cd src-tauri
  cargo build --release --features custom-protocol
  cd ..
  echo -e "\033[0;32m==> Binary built: src-tauri/target/release/nexus\033[0m"
else
  echo -e "\033[0;36m==> Building Tauri app (tauri build)\033[0m"
  cd src-tauri
  CARGO_ARGS=("tauri" "build")
  if [ -n "$TARGET" ]; then
    CARGO_ARGS+=("--target" "$TARGET")
  fi
  if [ -n "$BUNDLES" ]; then
    CARGO_ARGS+=("--bundles" "$BUNDLES")
  fi
  
  TAURI_BIN="$ROOT_DIR/frontend/node_modules/.bin/tauri"
  if command -v cargo-tauri &> /dev/null; then
    cargo "${CARGO_ARGS[@]}"
  elif [ -x "$TAURI_BIN" ] && "$TAURI_BIN" --version &> /dev/null; then
    "$TAURI_BIN" "${CARGO_ARGS[@]:1}"
  else
    echo -e "\033[0;33m==> Installing tauri-cli (one-time, may take a few minutes)\033[0m"
    cargo install tauri-cli --version "^2" --locked
    cargo "${CARGO_ARGS[@]}"
  fi
  cd ..
  echo -e "\033[0;32m==> Artifacts in src-tauri/target/release/bundle/\033[0m"
fi
