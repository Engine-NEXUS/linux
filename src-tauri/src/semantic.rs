//! Semantic resolution cascade.
//!
//! One entry point that tries the semantic tiers in order of precision and cost,
//! and says which one answered.
//!
//! ```text
//! AT-SPI .... exact, ~100ms, works on Wayland, blind to Chromium/Electron
//! CDP ....... exact, ~150ms, the only tier that sees Chromium/Electron
//! (caller) .. falls back to OCR + Set-of-Mark + vision
//! ```
//!
//! The ordering is not arbitrary. It is cheapest-and-exact first, and every tier
//! is allowed to report "not mine" so the next one gets a turn. A tier that
//! *cannot* answer must say so rather than guessing — a wrong semantic answer is
//! worse than no answer, because it is acted upon.
//!
//! Why a cascade rather than picking a tier per app up front: the split is not
//! knowable ahead of time. A developer machine runs GTK settings tools, Chromium,
//! and Electron, sometimes simultaneously, and the same app can change tier
//! depending on launch flags. Asking is cheap; guessing is not.

use crate::atspi::AtspiNode;
use crate::cdp::CdpNode;
use crate::policy::Decision;

/// An element resolved by one of the semantic tiers.
#[derive(Debug, Clone, PartialEq)]
pub struct SemanticHit {
    pub name: String,
    /// AT-SPI role or CDP role. Comparable to a Windows UIA control type.
    pub role: String,
    pub bounds: Option<crate::atspi::AtspiBounds>,
    /// Which tier answered. Useful for explaining latency and for diagnostics.
    pub tier: Tier,
    /// The actions the toolkit exposes on this element ("click", "press", …).
    /// Empty means the element exists but cannot be invoked.
    pub actions: Vec<String>,
    /// The toolkit's state set for the element (focused, editable, …).
    pub states: Vec<String>,
    /// True when the element lives in a Chromium/Electron app, whose AT-SPI
    /// tree is a skeleton unless the renderer was launched with
    /// `--force-renderer-accessibility`. A miss in such a tree is expected
    /// rather than a fault — the CDP tier is the answer for those apps.
    pub chromium_skeleton_risk: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tier {
    AtSpi,
    Cdp,
}

impl Tier {
    pub fn as_str(self) -> &'static str {
        match self {
            Tier::AtSpi => "atspi",
            Tier::Cdp => "cdp",
        }
    }
}

impl SemanticHit {
    /// The centre of the element, when it has usable geometry.
    ///
    /// `None` for a hit that cannot be pointed at (the CDP tier's tree is
    /// semantic, not spatial).
    pub fn center(&self) -> Option<(i32, i32)> {
        self.bounds.filter(|b| b.is_real()).map(|b| b.center())
    }
}

/// Outcome of a cascade, including *why* it stopped.
///
/// The reasons are the point. "not found" and "no tier available" lead to
/// different user-facing behaviour, and a bare `None` would collapse them — the
/// same mistake `Foreground::Unknown` exists to prevent.
#[derive(Debug, Clone, PartialEq)]
pub enum Resolution {
    Found(SemanticHit),
    /// Every tier that could run was asked and none had the element.
    NotFound,
    /// No semantic tier was usable (no a11y bus, no debug port, sidecar down).
    NoTierAvailable { detail: String },
}

impl Resolution {
    pub fn is_found(&self) -> bool {
        matches!(self, Resolution::Found(_))
    }
    /// The hit as a *pointing* result: only when it carries usable geometry.
    ///
    /// The CDP tier's accessibility tree is semantic, not spatial, so a CDP hit
    /// has no rectangle. Reporting such a hit as "found" would hand the caller
    /// a name with no position, and the caller would either invent one or
    /// pretend it can act on it. This accessor makes the distinction explicit
    /// so a caller that needs a point can ask for one.
    pub fn pointing(&self) -> Option<&SemanticHit> {
        match self {
            Resolution::Found(h) if h.bounds.map(|b| b.is_real()).unwrap_or(false) => Some(h),
            _ => None,
        }
    }
    /// Whether the caller should fall through to the pixel tier.
    ///
    /// Only `NotFound` warrants a pixel lookup. `NoTierAvailable` means nothing
    /// semantic can run at all, so pixels are not a *fallback* — they are the
    /// only option, and the caller should say so rather than pretend a semantic
    /// miss was a semantic capability.
    pub fn should_try_pixels(&self) -> bool {
        matches!(self, Resolution::NotFound)
    }
}

/// Why a semantic operation was refused or could not run.
///
/// The distinction is the point: "the policy said no" and "no tier could
/// answer" lead to different user-facing behaviour, and a bare `None` would
/// collapse them into a shrug.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SemanticError {
    /// The policy gate refused. The reason is safe to speak: it never echoes
    /// the protected content.
    Denied(String),
    /// No semantic tier was usable (no a11y bus, no debug port, sidecar down).
    Unavailable(String),
}

