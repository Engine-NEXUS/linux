//! AT-SPI semantic perception and actuation client.
//!
//! The tier that makes NEXUS able to control a native Wayland session.
//!
//! # Why this exists
//!
//! Wayland forbids *synthetic input*. It does not forbid an application from
//! activating its own button. So `org.a11y.atspi.Action.do_action` and
//! `org.a11y.atspi.EditableText.set_text_contents` work on Wayland with no
//! portal, no `/dev/uinput`, no `input` group and no consent dialog — verified on
//! this machine before this client was written. See
//! `docs/features/research/20-…` for the architecture and `21-…` for the channel
//! comparison.
//!
//! That inverts the usual ordering. Rather than screenshot → reason → pixel
//! click, this resolves a target by *identity* (accessible name + role) and acts
//! on it directly:
//!
//! ```text
//! semantic resolve → activate/fill        (exact, and Wayland-safe)
//!                  → libei/portal         (only if semantics cannot cover it)
//!                  → pixel + Set-of-Mark  (last resort)
//! ```
//!
//! # Identity, not coordinates
//!
//! [`AtspiNode`] carries the target's accessible name, role and geometry rather
//! than a bare point. That matters for correctness, not elegance: a layout shift
//! does not invalidate a node reference, and re-resolving the name immediately
//! before dispatch is what makes it impossible for an unrelated element to be
//! substituted at the same pixels.
//!
//! # Failure is reported, not assumed
//!
//! Every call distinguishes "resolved to nothing" from "could not resolve", and
//! a 504 search timeout never surfaces as a 404 not-found. `docs/features/research/22`
//! documented the class of bug where a tier quietly reported success (or quietly
//! reported failure) and the user was told the opposite.

use serde::Deserialize;
use std::time::Duration;

/// A resolved accessible. This is a *reference to an element*, not a point.
#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct AtspiNode {
    #[serde(default)]
    pub app: String,
    #[serde(default)]
    pub role: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub bounds: Option<AtspiBounds>,
    #[serde(default)]
    pub states: Vec<String>,
    #[serde(default)]
    pub actions: Vec<String>,
    /// True when the app is Chromium/Electron, whose AT-SPI tree is a skeleton
    /// unless launched with `--force-renderer-accessibility`. VS Code measures at
    /// 1 node without the flag and 140 with it, so a miss here is expected
    /// rather than a fault — the CDP tier is the answer for those apps.
    #[serde(default)]
    pub chromium_skeleton_risk: bool,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq)]
pub struct AtspiBounds {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl AtspiBounds {
    /// Centre point, for callers that must fall back to a coordinate.
    pub fn center(&self) -> (i32, i32) {
        (self.x + self.w / 2, self.y + self.h / 2)
    }
    /// Whether the toolkit has actually laid this out.
    ///
    /// GTK reports `INT_MIN` (-2147483648) as a "not yet positioned" sentinel.
    /// Those values must never reach an actuator, which is why the server
    /// filters them and this re-checks.
    pub fn is_real(&self) -> bool {
        self.w > 0 && self.h > 0 && self.x > -1000 && self.y > -1000
    }
}

/// Result of an activation. Carries the post-state so the caller gets a
/// condition rather than taking the return value on faith.
#[derive(Debug, Clone, Deserialize)]
pub struct AtspiEffect {
    #[serde(default)]
    pub ok: bool,
    #[serde(default)]
    pub app: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub action: String,
    #[serde(default)]
    pub states_before: Vec<String>,
    #[serde(default)]
    pub states_after: Vec<String>,
    #[serde(default)]
    pub states_changed: Vec<String>,
}

/// Outcome of identifying the foreground window.
///
/// `Known` and `Unknown` are deliberately distinct. Collapsing them is the bug
/// `docs/features/research/22` listed as severity 1: the privacy gate returned
/// "no match" for "I could not tell", and bank windows got screenshotted.
#[derive(Debug, Clone, PartialEq)]
pub enum Foreground {
    Known {
        app: String,
        title: String,
    },
    /// No mechanism available, or nothing reported as active.
    Unknown { reason: String },
}

impl Foreground {
    /// Title when known, else `None`. Callers must treat `None` as "refuse".
    pub fn title(&self) -> Option<&str> {
        match self {
            Foreground::Known { title, .. } => Some(title.as_str()),
            Foreground::Unknown { .. } => None,
        }
    }
    pub fn is_known(&self) -> bool {
        matches!(self, Foreground::Known { .. })
    }
}

fn client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(20))
        .build()
        .map_err(|e| format!("http client: {e}"))
}

