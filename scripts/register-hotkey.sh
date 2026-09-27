#!/usr/bin/env bash
# scripts/register-hotkey.sh — bind Ctrl+Super+Space to `nexus --wake` (Linux).
# GNOME path via gsettings custom-keybinding. COSMIC has no stable CLI API
# yet, so on COSMIC this prints the manual fallback and exits 0.
# Idempotent: re-runs overwrite the same "nexus-wake" slot, never duplicate.
# Binding: Ctrl+Super+Space — Super+Space is owned by the launcher /
# switch-input-source on COSMIC and GNOME, so we avoid it by default.
# Usage: ./scripts/register-hotkey.sh [path-to-nexus-binary] [binding]
set -e
NEXUS_BIN="${1:-$HOME/.local/bin/nexus}"
BINDING="${2:-<Primary><Super>space}"
[ -x "$NEXUS_BIN" ] || NEXUS_BIN="$(command -v nexus || echo "$NEXUS_BIN")"

if [[ "${XDG_CURRENT_DESKTOP:-}" == *"COSMIC"* ]]; then
  echo "COSMIC detected — no CLI API for custom shortcuts."
  echo "Set manually: Settings > Keyboard > Shortcuts > Custom:"
  echo "  Name:    NEXUS Wake"
  echo "  Command: $NEXUS_BIN --wake"
  echo "  Binding: Ctrl+Super+Space"
  exit 0
fi

if ! command -v gsettings &>/dev/null; then
  echo "No gsettings found — set manually: Settings > Keyboard > Custom Shortcut:"
  echo "  Command: $NEXUS_BIN --wake"
  echo "  Binding: Ctrl+Super+Space"
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
gsettings set "$CUSTOM:$SLOT" binding "$BINDING"
echo "Hotkey registered: Ctrl+Super+Space -> $NEXUS_BIN --wake"
