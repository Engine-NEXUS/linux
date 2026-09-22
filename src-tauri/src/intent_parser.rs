//! Enhanced intent parser ΓÇö Rust-side command understanding with app registry
//! fuzzy matching, analyse/repo/PR entity extraction, and NLU server fallback.
//!
//! Architecture:
//!   1. Deterministic regex patterns for command structure (open, analyse, search, media)
//!   2. App registry fuzzy matching for app names (uses ALL installed apps, not a fixed list)
//!   3. Entity extraction for repo names, PR numbers, owners
//!   4. NLU server (BERT-Mini) as a confidence booster ΓÇö lazy-started Python sidecar
//!   5. Falls back to the frontend regex parser if NLU is unavailable
//!
//! This replaces the frontend TypeScript parser for better accuracy:
//!   - Uses the app registry (hundreds of installed apps) instead of a fixed list of 50
//!   - Handles "analyse PR 23 servx", "analyse servx repo", "analyse owner/repo"
//!   - Phonetic + Levenshtein matching against real installed app names
//!   - Confidence scoring with fallback to remote backend

use crate::app_registry;
use serde::{Deserialize, Serialize};

// ΓöÇΓöÇΓöÇ Types ΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇ

/// Parsed intent ΓÇö same shape as the frontend Intent type, plus new analyse intents.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "action")]
pub enum ParsedIntent {
    #[serde(rename = "open_app")]
    OpenApp { target: String },
    #[serde(rename = "open_url")]
    OpenUrl { target: String, url: String },
    #[serde(rename = "close_app")]
    CloseApp { target: String },
    #[serde(rename = "whatsapp_chat")]
    WhatsappChat { contact: String },
    #[serde(rename = "open_architect")]
    OpenArchitect,
    /// Open the settings sidebar (command center).
    /// "open settings" / "open command center" / "show preferences" / "configure NEXUS"
    #[serde(rename = "open_settings")]
    OpenSettings,
    #[serde(rename = "search")]
    Search { query: String },
    #[serde(rename = "analyse_repo")]
    AnalyseRepo { owner: Option<String>, repo: String },
    #[serde(rename = "analyse_pr")]
    AnalysePr {
        owner: Option<String>,
        repo: String,
        pr_number: u32,
    },
    /// Analyse the latest PR in a repo, optionally filtered by author.
    /// "analyse the pr in zync" → AnalyseLatestPr { repo: "zync", author: None }
    /// "analyse the pr by prem in servx" → AnalyseLatestPr { repo: "servx", author: Some("prem") }
    #[serde(rename = "analyse_latest_pr")]
    AnalyseLatestPr {
        owner: Option<String>,
        repo: String,
        author: Option<String>,
    },
    /// Check the latest branch in a repo, optionally filtered by author.
    /// "check the latest branch of servx created by eesha"
    ///   → CheckBranch { repo: "servx", author: Some("eesha") }
    #[serde(rename = "check_branch")]
    CheckBranch {
        owner: Option<String>,
        repo: String,
        author: Option<String>,
    },
    #[serde(rename = "media_play_pause")]
    MediaPlayPause,
    #[serde(rename = "media_next")]
    MediaNext,
    #[serde(rename = "media_previous")]
    MediaPrevious,
    #[serde(rename = "media_stop")]
    MediaStop,
    /// Local conversational reply (greetings, thanks, etc.) ΓÇö handled
    /// entirely locally, no Cloudflare Worker round-trip needed.
    #[serde(rename = "greeting")]
    Greeting { reply: String },
    /// NLU server result ΓÇö used when the deterministic parser is uncertain
    /// and the NLU server returns a classification.
    #[serde(rename = "nlu_result")]
    NluResult {
        intent: String,
        slots: serde_json::Value,
        confidence: f32,
    },
    /// GitHub sub-command — parsed from voice/text into a structured
    /// `GitHubCommand` that the orchestrator routes to `Subsystem::GitHub`.
    #[serde(rename = "github_command")]
    GitHubCommand {
        command: crate::github_cmd::GitHubCommand,
    },
    /// Order food from a restaurant (Swiggy Food MCP).
    /// "order pizza from dominos" → OrderFood { query: "pizza", restaurant: Some("dominos") }
    /// "order food from swiggy" → OrderFood { query: "", restaurant: None }
    #[serde(rename = "order_food")]
    OrderFood {
        query: String,
        restaurant: Option<String>,
    },
    /// Search for a product on Amazon.
    /// "search for sony headphones on amazon" → SearchProduct { query: "sony headphones" }
    #[serde(rename = "search_product")]
    SearchProduct { query: String },
    /// Send a WhatsApp message to a contact.
    /// "send mom a whatsapp message saying i'll be late"
    /// → SendWhatsAppMessage { contact: "mom", message: "i'll be late" }
    #[serde(rename = "send_whatsapp_message")]
    SendWhatsAppMessage {
        contact: String,
        message: String,
    },
    /// Partial MCP command — the user named the action but not all slots
    /// (e.g. "send message to mummy" with no message body). The
    /// orchestrator speaks `prompt` instead of letting the transcript fall
    /// to the Worker, whose LLM can only guess (or refuse). Never executes.
    #[serde(rename = "need_more_info")]
    NeedMoreInfo { prompt: String },
    /// Enter the Ghostwriter dictation room (persistent session).
    /// "ghostwriter", "take a letter", "write this down for mom".
    /// Contact is optional — asked inside the room if missing.
    #[serde(rename = "enter_ghostwriter")]
    EnterGhostwriter { contact: Option<String> },
    /// Click the Nth on-screen actionable (1-based reading order).
    /// "click the 3rd option" → ScreenClick { ordinal: 3 }.
    #[serde(rename = "screen_click")]
    ScreenClick { ordinal: u32 },
    /// Read back the Nth on-screen actionable ("what's the 2nd button").
    #[serde(rename = "screen_read")]
    ScreenRead { ordinal: u32 },
    /// Switch browser tab ("move to the 4th tab" → Ctrl+4).
    #[serde(rename = "browser_tab")]
    BrowserTab { index: u32 },
    #[serde(rename = "unknown")]
    Unknown { raw: String },
}

/// Result of parsing a transcript.
#[derive(Debug, Clone, Serialize)]
pub struct ParseResult {
    pub intent: ParsedIntent,
    /// Confidence score 0.0ΓÇô1.0. Deterministic matches are 1.0.
    /// NLU server matches are the model's confidence.
    pub confidence: f32,
    /// Source of the parse: "deterministic", "nlu", "fallback"
    pub source: String,
}

/// Convert a `ParsedIntent` to its snake_case NLU label.
/// This is the reverse of `nlu_client::nlu_to_parsed_intent`.
/// Used by the brain monitor and orchestrator to store intent labels
/// that `merge_and_train.py` can use for BERT-Mini training.
///
/// Without this, `format!("{:?}", intent)` produces Debug output like
/// `GitHubCommand { command: ListPrs { repo: "...", state: "..." } }`
/// which doesn't match any NLU training label.
pub fn intent_to_label(intent: &ParsedIntent) -> &'static str {
    use crate::github_cmd::GitHubCommand;
    match intent {
        ParsedIntent::OpenApp { .. } => "open_app",
        ParsedIntent::OpenUrl { .. } => "open_url",
        ParsedIntent::CloseApp { .. } => "close_app",
        ParsedIntent::WhatsappChat { .. } => "whatsapp_chat",
        ParsedIntent::OpenArchitect => "open_architect",
        ParsedIntent::OpenSettings => "open_settings",
        ParsedIntent::Search { .. } => "search",
        ParsedIntent::AnalyseRepo { .. } => "analyse_repo",
        ParsedIntent::AnalysePr { .. } => "analyse_pr",
        ParsedIntent::AnalyseLatestPr { .. } => "analyse_latest_pr",
        ParsedIntent::CheckBranch { .. } => "check_branch",
        ParsedIntent::MediaPlayPause => "media_play_pause",
        ParsedIntent::MediaNext => "media_next",
        ParsedIntent::MediaPrevious => "media_previous",
        ParsedIntent::MediaStop => "media_stop",
        ParsedIntent::Greeting { .. } => "greeting",
        ParsedIntent::NluResult { .. } => "nlu_result",
        ParsedIntent::Unknown { .. } => "unknown",
        ParsedIntent::OrderFood { .. } => "order_food",
        ParsedIntent::SearchProduct { .. } => "search_product",
        ParsedIntent::SendWhatsAppMessage { .. } => "send_whatsapp_message",
        ParsedIntent::NeedMoreInfo { .. } => "need_more_info",
        ParsedIntent::EnterGhostwriter { .. } => "enter_ghostwriter",
        ParsedIntent::ScreenClick { .. } => "screen_click",
        ParsedIntent::ScreenRead { .. } => "screen_read",
        ParsedIntent::BrowserTab { .. } => "browser_tab",
        ParsedIntent::GitHubCommand { command } => match command {
            GitHubCommand::MergePr { .. } => "merge_pr",
            GitHubCommand::ApprovePr { .. } => "approve_pr",
            GitHubCommand::ClosePr { .. } => "close_pr",
            GitHubCommand::ListPrs { .. } => "list_prs",
            GitHubCommand::GetPr { .. } => "get_pr",
            GitHubCommand::CreatePr { .. } => "create_pr",
            GitHubCommand::UpdateBranch { .. } => "update_branch",
            GitHubCommand::RevertPr { .. } => "revert_pr",
            GitHubCommand::ListPrFiles { .. } => "list_pr_files",
            GitHubCommand::CommentPr { .. } => "comment_pr",
            GitHubCommand::AddCollaborator { .. } => "add_collaborator",
            GitHubCommand::RemoveCollaborator { .. } => "remove_collaborator",
            GitHubCommand::ListCollaborators { .. } => "list_collaborators",
            GitHubCommand::AddOrgMember { .. } => "add_org_member",
            GitHubCommand::RemoveOrgMember { .. } => "remove_org_member",
            GitHubCommand::ListOrgMembers { .. } => "list_org_members",
            GitHubCommand::ConvertToOutsideCollaborator { .. } => "convert_to_outside_collaborator",
            GitHubCommand::ListOutsideCollaborators { .. } => "list_outside_collaborators",
            GitHubCommand::SetBranchProtection { .. } => "set_branch_protection",
            GitHubCommand::DeleteBranch { .. } => "delete_branch",
            GitHubCommand::ListBranches { .. } => "list_branches",
            GitHubCommand::CreateRelease { .. } => "create_release",
            GitHubCommand::ListReleases { .. } => "list_releases",
            GitHubCommand::DeleteRelease { .. } => "delete_release",
            GitHubCommand::ListWorkflows { .. } => "list_workflows",
            GitHubCommand::ListWorkflowRuns { .. } => "list_workflow_runs",
            GitHubCommand::RerunWorkflow { .. } => "rerun_workflow",
            GitHubCommand::CancelWorkflow { .. } => "cancel_workflow",
        },
    }
}

// ΓöÇΓöÇΓöÇ Deterministic parser ΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇ

/// Parse a transcript into a structured intent using deterministic rules.
///
/// This is the primary parser. It handles:
/// - "open <app>" / "launch <app>" / etc. ΓåÆ open_app (with app registry fuzzy match)
/// - "analyse <repo>" / "analyse PR <num> <repo>" / "analyse <owner>/<repo>"
/// - "search for <query>" / "google <query>"
/// - "open architecture mapper"
/// - Media controls (pause, next, previous, stop)
/// - "open <url>" (direct URL)
pub fn parse_deterministic(transcript: &str) -> Option<ParseResult> {
    let text = transcript.trim().to_lowercase();
    let text = normalize_whitespace(&text);
    // Strip trailing punctuation that STT often appends (e.g. "Open the PR list."
    // from Groq/Whisper). Without this, regexes anchored with `$` (like the
    // ListPrs pattern) fail to match and the permissive OpenApp fallback
    // catches the phrase — "the pr list." resolved to a cached app target.
    let text = strip_trailing_punctuation(&text);

    if text.is_empty() {
        return None;
    }

    // Strip leading filler words that STT often inserts (e.g. "And analyse
    // PR 254 in zync", "So open chrome", "But first close notepad").
    // These conversational connectors are not part of the command and cause
    // every starts_with() check below to fail.
    let text = strip_leading_filler(&text);

    // --- Open Architecture Mapper ---
    if is_architect_command(&text) {
        return Some(ParseResult {
            intent: ParsedIntent::OpenArchitect,
            confidence: 1.0,
            source: "deterministic".to_string(),
        });
    }

    // --- Fuzzy match for architecture mapper (STT mishearings) ---
    // faster-whisper tiny.en often mishears "architecture mapper" as:
    //   "octach at mapper", "architecture mapper", "arcade mapper", etc.
    // Check for the pattern: (open|launch|start|show) + <garbled> + "mapper"
    if is_architect_fuzzy(&text) {
        return Some(ParseResult {
            intent: ParsedIntent::OpenArchitect,
            confidence: 0.85,
            source: "deterministic-fuzzy".to_string(),
        });
    }

    // --- Open Settings / Command Center ---
    // "open settings" / "show settings" / "open command center" / "open preferences"
    // "configure NEXUS" / "NEXUS settings" / "open config" / "show preferences"
    if is_settings_command(&text) {
        return Some(ParseResult {
            intent: ParsedIntent::OpenSettings,
            confidence: 1.0,
            source: "deterministic".to_string(),
        });
    }

    // --- Ghostwriter room entry ---
    // Must precede greeting/media: "write this down" is dictation, not chat.
    if let Some(result) = parse_ghostwriter_entry(&text) {
        return Some(result);
    }

    // --- Screen control (click Nth option, Nth tab) ---
    // Must precede live/open: "press the 3rd button" is grounding, not a
    // hotkey; "open 4th tab" is a tab switch, not an app.
    if let Some(result) = parse_screen_command(&text) {
        return Some(result);
    }

    // --- Media Control ---
    if let Some(media) = parse_media(&text) {
        return Some(ParseResult {
            intent: media,
            confidence: 1.0,
            source: "deterministic".to_string(),
        });
    }

    // --- Greetings / conversational replies (local, no Worker round-trip) ---
    if let Some(result) = parse_greeting(&text) {
        return Some(result);
    }

    // --- Analyse commands ---
    // "analyse PR 23 servx", "analyse pr 23 in servx", "analyse pull request 23 servx"
    // "analyse servx repo", "analyse servx", "analyse owner/repo"
    // "analyse repo servx", "analyse the repo servx"
    // "analyse the pr in zync" (latest PR, no number)
    // "analyse the pr by prem in servx" (latest PR by author)
    if let Some(result) = parse_analyse_command(&text) {
        return Some(result);
    }

    // --- Branch commands ---
    // "check the latest branch of servx created by eesha"
    // "check latest branch by eesha in servx"
    // "show the latest branch of servx by eesha"
    // "what is the latest branch of servx created by eesha"
    if let Some(result) = parse_branch_command(&text) {
        return Some(result);
    }

    // --- Social: send WhatsApp message ---
    // "send mom a whatsapp message saying i'll be late",
    // "whatsapp dad saying i'm coming", "message mom on whatsapp saying hi"
    // Must be BEFORE parse_whatsapp_command — "whatsapp mom saying X" would
    // otherwise match the "whatsapp " prefix and swallow the whole message
    // as the contact name.
    if let Some(result) = parse_send_whatsapp_message(&text) {
        return Some(result);
    }

    // --- WhatsApp chat (must be BEFORE open command ΓÇö "open chat with X" would match open) ---
    // "open chat with lakshya", "message lakshya on whatsapp", "chat with mom"
    if let Some(result) = parse_whatsapp_command(&text) {
        return Some(result);
    }

    // --- GitHub commands ---
    // "merge PR 23 in owner/repo", "approve PR 5 in servx",
    // "close PR 10 in zync", "list PRs in owner/repo",
    // "add user X as collaborator to owner/repo", etc.
    // Must be BEFORE open/close app — "close PR 10" and "show PR 42"
    // would match close_app / open_app respectively.
    if let Some(result) = parse_github_command(&text) {
        return Some(result);
    }

    // --- Commerce: order food (Swiggy) ---
    // "order pizza from dominos", "order food from swiggy",
    // "order biryani", "get food from swiggy"
    if let Some(result) = parse_order_food(&text) {
        return Some(result);
    }

    // --- Commerce: product search (Amazon) ---
    // "search for sony headphones on amazon",
    // "find wireless earbuds on amazon", "amazon search for laptop"
    if let Some(result) = parse_search_product(&text) {
        return Some(result);
    }

    // --- Live mode commands ---
    // "type hello world", "press enter", "press ctrl a"
    // "send", "new tab", "open new tab"
    // Must be BEFORE open/close — "open new tab" would match parse_open_command.
    // "cancel" and "never mind" are already handled by the greeting parser
    // above (they return a Greeting with "Very well, sir." which is appropriate).
    if let Some(result) = parse_live_command(&text) {
        return Some(result);
    }

    // --- Open app / URL ---
    // "open whatsapp", "launch gemini", "start calculator", etc.
    if let Some(result) = parse_open_command(&text) {
        return Some(result);
    }

    // --- Close app ---
    // "close whatsapp", "quit chrome", "exit notepad"
    if let Some(result) = parse_close_command(&text) {
        return Some(result);
    }

    // --- Search ---
    // "search for cats", "google cats", "look up cats"
    if let Some(result) = parse_search_command(&text) {
        return Some(result);
    }

    None
}

// ΓöÇΓöÇΓöÇ Open command ΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇ

/// Verbs that trigger an "open" command.
const OPEN_VERBS: &[&str] = &[
    "open", "launch", "start", "run", "fire up", "bring up", "show", "pull up",
    "go to", "visit", "browse to", "navigate to",
];

/// Heuristic: does this target phrase look like a GitHub command that
/// `parse_github_command` somehow missed? Used as a defensive guard in
/// `parse_open_command` so we don't launch a wrong app (e.g. Dribbble
/// for "the pr list.") when the GitHub regex failed on a punctuation or
/// phrasing variant.
///
/// Matches phrases containing PR/pull-request/repo keywords. Returns
/// `true` for things like "the pr list", "pull requests", "open prs",
/// "the repo list", "my prs".
fn looks_like_github_phrase(s: &str) -> bool {
    let s = s.trim().to_lowercase();
    if s.is_empty() {
        return false;
    }
    // Word-boundary checks so "pr" doesn't match inside "preview" / "process".
    // "pr" as a standalone token or followed by "s"/"list"/"number"/"#".
    let tokens: Vec<&str> = s.split_whitespace().collect();
    let has_pr = tokens.iter().any(|&t| {
        t == "pr" || t == "prs" || t == "pr's"
            || t == "pr-list" || t == "prlist"
            || t.starts_with("pr#")
            || t == "pull" // "pull request(s)" — check bigram below
    });
    let has_pull_request = s.contains("pull request") || s.contains("pull requests");
    let has_repo_phrase = tokens.iter().any(|&t| {
        t == "repo" || t == "repos" || t == "repository" || t == "repositories"
    }) || s.contains("repo list");
    // "pull request(s)" bigram
    let has_pull_bigram = tokens.windows(2).any(|w| {
        w[0] == "pull" && (w[1] == "request" || w[1] == "requests")
    });
    has_pr || has_pull_request || has_pull_bigram || has_repo_phrase
}

fn parse_open_command(text: &str) -> Option<ParseResult> {
    // Try each open verb
    for verb in OPEN_VERBS {
        let prefix = format!("{} ", verb);
        if text.starts_with(&prefix) {
            let target = &text[prefix.len()..];
            let target = target.trim();

            // Strip trailing "app", "application", "for me"
            let cleaned = strip_trailing_app_words(target);

            // Check for "in browser" / "website" / "site" escape hatch
            if let Some(result) = parse_browser_force(&cleaned) {
                return Some(result);
            }

            // Strip trailing "website"/"site"
            let cleaned_no_site = strip_trailing_site(&cleaned);

            // Direct URL: has a dot, no spaces
            if is_url_like(&cleaned_no_site) {
                let url = if cleaned_no_site.starts_with("http") {
                    cleaned_no_site.clone()
                } else {
                    format!("https://{}", cleaned_no_site)
                };
                return Some(ParseResult {
                    intent: ParsedIntent::OpenUrl {
                        target: cleaned_no_site.clone(),
                        url,
                    },
                    confidence: 1.0,
                    source: "deterministic".to_string(),
                });
            }

            // App name ΓÇö resolve against the app registry
            // Defensive guard: if the target looks like a GitHub command
            // phrase that the GitHub parser somehow missed (e.g. STT
            // punctuation variants, unusual phrasing), do NOT treat it as
            // an app name. Returning None lets the NLU/brain fallback
            // classify it correctly instead of launching a wrong app.
            if looks_like_github_phrase(&cleaned_no_site) {
                tracing::info!(
                    "[intent_parser] open-command fallback: '{}' looks like a GitHub phrase, skipping app resolution",
                    cleaned_no_site
                );
                return None;
            }

            let resolved = resolve_app_name(&cleaned_no_site);
            return Some(ParseResult {
                intent: ParsedIntent::OpenApp {
                    target: resolved.unwrap_or_else(|| cleaned_no_site.to_string()),
                },
                confidence: 1.0,
                source: "deterministic".to_string(),
            });
        }
    }

    None
}

/// Resolve an app name using the app registry with fuzzy matching.
/// Falls back to the original text if no match is found.
fn resolve_app_name(name: &str) -> Option<String> {
    let name = name.trim();

    // 1. Direct registry lookup (handles exact, prefix, contains, Levenshtein)
    if let Some(entry) = app_registry::lookup(name) {
        // Return the display name (lowercased) — NOT search_names.first().
        // The first search name is alphabetical ("creative" for a Dribbble
        // PWA title), which launders any fuzzy hit into a nonsense target
        // that then self-resolves forever. Display name round-trips through
        // lookup() exactly (it's always names[0]).
        return Some(entry.display_name.to_lowercase());
    }

    // 2. Phonetic correction against the app registry
    // This handles Whisper mishearings like "what's app" ΓåÆ "whatsapp"
    if let Some(corrected) = phonetic_app_lookup(name) {
        tracing::debug!("phonetic app match: '{}' ΓåÆ '{}'", name, corrected);
        return Some(corrected);
    }

    // 3. Try with spaces removed/added (e.g. "whats app" ΓåÆ "whatsapp", "googlechrome" ΓåÆ "google chrome")
    if let Some(corrected) = space_variation_lookup(name) {
        tracing::debug!("space variation match: '{}' ΓåÆ '{}'", name, corrected);
        return Some(corrected);
    }

    None
}

/// Try looking up the app name with space variations.
/// "whats app" ΓåÆ try "whatsapp", "googlechrome" ΓåÆ try "google chrome"
fn space_variation_lookup(name: &str) -> Option<String> {
    // Remove all spaces: "what's app" ΓåÆ "what'sapp"
    let no_spaces = name.replace(' ', "");
    if no_spaces != name {
        if let Some(entry) = app_registry::lookup(&no_spaces) {
            return Some(entry.display_name.to_lowercase());
        }
    }

    // Try adding a space at common boundaries (consonantΓåÆvowel transitions)
    // This is a simple heuristic for compound words
    let chars: Vec<char> = name.chars().collect();
    for i in 1..chars.len() {
        let prev = chars[i - 1];
        let curr = chars[i];
        // Insert space between consonant and vowel (e.g. "googlechrome" ΓåÆ "google chrome")
        if !is_vowel(prev) && is_vowel(curr) {
            let mut modified = name[..i].to_string();
            modified.push(' ');
            modified.push_str(&name[i..]);
            if let Some(entry) = app_registry::lookup(&modified) {
                return Some(entry.display_name.to_lowercase());
            }
        }
    }

    None
}

fn is_vowel(c: char) -> bool {
    matches!(c.to_ascii_lowercase(), 'a' | 'e' | 'i' | 'o' | 'u' | 'y')
}

/// Phonetic app lookup ΓÇö tries to match the spoken word against app names
/// using simple phonetic similarity (sound-alike matching).
///
/// This is a lightweight alternative to Double Metaphone that works against
/// the live app registry instead of a fixed list.
fn phonetic_app_lookup(name: &str) -> Option<String> {
    let name_lower = name.to_lowercase();
    let name_pho = simple_phonetic(&name_lower);

    // Get all app names from the registry
    let search_names = app_registry::all_search_names();

    let mut best_match: Option<(String, usize)> = None;

    for search_name in &search_names {
        let app_pho = simple_phonetic(search_name);
        if app_pho.is_empty() || name_pho.is_empty() {
            continue;
        }

        // Exact phonetic match
        if app_pho == name_pho {
            let score = if search_name.len() == name_lower.len() {
                3
            } else {
                2
            };
            if best_match.as_ref().map_or(true, |b| score > b.1) {
                best_match = Some((search_name.to_string(), score));
            }
        }
        // Partial phonetic match (first 2 chars)
        else if app_pho.len() >= 2 && name_pho.len() >= 2 {
            if app_pho[..2] == name_pho[..2] {
                let dist = levenshtein(&name_lower, search_name);
                if dist <= 3 && dist < name_lower.len() / 2 + 1 {
                    let score = 1;
                    if best_match.as_ref().map_or(true, |b| score > b.1) {
                        best_match = Some((search_name.to_string(), score));
                    }
                }
            }
        }
    }

    best_match.map(|(name, _)| name)
}

/// Simple phonetic encoding ΓÇö removes vowels and normalizes consonant clusters.
/// This is a very lightweight phonetic representation (not as sophisticated as
/// Double Metaphone, but good enough for app name matching against the registry).
fn simple_phonetic(word: &str) -> String {
    let w = word.to_uppercase();
    let mut result = String::new();
    let chars: Vec<char> = w.chars().filter(|c| c.is_alphabetic()).collect();

    for (i, &c) in chars.iter().enumerate() {
        if i == 0 {
            result.push(c);
            continue;
        }

        // Skip vowels (except at start)
        if is_vowel(c) {
            continue;
        }

        // Normalize consonant clusters
        let prev = chars[i - 1];
        match c {
            // C and K sound the same
            'C' => {
                if prev != 'C' && prev != 'K' {
                    result.push('K');
                }
            }
            'K' => {
                if prev != 'C' && prev != 'K' {
                    result.push('K');
                }
            }
            // PH ΓåÆ F
            'H' => {
                if prev == 'P' {
                    // Replace last P with F
                    if let Some(last) = result.chars().last() {
                        if last == 'P' {
                            result.pop();
                            result.push('F');
                        }
                    }
                }
            }
            // Skip duplicate consonants
            _ => {
                if result.chars().last() != Some(c) {
                    result.push(c);
                }
            }
        }
    }

    result
}

