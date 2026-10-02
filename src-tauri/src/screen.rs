//! Screen vision + control — see the screen, resolve ordinals, click.
//!
//! Grounding order (see docs/mcp/09-screen-vision-control.md):
//!   1. Browser tabs → hotkeys first (Ctrl+1..8, 100% reliable, instant).
//!   2. Everything else → Windows UIA tree (exact names + boxes, free).
//!   3. Canvas/custom UI → vision model / OCR (`vision.rs`, Phase 1+).
//!
//! Ordinal rule: visible + enabled actionables in the foreground window,
//! sorted top-to-bottom then left-to-right. "3rd option" = 3rd actionable.
//! Password/secure fields never resolve; destructive targets need confirm
//! (enforced by callers, not here).

/// One actionable UI element.
#[derive(Debug, Clone, PartialEq)]
pub struct UiElement {
    pub name: String,
    pub kind: String,
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

/// Sort elements in reading order: top-to-bottom, then left-to-right.
/// Pure function — unit-tested below.
pub fn sort_reading_order(mut els: Vec<UiElement>) -> Vec<UiElement> {
    // Row tolerance: items within 12px vertically share a row.
    els.sort_by(|a, b| {
        let row_a = a.y / 12;
        let row_b = b.y / 12;
        row_a.cmp(&row_b).then(a.x.cmp(&b.x))
    });
    els
}

/// 1-based ordinal pick. Returns None when out of range.
pub fn pick_ordinal(sorted: &[UiElement], n: u32) -> Option<&UiElement> {
    if n == 0 {
        return None;
    }
    sorted.get((n - 1) as usize)
}

/// Parse "3rd", "2nd", "1st", "4th", "12th" (and bare "3") into a number.
pub fn parse_ordinal(text: &str) -> Option<u32> {
    let t = text.trim().to_lowercase();
    let digits: String = t.chars().take_while(|c| c.is_ascii_digit()).collect();
    if digits.is_empty() {
        return None;
    }
    // Accept "3", "3rd", "3th"-ish; reject trailing junk like "3x".
    let rest = t[digits.len()..].trim_start();
    if !rest.is_empty()
        && !["st", "nd", "rd", "th"].iter().any(|suf| rest.starts_with(suf))
    {
        return None;
    }
    digits.parse().ok()
}

/// Pure: fuzzy match score 0.0-1.0 of a locate query against a UI name.
/// Exact > substring > all-words > any-word. Locate hits need >= 0.5.
pub fn score_match(query: &str, name: &str) -> f32 {
    let norm = |s: &str| {
        s.to_lowercase()
            .split(|c: char| !c.is_alphanumeric())
            .filter(|w| !w.is_empty())
            .collect::<Vec<_>>()
            .join(" ")
    };
    let q = norm(query);
    let n = norm(name);
    if q.is_empty() || n.is_empty() {
        return 0.0;
    }
    if q == n {
        return 1.0;
    }
    if n.contains(&q) || q.contains(&n) {
        return 0.8;
    }
    // Space-insensitive containment: OCR routinely drops inter-word
    // spaces ("Closewindow", "Signin"). Scores just below substring.
    let nospace = |s: &str| s.split_whitespace().collect::<String>();
    let (nq, nn) = (nospace(&q), nospace(&n));
    if nn.contains(&nq) || nq.contains(&nn) {
        return 0.75;
    }
    let qw: Vec<&str> = q.split(' ').collect();
    let nw: Vec<&str> = n.split(' ').collect();
    let hits = qw.iter().filter(|w| nw.contains(w)).count();
    if hits == qw.len() {
        return 0.7;
    }
    if hits > 0 {
        return 0.4;
    }
    0.0
}

#[cfg(target_os = "windows")]
mod win {
    use super::UiElement;
    use uiautomation::controls::ControlType;
    use uiautomation::UIAutomation;

