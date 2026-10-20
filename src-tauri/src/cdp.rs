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
    /// The server speaks CDP's own spelling, `backendDOMNodeId`. Serde's
    /// `camelCase` would produce `backendDomNodeId`, which is a different
    /// string — so the rename is explicit, and the snake_case spelling is
    /// kept as an alias for callers that build the JSON themselves.
    #[serde(default, rename = "backendDOMNodeId", alias = "backend_dom_node_id")]
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
    /// The identity of the endpoint that acted. The server reports it so the
    /// caller can check that it really was the application in front.
    #[serde(default)]
    pub identity: Option<CdpEndpointIdentity>,
}

/// The identity of a CDP endpoint, as the server reports it.
#[derive(Debug, Clone, Deserialize, PartialEq, Default)]
pub struct CdpEndpointIdentity {
    /// "chromium" or "electron" — the server knows which kind of endpoint it
    /// is (an Electron inspector port answers to the Node main process, a
    /// direct port to a page target).
    #[serde(default)]
    pub kind: String,
    /// The application the endpoint belongs to ("brave", "Visual Studio Code").
    #[serde(default)]
    pub app: String,
    /// The target's window/page title.
    #[serde(default)]
    pub title: String,
    /// The target's URL.
    #[serde(default)]
    pub url: String,
}

/// What the caller believes is in front.
///
/// The CDP tier is scoped by DEBUG PORT, not by application: it sees
/// whichever app holds the lowest reachable port. These two fields let the
/// server refuse an endpoint that does not belong to the application the user
/// is actually looking at.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CdpExpectation {
    /// The observed foreground application.
    pub app: String,
    /// The observed foreground window title.
    pub title: String,
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
///
/// `expect` names the application and window title the caller believes are in
/// front. The server then acts ONLY on a debug port that PROVES it belongs to
/// that application, and refuses with 409 when none does — so a click meant
/// for the window in front can never land in a background app that happens to
/// hold a debug port.
pub async fn activate(
    name: &str,
    role: Option<&str>,
    port: Option<u16>,
    expect: Option<&CdpExpectation>,
) -> Result<CdpEffect, String> {
    crate::lazy_cdp::ensure_cdp_running();
    crate::lazy_cdp::mark_cdp_request();

    let mut body = serde_json::json!({ "name": name });
    if let Some(r) = role {
        body["role"] = serde_json::json!(r);
    }
    if let Some(p) = port {
        body["port"] = serde_json::json!(p);
    }
    if let Some(e) = expect {
        body["app"] = serde_json::json!(e.app);
        body["title"] = serde_json::json!(e.title);
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
        409 => {
            // The endpoint could not prove it belongs to the application in
            // front. This is a refusal, not a transport failure. The server
            // says WHY in the body (no candidate proved the focus, the
            // endpoint belongs to another program, ...) and that reason is
            // what the caller needs, so it is surfaced instead of a generic
            // message.
            let why = resp
                .json::<serde_json::Value>()
                .await
                .ok()
                .and_then(|v| {
                    v.get("reason")
                        .and_then(|r| r.as_str())
                        .map(str::to_string)
                })
                .filter(|s| !s.trim().is_empty())
                .unwrap_or_else(|| {
                    "no reachable debug endpoint proves it is the application in front"
                        .to_string()
                });
            return Err(format!("{why} ({name:?})"));
        }
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
    fn node_decodes_with_the_servers_dom_id_spelling() {
        // The server emits `backendDOMNodeId` — CDP's own spelling. Serde's
        // `camelCase` would have asked for `backendDomNodeId`, which is a
        // different string, so the field used to decode as 0 and the node
        // looked un-actionable.
        let raw = r#"{"port":9229,"name":"Open Quick Access","role":"button",
                      "backendDOMNodeId":210,"truncated":false}"#;
        let n: CdpNode = serde_json::from_str(raw).expect("decode");
        assert_eq!(n.name, "Open Quick Access");
        assert_eq!(n.role, "button");
        assert_eq!(n.backend_dom_node_id, 210);
        assert!(!n.truncated);
    }

    #[test]
    fn node_also_decodes_the_snake_case_spelling() {
        // Hand-built JSON may use either spelling; both must work.
        let raw = r#"{"port":9229,"name":"Open Quick Access","role":"button",
                      "backend_dom_node_id":210,"truncated":false}"#;
        let n: CdpNode = serde_json::from_str(raw).expect("decode");
        assert_eq!(n.backend_dom_node_id, 210);
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
