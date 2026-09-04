# Linux Pop!_OS Support (Round 1)

*Date: 2026-09-04*
*Target: Pop!_OS 24.04 (COSMIC, Wayland). Generalizes to GNOME/KDE Wayland.*

Wayland blocks three things NEXUS relies on: global hotkeys (`XGrabKey`
dead), `set_position` (ignored), click-through toggle (ignored). Round 1
works around all three with zero new deps. Portal GlobalShortcuts (ashpd)
deferred — COSMIC/GNOME coverage still thin.

---

## 1. Changes

| File | Change | Reason |
|---|---|---|
| `wakeword_oww.rs` | Linux: `try_order.sort_by_key` pushes `*monitor*` devices last | PipeWire `...monitor` = speaker loopback, never mic input. Old code probed it first, wasted 5s, sometimes settled on static (RMS ~0.003). |
| `lib.rs` | Cold-start `--wake` handler: show + `configure_non_activating_overlay` + `__NEXUS_WAKE__()` | Second-instance `--wake` already worked via single-instance callback. But DE keybind with no instance running launched a dead-idle orb. Now it wakes too. |
| `command_executor.rs` (`find_desktop_entry`) | + Flatpak (`/var/lib/flatpak`, `~/.local/share/flatpak`) + snap dirs | Pop!_OS ships many apps as Flatpak. Old 3-dir list missed them. |
| `command_executor.rs` (`close_app`) | Fallback: pkill Exec basename when display-name pkill fails | Flatpak apps run as `bwrap`; display-name match misses. Exec basename (`org.gimp.GIMP` → still bwrap, but native-packaged apps now close reliably). |
| `command_executor.rs` (`take_screenshot`) | `grim -g "$(slurp)"` first, then COSMIC/GNOME/Spectacle/flameshot | grim works COSMIC+Sway+Hyprland today. Silent portal capture deferred. |
| `command_executor.rs` (`browser_key`) | `wtype` first, `xdotool` fallback | xdotool = X11-only, dead on Wayland. wtype speaks Wayland natively. |

`cargo check` clean (14 warnings, all pre-existing dead-code).

---

## 2. User setup (Pop!_OS)

1. Build + install per README.
2. Settings → Keyboard → Custom Shortcut:
   - Command: `/path/to/nexus --wake`
   - Binding: `Super+Space` (Wayland-safe; no `XGrabKey` involved).
3. First run: `pavucontrol` → Input Devices → confirm real mic selected
   (PipeWire default sometimes lands on monitor/dummy).
4. Screenshot picker needs `grim` + `slurp`:
   `sudo apt install grim slurp wtype`.

---

## 3. Verify on machine

```
cargo check --manifest-path src-tauri/Cargo.toml   # compiles
nexus --wake          # orb shows + listens (cold start)
nexus --wake          # same while running (single-instance path)
"open firefox"        # .desktop launch incl. Flatpak apps
"close firefox"       # pkill path
"take screenshot"     # grim region picker
"mute"                # wpctl toggle
```

---

## 4. Remaining gaps (next rounds)

1. **Portal GlobalShortcuts** (`ashpd` `GlobalShortcuts` interface) —
   real system-wide hotkey without DE keybind. Blocked on COSMIC portal
   support; re-check per release.
2. **Layer-shell orb** (`gtk-layer-shell`) — true always-on-top +
   click-through on Wayland. Current orb = normal window, fine for v1.
3. **Precise Flatpak kill** — `flatpak kill <app-id>` when Exec basename
   still misses (bwrap). Needs app-id plumbed through `AppEntry`.
4. **Silent screenshot** — Screenshot portal via ashpd when COSMIC portal
   supports it. Current grim path always shows picker.
5. **manylinux CI** — Python sidecars link host glibc; build in
   manylinux2014 container for Ubuntu 22.04 / Debian 12 compat.

---

## 5. Deliberate simplifications

- Monitor-skip = name heuristic, not `pw-cli` node-type check.
  Upgrade when PipeWire-native enumeration lands.
- `ponytail:` comments in code mark each ceiling + upgrade path.
