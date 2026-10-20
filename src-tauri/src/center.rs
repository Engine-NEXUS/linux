use crate::intent_parser::ParsedIntent;
use crate::voice_profile::TurnOwnership;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Result of validating required dialog slots and invariants for a sub-center.
#[derive(Debug, Clone, PartialEq)]
pub enum Validity {
    Ok,
    Unheard { prompt: String },
    NeedSlot { slot: &'static str, prompt: String },
    Invalid { reason: &'static str, prompt: String },
}

/// Strength of the textual evidence that the user actually requested the
/// parsed action. Deterministic confidence alone is insufficient because
/// background speech can produce a fluent transcript with weak command
/// anchors.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandEvidence {
    Strong,
    Medium,
    Weak,
    None,
}

/// Final routing disposition after combining command evidence, turn
/// ownership, and session context.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActionDisposition {
    Allow,
    Clarify,
    AmbientDrop,
}

fn owner_repo_token(text: &str) -> bool {
    text.split_whitespace().any(|raw| {
        let token = raw.trim_matches(|c: char| !(c.is_ascii_alphanumeric() || c == '/' || c == '-' || c == '_' || c == '.'));
        match token.split_once('/') {
            Some((owner, repo)) => {
                !owner.is_empty()
                    && !repo.is_empty()
                    && owner.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
                    && repo.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')
            }
            None => false,
        }
    })
}

fn source_strength(source: &str) -> u8 {
    // Every rule-based parser ("deterministic", "-live", "-dictation",
    // "-directions", "-memory", ...) is a phrase-grammar match. Only the fuzzy
    // variants are weak. An unlisted `deterministic-*` source used to score 0
    // and the turn was silently dropped as ambient in a Ghost session (the
    // "type google meet" turn that produced no reply and a 28 s stall).
    if source == "deterministic-fuzzy" {
        1
    } else if source == "deterministic" || source.starts_with("deterministic-") {
        3
    } else if source == "nlu" || source == "brain" {
        2
    } else if source == "fuzzy" {
        1
    } else {
        0
    }
}

/// Whether executing this intent can change external state, spend quota/API
/// calls, move the cursor, type text, or open a window.
pub fn is_side_effect(intent: &ParsedIntent) -> bool {
    matches!(
        intent,
        ParsedIntent::OpenArchitect
            | ParsedIntent::GitHubCommand { .. }
            | ParsedIntent::OrderFood { .. }
            | ParsedIntent::SearchProduct { .. }
            | ParsedIntent::SendWhatsAppMessage { .. }
            | ParsedIntent::AnalyseRepo { .. }
            | ParsedIntent::AnalysePr { .. }
            | ParsedIntent::AnalyseLatestPr { .. }
            | ParsedIntent::CheckBranch { .. }
            | ParsedIntent::Search { .. }
            | ParsedIntent::ScreenClick { .. }
            | ParsedIntent::GhostAction { .. }
            | ParsedIntent::ScreenRead { .. }
            // Locating a target reads the screen (and may spend a vision
            // call), so it is a side effect like the other screen grounding.
            | ParsedIntent::ScreenLocate { .. }
            | ParsedIntent::BrowserTab { .. }
            | ParsedIntent::BrowserCloseTab { .. }
            | ParsedIntent::BrowserSearch { .. }
            | ParsedIntent::BrowserSearchFocus
            | ParsedIntent::WatchScreenEmail
            | ParsedIntent::StartDictation
            | ParsedIntent::StopDictation
            | ParsedIntent::EnterHandMode
            | ParsedIntent::HandMouseLive { .. }
            | ParsedIntent::PointerClick { .. }
            | ParsedIntent::NluResult { .. }
    )
}

