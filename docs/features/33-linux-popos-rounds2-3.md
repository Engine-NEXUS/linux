# Linux Pop!_OS Support (Rounds 2–3)

*Date: 2026-09-04*
*Extends: [32-linux-popos-round1](32-linux-popos-round1.md). Target: Pop!_OS 24.04 COSMIC Wayland.*

Round 1 shipped with three defects found by reading code against the real
machine instead of trusting memory. Round 3 deletes a duplicated scanner.

---

## Round 2: corrections (commit `57f59bb`)

| # | Was | Now | How found |
|---|---|---|---|
| 1 | `close_app`: pkill display name → pkill Exec basename | `flatpak kill <app-id>` → pkill basename → pkill display name | `flatpak list` on machine shows Spotify/OBS as `com.spotify.Client` etc. — bwrap hides real names, pkill never matches. App-id parsed from `flatpak run <app-id>` Exec line. |
| 2 | `browser_key`: `wtype -k "ctrl+t"` | `wtype -M ctrl -k t -m ctrl` per-key mapping | wtype 0.4 man page: no combo-string mode, only modifier flags. Old call silently failed every browser key. |
| 3 | `take_screenshot`: grim+slurp first | `cosmic-screenshot` first | `command -v` on machine: cosmic-screenshot installed, grim/slurp absent. Old order always fell through two missing binaries. |
| 4 | `close_app`: two separate `lookup()` calls | single `entry` reused; `exe_name` Windows-only `cfg` | Second lookup = wasted fuzzy match; `exe_name` unused on Linux (warning). |

## Round 3: dedup (this commit)

`find_desktop_entry` deleted. It rescanned 6 dirs by filename on every Tier-2
miss — but `app_registry::discover_linux` already indexes XDG + Flatpak +
snap + PWA at startup with fuzzy match (~0.1ms). `check_installed_linux`
now = registry lookup + launch. One scanner, not two. Zero errors, warnings
all pre-existing (`commands.rs` app_registry import predates this).

---

## Machine facts (verified, not assumed)

- `pactl get-default-source` → `...HiFi__Mic1__source` (Digital Microphone).
  Monitor-sink deprioritization (round 1) correct: 4 `.monitor` sources exist.
- 189 apps in `/usr/share/applications`; Brave Exec line confirmed
  absolute-path + flags (`/usr/bin/brave-browser-stable ... %U`) — registry
  Exec cleaning handles it.
- wtype/grim/slurp NOT installed (`apt policy` shows candidates) —
  doc setup step `sudo apt install grim slurp wtype` still required.
- `nexus.desktop` already installed, Exec points at dev release binary.

## Remaining gaps

Unchanged from round 1 doc section 4: portal GlobalShortcuts, layer-shell
orb, silent screenshot portal, manylinux CI. Plus new: `wmctrl -a` focus
path untested on COSMIC (wmctrl not installed; XWayland-only tool).