fn base() -> String {
    format!("http://127.0.0.1:{}", crate::lazy_atspi::atspi_port())
}

/// Resolve an accessible by name, optionally constrained to one app.
///
/// `Err` means the tier could not answer. `Ok(None)` means it answered and there
/// is no such element. Keeping those apart is the whole point.
pub async fn find(name: &str, app: Option<&str>, role: Option<&str>) -> Result<Option<AtspiNode>, String> {
    if name.trim().is_empty() {
        return Err("empty name".into());
    }
    crate::lazy_atspi::ensure_atspi_running();
    crate::lazy_atspi::mark_atspi_request();

    let mut q = vec![("name", name.to_string())];
    if let Some(a) = app {
        q.push(("app", a.to_string()));
    }
    if let Some(r) = role {
        q.push(("role", r.to_string()));
    }

    let resp = client()?
        .get(format!("{}/find", base()))
        .query(&q)
        .send()
        .await
        .map_err(|e| format!("atspi request failed: {e}"))?;

    match resp.status().as_u16() {
        200 => {}
        404 => return Ok(None),
        504 => return Err("atspi search timed out — pass a specific app".into()),
        s => return Err(format!("atspi HTTP {s}")),
    }

    let v: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("atspi parse: {e}"))?;
    if let Some(err) = v.get("error").and_then(|e| e.as_str()) {
        return Err(format!("atspi: {err}"));
    }
    serde_json::from_value(v)
        .map(Some)
        .map_err(|e| format!("atspi node decode: {e}"))
}

/// Invoke an element's action. No input synthesis is performed.
pub async fn activate(
    name: &str,
    app: Option<&str>,
    role: Option<&str>,
    action: &str,
) -> Result<AtspiEffect, String> {
    crate::lazy_atspi::ensure_atspi_running();
    crate::lazy_atspi::mark_atspi_request();

    let mut body = serde_json::json!({ "name": name, "action": action });
    if let Some(a) = app {
        body["app"] = serde_json::json!(a);
    }
    if let Some(r) = role {
        body["role"] = serde_json::json!(r);
    }

    let resp = client()?
        .post(format!("{}/activate", base()))
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("atspi activate failed: {e}"))?;

    match resp.status().as_u16() {
        200 => {}
        404 => return Err(format!("no element named {name:?}")),
        // 409 is the honest "this element has no such action" — the target
        // exists but cannot be actuated. Distinct from 404 on purpose.
        409 => {
            let v: serde_json::Value = resp.json().await.unwrap_or(serde_json::Value::Null);
            let available = v
                .get("available")
                .and_then(|a| a.as_array())
                .map(|a| {
                    a.iter()
                        .filter_map(|x| x.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                })
                .unwrap_or_default();
            return Err(format!(
                "element {name:?} cannot be activated (available: {available})"
            ));
        }
        504 => return Err("atspi search timed out — pass a specific app".into()),
        s => return Err(format!("atspi HTTP {s}")),
    }

    let v: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("atspi effect parse: {e}"))?;
    serde_json::from_value(v).map_err(|e| format!("atspi effect decode: {e}"))
}

/// Set an editable field's contents. No keyboard synthesis is performed.
///
/// Fails on Chromium/Electron, which never expose `EditableText` — the server
/// reports that as 409 rather than pretending.
pub async fn fill(name: Option<&str>, app: Option<&str>, text: &str) -> Result<AtspiEffect, String> {
    crate::lazy_atspi::ensure_atspi_running();
    crate::lazy_atspi::mark_atspi_request();

    let mut body = serde_json::json!({ "text": text });
    if let Some(n) = name {
        body["name"] = serde_json::json!(n);
    }
    if let Some(a) = app {
        body["app"] = serde_json::json!(a);
    }

    let resp = client()?
        .post(format!("{}/fill", base()))
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("atspi fill failed: {e}"))?;

    match resp.status().as_u16() {
        200 => {}
        404 => return Err("no editable field matched".into()),
        409 => return Err("target has no EditableText (Chromium/Electron) — use the CDP tier".into()),
        504 => return Err("atspi search timed out — pass a specific app".into()),
        s => return Err(format!("atspi HTTP {s}")),
    }

    let v: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("atspi fill parse: {e}"))?;
    serde_json::from_value(v).map_err(|e| format!("atspi fill decode: {e}"))
}

