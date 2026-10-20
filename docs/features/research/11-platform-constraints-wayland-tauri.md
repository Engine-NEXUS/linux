# Platform Constraints — Tauri v2 on Linux, and the Orb Architecture That Cannot Work

> **Part of the 2026-10 competitive & platform audit.**
> Decision record: [`docs/features/62-competitive-and-platform-audit-2026-10.md`](../62-competitive-and-platform-audit-2026-10.md)
> Companion compendium (identical content, canonical home):
> [`NEXUS-PAPERS → research/2026-10-competitive-audit/`](https://github.com/Engine-NEXUS/NEXUS-PAPERS)
>
> This file is **evidence**. It records what was verified on 2026-10-02, from
> where, and what could not be verified. It is not a work order — see the spec above
> for that.

> **What this covers:** Tauri v2 / Linux Wayland capability matrix, the unfixed `tao` panic, and the window-architecture alternatives.


**Date:** 2026-10-02
**Scope:** What Tauri v2 and the underlying GTK/WebKitGTK stack can actually do on
Linux today, specifically regarding the always-on floating orb.
**Verdict:** The current architecture (`dyn_windows.rs`) relies on three Wayland
capabilities that are all either no-ops, panics, or compositor-dependent bugs.

---

## 1. The current architecture, and why each part fails

`src-tauri/src/dyn_windows.rs:43-50`:

```rust
label: "main", url: "index.html",
width: 200., height: 200., resizable: false,
decorations: false, transparent: true,
always_on_top: true, skip_taskbar: true, shadow: false,
focus: false, center: false, hidden_title: true,
```

…and `:174`: `builder = builder.maximized(true);` — a fullscreen transparent
work-area overlay, with the orb positioned inside by frontend CSS transform
(`useRoam.ts`) and click-through via `set_ignore_cursor_events`.

### 1.1 `always_on_top` is an empty function on Wayland

Verified against GTK 3.24.50 source, `gdk/wayland/gdkwindow-wayland.c`:

```c
static void
gdk_wayland_window_set_keep_above (GdkWindow *window, gboolean setting)
{
}                                    /* lines 4631-4634 */

static void
gdk_wayland_window_set_keep_below (GdkWindow *window, gboolean setting)
{
}                                    /* lines 4636-4639 */
```

Both are wired directly into the vtable at `:5125-5126`. The call chain is:
`tao::Window::set_always_on_top` → `WindowRequest::AlwaysOnTop` →
`gtk_window_set_keep_above(true)` → `window.set_keep_above(true)` → **nothing,
silently, forever.**

Tauri's own docs say as much: `tao::Window::set_outer_position` rustdoc states
*"Linux(Wayland): Has no effect, since Wayland doesn't support a global
cordinate system."*

Open issues, all still open:
- `tauri#14913` — "[bug] Window position & order management does not work on
  Linux (wayland backend)", filed **2026-02-08**. Verbatim: *"Calling
  `tauri::Window::set_position` and configuring a window with
  `tauri::Window::always_on_top` does not throw an error or successfully resolve
  (**it silently fails/no-ops**)… If tauri won't provide a route to supporting
  `set_position` on gnome+wayland, please give a way to expose/access the raw u32
  window id that can be routed to a gnome extension."*
- `tauri#3117` — "Always on top not working on Wayland"
- `tao#1134` — closed 2025-08-25, redirected to `winit#2435`

**Our own code already documents half of this.** `window_manager.rs:19-22`:
*"Linux/Wayland no-op: the compositor forbids client positioning… Kept as a
command so the settings slider + lib.rs handler compile; accepts and ignores the
values."* The `always_on_top` half is unfixed and uncommented.

**User-facing consequence:** the Display tab offers Orb Position (horizontal %
and vertical % sliders) and Orb Size (100–300 px). On Wayland, position does
nothing. Size is read by `SettingsSidebarApp.tsx:243` but consumed by **nothing**
in the frontend — the orb size is hardcoded `180` in four separate places
(`styles.css:35-36`, `Avatar.tsx:177-187`, `useRoam.ts:3`). **Two of the five
Display-tab settings are non-functional, and the UI does not say so.**

### 1.2 `set_position` is impossible by design, not by bug

Authoritative sources:

- **w3c/window-management#68**, comment by pq (Chromium/Wayland maintainer):
  > *"Wayland was explicitly designed to prohibit clients from introspecting or
  > programmatically changing their global window coordinates. 'Wayland doesn't
  > provide clients with global screen coordinates.' It is a design decision in
  > Wayland/desktop to not expose absolute window positions to clients at all."*
- **SDL3 `README-wayland.md`** lists it under the heading literally titled:
  > *"### `SDL_SetWindowPosition()` doesn't work on non-popup windows — Wayland
  > does not allow toplevel windows to position themselves programmatically."*
- **swaywm/sway#6937**, maintainer answer: *"This is by design. Wayland clients
  can't dictate their absolute position."*
- **Chromium `exo`** implements `xdg_positioner_set_anchor_rect` and posts
  *"Wayland Positioner"* errors — positioning is negotiated with the compositor,
  never unilateral.

What IS allowed: `zwlr_layer_shell_v1`, which replaces coordinates with
**anchors + margins + exclusive zone**.

### 1.3 `set_ignore_cursor_events` panics — unfixed in current stable tao

Read from the released source of **`tao 0.37.1`**,
`src/platform_impl/linux/event_loop.rs:452-462`:

```rust
WindowRequest::CursorIgnoreEvents(ignore) => {
  if ignore {
    let empty_region = Region::create_rectangle(&RectangleInt::new(0, 0, 1, 1));
    window
      .window()
      .unwrap()                    // ← PANICS when GdkWindow is None
      .input_shape_combine_region(&empty_region, 0, 0);
  } else {
    window.input_shape_combine_region(None)
  };
}
```

The identical `.unwrap()` is on the `dev` branch. `gtk::Window::window()`
returns `None` whenever the window is unrealized (never mapped, or hidden).

**Independent reports:**
- `jamiepine/voicebox#873` — 2026-07-09, 100% reproducible crash. Ubuntu 24.04
  X11, tao 0.34.5, Tauri 2.9.5. Workaround shipped in PR #906: **disable
  `transparent` on Linux and `#[cfg(not(target_os = "linux"))]` around every
  `set_ignore_cursor_events` call.** Closed 2026-07-20.
- `jamiepine/voicebox` PR #748 (`fb6d8d9`, 2026-06-13): *"On Fedora 44 / GNOME 50
  (Wayland-only), tao's CursorIgnoreEvents handler calls
  `window.window().unwrap()` which returns None."* Their fix was to
  **vendor-patch tao** (110 `.rs` files committed into their repo).

**There is no upstream fix and no open upstream issue tracking this specific
panic.** The only fixes in the wild are third-party workarounds.

Note also: tao passes `Region::create_rectangle(&RectangleInt::new(0,0,1,1))` —
a **1×1 region at the origin, not an empty region**. The top-left pixel of the
"click-through" overlay stays clickable forever. GDK has a dedicated helper
`gdk_wayland_set_input_region_if_empty()` (`:1524`) that is never reached.

### 1.4 Click-through on GNOME/Mutter is broken — a compositor bug

The most subtle failure. GDK3's Wayland backend *does* implement input shapes
(`gdk_window_wayland_input_shape_combine_region`, `:3900` sets
`input_region_dirty = TRUE`), applied by `gdk_wayland_window_sync_input_region()`
(`:1500`) via `wl_surface_set_input_region()`. But that sync is invoked **only
from `end_paint`** (`:1099`).

Three concrete failure modes:

**(a) Input region lands only on a frame commit.** If the window is idle and not
repainting, `wl_surface_set_input_region` is never sent. This is exactly what
Electron diagnosed in `electron/electron#51808` (2026-05-30), root-causing
Chromium 150:
> *"`WaylandSurface::set_input_region()` only forwards directly to
> `wl_surface_set_input_region` when `apply_state_immediately_` is true. Otherwise
> it stores the region in `pending_state_`… `ApplyPendingState()` is invoked
> solely from `WaylandFrameManager` on frame submission. With no GL surface and
> no visual change, the compositor may skip the frame and the input region change
> never lands."*

**(b) Mutter does not reliably send `wl_pointer.leave`.** From the same issue,
verified on GNOME Shell 46 / Mutter / Chromium 150:
> *"Chromium does set and commit an empty input region, but Mutter may fail to
> send `wl_pointer.leave` if the pointer is already inside. In this case, mouse
> events (motion, button) continue to be delivered to the overlay for several
> seconds or **indefinitely** — even when the client is committing the empty
> region continuously at ~60 Hz via animation."*
>
> Cross-compositor, same build: *"under KWin 5.27 consistently receives
> `wl_pointer.leave` within **30–290 ms** after each empty-region commit, and
> click pass-through succeeds reliably (**3/3 attempts**)."*
>
> Reproduced **170×** in one capture with two identical toggles producing
> opposite results.

**For us this is fatal on GNOME.** A user moving the mouse onto the orb and
clicking may hit NEXUS instead of the app underneath, with no code change on our
side to fix it.

**(c) The two requirements conflict.** Because the input region only lands on a
frame commit, **throttling the orb's animation to save CPU can break
click-through.** A continuously-animating orb may appear to work; a
CPU-optimised static one may not. This is a direct design tension, not a bug to
be patched later.

### 1.5 Shaped windows are impossible

`gdk_wayland_window_shape_combine_region()` at `gdkwindow-wayland.c:3873-3879`
is also empty. Compare X11, where the SHAPE extension works fully. So any
non-rectangular orb silhouette is unavailable on Wayland.

### 1.6 Fullscreen transparent is expensive

| Cost | Evidence |
|---|---|
| Full-screen alpha blend every frame | 2560×1440 @60fps ≈ 221 Mpixel/s for a window that is 99.5% empty |
| Keeps the dGPU awake | mutter#2969 — *"gnome-shell keeps the dGPU open and awake, preventing power savings"* |
| CPU collapse on software rendering | GNOME/mutter dev blog 2026-04-01: *"Animations… will also be automatically disabled when the backend reports no hardware-accelerated rendering **for performance purposes**"* |
| Catastrophic on llvmpipe | Emmanuele Bassi, GNOME Discourse 2024-04: gnome-shell at **100-150% CPU** on a 2-core VM; *"even the animated 'Resources' view… was enough to keep CPU at >20%"* |
| Evidenced in our own stack | wry#1827 (2026-08-31) *"feat(webkitgtk): hint at compositing env vars when software rendering is detected"* |
| Hyprland/Sway | Disable blur and drop-shadow or FPS suffers; fractional scaling is *"experimental at best"* and breaks sized always-on-top windows |

Compositor-specific notes:
- **GNOME/Mutter** — won't send `wl_pointer.leave`; blocks the Activities
  overview gesture in some versions.
- **KWin 6** — bug 474039 (cursor shapes on Wayland); handles input regions
  reliably.
- **Sway/Hyprland** — no blur, no shadow, be careful with fractional scaling.

---

## 2. The sanctioned escape, and why it does not save us

`zwlr_layer_shell_v1` is the intended mechanism for always-on-top panels. Four
independent blockers:

1. **Tauri does not expose it.** `tauri-apps/tauri#15913` — "[feat] Support
   wayland's layer shell initialization in Tauri via the Tao builder", opened
   **2026-08-25**, still open. The reporter has a working demo and states:
   > *"GTK provides support for this protocol but critically it requires
   > initialisation before the window is first realised… the way Tauri and Tao
   > work means that **there's currently no convenient gap in the
   > builder-based pipeline for this to occur.**"*

   The only documented workaround is *"transplant the WebView instance out of a
   tauri-owned window and into an unmanaged GTK window of their own creation"* —
   which *"causes its own problems. It's noted to cause **ghosting issues**. It
   also sacrifices various Tauri features."*

   Companion PRs open: `tao#1316`, `tauri#15914`. Also open: `tao#925` ("Access
   GTK window before it's mapped"), `tauri#14277` — whose author notes the
   existing window-creation hook *"necessarily fires too late."*

2. **Requires GTK4.** `gtk4-layer-shell` needs GTK4. Tauri v2 on Linux is GTK3
   (`webkit2gtk-4.1`, `gtk v0.18.2` bindings). The GTK3 `gtk-layer-shell` is in
   **maintenance mode**: *"This project is complete and in maintenance mode…
   New projects are recommended to use GTK4 instead."*

   GTK4 migration in Tauri is **in progress, not done**: `tauri#14684`
   (in progress), `wry#1767` (open since 2026-07-14), `tao#1104`. Assume GTK3
   for the v2 line.

3. **GNOME does not support layer-shell.** `mutter#1922`, open since 2021. Per
   `gtk4-layer-shell`'s own README: *"Layer shell is not supported on:
   Gnome-on-Wayland"*. Supported: Sway, Hyprland, river, Smithay/COSMIC, Mir,
   KDE Plasma on Wayland.

4. **It is the wrong tool anyway.** SDL maintainers:
   > *"Using layer-shell to implement always-on-top would be a huge abuse of the
   > protocol and would be insanely broken when it comes to window management."*
   (libsdl-org/SDL#5779)

Real-world confirmation it's hard: `arlenos/desktop-shell#11` (Lunaris) planned a
Tauri top bar and wrote: *"Plan for Option A first; **if blocked, escalate to
Option B**"* — Option B being a standalone Rust window with raw Wayland +
WebKitGTK, abandoning Tauri for that window.

### GTK4 removed `set_keep_above` too

A GTK developer on GNOME Discourse (2025-09):
> *"In gtk3 you could use the `set_keep_above` method… but that seems to have been
> removed from GTK4. It was implemented using property hints which are just that:
> hints that the window manager may or may not implement. So nothing has really
> changed there from the GTK side. **You can think of Wayland as being more
> explicit about not supporting these hints.**"*

### What Qt and Electron do

- **Qt** — nothing. `Qt::WindowStaysOnTopHint` is an X11 hint.
- **Electron** — went Wayland-native in 38.2 (2026-03-17) and only implemented
  `setIgnoreMouseEvents` on Wayland in **2026** (PR #51769). Half a decade. Still
  unreliable on Mutter (#51808). **No positioning API either.**
- **GNOME maintainers' own answer** — asked directly "any way, even hacky",
  Emmanuele Bassi answered: *"If targeting GNOME, the most effective way to do
  it would probably be to **make a GNOME extension** that moves the window when
  it appears and set the above flag."*

---

## 3. Recommended architecture

### Option 1 (RECOMMENDED) — small window + tray, no overlay

**What every supported platform gets:** a ~120×120 transparent, undecorated,
non-resizable, always-present window, plus a tray icon as the reliable control
surface. Your existing `dyn_windows.rs` lazy-create/destroy machinery and tray
code are already the right foundation.

| Capability | X11 | Wayland |
|---|---|---|
| `always_on_top` | ✅ works | ❌ no-op — do not pretend |
| `set_position` | ✅ works | ❌ impossible |
| Click-through | ✅ X11 SHAPE ext | ⚠️ KWin/Sway yes; **GNOME unreliable** |
| Shaped window | ✅ | ❌ |
| Tray icon | ✅ | ✅ StatusNotifier (ksni) |
| `decorations: false` | ✅ | ✅ fixed in tao 0.36.0 (2026-07-29) |

**Cost:** we lose arbitrary positioning on Wayland. **Accept it.** The user gets
the orb where GNOME put it; on KWin/Sway they can hint placement via compositor
config. Ship the config snippets.

**Why this is strictly better:** blended pixels drop from O(screen area) to
O(orb area) — roughly a **200-400× reduction** on a 1440p display. It removes the
GPU-awake problem, the llvmpipe problem, and the click-through panic simultaneously.

### Option 2 — compositor-specific hints, no code changes

Because placement must be compositor-determined, hand the user the config:

```ini
# Sway
for_window [app_id="com.nexus.assistant"] floating enable
for_window [app_id="com.nexus.assistant"] move position 1920 100

# Hyprland
windowrule {
  app_id = com.nexus.assistant
  float, ontop
  move 1920 100
}
```

| Tool | Move? | Notes |
|---|---|---|
| `swaymsg` | ✅ `move position<x> <y>` | Sway only |
| `hyprctl` | ✅ `dispatch movewindow <x> <y>` | Hyprland 0.56.2 |
| `wlrctl` | ❌ **no move command** | minimize/maximize/fullscreen/focus/find/wait/waitfor only |
| KWin 6 scripting | ✅ | QML API can set a window's layer — **real always-on-top on KDE** |
| `wtype` | n/a | synthetic keyboard input only |

### Option 3 — GNOME Shell extension, for GNOME only

If floating above everything on GNOME is non-negotiable, **the compositor must
do it.** Shipping extensions already do this:

| Extension | Downloads | Notes |
|---|---|---|
| `window-on-top-float` | — | GNOME 45-49, floating circular button, proximity reveal, persistent visibility for pinned windows, light/dark adaptation |
| `window-on-top` | 10,781 | trivial always-on-top toggle |
| `Floating Mini Panel` | 9,637 | movable always-on-top mini panel |
| `panel-floater` | — | GNOME 45-50, animated rounded corners; handles GNOME forcibly resetting panel styles on workspace switch |

An extension can also **drive our app**: read the window id (exactly what
`tauri#14913` asks for), call `Meta.Window.move_resize_frame()`, set
`Meta.WindowActor` above, and signal the Rust side over D-Bus.

**Cost:** per-DE maintenance, and we don't control placement.

### Option 4 — layer-shell panel (wlroots/KDE only)

Only if we need a docked panel. Requires a Tauri patch (tao#1316 / tauri#15913)
and doesn't work on GNOME. **Do not use it for the floating orb.**

---

## 4. Animation performance

### The rAF/click-through coupling

Because the input region only lands on a frame commit (§1.4a), **rAF throttling
and Wayland click-through are in direct conflict.** Decide deliberately; they
cannot be optimized independently.

### Page Visibility API is unreliable here

`document.hidden` tracks the *tab*, but wry puts the `WebKitWebView` inside a
GTK container as the only page — there are no tabs to background. It will likely
only flip on hide/unrealize, **not on occlusion**. Do not rely on it for occlusion
detection on Linux; test before depending on it.

### WebKitGTK rAF throttling is aggressive

WebKit's `ScriptedAnimationController` (changeset 261113) applies two throttles:

| Mechanism | Effect |
|---|---|
| Page throttling | `VisuallyIdle` (aggressive), `LowPowerMode` (half speed) |
| Document throttling | **`OutsideViewport`: drops to one callback per 10 seconds** |

WebKit bug 178396 documents over-aggressive `OutsideViewport` throttling from
clip-rect miscalculation; workaround is a full-viewport filler element.

### Recommendations

1. **Cap frame rate explicitly at ~20-30 fps.** Don't rely on 60fps rAF. A
   subtle glow at 30fps is indistinguishable and halves GPU cost.
2. **Pause on our own state machine**, not on `document.hidden`. Stop the Lottie
   renderer entirely at rest (`stop()` + drop the rAF loop).
3. **No `blur()`, no `box-shadow`, no `backdrop-filter`.** Expensive *and*
   `backdrop-filter` is a documented no-op in a transparent WebKit view (already
   established in our `AGENTS.md`).
4. **If click-through is still needed on Wayland, keep a ~1fps heartbeat frame**
   and comment why.
5. **Occlusion detection does not exist.** No WebKitGTK, Tauri, or Wayland API.
   On Wayland a client *cannot* know what covers it (w3c#68). Accept that a
   covered orb burns a few percent.

---

## 5. Tauri version status (2026-10-02)

| Crate | Latest stable | Date | Newest incl. prerelease |
|---|---|---|---|
| `tauri` | **2.12.1** | 2026-09-30 | `3.0.0-alpha.4` (2026-10-01) |
| `tauri-utils` | 2.10.1 | 2026-09-30 | `3.0.0-alpha.3` |
| `tauri-build` | 2.7.1 | 2026-09-30 | `3.0.0-alpha.3` |
| `tauri-runtime-wry` | **2.12.1** | 2026-09-30 | `3.0.0-alpha.4` |
| `tao` | **0.37.1** | 2026-09-26 | — |
| `wry` | **0.57.0** | 2026-09-08 | — |
| `@tauri-apps/api` | 2.12.0 | 2026-09-26 | — |
| `@tauri-apps/cli` | 2.11.4 | 2026-06-28 | — |

**Cadence:** ~2 minor releases per quarter (2.9.0 Oct 2025 → 2.10.0 Feb 2026 →
2.11.0 Apr 2026 → 2.11.5 Jul 2026 → 2.12.1 Sep 30 2026).

### Notable 2.12 changes

- **MSRV raised to 1.90**; Windows 7 dropped; policy is `stable - 3`.
- `tao 0.36.0` (2026-07-29) fixed a batch of Wayland decoration bugs — commit
  `07f3742b` closed tao#899, tao#1046, tauri#6562, tauri#13440, tauri#13749,
  tauri#14251, tauri#14748. Undecorated Wayland windows are much better behaved.
- `tao 0.36.0` commit `4d5005ff` (#1173) fixed **thread-safety** on
  `set_min_inner_size`, `set_max_inner_size`, `set_inner_size_constraints`,
  `set_fullscreen`, `set_theme` — *"were not properly thread-safe on Linux."* If
  we call window mutators off the main thread, that was a live bug until
  2026-07-29.

### Tauri 3.0.0-alpha.4 shipped 2026-10-01

Breaking change, verbatim from the changelog:
> *"Removed the `macos-private-api` Cargo feature and the
> `app > macOSPrivateApi` configuration option. Window transparency and the
> `fullScreenEnabled` preference no longer rely on macOS private APIs, so they
> are always available."*

Relevant only if macOS returns to scope (it removes an App Store-hostile
distribution blocker). The alpha also ships `gtk3`/`gtk4` Cargo feature flags,
but **the GTK4/WebKitGTK 6.0 webview port is unfinished** (`wry#1767` open).
**Stay on 2.12.1.**

### macOS click-through gap

Tauri has **no `forward` option** for `set_ignore_cursor_events` — Electron has
`{forward: true}`; Tauri does not. So on macOS you cannot both ignore *and*
observe mouse events. `tauri#6164` opened **2023-01-28**, reopened, last activity
2026-04-17. Unresolved discussion: `tauri-apps/discussions#11507` (2026-03-30).

---

## 6. Action list

1. **Replace the fullscreen overlay with a ~120×120 window + tray.** Unblocks Linux.
2. **Gate all Wayland-unsupported calls behind an `XDG_SESSION_TYPE` check.** One
   codebase, two honest capability sets. The current failure mode is treating them
   as identical.
3. **Guard the panic:** never call `set_ignore_cursor_events` before realization;
   use `.ok()`. Better: stop calling it on Linux at all.
4. **Fix the two lying settings** — Orb Position and Orb Size currently do
   nothing on Linux. Wire them, or remove them and say why in the UI.
5. **Ship compositor config snippets** for Sway / Hyprland / KWin.
6. **Investigate KWin 6 window scripting** — it may give real always-on-top on
   KDE Wayland, the one desktop where that is achievable.
7. **Only if GNOME floating is non-negotiable:** ship a companion GNOME Shell
   extension and drive it over D-Bus. Budget for per-DE maintenance.
8. **Track `tauri#15913` / `tao#1316`** — if they land, Tauri gains a first-class
   layer-shell builder property. Do not depend on it now.
9. **Animation:** 30fps cap, `stop()` at rest, no blur/shadow/backdrop-filter,
   heartbeat frame only if click-through is still required.