// ΓöÇΓöÇΓöÇ Analyse command ΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇ

/// Parse "analyse" commands:
/// - "analyse PR 23 servx" / "analyse pr 23 in servx" / "analyse pull request 23 servx"
/// - "analyse servx repo" / "analyse the repo servx" / "analyse repo servx"
/// - "analyse servx" / "analyse owner/repo"
/// - "analyse PR 23 owner/repo"
/// - "deep analysis PR 24 in servx" / "deep analyse PR 24 in servx"
/// - "analysis PR 24 in servx" (noun form)
fn parse_analyse_command(text: &str) -> Option<ParseResult> {
    // Must start with "analyse", "analyze", "analysis", "deep analyse",
    // "deep analyze", or "deep analysis"
    let analyse_text = if text.starts_with("analyse ") {
        &text[8..]
    } else if text.starts_with("analyze ") {
        &text[8..]
    } else if text.starts_with("analysis ") {
        &text[9..]
    } else if text.starts_with("deep analyse ") {
        &text[13..]
    } else if text.starts_with("deep analyze ") {
        &text[13..]
    } else if text.starts_with("deep analysis ") {
        &text[14..]
    } else {
        return None;
    };

    let analyse_text = analyse_text.trim();

    // Pattern 1: "PR <num> [in|of|for|from] <repo>" or "pull request <num> ..."
    // e.g. "PR 23 servx", "PR 23 in servx", "pull request 23 servx"
    if let Some(result) = parse_pr_analyse(analyse_text) {
        return Some(result);
    }

    // Pattern 2: "<owner>/<repo>" ΓÇö e.g. "zync-meet/zync", "eesh264/congi"
    if let Some(result) = parse_owner_repo_analyse(analyse_text) {
        return Some(result);
    }

    // Pattern 3: "<repo> repo" or "repo <repo>" or "the repo <repo>"
    // e.g. "servx repo", "repo servx", "the repo servx"
    if let Some(result) = parse_repo_keyword_analyse(analyse_text) {
        return Some(result);
    }

    // Pattern 3b: "the pr [by|of|from] <author> in <repo>" or "the pr in <repo>"
    // or "latest pr [by|of|from] <author> in <repo>" or "latest pr in <repo>"
    // or "the latest pr ..." / "the pull request ..." / "latest pull request ..."
    // These are "latest PR" commands — no PR number, fetch the most recent PR.
    if let Some(result) = parse_latest_pr_analyse(analyse_text) {
        return Some(result);
    }

    // Pattern 4: Just "<repo>" ΓÇö e.g. "analyse servx", "analyse zync"
    // Treat the whole remaining text as the repo name
    let repo = clean_repo_name(analyse_text);
    if !repo.is_empty() {
        return Some(ParseResult {
            intent: ParsedIntent::AnalyseRepo {
                owner: None,
                repo,
            },
            confidence: 0.9, // slightly lower ΓÇö we're guessing this is a repo name
            source: "deterministic".to_string(),
        });
    }

    None
}

/// Known repos for fuzzy matching. These are the user's commonly-analyzed repos.
/// In production, this could be populated from GitHub OAuth (user's repos).
const KNOWN_REPOS: &[&str] = &[
    "nexus",
    "ultron",
    "servx",
    "zync",
    "ledger-ai",
    "nexus-agent",
];

/// Parse "PR <num> [in|of|for|from|on] <repo>" patterns.
fn parse_pr_analyse(text: &str) -> Option<ParseResult> {
    // Match: "PR <num> [in|of|for|from|on] <repo>" or "pull request <num> ..."
    // Also handles "PR number <num>" and "PR # <num>" (STT variations)
    // Also handles "the PR" (user says "analyse the pr 254 in zync")
    let pr_patterns: &[&str] = &[
        // "PR number 24 on NEXUS agent" / "PR number 24 in repo"
        r"^pr\s*(?:number|#\s*)?\s*#?\s*(\d+)\s+(?:in|of|for|from|on)\s+(.+)$",
        // "PR number 24 NEXUS agent" (no preposition)
        r"^pr\s*number\s*#?\s*(\d+)\s+(.+)$",
        r"^pr\s*#?\s*(\d+)\s+(?:in|of|for|from)\s+(.+)$",
        r"^pr\s*#?\s*(\d+)\s+(.+)$",
        r"^pull\s+request\s*#?\s*(\d+)\s+(?:in|of|for|from|on)\s+(.+)$",
        r"^pull\s+request\s*#?\s*(\d+)\s+(.+)$",
        // "PR <num> owner/repo"
        r"^pr\s*#?\s*(\d+)\s+(\S+/\S+)$",
        // "the PR <num> in <repo>" ΓÇö user says "analyse the pr 254 in zync"
        r"^the\s+pr\s*#?\s*(\d+)\s+(?:in|of|for|from|on)\s+(.+)$",
        // "the PR <num> <repo>" (no preposition)
        r"^the\s+pr\s*#?\s*(\d+)\s+(.+)$",
        // "the pull request <num> in <repo>"
        r"^the\s+pull\s+request\s*#?\s*(\d+)\s+(?:in|of|for|from|on)\s+(.+)$",
    ];

    for &pat in pr_patterns {
        if let Some(caps) = regex_captures(text, pat) {
            let pr_number: u32 = caps[1].parse().ok()?;
            let repo_part = caps[2].trim();

            // Check if repo_part is owner/repo format
            if let Some((owner, repo)) = parse_owner_repo(repo_part) {
                return Some(ParseResult {
                    intent: ParsedIntent::AnalysePr {
                        owner: Some(owner),
                        repo,
                        pr_number,
                    },
                    confidence: 1.0,
                    source: "deterministic".to_string(),
                });
            }

            // Just repo name ΓÇö try exact match first
            let repo = clean_repo_name(repo_part);
            if !repo.is_empty() {
                // If the repo isn't an exact known repo, try fuzzy matching
                // against known repos. This catches STT mishearings like
                // "zink" ΓåÆ "zync" that haven't been learned yet.
                let lower_repo = repo.to_lowercase();
                if !KNOWN_REPOS.contains(&lower_repo.as_str()) {
                    if let Some(fuzzy_repo) = fuzzy_match_repo_name(&lower_repo) {
                        tracing::info!(
                            "intent_parser: fuzzy matched repo '{}' ΓåÆ '{}' in PR command",
                            repo,
                            fuzzy_repo
                        );
                        return Some(ParseResult {
                            intent: ParsedIntent::AnalysePr {
                                owner: None,
                                repo: fuzzy_repo,
                                pr_number,
                            },
                            confidence: 0.8,
                            source: "fuzzy".to_string(),
                        });
                    }
                }

                return Some(ParseResult {
                    intent: ParsedIntent::AnalysePr {
                        owner: None,
                        repo,
                        pr_number,
                    },
                    confidence: 1.0,
                    source: "deterministic".to_string(),
                });
            }
        }
    }

    None
}

/// Fuzzy-match a repo name against known repos using Levenshtein distance.
/// Returns the matched repo name if within threshold, None otherwise.
fn fuzzy_match_repo_name(repo: &str) -> Option<String> {
    for &known in KNOWN_REPOS {
        let dist = levenshtein(repo, known);
        // Threshold: 2 for short repos (≤6 chars), 3 for longer.
        // STT mishearings are primarily handled in the frontend
        // (correctSttTranscript) before reaching the parser.
        // The fuzzy matcher is a safety net for residual mishearings.
        let threshold = if known.len() <= 6 { 2 } else { 3 };
        if dist <= threshold && dist > 0 {
            return Some(known.to_string());
        }
    }
    None
}

/// Parse "latest PR" commands — PR without a number, optionally filtered by author.
///
/// Patterns:
/// - "the pr in <repo>" → latest PR in repo
/// - "the pr [of|by|from] <author> in <repo>" → latest PR by author in repo
/// - "latest pr in <repo>" → latest PR in repo
/// - "latest pr [of|by|from] <author> in <repo>" → latest PR by author in repo
/// - "the latest pr in <repo>" → latest PR in repo
/// - "the latest pr [of|by|from] <author> in <repo>" → latest PR by author in repo
/// - "the pull request in <repo>" → latest PR in repo
/// - "the pull request [of|by|from] <author> in <repo>" → latest PR by author in repo
/// - "latest pull request in <repo>" → latest PR in repo
/// - "the pr of <repo>" → latest PR in repo (when "of" is followed by a known repo)
/// - "pr in <repo>" → latest PR in repo (no "the" / "latest")
/// - "pr [of|by|from] <author> in <repo>" → latest PR by author in repo
fn parse_latest_pr_analyse(text: &str) -> Option<ParseResult> {
    // Must contain "pr" or "pull request" but NOT followed by a number
    // (if followed by a number, it's a specific PR, handled by parse_pr_analyse)

    // Strip leading "the " if present
    let text = text.strip_prefix("the ").unwrap_or(text);

    // Check if it starts with "latest " or "newest " or "recent " or "open " or "current "
    let text = text
        .strip_prefix("latest ")
        .or_else(|| text.strip_prefix("newest "))
        .or_else(|| text.strip_prefix("recent "))
        .or_else(|| text.strip_prefix("current "))
        .unwrap_or(text);

    // Now text should start with "pr " or "pull request "
    let after_pr = if let Some(rest) = text.strip_prefix("pr ") {
        rest
    } else if let Some(rest) = text.strip_prefix("pull request ") {
        rest
    } else {
        return None; // Not a PR command
    };

    // Check that "pr" is NOT followed by a number (that's parse_pr_analyse's job)
    // If after_pr starts with a digit or "#", skip — it's a specific PR
    let after_pr_trimmed = after_pr.trim_start_matches('#').trim_start();
    if after_pr_trimmed.chars().next().map(|c| c.is_ascii_digit()).unwrap_or(false) {
        return None; // Has a PR number — not a "latest PR" command
    }

    // Patterns for extracting author and repo:
    // 1. "[of|by|from] <author> in <repo>"
    // 2. "in <repo>" (no author)
    // 3. "of <repo>" (when "of" is followed by a known repo, not a person)

    // Pattern 1: "[of|by|from] <author> in <repo>"
    if let Some(caps) = regex_captures(after_pr, r"^(?:of|by|from)\s+(\S+)\s+in\s+(.+)$") {
        let author = caps[1].trim().to_string();
        let repo_part = caps[2].trim();

        // Check if repo_part is owner/repo format
        if let Some((owner, repo)) = parse_owner_repo(repo_part) {
            return Some(ParseResult {
                intent: ParsedIntent::AnalyseLatestPr {
                    owner: Some(owner),
                    repo,
                    author: Some(author),
                },
                confidence: 1.0,
                source: "deterministic".to_string(),
            });
        }

        let repo = clean_repo_name(repo_part);
        if !repo.is_empty() {
            // Try fuzzy matching for repo name
            let lower_repo = repo.to_lowercase();
            let final_repo = if !KNOWN_REPOS.contains(&lower_repo.as_str()) {
                fuzzy_match_repo_name(&lower_repo).unwrap_or(repo)
            } else {
                repo
            };
            return Some(ParseResult {
                intent: ParsedIntent::AnalyseLatestPr {
                    owner: None,
                    repo: final_repo,
                    author: Some(author),
                },
                confidence: 0.95,
                source: "deterministic".to_string(),
            });
        }
    }

    // Pattern 2: "in <repo>" (no author)
    if let Some(caps) = regex_captures(after_pr, r"^in\s+(.+)$") {
        let repo_part = caps[1].trim();

        if let Some((owner, repo)) = parse_owner_repo(repo_part) {
            return Some(ParseResult {
                intent: ParsedIntent::AnalyseLatestPr {
                    owner: Some(owner),
                    repo,
                    author: None,
                },
                confidence: 1.0,
                source: "deterministic".to_string(),
            });
        }

        let repo = clean_repo_name(repo_part);
        if !repo.is_empty() {
            let lower_repo = repo.to_lowercase();
            let final_repo = if !KNOWN_REPOS.contains(&lower_repo.as_str()) {
                fuzzy_match_repo_name(&lower_repo).unwrap_or(repo)
            } else {
                repo
            };
            return Some(ParseResult {
                intent: ParsedIntent::AnalyseLatestPr {
                    owner: None,
                    repo: final_repo,
                    author: None,
                },
                confidence: 0.95,
                source: "deterministic".to_string(),
            });
        }
    }

    // Pattern 3: "of <repo>" (when "of" is followed by a known repo)
    // This is ambiguous — "of prem" could be author "prem" or repo "prem"
    // Only treat as repo if it matches a KNOWN_REPO
    if let Some(caps) = regex_captures(after_pr, r"^of\s+(.+)$") {
        let repo_part = caps[1].trim();
        let lower_repo = repo_part.to_lowercase();
        if KNOWN_REPOS.contains(&lower_repo.as_str()) {
            return Some(ParseResult {
                intent: ParsedIntent::AnalyseLatestPr {
                    owner: None,
                    repo: repo_part.to_string(),
                    author: None,
                },
                confidence: 0.9,
                source: "deterministic".to_string(),
            });
        }
        // If not a known repo, "of <word>" is likely an author — but we need a repo too
        // Check if there's "in <repo>" after the author
        // This is already handled by Pattern 1 above
    }

    // Pattern 4: Just "<repo>" (no preposition) — e.g. "latest pr zync"
    // This handles "analyse latest pr zync" where "latest" was stripped and "pr" was stripped,
    // leaving just "zync"
    let repo = clean_repo_name(after_pr);
    if !repo.is_empty() {
        let lower_repo = repo.to_lowercase();
        // Only accept if it's a known repo or fuzzy-matches one — otherwise
        // "pr something" could be garbage
        if KNOWN_REPOS.contains(&lower_repo.as_str()) {
            return Some(ParseResult {
                intent: ParsedIntent::AnalyseLatestPr {
                    owner: None,
                    repo,
                    author: None,
                },
                confidence: 0.85,
                source: "deterministic".to_string(),
            });
        }
        if let Some(fuzzy_repo) = fuzzy_match_repo_name(&lower_repo) {
            return Some(ParseResult {
                intent: ParsedIntent::AnalyseLatestPr {
                    owner: None,
                    repo: fuzzy_repo,
                    author: None,
                },
                confidence: 0.8,
                source: "fuzzy".to_string(),
            });
        }
    }

    None
}

/// Parse "check branch" / "show branch" / "what is branch" commands.
///
/// Patterns:
/// - "check [the] [latest|recent|newest] branch [of|in] <repo> [created] [by] <author>"
/// - "check [the] [latest|recent|newest] branch [by] <author> [in|of] <repo>"
/// - "show [the] [latest|recent|newest] branch [of|in] <repo> [created] [by] <author>"
/// - "what is [the] [latest|recent|newest] branch [of|in] <repo> [created] [by] <author>"
/// - "check [the] [latest|recent|newest] branch [of|in] <repo>" (no author — just latest branch)
fn parse_branch_command(text: &str) -> Option<ParseResult> {
    // Must start with "check", "show", or "what is"
    let branch_text = if let Some(rest) = text.strip_prefix("check ") {
        rest
    } else if let Some(rest) = text.strip_prefix("show ") {
        rest
    } else if let Some(rest) = text.strip_prefix("what is ") {
        rest
    } else if let Some(rest) = text.strip_prefix("what's ") {
        rest
    } else {
        return None;
    };

    // Must contain "branch" (or "branches")
    if !branch_text.contains("branch") && !branch_text.contains("branches") {
        return None;
    }

    // Strip "the " prefix
    let branch_text = branch_text.strip_prefix("the ").unwrap_or(branch_text);

    // Strip "latest" / "newest" / "recent" / "new"
    let branch_text = branch_text
        .strip_prefix("latest ")
        .or_else(|| branch_text.strip_prefix("newest "))
        .or_else(|| branch_text.strip_prefix("recent "))
        .or_else(|| branch_text.strip_prefix("new "))
        .unwrap_or(branch_text);

    // Strip "branch " or "branches "
    let after_branch = if let Some(rest) = branch_text.strip_prefix("branch ") {
        rest
    } else if let Some(rest) = branch_text.strip_prefix("branches ") {
        rest
    } else {
        // "branch" might be at the end with no trailing space
        if branch_text == "branch" || branch_text == "branches" {
            return None; // No repo specified
        }
        return None;
    };

    // Now after_branch should contain repo and/or author info.
    // Patterns:
    // A: "[of|in] <repo> [created] [by] <author>"
    // B: "[by] <author> [in|of] <repo>"
    // C: "[of|in] <repo>" (no author)

    // Strip "created " if present anywhere (user says "of servx created by eesha")
    let after_branch = after_branch.replace(" created ", " ");

    // Pattern A: "[of|in] <repo> [by] <author>"
    // The repo is everything between "of/in" and "by", or the rest if no "by"
    if let Some(caps) = regex_captures(&after_branch, r"^(?:of|in)\s+(.+?)\s+by\s+(\S+)$") {
        let repo_part = caps[1].trim();
        let author = caps[2].trim().to_string();

        if let Some((owner, repo)) = parse_owner_repo(repo_part) {
            return Some(ParseResult {
                intent: ParsedIntent::CheckBranch {
                    owner: Some(owner),
                    repo,
                    author: Some(author),
                },
                confidence: 1.0,
                source: "deterministic".to_string(),
            });
        }

        let repo = clean_repo_name(repo_part);
        if !repo.is_empty() {
            let lower_repo = repo.to_lowercase();
            let final_repo = if !KNOWN_REPOS.contains(&lower_repo.as_str()) {
                fuzzy_match_repo_name(&lower_repo).unwrap_or(repo)
            } else {
                repo
            };
            return Some(ParseResult {
                intent: ParsedIntent::CheckBranch {
                    owner: None,
                    repo: final_repo,
                    author: Some(author),
                },
                confidence: 0.95,
                source: "deterministic".to_string(),
            });
        }
    }

    // Pattern B: "[by] <author> [in|of] <repo>"
    if let Some(caps) = regex_captures(&after_branch, r"^by\s+(\S+)\s+(?:in|of)\s+(.+)$") {
        let author = caps[1].trim().to_string();
        let repo_part = caps[2].trim();

        if let Some((owner, repo)) = parse_owner_repo(repo_part) {
            return Some(ParseResult {
                intent: ParsedIntent::CheckBranch {
                    owner: Some(owner),
                    repo,
                    author: Some(author),
                },
                confidence: 1.0,
                source: "deterministic".to_string(),
            });
        }

        let repo = clean_repo_name(repo_part);
        if !repo.is_empty() {
            let lower_repo = repo.to_lowercase();
            let final_repo = if !KNOWN_REPOS.contains(&lower_repo.as_str()) {
                fuzzy_match_repo_name(&lower_repo).unwrap_or(repo)
            } else {
                repo
            };
            return Some(ParseResult {
                intent: ParsedIntent::CheckBranch {
                    owner: None,
                    repo: final_repo,
                    author: Some(author),
                },
                confidence: 0.95,
                source: "deterministic".to_string(),
            });
        }
    }

    // Pattern C: "[of|in] <repo>" (no author — just latest branch)
    if let Some(caps) = regex_captures(&after_branch, r"^(?:of|in)\s+(.+)$") {
        let repo_part = caps[1].trim();

        // Strip trailing " by <something>" if present (already handled above, but just in case)
        let repo_part = if let Some(pos) = repo_part.find(" by ") {
            &repo_part[..pos]
        } else {
            repo_part
        };

        if let Some((owner, repo)) = parse_owner_repo(repo_part) {
            return Some(ParseResult {
                intent: ParsedIntent::CheckBranch {
                    owner: Some(owner),
                    repo,
                    author: None,
                },
                confidence: 0.9,
                source: "deterministic".to_string(),
            });
        }

        let repo = clean_repo_name(repo_part);
        if !repo.is_empty() {
            let lower_repo = repo.to_lowercase();
            let final_repo = if !KNOWN_REPOS.contains(&lower_repo.as_str()) {
                fuzzy_match_repo_name(&lower_repo).unwrap_or(repo)
            } else {
                repo
            };
            return Some(ParseResult {
                intent: ParsedIntent::CheckBranch {
                    owner: None,
                    repo: final_repo,
                    author: None,
                },
                confidence: 0.9,
                source: "deterministic".to_string(),
            });
        }
    }

    None
}

/// Parse "owner/repo" format.
fn parse_owner_repo_analyse(text: &str) -> Option<ParseResult> {
    if let Some((owner, repo)) = parse_owner_repo(text) {
        return Some(ParseResult {
            intent: ParsedIntent::AnalyseRepo {
                owner: Some(owner),
                repo,
            },
            confidence: 1.0,
            source: "deterministic".to_string(),
        });
    }
    None
}

/// Parse "<repo> repo" / "repo <repo>" / "the repo <repo>" patterns.
fn parse_repo_keyword_analyse(text: &str) -> Option<ParseResult> {
    // "the repo <name>" or "repo <name>"
    if let Some(rest) = text.strip_prefix("the repo ") {
        let repo = clean_repo_name(rest);
        if !repo.is_empty() {
            return Some(ParseResult {
                intent: ParsedIntent::AnalyseRepo {
                    owner: None,
                    repo,
                },
                confidence: 1.0,
                source: "deterministic".to_string(),
            });
        }
    }
    if let Some(rest) = text.strip_prefix("repo ") {
        let repo = clean_repo_name(rest);
        if !repo.is_empty() {
            return Some(ParseResult {
                intent: ParsedIntent::AnalyseRepo {
                    owner: None,
                    repo,
                },
                confidence: 1.0,
                source: "deterministic".to_string(),
            });
        }
    }
    // "<name> repo" ΓÇö trailing "repo" keyword
    if let Some(rest) = text.strip_suffix(" repo") {
        let repo = clean_repo_name(rest);
        if !repo.is_empty() {
            return Some(ParseResult {
                intent: ParsedIntent::AnalyseRepo {
                    owner: None,
                    repo,
                },
                confidence: 1.0,
                source: "deterministic".to_string(),
            });
        }
    }
    // "<name> repository"
    if let Some(rest) = text.strip_suffix(" repository") {
        let repo = clean_repo_name(rest);
        if !repo.is_empty() {
            return Some(ParseResult {
                intent: ParsedIntent::AnalyseRepo {
                    owner: None,
                    repo,
                },
                confidence: 1.0,
                source: "deterministic".to_string(),
            });
        }
    }

    None
}

/// Parse "owner/repo" string into (owner, repo).
fn parse_owner_repo(text: &str) -> Option<(String, String)> {
    let text = text.trim();
    if let Some(slash_idx) = text.find('/') {
        let owner = text[..slash_idx].trim().to_string();
        let repo = text[slash_idx + 1..].trim().to_string();
        // Validate: both parts should be non-empty and contain only valid chars
        if !owner.is_empty() && !repo.is_empty() && is_valid_repo_name(&owner) && is_valid_repo_name(&repo) {
            return Some((owner, repo));
        }
    }
    None
}

/// Check if a string is a valid GitHub repo/owner name.
/// GitHub names: alphanumeric, hyphens, underscores, dots.
fn is_valid_repo_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 100
        && name.chars().all(|c| c.is_alphanumeric() || c == '-' || c == '_' || c == '.')
        && !name.starts_with('-')
        && !name.starts_with('.')
}

/// Clean a repo name ΓÇö strip articles, trailing keywords, whitespace.
fn clean_repo_name(text: &str) -> String {
    let text = text.trim();
    // Strip leading "the "
    let text = text.strip_prefix("the ").unwrap_or(text);
    // Strip trailing "repo", "repository", "project", "codebase"
    let text = text
        .strip_suffix(" repo")
        .or_else(|| text.strip_suffix(" repository"))
        .or_else(|| text.strip_suffix(" project"))
        .or_else(|| text.strip_suffix(" codebase"))
        .unwrap_or(text);
    canonical_repo_name(text.trim())
}

/// Sound-alike aliases for known repos: STT (and fast speech) renders the
/// same name many ways — "servx" arrives as "cervix", "srvx", "service".
/// Every alias points at the canonical repo, so all soundings resolve to
/// the same entity. Applied per path segment (owner + name each), inside
/// EVERY category — deterministic and NLU paths alike.
fn repo_sound_alias(canonical: &str) -> Option<&'static str> {
    match canonical {
        "cervix" | "cervx" | "srvx" | "service" | "cervex" | "cervets"
        | "servetus" | "servex" | "survex" => Some("servx"),
        "zinc" | "zink" | "sync" | "zynk" | "zincs" => Some("zync"),
        "incognito" | "incognit" | "congy" | "conji" => Some("congi"),
        "meat" | "meets" => Some("meet"),
        "shopcart" => Some("shopkart"),
        "ledgerai" => Some("ledger-ai"),
        _ => None,
    }
}

/// Map a repo reference to its canonical form via the sound-alias table.
/// Unknown segments pass through untouched (never rewrites a real name).
pub fn canonical_repo_name(text: &str) -> String {
    let mapped: Vec<String> = text
        .split('/')
        .map(|seg| {
            let compact = seg.trim().to_lowercase().replace([' ', '-', '_'], "");
            // "ledger ai" compacts to "ledgerai" and hits the table.
            let plain = seg.trim().to_lowercase();
            repo_sound_alias(&compact)
                .or_else(|| repo_sound_alias(&plain))
                .map(str::to_string)
                .unwrap_or_else(|| seg.trim().to_string())
        })
        .collect();
    mapped.join("/")
}

// ΓöÇΓöÇΓöÇ Close app command ΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇ

const CLOSE_VERBS: &[&str] = &["close", "quit", "exit", "kill", "terminate", "end", "shut down", "shut"];

fn parse_close_command(text: &str) -> Option<ParseResult> {
    for verb in CLOSE_VERBS {
        let prefix = format!("{} ", verb);
        if text.starts_with(&prefix) {
            let target = text[prefix.len()..].trim();
            if !target.is_empty() && target != "nexus" && target != "the app" {
                return Some(ParseResult {
                    intent: ParsedIntent::CloseApp { target: target.to_string() },
                    confidence: 1.0,
                    source: "deterministic".to_string(),
                });
            }
        }
    }
    None
}

// ΓöÇΓöÇΓöÇ WhatsApp chat command ΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇ

fn parse_whatsapp_command(text: &str) -> Option<ParseResult> {
    // "open chat with lakshya", "chat with lakshya", "message lakshya"
    // "open my chat with lakshya", "whatsapp lakshya"
    let patterns: &[&str] = &[
        "open chat with ",
        "open my chat with ",
        "chat with ",
        "message ",
        "whatsapp ",
        "open whatsapp chat with ",
        "send message to ",
        "send whatsapp to ",
    ];
    for pat in patterns {
        if text.starts_with(pat) {
            let contact = text[pat.len()..].trim();
            // Strip trailing "on whatsapp"
            let contact = contact
                .strip_suffix(" on whatsapp")
                .or_else(|| text.strip_suffix(" on wa"))
                .unwrap_or(contact);
            if !contact.is_empty() {
                return Some(ParseResult {
                    intent: ParsedIntent::WhatsappChat { contact: contact.to_string() },
                    confidence: 1.0,
                    source: "deterministic".to_string(),
                });
            }
        }
    }
    None
}

// ΓöÇΓöÇΓöÇ Search command ΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇ

const SEARCH_VERBS: &[&str] = &[
    "search for", "search", "google", "look up", "find me", "find", "look for",
];

fn parse_search_command(text: &str) -> Option<ParseResult> {
    for verb in SEARCH_VERBS {
        let prefix = format!("{} ", verb);
        if text.starts_with(&prefix) {
            let query = text[prefix.len()..].trim();
            if !query.is_empty() {
                return Some(ParseResult {
                    intent: ParsedIntent::Search {
                        query: query.to_string(),
                    },
                    confidence: 1.0,
                    source: "deterministic".to_string(),
                });
            }
        }
    }
    None
}

// ─── GitHub command parsing ───────────────────────────────────────────

/// Parse GitHub sub-commands from natural language.
///
/// Supported patterns:
///   "merge PR <num> in <owner/repo>"        → MergePr
///   "squash merge PR <num> in <repo>"       → MergePr (squash)
///   "rebase merge PR <num> in <repo>"       → MergePr (rebase)
///   "approve PR <num> in <repo>"            → ApprovePr
///   "close PR <num> in <repo>"              → ClosePr
///   "list PRs in <repo>"                    → ListPrs
///   "list open PRs in <repo>"               → ListPrs (open)
///   "list closed PRs in <repo>"             → ListPrs (closed)
///   "get PR <num> in <repo>"                → GetPr
///   "show PR <num> in <repo>"               → GetPr
///   "create PR <title> from <head> to <base> in <repo>" → CreatePr
///   "comment on PR <num> in <repo>: <body>" → CommentPr
///   "list PR files for PR <num> in <repo>"  → ListPrFiles
///   "update branch for PR <num> in <repo>"  → UpdateBranch
///   "revert PR <num> in <repo>"             → RevertPr
///   "add <user> as collaborator to <repo>"  → AddCollaborator
///   "remove <user> from collaborators in <repo>" → RemoveCollaborator
///   "list collaborators in <repo>"          → ListCollaborators
///   "add <user> to org <org>"               → AddOrgMember
///   "remove <user> from org <org>"          → RemoveOrgMember
///   "list members of org <org>"             → ListOrgMembers
///   "list branches in <repo>"               → ListBranches
///   "delete branch <name> in <repo>"        → DeleteBranch
///   "list releases in <repo>"               → ListReleases
///   "create release <tag> in <repo>"        → CreateRelease
///   "list workflows in <repo>"              → ListWorkflows
///   "list workflow runs in <repo>"          → ListWorkflowRuns
///   "rerun workflow <id> in <repo>"         → RerunWorkflow
///   "cancel workflow <id> in <repo>"        → CancelWorkflow
fn parse_github_command(text: &str) -> Option<ParseResult> {
    use crate::github_cmd::{CollaboratorPermission, GitHubCommand, MergeMethod, OrgRole};

    // Helper: extract "in <owner/repo>" or "in <repo>" from the end of text.
    // Returns (repo, remaining_text_before_in).
    let extract_repo = |t: &str| -> Option<(String, String)> {
        // Try "in <owner/repo>" pattern
        if let Some(pos) = t.rfind(" in ") {
            let repo_part = t[pos + 4..].trim();
            let repo = clean_repo_name(repo_part);
            if !repo.is_empty() {
                return Some((repo, t[..pos].trim().to_string()));
            }
        }
        // Try "for <owner/repo>"
        if let Some(pos) = t.rfind(" for ") {
            let repo_part = t[pos + 5..].trim();
            let repo = clean_repo_name(repo_part);
            if !repo.is_empty() {
                return Some((repo, t[..pos].trim().to_string()));
            }
        }
        None
    };

    // --- Merge PR ---
    // "merge PR 23 in owner/repo"
    // "squash merge PR 23 in owner/repo"
    // "rebase merge PR 23 in owner/repo"
    if let Some(caps) = regex_captures(text, r"^(?:(squash|rebase)\s+)?merge\s+(?:pr|pull\s+request)\s*#?\s*(\d+)(?:\s+in\s+(\S+))?$") {
        let method = match caps.get(1).map(|m| m.as_str()) {
            Some("squash") => MergeMethod::Squash,
            Some("rebase") => MergeMethod::Rebase,
            _ => MergeMethod::Merge,
        };
        let pr_number: u64 = caps[2].parse().ok()?;
        let repo = if let Some(r) = caps.get(3) {
            clean_repo_name(r.as_str())
        } else {
            // Try "in <repo>" from full text
            return extract_repo(text).map(|(repo, _)| ParseResult {
                intent: ParsedIntent::GitHubCommand {
                    command: GitHubCommand::MergePr { repo, pr_number, method },
                },
                confidence: 0.9,
                source: "deterministic".to_string(),
            });
        };
        if !repo.is_empty() {
            return Some(ParseResult {
                intent: ParsedIntent::GitHubCommand {
                    command: GitHubCommand::MergePr { repo, pr_number, method },
                },
                confidence: 0.95,
                source: "deterministic".to_string(),
            });
        }
    }

    // --- Approve PR ---
    if let Some(caps) = regex_captures(text, r"^approve\s+(?:pr|pull\s+request)\s*#?\s*(\d+)(?:\s+in\s+(\S+))?$") {
        let pr_number: u64 = caps[1].parse().ok()?;
        let repo = if let Some(r) = caps.get(2) {
            clean_repo_name(r.as_str())
        } else {
            return extract_repo(text).map(|(repo, _)| ParseResult {
                intent: ParsedIntent::GitHubCommand {
                    command: GitHubCommand::ApprovePr { repo, pr_number },
                },
                confidence: 0.9,
                source: "deterministic".to_string(),
            });
        };
        if !repo.is_empty() {
            return Some(ParseResult {
                intent: ParsedIntent::GitHubCommand {
                    command: GitHubCommand::ApprovePr { repo, pr_number },
                },
                confidence: 0.95,
                source: "deterministic".to_string(),
            });
        }
    }

    // --- Close PR ---
    if let Some(caps) = regex_captures(text, r"^close\s+(?:pr|pull\s+request)\s*#?\s*(\d+)(?:\s+in\s+(\S+))?$") {
        let pr_number: u64 = caps[1].parse().ok()?;
        let repo = if let Some(r) = caps.get(2) {
            clean_repo_name(r.as_str())
        } else {
            return extract_repo(text).map(|(repo, _)| ParseResult {
                intent: ParsedIntent::GitHubCommand {
                    command: GitHubCommand::ClosePr { repo, pr_number },
                },
                confidence: 0.9,
                source: "deterministic".to_string(),
            });
        };
        if !repo.is_empty() {
            return Some(ParseResult {
                intent: ParsedIntent::GitHubCommand {
                    command: GitHubCommand::ClosePr { repo, pr_number },
                },
                confidence: 0.95,
                source: "deterministic".to_string(),
            });
        }
    }

    // --- Get PR ---
    if let Some(caps) = regex_captures(text, r"^(?:get|show|tell\s+me\s+about)\s+(?:pr|pull\s+request)\s*#?\s*(\d+)(?:\s+in\s+(\S+))?$") {
        let pr_number: u64 = caps[1].parse().ok()?;
        let repo = if let Some(r) = caps.get(2) {
            clean_repo_name(r.as_str())
        } else {
            return extract_repo(text).map(|(repo, _)| ParseResult {
                intent: ParsedIntent::GitHubCommand {
                    command: GitHubCommand::GetPr { repo, pr_number },
                },
                confidence: 0.9,
                source: "deterministic".to_string(),
            });
        };
        if !repo.is_empty() {
            return Some(ParseResult {
                intent: ParsedIntent::GitHubCommand {
                    command: GitHubCommand::GetPr { repo, pr_number },
                },
                confidence: 0.95,
                source: "deterministic".to_string(),
            });
        }
    }

    // --- List PRs ---
    // Natural language patterns: "give me the pr list", "show live prs",
    // "show open prs", "latest prs", "what prs are open", "open pr list", etc.
    // These all map to ListPrs with a state filter.
    // "open pr list" is included because STT often transcribes "show" as "open"
    // and the user means "open the PR list sidebar", not "open an app called pr list".
    // "pull request(s)" is a full alternative to "prs" ("show me the pull
    // requests"), and trailing "and all / all of them / everything" is
    // tolerated ("show me the pull requests and all").
    if let Some(caps) = regex_captures(text, r"^(?:give\s+me\s+(?:the\s+)?|show\s+(?:me\s+)?(?:the\s+)?|get\s+(?:me\s+)?(?:the\s+)?|tell\s+me\s+(?:the\s+)?|what\s+(?:are\s+|is\s+)?(?:the\s+)?|open\s+(?:the\s+)?|view\s+(?:the\s+)?|fetch\s+(?:me\s+)?(?:the\s+)?|display\s+(?:the\s+)?|bring\s+(?:me\s+)?(?:the\s+)?|pull\s+up\s+(?:the\s+)?)?(?:(open|closed|all|live|latest|active|merged)\s+)?(?:the\s+)?(?:prs?(?:\s+list)?|pull\s+requests?(?:\s+list)?)(?:\s+(?:and\s+)?all(?:\s+of\s+them)?|\s+everything)?(?:\s+in\s+(\S+))?(?:\s+(?:and\s+)?all(?:\s+of\s+them)?|\s+everything)?$") {
        let raw_state = caps.get(1).map(|m| m.as_str().trim()).unwrap_or("open");
        let state = match raw_state {
            "live" | "active" | "open" => "open",
            "closed" => "closed",
            "all" => "all",
            "latest" => "open", // latest implies most recent open PRs
            "merged" => "closed", // merged PRs are closed
            _ => "open",
        }.to_string();
        let repo = if let Some(r) = caps.get(2) {
            clean_repo_name(r.as_str())
        } else {
            // Try "in <repo>" / "for <repo>" from text
            if let Some((repo, _)) = extract_repo(text) {
                return Some(ParseResult {
                    intent: ParsedIntent::GitHubCommand {
                        command: GitHubCommand::ListPrs { repo, state },
                    },
                    confidence: 0.9,
                    source: "deterministic".to_string(),
                });
            }
            // No repo in text — try auto-detection (browser URL, etc.)
            if let Some(repo_id) = crate::architect::get_active_repo_url() {
                let repo = format!("{}/{}", repo_id.owner, repo_id.repo);
                tracing::info!(
                    "[intent_parser] ListPrs: auto-detected repo '{}'",
                    repo
                );
                return Some(ParseResult {
                    intent: ParsedIntent::GitHubCommand {
                        command: GitHubCommand::ListPrs { repo, state },
                    },
                    confidence: 0.85,
                    source: "deterministic".to_string(),
                });
            }
            // No repo found at all — return with empty repo, let the GitHub
            // subsystem handle the error or use a default repo
            return Some(ParseResult {
                intent: ParsedIntent::GitHubCommand {
                    command: GitHubCommand::ListPrs { repo: String::new(), state },
                },
                confidence: 0.7,
                source: "deterministic".to_string(),
            });
        };
        if !repo.is_empty() {
            return Some(ParseResult {
                intent: ParsedIntent::GitHubCommand {
                    command: GitHubCommand::ListPrs { repo, state },
                },
                confidence: 0.95,
                source: "deterministic".to_string(),
            });
        }
    }

    // --- "what prs are open" / "which prs are open" / "are there any open prs" ---
    // Different word order: state comes AFTER "prs"
    if let Some(caps) = regex_captures(text, r"^(?:what|which|any)\s+prs?\s+(?:are\s+)?(?:(open|closed|all|live|active))?(?:\s+in\s+(\S+))?$") {
        let raw_state = caps.get(1).map(|m| m.as_str().trim()).unwrap_or("open");
        let state = match raw_state {
            "live" | "active" | "open" => "open",
            "closed" => "closed",
            "all" => "all",
            _ => "open",
        }.to_string();
        let repo = if let Some(r) = caps.get(2) {
            clean_repo_name(r.as_str())
        } else {
            if let Some((repo, _)) = extract_repo(text) {
                return Some(ParseResult {
                    intent: ParsedIntent::GitHubCommand {
                        command: GitHubCommand::ListPrs { repo, state },
                    },
                    confidence: 0.9,
                    source: "deterministic".to_string(),
                });
            }
            if let Some(repo_id) = crate::architect::get_active_repo_url() {
                let repo = format!("{}/{}", repo_id.owner, repo_id.repo);
                return Some(ParseResult {
                    intent: ParsedIntent::GitHubCommand {
                        command: GitHubCommand::ListPrs { repo, state },
                    },
                    confidence: 0.85,
                    source: "deterministic".to_string(),
                });
            }
            return Some(ParseResult {
                intent: ParsedIntent::GitHubCommand {
                    command: GitHubCommand::ListPrs { repo: String::new(), state },
                },
                confidence: 0.7,
                source: "deterministic".to_string(),
            });
        };
        if !repo.is_empty() {
            return Some(ParseResult {
                intent: ParsedIntent::GitHubCommand {
                    command: GitHubCommand::ListPrs { repo, state },
                },
                confidence: 0.95,
                source: "deterministic".to_string(),
            });
        }
    }

    // Original pattern: "list (open|closed|all) prs in <repo>"
    if let Some(caps) = regex_captures(text, r"^list\s+(open\s+|closed\s+|all\s+)?prs?(?:\s+in\s+(\S+))?$") {
        let state = caps.get(1).map(|m| m.as_str().trim()).unwrap_or("open").to_string();
        let repo = if let Some(r) = caps.get(2) {
            clean_repo_name(r.as_str())
        } else {
            // Try "in <repo>" / "for <repo>" from text
            if let Some((repo, _)) = extract_repo(text) {
                return Some(ParseResult {
                    intent: ParsedIntent::GitHubCommand {
                        command: GitHubCommand::ListPrs { repo, state },
                    },
                    confidence: 0.9,
                    source: "deterministic".to_string(),
                });
            }
            // No repo in text — try auto-detection
            if let Some(repo_id) = crate::architect::get_active_repo_url() {
                let repo = format!("{}/{}", repo_id.owner, repo_id.repo);
                return Some(ParseResult {
                    intent: ParsedIntent::GitHubCommand {
                        command: GitHubCommand::ListPrs { repo, state },
                    },
                    confidence: 0.85,
                    source: "deterministic".to_string(),
                });
            }
            return Some(ParseResult {
                intent: ParsedIntent::GitHubCommand {
                    command: GitHubCommand::ListPrs { repo: String::new(), state },
                },
                confidence: 0.7,
                source: "deterministic".to_string(),
            });
        };
        if !repo.is_empty() {
            return Some(ParseResult {
                intent: ParsedIntent::GitHubCommand {
                    command: GitHubCommand::ListPrs { repo, state },
                },
                confidence: 0.95,
                source: "deterministic".to_string(),
            });
        }
    }

    // --- Fuzzy "list" fallback ---
    // Catches STT mishearings where the user said "show me the pr list" or
    // "list prs" but STT heard something like "so you have to list" or
    // "show me the list". If the transcript contains "list" and doesn't
    // match any other pattern, try ListPrs with auto-detected repo.
    // This is a low-confidence fallback (0.6) — the brain/NLU can override.
    // IMPORTANT: This must NOT catch "list branches", "list releases", etc.
    // Those have their own patterns below. We exclude them here.
    if text.contains("list") {
        let lower = text.to_lowercase();
        // Skip if it looks like a different list command or non-PR list
        if lower.contains("to do") || lower.contains("todo") || lower.contains("shopping")
            || lower.contains("bucket") || lower.contains("wait") || lower.contains("listen")
            || lower.contains("branch") || lower.contains("release")
            || lower.contains("workflow") || lower.contains("collaborator")
            || lower.contains("file") || lower.contains("member")
            || lower.contains("run") || lower.contains("pr files") {
            // Not a PR list command — let the specific patterns handle it
        } else {
            // Try "in <repo>" / "for <repo>" first
            if let Some((repo, _)) = extract_repo(text) {
                return Some(ParseResult {
                    intent: ParsedIntent::GitHubCommand {
                        command: GitHubCommand::ListPrs {
                            repo,
                            state: "open".to_string(),
                        },
                    },
                    confidence: 0.6,
                    source: "deterministic-fuzzy".to_string(),
                });
            }
            // No repo in text — try auto-detection (browser URL, clipboard, etc.)
            // This is a blocking call, so we do it last.
            if let Some(repo_id) = crate::architect::get_active_repo_url() {
                let repo = format!("{}/{}", repo_id.owner, repo_id.repo);
                tracing::info!(
                    "[intent_parser] fuzzy 'list' fallback: auto-detected repo '{}'",
                    repo
                );
                return Some(ParseResult {
                    intent: ParsedIntent::GitHubCommand {
                        command: GitHubCommand::ListPrs {
                            repo,
                            state: "open".to_string(),
                        },
                    },
                    confidence: 0.6,
                    source: "deterministic-fuzzy".to_string(),
                });
            }
            // No repo found at all — return with empty repo for account-wide
            // PR search. Without this, "show me the pr list" (no repo) falls
            // through to Unknown even though it contains "list".
            tracing::info!(
                "[intent_parser] fuzzy 'list' fallback: no repo detected, returning account-wide ListPrs"
            );
            return Some(ParseResult {
                intent: ParsedIntent::GitHubCommand {
                    command: GitHubCommand::ListPrs {
                        repo: String::new(),
                        state: "open".to_string(),
                    },
                },
                confidence: 0.6,
                source: "deterministic-fuzzy".to_string(),
            });
        }
    }

    // --- List PR files ---
    // "list pr files for PR <num> in <repo>" or "list pr files <num> in <repo>"
    if let Some(caps) = regex_captures(text, r"^list\s+pr\s+files\s+(?:for\s+)?(?:pr\s+)?#?\s*(\d+)(?:\s+in\s+(\S+))?$") {
        let pr_number: u64 = caps[1].parse().ok()?;
        let repo = if let Some(r) = caps.get(2) {
            clean_repo_name(r.as_str())
        } else {
            return extract_repo(text).map(|(repo, _)| ParseResult {
                intent: ParsedIntent::GitHubCommand {
                    command: GitHubCommand::ListPrFiles { repo, pr_number },
                },
                confidence: 0.9,
                source: "deterministic".to_string(),
            });
        };
        if !repo.is_empty() {
            return Some(ParseResult {
                intent: ParsedIntent::GitHubCommand {
                    command: GitHubCommand::ListPrFiles { repo, pr_number },
                },
                confidence: 0.95,
                source: "deterministic".to_string(),
            });
        }
    }

    // --- Update branch ---
    if let Some(rest) = text.strip_prefix("update branch ") {
        let rest = rest.trim_start_matches("for ").trim();
        let rest = rest.trim_start_matches("pr ").trim();
        if let Ok(pr_number) = rest.parse::<u64>() {
            return extract_repo(text).map(|(repo, _)| ParseResult {
                intent: ParsedIntent::GitHubCommand {
                    command: GitHubCommand::UpdateBranch { repo, pr_number },
                },
                confidence: 0.9,
                source: "deterministic".to_string(),
            });
        }
    }

    // --- Revert PR ---
    if let Some(caps) = regex_captures(text, r"^revert\s+(?:pr|pull\s+request)\s*#?\s*(\d+)(?:\s+in\s+(\S+))?$") {
        let pr_number: u64 = caps[1].parse().ok()?;
        let repo = if let Some(r) = caps.get(2) {
            clean_repo_name(r.as_str())
        } else {
            return extract_repo(text).map(|(repo, _)| ParseResult {
                intent: ParsedIntent::GitHubCommand {
                    command: GitHubCommand::RevertPr { repo, pr_number, title: None },
                },
                confidence: 0.9,
                source: "deterministic".to_string(),
            });
        };
        if !repo.is_empty() {
            return Some(ParseResult {
                intent: ParsedIntent::GitHubCommand {
                    command: GitHubCommand::RevertPr { repo, pr_number, title: None },
                },
                confidence: 0.95,
                source: "deterministic".to_string(),
            });
        }
    }

    // --- Comment on PR ---
    // "comment on PR <num> in <repo> <body>" or "comment on PR <num> <body>"
    if let Some(caps) = regex_captures(text, r"^comment\s+on\s+(?:pr|pull\s+request)\s*#?\s*(\d+)\s+in\s+(\S+)\s+(.+)$") {
        let pr_number: u64 = caps[1].parse().ok()?;
        let repo = clean_repo_name(&caps[2]);
        let body = caps[3].trim().to_string();
        if !repo.is_empty() && !body.is_empty() {
            return Some(ParseResult {
                intent: ParsedIntent::GitHubCommand {
                    command: GitHubCommand::CommentPr { repo, pr_number, body },
                },
                confidence: 0.95,
                source: "deterministic".to_string(),
            });
        }
    }
    // "comment on PR <num> <body>" (repo extracted from context)
    if let Some(caps) = regex_captures(text, r"^comment\s+on\s+(?:pr|pull\s+request)\s*#?\s*(\d+)\s+(.+)$") {
        let pr_number: u64 = caps[1].parse().ok()?;
        let rest = caps[2].trim();
        // Try to split "in <repo> <body>" from "<body>"
        if let Some(pos) = rest.find(" in ") {
            let repo = clean_repo_name(&rest[..pos].trim());
            let body = rest[pos + 4..].trim().to_string();
            if !repo.is_empty() && !body.is_empty() {
                return Some(ParseResult {
                    intent: ParsedIntent::GitHubCommand {
                        command: GitHubCommand::CommentPr { repo, pr_number, body },
                    },
                    confidence: 0.9,
                    source: "deterministic".to_string(),
                });
            }
        }
        // No "in <repo>" — try extracting repo from full text
        return extract_repo(text).map(|(repo, _)| ParseResult {
            intent: ParsedIntent::GitHubCommand {
                command: GitHubCommand::CommentPr { repo, pr_number, body: rest.to_string() },
            },
            confidence: 0.85,
            source: "deterministic".to_string(),
        });
    }

    // --- Add collaborator ---
    // "add <user> as collaborator to <repo>" or "add <user> as admin to <repo>"
    // Also: "add <user> as admin to <repo>" (without "collaborator" keyword)
    if let Some(caps) = regex_captures(text, r"^add\s+(\S+)\s+as\s+(admin|push|pull|triage|maintain)?\s*(?:collaborator\s+)?to\s+(\S+)$") {
        let username = caps[1].to_string();
        let permission = match caps.get(2).map(|m| m.as_str().trim()) {
            Some("admin") => CollaboratorPermission::Admin,
            Some("push") => CollaboratorPermission::Push,
            Some("pull") => CollaboratorPermission::Pull,
            Some("triage") => CollaboratorPermission::Triage,
            Some("maintain") => CollaboratorPermission::Maintain,
            _ => CollaboratorPermission::Push,
        };
        let repo = clean_repo_name(&caps[3]);
        if !repo.is_empty() {
            return Some(ParseResult {
                intent: ParsedIntent::GitHubCommand {
                    command: GitHubCommand::AddCollaborator { repo, username, permission },
                },
                confidence: 0.95,
                source: "deterministic".to_string(),
            });
        }
    }

    // --- Remove collaborator ---
    // "remove <user> from <repo>" or "remove <user> as collaborator from <repo>"
    if let Some(caps) = regex_captures(text, r"^remove\s+(\S+)\s+(?:as\s+)?(?:collaborator\s+)?from\s+(\S+)$") {
        let username = caps[1].to_string();
        let repo = clean_repo_name(&caps[2]);
        if !repo.is_empty() {
            return Some(ParseResult {
                intent: ParsedIntent::GitHubCommand {
                    command: GitHubCommand::RemoveCollaborator { repo, username },
                },
                confidence: 0.95,
                source: "deterministic".to_string(),
            });
        }
    }

    // --- List collaborators ---
    if let Some(caps) = regex_captures(text, r"^list\s+collaborators\s+in\s+(\S+)$") {
        let repo = clean_repo_name(&caps[1]);
        if !repo.is_empty() {
            return Some(ParseResult {
                intent: ParsedIntent::GitHubCommand {
                    command: GitHubCommand::ListCollaborators { repo },
                },
                confidence: 0.95,
                source: "deterministic".to_string(),
            });
        }
    }

    // --- Add org member ---
    // "add <user> to org <org>" or "add <user> as admin to org <org>"
    if let Some(caps) = regex_captures(text, r"^add\s+(\S+)\s+(?:as\s+(admin|member)\s+)?to\s+org\s+(\S+)$") {
        let username = caps[1].to_string();
        let role = match caps.get(2).map(|m| m.as_str()) {
            Some("admin") => OrgRole::Admin,
            _ => OrgRole::Member,
        };
        let org = caps[3].to_string();
        return Some(ParseResult {
            intent: ParsedIntent::GitHubCommand {
                command: GitHubCommand::AddOrgMember { org, username, role },
            },
            confidence: 0.95,
            source: "deterministic".to_string(),
        });
    }

    // --- Remove org member ---
    if let Some(caps) = regex_captures(text, r"^remove\s+(\S+)\s+from\s+org\s+(\S+)$") {
        let username = caps[1].to_string();
        let org = caps[2].to_string();
        return Some(ParseResult {
            intent: ParsedIntent::GitHubCommand {
                command: GitHubCommand::RemoveOrgMember { org, username },
            },
            confidence: 0.95,
            source: "deterministic".to_string(),
        });
    }

    // --- List org members ---
    if let Some(caps) = regex_captures(text, r"^list\s+members\s+of\s+org\s+(\S+)$") {
        let org = caps[1].to_string();
        return Some(ParseResult {
            intent: ParsedIntent::GitHubCommand {
                command: GitHubCommand::ListOrgMembers { org },
            },
            confidence: 0.95,
            source: "deterministic".to_string(),
        });
    }

    // --- List branches ---
    if let Some(caps) = regex_captures(text, r"^list\s+branches\s+in\s+(\S+)$") {
        let repo = clean_repo_name(&caps[1]);
        if !repo.is_empty() {
            return Some(ParseResult {
                intent: ParsedIntent::GitHubCommand {
                    command: GitHubCommand::ListBranches { repo },
                },
                confidence: 0.95,
                source: "deterministic".to_string(),
            });
        }
    }

    // --- Delete branch ---
    if let Some(caps) = regex_captures(text, r"^delete\s+branch\s+(\S+)\s+in\s+(\S+)$") {
        let branch = caps[1].to_string();
        let repo = clean_repo_name(&caps[2]);
        if !repo.is_empty() {
            return Some(ParseResult {
                intent: ParsedIntent::GitHubCommand {
                    command: GitHubCommand::DeleteBranch { repo, branch },
                },
                confidence: 0.95,
                source: "deterministic".to_string(),
            });
        }
    }

    // --- List releases ---
    if let Some(caps) = regex_captures(text, r"^list\s+releases\s+in\s+(\S+)$") {
        let repo = clean_repo_name(&caps[1]);
        if !repo.is_empty() {
            return Some(ParseResult {
                intent: ParsedIntent::GitHubCommand {
                    command: GitHubCommand::ListReleases { repo },
                },
                confidence: 0.95,
                source: "deterministic".to_string(),
            });
        }
    }

    // --- Create release ---
    // "create release v1.0 in owner/repo" or "create release v1.0 in owner/repo with notes <body>"
    if let Some(caps) = regex_captures(text, r"^create\s+release\s+(\S+)\s+in\s+(\S+)(?:\s+with\s+notes\s+(.+))?$") {
        let tag = caps[1].to_string();
        let repo = clean_repo_name(&caps[2]);
        let body = caps.get(3).map(|m| m.as_str().trim().to_string());
        let name = tag.clone();
        if !repo.is_empty() {
            return Some(ParseResult {
                intent: ParsedIntent::GitHubCommand {
                    command: GitHubCommand::CreateRelease {
                        repo,
                        tag,
                        name,
                        body,
                        draft: false,
                        prerelease: false,
                        target_commitish: None,
                    },
                },
                confidence: 0.95,
                source: "deterministic".to_string(),
            });
        }
    }

    // --- List workflows ---
    if let Some(caps) = regex_captures(text, r"^list\s+workflows\s+in\s+(\S+)$") {
        let repo = clean_repo_name(&caps[1]);
        if !repo.is_empty() {
            return Some(ParseResult {
                intent: ParsedIntent::GitHubCommand {
                    command: GitHubCommand::ListWorkflows { repo },
                },
                confidence: 0.95,
                source: "deterministic".to_string(),
            });
        }
    }

    // --- List workflow runs ---
    if let Some(caps) = regex_captures(text, r"^list\s+workflow\s+runs\s+in\s+(\S+)$") {
        let repo = clean_repo_name(&caps[1]);
        if !repo.is_empty() {
            return Some(ParseResult {
                intent: ParsedIntent::GitHubCommand {
                    command: GitHubCommand::ListWorkflowRuns { repo, workflow_file: None },
                },
                confidence: 0.95,
                source: "deterministic".to_string(),
            });
        }
    }

    // --- Rerun workflow ---
    if let Some(caps) = regex_captures(text, r"^rerun\s+workflow\s+(\d+)\s+in\s+(\S+)$") {
        let run_id: u64 = caps[1].parse().ok()?;
        let repo = clean_repo_name(&caps[2]);
        if !repo.is_empty() {
            return Some(ParseResult {
                intent: ParsedIntent::GitHubCommand {
                    command: GitHubCommand::RerunWorkflow { repo, run_id },
                },
                confidence: 0.95,
                source: "deterministic".to_string(),
            });
        }
    }

    // --- Cancel workflow ---
    if let Some(caps) = regex_captures(text, r"^cancel\s+workflow\s+(\d+)\s+in\s+(\S+)$") {
        let run_id: u64 = caps[1].parse().ok()?;
        let repo = clean_repo_name(&caps[2]);
        if !repo.is_empty() {
            return Some(ParseResult {
                intent: ParsedIntent::GitHubCommand {
                    command: GitHubCommand::CancelWorkflow { repo, run_id },
                },
                confidence: 0.95,
                source: "deterministic".to_string(),
            });
        }
    }

    None
}

