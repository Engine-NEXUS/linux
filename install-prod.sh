#!/usr/bin/env bash
# install-prod.sh — one-line production install (Linux).
# Usage: curl -fsSL https://raw.githubusercontent.com/Engine-NEXUS/linux/main/install-prod.sh | bash
# Fetches latest release .deb from the linux remote and installs it.
set -e
REPO="Engine-NEXUS/linux"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

echo "==> Fetching latest NEXUS release"
TAG="$(curl -fsSL "https://api.github.com/repos/$REPO/releases/latest" | grep -m1 '"tag_name"' | cut -d'"' -f4)"
[ -n "$TAG" ] || { echo "Error: no releases found for $REPO."; exit 1; }
echo "==> Latest: $TAG"
DEB_URL="$(curl -fsSL "https://api.github.com/repos/$REPO/releases/tags/$TAG" | grep -m1 'browser_download_url.*\.deb"' | cut -d'"' -f4)"
[ -n "$DEB_URL" ] || { echo "Error: no .deb asset on $TAG."; exit 1; }
curl -fsSL -o "$TMP/nexus.deb" "$DEB_URL"

echo "==> Installing (needs sudo for apt)"
sudo apt install -y "$TMP/nexus.deb"
update-desktop-database ~/.local/share/applications 2>/dev/null || true

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd 2>/dev/null || echo .)"
if [ -x "$SCRIPT_DIR/scripts/register-hotkey.sh" ]; then
  "$SCRIPT_DIR/scripts/register-hotkey.sh" "$(command -v nexus)" || true
else
  echo "Hotkey: Settings > Keyboard > Custom Shortcut: nexus --wake on Super+Space"
fi

echo "Done. Run: nexus --setup"