pub fn command_evidence(
    intent: &ParsedIntent,
    transcript: &str,
    source: &str,
) -> CommandEvidence {
    let strength = source_strength(source);
    match intent {
        ParsedIntent::Unknown { .. } => CommandEvidence::None,
        ParsedIntent::Greeting { .. } | ParsedIntent::NeedMoreInfo { .. } => CommandEvidence::Strong,
        ParsedIntent::OpenArchitect => {
            if strength >= 3 {
                CommandEvidence::Strong
            } else if strength == 2 {
                CommandEvidence::Medium
            } else {
                CommandEvidence::Weak
            }
        }
        ParsedIntent::AnalyseRepo { repo, .. } => {
            if repo.trim().is_empty() || !owner_repo_token(transcript) {
                // Pattern-4 style inference ("analyse <arbitrary words>") is a
                // repository guess, not repository evidence.
                if strength >= 3 {
                    CommandEvidence::Medium
                } else {
                    CommandEvidence::Weak
                }
            } else if strength >= 3 {
                CommandEvidence::Strong
            } else {
                CommandEvidence::Medium
            }
        }
        ParsedIntent::AnalysePr { repo, .. }
        | ParsedIntent::AnalyseLatestPr { repo, .. }
        | ParsedIntent::CheckBranch { repo, .. } => {
            if repo.trim().is_empty() {
                CommandEvidence::Weak
            } else if strength >= 3 {
                CommandEvidence::Strong
            } else {
                CommandEvidence::Medium
            }
        }
        ParsedIntent::GitHubCommand { command } => {
            let has_repo = !format!("{command:?}").contains("repo: \"\"");
            if has_repo && strength >= 3 {
                CommandEvidence::Strong
            } else if has_repo {
                CommandEvidence::Medium
            } else {
                CommandEvidence::Weak
            }
        }
        _ => {
            if strength >= 3 {
                CommandEvidence::Strong
            } else if strength == 2 {
                CommandEvidence::Medium
            } else if strength == 1 {
                CommandEvidence::Weak
            } else {
                CommandEvidence::None
            }
        }
    }
}

/// Combine authorship, evidence, and session context into one routing
/// decision. Rejected turns are always ambient. Uncertain ghost turns never
/// execute or prompt; they must wait for an owner-verified repetition.
pub fn action_disposition(
    intent: &ParsedIntent,
    transcript: &str,
    source: &str,
    ownership: TurnOwnership,
    ghost_active: bool,
) -> ActionDisposition {
    if ownership == TurnOwnership::Rejected {
        return ActionDisposition::AmbientDrop;
    }

    let evidence = command_evidence(intent, transcript, source);
    if evidence == CommandEvidence::None {
        return match ownership {
            TurnOwnership::Verified => ActionDisposition::Allow,
            TurnOwnership::Unenrolled if !ghost_active => ActionDisposition::Allow,
            TurnOwnership::Uncertain if !ghost_active => ActionDisposition::Clarify,
            _ => ActionDisposition::AmbientDrop,
        };
    }

    if !is_side_effect(intent) {
        return match ownership {
            TurnOwnership::Verified | TurnOwnership::Unenrolled => ActionDisposition::Allow,
            TurnOwnership::Uncertain if !ghost_active => ActionDisposition::Clarify,
            _ => ActionDisposition::AmbientDrop,
        };
    }

    match (evidence, ownership) {
        (CommandEvidence::Strong, TurnOwnership::Verified) => ActionDisposition::Allow,
        (CommandEvidence::Strong, TurnOwnership::Unenrolled) => ActionDisposition::Allow,
        (CommandEvidence::Medium, TurnOwnership::Verified) => ActionDisposition::Allow,
        // Ghost mode is full control: an unenrolled owner's medium-evidence command (the NLU's
        // classification of "nexus. oco notepad.") runs instead of vanishing with no reply, which
        // is what the live test of 2026-10-13 did. Outside Ghost it still asks first.
        (CommandEvidence::Medium, TurnOwnership::Unenrolled) => {
            if ghost_active {
                ActionDisposition::Allow
            } else {
                ActionDisposition::Clarify
            }
        }
        (_, _) if ghost_active => ActionDisposition::AmbientDrop,
        _ => ActionDisposition::Clarify,
    }
}

/// Execution outcome returned by a sub-center.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[allow(dead_code)]
pub struct Receipt {
    pub ok: bool,
    pub summary: String,
    pub data: Value,
    pub latency_ms: u64,
}

/// Confirmation policy before execution (Alexa standard: routine repeat-back vs destructive gate).
#[derive(Debug, Clone, PartialEq)]
#[allow(dead_code)]
pub enum ConfirmKind {
    None,
    RepeatBack { ack: String },
    GatePrompt { prompt: String, timeout_secs: u64 },
}

/// Provenance supplied with a transcript turn. Unknown/missing fields mean an
/// older or non-Rust-capture caller; those turns use the legacy fail-open path.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct TurnContext {
    #[serde(default)]
    pub ownership: TurnOwnership,
    #[serde(default)]
    pub owner_score: f32,
    #[serde(default)]
    #[allow(dead_code)]
    pub decoder_bias: String,
    #[serde(default)]
    #[allow(dead_code)]
    pub language: String,
    #[serde(default)]
    pub session: u64,
}