// ─── Commerce: order food (Swiggy) ─────────────────────────────────────

/// Parse food-ordering commands.
///
/// Patterns:
///   "order pizza from dominos"      → OrderFood { query: "pizza", restaurant: Some("dominos") }
///   "order food from swiggy"        → OrderFood { query: "", restaurant: None }
///   "order biryani"                 → OrderFood { query: "biryani", restaurant: None }
/// Parse food ordering commands (Swiggy MCP).
///
/// Patterns:
///   "order pizza from dominos"      → OrderFood { query: "pizza", restaurant: Some("dominos") }
///   "order a pizza from dominos"    → OrderFood { query: "pizza", restaurant: Some("dominos") }
///   "order food from swiggy"        → OrderFood { query: "", restaurant: None }
///   "order biryani on swiggy"       → OrderFood { query: "biryani", restaurant: None }
///   "order biryani"                 → OrderFood { query: "biryani", restaurant: None }
///   "get food from swiggy"          → OrderFood { query: "", restaurant: None }
///   "order food"                    → OrderFood { query: "", restaurant: None }
fn parse_order_food(text: &str) -> Option<ParseResult> {
    let mut clean = text.trim();
    for prefix in ["can you ", "could you ", "please ", "i want to "] {
        if let Some(rest) = clean.strip_prefix(prefix) {
            clean = rest.trim();
            break;
        }
    }

    // "order <dish> from <restaurant>" or "order <dish> on swiggy" or "order <dish>"
    if let Some(rest) = clean.strip_prefix("order ") {
        let rest = rest.trim_start_matches("a ").trim_start_matches("some ").trim();
        if let Some(pos) = rest.find(" from ") {
            let mut query = rest[..pos].trim();
            let restaurant = rest[pos + 6..].trim();
            if matches!(query, "food" | "something" | "lunch" | "dinner" | "something to eat") {
                query = "";
            }
            return Some(ParseResult {
                intent: ParsedIntent::OrderFood {
                    query: query.to_string(),
                    restaurant: if restaurant.is_empty() { None } else { Some(restaurant.to_string()) },
                },
                confidence: 0.95,
                source: "deterministic".to_string(),
            });
        }
        if let Some(pos) = rest.find(" on ") {
            let mut query = rest[..pos].trim();
            let restaurant = rest[pos + 4..].trim();
            if matches!(query, "food" | "something" | "lunch" | "dinner" | "something to eat") {
                query = "";
            }
            return Some(ParseResult {
                intent: ParsedIntent::OrderFood {
                    query: query.to_string(),
                    restaurant: if restaurant.is_empty() { None } else { Some(restaurant.to_string()) },
                },
                confidence: 0.95,
                source: "deterministic".to_string(),
            });
        }
        // "order biryani" (no restaurant specified)
        let mut query = rest.trim();
        if matches!(query, "food" | "something" | "lunch" | "dinner" | "something to eat") {
            query = "";
        }
        return Some(ParseResult {
            intent: ParsedIntent::OrderFood {
                query: query.to_string(),
                restaurant: None,
            },
            confidence: 0.90,
            source: "deterministic".to_string(),
        });
    }

    // "get me food from swiggy" / "get food from swiggy" / "get food"
    if let Some(rest) = clean.strip_prefix("get me food").or_else(|| clean.strip_prefix("get food")) {
        let restaurant = if let Some(r) = rest.strip_prefix(" from ") {
            r.trim()
        } else if let Some(r) = rest.strip_prefix(" on ") {
            r.trim()
        } else {
            ""
        };
        return Some(ParseResult {
            intent: ParsedIntent::OrderFood {
                query: "".to_string(),
                restaurant: if restaurant.is_empty() { None } else { Some(restaurant.to_string()) },
            },
            confidence: 0.90,
            source: "deterministic".to_string(),
        });
    }

    None
}

// ─── Commerce: product search (Amazon) ─────────────────────────────────

/// Parse Amazon product search commands.
///
/// Patterns:
///   "search for sony headphones on amazon" → SearchProduct { query: "sony headphones" }
///   "find wireless earbuds on amazon"      → SearchProduct { query: "wireless earbuds" }
///   "buy laptop on amazon"                 → SearchProduct { query: "laptop" }
///   "look for shoes on amazon"             → SearchProduct { query: "shoes" }
///   "amazon search for laptop"            → SearchProduct { query: "laptop" }
///   "search amazon for laptop"            → SearchProduct { query: "laptop" }
fn parse_search_product(text: &str) -> Option<ParseResult> {
    let mut clean = text.trim();
    for prefix in ["can you ", "could you ", "please ", "i want to "] {
        if let Some(rest) = clean.strip_prefix(prefix) {
            clean = rest.trim();
            break;
        }
    }

    // "search for <query> on amazon" / "find <query> on amazon" / "buy <query> on amazon" / "look for <query> on amazon"
    for verb in ["search for ", "find ", "buy ", "look for ", "search "] {
        if let Some(rest) = clean.strip_prefix(verb) {
            if let Some(query) = rest.strip_suffix(" on amazon").or_else(|| rest.strip_suffix(" in amazon")) {
                let query = query.trim();
                if !query.is_empty() {
                    return Some(ParseResult {
                        intent: ParsedIntent::SearchProduct {
                            query: query.to_string(),
                        },
                        confidence: 0.95,
                        source: "deterministic".to_string(),
                    });
                }
            }
        }
    }

    // "amazon search for <query>" / "amazon search <query>"
    if let Some(rest) = clean.strip_prefix("amazon search for ").or_else(|| clean.strip_prefix("amazon search ")) {
        return Some(ParseResult {
            intent: ParsedIntent::SearchProduct {
                query: rest.trim().to_string(),
            },
            confidence: 0.95,
            source: "deterministic".to_string(),
        });
    }

    // "search amazon for <query>" / "search on amazon for <query>"
    if let Some(rest) = clean.strip_prefix("search amazon for ").or_else(|| clean.strip_prefix("search on amazon for ")) {
        return Some(ParseResult {
            intent: ParsedIntent::SearchProduct {
                query: rest.trim().to_string(),
            },
            confidence: 0.95,
            source: "deterministic".to_string(),
        });
    }

    None
}

// ─── Ghostwriter room entry ──────────────────────────────────────────

/// Enter the persistent dictation room.
///
/// Triggers: "ghostwriter", "ghost writer", "take a letter",
/// "take dictation", "write this down", "start writing", "note this down",
/// "scribe", "type for me" — optionally "for <contact> [on whatsapp]".
/// Contact is optional; the room asks when missing.
fn parse_ghostwriter_entry(text: &str) -> Option<ParseResult> {
    // Ordered longest-first: "ghostwriter mode" must match before the
    // "ghostwriter" prefix leaves a stray " mode" tail (which rejects).
    const TRIGGERS: &[&str] = &[
        "open the ghostwriter mode",
        "open ghostwriter mode",
        "open the ghostwriter",
        "open the ghost mode",
        "take a letter",
        "write this down",
        "take dictation",
        "note this down",
        "ghostwriter mode",
        "enter ghostwriter",
        "start ghostwriter",
        "enable ghostwriter",
        "open ghostwriter",
        "start ghost mode",
        "open ghost mode",
        "go ghostwriter",
        "go ghost mode",
        "start writing",
        "ghostwriter on",
        "ghostwriter",
        "ghost writer",
        "ghost mode",
        "type for me",
        "scribe mode",
        "scribe",
    ];
    for trigger in TRIGGERS {
        if let Some(rest) = text.strip_prefix(trigger) {
            let rest = rest.trim();
            // Optional "for <contact> [on whatsapp]" tail.
            let mut contact: Option<String> = None;
            if let Some(after_for) = rest.strip_prefix("for ") {
                let c = after_for
                    .strip_suffix(" on whatsapp")
                    .or_else(|| after_for.strip_suffix(" whatsapp"))
                    .unwrap_or(after_for)
                    .trim();
                if !c.is_empty() {
                    contact = Some(c.to_string());
                }
            } else if !rest.is_empty() {
                // Trailing words that aren't a contact tail → not an entry.
                return None;
            }
            return Some(ParseResult {
                intent: ParsedIntent::EnterGhostwriter { contact },
                confidence: 0.95,
                source: "deterministic".to_string(),
            });
        }
    }
    None
}

// ─── Screen control (ordinal click, tab switch, read-back) ──────────

/// "click on the 3rd option", "press the 2nd button", "choose 1st",
/// "move to the 4th tab", "what's the 2nd link".
fn parse_screen_command(text: &str) -> Option<ParseResult> {
    // Tab switch: "move|go|switch|open to the Nth tab", "tab N".
    for prefix in [
        "move to the ",
        "move to ",
        "go to the ",
        "go to ",
        "switch to the ",
        "switch to ",
        "open ",
    ] {
        if let Some(rest) = text.strip_prefix(prefix) {
            // rest like "4th tab" / "4 tab"
            if let Some(num_part) = rest.strip_suffix(" tab") {
                if let Some(n) = crate::screen::parse_ordinal(num_part) {
                    if (1..=9).contains(&n) {
                        return Some(ParseResult {
                            intent: ParsedIntent::BrowserTab { index: n },
                            confidence: 0.95,
                            source: "deterministic".to_string(),
                        });
                    }
                }
            }
        }
    }
    if let Some(rest) = text.strip_prefix("tab ") {
        if let Some(n) = crate::screen::parse_ordinal(rest.trim()) {
            if (1..=9).contains(&n) {
                return Some(ParseResult {
                    intent: ParsedIntent::BrowserTab { index: n },
                    confidence: 0.9,
                    source: "deterministic".to_string(),
                });
            }
        }
    }

    // Read-back: "what's the 3rd option/button/link".
    for prefix in ["what's the ", "what is the ", "which is the ", "read the "] {
        if let Some(rest) = text.strip_prefix(prefix) {
            for noun in ["option", "button", "link", "tab", "item", "choice"] {
                if let Some(num_part) = rest.strip_suffix(&format!(" {noun}")) {
                    if let Some(n) = crate::screen::parse_ordinal(num_part) {
                        return Some(ParseResult {
                            intent: ParsedIntent::ScreenRead { ordinal: n },
                            confidence: 0.9,
                            source: "deterministic".to_string(),
                        });
                    }
                }
            }
        }
    }

    // Click: "click|press|choose|tap|select [on] [the] <ordinal> [noun]".
    for verb in ["click on the ", "click on ", "click the ", "click ", "press the ", "press ",
                 "choose the ", "choose ", "tap the ", "tap ", "select the ", "select "] {
        if let Some(rest) = text.strip_prefix(verb) {
            for noun in ["option", "button", "link", "tab", "item", "choice", "one"] {
                let num_part = rest
                    .strip_suffix(&format!(" {noun}"))
                    .unwrap_or(rest)
                    .trim();
                // Accept when a noun was present, the rest is a bare number
                // ("click 3"), or the whole rest is the ordinal ("choose 4th").
                // This avoids eating non-ordinal tails ("click chrome" falls
                // through to app resolution).
                let noun_present = rest.len() != num_part.len();
                let whole_is_ordinal = rest.trim() == num_part;
                if let Some(n) = crate::screen::parse_ordinal(num_part) {
                    if noun_present || whole_is_ordinal || num_part.chars().all(|c| c.is_ascii_digit()) {
                        return Some(ParseResult {
                            intent: ParsedIntent::ScreenClick { ordinal: n },
                            confidence: 0.9,
                            source: "deterministic".to_string(),
                        });
                    }
                }
            }
        }
    }
    None
}

// ─── Social: send WhatsApp message ─────────────────────────────────────

/// Parse WhatsApp message-sending commands.
///
/// Patterns:
///   "send <message> to <contact> in/on/via whatsapp"
///   "send <contact> a whatsapp message saying <message>"
///   "send a whatsapp message to <contact> saying <message>"
///   "send whatsapp message to <contact> saying <message>"
///   "send whatsapp to <contact> saying <message>"
///   "send a whatsapp to <contact> saying <message>"
///   "whatsapp <contact> saying <message>"
///   "message <contact> on whatsapp saying <message>"
///   "send <contact> a message saying <message>"
///   "send a message to <contact> saying <message>"
fn parse_send_whatsapp_message(text: &str) -> Option<ParseResult> {
    let mut clean_text = text.trim();
    for prefix in ["can you ", "could you ", "please ", "i want to ", "just "] {
        if let Some(rest) = clean_text.strip_prefix(prefix) {
            clean_text = rest.trim();
            break;
        }
    }

    // Pattern 1: (send|message|text|tell|say) <message> to <contact> (in|on|via) whatsapp
    // e.g. "send hi to mummy in whatsapp", "message hi to mommy in whatsapp", "say hi to mummy in whatsapp"
    for verb in ["send ", "message ", "text ", "tell ", "say "] {
        if let Some(rest) = clean_text.strip_prefix(verb) {
            for wa_suffix in [" in whatsapp", " on whatsapp", " via whatsapp", " on wa", " in wa", " via wa"] {
                if let Some(target_part) = rest.strip_suffix(wa_suffix) {
                    // Must have " to " between message and contact
                    // e.g. "hi to mummy", "good morning to mom"
                    if let Some((msg, contact)) = target_part.rsplit_once(" to ") {
                        let msg = msg.trim();
                        let contact = contact.trim();
                        let is_scaffolding = matches!(
                            msg,
                            "a message"
                                | "a whatsapp message"
                                | "a whatsapp"
                                | "message"
                                | "whatsapp message"
                                | "whatsapp"
                        );
                        if !contact.is_empty() && !msg.is_empty() && !is_scaffolding {
                            let msg_clean = msg
                                .trim_start_matches("a whatsapp message saying ")
                                .trim_start_matches("a message saying ")
                                .trim_start_matches("a whatsapp saying ")
                                .trim_start_matches("whatsapp message saying ")
                                .trim();
                            return Some(ParseResult {
                                intent: ParsedIntent::SendWhatsAppMessage {
                                    contact: contact.to_string(),
                                    message: msg_clean.to_string(),
                                },
                                confidence: 0.95,
                                source: "deterministic".to_string(),
                            });
                        }
                    }
                }
            }
        }
    }

    // Pattern 2: (send|message|text|tell|say) <message> (on|in|via) whatsapp to <contact>
    // e.g. "send hi on whatsapp to mummy", "message good morning in whatsapp to dad", "say hi on whatsapp to mummy"
    for verb in ["send ", "message ", "text ", "tell ", "say "] {
        if let Some(rest) = clean_text.strip_prefix(verb) {
            for wa_mid in [" on whatsapp to ", " in whatsapp to ", " via whatsapp to ", " on wa to ", " in wa to "] {
                if let Some(pos) = rest.find(wa_mid) {
                    let msg = rest[..pos].trim();
                    let contact = rest[pos + wa_mid.len()..].trim();
                    let is_scaffolding = matches!(
                        msg,
                        "a message"
                            | "a whatsapp message"
                            | "a whatsapp"
                            | "message"
                            | "whatsapp message"
                            | "whatsapp"
                    );
                    if !contact.is_empty() && !msg.is_empty() && !is_scaffolding {
                        return Some(ParseResult {
                            intent: ParsedIntent::SendWhatsAppMessage {
                                contact: contact.to_string(),
                                message: msg.to_string(),
                            },
                            confidence: 0.95,
                            source: "deterministic".to_string(),
                        });
                    }
                }
            }
        }
    }

    // Pattern 3: (send|tell|message|text|say) + [a] [whatsapp] message to <contact> + (saying|that|: |. |,) + <message>
    // e.g. "tell message to mommy. hi. all right", "tell message to mommy: hi", "send message to mommy. hi"
    for verb in ["send ", "tell ", "message ", "text ", "say "] {
        if let Some(rest) = clean_text.strip_prefix(verb) {
            for prefix in [
                "a whatsapp message to ",
                "a whatsapp to ",
                "whatsapp message to ",
                "whatsapp to ",
                "a message to ",
                "message to ",
            ] {
                if let Some(after_prefix) = rest.strip_prefix(prefix) {
                    for sep in [" saying ", " that ", ": ", ". ", ", "] {
                        if let Some(pos) = after_prefix.find(sep) {
                            let mut contact = after_prefix[..pos].trim();
                            for cut in [" on whatsapp", " in whatsapp", " via whatsapp", " on wa"] {
                                if let Some(c) = contact.strip_suffix(cut) {
                                    contact = c.trim();
                                }
                            }
                            let message = after_prefix[pos + sep.len()..].trim();
                            if !contact.is_empty() && !message.is_empty() {
                                return Some(ParseResult {
                                    intent: ParsedIntent::SendWhatsAppMessage {
                                        contact: contact.to_string(),
                                        message: message.to_string(),
                                    },
                                    confidence: 0.95,
                                    source: "deterministic".to_string(),
                                });
                            }
                        }
                    }
                    // If no message separator found, it's a partial send: "tell message to mommy"
                    let mut contact = after_prefix.trim();
                    for cut in [" on whatsapp", " in whatsapp", " via whatsapp", " on wa"] {
                        if let Some(c) = contact.strip_suffix(cut) {
                            contact = c.trim();
                        }
                    }
                    if !contact.is_empty() {
                        return Some(ParseResult {
                            intent: ParsedIntent::NeedMoreInfo {
                                prompt: format!("What should I say to {}?", contact),
                            },
                            confidence: 0.90,
                            source: "deterministic-partial".to_string(),
                        });
                    }
                }
            }
        }
    }

    // Pattern 4: (send|tell|text|message|whatsapp|say) + <contact> + (a whatsapp message|a message|on whatsapp|...) + (saying|that|: |. |,) + <message>
    for verb in ["send ", "tell ", "text ", "message ", "whatsapp ", "say "] {
        if let Some(rest) = clean_text.strip_prefix(verb) {
            for mid in [
                " a whatsapp message saying ",
                " a whatsapp message that ",
                " a whatsapp message: ",
                " a message saying ",
                " a message that ",
                " a message: ",
                " whatsapp message saying ",
                " whatsapp message that ",
                " on whatsapp saying ",
                " on whatsapp that ",
                " on whatsapp: ",
                " in whatsapp saying ",
                " in whatsapp that ",
                " in whatsapp: ",
                " via whatsapp saying ",
                " on whatsapp ",
                " in whatsapp ",
                " via whatsapp ",
                " on wa ",
                " in wa ",
                " via wa ",
                " saying ",
                " that ",
                ": ",
                ". ",
            ] {
                if let Some(pos) = rest.find(mid) {
                    let mut contact = rest[..pos].trim();
                    for cut in [" on whatsapp", " in whatsapp", " via whatsapp", " on wa"] {
                        if let Some(c) = contact.strip_suffix(cut) {
                            contact = c.trim();
                        }
                    }
                    let message = rest[pos + mid.len()..].trim();
                    if !contact.is_empty() && !message.is_empty() {
                        return Some(ParseResult {
                            intent: ParsedIntent::SendWhatsAppMessage {
                                contact: contact.to_string(),
                                message: message.to_string(),
                            },
                            confidence: 0.95,
                            source: "deterministic".to_string(),
                        });
                    }
                }
            }
        }
    }

    // Pattern 5: Partial send / tell without message body
    // ("send message to mummy", "can you send mummy a whatsapp message", "tell mom a message")
    if (clean_text.contains("send") || clean_text.contains("tell") || clean_text.contains("text"))
        && (clean_text.contains("whatsapp") || clean_text.contains("message"))
    {
        for verb in ["send ", "tell ", "text "] {
            if let Some(rest) = clean_text.strip_prefix(verb) {
                let mut contact_words = Vec::new();
                for word in rest.split_whitespace() {
                    let lower = word.to_lowercase();
                    let trimmed_word = lower.trim_matches(|c: char| !c.is_alphanumeric());
                    if !matches!(
                        trimmed_word,
                        "a" | "an" | "the" | "message" | "messages" | "whatsapp" | "wa" | "to" | "on" | "in" | "via"
                    ) {
                        contact_words.push(word);
                    }
                }
                let contact = contact_words.join(" ");
                let contact = contact.trim().trim_matches(|c: char| !c.is_alphanumeric());
                if !contact.is_empty() && !contact.contains("saying") {
                    return Some(ParseResult {
                        intent: ParsedIntent::NeedMoreInfo {
                            prompt: format!("What should I say to {}?", contact),
                        },
                        confidence: 0.85,
                        source: "deterministic-partial".to_string(),
                    });
                } else if contact.is_empty() {
                    return Some(ParseResult {
                        intent: ParsedIntent::NeedMoreInfo {
                            prompt: "Who should I message on WhatsApp, and what should I say?".to_string(),
                        },
                        confidence: 0.85,
                        source: "deterministic-partial".to_string(),
                    });
                }
            }
        }
    }

    None
}

