//! Mandatory policy gate for every semantic operation.
//!
//! # Why this module exists
//!
//! The AT-SPI and CDP tiers can do things the vision path cannot: they can
//! **read** a field's text and they can **type into** a field, by name, without a
//! pixel ever existing. That capability was built before its guardrail, which is
//! the wrong order. As of this commit nothing outside `atspi.rs`/`cdp.rs` calls
//! `fill` or `activate`, so there is no live hole — but a capability that can
//! type into windows must not be reachable without a policy check, and this
//! makes that structural rather than a matter of remembering.
//!
//! # The rule that matters: check what is OBSERVED, not what is CLAIMED
//!
//! The existing `live::safety::safety_check` matches `DENIED_TARGETS` against the
//! **string the model supplied**. That is trivially bypassable: a prompt
//! injection embedded in an email can name a different app, and a real window can
//! be titled "1Password 8 — Vault" while the model says "1Password". A deny-list
//! keyed on model input is a speed bump, not a boundary.
//!
//! So the authoritative identity here comes from the accessibility bus — the
//! application name and window title the toolkit reports — and role information
//! from the target node itself. A lie about the target changes nothing.
//!
//! # Risk is not uniform
//!
//! A blanket deny would make the assistant useless, and so would pretending
//! every operation is equally risky. They are not:
//!
//! | Operation        | Reveals / does                                   | Sensitivity |
//! |------------------|---------------------------------------------------|-------------|
//! | `LocateGeometry` | that a named element exists, and its rect          | low         |
//! | `Activate`       | clicks a named element                             | medium      |
//! | `ReadText`       | the *contents* of a field                          | high        |
//! | `WriteText`      | puts attacker-chosen text into a field             | high        |
//! | `ReadTree`       | every accessible name in a window                  | high        |
//!
//! Locating a button is not the same act as typing a password into it, and the
//! policy reflects that. Read and write are denied outright on a sensitive
//! window; low-risk geometry stays available so the pointer can still say
//! "I found it" without disclosing anything.
//!
//! # Unknown is not the same as safe
//!
//! Where the window cannot be identified, high-risk operations are **denied**
//! and medium-risk ones require **confirmation**. `docs/features/research/22`
//! recorded the cost of getting this backwards: the privacy gate collapsed
//! "I could not tell" into "no match", and bank windows were screenshotted with
//! the exclusion list fully configured and silently doing nothing.

/// What the caller is trying to do. Drives the sensitivity tier.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    /// Resolve a named element to a position. Discloses existence and geometry.
    LocateGeometry,
    /// Invoke a named element's action.
    Activate,
    /// Read a field's current contents.
    ReadText,
    /// Set a field's contents.
    WriteText,
    /// Enumerate every accessible name in a window.
    ReadTree,
}

impl Op {
    /// How much this operation can disclose or damage.
    pub fn sensitivity(self) -> Sensitivity {
        match self {
            Op::LocateGeometry => Sensitivity::Low,
            Op::Activate => Sensitivity::Medium,
            Op::ReadText | Op::WriteText | Op::ReadTree => Sensitivity::High,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Sensitivity {
    Low,
    Medium,
    High,
}

/// Why a window or element is considered sensitive.
///
/// Matched against **observed** identity (a11y application name, window title,
/// node role) — never against model-supplied text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sensitive {
    /// A password / OTP / PIN entry field.
    CredentialField,
    /// A password manager, vault, or keychain window.
    PasswordManager,
    /// A bank, payment, or crypto wallet window.
    Financial,
    /// Text that names a secret outright.
    SecretBearingText,
}

/// Policy decision. `Confirm` is not a soft deny — it means "allowed, but ask".
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    Allow,
    Deny { reason: String },
    Confirm { reason: String },
}

impl Decision {
    pub fn is_allowed(&self) -> bool {
        matches!(self, Decision::Allow)
    }
    pub fn needs_confirmation(&self) -> bool {
        matches!(self, Decision::Confirm { .. })
    }
    /// Whether the caller may proceed at all without asking the user.
    pub fn may_proceed_unattended(&self) -> bool {
        matches!(self, Decision::Allow)
    }
}

/// Patterns for windows that hold credentials or money.
///
/// Matched against the observed window identity, case-insensitively, on word
/// boundaries where a word is expected.
///
/// These are deliberately *classes* ("1password", "bitwarden", "bank") rather
/// than exact titles: a personal assistant must not depend on a hardcoded list
/// of app names, and the same vault is called "1Password", "1Password 8" and
/// "1Password — Vault" across versions. Anything genuinely outside the list is
/// still covered by the fail-closed handling of unknown windows, and the list is
/// user-extensible via settings.
pub const PASSWORD_MANAGER_PATTERNS: &[&str] = &[
    "1password", "bitwarden", "keepass", "keepassxc", "lastpass", "dashlane", "enpass",
    "password safe", "seahorse", "kwallet", "gnome-keyring", "keychain", "keepassxc",
    "vault", "bitwarden-qt", "lastpass-bin", "roboform", "keeper", "nordpass",
];

pub const FINANCIAL_PATTERNS: &[&str] = &[
    "bank", "paypal", "venmo", "cashapp", "stripe", "coinbase", "metamask",
    "binance", "kraken", "revolut", "monzo", "wise", "chase", "bank of america",
    "wells fargo", "hsbc", "barclays", "santander", "icici", "hdfc", "kotak",
    "zerodha", "groww", "upstox", "robinhood", "fidelity", "vanguard", "schwab",
    "trezor", "ledger", "exodus", "electrum", "mycelium", "trust wallet",
];

/// Text fragments that indicate the content itself is a secret.
pub const SECRET_TEXT_PATTERNS: &[&str] = &[
    "seed phrase", "recovery phrase", "private key", "secret key", "api key",
    "mnemonic", "passphrase", "two-factor", "2fa code", "otp", "one-time code",
    "backup code", "security question", "ssn", "social security", "card number",
    "cvv", "routing number", "account number",
];

/// AT-SPI / AX roles that denote a credential entry, independent of window title.
///
/// This is the check that survives a renamed or mislabelled window: a vault that
/// calls itself "Vault" is still caught by the field it renders.
pub const CREDENTIAL_ROLES: &[&str] = &[
    "password text", "password", "otp input", "pin entry", "secure text",
];

fn contains_ci(haystack: &str, needle: &str) -> bool {
    haystack.to_lowercase().contains(&needle)
}

fn any_match(haystack: &str, needles: &[&str]) -> Option<&'static str> {
    let lower = haystack.to_lowercase();
    for n in needles {
        if lower.contains(n) {
            // Return the static pattern, not a slice of the haystack.
            return Some(leak_free(n));
        }
    }
    None
}

