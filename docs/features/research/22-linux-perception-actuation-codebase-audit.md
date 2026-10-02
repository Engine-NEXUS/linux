# 22 — Appendix: Linux Perception & Actuation Codebase Audit

**Date:** 2026-10-02
**Status:** Evidence base for [20 — Is Pixels the Right Abstraction?](./20-linux-computer-control-architecture-rethink.md) and [21 — Full Comparison](./21-linux-computer-control-full-comparison.md).
**Method:** systematic grep/read of the tree at commit `86a35b1`. Every claim below carries a `file:line` citation. Claims without one were verified by absence (an exhaustive search that returned nothing), and those searches are stated explicitly.

**Read this before acting on doc 20 or 21.** Those documents argue from what the
platform offers; this one records what the codebase actually does. Where they
disagree, this document is the current state.

---

## 1. Actuation

### 1.1 Keyboard — `enigo`, X11-only by dependency default

`src-tauri/Cargo.toml:112` declares `enigo = "0.5"` with **no feature flags**.

The vendored crate resolves to `enigo-0.5.0`, whose `Cargo.toml` sets
`[features] default = ["x11rb"]`. The alternative Linux backends — `wayland`,
`libei`, `xdo` — exist but are **non-default and not enabled**. With only `x11rb`
compiled, `Enigo::new` constructs a single X11 connection and returns
`Err(NewConError::EstablishCon("no successful connection"))` when `$DISPLAY` is
unset.

| Call site | Mechanism |
|---|---|
| `src-tauri/src/live/commands/keyboard.rs:30` | `Enigo::new(&Settings::default())` → `.text()` |
| `keyboard.rs:63-80` | `Ctrl/Cmd+V` via enigo (clipboard-paste path for >50 chars) |
| `keyboard.rs:95-102` | `press_key` |
| `keyboard.rs:116-143` | `press_hotkey` (mods down → key → mods up) |
| `src-tauri/src/screen.rs:266-287` | `switch_browser_tab` (Ctrl+1..9) |
| `src-tauri/src/screen.rs:178-189` | `move_mouse` + `Button::Left` — **inside a `#[cfg(target_os = "windows")]` module** |

It works only because `install.sh:76` forces `Exec=env GDK_BACKEND=x11 …/nexus`,
putting NEXUS itself under XWayland. **Native Wayland applications are therefore
unreachable.** Searched for `WAYLAND_DISPLAY` and `XDG_SESSION_TYPE` across
`src-tauri/src`, `nexus.mjs`, `scripts/*.sh`: **zero hits**. The app cannot detect
its session type, so it cannot choose a backend or explain a failure.

**Clipboard:** `arboard = "3.4"` (`Cargo.toml:113`, no features) → resolves to
`3.6.1` (`Cargo.lock:158`). `arboard-3.6.1` sets `default = ["image-data"]`; its
`wayland-data-control` feature is not enabled, and its Linux implementation is
`src/platform/linux/x11.rs`. So `keyboard.rs:48-57` is X11-only.

### 1.2 Mouse — absent on Linux

The only mouse code in the repository is `screen.rs:176-190` (`click_element`),
exposed by `#[cfg(target_os = "windows")] pub use win::{click_element, …}` at
`screen.rs:250-251`. Its sole caller is `orchestrator.rs:2125`, itself inside a
`#[cfg(target_os = "windows")]` block (`orchestrator.rs:2119-2136`). The
non-Windows arm at `orchestrator.rs:2137-2141` returns the string
`"Screen clicking needs Windows, sir."` and performs no action.

Searched for `scroll`, `move_mouse`, `Button::`, `Direction::`: matches only at
`screen.rs:178-187`, `screen.rs:266-286`, `keyboard.rs:17`, plus an unrelated
`petgraph::Direction` in `architect.rs`. **No scroll, no drag, no right-click, no
double-click exists anywhere.**

### 1.3 The one Wayland-native input path — 4 hotkeys, errors discarded

`src-tauri/src/command_executor.rs:748-768`, `browser_key()`:

| Platform | Mechanism |
|---|---|
| Windows | PowerShell `SendKeys` (`:724`) |
| macOS | `osascript` keystroke (`:745`) |
| Linux | `wtype -M ctrl -k t -m ctrl` (virtual-keyboard protocol), `.or_else` → `xdotool key` (`:748-768`) |

