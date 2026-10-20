//! CDP tier client — semantic perception and actuation for Chromium/Electron.
//!
//! # Why this exists
//!
//! AT-SPI is the right tier for GTK/Qt and useless for Chromium/Electron, which
//! expose only an `application -> frame` skeleton over AT-SPI unless launched
//! with `--force-renderer-accessibility`. Measured on this machine:
//!
//! ```text
//! VS Code via AT-SPI .... 1 node
//! VS Code via CDP ....... 1026 nodes
//! ```
//!
//! One versus a thousand. Chromium/Electron is where a developer's time is spent
//! — VS Code, Chrome, Slack, Discord — so without this tier the accessibility
//! path covers most of the desktop but none of the apps that matter most.
//!
//! # Why it is worth having over pixels
//!
//! The target is addressed by *identity*: an accessible name plus a role, which
//! resolves to a `backendDOMNodeId`, which resolves to a live JavaScript object.
//! A layout shift cannot invalidate that, and an unrelated element cannot be
//! substituted at the same coordinates. See `docs/features/research/20`.
//!
//! # Launch requirements, which are not optional
//!
//! * Chromium (Chrome/Brave/Edge): `--remote-debugging-port=<port>`.
//! * Electron (VS Code, Discord, Slack): `--inspect=<port>`. **Not**
//!   `--remote-debugging-port` — shipped Electron has an authenticated-CDP gate
//!   that terminates the app when that flag is on argv.
//!
//! An already-running Electron app cannot be attached after the fact; the flag is
//! a launch-time decision. This client reports "nothing reachable" rather than
//! pretending, and the caller falls back to the pixel tier.

use serde::Deserialize;
use std::time::Duration;

/// A resolved element, addressed by DOM node identity rather than by pixel.
#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct CdpNode {
    #[serde(default)]
    pub port: u16,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub role: String,
    #[serde(default)]
    pub backend_dom_node_id: u64,
    /// True when the read was cut off by the server's node budget.
    #[serde(default)]
    pub truncated: bool,
}

/// Result of a click. The server reports whether `Runtime.callFunctionOn`
/// actually threw, so a silent no-op cannot be mistaken for success.
#[derive(Debug, Clone, Deserialize)]
pub struct CdpEffect {
    #[serde(default)]
    pub ok: bool,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub role: String,
    /// The node threw during `this.click()`. Distinct from a transport failure.
    #[serde(default)]
    pub threw: bool,
    #[serde(default)]
    pub observed: Option<String>,
}

/// Which debug endpoints are live.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct CdpHealth {
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub direct_ports: Vec<u16>,
    #[serde(default)]
    pub electron_ports: Vec<u16>,
    #[serde(default)]
    pub note: String,
}

impl CdpHealth {
    /// Whether any usable endpoint exists.
    pub fn any(&self) -> bool {
        !self.direct_ports.is_empty() || !self.electron_ports.is_empty()
    }
    pub fn is_idle(&self) -> bool {
        !self.any()
    }
}

fn client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(20))
        .build()
        .map_err(|e| format!("http client: {e}"))
}

fn base() -> String {
    format!("http://127.0.0.1:{}", crate::lazy_cdp::cdp_server_port())
}

/// Which endpoints are reachable. Cheap; safe to call before deciding to try.
pub async fn health() -> Result<CdpHealth, String> {
    crate::lazy_cdp::ensure_cdp_running();
    crate::lazy_cdp::mark_cdp_request();
    let resp = client()?
        .get(format!("{}/health", base()))
        .send()
        .await
        .map_err(|e| format!("cdp health failed: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("cdp HTTP {}", resp.status()));
    }
    resp.json()
        .await
        .map_err(|e| format!("cdp health parse: {e}"))
}

/// Resolve an element by accessible name, optionally constrained by role and port.
pub async fn find(
    name: &str,
    role: Option<&str>,
    port: Option<u16>,
) -> Result<Option<CdpNode>, String> {
    if name.trim().is_empty() {
        return Err("empty name".into());
    }
    crate::lazy_cdp::ensure_cdp_running();
    crate::lazy_cdp::mark_cdp_request();

    let mut q = vec![("name", name.to_string())];
    if let Some(r) = role {
        q.push(("role", r.to_string()));
    }
    if let Some(p) = port {
        q.push(("port", p.to_string()));
    }

    let resp = client()?
        .get(format!("{}/find", base()))
        .query(&q)
        .send()
        .await
        .map_err(|e| format!("cdp find failed: {e}"))?;

    match resp.status().as_u16() {
        200 => {}
        // "not found" and "no endpoint" are different answers, and the caller
        // needs to tell them apart: the first means try another tier, the second
        // means nothing is controllable here at all.
        404 => return Ok(None),
        503 => return Err("no CDP endpoint reachable (app not launched with a debug port)".into()),
        502 => return Err("CDP transport failure".into()),
        s => return Err(format!("cdp HTTP {s}")),
    }

    let v: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("cdp find parse: {e}"))?;
    if let Some(err) = v.get("error").and_then(|e| e.as_str()) {
        return Err(format!("cdp: {err}"));
    }
    serde_json::from_value(v)
        .map(Some)
        .map_err(|e| format!("cdp node decode: {e}"))
}

