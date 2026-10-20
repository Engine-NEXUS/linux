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

/// Edge anchor indices for gtk-layer-shell.
const GTK_LAYER_SHELL_EDGE_LEFT: i32 = 0;
const GTK_LAYER_SHELL_EDGE_RIGHT: i32 = 1;
const GTK_LAYER_SHELL_EDGE_TOP: i32 = 2;
const GTK_LAYER_SHELL_EDGE_BOTTOM: i32 = 3;

/// Keyboard interactivity mode for gtk-layer-shell (0 = NONE).
const GTK_LAYER_SHELL_KEYBOARD_MODE_NONE: i32 = 0;

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
                    (lib.set_exclusive_zone)(raw_ptr, -1);
                    if let Some(set_km) = lib.set_keyboard_mode {
                        (set_km)(raw_ptr, GTK_LAYER_SHELL_KEYBOARD_MODE_NONE);
                    } else if let Some(set_ki) = lib.set_keyboard_interactivity {
                        (set_ki)(raw_ptr, 0);
                    }
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
                LAYER_SHELL_ACTIVE.store(true, Ordering::SeqCst);
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

#[cfg(test)]
mod tests {
    use super::*;

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