Callers are `command_executor.rs:121-124` — `browser_new_tab`,
`browser_close_tab`, `browser_next_tab`, `browser_back`. That is the whole
surface.

Two defects: `wtype` speaks `wlr-virtual-keyboard`, which **does not exist on
GNOME** — so this cannot work on the most common Linux desktop. And both branches
use `.spawn()` with the `Result` **discarded** (`:761-765`), so a missing binary
produces silence rather than an error.

### 1.4 X11-only shell-outs that no-op on native Wayland

| Location | Tool | Effect on native Wayland |
|---|---|---|
| `browser_url.rs:246-264` | `xdotool getactivewindow`, then `xdotool key --window<wid>` | `getactivewindow` exits non-zero → returns `None` (`:250-252`) |
| `browser_url.rs:267-270` | `xclip -selection clipboard -o` | reads the X11 clipboard only |
| `architect.rs:437-441` | `xdotool getactivewindow getwindowname` | `None` → repo auto-detect layer 2 is dead |
| `app_registry.rs:666` | `wmctrl -l -p` | EWMH is X11/XWayland-only → `WINDOW_CACHE` stays empty |
| `app_registry.rs:589-592` | `focus_window()` | `#[cfg(not(target_os = "windows"))] { let _ = hwnd_val; }` — **literal no-op** |
| `command_executor.rs:923` | `wmctrl -a <title>` | X11 only |

### 1.5 Window focus — two paths that report success while doing nothing

- `live/commands/window.rs:111-120`: `#[cfg(not(windows))] pub fn
  focus_app_by_title(_: &str) -> bool { tracing::warn!("…"); true }`. Returns
  `true` after doing nothing.
- `app_registry.rs:485-543` `try_focus_existing` calls it and returns `true`;
  `command_executor.rs:799-805` then speaks a success message.

The Tauri command `live_focus_app` (`live/mod.rs:375-401`) short-circuits even
earlier with `Err("Window focus not supported on this platform")` (`:397-400`).

**Net effect: the user is told the window was focused. Nothing was focused.**

### 1.6 Actuation that does work on Linux

| Mechanism | Location |
|---|---|
| MPRIS media control over D-Bus (`zbus`) | `src-tauri/src/mpris.rs:5-64` |
| `org.freedesktop.Notifications` | `mpris.rs:73-103` |
| Volume get/set — `wpctl` → `pactl` → `amixer` | `volume.rs:311-390`; mute at `command_executor.rs:630-638` |
| Launch/open — `open::that` / `xdg-open` / `flatpak` | `command_executor.rs:417-439`, `live/commands/browser.rs:67`, `live/commands/whatsapp.rs:30` |
| Kill — `pkill` / `flatpak kill` | `command_executor.rs:265-282` |
| Lock session — `loginctl lock-session` | `command_executor.rs:696-699` |
| Screenshot picker — `cosmic-screenshot` → `grim+slurp` → `gnome-screenshot` → `spectacle` → `flameshot` | `command_executor.rs:661-674` |
| Global hotkey | `hotkey.rs:20` is `#![cfg(not(target_os = "linux"))]` — **absent**; replaced by a `gsettings` DE keybind at `lib.rs:887-932` and `scripts/register-hotkey.sh` |

### 1.7 The whole live-mode actuation layer is unreachable from voice

14 `live_*` Tauri commands are registered at `lib.rs:1060-1073`. Searching
`frontend/src` for `live_type_text`, `live_press_key`, `live_press_hotkey`,
`live_whatsapp*`, `live_browser*`, `live_open_site`, `live_focus_app`,
`live_cancel`, `live_get_state`: **zero hits.**

The NLU bridge maps all 11 live intents into `ParsedIntent::NluResult`
(`nlu_client.rs:415-427`), and `route_intent` sends `NluResult` to
`Subsystem::WorkerBackend` (`orchestrator.rs:462-463`) — i.e. the raw text goes to
a cloud LLM for a text reply.

**Therefore `keyboard.rs`, `whatsapp.rs` and `browser.rs` are dead code from the
product's voice path**, reachable only by a renderer explicitly invoking them,
which nothing does.

---

## 2. Perception

