//! Kokoro-82M offline TTS (Apache-2.0 model; English only here).
//!
//! Replaces the Piper engine (GPL espeak-ng) as the local voice. Design:
//!   text --misaki-rs G2P (no espeak)--> IPA phonemes --vocab--> token ids
//!        --ort (Kokoro ONNX) + voice style vector--> 24 kHz f32 audio
//! One shared model (`model_quantized.onnx`, ~88 MB) serves every voice; a voice is a
//! 522,240-byte style pack (`<name>.bin` = 510 x 1 x 256 f32). Switching voice = swapping the
//! style vector — no model reload (see docs/research/jarvis-landscape/09).
//!
//! Adapted from pguso/kokoro (Apache-2.0): vocabulary table and 510-phoneme chunking
//! (`tokenizer.rs`, `pipeline.rs`). G2P is `MicheleYin/misaki-rs` (MIT) with
//! `default-features = false`. Synthesis glue is written against our pinned `ort`.
//! See THIRD_PARTY_NOTICES.md.

use std::path::Path;
use std::sync::Arc;
use tokio::sync::Mutex;

pub const SAMPLE_RATE: u32 = 24_000;
/// Max phoneme characters per model call (matches the reference KPipeline budget).
#[allow(dead_code)]
pub const MAX_PHONEME_CHARS: usize = 510;
const STYLE_DIM: usize = 256;
const PACK_ROWS: usize = 510;
/// Exact size of a voice pack file.
pub const VOICE_PACK_BYTES: usize = PACK_ROWS * STYLE_DIM * 4;

const VOCAB: &[(char, i64)] = &[
    ('\u{3B}', 1),
    ('\u{3A}', 2),
    ('\u{2C}', 3),
    ('\u{2E}', 4),
    ('\u{21}', 5),
    ('\u{3F}', 6),
    ('\u{2014}', 9),
    ('\u{2026}', 10),
    ('\u{22}', 11),
    ('\u{28}', 12),
    ('\u{29}', 13),
    ('\u{201C}', 14),
    ('\u{201D}', 15),
    ('\u{20}', 16),
    ('\u{303}', 17),
    ('\u{2A3}', 18),
    ('\u{2A5}', 19),
    ('\u{2A6}', 20),
    ('\u{2A8}', 21),
    ('\u{1D5D}', 22),
    ('\u{AB67}', 23),
    ('\u{41}', 24),
    ('\u{49}', 25),
    ('\u{4F}', 31),
    ('\u{51}', 33),
    ('\u{53}', 35),
    ('\u{54}', 36),
    ('\u{57}', 39),
    ('\u{59}', 41),
    ('\u{1D4A}', 42),
    ('\u{61}', 43),
    ('\u{62}', 44),
    ('\u{63}', 45),
    ('\u{64}', 46),
    ('\u{65}', 47),
    ('\u{66}', 48),
    ('\u{68}', 50),
    ('\u{69}', 51),
    ('\u{6A}', 52),
    ('\u{6B}', 53),
    ('\u{6C}', 54),
    ('\u{6D}', 55),
    ('\u{6E}', 56),
    ('\u{6F}', 57),
    ('\u{70}', 58),
    ('\u{71}', 59),
    ('\u{72}', 60),
    ('\u{73}', 61),
    ('\u{74}', 62),
    ('\u{75}', 63),
    ('\u{76}', 64),
    ('\u{77}', 65),
    ('\u{78}', 66),
    ('\u{79}', 67),
    ('\u{7A}', 68),
    ('\u{251}', 69),
    ('\u{250}', 70),
    ('\u{252}', 71),
    ('\u{E6}', 72),
    ('\u{3B2}', 75),
    ('\u{254}', 76),
    ('\u{255}', 77),
    ('\u{E7}', 78),
    ('\u{256}', 80),
    ('\u{F0}', 81),
    ('\u{2A4}', 82),
    ('\u{259}', 83),
    ('\u{25A}', 85),
    ('\u{25B}', 86),
    ('\u{25C}', 87),
    ('\u{25F}', 90),
    ('\u{261}', 92),
    ('\u{265}', 99),
    ('\u{268}', 101),
    ('\u{26A}', 102),
    ('\u{29D}', 103),
    ('\u{26F}', 110),
    ('\u{270}', 111),
    ('\u{14B}', 112),
    ('\u{273}', 113),
    ('\u{272}', 114),
    ('\u{274}', 115),
    ('\u{F8}', 116),
    ('\u{278}', 118),
    ('\u{3B8}', 119),
    ('\u{153}', 120),
    ('\u{279}', 123),
    ('\u{27E}', 125),
    ('\u{27B}', 126),
    ('\u{281}', 128),
    ('\u{27D}', 129),
    ('\u{282}', 130),
    ('\u{283}', 131),
    ('\u{288}', 132),
    ('\u{2A7}', 133),
    ('\u{28A}', 135),
    ('\u{28B}', 136),
    ('\u{28C}', 138),
    ('\u{263}', 139),
    ('\u{264}', 140),
    ('\u{3C7}', 142),
    ('\u{28E}', 143),
    ('\u{292}', 147),
    ('\u{294}', 148),
    ('\u{2C8}', 156),
    ('\u{2CC}', 157),
    ('\u{2D0}', 158),
    ('\u{2B0}', 162),
    ('\u{2B2}', 164),
    ('\u{2193}', 169),
    ('\u{2192}', 171),
    ('\u{2197}', 172),
    ('\u{2198}', 173),
    ('\u{1D7B}', 177),
];


