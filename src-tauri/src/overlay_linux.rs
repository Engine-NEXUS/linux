//! Linux stage overlay via layer-shell (Wayland) or non-focusable notification window (X11).
//!
//! Under native Wayland on a compositor supporting `zwlr_layer_shell_v1`
//! (COSMIC, KDE Plasma, Hyprland, Sway, river, niri, Wayfire, labwc) and with
//! `libgtk-layer-shell.so.0` installed, the stage is converted into an overlay
//! surface anchored to all four edges on the overlay layer, with keyboard
//! interactivity set to NONE so it never appears as a window, never steals
//! focus, and never intercepts keystrokes.
//!
//! Click-through is governed by the Wayland input region: empty when idle (all clicks
//! pass through directly), and shaped to the exact union of active hitbox rects when
//! interactive elements (such as the orb) are present.
//!
//! On compositors without layer-shell (e.g. GNOME Mutter or pure X11), or when
//! `libgtk-layer-shell.so.0` is missing, the fallback path keeps XWayland/X11
//! but marks the window non-focusable (`accept_focus: false`, `focus_on_map: false`),
//! with `Notification` window type hint and input shape union.

use std::cell::RefCell;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;

// Cairo & GTK / GDK types
use cairo::{RectangleInt, Region};
use glib::translate::ToGlibPtr;
use gtk::prelude::*;

use crate::stage::StageRect;

/// Target layer: overlay (above normal windows, panels, and full-screen apps).
const GTK_LAYER_SHELL_LAYER_OVERLAY: i32 = 3;
/// The sidebar drawer's layer: above normal windows, below the overlay.
const GTK_LAYER_SHELL_LAYER_TOP: i32 = 2;

/// Edge anchor indices for gtk-layer-shell.
const GTK_LAYER_SHELL_EDGE_LEFT: i32 = 0;
const GTK_LAYER_SHELL_EDGE_RIGHT: i32 = 1;
const GTK_LAYER_SHELL_EDGE_TOP: i32 = 2;
const GTK_LAYER_SHELL_EDGE_BOTTOM: i32 = 3;

/// Keyboard interactivity mode for gtk-layer-shell (0 = NONE).
const GTK_LAYER_SHELL_KEYBOARD_MODE_NONE: i32 = 0;
/// The surface takes the keyboard when the user clicks into it (the drawer).
const GTK_LAYER_SHELL_KEYBOARD_MODE_ON_DEMAND: i32 = 2;

/// Reason why the fallback path was chosen instead of layer-shell.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FallbackReason {
    NotWaylandBackend,
    LibraryMissing,
    CompositorUnsupported,
}

/// The overlay path chosen for the stage window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverlayPath {
    LayerShell,
    Fallback(FallbackReason),
}

/// Dynamic function bindings for `libgtk-layer-shell.so.0`.
pub struct LayerShellBindings {
    pub is_supported: unsafe extern "C" fn() -> i32,
    pub init_for_window: unsafe extern "C" fn(*mut gtk::ffi::GtkWindow),
    pub set_layer: unsafe extern "C" fn(*mut gtk::ffi::GtkWindow, i32),
    pub set_anchor: unsafe extern "C" fn(*mut gtk::ffi::GtkWindow, i32, i32),
    pub set_exclusive_zone: unsafe extern "C" fn(*mut gtk::ffi::GtkWindow, i32),
    pub set_namespace: unsafe extern "C" fn(*mut gtk::ffi::GtkWindow, *const std::ffi::c_char),
    pub set_keyboard_mode: Option<unsafe extern "C" fn(*mut gtk::ffi::GtkWindow, i32)>,
    pub set_keyboard_interactivity: Option<unsafe extern "C" fn(*mut gtk::ffi::GtkWindow, i32)>,
    pub set_monitor: Option<unsafe extern "C" fn(*mut gtk::ffi::GtkWindow, *mut gdk::ffi::GdkMonitor)>,
    pub set_margin: Option<unsafe extern "C" fn(*mut gtk::ffi::GtkWindow, i32, i32)>,
}

/// Loaded `libgtk-layer-shell.so.0` handle and function pointers.
struct LayerShellLib {
    _lib: libloading::Library,
    bindings: LayerShellBindings,
}