    const CLICKABLE: &[ControlType] = &[
        ControlType::Button,
        ControlType::Hyperlink,
        ControlType::TabItem,
        ControlType::MenuItem,
        ControlType::ListItem,
        ControlType::CheckBox,
        ControlType::RadioButton,
        ControlType::SplitButton,
    ];

    /// List actionable elements in the foreground window via UIA.
    /// Best-effort: any failing element is skipped, never fatal.
    pub fn list_actionables() -> Vec<UiElement> {
        let automation = match UIAutomation::new() {
            Ok(a) => a,
            Err(_) => return Vec::new(),
        };
        let focused = match automation.get_focused_element() {
            Ok(e) => e,
            Err(_) => return Vec::new(),
        };
        let mut out = Vec::new();
        for ct in CLICKABLE {
            let found = automation
                .create_matcher()
                .from(focused.clone())
                .timeout(500)
                .control_type(*ct)
                .find_all();
            let elements = match found {
                Ok(e) => e,
                Err(_) => continue,
            };
            for el in elements.iter() {
                let enabled = el.is_enabled().unwrap_or(false);
                if !enabled {
                    continue;
                }
                let name = el.get_name().unwrap_or_default();
                if name.trim().is_empty() {
                    continue;
                }
                let kind = format!("{:?}", ct);
                // Skip password/secure fields and address bars by name.
                let lower = name.to_lowercase();
                if lower.contains("password") {
                    continue;
                }
                let (x, y, w, h) = match el.get_bounding_rectangle() {
                    Ok(r) => (r.get_left(), r.get_top(), r.get_width(), r.get_height()),
                    Err(_) => continue,
                };
                if w <= 0 || h <= 0 {
                    continue;
                }
                out.push(UiElement {
                    name,
                    kind,
                    x,
                    y,
                    w,
                    h,
                });
            }
        }
        super::sort_reading_order(out)
    }

    /// Move the mouse to an element's center and left-click.
    pub fn click_element(el: &UiElement) -> Result<(), String> {
        use enigo::{Button, Coordinate, Direction, Enigo, Mouse, Settings};
        let mut enigo =
            Enigo::new(&Settings::default()).map_err(|e| format!("enigo init: {e}"))?;
        let cx = el.x + el.w / 2;
        let cy = el.y + el.h / 2;
        enigo
            .move_mouse(cx, cy, Coordinate::Abs)
            .map_err(|e| format!("mouse move: {e}"))?;
        enigo
            .button(Button::Left, Direction::Click)
            .map_err(|e| format!("mouse click: {e}"))?;
        Ok(())
    }