### 2.1 Screenshot — portal-based, primary monitor only

| Platform | API | Location |
|---|---|---|
| **Linux** | `org.freedesktop.portal.Desktop` `Screenshot` over raw `zbus` | `screen.rs:393-399`, `:404-495` |
| Windows | GDI `BitBlt` via `sidebar_backdrop::capture_region_bgra_public` | `screen.rs:370-391` |
| macOS | `/usr/sbin/screencapture -x -tjpg` | `screen.rs:527-549` |
| other | hard error | `screen.rs:551-554` |

Linux notes: fast-fails with a user-facing message when
`org.freedesktop.portal.Desktop` is absent (`screen.rs:419-431`); registers
`AddMatch` by hand because zbus 5 lacks server-side match registration
(`:449-467`); 25 s response timeout (`:469-471`); reads the portal temp file,
percent-decodes the URI, deletes it (`:481-493`). Output is downscaled to max
width 1280 (`:299`, `:359-368`) and JPEG-encoded under 900 KB (`:334-355`).

**No `current_monitors()` loop** — single "primary" screen only. `dyn_windows.rs:40-41`
concedes: *"single-monitor stage; multi-monitor roam needs one stage per monitor."*
Cache TTL is 10 s (`orchestrator.rs:171`, `:2223-2245`).

### 2.2 OCR — Rust client + Python sidecar

- `src-tauri/src/ocr.rs`: `OcrBox` (`:10-18`), `merge_lines()` word→line
  (`:31-91`), `snap_to_text()` VLM-point→box with distance rejection
  (`:99-126`), `parse_response()` (`:130-156`), `ocr_image()` POST of raw JPEG to
  `127.0.0.1:39220` (`:160-189`).
- `src-tauri/src/lazy_ocr.rs`: spawn on demand (`:84-163`), port 39220
  (`:21`, `:27-32`), 120 s idle kill (`:20`, `:165-183`), 60 s failure cooldown
  (`:24`).
- `server/ocr_server.py`, bundled to `src-tauri/resources/server/ocr_server.py`
  and listed in `tauri.conf.json:36`.

**Deployment gap:** `rapidocr`, `opencv-python-headless`, `fastapi`, `uvicorn`
appear in **no `requirements.txt`** and in **no install script** — `install.sh`,
`install-prod.sh` and `scripts/build*.sh` contain no `pip` invocation at all. On a
clean Linux install this tier fails, and the caller falls back to raw VLM
coordinates after `lazy_ocr` burns its **60 s** first-start budget
(`lazy_ocr.rs:23`, `:123-137`).

Offline harness: `scripts/vision_accuracy.py` (synthetic images, 90% snap
hit-rate bar).

### 2.3 Vision LLM — cloud only

`src-tauri/src/vision.rs`: `ScreenVisionKind{Describe, Locate, ReadText}`
(`:21-26`); `PointNorm` normalised 0–1000 (`:29-34`); JSON `{speak, point}`
contract (`:43-51`); `ask_about_screen()` Gemini→Groq cascade (`:112-138`); Gemini
`inline_data` (`:140-199`); Groq `image_url` data-URI, two model IDs (`:201-270`).
Keys from `router::read_provider_keys` (`orchestrator.rs:2194`); **errors with
`"no vision provider available"` without a Gemini or Groq key** (`:137`). Wired at
`orchestrator.rs:567-600` → `run_screen_vision` (`:2188-2332`).

**No local or offline vision path exists.**

### 2.4 Accessibility tree — absent, by explicit admission

`screen.rs:253-258`:

```rust
/// Non-Windows locate scaffolding: no a11y tree wired yet (Phase 3b/4
/// covers Linux/macOS via OCR+VLM). Always misses so callers fall through.
#[cfg(not(target_os = "windows"))]
pub fn find_by_name(_query: &str) -> Option<UiElement> { None }
```

Searched `atspi`, `at-spi`, `pyatspi`, `dogtail`, `AT-SPI` across `src-tauri/`,
`server/`, `frontend/`: **zero code hits** (matches only in `docs/` and the BERT
tokenizer vocabulary). The repository already names this the top perception gap:
`research/16-computer-use-linux-gap-perception.md:182-205` ("NOT IMPLEMENTED"),
`features/62-…md:214`.

