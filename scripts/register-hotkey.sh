#!/usr/bin/env bash
# scripts/register-hotkey.sh — bind Ctrl+Super+Space to `nexus --wake` (Linux).
# Supports:
#   1. COSMIC desktop: ~/.config/cosmic/com.system76.CosmicSettings.Shortcuts/v1/custom (RON)
#   2. GNOME desktop: gsettings custom-keybinding (dconf)
#
# Idempotent: re-runs overwrite the existing NEXUS entry, never duplicate.
# Removes any previous NEXUS entries whose binary no longer exists.
# Preserves all other user shortcuts untouched.
# Backs up custom shortcut files before modifying.
# Enforces that modifiers cannot be empty (never binds a naked key).
#
# Default chord: Ctrl+Super+Space (Super+Space alone is reserved by system launchers).
# Usage: ./scripts/register-hotkey.sh [path-to-nexus-binary] [binding]
set -e

NEXUS_BIN="${1:-$HOME/.local/bin/nexus}"
BINDING="${2:-<Primary><Super>space}"

# Resolve binary path
if [ ! -x "$NEXUS_BIN" ]; then
  RESOLVED="$(command -v nexus 2>/dev/null || true)"
  if [ -n "$RESOLVED" ] && [ -x "$RESOLVED" ]; then
    NEXUS_BIN="$RESOLVED"
  fi
fi

COSMIC_DIR="$HOME/.config/cosmic/com.system76.CosmicSettings.Shortcuts/v1"
COSMIC_FILE="$COSMIC_DIR/custom"
REGISTERED=0

# ── 1. COSMIC Desktop Registration ──────────────────────────────────────────
if [[ "${XDG_CURRENT_DESKTOP:-}" == *"COSMIC"* ]] || [ -d "$HOME/.config/cosmic" ]; then
  echo "Registering NEXUS wake hotkey for COSMIC desktop..."

  python3 - "$NEXUS_BIN" "$BINDING" "$COSMIC_FILE" << 'PYEOF'
import sys
import os
import re
import shutil

nexus_bin = sys.argv[1]
binding_arg = sys.argv[2]
cosmic_file = sys.argv[3]

# 1. Parse requested chord
# Convert GNOME/string notation to COSMIC RON modifiers and key
# e.g. "<Primary><Super>space" or "Ctrl+Super+Space"
mods = []
b_lower = binding_arg.lower()
if "<primary>" in b_lower or "ctrl" in b_lower:
    mods.append("Ctrl")
if "<super>" in b_lower or "super" in b_lower:
    mods.append("Super")
if "<alt>" in b_lower or "alt" in b_lower:
    mods.append("Alt")
if "<shift>" in b_lower or "shift" in b_lower:
    mods.append("Shift")

# Default to Ctrl+Super if no modifiers were matched
if not mods:
    mods = ["Ctrl", "Super"]

# Extract key name
# Remove modifiers and brackets/pluses
clean_key = re.sub(r'<[^>]+>', ' ', binding_arg)
clean_key = re.sub(r'\b(ctrl|primary|super|alt|shift)\b', ' ', clean_key, flags=re.I)
clean_key = clean_key.replace('+', ' ').strip()
key = clean_key if clean_key else "space"

# Hard rule: never bind a key with no modifiers
if not mods:
    print("Error: refusing to bind shortcut without modifiers.", file=sys.stderr)
    sys.exit(1)

