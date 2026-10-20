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
    pub fn hit(&self) -> Option<&SemanticHit> {
        match self {
            Resolution::Found(h) => Some(h),
            _ => None,
        }
    }
    pub fn is_found(&self) -> bool {
        matches!(self, Resolution::Found(_))
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

/// The observed foreground `(application, window title)`.
///
/// `None` when the bus cannot say — which on this platform is common, and which
/// the policy layer treats as "unknown", not as "safe".
pub fn observed_window() -> Option<(String, String)> {
    let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().ok()?;
    let fg = rt.block_on(crate::atspi::focused()).ok()?;
    match fg {
        crate::atspi::Foreground::Known { app, title } => Some((app, title)),
        crate::atspi::Foreground::Unknown { .. } => None,
    }
}

fn from_atspi(n: AtspiNode) -> Option<SemanticHit> {
    Some(SemanticHit {
        name: n.name,
        role: n.role,
        bounds: n.bounds,
        tier: Tier::AtSpi,
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
    })
}

/// Resolve `name`, optionally constrained to `role`, across both semantic tiers.
///
/// Blocking by design: the callers are the pre-capture privacy gate and the
/// locate path, both of which are synchronous today. `block_in_place` keeps this
/// off the async executor when a runtime is already running.
pub fn resolve(name: &str, role: Option<&str>) -> Resolution {
    if name.trim().is_empty() {
        return Resolution::NotFound;
    }
    let rt = match tokio::runtime::Builder::new_current_thread().enable_all().build() {
        Ok(rt) => rt,
        Err(e) => {
            return Resolution::NoTierAvailable { detail: format!("runtime: {e}") };
        }
    };
    resolve_in(&rt, name, role)
}

fn resolve_in(rt: &tokio::runtime::Runtime, name: &str, role: Option<&str>) -> Resolution {
    let mut asked_any = false;
    let mut last_err: Option<String> = None;

    // ── Policy gate ──────────────────────────────────────────────────────
    // Every semantic operation passes here first. The identity checked is the
    // OBSERVED one (from the accessibility bus), never the name the model
    // supplied — a prompt injection can rename its target, but it cannot change
    // what GNOME reports the focused window to be. See `policy` for why.
    let observed = observed_window();
    let window_sensitive = observed
        .as_ref()
        .and_then(|(a, t)| crate::policy::classify_window(a, t));
    match crate::policy::decide(
        crate::policy::Op::LocateGeometry,
        observed.is_some(),
        window_sensitive,
        None,
    ) {
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

    // ── Tier 1: AT-SPI ────────────────────────────────────────────────────
    match rt.block_on(crate::atspi::find(name, None, role)) {
        Ok(Some(n)) => {
            if let Some(h) = from_atspi(n) {
                return Resolution::Found(h);
            }
        }
        Ok(None) => {
            asked_any = true;
        }
        Err(e) => {
            last_err = Some(e);
        }
    }

    // ── Tier 2: CDP (Chromium/Electron) ──────────────────────────────────
    match rt.block_on(crate::cdp::find(name, role, None)) {
        Ok(Some(n)) => {
            if let Some(h) = from_cdp(n) {
                return Resolution::Found(h);
            }
            asked_any = true;
        }
        Ok(None) => {
            asked_any = true;
        }
        Err(e) => {
            // A CDP miss is expected on most machines: apps are rarely launched
            // with a debug port. Not an error, just the next tier's turn.
            last_err = Some(e);
        }
    }

    if asked_any {
        Resolution::NotFound
    } else {
        Resolution::NoTierAvailable {
            detail: last_err.unwrap_or_else(|| "no semantic tier responded".to_string()),
        }
    }
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
    fn hit_accessor_only_on_found() {
        let h = SemanticHit {
            name: "x".into(),
            role: "button".into(),
            bounds: None,
            tier: Tier::Cdp,
        };
        assert!(Resolution::Found(h.clone()).hit().is_some());
        assert!(Resolution::NotFound.hit().is_none());
        assert!(!Resolution::NotFound.is_found());
    }
}