### 2.5 DOM / web extraction — absent

Searched `devtools`, `CDP`, `remote-debugging`, `9222`, `playwright`, `webdriver`,
`selenium`, `puppeteer` across `src-tauri/src`, `server/`, `frontend/src`,
`nexus.mjs`: **zero code hits.** Occurrences are documentation only, including
`features/21-liquid-glass-sidebar.md`, which describes how a *human* would manually
attach CDP to verify a release build. No `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS`,
no `--remote-debugging-port`, no headless browser.

### 2.6 Other perception inputs

| Signal | Linux behaviour |
|---|---|
| Foreground window title (`architect.rs:392-448`, `pointer.rs:98-114`) | **`None`** on non-Windows (`pointer.rs:116-119`) |
| Foreground geometry / fullscreen (`pointer.rs:123-150`) | **`false`** on non-Windows (`:147-150`) |
| Browser URL (`browser_url.rs:15-32`) | xdotool + xclip — X11 only |
| Process list (`sysinfo`) | works (`command_executor.rs:893-941`, `app_registry.rs:637-649`) |
| Installed-app registry | **Windows Registry only** for discovery (`app_registry.rs`); Linux falls through to `flatpak`/`pkill`/`xdg-open` guesses |

### 2.7 The privacy gate is inert on Linux

`pointer.rs:156-163` `exclusion_gate()` short-circuits on
`let title = foreground_title()?;` (`:161`). Because `foreground_title()` returns
`None` off-Windows (`:116-119`), the bank / `1password` / crypto-wallet exclusion
list **never matches**, and `run_screen_vision` proceeds to screenshot and OCR
(`orchestrator.rs:2196-2202`). Documented as "Windows-only enforcement" at
`pointer.rs:95-97` and `:152-155`.

**This is a live safety defect, not a roadmap item.**

---

## 3. Window management

| Concern | Implementation | Wayland reality |
|---|---|---|
| Overlay windows | `dyn_windows.rs:14-133` (7 `WindowConfig`s), created on demand `:137-218` | works as ordinary toplevels |
| Orb position | `window_manager.rs:14-16` `position_orb()` → `Ok(())`; `:23-31` `set_orb_position` *"accepts and ignores the values"* (`:19-22`) | impossible by design |
| Orb roaming | `main` is a maximized transparent stage; orb moves by CSS transform (`dyn_windows.rs:173-175`, `window_manager.rs:41-52`) | works |
| Click-through | `window_manager.rs:58-63` → `set_ignore_cursor_events`; skipped while hidden (`:47-51` documents the `tao` panic at `event_loop.rs:457`) | broken on Mutter; frontend works around it with `document.elementFromPoint` (`frontend/src/overlay/clickThrough.ts:23-33`) |
| Always-on-top | `window_manager.rs:36`, `:76`, `:85`, `:100` | no-op; `features/62-…md:41` cites `gdkwindow-wayland.c:4631-4634` as an empty function |
| Sidebar backdrop blur | `sidebar_backdrop.rs`, gated `#[cfg(windows)]` at `lib.rs:78-79` | absent |
| DWM corners | `dwm_corners.rs`, `#[cfg(windows)]` (`lib.rs:76-77`) | absent |
| macOS vibrancy | `dyn_windows.rs:201-214` | n/a |

Foreign-window enumeration: Windows `EnumWindows` (`app_registry.rs:597-654`);
Linux `wmctrl -l -p` (`:662-688`); **macOS: no implementation** (`:656-660`).
Focus: Windows `AttachThreadInput` (`app_registry.rs:548-587`,
`live/commands/window.rs:70-98`); non-Windows no-op as above.

Searched `xdg-shell`, `wlr-layer-shell`, `gtk-layer-shell`: **no matches**, and
no `wayland-client` / `wayland-protocols` dependency in `Cargo.toml`.
`features/62-…md:50-56` and `features/28-linux-wayland-compatibility.md:16` explain
why.

---

## 4. Tool surface

### 4.1 Generic GUI poking — 19 tools, all keyboard-only on Linux

