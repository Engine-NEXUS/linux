#!/usr/bin/env bash
# scripts/register-hotkey.sh — bind Super+Space to `nexus --wake` (Linux).
# GNOME path via gsettings custom-keybinding. COSMIC has no stable CLI API
# yet, so on unknown DEs this prints the manual fallback and exits 0.
# Idempotent: re-runs overwrite the same "nexus-wake" slot, never duplicate.
# Usage: ./scripts/register-hotkey.sh [path-to-nexus-binary]
set -e
NEXUS_BIN="${1:-$HOME/.local/bin/nexus}"
[ -x "$NEXUS_BIN" ] || NEXUS_BIN="$(command -v nexus || echo "$NEXUS_BIN")"

if ! command -v gsettings &>/dev/null; then
  echo "No gsettings found — set manually: Settings > Keyboard > Custom Shortcut:"
  echo "  Command: $NEXUS_BIN --wake"
  echo "  Binding: Super+Space"
  exit 0
fi

BASE="org.gnome.settings-daemon.plugins.media-keys"
CUSTOM="$BASE.custom-keybinding"
EXISTING="$(gsettings get $BASE custom-keybindings 2>/dev/null || echo "@as []")"
SLOT="/org/gnome/settings-daemon/plugins/media-keys/custom-keybindings/nexus-wake/"

if [[ "$EXISTING" != *"nexus-wake"* ]]; then
  if [[ "$EXISTING" == "@as []" || "$EXISTING" == "[]" ]]; then
    gsettings set $BASE custom-keybindings "['$SLOT']"
  else
    gsettings set $BASE custom-keybindings "${EXISTING%]*}, '$SLOT']"
  fi
fi
gsettings set "$CUSTOM:$SLOT" name "NEXUS Wake"
gsettings set "$CUSTOM:$SLOT" command "$NEXUS_BIN --wake"
gsettings set "$CUSTOM:$SLOT" binding "<Super>space"
echo "Hotkey registered: Super+Space -> $NEXUS_BIN --wake"
echo "If it does not fire (COSMIC), set manually: Settings > Keyboard > Custom Shortcut."
