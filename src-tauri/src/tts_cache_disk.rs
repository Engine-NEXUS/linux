//! Persistent voice cache: short spoken lines rendered ONCE and kept on disk, so most lines
//! ("Ok sir.", "New tab sir.", "Hand mode on, sir.", the greetings, "Good night, sir.") are played
//! from a file with nothing synthesised at run time.
//!
//! Before this the acknowledgement cache was an in-memory map rebuilt on EVERY start (42-51 s of
//! network/CPU while the user was logging in, mostly 403s followed by a local re-render).
//!
//! Layout: `<app data>/tts_cache/<voice>__<origin>__<hash>.wav` (16-bit mono) plus a `.json` sidecar
//! with the original text and the word timings for the caption. `origin` is `edge` (cloud voice with
//! real word boundaries) or `local` (the offline voice, estimated timings); a lookup prefers `edge`.
//! Files are bounded (`MAX_ENTRIES`, oldest evicted) and never contain anything but short stock lines.

use std::path::{Path, PathBuf};

pub const MAX_ENTRIES: usize = 200;
/// Only short lines are persisted: stock acknowledgements, not replies (which can contain private text).
pub const MAX_WORDS: usize = 9;
pub const MAX_CHARS: usize = 90;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Origin {
    Edge,
    Local,
}

impl Origin {
    pub fn as_str(self) -> &'static str {
        match self {
            Origin::Edge => "edge",
            Origin::Local => "local",
        }
    }
}

/// One cached line as loaded from disk.
#[derive(Debug, Clone, PartialEq)]
pub struct Loaded {
    pub text: String,
    pub origin: Origin,
    pub samples: Vec<f32>,
    pub sample_rate: u32,
    pub total_ms: u64,
    /// (word, start_ms, duration_ms)
    pub words: Vec<(String, u64, u64)>,
}

pub fn dir_for(app_data: &Path) -> PathBuf {
    app_data.join("tts_cache")
}

/// Lowercase, trailing punctuation removed: "Ok sir." and "ok sir" are the same line.
pub fn norm(text: &str) -> String {
    text.trim().trim_end_matches(['.', '!', '?', ',', ';']).trim().to_lowercase()
}

/// May this line be written to disk? Short stock phrases only, nothing with digits or addresses.
pub fn cacheable(text: &str) -> bool {
    let t = text.trim();
    let words = t.split_whitespace().count();
    words >= 1
        && words <= MAX_WORDS
        && t.chars().count() <= MAX_CHARS
        && !t.chars().any(|c| c.is_ascii_digit() || c == '@' || c == '/' || c == '\\')
}

fn fnv1a(s: &str) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in s.as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

fn sanitize(voice: &str) -> String {
    voice.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' { c } else { '_' }).collect()
}

fn stem(voice: &str, text: &str, origin: Origin) -> String {
    format!("{}__{}__{:016x}", sanitize(voice), origin.as_str(), fnv1a(&norm(text)))
}

/// Write one line (atomic: temp file then rename). Prunes the folder afterwards.
pub fn save(
    dir: &Path,
    voice: &str,
    text: &str,
    origin: Origin,
    samples: &[f32],
    sample_rate: u32,
    total_ms: u64,
    words: &[(String, u64, u64)],
) -> std::io::Result<()> {
    if !cacheable(text) || samples.is_empty() {
        return Ok(());
    }
    std::fs::create_dir_all(dir)?;
    let stem = stem(voice, text, origin);
    let wav_path = dir.join(format!("{stem}.wav"));
    let json_path = dir.join(format!("{stem}.json"));
    let meta = serde_json::json!({
        "text": text.trim(),
        "origin": origin.as_str(),
        "total_ms": total_ms,
        "words": words.iter().map(|(w, s, d)| serde_json::json!({"t": w, "s": s, "d": d})).collect::<Vec<_>>(),
    });
    let wav = crate::tts_wav::encode_wav_i16(samples, sample_rate);
    let tmp_wav = dir.join(format!("{stem}.wav.tmp"));
    let tmp_json = dir.join(format!("{stem}.json.tmp"));
    std::fs::write(&tmp_wav, wav)?;
    std::fs::write(&tmp_json, serde_json::to_vec(&meta).unwrap_or_default())?;
    std::fs::rename(&tmp_wav, &wav_path)?;
    std::fs::rename(&tmp_json, &json_path)?;
    prune(dir, MAX_ENTRIES);
    Ok(())
}

fn read_one(dir: &Path, stem: &str, origin: Origin) -> Option<Loaded> {
    let bytes = std::fs::read(dir.join(format!("{stem}.wav"))).ok()?;
    let (samples, sample_rate) = crate::tts_wav::decode_wav(&bytes)?;
    let meta: serde_json::Value = serde_json::from_slice(&std::fs::read(dir.join(format!("{stem}.json"))).ok()?).ok()?;
    let words = meta
        .get("words")
        .and_then(|w| w.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|w| {
                    Some((w.get("t")?.as_str()?.to_string(), w.get("s")?.as_u64()?, w.get("d")?.as_u64()?))
                })
                .collect()
        })
        .unwrap_or_default();
    Some(Loaded {
        text: meta.get("text").and_then(|t| t.as_str()).unwrap_or_default().to_string(),
        origin,
        total_ms: meta.get("total_ms").and_then(|t| t.as_u64()).unwrap_or_else(|| samples.len() as u64 * 1000 / sample_rate.max(1) as u64),
        samples,
        sample_rate,
        words,
    })
}

