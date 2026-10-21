//! Feature 83 — Top-10 iconic voice catalog (single source of truth).
//!
//! Each persona pairs a cloud Edge-TTS neural voice (instant switch, 0 MB RAM) with a local
//! **Kokoro-82M voice** (offline twin). Kokoro runs ONE shared model for every voice; a voice is a
//! 522,240-byte style file. The managed slot is therefore
//! `kokoro_model.onnx` (shared, downloaded once) + `active_voice.bin` (exactly one, replaced
//! atomically) + `manifest.json`. The hub grid, the background swap worker, and the TTS dispatcher
//! all read THIS table — never a duplicated list.
//!
//! Source: docs/features/83-top-10-iconic-voices-and-single-slot-dynamic-offline-swapping.md and
//! docs/research/jarvis-landscape/09-phase-3-kokoro-in-single-slot-architecture-2026-10-05.md
//! (Piper was removed: its espeak-ng dependency is GPL-3).

use serde::{Deserialize, Serialize};

/// One iconic voice persona: cloud neural ID + local Kokoro voice + signature preview phrase.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VoicePersona {
    /// Stable key (e.g. "jarvis") — stored in settings + memory.
    pub key: &'static str,
    /// Display name (e.g. "JARVIS").
    pub name: &'static str,
    /// One-line tone summary for the card.
    pub persona: &'static str,
    /// Region · gender tag (e.g. "UK · Male").
    pub accent_tag: &'static str,
    /// Avatar badge for the card (e.g. "🤖").
    pub avatar: &'static str,
    /// Cloud Edge-TTS voice id (e.g. "en-GB-RyanNeural").
    pub cloud_id: &'static str,
    /// Local Kokoro voice name = voice-pack file stem (e.g. "bm_george").
    pub kokoro_voice: &'static str,
    /// 2-second signature preview line (non-destructive demo).
    pub preview_phrase: &'static str,
}

/// The 10-voice lineup, in hub display order. Append-only: never reorder (hub grid, manifests, and
/// memory profiles key off position stability — use `key` for lookups, never the index).
///
/// Kokoro voice choice is a first proposal pending an audition (doc 09 §3). Kokoro's own quality
/// grades: af_heart A, af_bella A-, af_nicole B-, bf_emma B-, af_sarah C+, am_michael C+,
/// bm_george C, bm_fable C. There is no Irish voice, so FRIDAY approximates with bf_emma.
pub const VOICE_CATALOG: &[VoicePersona] = &[
    VoicePersona {
        key: "jarvis",
        name: "JARVIS",
        persona: "Calm, intelligent, British baritone with witty refinement",
        accent_tag: "UK · Male",
        avatar: "🤖",
        cloud_id: "en-GB-RyanNeural",
        kokoro_voice: "bm_george",
        preview_phrase: "At your service, sir. All systems operational.",
    },
    VoicePersona {
        key: "friday",
        name: "FRIDAY",
        persona: "Sharp tactical companion, fast and precise",
        accent_tag: "Ireland · Female",
        avatar: "🛡️",
        cloud_id: "en-IE-EmilyNeural",
        kokoro_voice: "bf_emma",
        preview_phrase: "Boss, tactical links and neural feeds are live.",
    },
    VoicePersona {
        key: "nexus",
        name: "NEXUS",
        persona: "Expressive flagship assistant, warm and clear",
        accent_tag: "US · Female",
        avatar: "🌟",
        cloud_id: "en-US-AvaNeural",
        kokoro_voice: "af_heart",
        preview_phrase: "Hello, I'm NEXUS. What are we building today?",
    },
    VoicePersona {
        key: "siri",
        name: "SIRI",
        persona: "Modern, crisp assistant delivery",
        accent_tag: "US · Female",
        avatar: "📱",
        cloud_id: "en-US-JennyNeural",
        kokoro_voice: "af_bella",
        preview_phrase: "Here is what I found for you.",
    },
    VoicePersona {
        key: "alexa",
        name: "ALEXA",
        persona: "Confident smart-home lead, steady and direct",
        accent_tag: "US · Female",
        avatar: "🔵",
        cloud_id: "en-US-AriaNeural",
        kokoro_voice: "af_nicole",
        preview_phrase: "Ready. Standing by for your instructions.",
    },
    VoicePersona {
        key: "google",
        name: "GOOGLE",
        persona: "Analytical tech specialist, measured and exact",
        accent_tag: "US · Male",
        avatar: "🎙️",
        cloud_id: "en-US-BrianNeural",
        kokoro_voice: "am_michael",
        preview_phrase: "Good day. Let me know what you need analyzed.",
    },
    VoicePersona {
        key: "cortana",
        name: "CORTANA",
        persona: "Heroic sci-fi companion, loyal and bold",
        accent_tag: "US · Female",
        avatar: "💠",
        cloud_id: "en-US-MichelleNeural",
        kokoro_voice: "af_sarah",
        preview_phrase: "Chief, telemetry is locked. I'm with you.",
    },
    VoicePersona {
        key: "samantha",
        name: "SAMANTHA",
        persona: "Warm, intimate conversationalist",
        accent_tag: "US · Female",
        avatar: "💫",
        cloud_id: "en-US-SaraNeural",
        kokoro_voice: "af_sky",
        preview_phrase: "I'm here. It's really good to hear your voice.",
    },
    VoicePersona {
        key: "alfred",
        name: "ALFRED",
        persona: "Master butler, formal and reassuring",
        accent_tag: "UK · Male",
        avatar: "🎩",
        cloud_id: "en-GB-OliverNeural",
        kokoro_voice: "bm_fable",
        preview_phrase: "Very good, sir. I have prepared your workspace.",
    },
    VoicePersona {
        key: "offline_safe",
        name: "EMERGENCY",
        persona: "Offline-safe fallback, always available",
        accent_tag: "US · Female",
        avatar: "🆘",
        cloud_id: "en-US-AvaNeural",
        kokoro_voice: "af_heart",
        preview_phrase: "Local speech synthesizer operational.",
    },
];