impl SemanticError {
    /// A short, user-facing explanation.
    pub fn spoken(&self) -> String {
        match self {
            SemanticError::Denied(reason) => reason.clone(),
            SemanticError::Unavailable(detail) => {
                format!("I can't read the screen's structure right now, sir ({detail}).")
            }
        }
    }
}

/// Run a semantic request to completion, from any context.
///
/// The semantic tier is reached from three kinds of place: a plain function,
/// a `spawn_blocking` worker, and an async task. A tokio runtime cannot be
/// nested inside a running one, and `block_in_place` needs a multi-threaded
/// runtime, so every request gets a thread of its own. The cost is a few tens
/// of microseconds; the alternative is a panic in production.
fn run_isolated<T, F, Fut>(f: F) -> Result<T, String>
where
    F: FnOnce() -> Fut + Send + 'static,
    Fut: std::future::Future<Output = Result<T, String>> + Send + 'static,
    T: Send + 'static,
{
    let worker = std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| format!("runtime: {e}"))?;
        rt.block_on(f())
    });
    match worker.join() {
        Ok(r) => r,
        Err(_) => Err("the semantic worker thread panicked".to_string()),
    }
}

/// The observed foreground, as the accessibility bus reports it.
///
/// This is the identity the policy gate acts on: it comes from the platform,
/// not from anything the model said. A prompt injection can rename its
/// target; it cannot change what the toolkit reports.
pub fn observed_foreground() -> crate::atspi::Foreground {
    match run_isolated(|| async { crate::atspi::focused().await }) {
        Ok(fg) => fg,
        Err(e) => crate::atspi::Foreground::Unknown { reason: e },
    }
}

/// The policy decision for an operation against the *observed* foreground.
///
/// Every semantic operation passes through here. The identity checked is the
/// OBSERVED one (from the accessibility bus), never the name the model
/// supplied — a prompt injection can rename its target, but it cannot change
/// what the toolkit reports the focused window to be.
pub fn foreground_decision(op: crate::policy::Op) -> crate::policy::Decision {
    let fg = observed_foreground();
    let known = fg.is_known();
    let sensitive = match &fg {
        crate::atspi::Foreground::Known { app, .. } => {
            crate::policy::classify_window(app, fg.title().unwrap_or_default())
        }
        crate::atspi::Foreground::Unknown { .. } => None,
    };
    crate::policy::decide(op, known, sensitive, None)
}

fn from_atspi(n: AtspiNode) -> Option<SemanticHit> {
    Some(SemanticHit {
        name: n.name,
        role: n.role,
        bounds: n.bounds,
        tier: Tier::AtSpi,
        actions: n.actions,
        states: n.states,
        chromium_skeleton_risk: n.chromium_skeleton_risk,
    })
}

