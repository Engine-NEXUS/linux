# Linux port T4a — app discovery, launch and focus

Scope: `src-tauri/src/app_registry.rs`, `src-tauri/src/live/commands/window.rs`
(Linux `unix_impl` only), plus the new `src-tauri/src/app_registry_linux.rs`.

## Why

The audit of the Linux build on the target machine (Pop!_OS 24.04, COSMIC,
Wayland) found the app layer half working:

| Audit line | Verdict before | Cause |
|---|---|---|
| Apps | "Launch OK, focus no" | `.desktop` scanning ignored `Hidden` / `OnlyShowIn` / `NotShowIn` / `TryExec` |
| Apps | "wmctrl parsing broken even on X11" | ids are hex (`0x03e00006`) but were parsed as decimal; `splitn(5, char::is_whitespace)` split on single spaces, so padded columns broke the field count |
| Window focus | "Disabled on Wayland" | `focus_window` returned `true` on every platform, so "Ok sir" was spoken after a no-op |

## What changed

### `src-tauri/src/app_registry_linux.rs` (new)

The whole Linux platform layer, declared from the parent module
(`#[cfg(target_os = "linux")] pub(crate) mod app_registry_linux;`).

* **Discovery** — `XDG_DATA_HOME`, `XDG_DATA_DIRS`, the Flatpak exports
  (system and per-user) and the Snap exports. The user's own directories are
  scanned last, so a user's own copy of an entry beats the system one (the
  registry indexes by name and the last entry written wins).
* **Desktop-entry filters**, honoured the way the spec says:
  * `Hidden=true` and `NoDisplay=true` → never offered.
  * `OnlyShowIn=` → only in the listed desktops. `XDG_CURRENT_DESKTOP` is
    colon-separated and compared case-insensitively. When the current desktop
    is unknown the entry stays hidden: we cannot claim to be a desktop we are
    not (logged, so it is diagnosable).
  * `NotShowIn=` → never in the listed desktops.
  * `TryExec=` → the entry is skipped when the binary is not on `PATH`
    (absolute paths are accepted too).
* **Exec lines** — the field codes (`%f`, `%U`, `%i`, …) are removed without
  destroying the quoting, so `Exec=foo --title="Hello World" %f` stays a
  single, correct argument. `%%` stays a literal percent sign.
* **wmctrl parsing** — ids are read with radix 16, the columns are split on
  runs of whitespace with a fixed column count (`<id> <desktop> <pid> <host>
  <title>`), and the title keeps its internal spacing.
* **Focus** — `wmctrl -i -r … -b remove,hidden` restores, `wmctrl -i -a` then
  activates. On native Wayland there is no EWMH, so the call fails honestly
  instead of pretending.
* **Which window?** The window title rarely equals the `.desktop` name
  ("Text Editor" vs "gedit", "Firefox" vs "Mozilla Firefox"), so the window
  of the app is located by the **PID** the `wmctrl -l -p` output carries and
  raised with `wmctrl -i -a <id>`. The display-name regex is the last
  fallback.
* **"Is this app running?"** — **one** rule, used by both the "is it
  running?" question and the "which window is its?" pick, so the two can
  never disagree. The process name must be the app's own name or that name
  with a version/build decoration (`gimp-2.10`, `firefox-bin`); a different
  product never matches, so `code-insiders` is not `code` and "open code"
  launches VS Code instead of answering "it is running but its window isn't
  showing".
* **"Is it already running?"** — the process table answers this where the
  window list does not exist (Wayland). The table is read **once per
  `focus_existing` call** and that one snapshot is passed down; there is no
  global cache, because a cached table would report "not running" for an app
  that started a moment ago and the caller would launch a second copy of it.
  Shells, interpreters and terminal emulators are excluded on purpose:
  running them again *is* how the user asks for a new window.

### `src-tauri/src/app_registry.rs`

* `focus_window` now returns whether the window was actually raised. The
  Windows branch is unchanged apart from the trailing `true`; macOS was never
  reaching it (its window cache is always empty) and now reports `false`.
* New `FocusOutcome { Focused, WindowNotReady, RunningCannotFocus(String),
  NotRunning }` and `focus_existing()`, replacing `try_focus_existing() ->
  bool`. The tri-state is what lets the caller say *"I can't bring it to the
  front, sir — this is a native Wayland session"* instead of launching a
  second copy of an app the user already has open. `WindowNotReady` is the
  third state the wait loop needs: the app's **process** is up but its
  **window** is not mapped yet, so the caller keeps waiting instead of
  answering "cannot focus".
* On Linux the window of the running process is located by PID and raised by
  id; the display-name regex is only the fallback.
* New `focus_by_title()` (Linux), the EWMH title form the legacy resolver
  needs.
* The Linux window cache now uses the fixed wmctrl parser.

### `src-tauri/src/live/commands/window.rs` (Linux `unix_impl` only)

* New `focus_unavailable_reason()` — the speakable reason focus is not
  possible, so callers can be precise instead of guessing.

### `src-tauri/src/command_executor.rs` (small, outside the owned area)

* `resolve_and_open_app` uses the outcome: `RunningCannotFocus` is spoken and
  **no second copy is launched**; `WindowNotReady` waits (and, if the app was
  not launched recently, says "X is running, sir, but its window isn't
  showing" instead of starting a duplicate); `NotRunning` still launches.
* The Linux branch of `check_running_and_focus` used to spawn `wmctrl` and
  throw the result away while claiming success — and on returning `None` it
  made the caller launch a **second copy** of an app that was already running
  (the "7 Notepad windows" regression). The decision is now the pure
  `legacy_focus_action(focused, blocked)`, unit-tested so that a blocked
  session can never again resolve to "launch a copy".
* The Linux legacy `.desktop` grep now goes through the real scanner, so the
  filters apply to it too.

### `src-tauri/src/orchestrator.rs` (small, outside the owned area)

* `run_focus_app` speaks the specific Linux reason ("I can't switch to it,
  sir — …") instead of "Couldn't find X running".

## Not done / known gaps

* There is no "open a new window" phrase in the grammar
  (`ParsedIntent::OpenApp { target }` has no flag for it), so the
  "already running → don't launch a second copy" guard applies to every plain
  "open X". The carve-out is by Exec-line shape (terminals, shells,
  interpreters), not by intent.
* `get_foreground_window_title()` on Linux still uses `xdotool`, which is not
  installed on the target machine. It returns `None` honestly; the AT-SPI tier
  (`pointer.rs`) covers the privacy gate on Wayland.
* Launching still goes through `sh -c <exec>` (unchanged shared code). The
  Exec line is now cleaned correctly, so the quoting works.
* The disk cache version was not bumped: entries discovered by the old scanner
  stay indexed until the next daily rescan.