impl LayerShellLib {
    fn load() -> Result<Self, libloading::Error> {
        let lib = unsafe {
            libloading::Library::new("libgtk-layer-shell.so.0")
                .or_else(|_| libloading::Library::new("libgtk-layer-shell.so"))?
        };
        unsafe {
            let is_supported = *lib.get::<unsafe extern "C" fn() -> i32>(b"gtk_layer_is_supported\0")?;
            let init_for_window = *lib.get::<unsafe extern "C" fn(*mut gtk::ffi::GtkWindow)>(b"gtk_layer_init_for_window\0")?;
            let set_layer = *lib.get::<unsafe extern "C" fn(*mut gtk::ffi::GtkWindow, i32)>(b"gtk_layer_set_layer\0")?;
            let set_anchor = *lib.get::<unsafe extern "C" fn(*mut gtk::ffi::GtkWindow, i32, i32)>(b"gtk_layer_set_anchor\0")?;
            let set_exclusive_zone = *lib.get::<unsafe extern "C" fn(*mut gtk::ffi::GtkWindow, i32)>(b"gtk_layer_set_exclusive_zone\0")?;
            let set_namespace = *lib.get::<unsafe extern "C" fn(*mut gtk::ffi::GtkWindow, *const std::ffi::c_char)>(b"gtk_layer_set_namespace\0")?;
            let set_keyboard_mode = lib.get::<unsafe extern "C" fn(*mut gtk::ffi::GtkWindow, i32)>(b"gtk_layer_set_keyboard_mode\0").ok().map(|s| *s);
            let set_keyboard_interactivity = lib.get::<unsafe extern "C" fn(*mut gtk::ffi::GtkWindow, i32)>(b"gtk_layer_set_keyboard_interactivity\0").ok().map(|s| *s);
            let set_monitor = lib.get::<unsafe extern "C" fn(*mut gtk::ffi::GtkWindow, *mut gdk::ffi::GdkMonitor)>(b"gtk_layer_set_monitor\0").ok().map(|s| *s);
            let set_margin = lib.get::<unsafe extern "C" fn(*mut gtk::ffi::GtkWindow, i32, i32)>(b"gtk_layer_set_margin\0").ok().map(|s| *s);

            Ok(Self {
                _lib: lib,
                bindings: LayerShellBindings {
                    is_supported,
                    init_for_window,
                    set_layer,
                    set_anchor,
                    set_exclusive_zone,
                    set_namespace,
                    set_keyboard_mode,
                    set_keyboard_interactivity,
                    set_monitor,
                    set_margin,
                },
            })
        }
    }
}

static LAYER_SHELL_LIB: OnceLock<Option<LayerShellLib>> = OnceLock::new();

fn get_layer_shell_lib() -> Option<&'static LayerShellBindings> {
    LAYER_SHELL_LIB
        .get_or_init(|| LayerShellLib::load().ok())
        .as_ref()
        .map(|l| &l.bindings)
}

/// Pure decision function: determines whether the stage should use layer-shell or fallback.
pub fn decide_overlay_path(
    is_wayland_backend: bool,
    library_loaded: bool,
    compositor_supported: bool,
) -> OverlayPath {
    if !is_wayland_backend {
        return OverlayPath::Fallback(FallbackReason::NotWaylandBackend);
    }
    if !library_loaded {
        return OverlayPath::Fallback(FallbackReason::LibraryMissing);
    }
    if !compositor_supported {
        return OverlayPath::Fallback(FallbackReason::CompositorUnsupported);
    }
    OverlayPath::LayerShell
}

/// Check if GDK is running on the native Wayland backend.
pub fn is_gdk_wayland() -> bool {
    if let Some(display) = gdk::Display::default() {
        use glib::object::ObjectExt;
        if display.type_().name() == "GdkWaylandDisplay" {
            return true;
        }
    }
    std::env::var("GDK_BACKEND")
        .map(|v| v.trim().eq_ignore_ascii_case("wayland"))
        .unwrap_or(false)
}

/// Resolve the active overlay path by querying the runtime environment and library state.
pub fn resolve_overlay_path() -> OverlayPath {
    let is_wayland = is_gdk_wayland();
    let lib_res = get_layer_shell_lib();
    let (lib_loaded, comp_supported) = match lib_res {
        Some(lib) => {
            let supported = unsafe { (lib.is_supported)() != 0 };
            (true, supported)
        }
        None => (false, false),
    };
    decide_overlay_path(is_wayland, lib_loaded, comp_supported)
}

/// Pure geometry: convert physical hitbox rects into a cairo `Region` in logical window pixels.
///
/// Scale factor converts physical coordinates (as sent from frontend physical pixels)
/// to logical GTK / Cairo coordinates. Overlapping rectangles are automatically merged
/// into disjoint rectangles by Cairo's regional arithmetic.
pub fn build_cairo_region(hitboxes: &[StageRect], scale_factor: f64) -> Region {
    let region = Region::create();
    let scale = if scale_factor > 0.0 { scale_factor } else { 1.0 };
    for r in hitboxes {
        let x = (r.x as f64 / scale).floor() as i32;
        let y = (r.y as f64 / scale).floor() as i32;
        let width = ((r.w as f64 / scale).ceil() as i32).max(0);
        let height = ((r.h as f64 / scale).ceil() as i32).max(0);
        if width > 0 && height > 0 {
            let rect = RectangleInt::new(x, y, width, height);
            let _ = region.union_rectangle(&rect);
        }
    }
    region
}

