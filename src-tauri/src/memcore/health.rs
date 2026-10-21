//! Boot health report (plan D): after startup NEXUS checks the connections
//! the user actually relies on and, in the same breath as the greeting, says
//! what is wrong - once.
//!
//! * `decide` is pure: it turns raw check results into "things to say", with
//!   three rules - only checks that matter to THIS user count (a WhatsApp
//!   bridge that was never switched on is not a problem), only new or changed
//!   problems are spoken (the same outage is not announced every boot), and at
//!   most three are spoken (the full list goes on the briefing card).
//! * `collect` is the thin async part with a hard time budget. It reads, it
//!   never fixes: no keys are changed, nothing is restarted.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// Spoken at most this many problems per boot.
pub const MAX_SPOKEN: usize = 3;
/// The whole collection must finish within this long.
pub const BUDGET_SECS: u64 = 12;
const META_ANNOUNCED: &str = "health_announced";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Check {
    /// Stable id ("whatsapp_bridge").
    pub key: &'static str,
    pub ok: bool,
    /// Whether this check matters to this user (feature switched on / was set up).
    pub relevant: bool,
    /// Spoken sentence fragment when it fails ("the WhatsApp bridge is not running").
    pub spoken: String,
    /// Longer card line.
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Problem {
    pub key: String,
    pub spoken: String,
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Report {
    /// New or changed problems, at most `MAX_SPOKEN`.
    pub to_speak: Vec<Problem>,
    /// Every current problem (for the card).
    pub all: Vec<Problem>,
}

/// `announced` = problem key -> the spoken text last announced. Pure.
pub fn decide(checks: &[Check], announced: &BTreeMap<String, String>) -> Report {
    let all: Vec<Problem> = checks
        .iter()
        .filter(|c| c.relevant && !c.ok)
        .map(|c| Problem { key: c.key.to_string(), spoken: c.spoken.clone(), detail: c.detail.clone() })
        .collect();
    let to_speak: Vec<Problem> = all
        .iter()
        .filter(|p| announced.get(&p.key).map(|old| old != &p.spoken).unwrap_or(true))
        .take(MAX_SPOKEN)
        .cloned()
        .collect();
    Report { to_speak, all }
}

/// "Heads up: the WhatsApp bridge is not running and Google needs you to sign in again." Pure.
pub fn spoken_line(problems: &[Problem]) -> Option<String> {
    let parts: Vec<&str> = problems.iter().map(|p| p.spoken.as_str()).collect();
    let joined = match parts.len() {
        0 => return None,
        1 => parts[0].to_string(),
        2 => format!("{} and {}", parts[0], parts[1]),
        n => format!("{}, and {}", parts[..n - 1].join(", "), parts[n - 1]),
    };
    Some(format!("Heads up: {joined}."))
}

pub fn card_section(all: &[Problem]) -> Option<String> {
    if all.is_empty() {
        return None;
    }
    let mut md = String::from("### Connections\n");
    for p in all {
        md.push_str(&format!("- {}\n", p.detail));
    }
    md.push('\n');
    Some(md)
}

// ─── Persistence of what was announced ───────────────────────────────

pub fn load_announced(dir: &std::path::Path) -> BTreeMap<String, String> {
    super::with_store(dir, |s| s.meta_get(META_ANNOUNCED))
        .flatten()
        .and_then(|v| serde_json::from_str(&v).ok())
        .unwrap_or_default()
}

/// Remember what is currently wrong (problems that cleared are forgotten, so a
/// later recurrence is announced again).
pub fn save_announced(dir: &std::path::Path, all: &[Problem]) {
    let map: BTreeMap<&str, &str> = all.iter().map(|p| (p.key.as_str(), p.spoken.as_str())).collect();
    if let Ok(v) = serde_json::to_string(&map) {
        super::with_store(dir, |s| s.meta_set(META_ANNOUNCED, &v));
    }
}

// ─── Collection ──────────────────────────────────────────────────────

fn ok_check(key: &'static str) -> Check {
    Check { key, ok: true, relevant: true, spoken: String::new(), detail: String::new() }
}

/// Run every check within the time budget. Never panics, never fixes anything.
pub async fn collect<R: tauri::Runtime>(app: &tauri::AppHandle<R>, dir: &std::path::Path) -> Vec<Check> {
    match tokio::time::timeout(std::time::Duration::from_secs(BUDGET_SECS), collect_inner(app, dir)).await {
        Ok(c) => c,
        Err(_) => {
            tracing::warn!("health: collection exceeded {BUDGET_SECS}s - reporting nothing");
            vec![]
        }
    }
}

async fn collect_inner<R: tauri::Runtime>(app: &tauri::AppHandle<R>, dir: &std::path::Path) -> Vec<Check> {
    let mut out: Vec<Check> = vec![];

    // 1. Internet. Offline explains most cloud failures, so the cloud checks are skipped then.
    let online = crate::tts_network::check_network().await;
    out.push(Check {
        key: "internet",
        ok: online,
        relevant: true,
        spoken: "I'm offline, so cloud answers and online voices won't work".into(),
        detail: "No internet connection - cloud answers and online voices are unavailable.".into(),
    });

    // 2. NEXUS cloud service (Worker).
    if online {
        let url = crate::commands::get_server_config(app.clone()).map(|c| c.server_url).unwrap_or_default();
        let reachable = if url.is_empty() {
            false
        } else {
            let client = reqwest::Client::builder().timeout(std::time::Duration::from_secs(5)).build();
            match client {
                Ok(c) => c
                    .get(format!("{}/health", url.trim_end_matches('/')))
                    .send()
                    .await
                    .map(|r| r.status().is_success() || r.status().is_redirection())
                    .unwrap_or(false),
                Err(_) => false,
            }
        };
        out.push(Check {
            key: "worker",
            ok: reachable,
            relevant: true,
            spoken: "my cloud service isn't answering".into(),
            detail: "The NEXUS cloud service did not answer its health check.".into(),
        });
    }

    // 3. Speech recognition key: without it NEXUS falls back to the slower local model.
    let local_only = crate::commands::read_local_stt_only(app);
    let groq_missing = crate::commands::read_api_key(app, "groq").is_empty();
    out.push(Check {
        key: "groq_key",
        ok: !groq_missing,
        relevant: !local_only,
        spoken: "there's no Groq key, so speech recognition will be slower".into(),
        detail: "No Groq API key - speech recognition falls back to the slower local model (Command Hub, API Keys).".into(),
    });

    // 4. Google: only matters once an account was connected and has since expired.
    let needs_signin = super::with_store(dir, |s| s.meta_get("mail_needs_signin").map(|v| !v.is_empty()).unwrap_or(false))
        .unwrap_or(false);
    out.push(Check {
        key: "google_signin",
        ok: !needs_signin,
        relevant: super::flag(dir, "memcoreMail", true),
        spoken: "Google needs you to sign in again".into(),
        detail: "Google sign-in expired - mail and calendar are paused until you sign in again (Command Hub, Connections).".into(),
    });

    // 5. WhatsApp bridge: only if the user switched the WhatsApp watcher on.
    if super::flag(dir, "memcoreWhatsapp", false) {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(4))
            .connect_timeout(std::time::Duration::from_secs(3))
            .build();
        if let Ok(client) = client {
            use crate::mcp_client::{McpConnectState, McpServer};
            let card = crate::mcp_client::connect_card_for(McpServer::WhatsApp, &client).await;
            let (ok, spoken, detail) = match card.state {
                McpConnectState::Ready => (true, String::new(), String::new()),
                McpConnectState::AuthRequired => (
                    false,
                    "WhatsApp needs to be paired again".to_string(),
                    "WhatsApp bridge is running but the phone is not paired.".to_string(),
                ),
                _ => (
                    false,
                    "the WhatsApp bridge isn't running".to_string(),
                    "WhatsApp bridge is not running on port 8765.".to_string(),
                ),
            };
            out.push(Check { key: "whatsapp_bridge", ok, relevant: true, spoken, detail });
        }
    }

    out
}

/// Collect, decide and remember what was announced. Returns the report.
pub async fn run<R: tauri::Runtime>(app: &tauri::AppHandle<R>, dir: &std::path::Path) -> Report {
    let checks = collect(app, dir).await;
    if checks.is_empty() {
        return Report::default();
    }
    let announced = load_announced(dir);
    let report = decide(&checks, &announced);
    save_announced(dir, &report.all);
    for p in &report.all {
        println!("[HEALTH] problem: {}", p.detail);
    }
    if report.all.is_empty() {
        println!("[HEALTH] all relied-on connections look fine ({} checked)", checks.len());
    }
    let _ = ok_check; // kept for tests
    report
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bad(key: &'static str, relevant: bool, spoken: &str) -> Check {
        Check { key, ok: false, relevant, spoken: spoken.into(), detail: format!("{spoken} (detail)") }
    }

    #[test]
    fn irrelevant_and_healthy_checks_never_speak() {
        let checks = vec![ok_check("worker"), bad("whatsapp_bridge", false, "the WhatsApp bridge isn't running")];
        let r = decide(&checks, &BTreeMap::new());
        assert!(r.all.is_empty() && r.to_speak.is_empty());
        assert_eq!(spoken_line(&r.to_speak), None);
    }

    #[test]
    fn only_new_or_changed_problems_are_spoken() {
        let checks = vec![bad("worker", true, "my cloud service isn't answering"), bad("groq_key", true, "no Groq key")];
        let mut announced = BTreeMap::new();
        announced.insert("worker".to_string(), "my cloud service isn't answering".to_string());
        let r = decide(&checks, &announced);
        assert_eq!(r.all.len(), 2, "the card still lists everything");
        assert_eq!(r.to_speak.iter().map(|p| p.key.as_str()).collect::<Vec<_>>(), vec!["groq_key"]);

        // The same problem with different wording (state changed) is announced again.
        announced.insert("worker".to_string(), "something older".to_string());
        assert_eq!(decide(&checks, &announced).to_speak.len(), 2);
    }

    #[test]
    fn at_most_three_are_spoken_and_the_line_reads_naturally() {
        let checks: Vec<Check> = ["a", "b", "c", "d"].iter().map(|k| bad(k, true, &format!("{k} is down"))).collect();
        let r = decide(&checks, &BTreeMap::new());
        assert_eq!(r.all.len(), 4);
        assert_eq!(r.to_speak.len(), MAX_SPOKEN);
        assert_eq!(spoken_line(&r.to_speak).unwrap(), "Heads up: a is down, b is down, and c is down.");
        assert_eq!(spoken_line(&r.to_speak[..1]).unwrap(), "Heads up: a is down.");
        assert_eq!(spoken_line(&r.to_speak[..2]).unwrap(), "Heads up: a is down and b is down.");
    }

    #[test]
    fn card_lists_every_problem() {
        let r = decide(&[bad("a", true, "a is down"), bad("b", true, "b is down")], &BTreeMap::new());
        let md = card_section(&r.all).unwrap();
        assert!(md.starts_with("### Connections") && md.contains("a is down (detail)") && md.contains("b is down (detail)"));
        assert!(card_section(&[]).is_none());
    }

    #[test]
    fn cleared_problems_are_forgotten_so_a_recurrence_is_announced() {
        let dir = std::env::temp_dir().join(format!("nexus_health_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let p = Problem { key: "worker".into(), spoken: "down".into(), detail: "d".into() };
        save_announced(&dir, &[p.clone()]);
        assert_eq!(load_announced(&dir).get("worker").map(String::as_str), Some("down"));
        save_announced(&dir, &[]);
        assert!(load_announced(&dir).is_empty());
        let again = decide(&[bad("worker", true, "down")], &load_announced(&dir));
        assert_eq!(again.to_speak.len(), 1);
    }
}