    /// Find the best element matching a locate query ("save button") by
    /// name across the focused window. Returns the highest scorer >= 0.5,
    /// or None. Powers Phase-3a locate-first: exact boxes, ~5ms, no VLM.
    /// Password/secure fields never match (same rule as ordinals).
    pub fn find_by_name(query: &str) -> Option<UiElement> {
        let automation = UIAutomation::new().ok()?;
        let focused = automation.get_focused_element().ok()?;
        let mut best: Option<(f32, UiElement)> = None;
        for ct in CLICKABLE {
            let found = automation
                .create_matcher()
                .from(focused.clone())
                .timeout(500)
                .control_type(*ct)
                .find_all();
            let elements = match found {
                Ok(e) => e,
                Err(_) => continue,
            };
            for el in elements.iter() {
                if !el.is_enabled().unwrap_or(false) {
                    continue;
                }
                let name = el.get_name().unwrap_or_default();
                if name.trim().is_empty() {
                    continue;
                }
                let lower = name.to_lowercase();
                if lower.contains("password") {
                    continue;
                }
                let (x, y, w, h) = match el.get_bounding_rectangle() {
                    Ok(r) => (r.get_left(), r.get_top(), r.get_width(), r.get_height()),
                    Err(_) => continue,
                };
                if w <= 0 || h <= 0 {
                    continue;
                }
                let score = super::score_match(query, &name);
                if score >= 0.5 && best.as_ref().map_or(true, |(s, _)| score > *s) {
                    best = Some((
                        score,
                        UiElement {
                            name,
                            kind: format!("{:?}", ct),
                            x,
                            y,
                            w,
                            h,
                        },
                    ));
                }
            }
        }
        best.map(|(_, el)| el)
    }
}

#[cfg(target_os = "windows")]
pub use win::{click_element, find_by_name, list_actionables};

/// Linux locate via the AT-SPI semantic tier.
///
/// This used to be a stub returning `None`, so on Linux every "where is X?"
/// request fell through to screenshot + OCR + a cloud VLM — 1312ms of OCR plus a
/// network round trip, with a hallucinated coordinate as the output. The
/// accessibility tree answers the same question exactly, for ~100ms, locally.
///
/// Returns `None` on any failure so the caller falls through to the pixel tier.
/// That fallback is not a formality: Chromium/Electron expose only a skeleton over
/// AT-SPI unless launched with `--force-renderer-accessibility`, so VS Code and
/// Chrome legitimately miss here and are the CDP tier's job.
#[cfg(all(target_os = "linux", not(feature = "mock-wake")))]
pub fn find_by_name(query: &str) -> Option<UiElement> {
    if query.trim().is_empty() {
        return None;
    }
    // Cascade: AT-SPI first (exact, and the only tier that gives geometry for
    // GTK/Qt), then CDP for Chromium/Electron, which AT-SPI sees as a 1-node
    // skeleton against VS Code's real 1026. Only a genuine semantic miss falls
    // through to the pixel tier — see `Resolution::should_try_pixels`.
    match crate::semantic::resolve(query, None) {
        crate::semantic::Resolution::Found(hit) => {
            let Some(b) = hit.bounds else {
                // A CDP hit carries no geometry: the AX tree is semantic, not
                // spatial. There is nothing to point at, so report a miss rather
                // than a fabricated coordinate — the caller will fall through to
                // OCR/VLM, which can at least give a position.
                tracing::debug!(
                    "semantic: {query:?} found via {} but it has no geometry",
                    hit.tier.as_str()
                );
                return None;
            };
            if !b.is_real() {
                tracing::debug!("semantic: {query:?} found but has unlaid-out bounds");
                return None;
            }
            Some(UiElement {
                name: hit.name,
                // The role is the Linux equivalent of the Windows UIA control
                // type, so downstream filtering on `kind` keeps working.
                kind: hit.role,
                x: b.x,
                y: b.y,
                w: b.w,
                h: b.h,
            })
        }
        crate::semantic::Resolution::NotFound => None,
        crate::semantic::Resolution::NoTierAvailable { detail } => {
            tracing::debug!("semantic: no tier available ({detail})");
            None
        }
    }
}

/// macOS has no wired tree yet; the stub keeps the caller shape and falls through.
#[cfg(not(any(target_os = "windows", all(target_os = "linux", not(feature = "mock-wake")))))]
pub fn find_by_name(_query: &str) -> Option<UiElement> {
    None
}

/// Switch browser tab by index (1-based): Ctrl+1..8, Ctrl+9 = last.
/// Hotkeys beat grounding for tabs — 100% reliable, instant.
pub fn switch_browser_tab(index: u32) -> Result<String, String> {
    if index == 0 || index > 9 {
        return Err("tab number must be 1-9".to_string());
    }
    use enigo::{Direction, Enigo, Key, Keyboard, Settings};
    let mut enigo =
        Enigo::new(&Settings::default()).map_err(|e| format!("enigo init: {e}"))?;
    enigo
        .key(Key::Control, Direction::Press)
        .map_err(|e| format!("ctrl press: {e}"))?;
    let digit = match index {
        1 => Key::Unicode('1'),
        2 => Key::Unicode('2'),
        3 => Key::Unicode('3'),
        4 => Key::Unicode('4'),
        5 => Key::Unicode('5'),
        6 => Key::Unicode('6'),
        7 => Key::Unicode('7'),
        8 => Key::Unicode('8'),
        _ => Key::Unicode('9'),
    };
    let r = enigo
        .key(digit, Direction::Click)
        .map_err(|e| format!("digit: {e}"));
    let _ = enigo.key(Key::Control, Direction::Release);
    r?;
    Ok(format!("Switched to tab {index}, sir."))
}

// ─── Screen capture (Phase 1: feed screenshots to the vision model) ──
// Cross-platform, zero new crates: Windows GDI (mirrors
// `sidebar_backdrop.rs`), Linux via the xdg-desktop-portal Screenshot
// API over the existing `zbus` dependency, macOS via `screencapture`.

use image::DynamicImage;

/// Max width (px) sent to the vision model — clicky parity (1280px JPEG).
pub const VISION_MAX_WIDTH: u32 = 1280;
/// Hard cap on the JPEG payload (~900KB keeps base64 small for free tiers).
const VISION_MAX_BYTES: usize = 900_000;

/// A captured screen: original dimensions + downscaled image.
/// Original dims are kept so Phase-2 pointer coordinates (0-1000
/// normalized) can map back to physical pixels.
pub struct ScreenShot {
    pub image: DynamicImage,
    pub orig_w: u32,
    pub orig_h: u32,
    pub scaled_w: u32,
    pub scaled_h: u32,
}

impl ScreenShot {
    pub fn from_dynamic(img: DynamicImage) -> Self {
        let (orig_w, orig_h) = (img.width().max(1), img.height().max(1));
        let (tw, th) = target_dims(orig_w, orig_h, VISION_MAX_WIDTH);
        let scaled = if tw == orig_w && th == orig_h {
            img
        } else {
            img.resize(tw, th, image::imageops::FilterType::Triangle)
        };
        Self {
            image: scaled,
            orig_w,
            orig_h,
            scaled_w: tw,
            scaled_h: th,
        }
    }