/// Default persona key (ships as the active voice).
pub const DEFAULT_VOICE_KEY: &str = "jarvis";

/// Voice used when nothing else is available (best-graded Kokoro voice).
pub const FALLBACK_KOKORO_VOICE: &str = "af_heart";

/// Shared model file name inside the managed slot.
pub const MODEL_FILE: &str = "kokoro_model.onnx";
/// The single active voice pack inside the managed slot.
pub const VOICE_FILE: &str = "active_voice.bin";

/// Managed slot directory: %APPDATA%/com.nexus.assistant/voices/.
/// Holds at most: shared model + ONE voice pack + manifest (+ transient `.tmp` files).
pub fn managed_voices_dir() -> Option<std::path::PathBuf> {
    dirs_next::data_dir().map(|d| d.join("com.nexus.assistant").join("voices"))
}

/// Slot manifest: which voice occupies the slot and the hashes it was verified against.
/// Legacy (Piper-era) manifests deserialize with an empty `kokoro_voice` => "slot not ready",
/// which makes the swap worker repopulate it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VoiceManifest {
    #[serde(default)]
    pub voice_key: String,
    /// Kokoro voice name currently in `active_voice.bin` (e.g. "bm_george").
    #[serde(default)]
    pub kokoro_voice: String,
    /// SHA-256 of `kokoro_model.onnx` when it was installed.
    #[serde(default)]
    pub model_sha256: String,
    /// SHA-256 of `active_voice.bin`.
    #[serde(default)]
    pub voice_sha256: String,
    #[serde(default)]
    pub version: u32,
}

/// Read the slot manifest from an explicit dir (pure core for tests).
pub fn read_manifest_at(dir: &std::path::Path) -> Option<VoiceManifest> {
    let content = std::fs::read_to_string(dir.join("manifest.json")).ok()?;
    serde_json::from_str(&content).ok()
}

/// Write the slot manifest to an explicit dir. Pure core for tests.
pub fn write_manifest_at(dir: &std::path::Path, m: &VoiceManifest) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let json = serde_json::to_string_pretty(m)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    // Write-then-rename: a crash or full disk never leaves a half-written manifest.
    let tmp = dir.join("manifest.json.tmp");
    std::fs::write(&tmp, json)?;
    std::fs::rename(&tmp, dir.join("manifest.json")).inspect_err(|_| {
        let _ = std::fs::remove_file(&tmp);
    })
}

/// True when the installed voice file is exactly the one the manifest recorded (SHA-256). A crash
/// between "voice file replaced" and "manifest written" leaves them disagreeing; the slot is then
/// treated as not ready and the swap worker repairs it. A manifest without a hash (older build) is
/// accepted. Pure core for tests.
pub fn manifest_matches_file(manifest: &VoiceManifest, actual_voice_sha256: Option<&str>) -> bool {
    if manifest.voice_sha256.is_empty() {
        return true;
    }
    actual_voice_sha256.map(|a| a.eq_ignore_ascii_case(&manifest.voice_sha256)).unwrap_or(false)
}