fn vocab_id(c: char) -> Option<i64> {
    VOCAB.iter().find(|(ch, _)| *ch == c).map(|(_, id)| *id)
}

/// Phoneme string -> model input ids, padded with 0 at both ends. Characters the model does not
/// know are dropped (callers should check `unknown_phonemes` in tests/diagnostics).
pub fn token_ids(phonemes: &str) -> Vec<i64> {
    let mut ids = Vec::with_capacity(phonemes.chars().count() + 2);
    ids.push(0);
    ids.extend(phonemes.chars().filter_map(vocab_id));
    ids.push(0);
    ids
}

/// Distinct phoneme characters not in the model vocabulary.
#[allow(dead_code)]
pub fn unknown_phonemes(phonemes: &str) -> Vec<char> {
    let mut out: Vec<char> = Vec::new();
    for c in phonemes.chars() {
        if vocab_id(c).is_none() && !out.contains(&c) {
            out.push(c);
        }
    }
    out
}

/// Split a phoneme line into segments of at most `max_chars`, preferring whitespace boundaries.
pub fn chunk_phonemes(ps: &str, max_chars: usize) -> Vec<String> {
    let ps = ps.trim();
    if ps.is_empty() {
        return Vec::new();
    }
    if ps.chars().count() <= max_chars {
        return vec![ps.to_string()];
    }
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut cur_chars = 0usize;
    for word in ps.split_whitespace() {
        let wl = word.chars().count();
        if wl > max_chars {
            if !cur.is_empty() {
                out.push(std::mem::take(&mut cur));
                cur_chars = 0;
            }
            let chars: Vec<char> = word.chars().collect();
            for chunk in chars.chunks(max_chars) {
                out.push(chunk.iter().collect());
            }
            continue;
        }
        let add = if cur.is_empty() { wl } else { wl + 1 };
        if cur_chars + add <= max_chars {
            if !cur.is_empty() {
                cur.push(' ');
                cur_chars += 1;
            }
            cur.push_str(word);
            cur_chars += wl;
        } else {
            out.push(std::mem::take(&mut cur));
            cur.push_str(word);
            cur_chars = wl;
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// Phoneme budget per model call for live synthesis. RAM grows with input length (measured on the
/// int8 model: ~260 MB at 45 tokens, ~370 MB at 120, ~650 MB at 300), so live speech is cut at
/// sentence boundaries into chunks of at most this many phoneme characters.
pub const LIVE_CHUNK_CHARS: usize = 110;

/// Sentence-aware chunking for live synthesis: split after standalone `.`, `?`, `!` tokens, then pack
/// whole sentences greedily up to `budget`; over-long sentences fall back to `chunk_phonemes`.
pub fn chunk_for_synthesis(ps: &str, budget: usize) -> Vec<String> {
    let mut sentences: Vec<String> = Vec::new();
    let mut cur = String::new();
    for tok in ps.split_whitespace() {
        if !cur.is_empty() {
            cur.push(' ');
        }
        cur.push_str(tok);
        if matches!(tok, "." | "?" | "!") {
            sentences.push(std::mem::take(&mut cur));
        }
    }
    if !cur.trim().is_empty() {
        sentences.push(cur);
    }
    let mut out: Vec<String> = Vec::new();
    let mut pack = String::new();
    for sent in sentences {
        let sl = sent.chars().count();
        if sl > budget {
            if !pack.is_empty() {
                out.push(std::mem::take(&mut pack));
            }
            out.extend(chunk_phonemes(&sent, budget));
            continue;
        }
        let add = if pack.is_empty() { sl } else { sl + 1 };
        if !pack.is_empty() && pack.chars().count() + add > budget {
            out.push(std::mem::take(&mut pack));
        }
        if !pack.is_empty() {
            pack.push(' ');
        }
        pack.push_str(&sent);
    }
    if !pack.is_empty() {
        out.push(pack);
    }
    out
}

/// Parse a voice pack: little-endian f32, exactly `510 x 1 x 256`, all finite.
pub fn parse_voice_pack(bytes: &[u8]) -> Result<Vec<f32>, String> {
    if bytes.len() != VOICE_PACK_BYTES {
        return Err(format!(
            "kokoro: voice pack is {} bytes, expected {VOICE_PACK_BYTES}",
            bytes.len()
        ));
    }
    let pack: Vec<f32> = bytes
        .chunks_exact(4)
        .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
        .collect();
    if pack.iter().any(|v| !v.is_finite()) {
        return Err("kokoro: voice pack contains non-finite values".into());
    }
    Ok(pack)
}

/// Style vector for a phoneme string of `n_phonemes` characters (reference implementation:
/// `voices[len(ps) - 1]`, clamped to the pack).
pub fn style_row(pack: &[f32], n_phonemes: usize) -> &[f32] {
    let row = n_phonemes.saturating_sub(1).min(PACK_ROWS - 1);
    &pack[row * STYLE_DIM..(row + 1) * STYLE_DIM]
}

/// Kokoro voice names start with language+gender (`af_`, `bm_`...). `b*` = British English.
pub fn is_british_voice(name: &str) -> bool {
    name.starts_with('b')
}

/// Product/brand words the dictionary-only G2P cannot know. Substituted with explicit IPA using
/// Misaki's `[word](/phonemes/)` override syntax. Extend as mispronunciations are found.
const LEXICON: &[(&str, &str)] = &[
    ("NEXUS", "nˈɛksəs"),
    ("WhatsApp", "wˈɑtsˌæp"),
    ("Ghostwriter", "ɡˈOstɹˌItəɹ"),
    ("Groq", "ɡɹˈɑk"),
    ("Gmail", "ʤˈiˌmAl"),
    ("Moonshine", "mˈunʃˌIn"),
    ("Spotify", "spˈɑtəfˌI"),
    ("Cerebras", "sɚˈibɹəs"),
];

/// Apply the product lexicon (case-sensitive whole-word match; also the upper/lower-case forms).
pub fn apply_lexicon(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 16);
    let mut word = String::new();
    let flush = |word: &mut String, out: &mut String| {
        if word.is_empty() {
            return;
        }
        let hit = LEXICON.iter().find(|(w, _)| w.eq_ignore_ascii_case(word));
        match hit {
            Some((_, ph)) => {
                out.push('[');
                out.push_str(word);
                out.push_str("](/");
                out.push_str(ph);
                out.push_str("/)");
            }
            None => out.push_str(word),
        }
        word.clear();
    };
    for c in text.chars() {
        if c.is_alphanumeric() {
            word.push(c);
        } else {
            flush(&mut word, &mut out);
            out.push(c);
        }
    }
    flush(&mut word, &mut out);
    out
}

/// Out-of-vocabulary words (no espeak available): spell them out letter by letter in English
/// letter names. Always yields phonemes, never errors — a mispronounced name beats silence.
struct SpellOutFallback;

impl misaki_rs::Fallback for SpellOutFallback {
    fn phonemize(&self, word: &str) -> Result<String, misaki_rs::fallback::FallbackError> {
        let names = spell_out(word);
        if names.is_empty() {
            Err(misaki_rs::fallback::FallbackError::NoPhonemes { word: word.to_string() })
        } else {
            Ok(names)
        }
    }
}

fn spell_out(word: &str) -> String {
    let mut parts: Vec<&'static str> = Vec::new();
    for c in word.chars() {
        let p = match c.to_ascii_lowercase() {
            'a' => "ˈA",
            'b' => "bˈi",
            'c' => "sˈi",
            'd' => "dˈi",
            'e' => "ˈi",
            'f' => "ˈɛf",
            'g' => "ʤˈi",
            'h' => "ˈAʧ",
            'i' => "ˈI",
            'j' => "ʤˈA",
            'k' => "kˈA",
            'l' => "ˈɛl",
            'm' => "ˈɛm",
            'n' => "ˈɛn",
            'o' => "ˈO",
            'p' => "pˈi",
            'q' => "kjˈu",
            'r' => "ˈɑɹ",
            's' => "ˈɛs",
            't' => "tˈi",
            'u' => "jˈu",
            'v' => "vˈi",
            'w' => "dˈʌbᵊlju",
            'x' => "ˈɛks",
            'y' => "wˈI",
            'z' => "zˈi",
            '0'..='9' => "",
            _ => "",
        };
        if !p.is_empty() {
            parts.push(p);
        }
    }
    parts.join(" ")
}

/// Build a G2P for the voice's accent. No espeak: dictionary + rules + spell-out fallback.
fn build_g2p(british: bool) -> misaki_rs::G2P {
    use misaki_rs::{Language, G2P};
    let lang = if british { Language::EnglishGB } else { Language::EnglishUS };
    G2P::with_fallback(lang, Some(Box::new(SpellOutFallback)))
}

/// Text -> phoneme string for the given accent (lexicon applied first).
pub fn phonemize(g2p: &misaki_rs::G2P, text: &str) -> Result<String, String> {
    let (ps, _) = g2p
        .g2p(&apply_lexicon(text))
        .map_err(|e| format!("kokoro: g2p failed: {e}"))?;
    Ok(normalize_phonemes(&ps))
}

/// misaki-rs's expanded dictionary (espeak-derived entries) joins diphthongs with U+200D
/// (zero-width joiner), which is not in Kokoro's vocabulary; drop it ("o\u{200d}ʊ" -> "oʊ").
pub fn normalize_phonemes(ps: &str) -> String {
    ps.chars().filter(|c| *c != '\u{200d}' && *c != '\u{200c}').collect()
}

/// Loaded engine state.
pub struct Loaded {
    session: ort::session::Session,
    g2p: misaki_rs::G2P,
    british: bool,
    pack: Vec<f32>,
    voice: String,
}

/// Lazy engine handle (None = not loaded). Same shape the old Piper engine had.
pub type KokoroEngine = Arc<Mutex<Option<Loaded>>>;

pub fn new_engine() -> KokoroEngine {
    Arc::new(Mutex::new(None))
}

/// Load the shared model + one voice (`voice_name` = Kokoro name, e.g. "bm_george" — it decides the
/// accent; the pack file itself is anonymous, e.g. `active_voice.bin`). Blocking work runs off the
/// async runtime.
pub async fn load(engine: &KokoroEngine, model: &Path, voice_pack: &Path, voice_name: &str) -> Result<(), String> {
    let (model, voice_pack, voice) = (model.to_path_buf(), voice_pack.to_path_buf(), voice_name.to_string());
    let loaded = tokio::task::spawn_blocking(move || -> Result<Loaded, String> {
        let bytes = std::fs::read(&voice_pack).map_err(|e| format!("kokoro: read voice: {e}"))?;
        let pack = parse_voice_pack(&bytes)?;
        let session = ort::session::Session::builder()
            .map_err(|e| format!("kokoro: session builder: {e}"))?
            .commit_from_file(&model)
            .map_err(|e| format!("kokoro: load model {}: {e}", model.display()))?;
        let british = is_british_voice(&voice);
        Ok(Loaded { session, g2p: build_g2p(british), british, pack, voice })
    })
    .await
    .map_err(|e| format!("kokoro: load task: {e}"))??;
    *engine.lock().await = Some(loaded);
    Ok(())
}

/// Hot-swap the voice without reloading the model. Rebuilds G2P only if the accent changes.
pub async fn set_voice(engine: &KokoroEngine, voice_pack: &Path, voice_name: &str) -> Result<(), String> {
    let bytes = tokio::fs::read(voice_pack).await.map_err(|e| format!("kokoro: read voice: {e}"))?;
    let pack = parse_voice_pack(&bytes)?;
    let voice = voice_name.to_string();
    let mut guard = engine.lock().await;
    let l = guard.as_mut().ok_or_else(|| "kokoro: engine not loaded".to_string())?;
    let british = is_british_voice(&voice);
    if british != l.british {
        l.g2p = build_g2p(british);
        l.british = british;
    }
    l.pack = pack;
    l.voice = voice;
    Ok(())
}

/// Kokoro voice of the currently equipped persona. Used only when the engine has to fall back to a
/// bundled/dev copy of the voices (the managed slot always knows its own voice).
static PREFERRED_VOICE: std::sync::Mutex<String> = std::sync::Mutex::new(String::new());

pub fn set_preferred_voice(voice: &str) {
    if let Ok(mut v) = PREFERRED_VOICE.lock() {
        *v = voice.to_string();
    }
}

fn preferred_voice() -> String {
    PREFERRED_VOICE
        .lock()
        .ok()
        .map(|v| v.clone())
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| crate::voice_catalog::FALLBACK_KOKORO_VOICE.to_string())
}

/// Make sure the local engine is loaded with the right files: resolves the managed slot (or the
/// bundled/dev copy), loads the shared model if needed, hot-swaps the voice if only the voice
/// changed. Err = no local assets available (offline voice not downloaded yet).
pub async fn ensure_loaded(engine: &KokoroEngine) -> Result<(), String> {
    let (model, pack, voice) = crate::voice_catalog::resolve_assets(&preferred_voice())
        .ok_or_else(|| "kokoro: no offline voice installed yet".to_string())?;
    match current_voice(engine).await {
        Some(v) if v == voice => Ok(()),
        Some(_) => set_voice(engine, &pack, &voice).await,
        None => load(engine, &model, &pack, &voice).await,
    }
}

/// Free the model (RAM) — reloaded lazily on the next offline utterance.
pub async fn unload(engine: &KokoroEngine) {
    *engine.lock().await = None;
}

pub async fn is_loaded(engine: &KokoroEngine) -> bool {
    engine.lock().await.is_some()
}

/// Currently loaded voice name, if any.
pub async fn current_voice(engine: &KokoroEngine) -> Option<String> {
    engine.lock().await.as_ref().map(|l| l.voice.clone())
}

fn run_chunk(l: &mut Loaded, phonemes: &str, speed: f32) -> Result<Vec<f32>, String> {
    use ort::value::Tensor;
    let ids = token_ids(phonemes);
    let n = ids.len();
    let style = style_row(&l.pack, phonemes.chars().count()).to_vec();
    let input_ids = Tensor::<i64>::from_array(([1usize, n], ids.into_boxed_slice()))
        .map_err(|e| format!("kokoro: input_ids: {e}"))?;
    let style = Tensor::<f32>::from_array(([1usize, STYLE_DIM], style.into_boxed_slice()))
        .map_err(|e| format!("kokoro: style: {e}"))?;
    let speed = Tensor::<f32>::from_array(([1usize], vec![speed].into_boxed_slice()))
        .map_err(|e| format!("kokoro: speed: {e}"))?;
    let outputs = l
        .session
        .run(ort::inputs!["input_ids" => input_ids, "style" => style, "speed" => speed])
        .map_err(|e| format!("kokoro: run: {e}"))?;
    let (_, audio) = outputs["waveform"]
        .try_extract_tensor::<f32>()
        .map_err(|e| format!("kokoro: waveform: {e}"))?;
    Ok(audio.to_vec())
}

/// Synthesize `text` with the loaded voice. Returns (24 kHz mono f32 PCM, sample_rate).
/// `speed` 1.0 = normal. Runs the heavy work on a blocking thread.
pub async fn synthesize(engine: &KokoroEngine, text: &str, speed: f32) -> Result<(Vec<f32>, u32), String> {
    let text = text.to_string();
    let eng = engine.clone();
    tokio::task::spawn_blocking(move || -> Result<(Vec<f32>, u32), String> {
        let mut guard = eng.blocking_lock();
        let l = guard.as_mut().ok_or_else(|| "kokoro: engine not loaded".to_string())?;
        let phonemes = phonemize(&l.g2p, &text)?;
        let mut audio: Vec<f32> = Vec::new();
        for chunk in chunk_for_synthesis(&phonemes, LIVE_CHUNK_CHARS) {
            audio.extend(run_chunk(l, &chunk, speed.clamp(0.5, 2.0))?);
        }
        Ok((audio, SAMPLE_RATE))
    })
    .await
    .map_err(|e| format!("kokoro: synth task: {e}"))?
}

#[cfg(test)]
mod tests {
    use super::*;

    fn res(rel: &str) -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("resources/kokoro").join(rel)
    }

    #[test]
    fn vocab_table_is_sane() {
        assert_eq!(VOCAB.len(), 114);
        let mut ids: Vec<i64> = VOCAB.iter().map(|(_, i)| *i).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), 114, "duplicate ids");
        assert!(ids.iter().all(|i| (1..=177).contains(i)));
        assert_eq!(vocab_id(' '), Some(16));
        assert_eq!(vocab_id('a'), Some(43));
    }

    #[test]
    fn token_ids_pad_and_drop_unknown() {
        let ids = token_ids("ab");
        assert_eq!(ids, vec![0, 43, 44, 0]);
        // an unmapped symbol is dropped, never panics
        assert_eq!(token_ids("a\u{1F600}b"), vec![0, 43, 44, 0]);
        assert_eq!(unknown_phonemes("a\u{1F600}b"), vec!['\u{1F600}']);
        assert_eq!(token_ids(""), vec![0, 0]);
    }

    #[test]
    fn chunking_respects_budget() {
        assert!(chunk_phonemes("   ", 510).is_empty());
        assert_eq!(chunk_phonemes("abc def", 510), vec!["abc def".to_string()]);
        let long: String = (0..200).map(|i| format!("w{i}")).collect::<Vec<_>>().join(" ");
        let chunks = chunk_phonemes(&long, 60);
        assert!(chunks.len() > 1);
        assert!(chunks.iter().all(|c| c.chars().count() <= 60));
        // a single over-long word is split by characters, nothing lost
        let word = "x".repeat(130);
        let parts = chunk_phonemes(&word, 50);
        assert_eq!(parts.iter().map(|p| p.len()).sum::<usize>(), 130);
    }

    #[test]
    fn sentence_chunking_for_live_synthesis() {
        let ps = "a b . c d ? e f !";
        // all sentences fit one chunk
        assert_eq!(chunk_for_synthesis(ps, 100), vec!["a b . c d ? e f !".to_string()]);
        // small budget: whole sentences, never split mid-sentence when they fit
        assert_eq!(
            chunk_for_synthesis(ps, 8),
            vec!["a b .".to_string(), "c d ?".to_string(), "e f !".to_string()]
        );
        // unterminated tail still emitted
        assert_eq!(chunk_for_synthesis("a b . c d", 8), vec!["a b .".to_string(), "c d".to_string()]);
        assert!(chunk_for_synthesis("   ", 50).is_empty());
        // an over-long single sentence is split by the word-boundary chunker, nothing lost
        let long: String = (0..60).map(|i| format!("w{i}")).collect::<Vec<_>>().join(" ") + " .";
        let chunks = chunk_for_synthesis(&long, 40);
        assert!(chunks.len() > 1 && chunks.iter().all(|c| c.chars().count() <= 40));
        let rejoined: String = chunks.join(" ");
        assert_eq!(rejoined.split_whitespace().count(), long.split_whitespace().count());
        // live budget keeps real sentences intact
        assert!(LIVE_CHUNK_CHARS < MAX_PHONEME_CHARS);
    }

    #[test]
    fn voice_pack_validation() {
        assert!(parse_voice_pack(&[0u8; 10]).is_err());
        let good = vec![0u8; VOICE_PACK_BYTES];
        assert_eq!(parse_voice_pack(&good).unwrap().len(), PACK_ROWS * STYLE_DIM);
        let mut bad = vec![0u8; VOICE_PACK_BYTES];
        bad[0..4].copy_from_slice(&f32::NAN.to_le_bytes());
        assert!(parse_voice_pack(&bad).is_err());
    }

    #[test]
    fn style_row_indexing_and_clamp() {
        let pack: Vec<f32> = (0..PACK_ROWS * STYLE_DIM).map(|i| (i / STYLE_DIM) as f32).collect();
        assert_eq!(style_row(&pack, 1)[0], 0.0); // n-1
        assert_eq!(style_row(&pack, 10)[0], 9.0);
        assert_eq!(style_row(&pack, 0)[0], 0.0); // saturating
        assert_eq!(style_row(&pack, 9999)[0], 509.0); // clamped
        assert_eq!(style_row(&pack, 5).len(), STYLE_DIM);
    }

    #[test]
    fn accent_from_voice_name() {
        assert!(is_british_voice("bm_george"));
        assert!(is_british_voice("bf_emma"));
        assert!(!is_british_voice("af_heart"));
        assert!(!is_british_voice("am_michael"));
    }

    #[test]
    fn lexicon_substitutes_whole_words_only() {
        assert_eq!(apply_lexicon("open WhatsApp now"), "open [WhatsApp](/wˈɑtsˌæp/) now");
        assert_eq!(apply_lexicon("nexus!"), "[nexus](/nˈɛksəs/)!");
        // substring of a longer word is untouched
        assert_eq!(apply_lexicon("nexuses"), "nexuses");
        assert_eq!(apply_lexicon("plain text"), "plain text");
    }

    #[test]
    fn spell_out_uses_vocab_symbols_only() {
        assert_eq!(spell_out("ab"), "ˈA bˈi");
        assert_eq!(spell_out("x9"), "ˈɛks");
        for c in 'a'..='z' {
            let s = spell_out(&c.to_string());
            assert!(unknown_phonemes(&s).is_empty(), "letter {c} -> {s} has symbols outside the vocab");
        }
    }

    #[test]
    fn g2p_covers_assistant_sentences_without_unknown_symbols() {
        let corpus = [
            "Hello, I'm NEXUS. What are we building today?",
            "Opening WhatsApp, sir.",
            "On it sir.",
            "Didn't catch that sir.",
            "You have 3 new emails and a meeting at 2:30 PM tomorrow.",
            "The weather in London is 17 degrees and cloudy.",
            "Qzxwv is not a word, but I can still say it.",
            "Reminder: submit the report by Friday, October 10th.",
            "Stopped typing, sir.",
        ];
        for british in [false, true] {
            let g = build_g2p(british);
            for s in corpus {
                let ps = phonemize(&g, s).expect("g2p must not fail (spell-out fallback)");
                assert!(!ps.trim().is_empty(), "empty phonemes for {s:?}");
                let unk = unknown_phonemes(&ps);
                assert!(unk.is_empty(), "{s:?} (british={british}) -> {ps:?} has unknown symbols {unk:?}");
            }
        }
    }

    #[test]
    fn normalize_drops_zero_width_joiners() {
        assert_eq!(normalize_phonemes("həlˈo\u{200d}ʊ"), "həlˈoʊ");
        assert_eq!(normalize_phonemes("a\u{200c}b"), "ab");
    }

    /// Dev helper (ignored): dumps the G2P phoneme strings for the evaluation sentences as JSON so
    /// other model variants can be benchmarked on identical inputs.
    #[test]
    #[ignore]
    fn dump_phonemes_for_variant_eval() {
        let dir = std::env::var("KOKORO_DUMP_DIR").expect("set KOKORO_DUMP_DIR");
        let sentences = [
            "Hello, I'm NEXUS. What are we building today?",
            "Opening WhatsApp, sir.",
            "You have three new emails and a meeting at two thirty tomorrow.",
            "Reminder: submit the report by Friday.",
            "Stopped typing, sir.",
            "The weather in London is seventeen degrees and cloudy.",
            "I could not find that contact. Would you like me to search again?",
            "Your build finished successfully with no warnings.",
        ];
        let mut out = String::from("[");
        for (i, s) in sentences.iter().enumerate() {
            for british in [false, true] {
                let g = build_g2p(british);
                let ps = phonemize(&g, s).unwrap();
                if out.len() > 1 {
                    out.push(',');
                }
                out.push_str(&format!(
                    "{{\"i\":{i},\"british\":{british},\"text\":{},\"ps\":{}}}",
                    serde_json::to_string(s).unwrap(),
                    serde_json::to_string(&ps).unwrap()
                ));
            }
        }
        out.push(']');
        std::fs::write(format!("{dir}/phonemes.json"), out).unwrap();
    }

    /// Dev helper (ignored): writes WAVs of test sentences to $KOKORO_DUMP_DIR for an ASR round-trip.
    #[tokio::test]
    #[ignore]
    async fn dump_wavs_for_asr() {
        let dir = std::env::var("KOKORO_DUMP_DIR").expect("set KOKORO_DUMP_DIR");
        let model = res("model_quantized.onnx");
        let sentences = [
            "Hello, I'm NEXUS. What are we building today?",
            "Opening WhatsApp, sir.",
            "You have three new emails and a meeting at two thirty tomorrow.",
            "Reminder: submit the report by Friday.",
            "Stopped typing, sir.",
        ];
        for voice in ["af_heart", "bm_george", "bf_emma", "af_bella", "bm_fable"] {
            let eng = new_engine();
            load(&eng, &model, &res(&format!("voices/{voice}.bin")), voice).await.expect("load");
            for (i, s) in sentences.iter().enumerate() {
                let t0 = std::time::Instant::now();
                let (audio, sr) = synthesize(&eng, s, 1.0).await.expect("synth");
                let wall = t0.elapsed().as_secs_f32();
                println!("DUMP {voice} #{i}: {:.2}s audio, wall {wall:.2}s", audio.len() as f32 / sr as f32);
                let pcm: Vec<u8> = audio
                    .iter()
                    .flat_map(|x| ((x.clamp(-1.0, 1.0) * 32767.0) as i16).to_le_bytes())
                    .collect();
                let mut wav = Vec::new();
                wav.extend_from_slice(b"RIFF");
                wav.extend_from_slice(&(36 + pcm.len() as u32).to_le_bytes());
                wav.extend_from_slice(b"WAVEfmt ");
                wav.extend_from_slice(&16u32.to_le_bytes());
                wav.extend_from_slice(&1u16.to_le_bytes());
                wav.extend_from_slice(&1u16.to_le_bytes());
                wav.extend_from_slice(&sr.to_le_bytes());
                wav.extend_from_slice(&(sr * 2).to_le_bytes());
                wav.extend_from_slice(&2u16.to_le_bytes());
                wav.extend_from_slice(&16u16.to_le_bytes());
                wav.extend_from_slice(b"data");
                wav.extend_from_slice(&(pcm.len() as u32).to_le_bytes());
                wav.extend_from_slice(&pcm);
                std::fs::write(format!("{dir}/{voice}_{i}.wav"), wav).unwrap();
            }
        }
    }

    /// Dev helper (ignored): process memory after each stage of bringing Kokoro up, for the RAM plan
    /// (docs/features/131). Run once per variant in a fresh process:
    /// KOKORO_VARIANT = default | t2 | t2_noarena | t4_noarena
    fn proc_mem_mb() -> (f64, f64) {
        use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System};
        let pid = sysinfo::get_current_pid().expect("pid");
        let mut sys = System::new();
        sys.refresh_processes_specifics(ProcessesToUpdate::Some(&[pid]), true, ProcessRefreshKind::new().with_memory());
        let p = sys.process(pid).expect("self");
        (p.memory() as f64 / 1_048_576.0, p.virtual_memory() as f64 / 1_048_576.0)
    }

    #[tokio::test]
    #[ignore]
    async fn mem_breakdown() {
        let variant = std::env::var("KOKORO_VARIANT").unwrap_or_else(|_| "default".to_string());
        let model = res("model_quantized.onnx");
        let pack = parse_voice_pack(&std::fs::read(res("voices/bm_george.bin")).expect("voice")).expect("pack");
        let show = |label: &str| {
            let (ws, commit) = proc_mem_mb();
            println!("MEM [{variant}] {label:<38} ws={ws:>6.0} MB  virtual={commit:>6.0} MB (virtual size, NOT commit)");
        };
        show("0 baseline (test harness)");
        let g2p = build_g2p(true);
        show("1 G2P built (GB gold+silver+tagger)");
        let ps = phonemize(&g2p, "Opening WhatsApp, sir.").expect("phonemize");
        assert!(!ps.is_empty());
        show("2 after first phonemize");
        let mut b = ort::session::Session::builder().expect("builder");
        b = match variant.as_str() {
            "t2" => b.with_intra_threads(2).expect("threads"),
            "t2_noarena" => b
                .with_intra_threads(2)
                .expect("threads")
                .with_memory_pattern(false)
                .expect("pattern")
                .with_execution_providers([ort::ep::CPU::default().with_arena_allocator(false).build()])
                .expect("ep"),
            "noarena" => b
                .with_memory_pattern(false)
                .expect("pattern")
                .with_execution_providers([ort::ep::CPU::default().with_arena_allocator(false).build()])
                .expect("ep"),
            "t4_noarena" => b
                .with_intra_threads(4)
                .expect("threads")
                .with_memory_pattern(false)
                .expect("pattern")
                .with_execution_providers([ort::ep::CPU::default().with_arena_allocator(false).build()])
                .expect("ep"),
            _ => b,
        };
        let session = b.commit_from_file(&model).expect("load model");
        show("3 ORT session loaded (88 MB int8 file)");
        let eng = new_engine();
        *eng.lock().await = Some(Loaded { session, g2p, british: true, pack, voice: "bm_george".to_string() });
        let sentences = [
            "Opening WhatsApp, sir.",
            "You have three new emails and a meeting at two thirty tomorrow.",
            "Stopped typing, sir.",
            "The weather in London is seventeen degrees and cloudy.",
            "Your build finished successfully with no warnings.",
        ];
        let (mut audio_s, mut wall_s) = (0.0f32, 0.0f32);
        for (i, t) in sentences.iter().enumerate() {
            let t0 = std::time::Instant::now();
            let (a, sr) = synthesize(&eng, t, 1.0).await.expect("synth");
            let wall = t0.elapsed().as_secs_f32();
            audio_s += a.len() as f32 / sr as f32;
            wall_s += wall;
            println!(
                "MEMTTS [{variant}] #{i} {} words: audio {:.2}s, synth wall {:.2}s (RTF {:.2})",
                t.split_whitespace().count(),
                a.len() as f32 / sr as f32,
                wall,
                wall / (a.len() as f32 / sr as f32)
            );
            if i == 0 {
                show("4 after FIRST synthesis");
            }
        }
        show("5 after 5 syntheses");
        println!("MEM [{variant}] speed: {audio_s:.1}s audio in {wall_s:.1}s wall = RTF {:.2} (debug build)", wall_s / audio_s);
        unload(&eng).await;
        show("6 after unload() (Loaded dropped)");
    }

    /// Needs the downloaded model + voice (git-ignored). Skipped when absent.
    #[tokio::test]
    async fn synthesizes_audible_speech_when_model_present() {
        let (model, voice) = (res("model_quantized.onnx"), res("voices/af_heart.bin"));
        if !model.exists() || !voice.exists() {
            eprintln!("kokoro model not present — skipping integration test");
            return;
        }
        let eng = new_engine();
        load(&eng, &model, &voice, "af_heart").await.expect("load");
        assert_eq!(current_voice(&eng).await.as_deref(), Some("af_heart"));
        let t0 = std::time::Instant::now();
        let (audio, sr) = synthesize(&eng, "Hello, I'm NEXUS. What are we building today?", 1.0)
            .await
            .expect("synth");
        let wall = t0.elapsed().as_secs_f32();
        let secs = audio.len() as f32 / sr as f32;
        let rms = (audio.iter().map(|x| x * x).sum::<f32>() / audio.len().max(1) as f32).sqrt();
        let peak = audio.iter().fold(0f32, |m, x| m.max(x.abs()));
        println!("kokoro: {secs:.2}s audio in {wall:.2}s wall (RTF {:.2}), rms {rms:.3}, peak {peak:.2}", wall / secs);
        assert_eq!(sr, 24_000);
        assert!(audio.iter().all(|x| x.is_finite()));
        assert!(secs > 1.5 && secs < 8.0, "implausible duration {secs}s");
        assert!(rms > 0.01, "silent output (rms {rms})");
        assert!(peak <= 1.5, "clipping/garbage (peak {peak})");
        // voice hot-swap keeps working (British accent => new G2P)
        set_voice(&eng, &res("voices/bm_george.bin"), "bm_george").await.expect("swap");
        assert_eq!(current_voice(&eng).await.as_deref(), Some("bm_george"));
        let (a2, _) = synthesize(&eng, "At your service, sir.", 1.0).await.expect("synth2");
        assert!(a2.len() > 12_000);
        unload(&eng).await;
        assert!(!is_loaded(&eng).await);
    }
}