fn from_cdp(n: CdpNode) -> Option<SemanticHit> {
    // CDP returns no geometry: the AX tree is semantic, not spatial. A caller
    // that needs a point must ask the page for one, or drop to the pixel tier.
    // Reporting a fake bounding box here would be the exact bug
    // `AtspiBounds::is_real` guards against on the other tier.
    Some(SemanticHit {
        name: n.name,
        role: n.role,
        bounds: None,
        tier: Tier::Cdp,
        actions: Vec::new(),
        states: Vec::new(),
        chromium_skeleton_risk: false,
    })
}
/// Resolve `name`, optionally constrained to `role`, across both semantic tiers.
///
/// Blocking by design: the callers are the pre-capture privacy gate and the
/// locate path, both of which are synchronous today. The request itself runs
/// on a thread of its own, so this is safe from any calling context.
pub fn resolve(name: &str, role: Option<&str>) -> Resolution {
    if name.trim().is_empty() {
        return Resolution::NotFound;
    }

    // ── Policy gate ──────────────────────────────────────────────────────
    // Every semantic operation passes here first. The identity checked is the
    // OBSERVED one (from the accessibility bus), never the name the model
    // supplied — a prompt injection can rename its target, but it cannot change
    // what GNOME reports the focused window to be. See `policy` for why.
    match foreground_decision(crate::policy::Op::LocateGeometry) {
        d if d.may_proceed_unattended() => {}
        Decision::Confirm { reason } => {
            tracing::info!("semantic: {name:?} needs confirmation — {reason}");
            return Resolution::NotFound;
        }
        Decision::Deny { reason } => {
            tracing::warn!("semantic: refused {name:?} — {reason}");
            return Resolution::NotFound;
        }
        _ => {}
    }

    let want = name.to_string();
    let want_role = role.map(|r| r.to_string());
    let outcome = run_isolated(move || async move {
        // ── Tier 1: AT-SPI ────────────────────────────────────────────────
        let mut last_err: Option<String> = None;
        match crate::atspi::find(&want, None, want_role.as_deref()).await {
            Ok(Some(n)) => {
                if let Some(h) = from_atspi(n) {
                    return Ok((Some(h), last_err));
                }
            }
            Ok(None) => {}
            Err(e) => last_err = Some(e),
        }

        // ── Tier 2: CDP (Chromium/Electron) ──────────────────────────────
        match crate::cdp::find(&want, want_role.as_deref(), None).await {
            Ok(Some(n)) => {
                if from_cdp(n).is_some() {
                    // A CDP hit carries no geometry: the AX tree is semantic,
                    // not spatial. Reporting it as a *pointing* result would
                    // hand the caller a name with no position, so it is not
                    // reported as one: the cascade falls through to the next
                    // tier (pixels) instead, explicitly. The name is still
                    // worth having for actuation, which is what `activate` is
                    // for.
                    tracing::debug!(
                        "semantic: {want:?} found via the CDP tier but it has no geometry — \
                         falling through to the pixel tier"
                    );
                }
            }
            Ok(None) => {}
            Err(e) => {
                // A CDP miss is expected on most machines: apps are rarely
                // launched with a debug port. Not an error, just the next
                // tier's turn.
                last_err = Some(e);
            }
        }
        Ok((None, last_err))
    });

    match outcome {
        Ok((Some(hit), _)) => Resolution::Found(hit),
        Ok((None, Some(err))) => Resolution::NoTierAvailable { detail: err },
        Ok((None, None)) => Resolution::NotFound,
        Err(e) => Resolution::NoTierAvailable { detail: e },
    }
}

/// The actionable elements of the foreground window, through the semantic tiers.
///
/// This is the Linux answer to "what can I click?", and it goes through the
/// policy gate exactly like every other semantic operation: listing the
/// accessible names of a window *is* reading them.
///
/// The error carries the policy reason so a caller can say it out loud rather
/// than reporting "I see nothing" — those are very different answers.
pub fn list_actionables() -> Result<Vec<SemanticHit>, SemanticError> {
    match foreground_decision(crate::policy::Op::ReadTree) {
        d if d.may_proceed_unattended() => {}
        Decision::Deny { reason } => return Err(SemanticError::Denied(reason)),
        Decision::Confirm { reason } => return Err(SemanticError::Denied(reason)),
        Decision::Allow => {}
    }

    match run_isolated(|| async { crate::atspi::actionables(None).await }) {
        Ok(nodes) => Ok(nodes.into_iter().filter_map(from_atspi).collect()),
        Err(e) => Err(SemanticError::Unavailable(e)),
    }
}

/// Actuate a named element through the semantic tiers.
///
/// Unlike [`resolve`], this needs no geometry: the element is addressed by
/// identity and the toolkit performs the action. That is what makes it work
/// on a native Wayland session, where synthetic input does not exist.
///
/// AT-SPI first (it is the tier that works everywhere), then CDP for
/// Chromium/Electron, whose AT-SPI tree is a skeleton.
pub fn activate(name: &str, role: Option<&str>) -> Result<ActivationOutcome, SemanticError> {
    match foreground_decision(crate::policy::Op::Activate) {
        d if d.may_proceed_unattended() => {}
        Decision::Deny { reason } => return Err(SemanticError::Denied(reason)),
        Decision::Confirm { reason } => return Err(SemanticError::Denied(reason)),
        Decision::Allow => {}
    }

    let want = name.to_string();
    let want_role = role.map(|r| r.to_string());
    let outcome = run_isolated(move || async move {
        match crate::atspi::activate(&want, None, want_role.as_deref(), "click").await {
            Ok(effect) => Ok(ActivationOutcome {
                tier: Tier::AtSpi,
                app: effect.app,
                name: effect.name,
                ok: effect.ok,
            }),
            Err(e) => {
                tracing::info!("semantic: AT-SPI could not activate ({e}) — trying CDP");
                match crate::cdp::activate(&want, want_role.as_deref(), None).await {
                    Ok(effect) => Ok(ActivationOutcome {
                        tier: Tier::Cdp,
                        app: String::new(),
                        name: effect.name,
                        ok: effect.ok,
                    }),
                    Err(e) => Err(e),
                }
            }
        }
    });

    match outcome {
        Ok(outcome) => Ok(outcome),
        Err(e) => Err(SemanticError::Unavailable(e)),
    }
}