// ΓöÇΓöÇΓöÇ Media control ΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇ

// ΓöÇΓöÇΓöÇ Greetings / conversational replies ΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇ
//
// These are handled entirely locally ΓÇö no Cloudflare Worker round-trip.
// This saves ~1-3s of latency and avoids using GLM-4.7 Flash tokens for
// trivial conversational replies.

/// Parse greetings, farewells, and other conversational pleasantries.
/// Returns a `Greeting` intent with a pre-written reply.
fn parse_greeting(text: &str) -> Option<ParseResult> {
    // Hello / Hi / Hey
    if regex_match(text, r"^(?:hello|hi|hey|yo|sup|what'?s\s+up|howdy|greetings|hi\s+ya|hiya|hey\s+(?:there|nexus)|hello\s+nexus|hi\s+nexus|hey\s+nexus)$") {
        let replies = [
            "Hello, sir.",
            "Hi, sir. How can I help?",
            "Hey, sir. What can I do for you?",
            "At your service, sir.",
        ];
        let reply = pick(&replies, text);
        return Some(ParseResult {
            intent: ParsedIntent::Greeting { reply: reply.to_string() },
            confidence: 1.0,
            source: "deterministic".to_string(),
        });
    }

    // How are you
    if regex_match(text, r"^(?:how\s+(?:are\s+you|are\s+ya|r\s+u)|how'?s\s+it\s+going|how\s+are\s+things|how\s+do\s+you\s+do|how\s+are\s+you\s+doing|how\s+is\s+it\s+going)$") {
        let replies = [
            "Fully operational, sir. How can I assist?",
            "Running smoothly, sir. What do you need?",
            "All systems green, sir. Ready when you are.",
            "Doing well, sir. How can I help?",
        ];
        let reply = pick(&replies, text);
        return Some(ParseResult {
            intent: ParsedIntent::Greeting { reply: reply.to_string() },
            confidence: 1.0,
            source: "deterministic".to_string(),
        });
    }

    // Bye / Goodbye / See you
    if regex_match(text, r"^(?:bye|goodbye|good\s+bye|see\s+you|see\s+ya|see\s+u|catch\s+you\s+later|catch\s+ya\s+later|later|farewell|bye\s+bye|bye\s+nexus|goodbye\s+nexus)$") {
        let replies = [
            "Goodbye, sir.",
            "Until next time, sir.",
            "See you, sir.",
            "Farewell, sir.",
        ];
        let reply = pick(&replies, text);
        return Some(ParseResult {
            intent: ParsedIntent::Greeting { reply: reply.to_string() },
            confidence: 1.0,
            source: "deterministic".to_string(),
        });
    }

    // Thanks
    if regex_match(text, r"^(?:thanks|thank\s+you|thank\s+u|thx|ty|thanks\s+nexus|thank\s+you\s+nexus|appreciate\s+it|much\s+obliged)$") {
        let replies = [
            "You're welcome, sir.",
            "My pleasure, sir.",
            "Anytime, sir.",
            "Glad to help, sir.",
        ];
        let reply = pick(&replies, text);
        return Some(ParseResult {
            intent: ParsedIntent::Greeting { reply: reply.to_string() },
            confidence: 1.0,
            source: "deterministic".to_string(),
        });
    }

    // What is your name / Who are you
    if regex_match(text, r"^(?:what(?:'?s|\s+is)\s+your\s+name|who\s+are\s+you|what\s+are\s+you|your\s+name|who\s+is\s+nexus)$") {
        return Some(ParseResult {
            intent: ParsedIntent::Greeting {
                reply: "I'm NEXUS, your desktop assistant, sir.".to_string(),
            },
            confidence: 1.0,
            source: "deterministic".to_string(),
        });
    }

    // What can you do
    if regex_match(text, r"^(?:what\s+can\s+you\s+do|what\s+do\s+you\s+do|what\s+are\s+you\s+capable\s+of|help\s+me|what\s+commands\s+(?:do\s+you\s+(?:know|have)|can\s+you\s+(?:do|handle)))$") {
        return Some(ParseResult {
            intent: ParsedIntent::Greeting {
                reply: "I can open apps, search the web, analyse repositories and PRs, control media, and answer questions, sir.".to_string(),
            },
            confidence: 1.0,
            source: "deterministic".to_string(),
        });
    }

    // Good morning / afternoon / evening
    if regex_match(text, r"^good\s+(?:morning|afternoon|evening|night)(?:\s+nexus)?$") {
        let reply = if text.contains("morning") {
            "Good morning, sir. How can I help?"
        } else if text.contains("afternoon") {
            "Good afternoon, sir. What can I do for you?"
        } else if text.contains("evening") {
            "Good evening, sir. At your service."
        } else {
            "Good night, sir."
        };
        return Some(ParseResult {
            intent: ParsedIntent::Greeting { reply: reply.to_string() },
            confidence: 1.0,
            source: "deterministic".to_string(),
        });
    }

    // Yes / OK / Alright (acknowledgements)
    if regex_match(text, r"^(?:yes|yeah|yep|yup|sure|ok|okay|alright|sounds\s+good|got\s+it|understood|roger|affirmative)$") {
        let replies = [
            "Understood, sir.",
            "Very good, sir.",
            "Acknowledged, sir.",
        ];
        let reply = pick(&replies, text);
        return Some(ParseResult {
            intent: ParsedIntent::Greeting { reply: reply.to_string() },
            confidence: 1.0,
            source: "deterministic".to_string(),
        });
    }

    // No / Nope / No thanks
    if regex_match(text, r"^(?:no|nope|nah|no\s+thanks|never\s+mind|forget\s+it|cancel|disregard)$") {
        let replies = [
            "Very well, sir.",
            "As you wish, sir.",
            "Noted, sir.",
        ];
        let reply = pick(&replies, text);
        return Some(ParseResult {
            intent: ParsedIntent::Greeting { reply: reply.to_string() },
            confidence: 1.0,
            source: "deterministic".to_string(),
        });
    }

    None
}

/// Pick a reply from a list, deterministically based on a hash of the input
/// text. This gives variety (different replies for different inputs) while
/// remaining deterministic (same input ΓåÆ same reply, no randomness).
fn pick<'a>(replies: &[&'a str], text: &str) -> &'a str {
    let hash: u32 = text.bytes().fold(0u32, |acc, b| acc.wrapping_mul(31).wrapping_add(b as u32));
    replies[(hash as usize) % replies.len()]
}

fn parse_media(text: &str) -> Option<ParsedIntent> {
    if regex_match(text, r"^(?:pause|pause\s+music|pause\s+media|play|resume|resume\s+music|play\s*[/\s]*pause|toggle\s+media)$") {
        return Some(ParsedIntent::MediaPlayPause);
    }
    if regex_match(text, r"^(?:next|next\s+song|next\s+track|skip|skip\s+song|skip\s+track)$") {
        return Some(ParsedIntent::MediaNext);
    }
    if regex_match(text, r"^(?:previous|previous\s+song|previous\s+track|prev|prev\s+song|go\s+back\s+a\s+song)$") {
        return Some(ParsedIntent::MediaPrevious);
    }
    if regex_match(text, r"^(?:stop\s+music|stop\s+media|stop\s+playback|stop)$") {
        return Some(ParsedIntent::MediaStop);
    }
    None
}

// ΓöÇΓöÇΓöÇ Architecture mapper ΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇ

/// Exact match for architecture mapper commands.
/// Covers all natural phrasings:
///   "open architecture mapper"
///   "open the architecture mapper"
///   "open architect"
///   "show architecture map"
///   "launch architecture window"
///   "bring up architecture"
///   "pull up the architect mapper"
///   "open the architecture"
///   "show me the architecture"
///   "open codebase mapper"
///   "open dependency mapper"
fn is_architect_command(text: &str) -> bool {
    // Truncated "open-" or "open" — Intel SST mic silence cuts the utterance
    // mid-word. STT returns "open-" or "open" instead of "open architecture mapper".
    // In this app's context, "open" almost always means "open architecture mapper".
    let trimmed = text.trim().to_lowercase();
    if trimmed == "open-" || trimmed == "open" {
        tracing::info!("[intent_parser] truncated '{}' → open_architect (mic silence recovery)", trimmed);
        return true;
    }
    regex_match(
        text,
        r"^(?:open|launch|start|show|bring\s+up|pull\s+up|give\s+me|show\s+me)\s+(?:me\s+)?(?:the\s+)?(?:architecture|architect|codebase|dependency)(?:\s+(?:mapper|map|window|mapper\s+window|viewer|diagram|graph|explorer))?$",
    ) || regex_match(
        text,
        r"^(?:open|launch|start|show)\s+(?:the\s+)?(?:architecture|architect)(?:\s+(?:mapper|map|window))?$",
    ) || regex_match(
        text,
        r"^(?:show|display|view)\s+(?:me\s+)?(?:the\s+)?architecture$",
    )
}

// ─── Settings / Command Center ───────────────────────────────────────────

/// Match settings/command center commands.
/// Covers all natural phrasings:
///   "open settings" / "open the settings"
///   "show settings" / "show me the settings"
///   "open command center" / "open the command center"
///   "show command center" / "show me the command center"
///   "open preferences" / "show preferences"
///   "open configuration" / "open config"
///   "configure NEXUS" / "NEXUS settings" / "NEXUS config"
///   "NEXUS preferences" / "NEXUS command center"
///   "bring up settings" / "pull up settings"
///   "launch settings" / "start settings"
///
/// Also handles STT singular variants:
///   "open setting" / "show setting" / "open preference"
///
/// Also handles bare words (Intel SST mic truncation):
///   "settings" / "preferences" / "config" / "command center"
fn is_settings_command(text: &str) -> bool {
    let t = text.trim().to_lowercase();

    // Bare words — Intel SST mic silence cuts the utterance mid-word.
    // "settings" alone almost always means "open settings".
    if t == "settings" || t == "preferences" || t == "config" || t == "configuration" {
        return true;
    }

    // (open|launch|start|show|bring up|pull up|give me|show me) + (the)? + settings/setting
    if regex_match(
        text,
        r"^(?:open|launch|start|show|bring\s+up|pull\s+up|give\s+me|show\s+me)\s+(?:me\s+)?(?:the\s+)?settings?$",
    ) {
        return true;
    }

    // (open|show|launch) + (the)? + command center
    if regex_match(
        text,
        r"^(?:open|launch|start|show|bring\s+up|pull\s+up|give\s+me|show\s+me)\s+(?:me\s+)?(?:the\s+)?command\s+center$",
    ) {
        return true;
    }

    // (open|show) + (the)? + preferences? / preference
    if regex_match(
        text,
        r"^(?:open|launch|start|show|bring\s+up|pull\s+up|give\s+me|show\s+me)\s+(?:me\s+)?(?:the\s+)?preferences?$",
    ) {
        return true;
    }

    // (open|show) + (the)? + config / configuration
    if regex_match(
        text,
        r"^(?:open|launch|start|show|bring\s+up|pull\s+up|give\s+me|show\s+me)\s+(?:me\s+)?(?:the\s+)?(?:config|configuration)$",
    ) {
        return true;
    }

    // "configure NEXUS" / "configure nexus"
    if regex_match(text, r"^configure\s+nexus$") {
        return true;
    }

    // "NEXUS settings" / "NEXUS config" / "NEXUS preferences" / "NEXUS command center"
    if regex_match(
        text,
        r"^nexus\s+(?:settings?|config|configuration|preferences?|command\s+center)$",
    ) {
        return true;
    }

    false
}

/// Fuzzy match for architecture mapper commands that STT misheard.
///
/// faster-whisper tiny.en (39M params) commonly mishears "architecture mapper" as:
///   "octach at mapper", "arcade mapper", "arch at mapper", "arch mapper",
///   "architecture at mapper", "open architect mapper", etc.
///   "open up and remember" (severe mishearing)
///   "open are cat map", "open our cat map", "open ark map"
///   "open art at mapper", "open art map"
///   "open a cat map", "open acat mapper"
///
/// Strategy (layered, most-specific first):
/// 1. Exact-ish: ends with "mapper"/"map"/"diagram"/"graph" + has arch-like word
/// 2. Contains "arch" or "architect" anywhere
/// 3. Contains "codebase" or "dependency" + "map"/"mapper"
/// 4. Severe mishearing: "open" + 2-5 words with mapper-like or arch-like sounds
fn is_architect_fuzzy(text: &str) -> bool {
    let t = text.trim().to_lowercase();
    // Must start with an open/launch verb (or "show me" / "give me")
    let starts_with_verb = t.starts_with("open ")
        || t.starts_with("launch ")
        || t.starts_with("start ")
        || t.starts_with("show ")
        || t.starts_with("show me ")
        || t.starts_with("bring up ")
        || t.starts_with("bring me ")
        || t.starts_with("pull up ")
        || t.starts_with("give me ");
    if !starts_with_verb {
        return false;
    }

    // Pattern 1: ends with "mapper"/"map"/"diagram"/"graph"/"viewer"/"explorer"
    // (strong signal ΓÇö these are rare words in NEXUS context)
    if t.ends_with("mapper")
        || t.ends_with("map")
        || t.ends_with("mapper window")
        || t.ends_with("map window")
        || t.ends_with("diagram")
        || t.ends_with("graph")
        || t.ends_with("viewer")
        || t.ends_with("explorer")
    {
        return true;
    }

    // Pattern 2: contains "arch" or "architect" (medium signal)
    if t.contains("arch") || t.contains("architect") {
        return true;
    }

    // Pattern 3: contains "codebase" or "dependency" (NEXUS-specific architecture words)
    if t.contains("codebase") || t.contains("dependency") || t.contains("dependencies") {
        return true;
    }

    // Pattern 4: "open" + 2-5 words that could be misheard "architecture mapper"
    // Common mishearings of "architecture":
    //   "are cat", "our cat", "ark", "art", "octach", "arcade", "arc", "are"
    // Common mishearings of "mapper":
    //   "remember", "member", "december", "map", "mac", "mad", "matter", "master"
    let words: Vec<&str> = t.split_whitespace().collect();
    if words.len() >= 2 && words.len() <= 6 {
        // Words that sound like "architecture"
        let has_arch_like = words.iter().any(|w| {
            w.starts_with("arch") || w.starts_with("oct") || w.starts_with("arc")
            || w.starts_with("art") || w.starts_with("ark") || w.starts_with("are")
            || w.starts_with("our") || *w == "are" || *w == "our"
            || *w == "art" || *w == "ark" || *w == "arc"
        });
        // Words that sound like "mapper"
        let has_mapper_like = words.iter().any(|w| {
            w.starts_with("map") || w.starts_with("mem") || w.starts_with("rem")
            || w.starts_with("mac") || w.starts_with("mad") || w.starts_with("mat")
            || w.starts_with("mas")
            || *w == "remember" || *w == "member" || *w == "december"
            || *w == "map" || *w == "mac" || *w == "mad" || *w == "matter"
            || *w == "master" || *w == "manner"
        });
        // Need at least one arch-like OR one mapper-like word
        // (for 2-word phrases like "open map" we only need mapper-like)
        if words.len() <= 3 && has_mapper_like {
            return true;
        }
        // For longer phrases, need both signals OR just arch-like
        if has_arch_like {
            return true;
        }
        if has_mapper_like && words.len() >= 3 {
            return true;
        }
    }

    false
}

// ΓöÇΓöÇΓöÇ Browser force ΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇ

/// URL map for "open <app> in browser" commands.
const BROWSER_FORCE_URLS: &[(&str, &str)] = &[
    ("gmail", "https://mail.google.com"),
    ("google mail", "https://mail.google.com"),
    ("youtube", "https://www.youtube.com"),
    ("you tube", "https://www.youtube.com"),
    ("github", "https://github.com"),
    ("git hub", "https://github.com"),
    ("twitter", "https://twitter.com"),
    ("x", "https://x.com"),
    ("facebook", "https://facebook.com"),
    ("instagram", "https://instagram.com"),
    ("reddit", "https://reddit.com"),
    ("linkedin", "https://linkedin.com"),
    ("whatsapp", "https://web.whatsapp.com"),
    ("whatsapp web", "https://web.whatsapp.com"),
    ("spotify", "https://open.spotify.com"),
    ("netflix", "https://netflix.com"),
    ("amazon", "https://amazon.com"),
    ("google drive", "https://drive.google.com"),
    ("google docs", "https://docs.google.com"),
    ("google sheets", "https://sheets.google.com"),
    ("google slides", "https://slides.google.com"),
    ("google maps", "https://maps.google.com"),
    ("google calendar", "https://calendar.google.com"),
    ("google translate", "https://translate.google.com"),
    ("google photos", "https://photos.google.com"),
    ("google news", "https://news.google.com"),
    ("google meet", "https://meet.google.com"),
    ("google chat", "https://chat.google.com"),
    ("google play", "https://play.google.com"),
    ("play store", "https://play.google.com"),
    ("app store", "https://apps.apple.com"),
    ("chatgpt", "https://chat.openai.com"),
    ("chat gpt", "https://chat.openai.com"),
    ("open ai", "https://chat.openai.com"),
    ("openai", "https://chat.openai.com"),
    ("claude", "https://claude.ai"),
    ("figma", "https://figma.com"),
    ("notion", "https://notion.so"),
    ("slack", "https://slack.com"),
    ("discord", "https://discord.com/app"),
    ("twitch", "https://twitch.tv"),
    ("stack overflow", "https://stackoverflow.com"),
    ("stackoverflow", "https://stackoverflow.com"),
    ("wikipedia", "https://wikipedia.org"),
    ("chat", "https://chat.google.com"),
    ("maps", "https://maps.google.com"),
    ("translate", "https://translate.google.com"),
    ("calendar", "https://calendar.google.com"),
];

fn parse_browser_force(text: &str) -> Option<ParseResult> {
    // "open gmail in browser" / "open gmail website" / "open gmail site"
    let patterns: &[&str] = &[
        r"^(.+?)\s+in\s+(?:the\s+)?browser$",
        r"^(.+?)\s+website$",
        r"^(.+?)\s+site$",
        r"^(.+?)\s+on\s+(?:the\s+)?web$",
        r"^(.+?)\s+web\s+version$",
    ];

    for &pat in patterns {
        if let Some(caps) = regex_captures(text, pat) {
            let app_name = caps[1].trim();
            // Check URL map
            for (key, url) in BROWSER_FORCE_URLS {
                if *key == app_name {
                    return Some(ParseResult {
                        intent: ParsedIntent::OpenUrl {
                            target: app_name.to_string(),
                            url: url.to_string(),
                        },
                        confidence: 1.0,
                        source: "deterministic".to_string(),
                    });
                }
            }
            // Unknown app + "in browser" ΓåÆ construct URL if it looks like a domain
            if app_name.contains('.') && !app_name.contains(' ') {
                let url = if app_name.starts_with("http") {
                    app_name.to_string()
                } else {
                    format!("https://{}", app_name)
                };
                return Some(ParseResult {
                    intent: ParsedIntent::OpenUrl {
                        target: app_name.to_string(),
                        url,
                    },
                    confidence: 1.0,
                    source: "deterministic".to_string(),
                });
            }
        }
    }

    None
}

// ΓöÇΓöÇΓöÇ Helpers ΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇ

fn normalize_whitespace(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Strip trailing sentence-ending punctuation that STT engines (Groq
/// whisper-large-v3-turbo, faster-whisper) often append to transcripts.
///
/// Only strips from the very end of the string so internal punctuation
/// (e.g. "owner/repo", "what's up") is preserved. Handles repeated
/// punctuation ("ok...") and trailing quotes/brackets STT sometimes adds.
fn strip_trailing_punctuation(s: &str) -> String {
    let trimmed = s.trim_end_matches(|c: char| {
        matches!(c, '.' | ',' | '?' | '!' | ';' | ':' | '"' | '\'' | ')' | ']' | '…')
    });
    trimmed.to_string()
}

/// Leading filler words that STT often inserts before the actual command.
/// e.g. "And analyse PR 254 in zync" → "analyse PR 254 in zync"
///      "So open chrome" → "open chrome"
///      "But first close notepad" → "close notepad"
///
/// These are conversational connectors — the user is speaking naturally,
/// not issuing a robotic command. Without stripping, every `starts_with()`
/// check in the parser fails and the intent falls through to `Unknown`.
///
/// We only strip filler at the START of the transcript (not mid-sentence),
/// and we strip at most one filler word — "and so open chrome" keeps "so"
/// because "and so" is rare and stripping multiple words risks eating the
/// actual command ("so" alone is a valid filler, but "and so" could be
/// "answer so..." mishears).
fn strip_leading_filler(text: &str) -> String {
    /// Single-word fillers that can precede a command.
    const FILLERS: &[&str] = &[
        "and", "so", "but", "then", "now", "also", "plus", "like", "okay",
        "ok", "well", "um", "uh", "hmm", "actually", "basically",
        "just", "please", "now please",
    ];

    // Protect "hey nexus" / "hello nexus" / "hi nexus" — these are greetings,
    // not filler + command. "hey" alone is filler, but "hey nexus" is a wake.
    let lower = text.to_lowercase();
    if lower == "hey nexus" || lower == "hello nexus" || lower == "hi nexus"
        || lower == "hey there" || lower == "hey nexus wake up"
    {
        return text.to_string();
    }

    // Try two-word fillers first (e.g. "and so", "but first", "now just")
    let words: Vec<&str> = text.split_whitespace().collect();
    if words.len() >= 3 {
        let two = format!("{} {}", words[0], words[1]);
        if FILLERS.contains(&two.as_str())
            || (words[0] == "and" && (words[1] == "so" || words[1] == "then" || words[1] == "now"))
            || (words[0] == "but" && words[1] == "first")
            || (words[0] == "now" && (words[1] == "just" || words[1] == "please"))
        {
            return words[2..].join(" ");
        }
    }

    // Single-word filler — but NOT "hey" when followed by "nexus"/"there"
    if words.len() >= 2 {
        if FILLERS.contains(&words[0]) {
            return words[1..].join(" ");
        }
        // "hey" is filler only when NOT followed by "nexus" or "there"
        if words[0] == "hey" && words[1] != "nexus" && words[1] != "there" {
            return words[1..].join(" ");
        }
    }

    text.to_string()
}

fn strip_trailing_app_words(s: &str) -> String {
    let s = s.trim();
    let s = s
        .strip_suffix(" app")
        .or_else(|| s.strip_suffix(" application"))
        .or_else(|| s.strip_suffix(" for me"))
        .unwrap_or(s);
    s.trim().to_string()
}

fn strip_trailing_site(s: &str) -> String {
    let s = s.trim();
    let s = s
        .strip_suffix(" website")
        .or_else(|| s.strip_suffix(" site"))
        .unwrap_or(s);
    s.trim().to_string()
}

fn is_url_like(s: &str) -> bool {
    s.contains('.') && !s.contains(' ')
}

/// Levenshtein edit distance.
fn levenshtein(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let m = a.len();
    let n = b.len();
    if m == 0 {
        return n;
    }
    if n == 0 {
        return m;
    }

    let mut prev: Vec<usize> = (0..=n).collect();
    let mut curr: Vec<usize> = vec![0; n + 1];

    for i in 1..=m {
        curr[0] = i;
        for j in 1..=n {
            let cost = if a[i - 1] == b[j - 1] { 0 } else { 1 };
            curr[j] = (prev[j] + 1).min(curr[j - 1] + 1).min(prev[j - 1] + cost);
        }
        std::mem::swap(&mut prev, &mut curr);
    }

    prev[n]
}

// ΓöÇΓöÇΓöÇ Regex helper ΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇ
// We use the `regex` crate for pattern matching. It's already in the dependency
// tree via other crates, but we need to add it explicitly to Cargo.toml.

/// Simple regex match helper.
fn regex_match(text: &str, pattern: &str) -> bool {
    static CACHE: std::sync::OnceLock<std::sync::Mutex<std::collections::HashMap<String, regex::Regex>>> =
        std::sync::OnceLock::new();
    let cache = CACHE.get_or_init(|| std::sync::Mutex::new(std::collections::HashMap::new()));
    let mut guard = cache.lock().unwrap();
    let re = guard
        .entry(pattern.to_string())
        .or_insert_with(|| regex::Regex::new(pattern).unwrap_or_else(|_| regex::Regex::new("$^").unwrap()));
    re.is_match(text)
}

/// Cached regex captures helper. Uses the same OnceLock+Mutex cache as
/// `regex_match` but returns capture groups for pattern extraction.
///
/// `Captures` borrows from `text` (not from the `Regex`), so it is safe
/// to return even though the MutexGuard is dropped when this function
/// returns.
fn regex_captures<'t>(text: &'t str, pattern: &str) -> Option<regex::Captures<'t>> {
    static CACHE: std::sync::OnceLock<std::sync::Mutex<std::collections::HashMap<String, regex::Regex>>> =
        std::sync::OnceLock::new();
    let cache = CACHE.get_or_init(|| std::sync::Mutex::new(std::collections::HashMap::new()));
    let mut guard = cache.lock().unwrap();
    let re = guard
        .entry(pattern.to_string())
        .or_insert_with(|| regex::Regex::new(pattern).unwrap_or_else(|_| regex::Regex::new("$^").unwrap()));
    re.captures(text)
}

// ΓöÇΓöÇΓöÇ Tauri command ΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇ

/// Parse a transcript into a structured intent.
///
/// Tries the deterministic parser first (fast, zero-latency).
/// If the deterministic parser returns None or low confidence,
/// tries the NLU server (BERT-Mini, lazy-started Python sidecar).
/// Falls back to `unknown` if both fail.
#[tauri::command]
pub async fn parse_transcript(transcript: String) -> Result<ParseResult, String> {
    tracing::info!("[intent_parser] parsing: {:?}", transcript);

    // 1. Try deterministic parser
    if let Some(result) = parse_deterministic(&transcript) {
        tracing::info!(
            "[intent_parser] deterministic: {:?} (confidence={}, source={})",
            result.intent,
            result.confidence,
            result.source
        );
        return Ok(result);
    }

    // 2. Try brain server FIRST (admin-only, if enabled)
    // The brain (Qwen 0.5B LLM) is much smarter than BERT-Mini and can
    // understand mishearings, filler words, and unusual phrasing.
    // BERT-Mini often returns confident-but-wrong results (e.g. "So, you
    // have to list." → MediaPlayPause with 0.90 confidence), which blocks
    // the brain from ever being tried. By trying the brain first, we get
    // accurate classification for admin users.
    #[cfg(feature = "admin-brain")]
    if crate::admin_config::is_admin() {
        if let Some(result) = crate::brain_client::brain_classify(&transcript).await {
            tracing::info!(
                "[intent_parser] brain: {:?} (confidence={}, source={})",
                result.intent,
                result.confidence,
                result.source
            );

            // Brain monitor: observe the brain's own result
            let brain_intent_name = intent_to_label(&result.intent).to_string();
            crate::brain_monitor::monitor_transcript(
                transcript.clone(),
                None, // deterministic missed
                Some(brain_intent_name),
            );

            // Only accept brain result if confidence is reasonable
            if result.confidence >= 0.5 {
                return Ok(result);
            }
            tracing::info!(
                "[intent_parser] brain confidence too low ({:.2}), falling back to NLU",
                result.confidence
            );
        }
    }

    // 3. Try NLU server (BERT-Mini fallback)
    if let Some(result) = crate::nlu_client::parse_via_nlu(&transcript).await {
        tracing::info!(
            "[intent_parser] nlu: {:?} (confidence={})",
            result.intent,
            result.confidence
        );

        // Brain monitor: observe the NLU result (non-blocking, background)
        #[cfg(feature = "admin-brain")]
        {
            let nlu_intent_name = intent_to_label(&result.intent).to_string();
            crate::brain_monitor::monitor_transcript(
                transcript.clone(),
                None, // deterministic missed
                Some(nlu_intent_name),
            );
        }

        return Ok(result);
    }

    // 4. Fallback: unknown
    tracing::info!("[intent_parser] no match, returning unknown");
    Ok(ParseResult {
        intent: ParsedIntent::Unknown {
            raw: transcript.clone(),
        },
        confidence: 0.0,
        source: "fallback".to_string(),
    })
}

// ΓöÇΓöÇΓöÇ Tests ΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇ

// ─── Live mode commands ───────────────────────────────────────────────