/// True when the shared model AND the active voice pack both exist in `dir`.
pub fn slot_files_present_at(dir: &std::path::Path) -> bool {
    dir.join(MODEL_FILE).is_file() && dir.join(VOICE_FILE).is_file()
}

/// True when the shared model alone is present (a voice swap needs only the 0.5 MB pack).
pub fn base_present_at(dir: &std::path::Path) -> bool {
    dir.join(MODEL_FILE).is_file()
}

/// True when the slot holds THIS Kokoro voice (manifest agrees AND both files exist). Pure.
pub fn twin_ready_for_paths(voice: &str, manifest: Option<&VoiceManifest>, files_present: bool) -> bool {
    manifest.map(|m| !m.kokoro_voice.is_empty() && m.kokoro_voice == voice).unwrap_or(false)
        && files_present
}

/// True when the managed slot holds THIS Kokoro voice (reads the live slot).
pub fn twin_ready_for(voice: &str) -> bool {
    let Some(dir) = managed_voices_dir() else { return false };
    let manifest = read_manifest_at(&dir);
    let files_ok = slot_files_present_at(&dir)
        && manifest
            .as_ref()
            .map(|m| {
                let actual = crate::tts_swap::sha256_file(&dir.join(VOICE_FILE)).ok();
                manifest_matches_file(m, actual.as_deref())
            })
            .unwrap_or(false);
    twin_ready_for_paths(voice, manifest.as_ref(), files_ok)
}

/// Remove the 60 MB Piper-era slot files left by older builds. Safe to call repeatedly.
/// Returns the number of files removed.
pub fn cleanup_legacy_slot_at(dir: &std::path::Path) -> usize {
    let mut n = 0;
    for f in ["active_offline.onnx", "active_offline.onnx.json", "voice_download.tmp", "voice_download.tmp.json"] {
        if std::fs::remove_file(dir.join(f)).is_ok() {
            n += 1;
        }
    }
    n
}

/// Sync state of a persona's offline twin: ready now, or cloud-only (download queued — the swap
/// worker fills it in).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VoiceSyncState {
    Ready,
    CloudOnly,
}

/// Sync state for a persona key (unknown key → CloudOnly, never an error — the hub still shows the
/// card, just without offline).
pub fn sync_state_for(key: &str) -> VoiceSyncState {
    match find_by_key(key) {
        Some(p) if twin_ready_for(p.kokoro_voice) => VoiceSyncState::Ready,
        _ => VoiceSyncState::CloudOnly,
    }
}

/// Look up a persona by key. Pure.
pub fn find_by_key(key: &str) -> Option<&'static VoicePersona> {
    VOICE_CATALOG.iter().find(|p| p.key == key)
}

/// Look up a persona by cloud Edge-TTS id. First match wins — twins can share an id (e.g. nexus +
/// offline_safe both use AvaNeural). Pure.
#[allow(dead_code)]
pub fn find_by_cloud_id(cloud_id: &str) -> Option<&'static VoicePersona> {
    VOICE_CATALOG.iter().find(|p| p.cloud_id == cloud_id)
}

/// Look up a persona by Kokoro voice name. First match wins. Pure.
#[allow(dead_code)]
pub fn find_by_kokoro_voice(voice: &str) -> Option<&'static VoicePersona> {
    VOICE_CATALOG.iter().find(|p| p.kokoro_voice == voice)
}

/// Canonical prosody tuning for each iconic voice persona.
/// Maps cloud Edge-TTS IDs to signature (rate, pitch) offsets matching the
/// original character voices (e.g. Paul Bettany for JARVIS, Kerry Condon for FRIDAY,
/// Scarlett Johansson for SAMANTHA, Michael Caine for ALFRED).
pub fn persona_prosody(cloud_id: &str) -> (&'static str, &'static str) {
    match cloud_id {
        "en-GB-RyanNeural" => ("-5%", "-4Hz"),     // JARVIS (Paul Bettany)
        "en-IE-EmilyNeural" => ("+5%", "+1Hz"),    // FRIDAY (Kerry Condon)
        "en-US-SaraNeural" => ("-4%", "-3Hz"),     // SAMANTHA (Scarlett Johansson)
        "en-GB-OliverNeural" => ("-6%", "-5Hz"),   // ALFRED (Michael Caine)
        "en-US-JennyNeural" => ("+4%", "+1Hz"),    // SIRI
        "en-US-AriaNeural" => ("+2%", "+0Hz"),     // ALEXA
        "en-US-BrianNeural" => ("+2%", "+0Hz"),    // GOOGLE
        "en-US-MichelleNeural" => ("+3%", "+1Hz"), // CORTANA
        _ => ("+0%", "+0Hz"),                      // NEXUS (Ava) / Default
    }
}