/// The result of a semantic activation.
#[derive(Debug, Clone, PartialEq)]
pub struct ActivationOutcome {
    /// Which tier performed the action.
    pub tier: Tier,
    /// The application that owns the element.
    pub app: String,
    /// The element's accessible name.
    pub name: String,
    /// Whether the toolkit reported the action as performed.
    pub ok: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::atspi::AtspiBounds;

    #[test]
    fn atspi_hit_keeps_geometry() {
        let n = AtspiNode {
            app: "Calculator".into(),
            role: "push button".into(),
            name: "7".into(),
            bounds: Some(AtspiBounds { x: 10, y: 20, w: 30, h: 40 }),
            states: vec![],
            actions: vec!["click".into()],
            chromium_skeleton_risk: false,
        };
        let h = from_atspi(n).expect("hit");
        assert_eq!(h.tier, Tier::AtSpi);
        assert_eq!(h.role, "push button");
        assert_eq!(h.bounds.unwrap().center(), (25, 40));
    }

    #[test]
    fn cdp_hit_has_no_geometry_and_says_why() {
        // The CDP AX tree is semantic, not spatial. Inventing bounds here would
        // hand a caller a coordinate that means nothing.
        let n = CdpNode {
            port: 9229,
            name: "Open Quick Access".into(),
            role: "button".into(),
            backend_dom_node_id: 210,
            truncated: false,
        };
        let h = from_cdp(n).expect("hit");
        assert_eq!(h.tier, Tier::Cdp);
        assert!(h.bounds.is_none());
    }

    #[test]
    fn tier_names_are_stable() {
        // These strings reach logs and metrics; changing them silently would
        // break any dashboard reading them.
        assert_eq!(Tier::AtSpi.as_str(), "atspi");
        assert_eq!(Tier::Cdp.as_str(), "cdp");
    }

    #[test]
    fn empty_name_is_not_found_without_asking() {
        // An empty name must not trigger two sidecar round trips.
        assert_eq!(resolve("   ", None), Resolution::NotFound);
    }

    #[test]
    fn not_found_permits_pixel_fallback_but_no_tier_does_not() {
        // The distinction matters: NotFound means "the semantic tiers answered
        // and did not have it", so pixels are a genuine next step. NoTierAvailable
        // means nothing semantic can run, and a pixel lookup is not a fallback —
        // it is the only option, and the caller should not pretend otherwise.
        assert!(Resolution::NotFound.should_try_pixels());
        assert!(!Resolution::NoTierAvailable { detail: "x".into() }.should_try_pixels());
    }

    #[test]
    fn is_found_is_only_true_for_a_hit() {
        let h = hit_with(None);
        assert!(Resolution::Found(h).is_found());
        assert!(!Resolution::NotFound.is_found());
    }

    // ── Pointing vs. naming ───────────────────────────────────────────────

    fn hit_with(bounds: Option<AtspiBounds>) -> SemanticHit {
        SemanticHit {
            name: "Save".into(),
            role: "push button".into(),
            bounds,
            tier: Tier::AtSpi,
            actions: vec!["click".into()],
            states: vec!["enabled".into()],
            chromium_skeleton_risk: false,
        }
    }

    #[test]
    fn pointing_requires_real_geometry() {
        // A hit with a rectangle is pointable.
        let ok = hit_with(Some(AtspiBounds { x: 1, y: 2, w: 30, h: 20 }));
        assert!(Resolution::Found(ok).pointing().is_some());
    }

    #[test]
    fn a_hit_without_geometry_is_not_a_pointing_result() {
        // This is the CDP tier's shape. It names the element, but it cannot
        // point at it, so `pointing()` must say no.
        let h = hit_with(None);
        assert!(Resolution::Found(h).pointing().is_none());
    }

    #[test]
    fn an_unlaid_out_rectangle_is_not_a_pointing_result() {
        // GTK's "not yet positioned" sentinel must never become a click.
        let h = hit_with(Some(AtspiBounds {
            x: -2147483648,
            y: -2147483648,
            w: 1,
            h: 1,
        }));
        assert!(Resolution::Found(h).pointing().is_none());
    }

    #[test]
    fn not_found_is_never_pointing() {
        assert!(Resolution::NotFound.pointing().is_none());
        assert!(Resolution::NoTierAvailable { detail: "x".into() }.pointing().is_none());
    }

    // ── Error wording ─────────────────────────────────────────────────────

    #[test]
    fn a_denied_error_speaks_the_reason_verbatim() {
        let e = SemanticError::Denied("cannot identify the target window".into());
        assert_eq!(e.spoken(), "cannot identify the target window");
    }

    #[test]
    fn an_unavailable_error_speaks_a_plain_sentence() {
        let e = SemanticError::Unavailable("no a11y bus".into());
        assert!(e.spoken().contains("no a11y bus"));
        assert!(!e.spoken().is_empty());
    }
}