/// Common contract for all action sub-centers (Feature 74 & 75).
pub trait SubCenter: Send + Sync {
    #[allow(dead_code)]
    fn name(&self) -> &'static str;
    fn validate(&self, action: &str, slots: &Value) -> Validity;
    /// Confirmation policy (plan 120 H3 verdict: KEPT but explicitly
    /// not-yet-wired). The repeat-back/GatePrompt copy in the impls is real,
    /// reviewed policy — deleting it would destroy work a future confirm flow
    /// should reuse. But today NO production caller consults this method
    /// (only unit tests), and `center::validate` does not dispatch to it for
    /// deterministic intents. Do NOT rely on it for safety until a caller is
    /// wired; gating decisions live in `is_side_effect` + orchestrator/MCP
    /// confirm flows. New centers: implement it (keep the policy warm), and
    /// wire the caller as a separate approved change.
    #[allow(dead_code)]
    fn confirm_kind(&self, action: &str, slots: &Value) -> ConfirmKind;
}

/// Route parsed intent to its corresponding sub-center name (Feature 74 registry decision tree).
pub fn center_for(intent: &ParsedIntent) -> &'static str {
    match intent {
        ParsedIntent::OpenApp { .. }
        | ParsedIntent::OpenUrl { .. }
        | ParsedIntent::CloseApp { .. }
        | ParsedIntent::OpenSettings => "AppCenter",

        ParsedIntent::BrowserTab { .. }
        | ParsedIntent::BrowserCloseTab { .. }
        | ParsedIntent::BrowserSearch { .. }
        | ParsedIntent::BrowserSearchFocus => "BrowserCenter",

        ParsedIntent::MediaPlayPause
        | ParsedIntent::MediaNext
        | ParsedIntent::MediaPrevious
        | ParsedIntent::MediaStop
        | ParsedIntent::PlayMusic { .. }
        | ParsedIntent::OpenMusicPlayer { .. } => "MediaCenter",

        ParsedIntent::WhatsappChat { .. }
        | ParsedIntent::SendWhatsAppMessage { .. }
        | ParsedIntent::AnalyseChatDocs { .. } => "MessageCenter",

        ParsedIntent::OrderFood { .. }
        | ParsedIntent::SearchProduct { .. } => "CommerceCenter",

        ParsedIntent::GitHubCommand { .. } => "GitHubCenter",

        ParsedIntent::OpenArchitect
        | ParsedIntent::AnalyseRepo { .. }
        | ParsedIntent::AnalysePr { .. }
        | ParsedIntent::AnalyseLatestPr { .. }
        | ParsedIntent::CheckBranch { .. }
        | ParsedIntent::RunOnLocalhost
        | ParsedIntent::CodeProject { .. }
        | ParsedIntent::CodingAgentTask { .. }
        | ParsedIntent::WatchAgentTask { .. } => "ArchitectCenter",

        ParsedIntent::Search { .. } => "KnowledgeCenter",

        ParsedIntent::EnterGhostwriter { .. }
        | ParsedIntent::StartDictation
        | ParsedIntent::StopDictation => "DictationCenter",

        ParsedIntent::EnterHandMode
        | ParsedIntent::ExitHandMode
        | ParsedIntent::HandMouseLive { .. }
        | ParsedIntent::PointerClick { .. } => "HandCenter",

        ParsedIntent::EnterGhostControl
        | ParsedIntent::ExitGhostControl
        | ParsedIntent::GhostAction { .. }
        | ParsedIntent::ScreenClick { .. }
        | ParsedIntent::ScreenRead { .. }
        // The screen agent (locate / silence / replay / re-point) — the same
        // center as the other screen grounding.
        | ParsedIntent::ScreenLocate { .. }
        | ParsedIntent::StopSpeech
        | ParsedIntent::RepeatScreen
        | ParsedIntent::PointAgain => "GhostCenter",

        ParsedIntent::Greeting { .. }
        | ParsedIntent::NeedMoreInfo { .. } => "GreetingCenter",

        ParsedIntent::WatchScreenEmail
        | ParsedIntent::Directions { .. }
        | ParsedIntent::PlacesSearch { .. }
        | ParsedIntent::ScanInternshipEmails => "GoogleCenter",

        ParsedIntent::MemoryAudit
        | ParsedIntent::Briefing
        | ParsedIntent::ImportContacts
        | ParsedIntent::PowerAction { .. }
        | ParsedIntent::ReopenSession
        | ParsedIntent::TimetableAdd { .. }
        | ParsedIntent::TimetableShow
        | ParsedIntent::TimetableClear
        | ParsedIntent::TimetableCommit
        | ParsedIntent::StudyPref { .. }
        | ParsedIntent::MailDigest
        | ParsedIntent::MailMute
        | ParsedIntent::CalendarAgenda { .. }
        | ParsedIntent::CalendarAdd { .. }
        | ParsedIntent::WhatsappRead { .. }
        | ParsedIntent::PeopleFlag { .. }
        | ParsedIntent::PeopleList
        | ParsedIntent::MemoryForget { .. }
        | ParsedIntent::MemoryForgetAll
        | ParsedIntent::MemoryForgetAllConfirm
        | ParsedIntent::BlockContact { .. }
        | ParsedIntent::UnblockContact { .. }
        | ParsedIntent::ListBlockedContacts
        | ParsedIntent::CreateVoiceReminder { .. } => "MemoryCenter",

        ParsedIntent::PersonaFriend
        | ParsedIntent::PersonaButler
        | ParsedIntent::ShareConcern { .. } => "GreetingCenter",

        ParsedIntent::NluResult { intent: name, .. } => {
            if name.starts_with("youtube_") {
                "YouTubeCenter"
            } else if name.starts_with("system_") || name == "inspect_window" || name == "click_element" {
                "SystemCenter"
            } else if name.starts_with("browser_") {
                "BrowserCenter"
            } else if name.starts_with("whatsapp_") {
                "MessageCenter"
            } else if name == "type_text" || name == "press_key" || name == "press_hotkey" {
                "DictationCenter"
            } else if name == "focus_app" {
                "AppCenter"
            } else if name == "screen_analysis" {
                "GhostCenter"
            } else {
                "NluCenter"
            }
        }

        ParsedIntent::Unknown { .. } => "KnowledgeCenter",
    }
}