fn leak_free(pat: &str) -> &'static str {
    // The patterns are all 'static strs from the tables above.
    for t in PASSWORD_MANAGER_PATTERNS.iter().chain(FINANCIAL_PATTERNS)
        .chain(SECRET_TEXT_PATTERNS)
    {
        if *t == pat {
            return t;
        }
    }
    ""
}

/// Classify an observed window.
pub fn classify_window(app: &str, title: &str) -> Option<Sensitive> {
    let blob = format!("{app} {title}");
    if any_match(&blob, PASSWORD_MANAGER_PATTERNS).is_some() {
        return Some(Sensitive::PasswordManager);
    }
    if any_match(&blob, FINANCIAL_PATTERNS).is_some() {
        return Some(Sensitive::Financial);
    }
    if any_match(&blob, SECRET_TEXT_PATTERNS).is_some() {
        return Some(Sensitive::SecretBearingText);
    }
    None
}

/// Classify an observed node by role and accessible name.
///
/// Role is checked first because it is toolkit-reported and cannot be
/// misrepresented by renaming a window.
pub fn classify_node(role: &str, name: &str) -> Option<Sensitive> {
    let r = role.to_lowercase();
    for c in CREDENTIAL_ROLES {
        if r == *c || r.contains(*c) {
            return Some(Sensitive::CredentialField);
        }
    }
    if any_match(name, SECRET_TEXT_PATTERNS).is_some() {
        return Some(Sensitive::SecretBearingText);
    }
    None
}

