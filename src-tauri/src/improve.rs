//! Edge-case miner (D3): safe self-improvement without auto-training.
//!
//! Reads missed-intent logs + diary failure events, clusters repeated
//! misses by normalized text, and writes `suggested_phrases.json` — a
//! ranked list for the admin to promote into training data or intents.yaml.
//!
//! Safety invariant: record + suggest ONLY. Nothing here retrains models,
//! writes specs, or acts. BabyAGI-style self-building is explicitly out
//! of scope (confirmation gates stay human-owned).

use std::collections::HashMap;
use std::path::Path;

/// One suggestion cluster.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PhraseCluster {
    /// Canonical example (first-seen raw transcript).
    pub example: String,
    /// Normalized cluster key.
    pub key: String,
    /// How many times seen.
    pub count: usize,
    /// Where seen: orchestrator / stt_learning / diary.
    pub sources: Vec<String>,
}

pub const SUGGESTIONS_FILE: &str = "suggested_phrases.json";
/// Only clusters with at least this many hits are suggested.
pub const MIN_CLUSTER: usize = 2;
/// Weekly SLA slice: the top-N clusters by count are the promotion
/// candidates for training data / intents.yaml. `improvement_report`
/// returns the full ranked list; this is the "what to promote this
/// week" view. Clusters are pre-sorted by run_miner (count desc).
#[allow(dead_code)]
pub const WEEKLY_TOP_N: usize = 10;

/// Top-N promotion candidates. Pure + unit-tested.
pub fn top_suggestions(clusters: &[PhraseCluster], n: usize) -> Vec<PhraseCluster> {
    clusters.iter().take(n).cloned().collect()
}
/// Cap suggestions file size.
const MAX_CLUSTERS: usize = 100;

/// Normalize for clustering: lowercase, strip punctuation/fillers.
pub fn normalize_cluster_key(transcript: &str) -> String {
    const FILLERS: &[&str] = &["please", "sir", "hey", "okay", "ok", "uh", "um", "the", "a", "an"];
    transcript
        .to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty() && !FILLERS.contains(w))
        .collect::<Vec<_>>()
        .join(" ")
}

/// Cluster raw transcripts. Pure + unit-tested.
pub fn cluster_transcripts(items: &[(String, String)]) -> Vec<PhraseCluster> {
    let mut map: HashMap<String, (String, usize, Vec<String>)> = HashMap::new();
    for (transcript, source) in items {
        let key = normalize_cluster_key(transcript);
        if key.len() < 4 {
            continue;
        }
        let entry = map
            .entry(key.clone())
            .or_insert_with(|| (transcript.clone(), 0, vec![]));
        entry.1 += 1;
        if !entry.2.contains(source) {
            entry.2.push(source.clone());
        }
    }
    let mut out: Vec<PhraseCluster> = map
        .into_iter()
        .filter(|(_, (_, count, _))| *count >= MIN_CLUSTER)
        .map(|(key, (example, count, sources))| PhraseCluster {
            example,
            key,
            count,
            sources,
        })
        .collect();
    out.sort_by(|a, b| b.count.cmp(&a.count).then(a.key.cmp(&b.key)));
    out.truncate(MAX_CLUSTERS);
    out
}

/// Run the miner: missed intents + diary failures → suggestions file.
/// Returns the cluster list. Best-effort, never errors the caller.
pub fn run_miner(app_data_dir: &Path) -> Vec<PhraseCluster> {
    let mut items: Vec<(String, String)> = vec![];

    for rec in crate::missed_intent_logger::read_recent_missed_intents(500) {
        if !rec.transcript.trim().is_empty() {
            items.push((rec.transcript, format!("missed:{}", rec.source)));
        }
    }
    for ev in crate::diary::recent_events(app_data_dir, 500) {
        let kind = ev.get("kind").and_then(|k| k.as_str()).unwrap_or("");
        if matches!(kind, "mcp_failed" | "compound_failed") {
            if let Some(t) = ev.get("text").and_then(|t| t.as_str()) {
                items.push((t.to_string(), format!("diary:{kind}")));
            }
        }
    }

    let clusters = cluster_transcripts(&items);
    let path = app_data_dir.join(SUGGESTIONS_FILE);
    if let Ok(s) = serde_json::to_string_pretty(&clusters) {
        let _ = std::fs::write(&path, s);
    }
    tracing::info!(
        "improve: mined {} suggestion cluster(s) from {} events",
        clusters.len(),
        items.len()
    );
    clusters
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_strips_fillers() {
        assert_eq!(normalize_cluster_key("Hey NEXUS, please open the chrome sir"), "nexus open chrome");
    }

    #[test]
    fn test_normalize_short() {
        assert_eq!(normalize_cluster_key("hi"), "hi");
    }

    #[test]
    fn test_cluster_groups_variants() {
        let items = vec![
            ("movie night".to_string(), "a".to_string()),
            ("Movie Night please".to_string(), "a".to_string()),
            ("open chrome".to_string(), "b".to_string()),
        ];
        let clusters = cluster_transcripts(&items);
        assert_eq!(clusters.len(), 1);
        assert_eq!(clusters[0].key, "movie night");
        assert_eq!(clusters[0].count, 2);
    }

    #[test]
    fn test_cluster_min_threshold() {
        let items = vec![("lonely phrase here".to_string(), "a".to_string())];
        assert!(cluster_transcripts(&items).is_empty());
    }

    #[test]
    fn test_cluster_sources_merged() {
        let items = vec![
            ("play some music now".to_string(), "a".to_string()),
            ("play some music now".to_string(), "b".to_string()),
        ];
        let clusters = cluster_transcripts(&items);
        assert_eq!(clusters.len(), 1);
        assert_eq!(clusters[0].sources.len(), 2);
    }
    #[test]
    fn test_cluster_sorted_by_count() {
        let items = vec![
            ("alpha beta gamma".to_string(), "a".to_string()),
            ("alpha beta gamma".to_string(), "a".to_string()),
            ("alpha beta gamma".to_string(), "a".to_string()),
            ("delta epsilon zeta".to_string(), "a".to_string()),
            ("delta epsilon zeta".to_string(), "a".to_string()),
        ];
        let clusters = cluster_transcripts(&items);
        assert_eq!(clusters.len(), 2);
        assert_eq!(clusters[0].count, 3);
        assert_eq!(clusters[1].count, 2);
    }

    #[test]
    fn test_top_suggestions_weekly_slice() {
        let items = vec![
            ("alpha beta gamma".to_string(), "a".to_string()),
            ("alpha beta gamma".to_string(), "a".to_string()),
            ("alpha beta gamma".to_string(), "a".to_string()),
            ("delta epsilon zeta".to_string(), "a".to_string()),
            ("delta epsilon zeta".to_string(), "a".to_string()),
        ];
        let clusters = cluster_transcripts(&items);
        let top = top_suggestions(&clusters, 1);
        assert_eq!(top.len(), 1);
        assert_eq!(top[0].key, "alpha beta gamma");
        assert_eq!(top[0].count, 3);
        // n larger than the list returns everything, never panics.
        assert_eq!(top_suggestions(&clusters, WEEKLY_TOP_N).len(), 2);
        assert!(top_suggestions(&[], 5).is_empty());
    }
}
