//! Live Optical Pass-Through Frosted Glass & Selective Cursor Hit-Testing.
//!
//! Provides:
//! 1. ~~Hardware DWM blur~~ — REMOVED per ADR-05 (`docs/architecture/06`):
//!    DWM materials render solid-opaque on non-activating windows.
//!    Blur comes from the screenshot-capture pipeline (`sidebar_backdrop.rs`).
//! 2. Selective Cursor Hit-Testing ("The Cursor Exception"):
//!    The entire transparent overlay window is 100% click-through to apps behind it,
//!    EXCEPT over registered interactive hitboxes (Overlay Button, Sidebar cards).
//!    Over these hitboxes, mouse clicks, text selection, and copying are fully enabled.
//! 3. Real-time Luminance Sampling Integration with `luminance_probe`.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct GlassHitbox {
    pub id: Option<&'static str>,
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl GlassHitbox {
    pub fn new(x: i32, y: i32, w: i32, h: i32) -> Self {
        Self {
            id: None,
            x,
            y,
            w,
            h,
        }
    }

    pub fn contains(&self, px: i32, py: i32) -> bool {
        px >= self.x && px < self.x + self.w && py >= self.y && py < self.y + self.h
    }
}

/// Dynamic registry of interactive hitboxes where the cursor is active.
static ACTIVE_HITBOXES: once_cell::sync::Lazy<parking_lot::Mutex<Vec<GlassHitbox>>> =
    once_cell::sync::Lazy::new(|| parking_lot::Mutex::new(Vec::new()));

/// Register interactive hitboxes for the overlay (in physical pixels).
pub fn set_active_hitboxes(hitboxes: Vec<GlassHitbox>) {
    *ACTIVE_HITBOXES.lock() = hitboxes;
}

/// Check if a physical screen coordinate is inside any active interactive hitbox.
pub fn is_cursor_inside_hitbox(px: i32, py: i32) -> bool {
    ACTIVE_HITBOXES.lock().iter().any(|hb| hb.contains(px, py))
}

/// Returns true if any live glass hitboxes are currently active.
pub fn has_active_hitboxes() -> bool {
    !ACTIVE_HITBOXES.lock().is_empty()
}


/// Tauri IPC command: registers interactive hitboxes (physical pixels) for the
/// overlay window. Over these rectangles, the cursor interacts normally
/// (clickable buttons, text selection, copying); outside them, clicks pass
/// through to whatever application or desktop is behind.
#[tauri::command]
pub fn register_glass_hitboxes(rects: Vec<crate::stage::StageRect>) -> Result<(), String> {
    let hitboxes = rects
        .into_iter()
        .map(|r| GlassHitbox::new(r.x, r.y, r.w, r.h))
        .collect();
    set_active_hitboxes(hitboxes);
    // Also sync to stage's HITBOXES for stage mouse-hole loop
    let stage_rects: Vec<crate::stage::StageRect> = ACTIVE_HITBOXES
        .lock()
        .iter()
        .map(|h| crate::stage::StageRect {
            x: h.x,
            y: h.y,
            w: h.w,
            h: h.h,
        })
        .collect();
    crate::stage::set_hitbox_source("live-glass", stage_rects);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_glass_hitbox_contains() {
        let hb = GlassHitbox::new(100, 100, 200, 50);

        // Inside
        assert!(hb.contains(100, 100));
        assert!(hb.contains(200, 125));
        assert!(hb.contains(299, 149));

        // Outside edges
        assert!(!hb.contains(99, 100));
        assert!(!hb.contains(100, 99));
        assert!(!hb.contains(300, 125));
        assert!(!hb.contains(200, 150));
    }

    #[test]
    fn test_set_active_hitboxes_and_query() {
        let hbs = vec![
            GlassHitbox::new(50, 50, 100, 30),
            GlassHitbox::new(500, 500, 80, 80),
        ];
        set_active_hitboxes(hbs);

        assert!(is_cursor_inside_hitbox(60, 60));
        assert!(is_cursor_inside_hitbox(510, 510));
        assert!(!is_cursor_inside_hitbox(0, 0));
        assert!(!is_cursor_inside_hitbox(300, 300));
    }
}
