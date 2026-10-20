//! Linux screen queries: monitor size, semantic element lookup, portal capture.
//!
//! Three jobs, and they share the Linux platform story:
//!
//! 1. **Monitor size** — read from the DRM connectors, with a sane fallback.
//! 2. **Element lookup** — the AT-SPI / CDP semantic tiers, behind the
//!    identity-based policy gate (see `semantic` and `policy`). This is the
//!    Linux answer to UI Automation: on a native Wayland session it is the
//!    *only* way to name a control, and unlike synthetic input it needs no
//!    portal and no consent dialog.
//! 3. **Capture** — the xdg-desktop-portal `Screenshot` API, which is the only
//!    sanctioned way to read the screen on Wayland. It goes through the
//!    pre-capture privacy gate first, so an unidentified foreground refuses
//!    the capture instead of quietly grabbing everything.

use super::UiElement;

/// Parse a mode string like "1920x1080" into (w, h). Pure, unit-tested.
pub fn parse_mode_line(line: &str) -> Option<(i32, i32)> {
    let mut parts = line.trim().split('x');
    let w = parts.next()?.parse::<i32>().ok()?;
    let h = parts.next()?.parse::<i32>().ok()?;
    if w > 0 && h > 0 {
        Some((w, h))
    } else {
        None
    }
}

/// Primary monitor size in physical px on Linux.
/// Queries connected DRM connectors, falling back to 1920x1080.
pub fn primary_monitor_size() -> Option<(i32, i32)> {
    if let Ok(entries) = std::fs::read_dir("/sys/class/drm") {
        for entry in entries.flatten() {
            let path = entry.path();
            let status_file = path.join("status");
            if let Ok(status) = std::fs::read_to_string(&status_file) {
                if status.trim() == "connected" {
                    let modes_file = path.join("modes");
                    if let Ok(modes) = std::fs::read_to_string(&modes_file) {
                        if let Some(first_line) = modes.lines().next() {
                            if let Some((w, h)) = parse_mode_line(first_line) {
                                return Some((w, h));
                            }
                        }
                    }
                }
            }
        }
    }
    Some((1920, 1080))
}

/// List the actionable elements of the foreground window.
///
/// This is the Linux counterpart of the Windows `list_actionables`, and it
/// answers from the **accessibility tree** rather than from pixels: exact
/// names, exact roles, exact rectangles, in about a hundred milliseconds and
/// with no model call.
///
/// The policy gate runs inside `semantic::list_actionables`, so a window that
/// cannot be identified yields an empty list rather than a leak. The refusal
/// reason is available from `semantic::list_actionables` directly, for callers
/// that need to say it out loud.
pub fn list_actionables() -> Vec<UiElement> {
    match list_actionables_with_reason() {
        Ok(els) => els,
        Err(e) => {
            tracing::warn!("screen: semantic element list refused — {e}");
            Vec::new()
        }
    }
}