/// The best stored version of a line for this voice: `edge` first, then `local`.
pub fn load(dir: &Path, voice: &str, text: &str) -> Option<Loaded> {
    read_one(dir, &stem(voice, text, Origin::Edge), Origin::Edge)
        .or_else(|| read_one(dir, &stem(voice, text, Origin::Local), Origin::Local))
}

/// Does a file of this origin exist (without decoding it)?
#[cfg(test)]
pub fn has(dir: &Path, voice: &str, text: &str, origin: Origin) -> bool {
    dir.join(format!("{}.wav", stem(voice, text, origin))).is_file()
}

/// Remove one stored version (used when a `local` line is replaced by an `edge` one).
pub fn forget(dir: &Path, voice: &str, text: &str, origin: Origin) {
    let stem = stem(voice, text, origin);
    let _ = std::fs::remove_file(dir.join(format!("{stem}.wav")));
    let _ = std::fs::remove_file(dir.join(format!("{stem}.json")));
}

/// Keep at most `max` lines: the oldest files go first.
fn prune(dir: &Path, max: usize) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    let mut wavs: Vec<(std::time::SystemTime, PathBuf)> = rd
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().is_some_and(|x| x == "wav"))
        .filter_map(|e| Some((e.metadata().ok()?.modified().ok()?, e.path())))
        .collect();
    if wavs.len() <= max {
        return;
    }
    wavs.sort_by_key(|(t, _)| *t);
    for (_, p) in wavs.iter().take(wavs.len() - max) {
        let _ = std::fs::remove_file(p);
        let _ = std::fs::remove_file(p.with_extension("json"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("nexus_ttscache_{name}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    fn tone(n: usize) -> Vec<f32> {
        (0..n).map(|i| ((i as f32) * 0.07).sin() * 0.4).collect()
    }

    #[test]
    fn a_saved_line_loads_back_with_its_timings_and_ignores_punctuation_and_case() {
        let d = tmp("roundtrip");
        let words = vec![("Ok".to_string(), 0, 200), ("sir".to_string(), 200, 300)];
        save(&d, "en-US-AvaNeural", "Ok sir.", Origin::Edge, &tone(12_000), 24_000, 500, &words).unwrap();
        let got = load(&d, "en-US-AvaNeural", "ok SIR").expect("found");
        assert_eq!(got.origin, Origin::Edge);
        assert_eq!(got.sample_rate, 24_000);
        assert_eq!(got.samples.len(), 12_000);
        assert_eq!(got.words, words);
        assert_eq!(got.total_ms, 500);
        // a different voice never sees it
        assert!(load(&d, "en-GB-RyanNeural", "Ok sir.").is_none());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn edge_is_preferred_over_local_and_forget_removes_only_that_version() {
        let d = tmp("pref");
        save(&d, "v", "New tab sir", Origin::Local, &tone(4_000), 24_000, 166, &[]).unwrap();
        assert_eq!(load(&d, "v", "New tab sir.").unwrap().origin, Origin::Local);
        save(&d, "v", "New tab sir", Origin::Edge, &tone(4_000), 24_000, 166, &[]).unwrap();
        assert_eq!(load(&d, "v", "New tab sir.").unwrap().origin, Origin::Edge);
        forget(&d, "v", "New tab sir", Origin::Edge);
        assert_eq!(load(&d, "v", "New tab sir.").unwrap().origin, Origin::Local);
        assert!(has(&d, "v", "New tab sir", Origin::Local));
        assert!(!has(&d, "v", "New tab sir", Origin::Edge));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn only_short_stock_lines_are_cacheable() {
        assert!(cacheable("Ok sir."));
        assert!(cacheable("Hand mode on, sir."));
        assert!(!cacheable("Hello, sir. You left OpenCode open about 2 hours ago. Shall I reopen them?"));
        assert!(!cacheable("Call 9876543210"), "digits (numbers) never");
        assert!(!cacheable("mail me at a@b.com"), "addresses never");
        assert!(!cacheable("   "));
        let d = tmp("nocache");
        save(&d, "v", "mail me at a@b.com", Origin::Edge, &tone(100), 24_000, 4, &[]).unwrap();
        assert!(load(&d, "v", "mail me at a@b.com").is_none(), "refused lines are not written");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn the_folder_is_bounded_oldest_first() {
        let d = tmp("prune");
        for i in 0..8 {
            save(&d, "v", &format!("line {}", ["a", "b", "c", "d", "e", "f", "g", "h"][i]), Origin::Local, &tone(200), 24_000, 8, &[]).unwrap();
            std::thread::sleep(std::time::Duration::from_millis(15));
        }
        prune(&d, 5);
        let wavs = std::fs::read_dir(&d).unwrap().filter(|e| e.as_ref().unwrap().path().extension().is_some_and(|x| x == "wav")).count();
        assert_eq!(wavs, 5);
        assert!(load(&d, "v", "line h").is_some(), "the newest survives");
        assert!(load(&d, "v", "line a").is_none(), "the oldest was evicted");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn corrupt_files_are_ignored_not_fatal() {
        let d = tmp("corrupt");
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join(format!("{}.wav", stem("v", "bad line", Origin::Edge))), b"garbage").unwrap();
        assert!(load(&d, "v", "bad line").is_none());
        let _ = std::fs::remove_dir_all(&d);
    }
}
