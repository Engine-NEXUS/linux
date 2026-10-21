//! Declarative agent specs (D2): user-defined intents in YAML.
//!
//! `%APPDATA%/com.nexus.assistant/intents.yaml`:
//! ```yaml
//! version: 1
//! intents:
//!   - name: movie_night
//!     phrases: ["movie night", "it's movie time"]
//!     run:
//!       - open: "Plex"
//!       - say: "Enjoy the movie, sir."
//! ```
//!
//! Priority: built-in deterministic parse wins; specs fill the fallback
//! slot (checked on Unknown, before NLU/Worker). Actions: `open` (app
//! registry) + `say` (spoken reply). Nothing else — no shell, no clicks,
//! no sends. Validation caps keep a bad file from doing damage.

/// One spec action.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SpecAction {
    Open(String),
    Say(String),
}

/// One user-defined intent.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct CustomIntent {
    pub name: String,
    #[serde(default)]
    pub phrases: Vec<String>,
    #[serde(default)]
    pub run: Vec<SpecAction>,
}

#[derive(Debug, serde::Deserialize)]
struct SpecFile {
    #[serde(default)]
    version: u32,
    #[serde(default)]
    intents: Vec<RawIntent>,
}

/// Raw file form: run steps are single-key maps (`{open: X}`).
/// Converted strictly (unknown actions rejected) in load_specs.
#[derive(Debug, serde::Deserialize)]
struct RawIntent {
    #[serde(default)]
    name: String,
    #[serde(default)]
    phrases: Vec<String>,
    #[serde(default)]
    run: Vec<std::collections::HashMap<String, String>>,
}

/// Convert one raw action map to a SpecAction. Strict: exactly one entry,
/// key must be open/say (case-insensitive), value non-empty.
fn parse_action(map: &std::collections::HashMap<String, String>) -> Option<SpecAction> {
    if map.len() != 1 {
        return None;
    }
    let (k, v) = map.iter().next()?;
    let v = v.trim();
    if v.is_empty() {
        return None;
    }
    match k.trim().to_lowercase().as_str() {
        "open" => Some(SpecAction::Open(v.to_string())),
        "say" => Some(SpecAction::Say(v.to_string())),
        _ => None,
    }
}

pub const SPECS_FILE: &str = "intents.yaml";
const MAX_INTENTS: usize = 50;
const MAX_PHRASES: usize = 20;
const MAX_PHRASE_LEN: usize = 100;

/// Load + validate specs. Missing file → empty (not an error).
/// Invalid YAML → empty + warn (never crash the pipeline).
pub fn load_specs(app_data_dir: &std::path::Path) -> Vec<CustomIntent> {
    let path = app_data_dir.join(SPECS_FILE);
    let Ok(content) = std::fs::read_to_string(&path) else {
        return vec![];
    };
    let file: SpecFile = match serde_yaml::from_str(&content) {
        Ok(f) => f,
        Err(e) => {
            tracing::warn!("specs: invalid {}: {}", SPECS_FILE, e);
            return vec![];
        }
    };
    if file.version != 1 {
        tracing::warn!("specs: unsupported version {}, want 1", file.version);
        return vec![];
    }
    let mut out = vec![];
    for intent in file.intents.into_iter().take(MAX_INTENTS) {
        if intent.name.trim().is_empty() {
            continue;
        }
        let phrases: Vec<String> = intent
            .phrases
            .into_iter()
            .map(|p| p.trim().to_string())
            .filter(|p| !p.is_empty() && p.len() <= MAX_PHRASE_LEN)
            .take(MAX_PHRASES)
            .collect();
        if phrases.is_empty() {
            continue;
        }
        let run: Vec<SpecAction> = intent
            .run
            .iter()
            .filter_map(parse_action)
            .collect();
        if run.is_empty() {
            continue;
        }
        out.push(CustomIntent {
            name: intent.name.trim().to_string(),
            phrases,
            run,
        });
    }
    out
}

fn normalize(s: &str) -> String {
    s.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

/// Match a transcript against specs: exact (longest first), then contains.
/// Pure + unit-tested.
pub fn match_spec<'a>(specs: &'a [CustomIntent], transcript: &str) -> Option<&'a CustomIntent> {
    let t = normalize(transcript);
    if t.is_empty() {
        return None;
    }
    // Exact matches, longest phrase first (most specific wins).
    let mut best: Option<&'a CustomIntent> = None;
    let mut best_len = 0;
    for intent in specs {
        for phrase in &intent.phrases {
            let p = normalize(phrase);
            if p == t && p.len() > best_len {
                best_len = p.len();
                best = Some(intent);
            }
        }
    }
    if best.is_some() {
        return best;
    }
    // Substring fallback: longest containing phrase wins.
    best_len = 0;
    for intent in specs {
        for phrase in &intent.phrases {
            let p = normalize(phrase);
            if p.len() > 3 && t.contains(&p) && p.len() > best_len {
                best_len = p.len();
                best = Some(intent);
            }
        }
    }
    best
}