| Tool / intent | Definition | Actuation |
|---|---|---|
| `live_type_text` | `live/mod.rs:120-147` → `keyboard.rs:28-43` | enigo text or clipboard paste |
| `live_press_key` | `live/mod.rs:150-161` → `keyboard.rs:93-104` | enigo |
| `live_press_hotkey` | `live/mod.rs:164-176` → `keyboard.rs:109-146` | enigo |
| `live_browser_new_tab` / `navigate` / `search` | `live/mod.rs:306-352` → `browser.rs:17-58` | Ctrl+T / Ctrl+L + type + Enter |
| `live_open_site` | `live/mod.rs:355-371` → `browser.rs:62-70` | `open::that` (not GUI) |
| `live_whatsapp_*` (4) | `live/mod.rs:179-303` → `whatsapp.rs:27-98` | `open::that("whatsapp://")` then Ctrl+F + type + Enter |
| `live_focus_app` | `live/mod.rs:374-401` | Windows-only; errors on Linux |
| `screen_click` | `orchestrator.rs:2113-2148` | Windows UIA + enigo mouse |
| `screen_read` | `orchestrator.rs:2150-2183` | Windows UIA only |
| `browser_tab` | `orchestrator.rs:2412-2428` → `screen.rs:262-289` | enigo |
| `browser_new_tab` / `close_tab` / `next_tab` / `back` | `command_executor.rs:121-124` → `:708-774` | `wtype` → `xdotool` |

Safety layers: `live/safety.rs:14-31` (16 allowed), `:35-51` (15 denied), `:54-58`
(3 confirm-gated), `:80-98` `safety_check`.

Screen *reading as an action* (describes/points, does not act):
`screen_describe`, `screen_locate`, `screen_read_text`, `repeat_screen`,
`point_again` (`intent_parser.rs:135-153` → `orchestrator.rs:2188-2401`).

### 4.2 App-specific integration — ~66 tools

| Surface | Count | Location |
|---|---|---|
| GitHub via octocrab, on-device | **30** (`MergePr`, `ApprovePr`, `ClosePr`, `ListPrs`, `GetPr`, `CreatePr`, `UpdateBranch`, `RevertPr`, `ListPrFiles`, `CommentPr`, `AddCollaborator`, `RemoveCollaborator`, `ListCollaborators`, `AddOrgMember`, `RemoveOrgMember`, `ListOrgMembers`, `ConvertToOutsideCollaborator`, `ListOutsideCollaborators`, `SetBranchProtection`, `DeleteBranch`, `ListBranches`, `CreateRelease`, `ListReleases`, `DeleteRelease`, `ListWorkflows`, `ListWorkflowRuns`, `RerunWorkflow`, `CancelWorkflow`, …) | `github_cmd.rs:218-348+`; routed `orchestrator.rs:428` |
| Swiggy Food MCP | 7 | `mcp_client.rs:64`, `:99-104` |
| Swiggy Instamart MCP | 6 | `mcp_client.rs:65`, `:105-110` |
| Swiggy Dineout MCP | 3 | `mcp_client.rs:66`, `:111-113` |
| WhatsApp MCP (`127.0.0.1:8765`) | 6 | `mcp_client.rs:67`, `:114-116`, `:608` |
| Amazon MCP (`127.0.0.1:8766`) | 1 | `mcp_client.rs:68`, `:117-119`; `orchestrator.rs:1584` |
| Worker intents | 12 | `server/worker/src/index.ts:2764-2870`; handlers `:513`, `:655`, `:1296`, `:1496`, `:1630`, `:1679`, `:1711`, `:1799`, `:2900`, `:2935`, `:3364`, `:3468` |
| MPRIS / volume / open / lock | 4 | `mpris.rs`, `volume.rs`, `command_executor.rs` |

### 4.3 The ratio

**19 generic-GUI : ~66 API-integration ≈ 1 : 3.5.** Per docs 21 §7 this is the
*correct* direction — UI-TARS-2 argues pure GUI is the wrong default. But here it
is incidental rather than designed, and the generic-GUI half is X11-only and
unreachable from voice.

---

## 5. Absence register

Verified by exhaustive search. These are the concrete things docs 20 and 21 propose
adding.