/// The ONE rule for "is this element actionable?", shared by the click path
/// and the inspect path.
///
/// * a **credential field** is never actionable — the policy layer knows these
///   by their ROLE (a password entry says so), which a name-based check cannot
///   be trusted to catch;
/// * the element must be **enabled** (the toolkit reports no
///   STATE_ENABLED / STATE_SENSITIVE for an insensitive widget);
/// * it must be **on screen**. The toolkit reports STATE_VISIBLE for a widget
///   that is laid out and STATE_SHOWING for one that is actually RENDERED.
///   "Visible but not showing" is the off-screen verdict (a collapsed
///   container, a viewport that clips it). When the toolkit reports NEITHER,
///   the verdict is **unknown** — the absence of an optional state is not
///   evidence that the widget is hidden — so the element is kept and the
///   application is logged once, so an empty list is never confused with an
///   empty window;
/// * it must be something the toolkit can act on: it exposes an action, or it
///   is an editable field (a text field is a legitimate click target). The
///   server applies the same rule; repeating it here keeps the guarantee on
///   this side of the wire;
/// * and the shared acceptance rule (`screen::accept_actionable`) has the
///   last word: named, sized, not a password field by name.
pub fn is_actionable_hit(h: &crate::semantic::SemanticHit) -> bool {
    if crate::policy::classify_node(&h.role, &h.name).is_some() {
        return false;
    }
    let Some(b) = h.bounds else { return false };
    if !b.is_real() {
        return false;
    }
    let enabled = h.states.is_empty()
        || h
            .states
            .iter()
            .any(|s| s.as_str() == "enabled" || s.as_str() == "sensitive");
    let reports_visibility = h
        .states
        .iter()
        .any(|s| s.as_str() == "visible" || s.as_str() == "showing");
    if !reports_visibility {
        // The toolkit says nothing about visibility: that is "cannot tell",
        // not "nothing is on screen" — and it is logged once per application
        // so an empty list is never confused with an empty window.
        log_unknown_visibility_once(&h.app);
    }
    let offscreen = is_offscreen(&h.states);
    let actionable = !h.actions.is_empty()
        || h.states.iter().any(|s| s.as_str() == "editable");
    if !actionable {
        return false;
    }
    if h.chromium_skeleton_risk {
        // A Chromium/Electron tree is a skeleton unless the renderer was
        // launched with --force-renderer-accessibility, so this element may
        // be all this tier can see of that application.
        tracing::debug!("screen: {:?} comes from a Chromium/Electron tree", h.name);
    }
    crate::screen::accept_actionable(enabled, offscreen, &h.name, b.w, b.h)
}

/// Whether the toolkit's state set proves the element is off screen.
///
/// "Visible but not showing" is the only verdict that counts. A state set
/// that mentions neither is **unknown** — the toolkit may simply not implement
/// those states — and an unknown verdict must not disqualify the element.
fn is_offscreen(states: &[String]) -> bool {
    let visible = states.iter().any(|s| s == "visible");
    let showing = states.iter().any(|s| s == "showing");
    if !visible && !showing {
        return false;
    }
    visible && !showing
}

/// Applications whose toolkit reports neither STATE_VISIBLE nor
/// STATE_SHOWING, logged once each.
///
/// Without this, a toolkit that does not implement those states produces an
/// empty list with no explanation — which reads as "this window has no
/// controls" rather than "I could not tell whether anything was on screen".
static UNKNOWN_VISIBILITY_LOGGED: once_cell::sync::Lazy<
    std::sync::Mutex<std::collections::HashSet<String>>,
> = once_cell::sync::Lazy::new(|| {
    std::sync::Mutex::new(std::collections::HashSet::new())
});

fn log_unknown_visibility_once(app: &str) {
    let key = if app.trim().is_empty() {
        "<unnamed>"
    } else {
        app
    };
    if let Ok(mut seen) = UNKNOWN_VISIBILITY_LOGGED.lock() {
        if seen.insert(key.to_string()) {
            tracing::warn!(
                "screen: the toolkit of {key:?} reports neither STATE_VISIBLE nor \
                 STATE_SHOWING — the off-screen test cannot run for it, so elements are \
                 offered even when they are clipped"
            );
        }
    }
}

/// The actionable elements of the foreground window, with the refusal reason.
///
/// The reason matters: "I could not identify the window" and "this window has
/// no controls" are very different answers, and only the first one is safe to
/// act on.
///
/// The list is put through the **same contract as the Windows one**:
/// `screen::accept_actionable` decides which elements qualify (enabled, on
/// screen, named, sized, not a password field) and `screen::sort_reading_order`
/// puts them in reading order, so "the 3rd button" is the 3rd button the user
/// sees — not the 3rd node in the accessibility tree.
pub fn list_actionables_with_reason() -> Result<Vec<UiElement>, String> {
    let hits = crate::semantic::list_actionables().map_err(|e| e.spoken())?;
    Ok(to_actionable_list(hits))
}