/// Where to load the local engine's files from: `(model, voice_pack, voice_name)`.
/// 1. the managed slot (what the swap worker maintains);
/// 2. a bundled/dev copy `<root>/resources/kokoro/{model_quantized.onnx, voices/<voice>.bin}` for
///    the persona currently selected (installer bundle or a developer checkout).
/// Pure over injected roots so it is unit-testable.
pub fn resolve_assets_in(
    managed_dir: Option<&std::path::Path>,
    manifest: Option<&VoiceManifest>,
    bundled_roots: &[std::path::PathBuf],
    selected_voice: &str,
) -> Option<(std::path::PathBuf, std::path::PathBuf, String)> {
    if let (Some(dir), Some(m)) = (managed_dir, manifest) {
        if !m.kokoro_voice.is_empty() && slot_files_present_at(dir) {
            return Some((dir.join(MODEL_FILE), dir.join(VOICE_FILE), m.kokoro_voice.clone()));
        }
    }
    for root in bundled_roots {
        let model = root.join("resources").join("kokoro").join("model_quantized.onnx");
        if !model.is_file() {
            continue;
        }
        for v in [selected_voice, FALLBACK_KOKORO_VOICE] {
            let pack = root.join("resources").join("kokoro").join("voices").join(format!("{v}.bin"));
            if pack.is_file() {
                return Some((model, pack, v.to_string()));
            }
        }
    }
    None
}