/// Write an example specs file if none exists (first-run discoverability).
pub fn ensure_example(app_data_dir: &std::path::Path) {
    let path = app_data_dir.join(SPECS_FILE);
    if path.exists() {
        return;
    }
    let example = r#"# NEXUS custom intents (D2) — your phrases, your actions.
# Checked when built-in parsing misses, before NLU/Worker.
# Actions: open (app name) + say (spoken reply). Nothing else.
version: 1
intents:
  - name: movie_night
    phrases: ["movie night", "it's movie time"]
    run:
      - open: "Plex"
      - say: "Enjoy the movie, sir."
"#;
    let _ = std::fs::write(&path, example);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmpdir(name: &str) -> std::path::PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!("nexus_specs_test_{}_{}", name, std::process::id()));
        let _ = std::fs::create_dir_all(&p);
        p
    }

    const VALID: &str = r#"
version: 1
intents:
  - name: movie_night
    phrases: ["movie night", "it's movie time"]
    run:
      - open: "Plex"
      - say: "Enjoy."
  - name: focus
    phrases: ["focus time"]
    run:
      - say: "Locked in."
"#;

    #[test]
    fn test_load_valid() {
        let d = tmpdir("valid");
        std::fs::write(d.join(SPECS_FILE), VALID).unwrap();
        let specs = load_specs(&d);
        assert_eq!(specs.len(), 2);
        assert_eq!(specs[0].name, "movie_night");
        assert_eq!(specs[0].run.len(), 2);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn test_load_missing_is_empty() {
        let d = tmpdir("missing");
        assert!(load_specs(&d).is_empty());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn test_load_invalid_yaml_is_empty() {
        let d = tmpdir("invalid");
        std::fs::write(d.join(SPECS_FILE), "intents: [unclosed").unwrap();
        assert!(load_specs(&d).is_empty());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn test_load_wrong_version_is_empty() {
        let d = tmpdir("version");
        std::fs::write(d.join(SPECS_FILE), "version: 99\nintents: []").unwrap();
        assert!(load_specs(&d).is_empty());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn test_load_skips_empties() {
        let d = tmpdir("empty");
        let yml = "version: 1\nintents:\n  - name: ''\n    phrases: ['x']\n    run:\n      - say: 'y'\n  - name: ok\n    phrases: []\n    run:\n      - say: 'y'\n  - name: good\n    phrases: ['go']\n    run:\n      - say: 'y'\n";
        std::fs::write(d.join(SPECS_FILE), yml).unwrap();
        let specs = load_specs(&d);
        assert_eq!(specs.len(), 1);
        assert_eq!(specs[0].name, "good");
        let _ = std::fs::remove_dir_all(&d);
    }

    fn sample_specs() -> Vec<CustomIntent> {
        vec![
            CustomIntent {
                name: "movie_night".into(),
                phrases: vec!["movie night".into(), "it's movie time".into()],
                run: vec![SpecAction::Say("Enjoy.".into())],
            },
            CustomIntent {
                name: "focus".into(),
                phrases: vec!["focus time".into()],
                run: vec![SpecAction::Say("Locked in.".into())],
            },
        ]
    }

    #[test]
    fn test_match_exact() {
        let specs = sample_specs();
        assert_eq!(match_spec(&specs, "movie night").unwrap().name, "movie_night");
        assert_eq!(match_spec(&specs, "MOVIE NIGHT!").unwrap().name, "movie_night");
    }

    #[test]
    fn test_match_contains() {
        let specs = sample_specs();
        assert_eq!(
            match_spec(&specs, "it's movie time please").unwrap().name,
            "movie_night"
        );
    }

    #[test]
    fn test_match_longest_wins() {
        let specs = vec![
            CustomIntent {
                name: "short".into(),
                phrases: vec!["movie".into()],
                run: vec![SpecAction::Say("s".into())],
            },
            CustomIntent {
                name: "long".into(),
                phrases: vec!["movie night".into()],
                run: vec![SpecAction::Say("l".into())],
            },
        ];
        assert_eq!(match_spec(&specs, "movie night").unwrap().name, "long");
    }

    #[test]
    fn test_match_miss() {
        let specs = sample_specs();
        assert!(match_spec(&specs, "open chrome").is_none());
        assert!(match_spec(&specs, "").is_none());
    }

    #[test]
    fn test_parse_action_strict() {
        use std::collections::HashMap;
        let mut ok = HashMap::new();
        ok.insert("open".to_string(), "Plex".to_string());
        assert_eq!(parse_action(&ok), Some(SpecAction::Open("Plex".into())));
        let mut ci = HashMap::new();
        ci.insert("SAY".to_string(), "hi".to_string());
        assert_eq!(parse_action(&ci), Some(SpecAction::Say("hi".into())));
        let mut bad = HashMap::new();
        bad.insert("shell".to_string(), "rm -rf /".to_string());
        assert_eq!(parse_action(&bad), None);
        let mut multi = HashMap::new();
        multi.insert("open".to_string(), "x".to_string());
        multi.insert("say".to_string(), "y".to_string());
        assert_eq!(parse_action(&multi), None);
        let mut empty = HashMap::new();
        empty.insert("say".to_string(), "  ".to_string());
        assert_eq!(parse_action(&empty), None);
    }

    #[test]
    fn test_ensure_example() {
        let d = tmpdir("example");
        ensure_example(&d);
        assert!(d.join(SPECS_FILE).exists());
        // Second call never overwrites.
        std::fs::write(d.join(SPECS_FILE), "custom").unwrap();
        ensure_example(&d);
        assert_eq!(std::fs::read_to_string(d.join(SPECS_FILE)).unwrap(), "custom");
        let _ = std::fs::remove_dir_all(&d);
    }
}