/// Turn semantic hits into the actionable list, in reading order.
///
/// Pure, so the rules are unit-tested without a live accessibility bus.
pub fn to_actionable_list(hits: Vec<crate::semantic::SemanticHit>) -> Vec<UiElement> {
    let kept: Vec<crate::semantic::SemanticHit> =
        hits.into_iter().filter(is_actionable_hit).collect();
    let els: Vec<UiElement> = kept.iter().filter_map(ui_element).collect();
    crate::screen::sort_reading_order(els)
}

/// The shared element shape for one hit, when it has usable geometry.
pub fn ui_element(h: &crate::semantic::SemanticHit) -> Option<UiElement> {
    let b = h.bounds?;
    if !b.is_real() {
        return None;
    }
    Some(UiElement {
        name: h.name.clone(),
        // The role is the Linux equivalent of the Windows UIA
        // control type, so downstream filtering on `kind` works.
        kind: h.role.clone(),
        x: b.x,
        y: b.y,
        w: b.w,
        h: b.h,
    })
}

/// Find an element by name through the semantic cascade.
///
/// The query is scoped to the **observed foreground application**, so it can
/// only ever return a control from the window the user is looking at. When
/// the foreground cannot be identified, nothing is searched and `None` comes
/// back.
///
/// `None` is also the answer when the element was not found, when the tiers
/// could not run, or when the hit carries no usable geometry. A CDP hit has
/// no geometry by construction (its accessibility tree is semantic, not
/// spatial), so it is never reported as a pointing result — the caller falls
/// through to the pixel tier instead.
pub fn find_by_name(query: &str) -> Option<UiElement> {
    if query.trim().is_empty() {
        return None;
    }
    let resolution = crate::semantic::resolve(query, None);
    if !resolution.is_found() {
        return None;
    }
    // `pointing()` is the geometry-aware accessor: a CDP hit names the element
    // but has no rectangle, so it is not something we can point at.
    let hit = resolution.pointing()?;
    let b = hit.bounds?;
    if !b.is_real() {
        tracing::debug!("screen: {query:?} found but has unlaid-out bounds");
        return None;
    }
    Some(UiElement {
        name: hit.name.clone(),
        kind: hit.role.clone(),
        x: b.x,
        y: b.y,
        w: b.w,
        h: b.h,
    })
}

// ─── Capture (xdg-desktop-portal) ─────────────────────────────────────────

/// The xdg-desktop-portal request object path for a `handle_token`.
///
/// The portal derives it from the caller's unique bus name and the token:
/// `/org/freedesktop/portal/desktop/request/<sender>/<token>`, where the
/// sender's unique name has its leading `:` removed and its `.` replaced by
/// `_` (D-Bus object-path elements may only use `[A-Za-z0-9_]`).
///
/// Knowing this path is what makes the capture race-free: the match rule for
/// the `Response` signal can be registered **before** the `Screenshot` call
/// is made, instead of after it returns — by which time the portal may
/// already have answered and the signal would have been dropped.
pub fn portal_request_path(unique_name: &str, handle_token: &str) -> String {
    let sender = unique_name
        .trim_start_matches(':')
        .replace('.', "_")
        .replace(':', "_");
    format!("/org/freedesktop/portal/desktop/request/{sender}/{handle_token}")
}

/// A handle token for one portal request. Unique per process and per call.
fn handle_token() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("nexus{}{}", std::process::id(), n)
}