/// Main Center validity gate (Feature 74 §2).
/// Validates if the user command has sufficient input or required slots before execution.
pub fn validate(intent: &ParsedIntent, transcript: &str, has_dialog_context: bool) -> Validity {
    let clean = transcript.trim();

    // In a running multi-turn dialog, brief answers ("yes", "no", "5") are expected.
    if has_dialog_context {
        return Validity::Ok;
    }

    // Unheard / silence check
    if clean.is_empty() {
        return Validity::Unheard {
            prompt: "I didn't catch that clearly, sir — say it again.".to_string(),
        };
    }

    // Confabulation / sub-threshold check: must contain at least 2 alphanumeric characters
    let alpha_count = clean.chars().filter(|c| c.is_alphanumeric()).count();
    if alpha_count < 2 {
        return Validity::Unheard {
            prompt: "I didn't catch that clearly, sir — say it again.".to_string(),
        };
    }

    // Required slot validation
    match intent {
        ParsedIntent::OpenApp { target } if target.trim().is_empty() => Validity::NeedSlot {
            slot: "target",
            prompt: "Which app should I open, sir?".to_string(),
        },
        ParsedIntent::CloseApp { target } if target.trim().is_empty() => Validity::NeedSlot {
            slot: "target",
            prompt: "Which app should I close, sir?".to_string(),
        },
        ParsedIntent::WhatsappChat { contact } if contact.trim().is_empty() => Validity::NeedSlot {
            slot: "contact",
            prompt: "Who would you like to chat with on WhatsApp, sir?".to_string(),
        },
        ParsedIntent::SendWhatsAppMessage { contact, message } => {
            if contact.trim().is_empty() {
                Validity::NeedSlot {
                    slot: "contact",
                    prompt: "Who should I send the WhatsApp message to, sir?".to_string(),
                }
            } else if message.trim().is_empty() {
                Validity::NeedSlot {
                    slot: "message",
                    prompt: format!("What message should I send to {}, sir?", contact),
                }
            } else {
                Validity::Ok
            }
        }
        ParsedIntent::BrowserSearch { query } if query.trim().is_empty() => Validity::NeedSlot {
            slot: "query",
            prompt: "What should I search for in the browser, sir?".to_string(),
        },
        ParsedIntent::Search { query } if query.trim().is_empty() => Validity::NeedSlot {
            slot: "query",
            prompt: "What would you like me to look up, sir?".to_string(),
        },
        ParsedIntent::NeedMoreInfo { prompt } => Validity::NeedSlot {
            slot: "more_info",
            prompt: prompt.clone(),
        },
        ParsedIntent::GhostAction { task } if !crate::ghost_center::validate(task) => Validity::Invalid {
            reason: "ghost_task",
            prompt: "That Ghost command is incomplete or unsafe, sir.".to_string(),
        },
        ParsedIntent::NluResult { intent: name, slots, .. } => {
            if name.starts_with("youtube_") {
                crate::youtube_center::YouTubeCenter::new().validate(name, slots)
            } else if name.starts_with("system_")
                || name == "focus_app"
                || name == "inspect_window"
                || name == "click_element"
            {
                crate::system_center::SystemCenter::new().validate(name, slots)
            } else if name.starts_with("browser_") {
                crate::browser_center::BrowserCenter::new().validate(name, slots)
            } else {
                Validity::Ok
            }
        }
        _ => Validity::Ok,
    }
}