/// Pure helper for unit testing: compute the list of disjoint `RectangleInt` in the region.
pub fn compute_input_rects(hitboxes: &[StageRect], scale_factor: f64) -> Vec<RectangleInt> {
    let region = build_cairo_region(hitboxes, scale_factor);
    let count = region.num_rectangles();
    let mut rects = Vec::with_capacity(count as usize);
    for i in 0..count {
        rects.push(region.rectangle(i));
    }
    rects
}

// Main-thread-only weak reference to the stage GtkWindow.
// Upgrade() safely returns None after the window has been destroyed.
thread_local! {
    static STAGE: RefCell<Option<glib::WeakRef<gtk::Window>>> = RefCell::new(None);
}

static LAYER_SHELL_ACTIVE: AtomicBool = AtomicBool::new(false);
static LOGGED_ONCE: std::sync::Once = std::sync::Once::new();

/// Whether the stage overlay is actively managed by gtk-layer-shell.
pub fn is_layer_shell_active() -> bool {
    LAYER_SHELL_ACTIVE.load(Ordering::SeqCst)
}

/// Inform overlay_linux that the stage window has been destroyed.
///
/// `STAGE` is deliberately left alone: its weak reference stops upgrading by
/// itself once the window is gone, and clearing it from a deferred callback
/// could land AFTER a rebuilt stage registered itself, leaving the new stage
/// without its input region (a full-window click catcher).
pub fn note_stage_destroyed() {
    LAYER_SHELL_ACTIVE.store(false, Ordering::SeqCst);
}

/// Initialize the stage overlay window right after creation.
/// Dispatches work to the GTK main thread before the window is realized or mapped.
pub fn init_stage<R: tauri::Runtime>(win: &tauri::WebviewWindow<R>) {
    let win_clone = win.clone();
    let _ = win.run_on_main_thread(move || {
        init_stage_on_main_thread(&win_clone);
    });
}

fn init_stage_on_main_thread<R: tauri::Runtime>(win: &tauri::WebviewWindow<R>) {
    let gtk_win = match win.gtk_window() {
        Ok(w) => w,
        Err(e) => {
            tracing::error!("overlay_linux: gtk_window() unavailable: {e}");
            return;
        }
    };

    let raw_ptr: *mut gtk::ffi::GtkWindow = gtk_win.upcast_ref::<gtk::Window>().to_glib_none().0;

    STAGE.with(|cell| {
        *cell.borrow_mut() = Some(gtk_win.upcast_ref::<gtk::Window>().downgrade());
    });

    let path = resolve_overlay_path();
    LOGGED_ONCE.call_once(|| match path {
        OverlayPath::LayerShell => {
            tracing::info!("overlay_linux: stage overlay running on LayerShell (overlay layer, non-focusable)");
        }
        OverlayPath::Fallback(reason) => {
            tracing::warn!("overlay_linux: stage overlay running on fallback path ({reason:?})");
        }
    });

    match path {
        OverlayPath::LayerShell => {
            // Unrealize if Tao or WebKit already realized it
            if gtk_win.is_realized() {
                gtk_win.unrealize();
            }
            // A non-resizable GtkWindow keeps its default size and ignores the
            // compositor's configure, so the anchored surface could never take
            // the size the compositor gives it. A layer surface cannot be
            // resized by the user anyway.
            gtk_win.set_resizable(true);

            if let Some(lib) = get_layer_shell_lib() {
                unsafe {
                    (lib.init_for_window)(raw_ptr);
                    let ns = std::ffi::CStr::from_bytes_with_nul(b"nexus\0").unwrap();
                    (lib.set_namespace)(raw_ptr, ns.as_ptr());
                    (lib.set_layer)(raw_ptr, GTK_LAYER_SHELL_LAYER_OVERLAY);
                    (lib.set_anchor)(raw_ptr, GTK_LAYER_SHELL_EDGE_LEFT, 1);
                    (lib.set_anchor)(raw_ptr, GTK_LAYER_SHELL_EDGE_RIGHT, 1);
                    (lib.set_anchor)(raw_ptr, GTK_LAYER_SHELL_EDGE_TOP, 1);
                    (lib.set_anchor)(raw_ptr, GTK_LAYER_SHELL_EDGE_BOTTOM, 1);
                    // 0 first: the first configure then reports the area the
                    // panels leave free (see `measure_safe_area`), after which
                    // the stage goes full screen (-1) so screen coordinates
                    // stay exact for the pointer, the tour and the pins.
                    (lib.set_exclusive_zone)(raw_ptr, 0);
                    if let Some(set_km) = lib.set_keyboard_mode {
                        (set_km)(raw_ptr, GTK_LAYER_SHELL_KEYBOARD_MODE_NONE);
                    } else if let Some(set_ki) = lib.set_keyboard_interactivity {
                        (set_ki)(raw_ptr, 0);
                    }
                    set_primary_monitor(lib, raw_ptr);
                }
                LAYER_SHELL_ACTIVE.store(true, Ordering::SeqCst);

                // Measure the panels once per stage window, shortly after its
                // first map (the compositor's configure has arrived by then).
                let measured = std::rc::Rc::new(std::cell::Cell::new(false));
                let win_for_measure = win.clone();
                gtk_win.connect_map(move |w| {
                    if measured.replace(true) {
                        return;
                    }
                    let weak = w.downgrade();
                    let win = win_for_measure.clone();
                    glib::timeout_add_local_once(std::time::Duration::from_millis(700), move || {
                        if let Some(w) = weak.upgrade() {
                            measure_safe_area(w.upcast_ref::<gtk::Window>(), &win);
                        }
                    });
                });
            }

            gtk_win.set_accept_focus(false);
            gtk_win.set_focus_on_map(false);
        }
        OverlayPath::Fallback(_reason) => {
            LAYER_SHELL_ACTIVE.store(false, Ordering::SeqCst);
            gtk_win.set_accept_focus(false);
            gtk_win.set_focus_on_map(false);
            gtk_win.set_type_hint(gdk::WindowTypeHint::Notification);
            gtk_win.set_skip_taskbar_hint(true);
            gtk_win.set_skip_pager_hint(true);
            gtk_win.set_keep_above(true);
        }
    }

    // When the window is mapped/realized, apply the current input region
    gtk_win.connect_map(|_| {
        apply_input_region();
    });

    // Apply the initial input region (empty if idle)
    apply_input_region_on_main_thread();
}