/// Screenshot via `org.freedesktop.portal.Desktop` (works on X11 and Wayland
/// wherever xdg-desktop-portal runs — GNOME, KDE, COSMIC).
///
/// Returns raw PNG bytes; the portal-owned file is removed afterwards.
///
/// ## The race this fixes
///
/// The obvious implementation calls `Screenshot`, waits for the reply (which
/// carries the request object path), and *then* subscribes to the `Response`
/// signal. The portal can answer between those two steps, and the signal is
/// then never delivered — the capture hangs until the timeout and the user
/// sees a failure that has no cause.
///
/// Here the request path is derived from the `handle_token` and the caller's
/// unique bus name **before** the call, so the match rule is registered first
/// and the signal cannot be missed.
pub async fn portal_screenshot_png() -> Result<Vec<u8>, String> {
    use std::collections::HashMap;
    use std::time::Duration;
    use futures_util::StreamExt;
    use zbus::fdo::DBusProxy;
    use zbus::message::Type as MessageType;
    use zbus::zvariant::{OwnedValue, Value};
    use zbus::{Connection, MatchRule, MessageStream};

    let conn = Connection::session()
        .await
        .map_err(|e| format!("D-Bus session: {e}"))?;
    // Fast-fail when no portal exists (plain X11 without xdg-desktop-portal)
    // instead of burning the timeout below.
    {
        let dbus = DBusProxy::new(&conn)
            .await
            .map_err(|e| format!("D-Bus proxy: {e}"))?;
        let names = dbus
            .list_names()
            .await
            .map_err(|e| format!("D-Bus list: {e}"))?;
        if !names.iter().any(|n| n.as_str() == "org.freedesktop.portal.Desktop") {
            return Err("no screenshot portal found — install xdg-desktop-portal (and a \
                        backend for your desktop) to enable screen vision"
                .into());
        }
    }

    // The token and the request path, before anything is called.
    let token = handle_token();
    let unique_name = dbus_unique_name(&conn).await?;
    let request_path = portal_request_path(&unique_name, &token);

    // The signal stream must exist before the call, and the match rule must be
    // registered before the call — otherwise the portal's answer can arrive
    // before anyone is listening.
    let mut stream = MessageStream::from(&conn);
    // zbus 5 takes the request object path as a `zvariant::ObjectPath`; the
    // owned wrapper is the one that carries a `'static` lifetime, which is
    // what the rule needs (the derived path is a temporary).
    let req_path: zbus::zvariant::OwnedObjectPath = request_path
        .clone()
        .try_into()
        .map_err(|e| format!("bad portal request path: {e}"))?;
    let rule = MatchRule::builder()
        .msg_type(MessageType::Signal)
        .interface("org.freedesktop.portal.Request")
        .map_err(|e| format!("match rule: {e}"))?
        .path(req_path.into_inner())
        .map_err(|e| format!("match rule: {e}"))?
        .member("Response")
        .map_err(|e| format!("match rule: {e}"))?
        .build();
    // Server-side match registration is internal-only in zbus 5, so register
    // via the standard org.freedesktop.DBus AddMatch call (the same
    // DBusProxy pattern as mpris.rs). Without this the portal's Response
    // signal is never delivered to our connection.
    let dbus = DBusProxy::new(&conn)
        .await
        .map_err(|e| format!("D-Bus proxy: {e}"))?;
    dbus.add_match_rule(rule)
        .await
        .map_err(|e| format!("match register: {e}"))?;

    let mut opts: HashMap<&str, Value> = HashMap::new();
    opts.insert("handle_token", Value::from(token.as_str()));
    // `interactive: false` means the portal takes the shot without asking the
    // user to confirm. An interactive portal request would block on a consent
    // dialog that a voice assistant cannot answer, and would then time out.
    opts.insert("interactive", Value::from(false));

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

    // The reply carries the request object path. It is only used to report a
    // mismatch with the derived path, so a decode failure is not fatal.
    let answered_path = reply
        .body()
        .deserialize::<zbus::zvariant::OwnedObjectPath>()
        .map(|p| p.as_str().to_string())
        .unwrap_or_default();
    if !answered_path.is_empty() && answered_path != request_path {
        // The portal answered with a different object path than the one the
        // convention predicts. The match rule was registered for the derived
        // path, so the signal may not reach us — say so plainly instead of
        // timing out with no explanation.
        tracing::warn!(
            "portal: request path mismatch (derived {request_path}, portal said {answered_path})"
        );
    }

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
    let uri = String::try_from(uri_v.clone()).map_err(|e| format!("bad uri: {e}"))?;
    let raw = uri.strip_prefix("file://").ok_or("portal uri is not a file")?;
    let path = percent_decode(raw);
    let bytes = std::fs::read(&path).map_err(|e| format!("read screenshot file: {e}"))?;
    let _ = std::fs::remove_file(&path); // best-effort cleanup
    if bytes.is_empty() {
        return Err("screenshot file was empty".into());
    }
    Ok(bytes)
}

