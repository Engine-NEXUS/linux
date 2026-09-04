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
    // faster-whisper appends sentence punctuation ("hello." / "open youtube?").
    // Anchored intent regexes (^...$) reject it, so "hello." fell through to
    // unknown → backend → silence. Strip once here instead of per-regex.
    let text = text.trim_end_matches(|c: char| ".?!…,".contains(c));
    let text = normalize_whitespace(&text);

    if text.is_empty() {
        return None;
    }

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

    // --- WhatsApp chat (must be BEFORE open command ΓÇö "open chat with X" would match open) ---
    // "open chat with lakshya", "message lakshya on whatsapp", "chat with mom"
    if let Some(result) = parse_whatsapp_command(&text) {
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
        // Return the first search name (canonical form)
        if let Some(canonical) = entry.search_names.first() {
            tracing::debug!(
                "app registry match: '{}' ΓåÆ '{}' ({})",
                name,
                canonical,
                entry.display_name
            );
            return Some(canonical.clone());
        }
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
            if let Some(canonical) = entry.search_names.first() {
                return Some(canonical.clone());
            }
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
                if let Some(canonical) = entry.search_names.first() {
                    return Some(canonical.clone());
                }
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
    let pr_patterns = [
        // "PR number 24 on NEXUS agent" / "PR number 24 in repo"
        regex::Regex::new(r"^pr\s*(?:number|#\s*)?\s*#?\s*(\d+)\s+(?:in|of|for|from|on)\s+(.+)$").ok()?,
        // "PR number 24 NEXUS agent" (no preposition)
        regex::Regex::new(r"^pr\s*number\s*#?\s*(\d+)\s+(.+)$").ok()?,
        regex::Regex::new(r"^pr\s*#?\s*(\d+)\s+(?:in|of|for|from)\s+(.+)$").ok()?,
        regex::Regex::new(r"^pr\s*#?\s*(\d+)\s+(.+)$").ok()?,
        regex::Regex::new(r"^pull\s+request\s*#?\s*(\d+)\s+(?:in|of|for|from|on)\s+(.+)$").ok()?,
        regex::Regex::new(r"^pull\s+request\s*#?\s*(\d+)\s+(.+)$").ok()?,
        // "PR <num> owner/repo"
        regex::Regex::new(r"^pr\s*#?\s*(\d+)\s+(\S+/\S+)$").ok()?,
        // "the PR <num> in <repo>" ΓÇö user says "analyse the pr 254 in zync"
        regex::Regex::new(r"^the\s+pr\s*#?\s*(\d+)\s+(?:in|of|for|from|on)\s+(.+)$").ok()?,
        // "the PR <num> <repo>" (no preposition)
        regex::Regex::new(r"^the\s+pr\s*#?\s*(\d+)\s+(.+)$").ok()?,
        // "the pull request <num> in <repo>"
        regex::Regex::new(r"^the\s+pull\s+request\s*#?\s*(\d+)\s+(?:in|of|for|from|on)\s+(.+)$").ok()?,
    ];

    for pat in &pr_patterns {
        if let Some(caps) = pat.captures(text) {
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
        // Threshold: 2 for short repos (Γëñ6 chars), 3 for longer
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
    let author_repo_pat = regex::Regex::new(
        r"^(?:of|by|from)\s+(\S+)\s+in\s+(.+)$"
    ).ok()?;

    if let Some(caps) = author_repo_pat.captures(after_pr) {
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
    let in_repo_pat = regex::Regex::new(r"^in\s+(.+)$").ok()?;
    if let Some(caps) = in_repo_pat.captures(after_pr) {
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
    let of_repo_pat = regex::Regex::new(r"^of\s+(.+)$").ok()?;
    if let Some(caps) = of_repo_pat.captures(after_pr) {
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
    let repo_author_pat_a = regex::Regex::new(
        r"^(?:of|in)\s+(.+?)\s+by\s+(\S+)$"
    ).ok()?;

    if let Some(caps) = repo_author_pat_a.captures(&after_branch) {
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
    let author_repo_pat_b = regex::Regex::new(
        r"^by\s+(\S+)\s+(?:in|of)\s+(.+)$"
    ).ok()?;

    if let Some(caps) = author_repo_pat_b.captures(&after_branch) {
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
    let repo_only_pat = regex::Regex::new(r"^(?:of|in)\s+(.+)$").ok()?;
    if let Some(caps) = repo_only_pat.captures(&after_branch) {
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
    text.trim().to_string()
}

// ΓöÇΓöÇΓöÇ Close app command ΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇ

const CLOSE_VERBS: &[&str] = &["close", "quit", "exit", "kill", "shut down", "shut"];

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
    if regex_match(text, r"^(?:hello|hi|hey|yo|sup|what'?s\s+up|howdy|greetings|hi\s+ya|hiya|hey\s+(?:there|nexus)|hello\s+nexus|hi\s+nexus)$") {
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
    if regex_match(text, r"^(?:stop\s+music|stop\s+media|stop\s+playback)$") {
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
    let patterns = [
        regex::Regex::new(r"^(.+?)\s+in\s+(?:the\s+)?browser$").ok()?,
        regex::Regex::new(r"^(.+?)\s+website$").ok()?,
        regex::Regex::new(r"^(.+?)\s+site$").ok()?,
        regex::Regex::new(r"^(.+?)\s+on\s+(?:the\s+)?web$").ok()?,
        regex::Regex::new(r"^(.+?)\s+web\s+version$").ok()?,
    ];

    for pat in &patterns {
        if let Some(caps) = pat.captures(text) {
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

    // 2. Try NLU server (if available)
    if let Some(result) = crate::nlu_client::parse_via_nlu(&transcript).await {
        tracing::info!(
            "[intent_parser] nlu: {:?} (confidence={})",
            result.intent,
            result.confidence
        );
        return Ok(result);
    }

    // 3. Fallback: unknown
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
        // Fuzzy matches should have lower confidence (0.8) than exact (1.0)
        let result = parse_deterministic("analyse pr 254 in zink");
        assert!(result.is_some());
        let r = result.unwrap();
        if let ParsedIntent::AnalysePr { .. } = r.intent {
            assert_eq!(r.confidence, 0.8);
            assert_eq!(r.source, "fuzzy");
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
    fn test_greeting_stt_punctuation() {
        // faster-whisper appends sentence punctuation ("Hello."). The intent
        // must still parse — regression test for the silent-hello bug.
        for t in ["hello.", "Hello.", "hi?", "open youtube.", "thanks!"] {
            assert!(parse_deterministic(t).is_some(), "failed on {t:?}");
        }
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
}