/// Apply the union of all active hitboxes to the stage window's input shape region.
///
/// Thread-safe: dispatches to the GTK main loop via `glib::idle_add_once`.
pub fn apply_input_region() {
    glib::idle_add_once(move || {
        apply_input_region_on_main_thread();
    });
}

fn apply_input_region_on_main_thread() {
    let opt_win = STAGE.with(|cell| {
        cell.borrow().as_ref().and_then(|weak| weak.upgrade())
    });

    let Some(gtk_win) = opt_win else {
        return;
    };

    if let Some(gdk_win) = gtk_win.window() {
        let scale = gtk_win.scale_factor() as f64;
        let hitboxes = crate::stage::all_hitboxes();
        let region = build_cairo_region(&hitboxes, scale);
        gdk_win.input_shape_combine_region(&region, 0, 0);
        tracing::debug!(
            "overlay_linux: applied input region ({} rect(s), scale {})",
            region.num_rectangles(),
            scale
        );
    }
}

/// Pin a layer surface to the primary monitor (the one Rust's geometry uses).
unsafe fn set_primary_monitor(lib: &LayerShellBindings, raw_ptr: *mut gtk::ffi::GtkWindow) {
    if let Some(set_mon) = lib.set_monitor {
        if let Some(display) = gdk::Display::default() {
            let monitor = display.primary_monitor().or_else(|| display.monitor(0));
            if let Some(m) = monitor {
                let m_ptr: *mut gdk::ffi::GdkMonitor = m.to_glib_none().0;
                (set_mon)(raw_ptr, m_ptr);
            }
        }
    }
}

fn primary_monitor_size_logical() -> Option<(i32, i32)> {
    let display = gdk::Display::default()?;
    let m = display.primary_monitor().or_else(|| display.monitor(0))?;
    let g = m.geometry();
    Some((g.width(), g.height()))
}

// ─── Safe area: where the panels are (the orb sits inside it) ────────────────

/// Space the desktop's panels take on each edge, in logical pixels.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Insets {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

/// Which edges carry a panel that reserves space, when the desktop says so.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PanelEdges {
    /// The edges below come from the desktop's own config.
    pub known: bool,
    pub left: bool,
    pub top: bool,
    pub right: bool,
    pub bottom: bool,
}

static SAFE_INSETS: once_cell::sync::Lazy<parking_lot::Mutex<Insets>> =
    once_cell::sync::Lazy::new(|| parking_lot::Mutex::new(Insets::default()));