/// All 16 action sub-centers in the NEXUS Command Center architecture.
#[allow(dead_code)]
pub const ALL_SUB_CENTERS: &[&str] = &[
    "AppCenter",
    "BrowserCenter",
    "SystemCenter",
    "MediaCenter",
    "YouTubeCenter",
    "MessageCenter",
    "CommerceCenter",
    "GitHubCenter",
    "ArchitectCenter",
    "GhostCenter",
    "HandCenter",
    "GoogleCenter",
    "MemoryCenter",
    "DictationCenter",
    "GreetingCenter",
    "KnowledgeCenter",
];

// ─── Unit Tests ──────────────────────────────────────────────────────────────

// ─── P2: UI directives (doc 74) ───────────────────────────────────────
// The Main Center is the only entry point for Rust-originated visual
// transitions. `direct_ui` applies transition guards, logs every directive
// (receipt trail), then delegates to the existing implementations.
//
// Guard rules (each exists because a live bug proved it necessary):
// - Loading(true) needs an installed request: an orphan loading window
//   with no turn behind it leaves the user staring at a spinner.
// - Loading is suppressed inside ghost sessions (pre-existing rule, kept):
//   the waves own the visual there, never the top-right window.
// - Session(true/false) always passes: it IS the source of truth.
// - Ring stays in ghost.rs: per-tick hot path with its own spam cap and
//   session scoping — routing it through here would add overhead and
//   Send-complexity for zero gain.

/// Visual transitions the Main Center may direct.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UiDirective {
    /// Top-right loading window show/hide.
    #[allow(dead_code)]
    Loading(bool),
    /// Ghost session enter/exit (orb waves + flag).
    Session(bool),
}

/// Pure predicate behind the Loading guard — matrix-tested below.
#[allow(dead_code)]
pub fn loading_should_show(has_active_request: bool, ghost_session: bool) -> bool {
    has_active_request && !ghost_session
}

