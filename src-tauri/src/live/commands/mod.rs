//! Live-mode command implementations.
//!
//! Each command module maps to a specific capability:
//!   keyboard.rs  — type_text, press_key, press_hotkey (enigo + arboard)
//!   whatsapp.rs — full WhatsApp flow (open → search → type → send)
//!   browser.rs   — browser navigation (new tab, navigate, search)
//!   window.rs    — window focus management (AttachThreadInput trick)

pub mod keyboard;
pub mod whatsapp;
pub mod browser;
pub mod window;