/// The measured panel insets in PHYSICAL pixels, or zero when the stage is
/// not a layer surface (the X11 window is placed below the panels by the
/// window manager itself).
pub fn safe_insets_physical(scale: f64) -> Insets {
    if !is_layer_shell_active() {
        return Insets::default();
    }
    let i = *SAFE_INSETS.lock();
    let s = |v: i32| (v as f64 * scale).round() as i32;
    Insets { left: s(i.left), top: s(i.top), right: s(i.right), bottom: s(i.bottom) }
}

/// The measured panel insets in logical pixels (zero until measured).
pub fn safe_insets_logical() -> Insets {
    *SAFE_INSETS.lock()
}

/// Pure: share the measured free-area shortfall between the edges.
///
/// `total_x` / `total_y` are monitor size minus the area the panels leave
/// free. When the desktop config names exactly one panel edge on an axis,
/// that edge gets all of it; otherwise (both edges, or a panel the config
/// does not explain) each edge on that axis gets the whole amount, which
/// keeps the orb clear of either one at the cost of a small gap.
pub fn attribute_insets(total_x: i32, total_y: i32, edges: PanelEdges) -> Insets {
    let split = |total: i32, a: bool, b: bool| -> (i32, i32) {
        if total <= 0 {
            return (0, 0);
        }
        match (edges.known, a, b) {
            (true, true, false) => (total, 0),
            (true, false, true) => (0, total),
            _ => (total, total),
        }
    };
    let (top, bottom) = split(total_y, edges.top, edges.bottom);
    let (left, right) = split(total_x, edges.left, edges.right);
    Insets { left, top, right, bottom }
}