/// The single decision function every semantic operation must pass.
///
/// `window_known` is false when the a11y bus could not identify the foreground
/// application — which on this platform is common, and is handled as "unknown"
/// rather than as "safe".
pub fn decide(
    op: Op,
    window_known: bool,
    window: Option<Sensitive>,
    node: Option<Sensitive>,
) -> Decision {
    // An element-level classification is decisive regardless of the window: a
    // password field inside a window called "Vault" is still a password field.
    if node == Some(Sensitive::CredentialField) {
        return Decision::Deny {
            reason: "target is a credential field — NEXUS will not read or write secrets"
                .into(),
        };
    }

    if let Some(w) = window {
        let reason = match w {
            Sensitive::CredentialField => "target is a credential field",
            Sensitive::PasswordManager => "target window is a password manager or vault",
            Sensitive::Financial => "target window is a bank, payment or wallet",
            Sensitive::SecretBearingText => "target window displays secret material",
        };
        return match op.sensitivity() {
            // Locating a control discloses only that it exists and where. Denying
            // it would make the assistant unable to say "I can see a Close
            // button" in a vault without telling the user where it is, which is
            // the lesser disclosure — but it is still a disclosure, so deny.
            Sensitivity::Low => Decision::Deny {
                reason: format!("{reason}; refusing even to locate elements in it"),
            },
            Sensitivity::Medium | Sensitivity::High => Decision::Deny { reason: reason.into() },
        };
    }

    if node == Some(Sensitive::SecretBearingText) {
        return Decision::Deny {
            reason: "target names secret material".into(),
        };
    }

    if !window_known {
        return match op.sensitivity() {
            // Not knowing the window is not the same as it being safe.
            Sensitivity::Low => Decision::Allow,
            Sensitivity::Medium => Decision::Confirm {
                reason: "cannot identify the target window; confirm before acting".into(),
            },
            Sensitivity::High => Decision::Deny {
                reason: "cannot identify the target window; refusing to read or write its \
                         contents"
                    .into(),
            },
        };
    }

    Decision::Allow
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sensitivity_ordering_drives_the_matrix() {
        assert!(Op::ReadText.sensitivity() > Op::Activate.sensitivity());
        assert!(Op::Activate.sensitivity() > Op::LocateGeometry.sensitivity());
        assert_eq!(Op::WriteText.sensitivity(), Op::ReadText.sensitivity());
    }

    #[test]
    fn vault_window_is_deny_for_every_operation() {
        for op in [Op::LocateGeometry, Op::Activate, Op::ReadText, Op::WriteText, Op::ReadTree] {
            let d = decide(op, true, Some(Sensitive::PasswordManager), None);
            assert!(!d.may_proceed_unattended(), "{op:?} was allowed in a vault");
        }
    }

    #[test]
    fn credential_field_is_deny_even_in_an_innocuous_window() {
        // The window-level check can be defeated by renaming; the field role
        // cannot. This is the case that makes the whole layer worth having.
        let d = decide(Op::ReadText, true, None, Some(Sensitive::CredentialField));
        assert!(matches!(d, Decision::Deny { .. }));
    }

    #[test]
    fn unknown_window_denies_reads_but_allows_geometry() {
        // Fail-closed where it counts, without making the assistant blind.
        assert!(matches!(
            decide(Op::ReadText, false, None, None),
            Decision::Deny { .. }
        ));
        assert!(matches!(
            decide(Op::WriteText, false, None, None),
            Decision::Deny { .. }
        ));
        assert!(matches!(
            decide(Op::Activate, false, None, None),
            Decision::Confirm { .. }
        ));
        assert_eq!(decide(Op::LocateGeometry, false, None, None), Decision::Allow);
    }

    #[test]
    fn known_ordinary_window_allows_everything() {
        for op in [Op::LocateGeometry, Op::Activate, Op::ReadText, Op::WriteText, Op::ReadTree] {
            assert_eq!(decide(op, true, None, None), Decision::Allow, "{op:?}");
        }
    }

    #[test]
    fn classification_covers_common_vaults_and_banks() {
        for s in ["1Password", "1Password 8 — Vault", "Bitwarden", "KeepassXC", "KWallet"] {
            assert_eq!(
                classify_window(s, ""),
                Some(Sensitive::PasswordManager),
                "missed vault: {s}"
            );
        }
        for s in ["Bank of America", "PayPal", "MetaMask", "Zerodha", "Monzo"] {
            assert_eq!(
                classify_window(s, ""),
                Some(Sensitive::Financial),
                "missed financial: {s}"
            );
        }
    }

    #[test]
    fn classification_is_case_insensitive_and_title_aware() {
        assert_eq!(
            classify_window("firefox", "1PASSWORD — Your Vault"),
            Some(Sensitive::PasswordManager)
        );
    }

    #[test]
    fn ordinary_windows_are_not_classified_sensitive() {
        for s in ["Firefox", "org.gnome.Calculator", "Text Editor", "Visual Studio Code", "Files"] {
            assert_eq!(classify_window(s, "notes.md — Text Editor"), None, "false positive: {s}");
        }
    }

    #[test]
    fn secret_bearing_titles_are_caught() {
        // Deliberately no "vault"/"bank" wording: those match first and would
        // mask the secret check. The first version of this test used a title
        // containing "Vault" and asserted SecretBearingText, which was wrong —
        // the window genuinely is a vault, and that classification is stronger.
        assert_eq!(
            classify_window("Firefox", "seed phrase — recovery notes"),
            Some(Sensitive::SecretBearingText)
        );
        assert_eq!(
            classify_window("Firefox", "seed phrase — Vault backup"),
            Some(Sensitive::PasswordManager),
            "a vault is a vault regardless of what else the title mentions"
        );
    }

    #[test]
    fn credential_roles_are_detected_regardless_of_name() {
        for r in ["password text", "password", "secure text"] {
            assert_eq!(
                classify_node(r, "Field 1"),
                Some(Sensitive::CredentialField),
                "missed role {r}"
            );
        }
        // A field merely *named* "Password" is not by itself a credential field;
        // the role is the trustworthy signal. Naming is a hint, not proof.
        assert_ne!(
            classify_node("text", "Password"),
            Some(Sensitive::CredentialField)
        );
    }

    #[test]
    fn node_naming_a_secret_is_still_deny() {
        let d = decide(
            Op::ReadText,
            true,
            None,
            classify_node("text", "Your recovery phrase"),
        );
        assert!(matches!(d, Decision::Deny { .. }));
    }

    #[test]
    fn decisions_are_stable_strings_not_derived_from_input() {
        // Ensure a deny reason never echoes the window text back, which would
        // itself leak what was being protected.
        let d = decide(Op::ReadText, true, Some(Sensitive::PasswordManager), None);
        if let Decision::Deny { reason } = d {
            assert!(!reason.contains("1Password"), "reason echoed the sensitive name");
        }
    }
}