/// Single choke point for visual transitions. New code MUST use this;
/// legacy `show_loading`/`hide_loading` callers are covered because the
/// guard also lives in `show_loading` itself.
pub fn direct_ui<R: tauri::Runtime>(app: &tauri::AppHandle<R>, d: UiDirective) {
    tracing::info!("main-center: ui-directive {:?}", d);
    match d {
        UiDirective::Loading(true) => {
            if !crate::orchestrator::has_active_request() {
                tracing::warn!(
                    "main-center: Loading(true) ignored — no active request (orphan spinner)"
                );
                return;
            }
            crate::orchestrator::show_loading(app);
        }
        UiDirective::Loading(false) => {
            crate::orchestrator::hide_loading(app);
        }
        UiDirective::Session(active) => {
            crate::ghost::emit_session(crate::ghost::ghost_wry::g_wry_ref(app), active);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validity_gate_unheard_and_short() {
        let dummy = ParsedIntent::Unknown { raw: "".into() };
        assert!(matches!(
            validate(&dummy, "", false),
            Validity::Unheard { .. }
        ));
        assert!(matches!(
            validate(&dummy, ".", false),
            Validity::Unheard { .. }
        ));
    }

    #[test]
    fn test_validity_gate_slots() {
        let open_empty = ParsedIntent::OpenApp { target: "".into() };
        assert!(matches!(
            validate(&open_empty, "open", false),
            Validity::NeedSlot { slot: "target", .. }
        ));

        let open_valid = ParsedIntent::OpenApp { target: "Chrome".into() };
        assert_eq!(validate(&open_valid, "open chrome", false), Validity::Ok);
    }

    #[test]
    fn test_center_for_routing() {
        assert_eq!(center_for(&ParsedIntent::OpenSettings), "AppCenter");
        assert_eq!(center_for(&ParsedIntent::MediaPlayPause), "MediaCenter");
        assert_eq!(center_for(&ParsedIntent::EnterGhostControl), "GhostCenter");
        let ghost = ParsedIntent::GhostAction {
            task: crate::ghost_center::parse("save").unwrap(),
        };
        assert_eq!(center_for(&ghost), "GhostCenter");
        assert!(is_side_effect(&ghost));
        assert_eq!(validate(&ghost, "save", false), Validity::Ok);
        assert_eq!(center_for(&ParsedIntent::EnterHandMode), "HandCenter");
        assert_eq!(center_for(&ParsedIntent::HandMouseLive { live: true }), "HandCenter");
    }

    /// Plan 120 H1: every `center_for` output must be a registered sub-center.
    /// Coverage of *variants* is compiler-forced (exhaustive match, no wildcard);
    /// this pins the *spelling* — a typo'd center name fails here, not silently
    /// at runtime. One sample intent per center; extend when adding a center.
    #[test]
    fn test_center_for_names_are_registered() {
        let samples: &[(&str, ParsedIntent)] = &[
            ("AppCenter", ParsedIntent::OpenSettings),
            ("BrowserCenter", ParsedIntent::BrowserSearch { query: "x".into() }),
            (
                "SystemCenter",
                ParsedIntent::NluResult {
                    intent: "system_focus_app".into(),
                    slots: Value::Null,
                    confidence: 1.0,
                },
            ),
            ("MediaCenter", ParsedIntent::MediaNext),
            (
                "YouTubeCenter",
                ParsedIntent::NluResult {
                    intent: "youtube_search".into(),
                    slots: Value::Null,
                    confidence: 1.0,
                },
            ),
            ("MessageCenter", ParsedIntent::WhatsappChat { contact: "x".into() }),
            (
                "CommerceCenter",
                ParsedIntent::OrderFood { query: "x".into(), restaurant: None },
            ),
            (
                "GitHubCenter",
                ParsedIntent::GitHubCommand {
                    command: crate::github_cmd::GitHubCommand::ClosePr {
                        repo: "o/r".into(),
                        pr_number: 1,
                    },
                },
            ),
            ("ArchitectCenter", ParsedIntent::OpenArchitect),
            ("GhostCenter", ParsedIntent::EnterGhostControl),
            ("HandCenter", ParsedIntent::EnterHandMode),
            ("GoogleCenter", ParsedIntent::WatchScreenEmail),
            ("MemoryCenter", ParsedIntent::MemoryAudit),
            ("DictationCenter", ParsedIntent::StartDictation),
            ("GreetingCenter", ParsedIntent::Greeting { reply: "x".into() }),
            ("KnowledgeCenter", ParsedIntent::Search { query: "x".into() }),
            ("KnowledgeCenter", ParsedIntent::Unknown { raw: "x".into() }),
        ];
        for (expected, intent) in samples {
            let got = center_for(intent);
            assert_eq!(got, *expected);
            assert!(
                ALL_SUB_CENTERS.contains(&got),
                "center_for returned unregistered center '{got}'"
            );
        }
        // The registry itself stays clean: no duplicates, no dead entries —
        // every name must be produced by at least one sample above.
        let mut seen = std::collections::HashSet::new();
        for name in ALL_SUB_CENTERS {
            assert!(seen.insert(name), "duplicate center '{name}' in ALL_SUB_CENTERS");
            assert!(
                samples.iter().any(|(e, _)| e == name),
                "registered center '{name}' has no routing sample (dead entry?)"
            );
        }
    }

    /// Plan 120 H2: the side-effect list is safety-critical. `is_side_effect`
    /// FAILS OPEN (unlisted = false = Allow for unenrolled voices), so this
    /// pins the must-gate set in both directions: removing a gated intent, or
    /// silently relying on the default for a new state-changing runner, forces
    /// a deliberate test edit. Rule for new intents: if it changes external
    /// state, spends quota/API, moves the cursor, types, or opens a window
    /// beyond a plain app-open, list it in `is_side_effect` AND extend this test.
    #[test]
    fn test_side_effect_membership_is_pinned() {
        let must_gate: &[ParsedIntent] = &[
            ParsedIntent::SendWhatsAppMessage { contact: "x".into(), message: "y".into() },
            ParsedIntent::OrderFood { query: "x".into(), restaurant: None },
            ParsedIntent::GitHubCommand {
                command: crate::github_cmd::GitHubCommand::ClosePr {
                    repo: "o/r".into(),
                    pr_number: 1,
                },
            },
            ParsedIntent::ScreenClick { ordinal: 1 },
            ParsedIntent::StartDictation,
            ParsedIntent::WatchScreenEmail,
            ParsedIntent::Search { query: "x".into() },
        ];
        // NOTE (H2b, needs product approval — NOT pinned yet): these change
        // external state/quota by the `is_side_effect` doc definition but are
        // unlisted, so unenrolled weak-evidence turns Allow them today:
        // RunOnLocalhost (spawns servers), CloseApp (kills processes),
        // BlockContact/UnblockContact, MemoryForget/ForgetAll (deletion),
        // TimetableCommit, CalendarAdd, CreateVoiceReminder, AnalyseChatDocs,
        // ScanInternshipEmails, Directions/PlacesSearch (API spend, cf. Search
        // which IS listed), EnterGhostControl/ExitGhostControl (cursor
        // takeover), CodeProject/CodingAgentTask. OpenApp/OpenUrl are
        // deliberately unlisted (Ghost Oct-13 directive: unenrolled opens run).
        // Gating any of these changes live behavior — approve per item first.
        for intent in must_gate {
            assert!(
                is_side_effect(intent),
                "state-changing intent lost its side-effect gate: {:?}",
                crate::intent_parser::intent_to_label(intent)
            );
        }
        let pure: &[ParsedIntent] = &[
            ParsedIntent::Greeting { reply: "x".into() },
            ParsedIntent::MemoryAudit,
            ParsedIntent::OpenSettings,
            ParsedIntent::Unknown { raw: "x".into() },
        ];
        for intent in pure {
            assert!(
                !is_side_effect(intent),
                "pure intent wrongly gated as side-effect: {:?}",
                crate::intent_parser::intent_to_label(intent)
            );
        }
    }

    /// Loading guard matrix (P2): orphan spinners and ghost-session windows
    /// are refused; normal turns pass.
    #[test]
    fn test_loading_guard_matrix() {
        assert!(loading_should_show(true, false));
        assert!(!loading_should_show(false, false)); // orphan — no turn
        assert!(!loading_should_show(true, true)); // ghost — waves own it
        assert!(!loading_should_show(false, true));
    }
}

// ─── P3: voice directors (doc 74) ─────────────────────────────────────
// Pure policy behind STT mute gating and TTS engine selection. These mirror
// the inline conditions in the audio callback (`should_suppress_wake`) and
// the synthesis fallback chain — same outcomes, now pinned and tested, with
// human-readable reasons for logs (and future spoken turn-block notices).

/// STT capture gate verdict.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SttGate {
    Open,
    Muted(SttMuteReason),
}

/// Why capture is muted, in priority order (matches `should_suppress_wake`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SttMuteReason {
    ManualPause,
    TtsPlaying,
    Meeting,
}