/// Identify the foreground window, for the privacy exclusion gate.
pub async fn focused() -> Result<Foreground, String> {
    crate::lazy_atspi::ensure_atspi_running();
    crate::lazy_atspi::mark_atspi_request();

    let resp = client()?
        .get(format!("{}/focused", base()))
        .send()
        .await
        .map_err(|e| format!("atspi focused failed: {e}"))?;

    match resp.status().as_u16() {
        200 => {}
        503 => return Ok(Foreground::Unknown { reason: "tier unavailable".into() }),
        s => return Err(format!("atspi HTTP {s}")),
    }

    let v: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("atspi focused parse: {e}"))?;

    if v.get("known").and_then(|k| k.as_bool()).unwrap_or(false) {
        Ok(Foreground::Known {
            app: v.get("app").and_then(|x| x.as_str()).unwrap_or_default().to_string(),
            title: v.get("title").and_then(|x| x.as_str()).unwrap_or_default().to_string(),
        })
    } else {
        Ok(Foreground::Unknown {
            reason: v
                .get("reason")
                .and_then(|x| x.as_str())
                .unwrap_or("unspecified")
                .to_string(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounds_centre_and_sentinel_filter() {
        let b = AtspiBounds { x: 20, y: 57, w: 420, h: 34 };
        assert_eq!(b.center(), (230, 74));
        assert!(b.is_real());

        // GTK's "not yet laid out" sentinel must be rejected, not actuated.
        let unset = AtspiBounds { x: -2147483648, y: -2147483648, w: 1, h: 1 };
        assert!(!unset.is_real());

        let zero = AtspiBounds { x: 0, y: 0, w: 0, h: 0 };
        assert!(!zero.is_real());
    }

    #[test]
    fn foreground_known_exposes_title() {
        let f = Foreground::Known { app: "code".into(), title: "a.rs — nexus".into() };
        assert!(f.is_known());
        assert_eq!(f.title(), Some("a.rs — nexus"));
    }

    #[test]
    fn foreground_unknown_exposes_no_title() {
        // The critical distinction: Unknown must not be convertible to a title,
        // so a caller cannot accidentally treat it as "not excluded".
        let f = Foreground::Unknown { reason: "no a11y bus".into() };
        assert!(!f.is_known());
        assert_eq!(f.title(), None);
    }

    #[test]
    fn foreground_unknown_is_not_equal_to_a_known_empty_title() {
        // A window genuinely titled "" is different from "could not tell".
        let empty = Foreground::Known { app: String::new(), title: String::new() };
        let unknown = Foreground::Unknown { reason: String::new() };
        assert_ne!(empty, unknown);
    }

    #[test]
    fn node_decodes_from_server_shape() {
        let raw = r#"{
            "app":"code","role":"button","name":"Confirm",
            "bounds":{"x":10,"y":20,"w":100,"h":30},
            "states":["sensitive"],"actions":["doDefault"],
            "chromium_skeleton_risk":true
        }"#;
        let n: AtspiNode = serde_json::from_str(raw).expect("decode");
        assert_eq!(n.name, "Confirm");
        assert_eq!(n.role, "button");
        assert!(n.chromium_skeleton_risk);
        assert_eq!(n.bounds.unwrap().center(), (60, 35));
    }

    #[test]
    fn node_decodes_with_missing_optional_fields() {
        // The server may omit bounds for an unlaid-out element; that must not
        // fail the decode, or a locate would error instead of reporting "no
        // geometry".
        let n: AtspiNode = serde_json::from_str(r#"{"name":"x"}"#).expect("decode");
        assert!(n.bounds.is_none());
        assert!(n.actions.is_empty());
        assert!(!n.chromium_skeleton_risk);
    }

    #[test]
    fn effect_decodes_with_post_state() {
        let raw = r#"{"ok":true,"name":"Confirm","action":"doDefault",
            "states_before":[],"states_after":["pressed"],"states_changed":["pressed"]}"#;
        let e: AtspiEffect = serde_json::from_str(raw).expect("decode");
        assert!(e.ok);
        assert_eq!(e.states_changed, vec!["pressed".to_string()]);
    }

    #[test]
    fn find_rejects_empty_name_without_a_request() {
        // Guard: an empty name used to mean "search everything", which is the
        // unbounded walk that hung.
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");
        let r = rt.block_on(find("   ", None, None));
        assert!(r.is_err(), "empty name must be rejected locally");
    }
}