/// Our own unique bus name, via `org.freedesktop.DBus.GetId`.
///
/// Needed to derive the portal's request object path. Asked over the raw
/// method call so it does not depend on any proxy helper's shape.
async fn dbus_unique_name(conn: &zbus::Connection) -> Result<String, String> {
    let reply = conn
        .call_method(
            Some("org.freedesktop.DBus"),
            "/org/freedesktop/DBus",
            Some("org.freedesktop.DBus"),
            "GetId",
            &(),
        )
        .await
        .map_err(|e| format!("D-Bus GetId: {e}"))?;
    reply
        .body()
        .deserialize::<String>()
        .map_err(|e| format!("D-Bus GetId: {e}"))
}

/// Minimal %XX decoder for file:// URIs (avoids a new crate).
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

fn hex_val(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

/// Capture the screen as PNG bytes, from a synchronous context.
///
/// The portal round trip is asynchronous, and the capture helpers that call
/// this are synchronous and are themselves called from inside async code.
/// Nesting a runtime inside a runtime is not allowed, so the work happens on
/// its own thread — which is safe from any context.
pub fn capture_png_blocking() -> Result<Vec<u8>, String> {
    std::thread::spawn(|| {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| format!("runtime: {e}"))?;
        rt.block_on(portal_screenshot_png())
    })
    .join()
    .map_err(|_| "the capture thread panicked".to_string())?
}