/// `meeting_suppress` is `detection_enabled && meeting_active` (same inputs
/// as `MeetingState::should_suppress_wake`).
pub fn stt_gate(manual_pause: bool, tts_playing: bool, meeting_suppress: bool) -> SttGate {
    if manual_pause {
        SttGate::Muted(SttMuteReason::ManualPause)
    } else if tts_playing {
        SttGate::Muted(SttMuteReason::TtsPlaying)
    } else if meeting_suppress {
        SttGate::Muted(SttMuteReason::Meeting)
    } else {
        SttGate::Open
    }
}

/// Short reason for logs; doubles as spoken phrasing if a turn is ever
/// blocked audibly ("not listening — {note}").
pub fn stt_mute_note(gate: SttGate) -> &'static str {
    match gate {
        SttGate::Open => "listening",
        SttGate::Muted(SttMuteReason::ManualPause) => "manually paused",
        SttGate::Muted(SttMuteReason::TtsPlaying) => "NEXUS speaking (TTS mute)",
        SttGate::Muted(SttMuteReason::Meeting) => "meeting mode (mic in use)",
    }
}

/// TTS synthesis engine choice.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TtsEngine {
    Edge,
    Local,
}

/// Upfront engine policy: explicit local voice always goes local;
/// network-down skips Edge entirely (saves ~1-2s of failing). Otherwise
/// Edge is attempted first with the local Kokoro voice as the per-call fallback.
pub fn tts_engine_for(local_voice_selected: bool, network_up: bool) -> TtsEngine {
    if local_voice_selected || !network_up {
        TtsEngine::Local
    } else {
        TtsEngine::Edge
    }
}

#[cfg(test)]
mod director_tests {
    use super::{
        action_disposition, command_evidence, is_side_effect, stt_gate, stt_mute_note,
        tts_engine_for, ActionDisposition, CommandEvidence, SttGate, SttMuteReason, TtsEngine,
    };
    use crate::intent_parser::ParsedIntent;
    use crate::voice_profile::TurnOwnership;

    /// Priority order mirrors `should_suppress_wake`: pause > TTS > meeting.
    #[test]
    fn test_stt_gate_matrix() {
        assert_eq!(stt_gate(false, false, false), SttGate::Open);
        assert_eq!(stt_gate(true, false, false), SttGate::Muted(SttMuteReason::ManualPause));
        assert_eq!(stt_gate(false, true, false), SttGate::Muted(SttMuteReason::TtsPlaying));
        assert_eq!(stt_gate(false, false, true), SttGate::Muted(SttMuteReason::Meeting));
        // Priority, not first-match-any:
        assert_eq!(stt_gate(true, true, true), SttGate::Muted(SttMuteReason::ManualPause));
        assert_eq!(stt_gate(false, true, true), SttGate::Muted(SttMuteReason::TtsPlaying));
        assert_eq!(
            stt_mute_note(SttGate::Muted(SttMuteReason::Meeting)),
            "meeting mode (mic in use)"
        );
        assert_eq!(stt_mute_note(SttGate::Open), "listening");
    }