/// Click an element by accessible name. No pixels, no compositor, no portal.
pub async fn activate(name: &str, role: Option<&str>, port: Option<u16>) -> Result<CdpEffect, String> {
    crate::lazy_cdp::ensure_cdp_running();
    crate::lazy_cdp::mark_cdp_request();

    let mut body = serde_json::json!({ "name": name });
    if let Some(r) = role {
        body["role"] = serde_json::json!(r);
    }
    if let Some(p) = port {
        body["port"] = serde_json::json!(p);
    }

    let resp = client()?
        .post(format!("{}/activate", base()))
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("cdp activate failed: {e}"))?;

    match resp.status().as_u16() {
        200 => {}
        404 => return Err(format!("no clickable element named {name:?}")),
        503 => return Err("no CDP endpoint reachable".into()),
        s => return Err(format!("cdp HTTP {s}")),
    }

    let v: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("cdp effect parse: {e}"))?;
    let effect: CdpEffect = serde_json::from_value(v)
        .map_err(|e| format!("cdp effect decode: {e}"))?;

    // A node that threw is a failed action, not a successful one.
    if effect.threw {
        return Err(format!("clicking {name:?} threw in the page"));
    }
    Ok(effect)
}

/// Set an input's value through the prototype's native setter.
///
/// Assigning `.value` directly does not notify React/Vue, which track their own
/// state — the value would look set and the framework would ignore it.
pub async fn fill(name: Option<&str>, text: &str, port: Option<u16>) -> Result<bool, String> {
    crate::lazy_cdp::ensure_cdp_running();
    crate::lazy_cdp::mark_cdp_request();

    let mut body = serde_json::json!({ "text": text });
    if let Some(n) = name {
        body["name"] = serde_json::json!(n);
    }
    if let Some(p) = port {
        body["port"] = serde_json::json!(p);
    }

    let resp = client()?
        .post(format!("{}/fill", base()))
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("cdp fill failed: {e}"))?;

    match resp.status().as_u16() {
        200 => {}
        404 => return Err("no matching input field".into()),
        503 => return Err("no CDP endpoint reachable".into()),
        s => return Err(format!("cdp HTTP {s}")),
    }

    let v: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("cdp fill parse: {e}"))?;
    if let Some(err) = v.get("error").and_then(|e| e.as_str()) {
        return Err(format!("cdp: {err}"));
    }
    Ok(v.get("ok").and_then(|o| o.as_bool()).unwrap_or(false))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn health_defaults_are_empty_not_connected() {
        // A default-constructed health must read as "nothing reachable" so a
        // failed probe cannot be mistaken for a working tier.
        let h = CdpHealth::default();
        assert!(!h.any());
        assert!(h.is_idle());
    }

    #[test]
    fn health_recognises_either_transport() {
        let d = CdpHealth { direct_ports: vec![9222], ..Default::default() };
        assert!(d.any() && !d.is_idle());
        let e = CdpHealth { electron_ports: vec![9229], ..Default::default() };
        assert!(e.any() && !e.is_idle());
    }

    #[test]
    fn node_decodes_with_snake_case_dom_id() {
        // The server emits backendDOMNodeId; serde maps it via rename_all.
        let raw = r#"{"port":9229,"name":"Open Quick Access","role":"button",
                      "backend_dom_node_id":210,"truncated":false}"#;
        let n: CdpNode = serde_json::from_str(raw).expect("decode");
        assert_eq!(n.name, "Open Quick Access");
        assert_eq!(n.role, "button");
        assert_eq!(n.backend_dom_node_id, 210);
        assert!(!n.truncated);
    }

    #[test]
    fn node_decodes_when_server_reports_zero_node_id() {
        // A node with no backendDOMNodeId is not actionable and must not look
        // like one that is.
        let n: CdpNode = serde_json::from_str(r#"{"name":"x","role":"StaticText"}"#)
            .expect("decode");
        assert_eq!(n.backend_dom_node_id, 0);
    }

    #[test]
    fn effect_flags_a_thrown_click() {
        let ok: CdpEffect =
            serde_json::from_str(r#"{"ok":true,"name":"Go","role":"button","threw":false}"#)
                .expect("decode");
        assert!(ok.ok && !ok.threw);
        let bad: CdpEffect =
            serde_json::from_str(r#"{"ok":true,"name":"Go","role":"button","threw":true}"#)
                .expect("decode");
        assert!(bad.threw, "a throwing click must be visible to the caller");
    }

    #[test]
    fn find_rejects_empty_name_locally() {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");
        assert!(rt.block_on(find("  ", None, None)).is_err());
    }
}