    /// JPEG-encode the (already downscaled) image. Retries once smaller
    /// when the payload would exceed the free-tier-friendly byte cap.
    pub fn jpeg(&self) -> Vec<u8> {
        for (scale, quality) in [(1.0, 80u8), (0.75, 70u8)] {
            let img = if scale == 1.0 {
                self.image.clone()
            } else {
                let tw = ((self.scaled_w as f32 * scale) as u32).max(1);
                let th = ((self.scaled_h as f32 * scale) as u32).max(1);
                self.image
                    .resize(tw, th, image::imageops::FilterType::Triangle)
            };
            let mut buf = Vec::new();
            let mut enc =
                image::codecs::jpeg::JpegEncoder::new_with_quality(&mut buf, quality);
            if enc.encode_image(&img).is_ok() && buf.len() <= VISION_MAX_BYTES {
                return buf;
            }
            if scale < 1.0 {
                return buf;
            }
        }
        Vec::new()
    }
}

/// Pure: (w, h) scaled to fit `max_w`, preserving aspect. Never upscales.
pub fn target_dims(w: u32, h: u32, max_w: u32) -> (u32, u32) {
    if w == 0 || h == 0 {
        return (1, 1);
    }
    if w <= max_w {
        return (w, h);
    }
    let s = max_w as f64 / w as f64;
    (max_w, ((h as f64 * s).round() as u32).max(1))
}

#[cfg(target_os = "windows")]
pub async fn capture_primary() -> Result<ScreenShot, String> {
    use image::{ImageBuffer, Rgba};
    use windows::Win32::UI::WindowsAndMessaging::{
        GetSystemMetrics, SM_CXSCREEN, SM_CYSCREEN,
    };
    let w = unsafe { GetSystemMetrics(SM_CXSCREEN) };
    let h = unsafe { GetSystemMetrics(SM_CYSCREEN) };
    if w <= 0 || h <= 0 {
        return Err("could not query screen size".into());
    }
    let bgra = crate::sidebar_backdrop::capture_region_bgra_public(0, 0, w, h)
        .ok_or_else(|| "screen capture failed (BitBlt)".to_string())?;
    // BGRA (GDI) -> RGBA (image crate); BitBlt leaves alpha empty.
    let mut rgba = Vec::with_capacity(bgra.len());
    for px in bgra.chunks_exact(4) {
        rgba.extend_from_slice(&[px[2], px[1], px[0], 255]);
    }
    let buf: ImageBuffer<Rgba<u8>, Vec<u8>> =
        ImageBuffer::from_raw(w as u32, h as u32, rgba).ok_or("bad capture buffer")?;
    Ok(ScreenShot::from_dynamic(DynamicImage::ImageRgba8(buf)))
}

#[cfg(target_os = "linux")]
pub async fn capture_primary() -> Result<ScreenShot, String> {
    let png = portal_screenshot_png().await?;
    let img =
        image::load_from_memory(&png).map_err(|e| format!("decode screenshot: {e}"))?;
    Ok(ScreenShot::from_dynamic(img))
}

/// Screenshot via org.freedesktop.portal.Desktop (works on X11 and
/// Wayland wherever xdg-desktop-portal runs — GNOME, KDE, COSMIC).
/// Returns raw PNG bytes; the portal-owned file is removed afterwards.
#[cfg(target_os = "linux")]
async fn portal_screenshot_png() -> Result<Vec<u8>, String> {
    use std::collections::HashMap;
    use std::time::Duration;
    use futures_util::StreamExt;
    use zbus::fdo::DBusProxy;
    use zbus::message::Type as MessageType;
    use zbus::zvariant::{OwnedObjectPath, OwnedValue, Value};
    use zbus::{Connection, MatchRule, MessageStream};

    let conn = Connection::session()
        .await
        .map_err(|e| format!("D-Bus session: {e}"))?;
    // Fast-fail when no portal exists (plain X11 without xdg-desktop-portal)
    // instead of burning the 25s Response timeout below.
    {
        use zbus::fdo::DBusProxy;
        let dbus = DBusProxy::new(&conn)
            .await
            .map_err(|e| format!("D-Bus proxy: {e}"))?;
        let names = dbus
            .list_names()
            .await
            .map_err(|e| format!("D-Bus list: {e}"))?;
        if !names.iter().any(|n| n.as_str() == "org.freedesktop.portal.Desktop") {
            return Err("no screenshot portal found — install xdg-desktop-portal (and a backend for your desktop) to enable screen vision".into());
        }
    }
    let token = format!("nexus{}", std::process::id());
    let mut opts: HashMap<&str, Value> = HashMap::new();
    opts.insert("handle_token", Value::from(token.as_str()));
    let reply = conn
        .call_method(
            Some("org.freedesktop.portal.Desktop"),
            "/org/freedesktop/portal/desktop",
            Some("org.freedesktop.portal.Screenshot"),
            "Screenshot",
            &("", opts),
        )
        .await
        .map_err(|e| format!("portal Screenshot call: {e}"))?;
    let req_path: OwnedObjectPath = reply
        .body()
        .deserialize()
        .map_err(|e| format!("portal reply: {e}"))?;
    let rule = MatchRule::builder()
        .msg_type(MessageType::Signal)
        .interface("org.freedesktop.portal.Request")
        .map_err(|e| format!("match rule: {e}"))?
        .path(req_path.into_inner())
        .map_err(|e| format!("match rule: {e}"))?
        .member("Response")
        .map_err(|e| format!("match rule: {e}"))?
        .build();
    // Server-side match registration is internal-only in zbus 5, so
    // register via the standard org.freedesktop.DBus AddMatch call
    // (same DBusProxy pattern as mpris.rs). Without this the portal's
    // Response signal is never delivered to our connection.
    let dbus = DBusProxy::new(&conn)
        .await
        .map_err(|e| format!("D-Bus proxy: {e}"))?;
    dbus.add_match_rule(rule)
        .await
        .map_err(|e| format!("match register: {e}"))?;
    let mut stream = MessageStream::from(&conn);
    let msg = tokio::time::timeout(Duration::from_secs(25), stream.next())
        .await
        .map_err(|_| "screenshot timed out (portal gave no response)".to_string())?
        .ok_or("portal signal stream ended")?
        .map_err(|e| format!("portal signal: {e}"))?;
    let (code, results): (u32, HashMap<String, OwnedValue>) = msg
        .body()
        .deserialize()
        .map_err(|e| format!("portal response: {e}"))?;
    if code != 0 {
        return Err(format!("screenshot cancelled/failed (response {code})"));
    }
    let uri_v = results.get("uri").ok_or("portal gave no file uri")?;
    let uri =
        String::try_from(uri_v.clone()).map_err(|e| format!("bad uri: {e}"))?;
    let raw = uri
        .strip_prefix("file://")
        .ok_or("portal uri is not a file")?;
    let path = percent_decode(raw);
    let bytes =
        std::fs::read(&path).map_err(|e| format!("read screenshot file: {e}"))?;
    let _ = std::fs::remove_file(&path); // best-effort cleanup
    if bytes.is_empty() {
        return Err("screenshot file was empty".into());
    }
    Ok(bytes)
}

/// Minimal %XX decoder for file:// URIs (avoids a new crate).
#[cfg(target_os = "linux")]
fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(h), Some(l)) = (hex_val(bytes[i + 1]), hex_val(bytes[i + 2])) {
                out.push(h * 16 + l);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(target_os = "linux")]