    #[test]
    fn test_tts_engine_matrix() {
        assert_eq!(tts_engine_for(true, true), TtsEngine::Local);
        assert_eq!(tts_engine_for(false, false), TtsEngine::Local);
        assert_eq!(tts_engine_for(false, true), TtsEngine::Edge);
    }

    #[test]
    fn test_background_narrative_never_becomes_repository_action() {
        let narrative = "I tell my job is to say I should be the king of the king";
        let unknown = ParsedIntent::Unknown {
            raw: narrative.to_string(),
        };
        assert_eq!(
            command_evidence(&unknown, narrative, "none"),
            CommandEvidence::None
        );
        assert!(!is_side_effect(&unknown));
        assert_eq!(
            action_disposition(&unknown, narrative, "none", TurnOwnership::Uncertain, true),
            ActionDisposition::AmbientDrop
        );
        assert_eq!(
            action_disposition(&unknown, narrative, "none", TurnOwnership::Rejected, false),
            ActionDisposition::AmbientDrop
        );
    }

    #[test]
    fn test_repository_actions_require_repository_evidence() {
        let bare = ParsedIntent::AnalyseRepo {
            owner: None,
            repo: "king of the king".to_string(),
        };
        assert_eq!(
            command_evidence(&bare, "analyse king of the king", "deterministic"),
            CommandEvidence::Medium
        );
        assert_eq!(
            action_disposition(
                &bare,
                "analyse king of the king",
                "deterministic",
                TurnOwnership::Uncertain,
                true
            ),
            ActionDisposition::AmbientDrop
        );

        let grounded = ParsedIntent::AnalyseRepo {
            owner: None,
            repo: "zync".to_string(),
        };
        assert_eq!(
            command_evidence(&grounded, "analyse zync", "deterministic"),
            CommandEvidence::Medium
        );
        assert_eq!(
            action_disposition(
                &grounded,
                "analyse zync",
                "deterministic",
                TurnOwnership::Verified,
                true
            ),
            ActionDisposition::Allow
        );
    }

    #[test]
    fn test_ghost_mode_unenrolled_strong_command_allowed() {
        let tab_cmd = ParsedIntent::NluResult {
            intent: "browser_new_tab".to_string(),
            slots: serde_json::json!({}),
            confidence: 0.95,
        };
        assert_eq!(
            command_evidence(&tab_cmd, "create a new tab", "deterministic"),
            CommandEvidence::Strong
        );
        assert_eq!(
            action_disposition(
                &tab_cmd,
                "create a new tab",
                "deterministic",
                TurnOwnership::Unenrolled,
                true
            ),
            ActionDisposition::Allow
        );
    }

    /// Every phrase-grammar parse is evidence in a Ghost session, whatever its
    /// `deterministic-*` source label. "type google meet" (source
    /// `deterministic-dictation`) used to score 0 and vanish without a word.
    #[test]
    fn ghost_unenrolled_accepts_every_deterministic_source() {
        for src in [
            "deterministic",
            "deterministic-live",
            "deterministic-dictation",
            "deterministic-screen-analysis",
            "deterministic-directions",
            "deterministic-memory",
            "deterministic-persona",
            "deterministic-places",
        ] {
            assert_eq!(crate::center::source_strength(src), 3, "{src}");
        }
        assert_eq!(crate::center::source_strength("deterministic-fuzzy"), 1);

        let parsed = crate::intent_parser::parse_deterministic("type google meet.")
            .expect("'type google meet' must parse");
        assert_eq!(parsed.source, "deterministic-dictation");
        assert_eq!(
            action_disposition(&parsed.intent, "type google meet.", &parsed.source, TurnOwnership::Unenrolled, true),
            ActionDisposition::Allow
        );
    }

    #[test]
    fn ghost_unenrolled_medium_evidence_runs_instead_of_vanishing() {
        let nlu = ParsedIntent::NluResult {
            intent: "open_app".to_string(),
            slots: serde_json::json!({ "target": "notepad" }),
            confidence: 0.945,
        };
        assert_eq!(command_evidence(&nlu, "nexus. oco notepad.", "nlu"), CommandEvidence::Medium);
        assert_eq!(
            action_disposition(&nlu, "nexus. oco notepad.", "nlu", TurnOwnership::Unenrolled, true),
            ActionDisposition::Allow
        );
        // Outside Ghost the same turn is still confirmed first.
        assert_eq!(
            action_disposition(&nlu, "nexus. oco notepad.", "nlu", TurnOwnership::Unenrolled, false),
            ActionDisposition::Clarify
        );
        // A voice that is positively NOT the owner is still dropped.
        assert_eq!(
            action_disposition(&nlu, "nexus. oco notepad.", "nlu", TurnOwnership::Rejected, true),
            ActionDisposition::AmbientDrop
        );
    }
}