| Technology | Verdict | Evidence |
|---|---|---|
| AT-SPI accessibility tree | **absent** | stub `None` at `screen.rs:253-258`; no `atspi`/`pyatspi`/`dogtail` symbols |
| CDP / DevTools Protocol | **absent** | no `devtools`, `remote-debugging`, `9222` in code |
| Playwright / WebDriver / Selenium / Puppeteer | **absent** | same search; docs only |
| `libei` / `ei` | **absent** | enigo has a `libei` backend but no feature enables it (`Cargo.toml:112`); no `libei` in NEXUS code. Only false positives: `resources/espeak-ng-data/phondata-manifest` |
| `ydotool` | **absent** | appears only in `research/16-…:79,93,149` as competitor research |
| `wlr-virtual-keyboard` | **absent** | appears only in `features/28-…:16` and `features/62-…:50` as a rejected option |
| `wtype` | **partial** | `command_executor.rs:752-762`; 4 fixed hotkeys; silent failure |
| `xdg-shell` / `wlr-layer-shell` | **absent** | no `wayland-client` dependency |
| `org.freedesktop.portal.RemoteDesktop` | **absent** | only `Screenshot` is used (`screen.rs:437-439`); `GlobalShortcuts` also absent (`lib.rs:370-373`, `hotkey.rs:20`) |
| Direct `x11rb` use | **absent (transitive only)** | `Cargo.lock:8367` pulls it via `enigo` (`:1606`), `arboard` (`:174`), tauri (`:2205`); no `use x11rb` in `src-tauri/src` |
| DRM/KMS capture | **absent** | no `libdrmtap` / `drmModeGetFB2` |
| Coordinate→element hit-test | **partial** | ordinal→element `screen.rs:26-42`; text→pixel `ocr.rs:99-126`; normalized→physical `pointer.rs:189-190`; physical→CSS `pointerMath.ts:40-47`; clamp `:23-35`. **No verification that the resulting coordinate landed on anything.** `pointer::show_direct` (`:198-213`) never re-checks |

---

## 6. Defect summary, ordered by severity

Ranked by consequence, not effort.

| # | Defect | Severity | Location |
|---|---|---|---|
| 1 | Privacy exclusion gate inert → banking and password-manager windows are screenshotted and OCR'd | **Safety** | `pointer.rs:156-163` + `:116-119` |
| 2 | `focus_window` returns `true` while doing nothing, and the caller says "Ok sir" | **Correctness + trust** | `live/commands/window.rs:116`, `app_registry.rs:589-592`, `command_executor.rs:799-805` |
| 3 | No mouse actuation on Linux, while the app offers `screen_click` | **Capability** | `screen.rs:250-251`, `orchestrator.rs:2137-2141` |
| 4 | Keyboard is X11-only; no session-type detection to fail loudly or fall back | **Capability** | `Cargo.toml:112`, zero `XDG_SESSION_TYPE` hits |
| 5 | Input-path `.spawn()` results discarded → silent no-op | **Diagnosability** | `command_executor.rs:761-765` |
| 6 | live-mode actuation dead from voice — 14 commands, 0 callers | **Capability** | `lib.rs:1060-1073`, `orchestrator.rs:462-463` |
| 7 | OCR Python deps in no requirements/install path; 60 s silent first-start | **Deployment** | `install.sh`, `lazy_ocr.rs:23` |
| 8 | No verification that a dispatched action landed | **Correctness** | `pointer.rs:198-213` |
| 9 | Single-monitor capture only | **Capability** | `screen.rs:393-399`, `dyn_windows.rs:40-41` |
| 10 | Vision requires a cloud key; no offline path | **Capability** | `vision.rs:137` |

**Items 1, 2 and 5 are independent of any architectural debate and should be fixed
regardless of which approach is chosen.**

---

### Sources

Internal: repository at commit `86a35b1`, plus
`features/62-competitive-and-platform-audit-2026-10.md`,
`research/16-computer-use-linux-gap-perception.md`,
`research/17-security-oauth-prompt-injection.md`,
`features/28-linux-wayland-compatibility.md`,
`features/21-liquid-glass-sidebar.md`.

External: enigo 0.5.0 and arboard 3.6.1 crate sources (`Cargo.toml` feature
tables); `@ricky0123/vad-web` 0.0.31 `frame-processor.js`; `rapidocr` 3.9.2;
GNOME `gdkwindow-wayland.c`; `tao` `event_loop.rs`.