/// Live asset resolution for the running app (managed slot first, then bundled/dev copy).
pub fn resolve_assets(selected_voice: &str) -> Option<(std::path::PathBuf, std::path::PathBuf, String)> {
    let dir = managed_voices_dir();
    let manifest = dir.as_deref().and_then(read_manifest_at);
    let mut roots: Vec<std::path::PathBuf> = Vec::new();
    if let Ok(exe) = std::env::current_exe() {
        if let Some(p) = exe.parent() {
            roots.push(p.to_path_buf());
        }
    }
    if let Some(m) = option_env!("CARGO_MANIFEST_DIR") {
        roots.push(std::path::PathBuf::from(m)); // dev checkout (git-ignored download)
    }
    resolve_assets_in(dir.as_deref(), manifest.as_ref(), &roots, selected_voice)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    fn tmp(tag: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("nexus_vc_{tag}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn test_catalog_has_ten_personas() {
        assert_eq!(VOICE_CATALOG.len(), 10);
    }

    #[test]
    fn test_catalog_keys_unique_and_nonempty() {
        let mut seen = HashSet::new();
        for p in VOICE_CATALOG {
            assert!(!p.key.is_empty(), "empty key");
            assert!(!p.name.is_empty(), "empty name for {}", p.key);
            assert!(seen.insert(p.key), "duplicate key {}", p.key);
        }
    }

    #[test]
    fn test_catalog_cloud_ids_wellformed() {
        for p in VOICE_CATALOG {
            assert!(
                p.cloud_id.ends_with("Neural") && p.cloud_id.contains('-'),
                "malformed cloud id {} for {}",
                p.cloud_id,
                p.key
            );
        }
    }

    #[test]
    fn test_catalog_kokoro_voices_are_real_english_voice_names() {
        // Names verified against the onnx-community/Kokoro-82M-v1.0-ONNX voices/ listing
        // (2026-10-05): English voices are af_/am_ (US) and bf_/bm_ (British).
        for p in VOICE_CATALOG {
            let v = p.kokoro_voice;
            assert!(
                v.len() >= 5 && v.chars().all(|c| c.is_ascii_lowercase() || c == '_'),
                "malformed kokoro voice {v} for {}",
                p.key
            );
            assert!(
                ["af_", "am_", "bf_", "bm_"].iter().any(|pre| v.starts_with(pre)),
                "{v} for {} is not an English Kokoro voice",
                p.key
            );
        }
    }

    #[test]
    fn test_catalog_preview_phrases_present() {
        for p in VOICE_CATALOG {
            assert!(!p.preview_phrase.is_empty(), "missing preview for {}", p.key);
        }
    }

    #[test]
    fn test_catalog_accent_matches_voice_prefix() {
        // UK personas must use British (b*) voices, US personas American (a*) ones, except the
        // Irish persona which has no Kokoro equivalent and approximates with British.
        for p in VOICE_CATALOG {
            let british_tag = p.accent_tag.starts_with("UK") || p.accent_tag.starts_with("Ireland");
            assert_eq!(
                british_tag,
                p.kokoro_voice.starts_with('b'),
                "accent/voice mismatch for {}",
                p.key
            );
        }
    }

    #[test]
    fn test_catalog_lookup_roundtrip() {
        for p in VOICE_CATALOG {
            assert_eq!(find_by_key(p.key).map(|q| q.key), Some(p.key));
        }
        // Shared ids resolve first-match-wins (nexus precedes offline_safe).
        assert_eq!(find_by_cloud_id("en-US-AvaNeural").map(|q| q.key), Some("nexus"));
        assert_eq!(find_by_kokoro_voice("af_heart").map(|q| q.key), Some("nexus"));
        assert_eq!(find_by_cloud_id("en-GB-RyanNeural").map(|q| q.key), Some("jarvis"));
        assert_eq!(find_by_kokoro_voice("bm_george").map(|q| q.key), Some("jarvis"));
        assert!(find_by_key("nope").is_none());
        assert!(find_by_cloud_id("xx-YY-NopeNeural").is_none());
        assert!(find_by_kokoro_voice("zz_nope").is_none());
    }

    #[test]
    fn test_default_voice_exists() {
        assert!(find_by_key(DEFAULT_VOICE_KEY).is_some());
        assert!(find_by_kokoro_voice(FALLBACK_KOKORO_VOICE).is_some());
    }

    #[test]
    fn test_offline_safe_uses_best_graded_voice() {
        assert_eq!(find_by_key("offline_safe").unwrap().kokoro_voice, FALLBACK_KOKORO_VOICE);
    }

    #[test]
    fn test_managed_dir_points_at_voices_slot() {
        assert!(managed_voices_dir().unwrap().ends_with("voices"));
    }

    #[test]
    fn test_twin_ready_for_paths_matrix() {
        let m = VoiceManifest {
            voice_key: "jarvis".into(),
            kokoro_voice: "bm_george".into(),
            model_sha256: String::new(),
            voice_sha256: String::new(),
            version: 2,
        };
        assert!(twin_ready_for_paths("bm_george", Some(&m), true));
        // another voice occupies the slot
        assert!(!twin_ready_for_paths("af_heart", Some(&m), true));
        // half-installed (files missing)
        assert!(!twin_ready_for_paths("bm_george", Some(&m), false));
        // empty slot
        assert!(!twin_ready_for_paths("bm_george", None, false));
        // a legacy (Piper-era) manifest has no kokoro_voice => never ready
        let legacy: VoiceManifest = serde_json::from_str(
            r#"{"voice_key":"jarvis","model_name":"en_GB-alan-medium","sha256":"x","version":1}"#,
        )
        .unwrap();
        assert!(legacy.kokoro_voice.is_empty());
        assert!(!twin_ready_for_paths("", Some(&legacy), true));
        assert!(!twin_ready_for_paths("bm_george", Some(&legacy), true));
    }

    #[test]
    fn test_manifest_file_roundtrip_in_temp_dir() {
        let dir = tmp("manifest_rt");
        let m = VoiceManifest {
            voice_key: "jarvis".into(),
            kokoro_voice: "bm_george".into(),
            model_sha256: "aa".into(),
            voice_sha256: "bb".into(),
            version: 2,
        };
        write_manifest_at(&dir, &m).unwrap();
        assert_eq!(read_manifest_at(&dir), Some(m));
        assert!(dir.join("manifest.json").is_file());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_manifest_missing_is_none() {
        let dir = tmp("manifest_missing");
        assert_eq!(read_manifest_at(&dir), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_slot_file_presence() {
        let dir = tmp("slot_files");
        assert!(!base_present_at(&dir) && !slot_files_present_at(&dir));
        std::fs::write(dir.join(MODEL_FILE), b"m").unwrap();
        assert!(base_present_at(&dir) && !slot_files_present_at(&dir));
        std::fs::write(dir.join(VOICE_FILE), b"v").unwrap();
        assert!(slot_files_present_at(&dir));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_legacy_cleanup_removes_only_piper_files() {
        let dir = tmp("legacy");
        for f in ["active_offline.onnx", "active_offline.onnx.json", "voice_download.tmp"] {
            std::fs::write(dir.join(f), b"x").unwrap();
        }
        std::fs::write(dir.join(MODEL_FILE), b"keep").unwrap();
        std::fs::write(dir.join("manifest.json"), b"{}").unwrap();
        assert_eq!(cleanup_legacy_slot_at(&dir), 3);
        assert!(dir.join(MODEL_FILE).is_file() && dir.join("manifest.json").is_file());
        assert_eq!(cleanup_legacy_slot_at(&dir), 0); // idempotent
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_resolve_assets_prefers_managed_then_bundled() {
        let managed = tmp("res_managed");
        let root = tmp("res_root");
        let kdir = root.join("resources").join("kokoro");
        std::fs::create_dir_all(kdir.join("voices")).unwrap();
        std::fs::write(kdir.join("model_quantized.onnx"), b"m").unwrap();
        std::fs::write(kdir.join("voices").join("af_heart.bin"), b"v").unwrap();
        std::fs::write(kdir.join("voices").join("bm_george.bin"), b"v").unwrap();

        // empty managed slot => bundled copy, selected voice honoured
        let got = resolve_assets_in(Some(&managed), None, &[root.clone()], "bm_george").unwrap();
        assert_eq!(got.2, "bm_george");
        // selected voice missing => falls back to af_heart
        let got = resolve_assets_in(Some(&managed), None, &[root.clone()], "am_michael").unwrap();
        assert_eq!(got.2, "af_heart");

        // populated managed slot wins over the bundled copy
        std::fs::write(managed.join(MODEL_FILE), b"m").unwrap();
        std::fs::write(managed.join(VOICE_FILE), b"v").unwrap();
        let m = VoiceManifest { kokoro_voice: "bf_emma".into(), version: 2, ..VoiceManifest::default_for_test() };
        let got = resolve_assets_in(Some(&managed), Some(&m), &[root.clone()], "af_heart").unwrap();
        assert_eq!(got.2, "bf_emma");
        assert!(got.0.ends_with(MODEL_FILE) && got.1.ends_with(VOICE_FILE));

        // nothing anywhere => None
        assert!(resolve_assets_in(None, None, &[], "af_heart").is_none());
        let _ = std::fs::remove_dir_all(&managed);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn test_sync_state_unknown_key_is_cloud_only() {
        assert_eq!(sync_state_for("nope"), VoiceSyncState::CloudOnly);
    }

    impl VoiceManifest {
        fn default_for_test() -> Self {
            VoiceManifest {
                voice_key: String::new(),
                kokoro_voice: String::new(),
                model_sha256: String::new(),
                voice_sha256: String::new(),
                version: 0,
            }
        }
    }
}


#[cfg(test)]
mod manifest_integrity_tests {
    use super::*;

    fn manifest(sha: &str) -> VoiceManifest {
        VoiceManifest {
            voice_key: "jarvis".into(),
            kokoro_voice: "bm_george".into(),
            model_sha256: "m".into(),
            voice_sha256: sha.into(),
            version: 2,
        }
    }

    #[test]
    fn matching_hash_is_consistent_mismatch_is_not() {
        assert!(manifest_matches_file(&manifest("abc123"), Some("ABC123")), "case-insensitive");
        assert!(!manifest_matches_file(&manifest("abc123"), Some("def456")), "different file");
        assert!(!manifest_matches_file(&manifest("abc123"), None), "unreadable file");
        assert!(manifest_matches_file(&manifest(""), None), "older manifest without a hash is accepted");
    }

    #[test]
    fn manifest_write_is_atomic_and_leaves_no_temp_file() {
        let d = std::env::temp_dir().join(format!("nexus_manifest_atomic_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        write_manifest_at(&d, &manifest("aa")).unwrap();
        write_manifest_at(&d, &manifest("bb")).unwrap(); // replaces an existing manifest
        assert_eq!(read_manifest_at(&d).unwrap().voice_sha256, "bb");
        assert!(!d.join("manifest.json.tmp").exists());
        let _ = std::fs::remove_dir_all(&d);
    }
}