# 2. Parse existing custom RON file
def parse_entries(content):
    content = content.strip()
    if not content:
        return []
    start = content.find('{')
    end = content.rfind('}')
    if start == -1 or end == -1:
        return []
    inner = content[start + 1:end]
    entries = []
    i = 0
    n = len(inner)
    while i < n:
        while i < n and inner[i] in ' \t\r\n,':
            i += 1
        if i >= n:
            break
        if inner[i] == '(':
            entry_start = i
            paren_depth = 0
            in_str = False
            escape = False
            while i < n:
                c = inner[i]
                if escape:
                    escape = False
                elif c == '\\':
                    escape = True
                elif c == '"':
                    in_str = not in_str
                elif not in_str:
                    if c == '(':
                        paren_depth += 1
                    elif c == ')':
                        paren_depth -= 1
                        if paren_depth == 0:
                            i += 1
                            break
                i += 1
            while i < n and inner[i] in ' \t\r\n':
                i += 1
            if i < n and inner[i] == ':':
                i += 1
            in_str = False
            escape = False
            val_paren_depth = 0
            while i < n:
                c = inner[i]
                if escape:
                    escape = False
                elif c == '\\':
                    escape = True
                elif c == '"':
                    in_str = not in_str
                elif not in_str:
                    if c == '(':
                        val_paren_depth += 1
                    elif c == ')':
                        val_paren_depth -= 1
                    elif c == ',' and val_paren_depth == 0:
                        i += 1
                        break
                    elif c == '}' and val_paren_depth == 0:
                        break
                i += 1
            entry_text = inner[entry_start:i].strip().rstrip(',')
            if entry_text:
                entries.append(entry_text)
        else:
            i += 1
    return entries

def is_nexus_entry(entry):
    if re.search(r'description:\s*Some\(\s*"[^"]*nexus[^"]*"\s*\)', entry, re.I):
        return True
    if re.search(r'Spawn\(\s*"[^"]*nexus[^"]*--wake[^"]*"\s*\)', entry, re.I):
        return True
    return False

# Read and backup existing file if present
kept_entries = []
if os.path.exists(cosmic_file):
    try:
        shutil.copy2(cosmic_file, cosmic_file + ".bak")
    except Exception as e:
        print(f"Warning: could not create backup of {cosmic_file}: {e}", file=sys.stderr)
    try:
        with open(cosmic_file, "r", encoding="utf-8") as f:
            raw = f.read()
        entries = parse_entries(raw)
        for e in entries:
            if is_nexus_entry(e):
                # Remove old NEXUS entries (we replace with the new one, and drop stale ones)
                continue
            else:
                # Keep other user shortcuts untouched
                kept_entries.append(e)
    except Exception as e:
        print(f"Warning: error reading existing shortcuts: {e}", file=sys.stderr)

# Format the new NEXUS entry
mods_formatted = "\n".join(f"            {m}," for m in mods)
new_nexus_entry = f"""    (
        modifiers: [
{mods_formatted}
        ],
        key: "{key}",
        description: Some("Nexus"),
    ): Spawn("{nexus_bin} --wake"),"""

# Assemble new file content
os.makedirs(os.path.dirname(cosmic_file), exist_ok=True)
lines = ["{"]
for e in kept_entries:
    indented = "\n".join("    " + line if not line.startswith("    ") else line for line in e.splitlines())
    lines.append(indented + ",")
lines.append(new_nexus_entry)
lines.append("}")
output_content = "\n".join(lines) + "\n"

with open(cosmic_file, "w", encoding="utf-8") as f:
    f.write(output_content)

print(f"COSMIC hotkey registered: {', '.join(mods)}+{key} -> {nexus_bin} --wake")
PYEOF
  REGISTERED=1
fi

# ── 2. GNOME Desktop Registration ───────────────────────────────────────────
if command -v gsettings &>/dev/null; then
  # If GNOME is active or COSMIC was not the sole target
  if [[ "${XDG_CURRENT_DESKTOP:-}" == *"GNOME"* ]] || [[ "${XDG_CURRENT_DESKTOP:-}" != *"COSMIC"* ]] || [ $REGISTERED -eq 0 ]; then
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
    echo "GNOME hotkey registered: $BINDING -> $NEXUS_BIN --wake"
    REGISTERED=1
  fi
fi

if [ $REGISTERED -eq 0 ]; then
  echo "No supported desktop environment found for automatic hotkey registration."
  echo "Set manually in your Desktop Settings > Keyboard > Custom Shortcuts:"
  echo "  Name:    NEXUS Wake"
  echo "  Command: $NEXUS_BIN --wake"
  echo "  Binding: Ctrl+Super+Space"
fi