fn hex_val(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

#[cfg(target_os = "macos")]
pub async fn capture_primary() -> Result<ScreenShot, String> {
    let tmp = std::env::temp_dir().join(format!("nexus_screen_{}.jpg", std::process::id()));
    let out = std::process::Command::new("/usr/sbin/screencapture")
        .args(["-x", "-tjpg"])
        .arg(&tmp)
        .output()
        .map_err(|e| format!("screencapture: {e}"))?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        // TCC denial surfaces on stderr — point at the exact Settings pane.
        if stderr.contains("not authorized") || stderr.contains("denied") {
            return Err("screen recording is blocked: allow NEXUS in System Settings → Privacy & Security → Screen Recording, then ask again".into());
        }
        let detail: String = stderr.chars().take(150).collect();
        return Err(format!("screencapture failed ({detail})"));
    }
    let bytes = std::fs::read(&tmp).map_err(|e| format!("read screenshot: {e}"))?;
    let _ = std::fs::remove_file(&tmp);
    let img =
        image::load_from_memory(&bytes).map_err(|e| format!("decode: {e}"))?;
    Ok(ScreenShot::from_dynamic(img))
}

#[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
pub async fn capture_primary() -> Result<ScreenShot, String> {
    Err("screen capture is not supported on this OS".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{ImageBuffer, Rgba};

    fn el(name: &str, x: i32, y: i32) -> UiElement {
        UiElement {
            name: name.into(),
            kind: "Button".into(),
            x,
            y,
            w: 80,
            h: 30,
        }
    }

    #[test]
    fn reading_order_top_to_bottom_then_left_to_right() {
        let v = sort_reading_order(vec![el("b", 200, 10), el("a", 10, 10), el("c", 10, 100)]);
        assert_eq!(v[0].name, "a");
        assert_eq!(v[1].name, "b");
        assert_eq!(v[2].name, "c");
    }

    #[test]
    fn ordinal_pick_one_based() {
        let v = vec![el("a", 0, 0), el("b", 0, 50)];
        assert_eq!(pick_ordinal(&v, 1).unwrap().name, "a");
        assert_eq!(pick_ordinal(&v, 2).unwrap().name, "b");
        assert!(pick_ordinal(&v, 0).is_none());
        assert!(pick_ordinal(&v, 3).is_none());
    }

    #[test]
    fn ordinal_parse_suffixes() {
        assert_eq!(parse_ordinal("3rd"), Some(3));
        assert_eq!(parse_ordinal("1st"), Some(1));
        assert_eq!(parse_ordinal("2nd"), Some(2));
        assert_eq!(parse_ordinal("12th"), Some(12));
        assert_eq!(parse_ordinal("4"), Some(4));
        assert_eq!(parse_ordinal("option"), None);
        assert_eq!(parse_ordinal("3x"), None);
    }

    #[test]
    fn target_dims_downscales_wide_screens() {
        assert_eq!(target_dims(1920, 1080, 1280), (1280, 720));
        assert_eq!(target_dims(3840, 2160, 1280), (1280, 720));
    }

    #[test]
    fn target_dims_never_upscales() {
        assert_eq!(target_dims(800, 600, 1280), (800, 600));
        assert_eq!(target_dims(1280, 800, 1280), (1280, 800));
        assert_eq!(target_dims(0, 0, 1280), (1, 1));
    }

    #[test]
    fn screenshot_from_dynamic_encodes_valid_jpeg() {
        let buf: ImageBuffer<Rgba<u8>, Vec<u8>> =
            ImageBuffer::from_fn(2000, 1000, |x, y| Rgba([(x % 256) as u8, (y % 256) as u8, 128, 255]));
        let shot = ScreenShot::from_dynamic(DynamicImage::ImageRgba8(buf));
        assert_eq!((shot.orig_w, shot.orig_h), (2000, 1000));
        assert_eq!((shot.scaled_w, shot.scaled_h), (1280, 640));
        let jpeg = shot.jpeg();
        assert!(jpeg.len() > 100, "jpeg should not be empty");
        assert_eq!(&jpeg[0..2], &[0xFF, 0xD8], "must start with JPEG magic");
        let decoded = image::load_from_memory(&jpeg).expect("jpeg must decode");
        assert_eq!((decoded.width(), decoded.height()), (1280, 640));
    }

    /// Manual smoke test: real OS capture end-to-end. Ignored by default
    /// (needs a live desktop session + portal/permission); run on demand:
    /// `cargo test manual_capture_smoke -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn manual_capture_smoke() {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("tokio runtime");
        let shot = rt
            .block_on(capture_primary())
            .expect("OS capture failed");
        let jpeg = shot.jpeg();
        assert!(!jpeg.is_empty(), "jpeg must not be empty");
        println!(
            "captured {}x{} -> {}x{} ({} byte jpeg)",
            shot.orig_w,
            shot.orig_h,
            shot.scaled_w,
            shot.scaled_h,
            jpeg.len()
        );
    }

    #[test]
    fn score_match_exact_beats_substring() {
        assert_eq!(score_match("save", "Save"), 1.0);
        assert_eq!(score_match("save button", "Save Button"), 1.0);
    }

    #[test]
    fn score_match_substring_hits() {
        assert!(((score_match("save", "Save As")) - 0.8).abs() < 1e-6);
        // Not contiguous ("address and search bar") → all-words score 0.7.
        assert!(((score_match("address bar", "Address and search bar")) - 0.7).abs() < 1e-6);
        assert!(((score_match("search bar", "Address and search bar")) - 0.8).abs() < 1e-6);
    }

    #[test]
    fn score_match_ignores_dropped_ocr_spaces() {
        // RapidOCR emits "Closewindow" for "Close window" — must still hit.
        assert!(((score_match("close window", "Closewindow")) - 0.75).abs() < 1e-6);
        assert!(((score_match("sign in", "Signin")) - 0.75).abs() < 1e-6);
    }

    #[test]
    fn score_match_word_overlap_ranks() {
        // All query words present, different order → 0.7 (above threshold).
        assert!(((score_match("button save", "Save Button")) - 0.7).abs() < 1e-6);
        // One word only → 0.4 (below threshold, correctly rejected).
        assert!(((score_match("save file", "Save Button")) - 0.4).abs() < 1e-6);
        assert_eq!(score_match("eiffel tower", "Save Button"), 0.0);
        assert_eq!(score_match("", "Save"), 0.0);
    }
}