/// Parse live-mode commands: type, press, send, cancel, new tab, etc.
///
/// These are only matched after all other parsers fail, so they don't
/// interfere with existing commands like "open app" or "search for X".
///
/// Patterns:
///   "type <text>"              → TypeText
///   "press <key>"              → PressKey
///   "press <key1> <key2>"       → PressHotkey (multi-key combo)
///   "press ctrl a"             → PressHotkey ["ctrl", "a"]
///   "send"                     → ConfirmSend (requires confirmation)
///   "cancel" / "never mind"    → CancelAction
///   "new tab"                  → BrowserNewTab
fn parse_live_command(text: &str) -> Option<ParseResult> {
    // --- Type text ---
    if let Some(rest) = text.strip_prefix("type ") {
        let typed = rest.trim();
        if !typed.is_empty() {
            return Some(ParseResult {
                intent: ParsedIntent::NluResult {
                    intent: "type_text".to_string(),
                    slots: serde_json::json!({ "text": typed }),
                    confidence: 1.0,
                },
                confidence: 1.0,
                source: "deterministic-live".to_string(),
            });
        }
    }

    // --- Press key / hotkey ---
    if let Some(rest) = text.strip_prefix("press ") {
        let keys_str = rest.trim();
        if !keys_str.is_empty() {
            let keys: Vec<&str> = keys_str.split_whitespace().collect();
            if keys.len() == 1 {
                return Some(ParseResult {
                    intent: ParsedIntent::NluResult {
                        intent: "press_key".to_string(),
                        slots: serde_json::json!({ "key": keys[0] }),
                        confidence: 1.0,
                    },
                    confidence: 1.0,
                    source: "deterministic-live".to_string(),
                });
            } else {
                return Some(ParseResult {
                    intent: ParsedIntent::NluResult {
                        intent: "press_hotkey".to_string(),
                        slots: serde_json::json!({ "keys": keys }),
                        confidence: 1.0,
                    },
                    confidence: 1.0,
                    source: "deterministic-live".to_string(),
                });
            }
        }
    }

    // --- Send (requires confirmation) ---
    if text == "send" || text == "send it" || text == "send message" {
        return Some(ParseResult {
            intent: ParsedIntent::NluResult {
                intent: "confirm_send".to_string(),
                slots: serde_json::json!({}),
                confidence: 1.0,
            },
            confidence: 1.0,
            source: "deterministic-live".to_string(),
        });
    }

    // --- Cancel ---
    // "stop" is not handled by the greeting parser, so we catch it here.
    // "cancel", "never mind", "forget it" are already handled by the
    // greeting parser (they return "Very well, sir." which is appropriate).
    // In live mode, the orchestrator should also reset the state machine
    // when it sees a Greeting that is actually a cancel.
    if text == "stop" {
        return Some(ParseResult {
            intent: ParsedIntent::NluResult {
                intent: "cancel_action".to_string(),
                slots: serde_json::json!({}),
                confidence: 1.0,
            },
            confidence: 1.0,
            source: "deterministic-live".to_string(),
        });
    }

    // --- New tab ---
    if text == "new tab" || text == "open new tab" || text == "open a new tab" {
        return Some(ParseResult {
            intent: ParsedIntent::NluResult {
                intent: "browser_new_tab".to_string(),
                slots: serde_json::json!({}),
                confidence: 1.0,
            },
            confidence: 1.0,
            source: "deterministic-live".to_string(),
        });
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_open_app() {
        let result = parse_deterministic("open whatsapp");
        assert!(result.is_some());
        let r = result.unwrap();
        assert!(matches!(r.intent, ParsedIntent::OpenApp { .. }));
    }

    #[test]
    fn test_open_settings() {
        let result = parse_deterministic("open the settings");
        assert!(result.is_some(), "should parse 'open the settings'");
        let r = result.unwrap();
        assert!(
            matches!(r.intent, ParsedIntent::OpenSettings),
            "expected OpenSettings, got {:?}",
            r.intent
        );
    }

    #[test]
    fn test_open_settings_no_the() {
        let result = parse_deterministic("open settings");
        assert!(result.is_some());
        let r = result.unwrap();
        assert!(matches!(r.intent, ParsedIntent::OpenSettings));
    }

    #[test]
    fn test_open_command_center() {
        let result = parse_deterministic("open the command center");
        assert!(result.is_some());
        let r = result.unwrap();
        assert!(matches!(r.intent, ParsedIntent::OpenSettings));
    }

    #[test]
    fn test_open_gemini() {
        let result = parse_deterministic("open gemini");
        assert!(result.is_some());
        let r = result.unwrap();
        assert!(matches!(r.intent, ParsedIntent::OpenApp { .. }));
    }

    #[test]
    fn test_open_chrome() {
        let result = parse_deterministic("open chrome");
        assert!(result.is_some());
        let r = result.unwrap();
        assert!(matches!(r.intent, ParsedIntent::OpenApp { .. }));
    }

    #[test]
    fn test_launch_spotify() {
        let result = parse_deterministic("launch spotify");
        assert!(result.is_some());
        let r = result.unwrap();
        assert!(matches!(r.intent, ParsedIntent::OpenApp { .. }));
    }

    #[test]
    fn test_open_app_with_app_suffix() {
        let result = parse_deterministic("open gmail app");
        assert!(result.is_some());
        let r = result.unwrap();
        if let ParsedIntent::OpenApp { target } = r.intent {
            assert_eq!(target, "gmail");
        } else {
            panic!("expected OpenApp");
        }
    }

    #[test]
    fn test_open_in_browser() {
        let result = parse_deterministic("open gmail in browser");
        assert!(result.is_some());
        let r = result.unwrap();
        assert!(matches!(r.intent, ParsedIntent::OpenUrl { .. }));
    }

    #[test]
    fn test_analyse_pr() {
        let result = parse_deterministic("analyse PR 23 servx");
        assert!(result.is_some());
        let r = result.unwrap();
        if let ParsedIntent::AnalysePr {
            repo, pr_number, ..
        } = r.intent
        {
            assert_eq!(repo, "servx");
            assert_eq!(pr_number, 23);
        } else {
            panic!("expected AnalysePr, got {:?}", r.intent);
        }
    }

    #[test]
    fn test_analyse_pr_with_in() {
        let result = parse_deterministic("analyse PR 23 in servx");
        assert!(result.is_some());
        let r = result.unwrap();
        if let ParsedIntent::AnalysePr {
            repo, pr_number, ..
        } = r.intent
        {
            assert_eq!(repo, "servx");
            assert_eq!(pr_number, 23);
        } else {
            panic!("expected AnalysePr");
        }
    }

    #[test]
    fn test_analyse_pr_with_leading_filler_and() {
        // "And analyse PR 254 in zync" — STT inserts "And" at the start
        let result = parse_deterministic("And analyse PR 254 in zync");
        assert!(result.is_some(), "should parse with leading 'And'");
        let r = result.unwrap();
        if let ParsedIntent::AnalysePr {
            repo, pr_number, ..
        } = r.intent
        {
            assert_eq!(repo, "zync");
            assert_eq!(pr_number, 254);
        } else {
            panic!("expected AnalysePr, got {:?}", r.intent);
        }
    }

    #[test]
    fn test_analyse_pr_with_leading_filler_so() {
        let result = parse_deterministic("so analyse PR 5 in servx");
        assert!(result.is_some());
        let r = result.unwrap();
        if let ParsedIntent::AnalysePr {
            repo, pr_number, ..
        } = r.intent
        {
            assert_eq!(repo, "servx");
            assert_eq!(pr_number, 5);
        } else {
            panic!("expected AnalysePr, got {:?}", r.intent);
        }
    }

    #[test]
    fn test_open_with_leading_filler() {
        let result = parse_deterministic("and open chrome");
        assert!(result.is_some());
        let r = result.unwrap();
        assert!(matches!(r.intent, ParsedIntent::OpenApp { .. }));
        if let ParsedIntent::OpenApp { target } = r.intent {
            assert_eq!(target, "chrome");
        }
    }

    #[test]
    fn test_open_the_x_never_hijacked_by_stopword_app() {
        // Regression: "open the room" used to resolve to a Dribbble PWA —
        // the word "the" was a registry key (last-write-wins) owned by its
        // long title, and resolve_app_name returned the alphabetical first
        // search name ("creative"). Must never happen again.
        let result = parse_deterministic("open the room");
        assert!(result.is_some());
        if let ParsedIntent::OpenApp { target } = result.unwrap().intent {
            assert_ne!(target, "creative", "stop-word hijack regressed");
            assert!(
                !target.to_lowercase().contains("dribbble"),
                "wrong app launched for 'open the room': {target}"
            );
        } else {
            panic!("expected OpenApp");
        }
    }

    #[test]
    fn test_strip_leading_filler_doesnt_eat_commands() {
        // "open and close chrome" should NOT strip "open"
        let result = parse_deterministic("open and close chrome");
        assert!(result.is_some());
        // "open" is the verb, "and close chrome" is the target — weird but valid
    }

    #[test]
    fn test_analyse_pr_owner_repo() {
        let result = parse_deterministic("analyse PR 5 zync-meet/zync");
        assert!(result.is_some());
        let r = result.unwrap();
        if let ParsedIntent::AnalysePr {
            owner,
            repo,
            pr_number,
        } = r.intent
        {
            assert_eq!(owner, Some("zync-meet".to_string()));
            assert_eq!(repo, "zync");
            assert_eq!(pr_number, 5);
        } else {
            panic!("expected AnalysePr");
        }
    }

    #[test]
    fn test_analyse_repo() {
        let result = parse_deterministic("analyse servx repo");
        assert!(result.is_some());
        let r = result.unwrap();
        if let ParsedIntent::AnalyseRepo { repo, owner } = r.intent {
            assert_eq!(repo, "servx");
            assert_eq!(owner, None);
        } else {
            panic!("expected AnalyseRepo");
        }
    }

    #[test]
    fn test_analyse_owner_repo() {
        let result = parse_deterministic("analyse zync-meet/zync");
        assert!(result.is_some());
        let r = result.unwrap();
        if let ParsedIntent::AnalyseRepo { owner, repo } = r.intent {
            assert_eq!(owner, Some("zync-meet".to_string()));
            assert_eq!(repo, "zync");
        } else {
            panic!("expected AnalyseRepo");
        }
    }

    #[test]
    fn test_analyse_just_repo() {
        let result = parse_deterministic("analyse servx");
        assert!(result.is_some());
        let r = result.unwrap();
        if let ParsedIntent::AnalyseRepo { repo, .. } = r.intent {
            assert_eq!(repo, "servx");
        } else {
            panic!("expected AnalyseRepo");
        }
    }

    #[test]
    fn test_analyse_zync() {
        let result = parse_deterministic("analyse zync");
        assert!(result.is_some());
        let r = result.unwrap();
        if let ParsedIntent::AnalyseRepo { repo, .. } = r.intent {
            assert_eq!(repo, "zync");
        } else {
            panic!("expected AnalyseRepo");
        }
    }

    #[test]
    fn test_analyze_american_spelling() {
        let result = parse_deterministic("analyze PR 23 servx");
        assert!(result.is_some());
        let r = result.unwrap();
        assert!(matches!(r.intent, ParsedIntent::AnalysePr { .. }));
    }

    #[test]
    fn test_search() {
        let result = parse_deterministic("search for cats");
        assert!(result.is_some());
        let r = result.unwrap();
        if let ParsedIntent::Search { query } = r.intent {
            assert_eq!(query, "cats");
        } else {
            panic!("expected Search");
        }
    }

    #[test]
    fn test_google_search() {
        let result = parse_deterministic("google rust async programming");
        assert!(result.is_some());
        let r = result.unwrap();
        if let ParsedIntent::Search { query } = r.intent {
            assert_eq!(query, "rust async programming");
        } else {
            panic!("expected Search");
        }
    }

    #[test]
    fn test_media_pause() {
        let result = parse_deterministic("pause");
        assert!(result.is_some());
        let r = result.unwrap();
        assert!(matches!(r.intent, ParsedIntent::MediaPlayPause));
    }

    #[test]
    fn test_media_next() {
        let result = parse_deterministic("next");
        assert!(result.is_some());
        let r = result.unwrap();
        assert!(matches!(r.intent, ParsedIntent::MediaNext));
    }

    #[test]
    fn test_architect() {
        let result = parse_deterministic("open architecture mapper");
        assert!(result.is_some());
        let r = result.unwrap();
        assert!(matches!(r.intent, ParsedIntent::OpenArchitect));
    }

    #[test]
    fn test_architect_short() {
        let result = parse_deterministic("open architect");
        assert!(result.is_some());
        let r = result.unwrap();
        assert!(matches!(r.intent, ParsedIntent::OpenArchitect));
    }

    #[test]
    fn test_architect_natural_variants() {
        // All natural ways of saying the command
        let variants = [
            "open architecture mapper",
            "open the architecture mapper",
            "open architect",
            "open the architect",
            "show architecture mapper",
            "show the architecture mapper",
            "launch architecture mapper",
            "launch the architecture mapper",
            "start architecture mapper",
            "bring up architecture mapper",
            "bring up the architecture mapper",
            "pull up architecture mapper",
            "pull up the architecture mapper",
            "show me the architecture",
            "show me architecture mapper",
            "give me the architecture",
            "open architecture map",
            "open architecture window",
            "open architecture diagram",
            "open architecture graph",
            "open architecture viewer",
            "open architecture explorer",
            "open codebase mapper",
            "open dependency mapper",
            "show architecture",
            "show the architecture",
            "display architecture",
            "open the architecture",
        ];
        for v in &variants {
            let result = parse_deterministic(v);
            assert!(result.is_some(), "should match: '{}'", v);
            if let Some(r) = result {
                assert!(matches!(r.intent, ParsedIntent::OpenArchitect), "should be OpenArchitect for: '{}'", v);
            }
        }
    }

    #[test]
    fn test_architect_fuzzy_mishearing() {
        // STT mishears "architecture mapper" as "octach at mapper"
        let result = parse_deterministic("open octach at mapper");
        assert!(result.is_some(), "fuzzy match should catch 'octach at mapper'");
        let r = result.unwrap();
        assert!(matches!(r.intent, ParsedIntent::OpenArchitect));
        assert_eq!(r.source, "deterministic-fuzzy");

        // Other common mishearings
        let r2 = parse_deterministic("open arcade mapper");
        assert!(r2.is_some(), "fuzzy match should catch 'arcade mapper'");
        assert!(matches!(r2.unwrap().intent, ParsedIntent::OpenArchitect));

        let r3 = parse_deterministic("launch arch at mapper");
        assert!(r3.is_some(), "fuzzy match should catch 'arch at mapper'");
        assert!(matches!(r3.unwrap().intent, ParsedIntent::OpenArchitect));

        // Severe mishearing: "open up and remember" (from real user test)
        let r4 = parse_deterministic("open up and remember");
        assert!(r4.is_some(), "fuzzy match should catch 'open up and remember'");
        assert!(matches!(r4.unwrap().intent, ParsedIntent::OpenArchitect));

        // "open up and member" (another common mishearing)
        let r5 = parse_deterministic("open up and member");
        assert!(r5.is_some(), "fuzzy match should catch 'open up and member'");
        assert!(matches!(r5.unwrap().intent, ParsedIntent::OpenArchitect));
    }

    #[test]
    fn test_architect_fuzzy_comprehensive() {
        // Comprehensive list of all known STT mishearings
        let mishearings = [
            // "architecture" mishearings + "mapper" correct
            "open octach at mapper",
            "open arcade mapper",
            "open arch at mapper",
            "open arch mapper",
            "open architecture at mapper",
            "open architect mapper",
            "open are cat mapper",
            "open our cat mapper",
            "open ark mapper",
            "open art at mapper",
            "open art mapper",
            "open a cat mapper",
            "open acat mapper",
            // "mapper" mishearings + "architecture" correct
            "open architecture remember",
            "open architecture member",
            "open architecture december",
            "open architecture mac",
            "open architecture mad",
            "open architecture matter",
            "open architecture master",
            // Both misheard
            "open up and remember",
            "open up and member",
            "open up and december",
            "open are cat map",
            "open our cat map",
            "open ark map",
            "open art map",
            "open a cat map",
            // Short forms
            "open map",
            "open the map",
            "show map",
            "show the map",
            // With "codebase" / "dependency"
            "open codebase",
            "open codebase map",
            "open dependency map",
            "open dependencies mapper",
            // "show me" variants
            "show me the architecture",
            "show me architecture",
            "give me the architecture",
            "give me architecture mapper",
        ];
        for m in &mishearings {
            let result = parse_deterministic(m);
            assert!(result.is_some(), "fuzzy should catch: '{}'", m);
            if let Some(r) = result {
                assert!(matches!(r.intent, ParsedIntent::OpenArchitect), "should be OpenArchitect for: '{}'", m);
            }
        }
    }

    #[test]
    fn test_url_direct() {
        let result = parse_deterministic("open google.com");
        assert!(result.is_some());
        let r = result.unwrap();
        assert!(matches!(r.intent, ParsedIntent::OpenUrl { .. }));
    }

    #[test]
    fn test_unknown_command() {
        let result = parse_deterministic("what's the weather like");
        assert!(result.is_none()); // deterministic parser returns None for unknown
    }

    #[test]
    fn test_empty_input() {
        let result = parse_deterministic("");
        assert!(result.is_none());
    }

    #[test]
    fn test_levenshtein() {
        assert_eq!(levenshtein("chrome", "chrome"), 0);
        assert_eq!(levenshtein("chroem", "chrome"), 2);
        assert_eq!(levenshtein("whatsapp", "whatsapp"), 0);
    }

    #[test]
    fn test_is_valid_repo_name() {
        assert!(is_valid_repo_name("servx"));
        assert!(is_valid_repo_name("zync-meet"));
        assert!(is_valid_repo_name("zync_meet"));
        assert!(is_valid_repo_name("eesh264"));
        assert!(!is_valid_repo_name(""));
        assert!(!is_valid_repo_name("-invalid"));
        assert!(!is_valid_repo_name(".invalid"));
        assert!(!is_valid_repo_name("has space"));
    }

    #[test]
    fn test_parse_owner_repo() {
        assert_eq!(
            parse_owner_repo("zync-meet/zync"),
            Some(("zync-meet".to_string(), "zync".to_string()))
        );
        assert_eq!(
            parse_owner_repo("eesh264/congi"),
            Some(("eesh264".to_string(), "congi".to_string()))
        );
        assert_eq!(parse_owner_repo("no-slash"), None);
    }

    #[test]
    fn test_clean_repo_name() {
        assert_eq!(clean_repo_name("servx"), "servx");
        assert_eq!(clean_repo_name("the servx"), "servx");
        assert_eq!(clean_repo_name("servx repo"), "servx");
        assert_eq!(clean_repo_name("servx repository"), "servx");
    }

    // ΓöÇΓöÇΓöÇ Edge cases for Whisper mishearings ΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇ

    #[test]
    fn test_open_whats_app_mishearing() {
        // Whisper might transcribe "whatsapp" as "whats app" or "what's app"
        let result = parse_deterministic("open whats app");
        assert!(result.is_some());
        let r = result.unwrap();
        assert!(matches!(r.intent, ParsedIntent::OpenApp { .. }));
    }

    #[test]
    fn test_open_gem_ini_mishearing() {
        // Whisper might transcribe "gemini" as "gem ini"
        let result = parse_deterministic("open gem ini");
        assert!(result.is_some());
        let r = result.unwrap();
        assert!(matches!(r.intent, ParsedIntent::OpenApp { .. }));
    }

    #[test]
    fn test_open_you_tube_mishearing() {
        let result = parse_deterministic("open you tube");
        assert!(result.is_some());
        let r = result.unwrap();
        assert!(matches!(r.intent, ParsedIntent::OpenApp { .. }));
    }

    #[test]
    fn test_open_chat_gpt_mishearing() {
        let result = parse_deterministic("open chat gpt");
        assert!(result.is_some());
        let r = result.unwrap();
        assert!(matches!(r.intent, ParsedIntent::OpenApp { .. }));
    }

    #[test]
    fn test_analyse_pr_variations() {
        // Various PR command formats
        for cmd in &[
            "analyse PR 1 servx",
            "analyse PR 99 servx",
            "analyse pr 23 servx",
            "analyse PR 23 in servx",
            "analyse pull request 23 servx",
            "analyze PR 23 servx",
        ] {
            let result = parse_deterministic(cmd);
            assert!(result.is_some(), "failed to parse: {}", cmd);
            let r = result.unwrap();
            assert!(
                matches!(r.intent, ParsedIntent::AnalysePr { .. }),
                "expected AnalysePr for: {}, got {:?}",
                cmd,
                r.intent
            );
        }
    }

    #[test]
    fn test_pr_list_commands() {
        use crate::github_cmd::GitHubCommand;
        // "open pr list" must NOT be parsed as OpenApp("pr list")
        // It should be ListPrs
        for cmd in &[
            "open pr list",
            "open the pr list",
            "show pr list",
            "show the pr list",
            "show me the pr list",
            "show me pr list",
            "show me prs",
            "give me the pr list",
            "view pr list",
            "view the pr list",
            "get pr list",
            "get me the pr list",
            "get me prs",
            "tell me the pr list",
            "what prs are open",
            "show open prs",
            "show live prs",
            "latest prs",
            "give me prs",
            "show prs",
            "get prs",
            "fetch prs",
            "display prs",
            "bring me the pr list",
            "pull up the pr list",
            "show merged prs",
            "merged prs",
        ] {
            let result = parse_deterministic(cmd);
            assert!(result.is_some(), "failed to parse: {}", cmd);
            let r = result.unwrap();
            assert!(
                matches!(
                    r.intent,
                    ParsedIntent::GitHubCommand {
                        command: GitHubCommand::ListPrs { .. }
                    }
                ),
                "expected ListPrs for: {}, got {:?}",
                cmd,
                r.intent
            );
        }
    }

    #[test]
    fn test_pr_list_with_trailing_punctuation() {
        use crate::github_cmd::GitHubCommand;
        // Groq whisper-large-v3-turbo appends trailing punctuation to
        // transcripts. Without strip_trailing_punctuation() the ListPrs
        // regex (anchored with $) fails and the permissive OpenApp
        // fallback catches "the pr list." → resolves to a cached app
        // (Dribbble). This test guards against that regression.
        for cmd in &[
            "open the pr list.",
            "open the pr list?",
            "open the pr list!",
            "show the pr list.",
            "show me the pr list.",
            "show me pr list.",
            "show open prs.",
            "list open prs.",
            "what prs are open.",
            "give me the pr list.",
            "view the pr list.",
            "get prs.",
            "get me the pr list.",
            "fetch prs.",
            "display prs.",
        ] {
            let result = parse_deterministic(cmd);
            assert!(result.is_some(), "failed to parse: {}", cmd);
            let r = result.unwrap();
            assert!(
                matches!(
                    r.intent,
                    ParsedIntent::GitHubCommand {
                        command: GitHubCommand::ListPrs { .. }
                    }
                ),
                "expected ListPrs for: {}, got {:?} (trailing punctuation not stripped?)",
                cmd,
                r.intent
            );
        }
    }

    #[test]
    fn test_open_app_still_works_with_punctuation() {
        // Ensure strip_trailing_punctuation doesn't break normal app opens
        let result = parse_deterministic("open chrome.");
        assert!(result.is_some());
        let r = result.unwrap();
        assert!(
            matches!(r.intent, ParsedIntent::OpenApp { .. }),
            "expected OpenApp for 'open chrome.', got {:?}",
            r.intent
        );
    }

    #[test]
    fn test_strip_trailing_punctuation_helper() {
        assert_eq!(strip_trailing_punctuation("hello."), "hello");
        assert_eq!(strip_trailing_punctuation("hello?"), "hello");
        assert_eq!(strip_trailing_punctuation("hello!"), "hello");
        assert_eq!(strip_trailing_punctuation("hello..."), "hello");
        assert_eq!(strip_trailing_punctuation("hello"), "hello");
        // Internal punctuation preserved
        assert_eq!(strip_trailing_punctuation("owner/repo"), "owner/repo");
        assert_eq!(strip_trailing_punctuation("what's up"), "what's up");
        assert_eq!(strip_trailing_punctuation("pr#23"), "pr#23");
    }

    #[test]
    fn test_looks_like_github_phrase() {
        assert!(looks_like_github_phrase("the pr list"));
        assert!(looks_like_github_phrase("pull requests"));
        assert!(looks_like_github_phrase("the repo list"));
        assert!(looks_like_github_phrase("open prs"));
        assert!(looks_like_github_phrase("my prs"));
        // Not github phrases
        assert!(!looks_like_github_phrase("chrome"));
        assert!(!looks_like_github_phrase("notepad"));
        assert!(!looks_like_github_phrase("whatsapp"));
        assert!(!looks_like_github_phrase("preview")); // "pr" inside word, not token
        assert!(!looks_like_github_phrase("process"));
    }

    #[test]
    fn test_deep_analysis_pr() {
        // "deep analysis PR 24 in nexus-agent" ΓÇö noun form with "deep" prefix
        for cmd in &[
            "deep analysis PR 24 in nexus-agent",
            "deep analyse PR 24 in nexus-agent",
            "deep analyze PR 24 in nexus-agent",
            "analysis PR 24 in nexus-agent",
        ] {
            let result = parse_deterministic(cmd);
            assert!(result.is_some(), "failed to parse: {}", cmd);
            let r = result.unwrap();
            if let ParsedIntent::AnalysePr {
                repo, pr_number, ..
            } = r.intent
            {
                assert_eq!(repo, "nexus-agent", "wrong repo for: {}", cmd);
                assert_eq!(pr_number, 24, "wrong pr_number for: {}", cmd);
            } else {
                panic!("expected AnalysePr for: {}, got {:?}", cmd, r.intent);
            }
        }
    }

    #[test]
    fn test_analyse_repo_variations() {
        for cmd in &[
            "analyse servx",
            "analyse zync",
            "analyse servx repo",
            "analyse repo servx",
            "analyse the repo servx",
            "analyse zync-meet/zync",
            "analyse eesh264/congi",
            "analyze servx",
        ] {
            let result = parse_deterministic(cmd);
            assert!(result.is_some(), "failed to parse: {}", cmd);
            let r = result.unwrap();
            assert!(
                matches!(r.intent, ParsedIntent::AnalyseRepo { .. }),
                "expected AnalyseRepo for: {}, got {:?}",
                cmd,
                r.intent
            );
        }
    }

    #[test]
    fn test_open_verb_variations() {
        for verb in &[
            "open", "launch", "start", "run", "show", "pull up", "bring up",
            "fire up", "go to", "visit",
        ] {
            let cmd = format!("{} whatsapp", verb);
            let result = parse_deterministic(&cmd);
            assert!(result.is_some(), "failed to parse: {}", cmd);
            let r = result.unwrap();
            assert!(
                matches!(r.intent, ParsedIntent::OpenApp { .. }),
                "expected OpenApp for: {}, got {:?}",
                cmd,
                r.intent
            );
        }
    }

    #[test]
    fn test_case_insensitivity() {
        let result = parse_deterministic("OPEN WHATSAPP");
        assert!(result.is_some());
        let r = result.unwrap();
        assert!(matches!(r.intent, ParsedIntent::OpenApp { .. }));

        let result = parse_deterministic("Analyse PR 23 Servx");
        assert!(result.is_some());
        let r = result.unwrap();
        assert!(matches!(r.intent, ParsedIntent::AnalysePr { .. }));
    }

    #[test]
    fn test_extra_whitespace() {
        let result = parse_deterministic("open   whatsapp");
        assert!(result.is_some());
        let r = result.unwrap();
        assert!(matches!(r.intent, ParsedIntent::OpenApp { .. }));

        let result = parse_deterministic("analyse  PR  23  servx");
        assert!(result.is_some());
        let r = result.unwrap();
        assert!(matches!(r.intent, ParsedIntent::AnalysePr { .. }));
    }

    #[test]
    fn test_pr_number_extraction() {
        // Verify PR numbers are correctly extracted
        let test_cases = [(1u32), (5), (23), (99), (100), (999)];
        for (i, expected_pr) in test_cases.iter().enumerate() {
            let cmd = format!("analyse PR {} servx", expected_pr);
            let result = parse_deterministic(&cmd);
            assert!(result.is_some());
            if let ParsedIntent::AnalysePr { pr_number, .. } = result.unwrap().intent {
                assert_eq!(pr_number, *expected_pr, "PR number mismatch for case {}", i);
            } else {
                panic!("expected AnalysePr");
            }
        }
    }

    // ΓöÇΓöÇΓöÇ "the PR" pattern tests ΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇ

    #[test]
    fn test_pr_with_the_preposition() {
        // "analyse the pr 254 in zync" ΓÇö user says "the" before "pr"
        let result = parse_deterministic("analyse the pr 254 in zync");
        assert!(result.is_some());
        if let ParsedIntent::AnalysePr { repo, pr_number, .. } = result.unwrap().intent {
            assert_eq!(pr_number, 254);
            assert_eq!(repo, "zync");
        } else {
            panic!("expected AnalysePr");
        }
    }

    #[test]
    fn test_pr_with_the_no_preposition() {
        // "analyse the pr 254 zync" ΓÇö "the" before "pr", no preposition
        let result = parse_deterministic("analyse the pr 254 zync");
        assert!(result.is_some());
        if let ParsedIntent::AnalysePr { repo, pr_number, .. } = result.unwrap().intent {
            assert_eq!(pr_number, 254);
            assert_eq!(repo, "zync");
        } else {
            panic!("expected AnalysePr");
        }
    }

    #[test]
    fn test_pr_the_pull_request() {
        // "analyse the pull request 254 in zync"
        let result = parse_deterministic("analyse the pull request 254 in zync");
        assert!(result.is_some());
        if let ParsedIntent::AnalysePr { repo, pr_number, .. } = result.unwrap().intent {
            assert_eq!(pr_number, 254);
            assert_eq!(repo, "zync");
        } else {
            panic!("expected AnalysePr");
        }
    }

    // ΓöÇΓöÇΓöÇ Fuzzy matching tests ΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇ

    #[test]
    fn test_fuzzy_match_zink_to_zync() {
        // "zink" is 1 edit from "zync" (iΓåÆy) ΓÇö should fuzzy match
        let result = parse_deterministic("analyse pr 254 in zink");
        assert!(result.is_some());
        if let ParsedIntent::AnalysePr { repo, pr_number, .. } = result.unwrap().intent {
            assert_eq!(pr_number, 254);
            assert_eq!(repo, "zync");
        } else {
            panic!("expected AnalysePr");
        }
    }

    #[test]
    fn test_fuzzy_match_zinc_to_zync() {
        // "zinc" is 1 edit from "zync" (iΓåÆy) ΓÇö should fuzzy match
        let result = parse_deterministic("analyse pr 254 in zinc");
        assert!(result.is_some());
        if let ParsedIntent::AnalysePr { repo, pr_number, .. } = result.unwrap().intent {
            assert_eq!(pr_number, 254);
            assert_eq!(repo, "zync");
        } else {
            panic!("expected AnalysePr");
        }
    }

    #[test]
    fn test_fuzzy_match_sink_to_zync() {
        // "sink" is 3 edits from "zync" (sΓåÆz, iΓåÆy, kΓåÆc) ΓÇö exceeds threshold 2
        // for short repos. Should NOT fuzzy match ΓÇö returns "sink" as-is.
        let result = parse_deterministic("analyse pr 254 in sink");
        assert!(result.is_some());
        if let ParsedIntent::AnalysePr { repo, pr_number, .. } = result.unwrap().intent {
            assert_eq!(pr_number, 254);
            // "sink" is too far from "zync" (distance 3 > threshold 2)
            // so it's returned as-is, not fuzzy-matched
            assert_eq!(repo, "sink");
        } else {
            panic!("expected AnalysePr");
        }
    }

    #[test]
    fn test_fuzzy_match_cervix_to_servx() {
        // "cervix" ΓåÆ "servx": cΓåÆs + delete i = 2 edits ΓÇö should fuzzy match
        let result = parse_deterministic("analyse pr 254 in cervix");
        assert!(result.is_some());
        if let ParsedIntent::AnalysePr { repo, pr_number, .. } = result.unwrap().intent {
            assert_eq!(pr_number, 254);
            assert_eq!(repo, "servx");
        } else {
            panic!("expected AnalysePr");
        }
    }

    #[test]
    fn test_fuzzy_does_not_match_too_far() {
        // "chrome" is too far from any known repo ΓÇö should NOT fuzzy match
        let result = parse_deterministic("analyse pr 254 in chrome");
        // "chrome" won't match any known repo within threshold
        // But it will still be accepted as a repo name (clean_repo_name)
        // because the exact pattern "pr <num> in <word>" matches.
        // This is correct behavior ΓÇö we only fuzzy match when exact fails.
        if let Some(r) = result {
            if let ParsedIntent::AnalysePr { repo, .. } = r.intent {
                // Should be "chrome" as-is, not fuzzy-matched to something
                assert_eq!(repo, "chrome");
            }
        }
    }

    #[test]
    fn test_fuzzy_match_confidence_lower() {
        // Fuzzy matches should have lower confidence (0.8) than exact (1.0).
        // NOTE: "zink" no longer goes fuzzy — the canonical sound-alias map
        // resolves it to "zync" inside clean_repo_name, so the exact path
        // wins at 1.0. A non-aliased misspelling still goes fuzzy.
        let result = parse_deterministic("analyse pr 254 in zink");
        assert!(result.is_some());
        let r = result.unwrap();
        if let ParsedIntent::AnalysePr { .. } = r.intent {
            assert_eq!(r.confidence, 1.0);
            assert_eq!(r.source, "deterministic");
        } else {
            panic!("expected AnalysePr");
        }
    }

    // ΓöÇΓöÇΓöÇ Greeting tests ΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇ

    #[test]
    fn test_greeting_hello() {
        let result = parse_deterministic("hello");
        assert!(result.is_some());
        assert!(matches!(result.unwrap().intent, ParsedIntent::Greeting { .. }));
    }

    #[test]
    fn test_greeting_hi() {
        let result = parse_deterministic("hi");
        assert!(result.is_some());
        assert!(matches!(result.unwrap().intent, ParsedIntent::Greeting { .. }));
    }

    #[test]
    fn test_greeting_hey() {
        let result = parse_deterministic("hey");
        assert!(result.is_some());
        assert!(matches!(result.unwrap().intent, ParsedIntent::Greeting { .. }));
    }

    #[test]
    fn test_greeting_how_are_you() {
        let result = parse_deterministic("how are you");
        assert!(result.is_some());
        assert!(matches!(result.unwrap().intent, ParsedIntent::Greeting { .. }));
    }

    #[test]
    fn test_greeting_hows_it_going() {
        let result = parse_deterministic("how's it going");
        assert!(result.is_some());
        assert!(matches!(result.unwrap().intent, ParsedIntent::Greeting { .. }));
    }

    #[test]
    fn test_greeting_bye() {
        let result = parse_deterministic("bye");
        assert!(result.is_some());
        assert!(matches!(result.unwrap().intent, ParsedIntent::Greeting { .. }));
    }

    #[test]
    fn test_greeting_goodbye() {
        let result = parse_deterministic("goodbye");
        assert!(result.is_some());
        assert!(matches!(result.unwrap().intent, ParsedIntent::Greeting { .. }));
    }

    #[test]
    fn test_greeting_see_you() {
        let result = parse_deterministic("see you");
        assert!(result.is_some());
        assert!(matches!(result.unwrap().intent, ParsedIntent::Greeting { .. }));
    }

    #[test]
    fn test_greeting_thanks() {
        let result = parse_deterministic("thanks");
        assert!(result.is_some());
        assert!(matches!(result.unwrap().intent, ParsedIntent::Greeting { .. }));
    }

    #[test]
    fn test_greeting_thank_you() {
        let result = parse_deterministic("thank you");
        assert!(result.is_some());
        assert!(matches!(result.unwrap().intent, ParsedIntent::Greeting { .. }));
    }

    #[test]
    fn test_greeting_what_is_your_name() {
        let result = parse_deterministic("what's your name");
        assert!(result.is_some());
        assert!(matches!(result.unwrap().intent, ParsedIntent::Greeting { .. }));
    }

    #[test]
    fn test_greeting_who_are_you() {
        let result = parse_deterministic("who are you");
        assert!(result.is_some());
        assert!(matches!(result.unwrap().intent, ParsedIntent::Greeting { .. }));
    }

    #[test]
    fn test_greeting_what_can_you_do() {
        let result = parse_deterministic("what can you do");
        assert!(result.is_some());
        assert!(matches!(result.unwrap().intent, ParsedIntent::Greeting { .. }));
    }

    #[test]
    fn test_greeting_good_morning() {
        let result = parse_deterministic("good morning");
        assert!(result.is_some());
        if let ParsedIntent::Greeting { reply } = result.unwrap().intent {
            assert!(reply.contains("morning"), "expected 'morning' in reply: {}", reply);
        } else {
            panic!("expected Greeting");
        }
    }

    #[test]
    fn test_greeting_good_evening() {
        let result = parse_deterministic("good evening");
        assert!(result.is_some());
        if let ParsedIntent::Greeting { reply } = result.unwrap().intent {
            assert!(reply.contains("evening"), "expected 'evening' in reply: {}", reply);
        } else {
            panic!("expected Greeting");
        }
    }

    #[test]
    fn test_greeting_yes() {
        let result = parse_deterministic("yes");
        assert!(result.is_some());
        assert!(matches!(result.unwrap().intent, ParsedIntent::Greeting { .. }));
    }

    #[test]
    fn test_greeting_ok() {
        let result = parse_deterministic("ok");
        assert!(result.is_some());
        assert!(matches!(result.unwrap().intent, ParsedIntent::Greeting { .. }));
    }

    #[test]
    fn test_greeting_no() {
        let result = parse_deterministic("no");
        assert!(result.is_some());
        assert!(matches!(result.unwrap().intent, ParsedIntent::Greeting { .. }));
    }

    #[test]
    fn test_greeting_never_mind() {
        let result = parse_deterministic("never mind");
        assert!(result.is_some());
        assert!(matches!(result.unwrap().intent, ParsedIntent::Greeting { .. }));
    }

    #[test]
    fn test_greeting_with_nexus_suffix() {
        let result = parse_deterministic("hello nexus");
        assert!(result.is_some());
        assert!(matches!(result.unwrap().intent, ParsedIntent::Greeting { .. }));
    }

    #[test]
    fn test_greeting_not_triggered_by_open() {
        // "open hello" should NOT be a greeting ΓÇö it's an open command
        let result = parse_deterministic("open hello");
        assert!(result.is_some());
        assert!(!matches!(result.unwrap().intent, ParsedIntent::Greeting { .. }));
    }

    #[test]
    fn test_greeting_not_triggered_by_search() {
        // "search for hello" should NOT be a greeting
        let result = parse_deterministic("search for hello");
        assert!(result.is_some());
        assert!(!matches!(result.unwrap().intent, ParsedIntent::Greeting { .. }));
    }

    #[test]
    fn test_greeting_pick_is_deterministic() {
        // Same input should always produce the same reply
        let r1 = parse_deterministic("hello");
        let r2 = parse_deterministic("hello");
        assert!(r1.is_some() && r2.is_some());
        if let (Some(ParseResult { intent: ParsedIntent::Greeting { reply: ref1 }, .. }),
                Some(ParseResult { intent: ParsedIntent::Greeting { reply: ref2 }, .. })) = (r1, r2) {
            assert_eq!(ref1, ref2, "same input should produce same reply");
        } else {
            panic!("expected Greeting");
        }
    }

    // ─── GitHub command parsing tests (Phase 2A-6) ────────────────

    #[test]
    fn test_parse_merge_pr() {
        let result = parse_deterministic("merge pr 23 in owner/repo");
        assert!(result.is_some());
        if let ParsedIntent::GitHubCommand {
            command: crate::github_cmd::GitHubCommand::MergePr { repo, pr_number, method },
        } = result.unwrap().intent
        {
            assert_eq!(repo, "owner/repo");
            assert_eq!(pr_number, 23);
            assert_eq!(method, crate::github_cmd::MergeMethod::Merge);
        } else {
            panic!("expected MergePr");
        }
    }

    #[test]
    fn test_parse_squash_merge_pr() {
        let result = parse_deterministic("squash merge pr 5 in owner/repo");
        assert!(result.is_some());
        if let ParsedIntent::GitHubCommand {
            command: crate::github_cmd::GitHubCommand::MergePr { method, .. },
        } = result.unwrap().intent
        {
            assert_eq!(method, crate::github_cmd::MergeMethod::Squash);
        } else {
            panic!("expected MergePr");
        }
    }

    #[test]
    fn test_parse_rebase_merge_pr() {
        let result = parse_deterministic("rebase merge pr 10 in owner/repo");
        assert!(result.is_some());
        if let ParsedIntent::GitHubCommand {
            command: crate::github_cmd::GitHubCommand::MergePr { method, .. },
        } = result.unwrap().intent
        {
            assert_eq!(method, crate::github_cmd::MergeMethod::Rebase);
        } else {
            panic!("expected MergePr");
        }
    }

    #[test]
    fn test_parse_approve_pr() {
        let result = parse_deterministic("approve pr 5 in owner/repo");
        assert!(result.is_some());
        if let ParsedIntent::GitHubCommand {
            command: crate::github_cmd::GitHubCommand::ApprovePr { repo, pr_number },
        } = result.unwrap().intent
        {
            assert_eq!(repo, "owner/repo");
            assert_eq!(pr_number, 5);
        } else {
            panic!("expected ApprovePr");
        }
    }

    #[test]
    fn test_parse_close_pr() {
        let result = parse_deterministic("close pr 10 in owner/repo");
        assert!(result.is_some());
        if let ParsedIntent::GitHubCommand {
            command: crate::github_cmd::GitHubCommand::ClosePr { pr_number, .. },
        } = result.unwrap().intent
        {
            assert_eq!(pr_number, 10);
        } else {
            panic!("expected ClosePr");
        }
    }

    #[test]
    fn test_parse_list_prs() {
        let result = parse_deterministic("list prs in owner/repo");
        assert!(result.is_some());
        if let ParsedIntent::GitHubCommand {
            command: crate::github_cmd::GitHubCommand::ListPrs { repo, state },
        } = result.unwrap().intent
        {
            assert_eq!(repo, "owner/repo");
            assert_eq!(state, "open");
        } else {
            panic!("expected ListPrs");
        }
    }

    #[test]
    fn test_parse_list_closed_prs() {
        let result = parse_deterministic("list closed prs in owner/repo");
        assert!(result.is_some());
        if let ParsedIntent::GitHubCommand {
            command: crate::github_cmd::GitHubCommand::ListPrs { state, .. },
        } = result.unwrap().intent
        {
            assert_eq!(state, "closed");
        } else {
            panic!("expected ListPrs");
        }
    }

    #[test]
    fn test_canonical_repo_name_sound_aliases() {
        // Every sounding from real STT logs resolves to the same entity.
        for heard in [
            "servx", "cervix", "cervx", "srvx", "service", "cervex",
            "cervets", "servetus", "servex",
        ] {
            assert_eq!(canonical_repo_name(heard), "servx", "heard: {heard}");
        }
        for heard in ["zync", "zinc", "zink", "sync", "zynk"] {
            assert_eq!(canonical_repo_name(heard), "zync", "heard: {heard}");
        }
        assert_eq!(canonical_repo_name("incognito"), "congi");
        assert_eq!(canonical_repo_name("meat"), "meet");
        assert_eq!(canonical_repo_name("shopcart"), "shopkart");
        assert_eq!(canonical_repo_name("ledger ai"), "ledger-ai");
        // Owner/repo paths map per segment.
        assert_eq!(canonical_repo_name("sync-meet/sync"), "sync-meet/zync");
        // Unknown names pass through untouched — never rewrites a real name.
        assert_eq!(canonical_repo_name("myrepo"), "myrepo");
        assert_eq!(canonical_repo_name("ChitkulLakshya/ultrabot"), "ChitkulLakshya/ultrabot");
    }

    #[test]
    fn test_parse_list_prs_heard_repo_resolves() {
        // End to end: STT heard "cervix", user meant servx.
        let result = parse_deterministic("list prs in cervix");
        assert!(result.is_some());
        if let ParsedIntent::GitHubCommand {
            command: crate::github_cmd::GitHubCommand::ListPrs { repo, .. },
        } = result.unwrap().intent
        {
            assert_eq!(repo, "servx");
        } else {
            panic!("expected ListPrs");
        }
    }

    #[test]
    fn test_parse_list_prs_pull_request_wording() {
        // "pull requests" is a full alternative to "prs".
        for phrase in [
            "show me the prs",
            "show me the pull requests",
            "show me the pull requests and all",
            "show the pull requests in owner/repo",
            "give me all the pull requests",
            "show prs and all of them",
            "pull up the pull request list",
        ] {
            let result = parse_deterministic(phrase);
            assert!(result.is_some(), "should parse '{phrase}'");
            if let ParsedIntent::GitHubCommand {
                command: crate::github_cmd::GitHubCommand::ListPrs { .. },
            } = result.unwrap().intent
            {
            } else {
                panic!("expected ListPrs for '{phrase}'");
            }
        }
        // …but a bare "everything" with no PR noun must NOT list PRs.
        let other = parse_deterministic("show me everything").map(|r| r.intent);
        assert!(
            !matches!(
                other,
                Some(ParsedIntent::GitHubCommand {
                    command: crate::github_cmd::GitHubCommand::ListPrs { .. },
                })
            ),
            "bare 'everything' must not list PRs"
        );
    }

    #[test]
    fn test_parse_get_pr() {
        let result = parse_deterministic("show pr 42 in owner/repo");
        assert!(result.is_some());
        if let ParsedIntent::GitHubCommand {
            command: crate::github_cmd::GitHubCommand::GetPr { pr_number, .. },
        } = result.unwrap().intent
        {
            assert_eq!(pr_number, 42);
        } else {
            panic!("expected GetPr");
        }
    }

    #[test]
    fn test_parse_revert_pr() {
        let result = parse_deterministic("revert pr 99 in owner/repo");
        assert!(result.is_some());
        if let ParsedIntent::GitHubCommand {
            command: crate::github_cmd::GitHubCommand::RevertPr { pr_number, .. },
        } = result.unwrap().intent
        {
            assert_eq!(pr_number, 99);
        } else {
            panic!("expected RevertPr");
        }
    }

    #[test]
    fn test_parse_add_collaborator() {
        let result = parse_deterministic("add user1 as admin collaborator to owner/repo");
        assert!(result.is_some());
        if let ParsedIntent::GitHubCommand {
            command: crate::github_cmd::GitHubCommand::AddCollaborator { username, permission, .. },
        } = result.unwrap().intent
        {
            assert_eq!(username, "user1");
            assert_eq!(permission, crate::github_cmd::CollaboratorPermission::Admin);
        } else {
            panic!("expected AddCollaborator");
        }
    }

    #[test]
    fn test_parse_add_collaborator_default_permission() {
        let result = parse_deterministic("add user1 as collaborator to owner/repo");
        assert!(result.is_some());
        if let ParsedIntent::GitHubCommand {
            command: crate::github_cmd::GitHubCommand::AddCollaborator { permission, .. },
        } = result.unwrap().intent
        {
            assert_eq!(permission, crate::github_cmd::CollaboratorPermission::Push);
        } else {
            panic!("expected AddCollaborator");
        }
    }

    #[test]
    fn test_parse_remove_collaborator() {
        let result = parse_deterministic("remove user1 as collaborator from owner/repo");
        assert!(result.is_some());
        if let ParsedIntent::GitHubCommand {
            command: crate::github_cmd::GitHubCommand::RemoveCollaborator { username, .. },
        } = result.unwrap().intent
        {
            assert_eq!(username, "user1");
        } else {
            panic!("expected RemoveCollaborator");
        }
    }

    #[test]
    fn test_parse_list_collaborators() {
        let result = parse_deterministic("list collaborators in owner/repo");
        assert!(result.is_some());
        assert!(matches!(
            result.unwrap().intent,
            ParsedIntent::GitHubCommand {
                command: crate::github_cmd::GitHubCommand::ListCollaborators { .. }
            }
        ));
    }

    #[test]
    fn test_parse_add_org_member() {
        let result = parse_deterministic("add user1 to org myorg");
        assert!(result.is_some());
        if let ParsedIntent::GitHubCommand {
            command: crate::github_cmd::GitHubCommand::AddOrgMember { org, username, role },
        } = result.unwrap().intent
        {
            assert_eq!(org, "myorg");
            assert_eq!(username, "user1");
            assert_eq!(role, crate::github_cmd::OrgRole::Member);
        } else {
            panic!("expected AddOrgMember");
        }
    }

    #[test]
    fn test_parse_add_org_admin() {
        let result = parse_deterministic("add user1 as admin to org myorg");
        assert!(result.is_some());
        if let ParsedIntent::GitHubCommand {
            command: crate::github_cmd::GitHubCommand::AddOrgMember { role, .. },
        } = result.unwrap().intent
        {
            assert_eq!(role, crate::github_cmd::OrgRole::Admin);
        } else {
            panic!("expected AddOrgMember");
        }
    }

    #[test]
    fn test_parse_remove_org_member() {
        let result = parse_deterministic("remove user1 from org myorg");
        assert!(result.is_some());
        if let ParsedIntent::GitHubCommand {
            command: crate::github_cmd::GitHubCommand::RemoveOrgMember { org, username },
        } = result.unwrap().intent
        {
            assert_eq!(org, "myorg");
            assert_eq!(username, "user1");
        } else {
            panic!("expected RemoveOrgMember");
        }
    }

    #[test]
    fn test_parse_list_org_members() {
        let result = parse_deterministic("list members of org myorg");
        assert!(result.is_some());
        assert!(matches!(
            result.unwrap().intent,
            ParsedIntent::GitHubCommand {
                command: crate::github_cmd::GitHubCommand::ListOrgMembers { .. }
            }
        ));
    }

    #[test]
    fn test_parse_list_branches() {
        let result = parse_deterministic("list branches in owner/repo");
        assert!(result.is_some());
        assert!(matches!(
            result.unwrap().intent,
            ParsedIntent::GitHubCommand {
                command: crate::github_cmd::GitHubCommand::ListBranches { .. }
            }
        ));
    }

    #[test]
    fn test_parse_delete_branch() {
        let result = parse_deterministic("delete branch feature in owner/repo");
        assert!(result.is_some());
        if let ParsedIntent::GitHubCommand {
            command: crate::github_cmd::GitHubCommand::DeleteBranch { branch, .. },
        } = result.unwrap().intent
        {
            assert_eq!(branch, "feature");
        } else {
            panic!("expected DeleteBranch");
        }
    }

    #[test]
    fn test_parse_list_releases() {
        let result = parse_deterministic("list releases in owner/repo");
        assert!(result.is_some());
        assert!(matches!(
            result.unwrap().intent,
            ParsedIntent::GitHubCommand {
                command: crate::github_cmd::GitHubCommand::ListReleases { .. }
            }
        ));
    }

    #[test]
    fn test_parse_list_workflows() {
        let result = parse_deterministic("list workflows in owner/repo");
        assert!(result.is_some());
        assert!(matches!(
            result.unwrap().intent,
            ParsedIntent::GitHubCommand {
                command: crate::github_cmd::GitHubCommand::ListWorkflows { .. }
            }
        ));
    }

    #[test]
    fn test_parse_list_workflow_runs() {
        let result = parse_deterministic("list workflow runs in owner/repo");
        assert!(result.is_some());
        assert!(matches!(
            result.unwrap().intent,
            ParsedIntent::GitHubCommand {
                command: crate::github_cmd::GitHubCommand::ListWorkflowRuns { .. }
            }
        ));
    }

    #[test]
    fn test_parse_rerun_workflow() {
        let result = parse_deterministic("rerun workflow 123 in owner/repo");
        assert!(result.is_some());
        if let ParsedIntent::GitHubCommand {
            command: crate::github_cmd::GitHubCommand::RerunWorkflow { run_id, .. },
        } = result.unwrap().intent
        {
            assert_eq!(run_id, 123);
        } else {
            panic!("expected RerunWorkflow");
        }
    }

    #[test]
    fn test_parse_cancel_workflow() {
        let result = parse_deterministic("cancel workflow 456 in owner/repo");
        assert!(result.is_some());
        if let ParsedIntent::GitHubCommand {
            command: crate::github_cmd::GitHubCommand::CancelWorkflow { run_id, .. },
        } = result.unwrap().intent
        {
            assert_eq!(run_id, 456);
        } else {
            panic!("expected CancelWorkflow");
        }
    }

    #[test]
    fn test_route_github_command_to_github_subsystem() {
        use crate::orchestrator::{route_intent, Subsystem};
        let intent = ParsedIntent::GitHubCommand {
            command: crate::github_cmd::GitHubCommand::MergePr {
                repo: "owner/repo".into(),
                pr_number: 1,
                method: crate::github_cmd::MergeMethod::Squash,
            },
        };
        assert_eq!(route_intent(&intent), Subsystem::GitHub);
    }

    #[test]
    fn test_non_github_not_routed_to_github() {
        use crate::orchestrator::{route_intent, Subsystem};
        let intent = ParsedIntent::Search { query: "test".into() };
        assert_ne!(route_intent(&intent), Subsystem::GitHub);
    }

    // ─── Live mode intent parsing tests ─────────────────────────────

    #[test]
    fn test_parse_live_type_text() {
        let result = parse_deterministic("type hello world");
        assert!(result.is_some());
        if let ParsedIntent::NluResult { intent, slots, .. } = result.unwrap().intent {
            assert_eq!(intent, "type_text");
            assert_eq!(slots["text"], "hello world");
        } else {
            panic!("expected NluResult for type_text");
        }
    }

    #[test]
    fn test_parse_live_press_key() {
        let result = parse_deterministic("press enter");
        assert!(result.is_some());
        if let ParsedIntent::NluResult { intent, slots, .. } = result.unwrap().intent {
            assert_eq!(intent, "press_key");
            assert_eq!(slots["key"], "enter");
        } else {
            panic!("expected NluResult for press_key");
        }
    }

    #[test]
    fn test_parse_live_press_hotkey() {
        let result = parse_deterministic("press ctrl a");
        assert!(result.is_some());
        if let ParsedIntent::NluResult { intent, slots, .. } = result.unwrap().intent {
            assert_eq!(intent, "press_hotkey");
            assert_eq!(slots["keys"][0], "ctrl");
            assert_eq!(slots["keys"][1], "a");
        } else {
            panic!("expected NluResult for press_hotkey");
        }
    }

    #[test]
    fn test_parse_live_send() {
        let result = parse_deterministic("send");
        assert!(result.is_some());
        if let ParsedIntent::NluResult { intent, .. } = result.unwrap().intent {
            assert_eq!(intent, "confirm_send");
        } else {
            panic!("expected NluResult for confirm_send");
        }
    }

    #[test]
    fn test_parse_live_send_it() {
        let result = parse_deterministic("send it");
        assert!(result.is_some());
        if let ParsedIntent::NluResult { intent, .. } = result.unwrap().intent {
            assert_eq!(intent, "confirm_send");
        } else {
            panic!("expected NluResult for confirm_send");
        }
    }

    #[test]
    fn test_parse_live_cancel() {
        // "cancel" is handled by the greeting parser, not live mode
        let result = parse_deterministic("cancel");
        assert!(result.is_some());
        assert!(matches!(result.unwrap().intent, ParsedIntent::Greeting { .. }));
    }

    #[test]
    fn test_parse_live_never_mind() {
        // "never mind" is handled by the greeting parser, not live mode
        let result = parse_deterministic("never mind");
        assert!(result.is_some());
        assert!(matches!(result.unwrap().intent, ParsedIntent::Greeting { .. }));
    }

    #[test]
    fn test_parse_live_new_tab() {
        let result = parse_deterministic("new tab");
        assert!(result.is_some());
        if let ParsedIntent::NluResult { intent, .. } = result.unwrap().intent {
            assert_eq!(intent, "browser_new_tab");
        } else {
            panic!("expected NluResult for browser_new_tab");
        }
    }

    #[test]
    fn test_parse_live_open_new_tab() {
        let result = parse_deterministic("open new tab");
        assert!(result.is_some());
        if let ParsedIntent::NluResult { intent, .. } = result.unwrap().intent {
            assert_eq!(intent, "browser_new_tab");
        } else {
            panic!("expected NluResult for browser_new_tab");
        }
    }

    #[test]
    fn test_parse_live_does_not_interfere_with_open() {
        // "open whatsapp" should still be OpenApp, not a live command
        let result = parse_deterministic("open whatsapp");
        assert!(result.is_some());
        assert!(matches!(result.unwrap().intent, ParsedIntent::OpenApp { .. }));
    }

    #[test]
    fn test_parse_live_does_not_interfere_with_search() {
        // "search for cats" should still be Search, not a live command
        let result = parse_deterministic("search for cats");
        assert!(result.is_some());
        assert!(matches!(result.unwrap().intent, ParsedIntent::Search { .. }));
    }

    // ─── Commerce: OrderFood ───────────────────────────────────────────

    #[test]
    fn test_order_food_with_restaurant() {
        let result = parse_deterministic("order pizza from dominos");
        assert!(result.is_some());
        if let ParsedIntent::OrderFood { query, restaurant } = result.unwrap().intent {
            assert_eq!(query, "pizza");
            assert_eq!(restaurant.as_deref(), Some("dominos"));
        } else {
            panic!("expected OrderFood");
        }
    }

    #[test]
    fn test_order_food_generic_from_swiggy() {
        let result = parse_deterministic("order food from swiggy");
        assert!(result.is_some());
        if let ParsedIntent::OrderFood { query, restaurant } = result.unwrap().intent {
            assert_eq!(query, "");
            assert_eq!(restaurant.as_deref(), Some("swiggy"));
        } else {
            panic!("expected OrderFood");
        }
    }

    #[test]
    fn test_order_food_no_restaurant() {
        let result = parse_deterministic("order biryani");
        assert!(result.is_some());
        if let ParsedIntent::OrderFood { query, restaurant } = result.unwrap().intent {
            assert_eq!(query, "biryani");
            assert!(restaurant.is_none());
        } else {
            panic!("expected OrderFood");
        }
    }

    #[test]
    fn test_order_food_generic() {
        let result = parse_deterministic("order food");
        assert!(result.is_some());
        if let ParsedIntent::OrderFood { query, .. } = result.unwrap().intent {
            assert_eq!(query, "");
        } else {
            panic!("expected OrderFood");
        }
    }

    #[test]
    fn test_get_food_from_swiggy() {
        let result = parse_deterministic("get food from swiggy");
        assert!(result.is_some());
        if let ParsedIntent::OrderFood { restaurant, .. } = result.unwrap().intent {
            assert_eq!(restaurant.as_deref(), Some("swiggy"));
        } else {
            panic!("expected OrderFood");
        }
    }

    // ─── Commerce: SearchProduct ───────────────────────────────────────

    #[test]
    fn test_search_product_on_amazon() {
        let result = parse_deterministic("search for sony headphones on amazon");
        assert!(result.is_some());
        if let ParsedIntent::SearchProduct { query } = result.unwrap().intent {
            assert_eq!(query, "sony headphones");
        } else {
            panic!("expected SearchProduct");
        }
    }

    #[test]
    fn test_find_product_on_amazon() {
        let result = parse_deterministic("find wireless earbuds on amazon");
        assert!(result.is_some());
        if let ParsedIntent::SearchProduct { query } = result.unwrap().intent {
            assert_eq!(query, "wireless earbuds");
        } else {
            panic!("expected SearchProduct");
        }
    }

    #[test]
    fn test_amazon_search_prefix() {
        let result = parse_deterministic("amazon search for laptop");
        assert!(result.is_some());
        if let ParsedIntent::SearchProduct { query } = result.unwrap().intent {
            assert_eq!(query, "laptop");
        } else {
            panic!("expected SearchProduct");
        }
    }

    #[test]
    fn test_search_amazon_for() {
        let result = parse_deterministic("search amazon for keyboard");
        assert!(result.is_some());
        if let ParsedIntent::SearchProduct { query } = result.unwrap().intent {
            assert_eq!(query, "keyboard");
        } else {
            panic!("expected SearchProduct");
        }
    }

    #[test]
    fn test_order_a_pizza_from_dominos() {
        let result = parse_deterministic("can you order a pizza from dominos");
        assert!(result.is_some());
        if let ParsedIntent::OrderFood { query, restaurant } = result.unwrap().intent {
            assert_eq!(query, "pizza");
            assert_eq!(restaurant.as_deref(), Some("dominos"));
        } else {
            panic!("expected OrderFood");
        }
    }

    #[test]
    fn test_order_biryani_on_swiggy() {
        let result = parse_deterministic("order biryani on swiggy");
        assert!(result.is_some());
        if let ParsedIntent::OrderFood { query, restaurant } = result.unwrap().intent {
            assert_eq!(query, "biryani");
            assert_eq!(restaurant.as_deref(), Some("swiggy"));
        } else {
            panic!("expected OrderFood");
        }
    }

    #[test]
    fn test_buy_product_on_amazon() {
        let result = parse_deterministic("buy laptop on amazon");
        assert!(result.is_some());
        if let ParsedIntent::SearchProduct { query } = result.unwrap().intent {
            assert_eq!(query, "laptop");
        } else {
            panic!("expected SearchProduct");
        }
    }

    #[test]
    fn test_look_for_shoes_on_amazon() {
        let result = parse_deterministic("can you look for shoes on amazon");
        assert!(result.is_some());
        if let ParsedIntent::SearchProduct { query } = result.unwrap().intent {
            assert_eq!(query, "shoes");
        } else {
            panic!("expected SearchProduct");
        }
    }

    #[test]
    fn test_search_without_amazon_is_regular_search() {
        // "search for cats" (no "on amazon") should still be regular Search
        let result = parse_deterministic("search for cats");
        assert!(result.is_some());
        assert!(matches!(result.unwrap().intent, ParsedIntent::Search { .. }));
    }

    // ─── Social: SendWhatsAppMessage ───────────────────────────────────

    #[test]
    fn test_send_whatsapp_message_full() {
        let result =
            parse_deterministic("send mom a whatsapp message saying i'll be late");
        assert!(result.is_some());
        if let ParsedIntent::SendWhatsAppMessage { contact, message } = result.unwrap().intent {
            assert_eq!(contact, "mom");
            assert_eq!(message, "i'll be late");
        } else {
            panic!("expected SendWhatsAppMessage");
        }
    }

    #[test]
    fn test_send_message_saying() {
        let result = parse_deterministic("send dad a message saying on my way");
        assert!(result.is_some());
        if let ParsedIntent::SendWhatsAppMessage { contact, message } = result.unwrap().intent {
            assert_eq!(contact, "dad");
            assert_eq!(message, "on my way");
        } else {
            panic!("expected SendWhatsAppMessage");
        }
    }

    #[test]
    fn test_whatsapp_contact_saying() {
        let result = parse_deterministic("whatsapp mom saying i'm coming");
        assert!(result.is_some());
        if let ParsedIntent::SendWhatsAppMessage { contact, message } = result.unwrap().intent {
            assert_eq!(contact, "mom");
            assert_eq!(message, "i'm coming");
        } else {
            panic!("expected SendWhatsAppMessage");
        }
    }

    #[test]
    fn test_message_on_whatsapp_saying() {
        let result = parse_deterministic("message lakshya on whatsapp saying hello");
        assert!(result.is_some());
        if let ParsedIntent::SendWhatsAppMessage { contact, message } = result.unwrap().intent {
            assert_eq!(contact, "lakshya");
            assert_eq!(message, "hello");
        } else {
            panic!("expected SendWhatsAppMessage");
        }
    }

    #[test]
    fn test_whatsapp_chat_still_parses_as_chat() {
        // "open chat with mom" should still be WhatsappChat, not SendWhatsAppMessage
        let result = parse_deterministic("open chat with mom");
        assert!(result.is_some());
        assert!(matches!(
            result.unwrap().intent,
            ParsedIntent::WhatsappChat { .. }
        ));
    }

    #[test]
    fn test_partial_send_asks_for_message() {
        // The exact failure from the field: action + contact, no body.
        // Must ask ("What should I say to mummy?") — never fall to Worker.
        let result = parse_deterministic("can you send message to mummy in whatsapp");
        assert!(result.is_some());
        if let ParsedIntent::NeedMoreInfo { prompt } = result.unwrap().intent {
            assert!(prompt.contains("mummy"), "prompt names the contact: {prompt}");
        } else {
            panic!("expected NeedMoreInfo");
        }
    }

    #[test]
    fn test_partial_send_without_saying_asks() {
        let result = parse_deterministic("send mom a whatsapp message");
        assert!(result.is_some());
        assert!(matches!(
            result.unwrap().intent,
            ParsedIntent::NeedMoreInfo { .. }
        ));
    }

    #[test]
    fn test_send_message_to_contact_in_whatsapp() {
        let result = parse_deterministic("send hi to mummy in whatsapp");
        match result {
            Some(ParseResult {
                intent: ParsedIntent::SendWhatsAppMessage { contact, message },
                ..
            }) => {
                assert_eq!(contact, "mummy");
                assert_eq!(message, "hi");
            }
            other => panic!("expected SendWhatsAppMessage, got {:?}", other),
        }
    }

    #[test]
    fn test_send_message_to_contact_on_whatsapp() {
        let result = parse_deterministic("send hello to mom on whatsapp");
        match result {
            Some(ParseResult {
                intent: ParsedIntent::SendWhatsAppMessage { contact, message },
                ..
            }) => {
                assert_eq!(contact, "mom");
                assert_eq!(message, "hello");
            }
            other => panic!("expected SendWhatsAppMessage, got {:?}", other),
        }
    }

    #[test]
    fn test_can_you_send_message_to_contact_via_whatsapp() {
        let result = parse_deterministic("can you send on my way to dad via whatsapp");
        match result {
            Some(ParseResult {
                intent: ParsedIntent::SendWhatsAppMessage { contact, message },
                ..
            }) => {
                assert_eq!(contact, "dad");
                assert_eq!(message, "on my way");
            }
            other => panic!("expected SendWhatsAppMessage, got {:?}", other),
        }
    }

    #[test]
    fn test_send_whatsapp_message_to_contact_saying() {
        let result = parse_deterministic("send a whatsapp message to mummy saying hi");
        match result {
            Some(ParseResult {
                intent: ParsedIntent::SendWhatsAppMessage { contact, message },
                ..
            }) => {
                assert_eq!(contact, "mummy");
                assert_eq!(message, "hi");
            }
            other => panic!("expected SendWhatsAppMessage, got {:?}", other),
        }
    }

    #[test]
    fn test_send_whatsapp_to_contact_saying() {
        let result = parse_deterministic("send whatsapp to dad saying dinner is ready");
        match result {
            Some(ParseResult {
                intent: ParsedIntent::SendWhatsAppMessage { contact, message },
                ..
            }) => {
                assert_eq!(contact, "dad");
                assert_eq!(message, "dinner is ready");
            }
            other => panic!("expected SendWhatsAppMessage, got {:?}", other),
        }
    }

    #[test]
    fn test_send_message_on_whatsapp_to_contact() {
        let result = parse_deterministic("send hi on whatsapp to mummy");
        match result {
            Some(ParseResult {
                intent: ParsedIntent::SendWhatsAppMessage { contact, message },
                ..
            }) => {
                assert_eq!(contact, "mummy");
                assert_eq!(message, "hi");
            }
            other => panic!("expected SendWhatsAppMessage, got {:?}", other),
        }
    }

    #[test]
    fn test_send_a_message_on_whatsapp_empty_contact() {
        let result = parse_deterministic("send a message on whatsapp");
        assert!(result.is_some());
        match result.unwrap().intent {
            ParsedIntent::NeedMoreInfo { prompt } => {
                assert!(prompt.contains("Who should I message"), "prompt is {prompt}");
            }
            other => panic!("expected NeedMoreInfo, got {:?}", other),
        }
    }

    #[test]
    fn test_tell_message_to_mommy_with_period_separator() {
        let result = parse_deterministic("Tell message to mommy. Hi. All right.");
        match result {
            Some(ParseResult {
                intent: ParsedIntent::SendWhatsAppMessage { contact, message },
                ..
            }) => {
                assert_eq!(contact, "mommy");
                assert_eq!(message, "hi. all right");
            }
            other => panic!("expected SendWhatsAppMessage, got {:?}", other),
        }
    }

    #[test]
    fn test_tell_message_to_mommy_partial() {
        let result = parse_deterministic("Tell message to mommy");
        match result {
            Some(ParseResult {
                intent: ParsedIntent::NeedMoreInfo { prompt },
                ..
            }) => {
                assert_eq!(prompt, "What should I say to mommy?");
            }
            other => panic!("expected NeedMoreInfo, got {:?}", other),
        }
    }

    #[test]
    fn test_tell_mommy_that_clause() {
        let result = parse_deterministic("tell mommy that dinner is ready");
        match result {
            Some(ParseResult {
                intent: ParsedIntent::SendWhatsAppMessage { contact, message },
                ..
            }) => {
                assert_eq!(contact, "mommy");
                assert_eq!(message, "dinner is ready");
            }
            other => panic!("expected SendWhatsAppMessage, got {:?}", other),
        }
    }

    #[test]
    fn test_text_mommy_colon() {
        let result = parse_deterministic("text mommy: I'm on my way");
        match result {
            Some(ParseResult {
                intent: ParsedIntent::SendWhatsAppMessage { contact, message },
                ..
            }) => {
                assert_eq!(contact, "mommy");
                assert_eq!(message, "i'm on my way");
            }
            other => panic!("expected SendWhatsAppMessage, got {:?}", other),
        }
    }

    #[test]
    fn test_message_hi_to_mommy_in_whatsapp() {
        let result = parse_deterministic("Message Hi to mommy in WhatsApp.");
        match result {
            Some(ParseResult {
                intent: ParsedIntent::SendWhatsAppMessage { contact, message },
                ..
            }) => {
                assert_eq!(contact, "mommy");
                assert_eq!(message, "hi");
            }
            other => panic!("expected SendWhatsAppMessage, got {:?}", other),
        }
    }

    #[test]
    fn test_text_dinner_is_ready_to_dad_on_whatsapp() {
        let result = parse_deterministic("Text dinner is ready to dad on WhatsApp");
        match result {
            Some(ParseResult {
                intent: ParsedIntent::SendWhatsAppMessage { contact, message },
                ..
            }) => {
                assert_eq!(contact, "dad");
                assert_eq!(message, "dinner is ready");
            }
            other => panic!("expected SendWhatsAppMessage, got {:?}", other),
        }
    }

    #[test]
    fn test_tell_on_my_way_to_mummy_in_whatsapp() {
        let result = parse_deterministic("Tell I am on my way to mummy in whatsapp");
        match result {
            Some(ParseResult {
                intent: ParsedIntent::SendWhatsAppMessage { contact, message },
                ..
            }) => {
                assert_eq!(contact, "mummy");
                assert_eq!(message, "i am on my way");
            }
            other => panic!("expected SendWhatsAppMessage, got {:?}", other),
        }
    }

    #[test]
    fn test_message_contact_on_whatsapp_message() {
        let result = parse_deterministic("message mommy on whatsapp hi");
        match result {
            Some(ParseResult {
                intent: ParsedIntent::SendWhatsAppMessage { contact, message },
                ..
            }) => {
                assert_eq!(contact, "mommy");
                assert_eq!(message, "hi");
            }
            other => panic!("expected SendWhatsAppMessage, got {:?}", other),
        }
    }

    #[test]
    fn test_bare_message_still_opens_chat() {
        // No "send" verb → chat-open meaning preserved (not a partial send).
        let result = parse_deterministic("message mom");
        assert!(result.is_some());
        assert!(matches!(
            result.unwrap().intent,
            ParsedIntent::WhatsappChat { .. }
        ));
    }

    // ─── Routing ───────────────────────────────────────────────────────

    #[test]
    fn test_ghostwriter_entry_triggers() {
        for phrase in [
            "ghostwriter",
            "ghost writer",
            "ghost mode",
            "go ghost mode",
            "ghostwriter mode",
            "open ghostwriter",
            "open ghostwriter mode",
            "open the ghostwriter mode",
            "open ghost mode",
            "open the ghost mode",
            "take a letter",
            "write this down",
            "scribe",
            "scribe mode",
        ] {
            let result = parse_deterministic(phrase);
            assert!(result.is_some(), "{phrase} should enter Ghostwriter");
            assert!(matches!(
                result.unwrap().intent,
                ParsedIntent::EnterGhostwriter { .. }
            ));
        }
    }

    #[test]
    fn test_ghostwriter_entry_with_contact() {
        let result = parse_deterministic("ghostwriter for mom on whatsapp");
        assert!(result.is_some());
        if let ParsedIntent::EnterGhostwriter { contact } = result.unwrap().intent {
            assert_eq!(contact.as_deref(), Some("mom"));
        } else {
            panic!("expected EnterGhostwriter");
        }
    }

    // ─── Screen control ──────────────────────────────────────────

    #[test]
    fn test_click_ordinal_forms() {
        for (phrase, want) in [
            ("click on the 3rd option", 3u32),
            ("click the 2nd button", 2),
            ("press the 1st link", 1),
            ("choose 4th", 4),
            ("tap the 12th item", 12),
        ] {
            let result = parse_deterministic(phrase);
            assert!(result.is_some(), "{phrase} should parse");
            if let ParsedIntent::ScreenClick { ordinal } = result.unwrap().intent {
                assert_eq!(ordinal, want, "{phrase}");
            } else {
                panic!("expected ScreenClick for {phrase}");
            }
        }
    }

    #[test]
    fn test_browser_tab_forms() {
        for (phrase, want) in [
            ("move to the 4th tab", 4u32),
            ("go to 2nd tab", 2),
            ("switch to the 1st tab", 1),
            ("tab 3", 3),
        ] {
            let result = parse_deterministic(phrase);
            assert!(result.is_some(), "{phrase} should parse");
            if let ParsedIntent::BrowserTab { index } = result.unwrap().intent {
                assert_eq!(index, want, "{phrase}");
            } else {
                panic!("expected BrowserTab for {phrase}");
            }
        }
    }

    #[test]
    fn test_screen_read_form() {
        let result = parse_deterministic("what's the 2nd button");
        assert!(result.is_some());
        assert!(matches!(
            result.unwrap().intent,
            ParsedIntent::ScreenRead { ordinal: 2 }
        ));
    }

    #[test]
    fn test_new_intents_have_labels() {
        assert_eq!(
            intent_to_label(&ParsedIntent::OrderFood {
                query: "pizza".to_string(),
                restaurant: None
            }),
            "order_food"
        );
        assert_eq!(
            intent_to_label(&ParsedIntent::SearchProduct {
                query: "laptop".to_string()
            }),
            "search_product"
        );
        assert_eq!(
            intent_to_label(&ParsedIntent::SendWhatsAppMessage {
                contact: "mom".to_string(),
                message: "hi".to_string()
            }),
            "send_whatsapp_message"
        );
    }
}