/// Capture the screen and return it as a JPEG, downscaled to `max_w`.
///
/// The whole screen is captured (the portal has no region option) and the
/// region is cropped locally afterwards.
///
/// `grid` overlays the axis grid the vision model reads coordinates off.
///
/// **The pre-capture privacy gate runs here**, not at the call site: this is
/// the only function that produces pixels on Linux, so putting the gate
/// inside it means no path can reach the screen without passing it. An
/// unidentified foreground refuses the capture — it does not fall back to a
/// full-screen grab, because "I could not tell which window this is" is
/// exactly the case the gate exists for.
pub fn capture_jpeg_base64(
    region: Option<(i32, i32, i32, i32)>,
    max_w: u32,
    grid: bool,
) -> Option<(String, i32, i32)> {
    let gate = crate::pointer::capture_gate_now();
    if !gate.allows_capture() {
        tracing::warn!(
            "screen: capture refused by the privacy gate — {}",
            gate.spoken()
        );
        return None;
    }

    let png = capture_png_blocking().ok()?;
    let img = image::load_from_memory(&png).ok()?.to_rgba8();
    let (full_w, full_h) = (img.width() as i32, img.height() as i32);

    let cropped = match region {
        Some((x, y, w, h)) if w > 0 && h > 0 => {
            let x = x.max(0).min(full_w);
            let y = y.max(0).min(full_h);
            let w = w.max(0).min(full_w - x);
            let h = h.max(0).min(full_h - y);
            if w <= 0 || h <= 0 {
                img
            } else {
                image::imageops::crop_imm(&img, x as u32, y as u32, w as u32, h as u32).to_image()
            }
        }
        _ => img,
    };
    let (cw, ch) = (cropped.width(), cropped.height());
    if cw == 0 || ch == 0 {
        return None;
    }
    let scale = (max_w as f32 / cw as f32).min(1.0);
    let (tw, th) = (
        ((cw as f32 * scale).round() as u32).max(1),
        ((ch as f32 * scale).round() as u32).max(1),
    );
    let small = if tw == cw && th == ch {
        cropped
    } else {
        image::imageops::resize(&cropped, tw, th, image::imageops::FilterType::Triangle)
    };
    let final_img = if grid {
        crate::vision::overlay_axis_grid(small)
    } else {
        small
    };
    let mut jpeg = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg, 60)
        .encode_image(&image::DynamicImage::ImageRgba8(final_img))
        .ok()?;
    use base64::Engine;
    Some((
        base64::engine::general_purpose::STANDARD.encode(&jpeg),
        full_w,
        full_h,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_mode_line() {
        assert_eq!(parse_mode_line("1920x1080"), Some((1920, 1080)));
        assert_eq!(parse_mode_line("2560x1440"), Some((2560, 1440)));
        assert_eq!(parse_mode_line("3840x2160\n"), Some((3840, 2160)));
        assert_eq!(parse_mode_line("invalid"), None);
        assert_eq!(parse_mode_line("0x0"), None);
    }

    #[test]
    fn test_primary_monitor_size_fallback() {
        let size = primary_monitor_size();
        assert!(size.is_some());
        let (w, h) = size.unwrap();
        assert!(w > 0 && h > 0);
    }

    // ── The actionable list: same contract as the Windows one ─────────────

    fn hit(name: &str, role: &str, x: i32, y: i32, states: &[&str]) -> crate::semantic::SemanticHit {
        crate::semantic::SemanticHit {
            name: name.into(),
            role: role.into(),
            app: "test-app".into(),
            bounds: Some(crate::atspi::AtspiBounds { x, y, w: 80, h: 30 }),
            tier: crate::semantic::Tier::AtSpi,
            actions: vec!["click".into()],
            states: states.iter().map(|s| s.to_string()).collect(),
            chromium_skeleton_risk: false,
        }
    }

    #[test]
    fn the_list_is_in_reading_order_not_tree_order() {
        // "The 3rd button" must be the 3rd button the user SEES, which is the
        // contract in screen.rs — the AT-SPI walk is in tree order, which is
        // not the same thing.
        let els = to_actionable_list(vec![
            hit("third", "push button", 10, 200, &["enabled", "sensitive", "showing"]),
            hit("first", "push button", 10, 10, &["enabled", "sensitive", "showing"]),
            hit("second", "push button", 10, 100, &["enabled", "sensitive", "showing"]),
        ]);
        let names: Vec<&str> = els.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, vec!["first", "second", "third"]);
    }

    #[test]
    fn same_row_is_ordered_left_to_right() {
        // Both elements sit in the SAME 12px row bucket (`y / 12`), which is
        // what `sort_reading_order` uses: two items a few pixels apart land in
        // different buckets when they straddle a bucket boundary, and the
        // order is then by row, not by x.
        let els = to_actionable_list(vec![
            hit("right", "push button", 300, 10, &["enabled", "sensitive", "showing"]),
            hit("left", "push button", 10, 11, &["enabled", "sensitive", "showing"]),
        ]);
        assert_eq!(els[0].name, "left");
        assert_eq!(els[1].name, "right");
    }

    #[test]
    fn a_credential_field_is_never_offered() {
        // The server offers every editable node, and a password entry IS
        // editable. The policy layer knows it by its ROLE.
        let els = to_actionable_list(vec![
            hit("Password", "password text", 10, 10, &["enabled", "sensitive", "showing"]),
            hit("Save", "push button", 10, 100, &["enabled", "sensitive", "showing"]),
        ]);
        assert_eq!(els.len(), 1);
        assert_eq!(els[0].name, "Save");
    }

    #[test]
    fn a_disabled_element_is_not_offered() {
        // No STATE_ENABLED / STATE_SENSITIVE means the widget is insensitive.
        let els = to_actionable_list(vec![
            hit("Save", "push button", 10, 10, &["showing"]),
            hit("Cancel", "push button", 10, 100, &["enabled", "sensitive", "showing"]),
        ]);
        assert_eq!(els.len(), 1);
        assert_eq!(els[0].name, "Cancel");
    }

    #[test]
    fn an_element_that_is_visible_but_not_shown_is_not_offered() {
        // "Visible but not showing" is the ONLY off-screen verdict: laid out,
        // but not rendered (a collapsed container, a clipped viewport). It
        // cannot be clicked, and offering it as "the 3rd option" sends the
        // pointer into the void.
        let els = to_actionable_list(vec![
            hit("Clipped", "push button", 10, 10, &["enabled", "sensitive", "visible"]),
            hit("Shown", "push button", 10, 100, &["enabled", "sensitive", "visible", "showing"]),
        ]);
        assert_eq!(els.len(), 1);
        assert_eq!(els[0].name, "Shown");
    }

    #[test]
    fn an_element_whose_visibility_is_unknown_is_kept() {
        // A toolkit that implements neither STATE_VISIBLE nor STATE_SHOWING
        // says NOTHING about visibility. Treating that as "not on screen"
        // would empty the whole list and read as "this window has no
        // controls" — so the verdict stays unknown and the element is kept.
        let els = to_actionable_list(vec![
            hit("Save", "push button", 10, 10, &["enabled", "sensitive"]),
        ]);
        assert_eq!(els.len(), 1);
        assert_eq!(els[0].name, "Save");
    }

    #[test]
    fn an_element_with_no_reported_states_is_not_judged_off_screen() {
        // A toolkit that reports nothing must not disqualify every element.
        let els = to_actionable_list(vec![
            hit("Save", "push button", 10, 10, &[]),
        ]);
        assert_eq!(els.len(), 1);
        assert_eq!(els[0].name, "Save");
    }

    #[test]
    fn the_off_screen_verdict_needs_a_positive_signal() {
        // Pure: only "visible and not showing" is off screen.
        let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<String>>();
        assert!(is_offscreen(&s(&["visible"])));
        assert!(!is_offscreen(&s(&["showing"])));
        assert!(!is_offscreen(&s(&["visible", "showing"])));
        // Neither reported: unknown, so not off screen.
        assert!(!is_offscreen(&s(&["enabled"])));
        assert!(!is_offscreen(&s(&[])));
    }

    #[test]
    fn an_element_without_geometry_is_not_offered() {
        let mut h = hit("Save", "push button", 10, 10, &["enabled", "sensitive", "showing"]);
        h.bounds = None;
        assert!(to_actionable_list(vec![h]).is_empty());
    }

    // ── The portal request path ───────────────────────────────────────────

    #[test]
    fn request_path_follows_the_portal_convention() {
        // The sender's unique name has its leading ':' removed and its '.'
        // replaced by '_' — D-Bus object-path elements may only use
        // [A-Za-z0-9_], so the raw unique name is not a legal path element.
        assert_eq!(
            portal_request_path(":1.42", "nexus123"),
            "/org/freedesktop/portal/desktop/request/1_42/nexus123"
        );
    }

    #[test]
    fn request_path_is_legal_for_every_unique_name() {
        for name in [":1.42", ":1.1234", ":2.7"] {
            let p = portal_request_path(name, "nexus1");
            for element in p.trim_start_matches('/').split('/') {
                assert!(
                    !element.is_empty()
                        && element
                            .chars()
                            .all(|c| c.is_ascii_alphanumeric() || c == '_'),
                    "illegal object-path element {element:?} in {p}"
                );
            }
        }
    }

    #[test]
    fn request_path_keeps_the_token() {
        assert!(portal_request_path(":1.2", "nexus99").ends_with("/nexus99"));
    }

    #[test]
    fn handle_tokens_are_unique() {
        let a = handle_token();
        let b = handle_token();
        assert_ne!(a, b);
        assert!(a.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'));
    }

    // ── Percent decoding ──────────────────────────────────────────────────

    #[test]
    fn percent_decoding_handles_spaces_and_plain_text() {
        assert_eq!(percent_decode("a%20b"), "a b");
        assert_eq!(percent_decode("plain"), "plain");
        assert_eq!(percent_decode("%2Ftmp%2Fx.png"), "/tmp/x.png");
    }
}