/// Pure: the quoted names in COSMIC's panel `entries` file (`["Panel", "Dock"]`).
pub fn parse_cosmic_entries(text: &str) -> Vec<String> {
    text.split('"')
        .enumerate()
        .filter(|(i, _)| i % 2 == 1)
        .map(|(_, s)| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

/// Pure: which edges reserve space, from each panel's (`anchor`, `exclusive_zone`) files.
pub fn cosmic_edges_from(panels: &[(String, String)]) -> PanelEdges {
    let mut e = PanelEdges { known: true, ..PanelEdges::default() };
    for (anchor, exclusive) in panels {
        if exclusive.trim() != "true" {
            continue;
        }
        match anchor.trim() {
            "Top" => e.top = true,
            "Bottom" => e.bottom = true,
            "Left" => e.left = true,
            "Right" => e.right = true,
            _ => {}
        }
    }
    e
}

/// COSMIC's panel layout from its config files; `known: false` elsewhere.
fn desktop_panel_edges() -> PanelEdges {
    let on_cosmic = std::env::var("XDG_CURRENT_DESKTOP")
        .map(|d| d.to_ascii_lowercase().contains("cosmic"))
        .unwrap_or(false);
    if !on_cosmic {
        return PanelEdges::default();
    }
    let base = std::env::var("XDG_CONFIG_HOME")
        .ok()
        .map(std::path::PathBuf::from)
        .or_else(|| dirs_next::home_dir().map(|h| h.join(".config")));
    let Some(root) = base.map(|b| b.join("cosmic")) else {
        return PanelEdges::default();
    };
    let Ok(entries) = std::fs::read_to_string(root.join("com.system76.CosmicPanel/v1/entries")) else {
        return PanelEdges::default();
    };
    let panels: Vec<(String, String)> = parse_cosmic_entries(&entries)
        .iter()
        .map(|name| {
            let dir = root.join(format!("com.system76.CosmicPanel.{name}/v1"));
            (
                std::fs::read_to_string(dir.join("anchor")).unwrap_or_default(),
                std::fs::read_to_string(dir.join("exclusive_zone")).unwrap_or_default(),
            )
        })
        .collect();
    cosmic_edges_from(&panels)
}

/// Runs once per stage window, on the main thread, after its first map: the
/// surface was created with exclusive zone 0, so its allocation is the area
/// the panels leave free. Store the insets, go full screen, re-place the orb.
fn measure_safe_area<R: tauri::Runtime>(gtk_win: &gtk::Window, win: &tauri::WebviewWindow<R>) {
    let Some((mw, mh)) = primary_monitor_size_logical() else {
        return;
    };
    let (w, h) = (gtk_win.allocated_width(), gtk_win.allocated_height());
    let edges = desktop_panel_edges();
    let insets = attribute_insets(mw - w, mh - h, edges);
    *SAFE_INSETS.lock() = insets;
    tracing::info!(
        "overlay_linux: free area {w}x{h} of {mw}x{mh} — panel insets {:?} (desktop config known: {})",
        insets,
        edges.known
    );
    if let Some(lib) = get_layer_shell_lib() {
        let raw_ptr: *mut gtk::ffi::GtkWindow = gtk_win.to_glib_none().0;
        unsafe { (lib.set_exclusive_zone)(raw_ptr, -1) };
    }
    use tauri::Manager;
    crate::window_manager::emit_orb_rect(win.app_handle());
}

// ─── The sidebar as a system drawer ──────────────────────────────────────────

static DRAWER_ACTIVE: AtomicBool = AtomicBool::new(false);

/// Whether the sidebar is a layer-shell drawer (positioned by margins).
pub fn is_drawer_active() -> bool {
    DRAWER_ACTIVE.load(Ordering::SeqCst)
}

/// Where a drawer goes: anchored to the top and to one side, with margins.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DrawerPlacement {
    /// Anchored to the right edge (else the left one).
    pub right: bool,
    /// Distance from that side edge.
    pub margin_side: i32,
    pub margin_top: i32,
}

/// Pure: turn the sidebar's (x, y, w) inside an area `area_w` wide into
/// layer-shell anchors and margins. A rect right of centre hugs the right
/// edge (so rounding can never push it off screen); anything else is placed
/// from the left, which also covers the centred views.
pub fn drawer_placement(x: f64, y: f64, w: f64, area_w: f64) -> DrawerPlacement {
    let right = x + w / 2.0 > area_w / 2.0;
    let margin_side = if right { area_w - x - w } else { x };
    DrawerPlacement {
        right,
        margin_side: margin_side.round().max(0.0) as i32,
        margin_top: y.round().max(0.0) as i32,
    }
}

/// The area the sidebar can use, in logical pixels: the monitor minus the
/// panels when the sidebar is a drawer (its margins count from the panels),
/// the whole monitor otherwise.
pub fn drawer_area_logical(monitor_w: f64, monitor_h: f64) -> (f64, f64) {
    if !is_drawer_active() {
        return (monitor_w, monitor_h);
    }
    let i = safe_insets_logical();
    (
        (monitor_w - (i.left + i.right) as f64).max(200.0),
        (monitor_h - (i.top + i.bottom) as f64).max(200.0),
    )
}

/// Make the sidebar a drawer before its first map: top layer, never pushes
/// windows aside, takes the keyboard only when clicked into, never in
/// Alt-Tab. Needs the keyboard-mode API (gtk-layer-shell >= 0.6): an older
/// library would only offer an all-or-nothing keyboard grab, so the sidebar
/// then stays a normal window.
pub fn init_drawer<R: tauri::Runtime>(win: &tauri::WebviewWindow<R>) {
    // Decided here, synchronously: `prepare_sidebar` computes the geometry
    // right after the window is created, before the main-thread task below
    // has run. The stage already settled whether layer-shell works.
    let eligible = is_layer_shell_active()
        && get_layer_shell_lib()
            .map(|l| l.set_keyboard_mode.is_some() && l.set_margin.is_some())
            .unwrap_or(false);
    DRAWER_ACTIVE.store(eligible, Ordering::SeqCst);
    if !eligible {
        if is_layer_shell_active() {
            tracing::warn!("overlay_linux: gtk-layer-shell too old for the drawer — sidebar stays a window");
        }
        return;
    }
    let win_clone = win.clone();
    let _ = win.run_on_main_thread(move || {
        let Some(lib) = get_layer_shell_lib() else { return };
        let Some(set_km) = lib.set_keyboard_mode else { return };
        let Ok(gtk_win) = win_clone.gtk_window() else {
            DRAWER_ACTIVE.store(false, Ordering::SeqCst);
            return;
        };
        if gtk_win.is_realized() {
            gtk_win.unrealize();
        }
        let raw_ptr: *mut gtk::ffi::GtkWindow = gtk_win.upcast_ref::<gtk::Window>().to_glib_none().0;
        unsafe {
            (lib.init_for_window)(raw_ptr);
            let ns = std::ffi::CStr::from_bytes_with_nul(b"nexus-sidebar\0").unwrap();
            (lib.set_namespace)(raw_ptr, ns.as_ptr());
            (lib.set_layer)(raw_ptr, GTK_LAYER_SHELL_LAYER_TOP);
            (lib.set_exclusive_zone)(raw_ptr, 0);
            (set_km)(raw_ptr, GTK_LAYER_SHELL_KEYBOARD_MODE_ON_DEMAND);
            set_primary_monitor(lib, raw_ptr);
        }
        tracing::info!("overlay_linux: sidebar is a layer-shell drawer (top layer, keyboard on demand)");
    });
}

/// Place the drawer at (x, y) with size (w, h) inside the area from
/// `drawer_area_logical`. No-op when the sidebar is not a drawer.
///
/// The size is set as the GTK size request: a drawer anchored to one side
/// takes the size its window asks for, and the window is non-resizable, so
/// a plain resize would be ignored.
pub fn place_drawer<R: tauri::Runtime>(win: &tauri::WebviewWindow<R>, x: f64, y: f64, w: f64, h: f64, area_w: f64) {
    if !is_drawer_active() {
        return;
    }
    let placement = drawer_placement(x, y, w, area_w);
    let win_clone = win.clone();
    let _ = win.run_on_main_thread(move || {
        let Some(lib) = get_layer_shell_lib() else { return };
        let Ok(gtk_win) = win_clone.gtk_window() else { return };
        gtk_win.set_size_request(w.round().max(1.0) as i32, h.round().max(1.0) as i32);
        let raw_ptr: *mut gtk::ffi::GtkWindow = gtk_win.upcast_ref::<gtk::Window>().to_glib_none().0;
        apply_drawer_placement(lib, raw_ptr, placement);
        let weak = gtk_win.downgrade();
        glib::timeout_add_local_once(std::time::Duration::from_millis(1500), move || {
            if let Some(g) = weak.upgrade() {
                tracing::debug!(
                    "overlay_linux: drawer asked {:.0}x{:.0} at {:?}, got {}x{}",
                    w, h, placement, g.allocated_width(), g.allocated_height()
                );
            }
        });
    });
}

fn apply_drawer_placement(lib: &LayerShellBindings, raw_ptr: *mut gtk::ffi::GtkWindow, p: DrawerPlacement) {
    let Some(set_margin) = lib.set_margin else { return };
    unsafe {
        (lib.set_anchor)(raw_ptr, GTK_LAYER_SHELL_EDGE_TOP, 1);
        (lib.set_anchor)(raw_ptr, GTK_LAYER_SHELL_EDGE_BOTTOM, 0);
        (lib.set_anchor)(raw_ptr, GTK_LAYER_SHELL_EDGE_LEFT, (!p.right) as i32);
        (lib.set_anchor)(raw_ptr, GTK_LAYER_SHELL_EDGE_RIGHT, p.right as i32);
        (set_margin)(raw_ptr, GTK_LAYER_SHELL_EDGE_TOP, p.margin_top);
        (set_margin)(raw_ptr, GTK_LAYER_SHELL_EDGE_LEFT, if p.right { 0 } else { p.margin_side });
        (set_margin)(raw_ptr, GTK_LAYER_SHELL_EDGE_RIGHT, if p.right { p.margin_side } else { 0 });
    }
}

#[cfg(test)]
mod tests {
    use super::*;


    #[test]
    fn insets_go_to_the_edge_the_desktop_names() {
        let top_only = PanelEdges { known: true, top: true, ..PanelEdges::default() };
        assert_eq!(
            attribute_insets(0, 32, top_only),
            Insets { left: 0, top: 32, right: 0, bottom: 0 }
        );
        let bottom_only = PanelEdges { known: true, bottom: true, ..PanelEdges::default() };
        assert_eq!(attribute_insets(0, 48, bottom_only).bottom, 48);
        assert_eq!(attribute_insets(0, 48, bottom_only).top, 0);
        let left_only = PanelEdges { known: true, left: true, ..PanelEdges::default() };
        assert_eq!(attribute_insets(60, 0, left_only).left, 60);
    }

    #[test]
    fn unexplained_or_two_sided_insets_keep_clear_of_both_edges() {
        // Unknown desktop: a 40px shortfall could be at the top or the bottom.
        let i = attribute_insets(0, 40, PanelEdges::default());
        assert_eq!((i.top, i.bottom), (40, 40));
        // Config says panels top AND bottom: the split is unknown.
        let both = PanelEdges { known: true, top: true, bottom: true, ..PanelEdges::default() };
        let i = attribute_insets(0, 80, both);
        assert_eq!((i.top, i.bottom), (80, 80));
        // Nothing missing: no inset, whatever the config says.
        assert_eq!(attribute_insets(0, 0, both), Insets::default());
        // A negative shortfall (allocation larger than the monitor) is no inset.
        assert_eq!(attribute_insets(-5, -5, PanelEdges::default()), Insets::default());
    }

    #[test]
    fn cosmic_panel_config_is_parsed() {
        let entries = "[\n    \"Panel\",\n    \"Dock\",\n]\n";
        assert_eq!(parse_cosmic_entries(entries), vec!["Panel", "Dock"]);
        assert!(parse_cosmic_entries("[]").is_empty());
        // The real layout on the target machine: exclusive top panel, dock
        // that does not reserve space.
        let e = cosmic_edges_from(&[
            ("Top\n".into(), "true\n".into()),
            ("Bottom".into(), "false".into()),
        ]);
        assert_eq!(
            e,
            PanelEdges { known: true, top: true, ..PanelEdges::default() }
        );
    }

    #[test]
    fn drawer_hugs_the_side_it_is_docked_to() {
        // Right dock: 520 wide, 10px from the right edge of a 1920 area.
        let p = drawer_placement(1920.0 - 520.0 - 10.0, 20.0, 520.0, 1920.0);
        assert_eq!(p, DrawerPlacement { right: true, margin_side: 10, margin_top: 20 });
        // Left dock.
        let p = drawer_placement(10.0, 20.0, 400.0, 1920.0);
        assert_eq!(p, DrawerPlacement { right: false, margin_side: 10, margin_top: 20 });
        // Centred view: placed from the left at its x.
        let p = drawer_placement(480.0, 40.0, 960.0, 1920.0);
        assert_eq!(p, DrawerPlacement { right: false, margin_side: 480, margin_top: 40 });
        // A rect hanging past the edge gets margin 0, never a negative one.
        assert_eq!(drawer_placement(1500.0, -3.0, 520.0, 1920.0).margin_side, 0);
        assert_eq!(drawer_placement(1500.0, -3.0, 520.0, 1920.0).margin_top, 0);
    }

    #[test]
    fn test_path_decision_layer_shell() {
        assert_eq!(
            decide_overlay_path(true, true, true),
            OverlayPath::LayerShell
        );
    }

    #[test]
    fn test_path_decision_not_wayland() {
        assert_eq!(
            decide_overlay_path(false, true, true),
            OverlayPath::Fallback(FallbackReason::NotWaylandBackend)
        );
        assert_eq!(
            decide_overlay_path(false, false, false),
            OverlayPath::Fallback(FallbackReason::NotWaylandBackend)
        );
    }

    #[test]
    fn test_path_decision_library_missing() {
        assert_eq!(
            decide_overlay_path(true, false, true),
            OverlayPath::Fallback(FallbackReason::LibraryMissing)
        );
    }

    #[test]
    fn test_path_decision_compositor_unsupported() {
        assert_eq!(
            decide_overlay_path(true, true, false),
            OverlayPath::Fallback(FallbackReason::CompositorUnsupported)
        );
    }

    #[test]
    fn test_input_rects_empty() {
        let rects = compute_input_rects(&[], 1.0);
        assert!(rects.is_empty());

        let region = build_cairo_region(&[], 1.0);
        assert!(region.is_empty());
        assert_eq!(region.num_rectangles(), 0);
    }

    #[test]
    fn test_input_rects_single() {
        let hitboxes = vec![StageRect {
            x: 100,
            y: 150,
            w: 200,
            h: 250,
        }];
        let rects = compute_input_rects(&hitboxes, 1.0);
        assert_eq!(rects.len(), 1);
        assert_eq!(rects[0].x(), 100);
        assert_eq!(rects[0].y(), 150);
        assert_eq!(rects[0].width(), 200);
        assert_eq!(rects[0].height(), 250);
        assert_eq!(rects[0], RectangleInt::new(100, 150, 200, 250));
    }

    #[test]
    fn test_input_rects_overlapping_union() {
        let hitboxes = vec![
            StageRect {
                x: 0,
                y: 0,
                w: 100,
                h: 100,
            },
            StageRect {
                x: 50,
                y: 0,
                w: 100,
                h: 100,
            },
        ];
        let rects = compute_input_rects(&hitboxes, 1.0);
        // Overlapping horizontal rects merge into one (0, 0, 150, 100)
        assert_eq!(rects.len(), 1);
        assert_eq!(rects[0].x(), 0);
        assert_eq!(rects[0].y(), 0);
        assert_eq!(rects[0].width(), 150);
        assert_eq!(rects[0].height(), 100);
        assert_eq!(rects[0], RectangleInt::new(0, 0, 150, 100));
    }

    #[test]
    fn test_input_rects_scale_factor() {
        // Physical pixels at 2x scale: 400x200 at (200, 300) -> logical 200x100 at (100, 150)
        let hitboxes = vec![StageRect {
            x: 200,
            y: 300,
            w: 400,
            h: 200,
        }];
        let rects = compute_input_rects(&hitboxes, 2.0);
        assert_eq!(rects.len(), 1);
        assert_eq!(rects[0].x(), 100);
        assert_eq!(rects[0].y(), 150);
        assert_eq!(rects[0].width(), 200);
        assert_eq!(rects[0].height(), 100);
        assert_eq!(rects[0], RectangleInt::new(100, 150, 200, 100));
    }

    #[test]
    fn test_input_rects_disjoint() {
        let hitboxes = vec![
            StageRect {
                x: 0,
                y: 0,
                w: 50,
                h: 50,
            },
            StageRect {
                x: 200,
                y: 200,
                w: 50,
                h: 50,
            },
        ];
        let rects = compute_input_rects(&hitboxes, 1.0);
        assert_eq!(rects.len(), 2);
    }
}
