//! In-process NLU inference (BERT-Mini via tract-onnx) — replaces the
//! Python NLU sidecar for the base classification tier. No subprocess
//! spawn, no HTTP round-trip, no 15s cold-start wait, no network.
//!
//! Mirrors `server/nlu_server.py`'s `/parse` endpoint exactly: same model
//! file, same fixed MAX_LEN=64 tokenization (padding="max_length",
//! truncation=True), same intent softmax+argmax, same BIO slot decoding.
//! Intent/slot label names are read from `labels.json` at load time (not
//! hardcoded) so this can never desync from the admin retrain pipeline the
//! way the Python server's hardcoded `INTENTS`/`SLOT_TYPES` lists already
//! have twice (see server/nlu_server.py's own history comments).
//!
//! Model dir resolution order (mirrors `wakeword_oww::resolve_oww_dir` +
//! `nlu_update::downloaded_model_dir_envless`):
//!   1. An admin-trained model pulled via the OTA updater (nlu_update.rs)
//!   2. Bundled resource dir (prod) / dev checkout fallback chain
//!
//! `get_model()` loads once and caches; `reload()` forces a fresh load so
//! the OTA updater can hot-swap a newly downloaded model without an app
//! restart (the Python server exposed this as POST /reload — this is the
//! in-process equivalent).

use once_cell::sync::Lazy;
use parking_lot::RwLock;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tract_onnx::prelude::*;

/// Fixed sequence length the model was exported with (both heads assume
/// this — see the ONNX graph's declared input shapes).
const MAX_LEN: usize = 64;

type ModelType = Arc<TypedSimplePlan>;

struct NluModel {
    plan: ModelType,
    tokenizer: tokenizers::Tokenizer,
    intents: Vec<String>,
    slots: Vec<String>,
}

static MODEL: Lazy<RwLock<Option<Arc<NluModel>>>> = Lazy::new(|| RwLock::new(None));
static INIT_ATTEMPTED: AtomicBool = AtomicBool::new(false);

/// Best-effort stand-in for `app.path().resource_dir()` without needing an
/// AppHandle threaded through the call chain. On Windows, Tauri v2's
/// resource_dir() IS the exe's directory for a bundled build (documented
/// in wakeword_oww.rs's resolve_oww_dir) — this mirrors that exactly.
fn resource_dir_guess() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|p| p.to_path_buf()))
        .unwrap_or_else(|| PathBuf::from("."))
}

fn resolve_nlu_dir() -> Option<PathBuf> {
    // 1. Admin-trained model pulled OTA (same source lazy_nlu.rs used for
    //    the Python sidecar's NEXUS_NLU_MODEL_DIR env var).
    if let Some(dir) = crate::nlu_update::downloaded_model_dir_envless() {
        return Some(dir);
    }
    let resource_dir = resource_dir_guess();
    let prod = resource_dir.join("resources").join("server").join("nlu").join("model");
    if prod.join("nexus_nlu.onnx").exists() {
        return Some(prod);
    }
    let prod_alt = resource_dir.join("server").join("nlu").join("model");
    if prod_alt.join("nexus_nlu.onnx").exists() {
        return Some(prod_alt);
    }
    if let Ok(manifest) = std::env::var("CARGO_MANIFEST_DIR") {
        let dev = PathBuf::from(manifest)
            .join("resources")
            .join("server")
            .join("nlu")
            .join("model");
        if dev.join("nexus_nlu.onnx").exists() {
            return Some(dev);
        }
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let dev = dir
                .join("..")
                .join("..")
                .join("resources")
                .join("server")
                .join("nlu")
                .join("model");
            if dev.join("nexus_nlu.onnx").exists() {
                return Some(dev.canonicalize().unwrap_or(dev));
            }
        }
    }
    None
}

fn load_model() -> Option<Arc<NluModel>> {
    let dir = resolve_nlu_dir()?;
    let onnx_path = dir.join("nexus_nlu.onnx");
    let labels_path = dir.join("labels.json");
    let tokenizer_path = dir.join("tokenizer").join("tokenizer.json");

    let labels_raw = match std::fs::read_to_string(&labels_path) {
        Ok(s) => s,
        Err(e) => {
            tracing::warn!("nlu_local: labels.json unreadable at {}: {e}", labels_path.display());
            return None;
        }
    };
    let labels_json: serde_json::Value = serde_json::from_str(&labels_raw).ok()?;
    let intents: Vec<String> = labels_json
        .get("intents")?
        .as_array()?
        .iter()
        .filter_map(|v| v.as_str().map(String::from))
        .collect();
    let slots: Vec<String> = labels_json
        .get("slots")?
        .as_array()?
        .iter()
        .filter_map(|v| v.as_str().map(String::from))
        .collect();
    if intents.is_empty() || slots.is_empty() {
        tracing::warn!("nlu_local: labels.json has empty intents/slots — refusing to load");
        return None;
    }

    let mut tokenizer = match tokenizers::Tokenizer::from_file(&tokenizer_path) {
        Ok(t) => t,
        Err(e) => {
            tracing::warn!("nlu_local: tokenizer load failed at {}: {e}", tokenizer_path.display());
            return None;
        }
    };
    let pad_id = tokenizer.token_to_id("[PAD]").unwrap_or(0);
    tokenizer.with_padding(Some(tokenizers::PaddingParams {
        strategy: tokenizers::PaddingStrategy::Fixed(MAX_LEN),
        direction: tokenizers::PaddingDirection::Right,
        pad_id,
        pad_type_id: 0,
        pad_token: "[PAD]".to_string(),
        pad_to_multiple_of: None,
    }));
    if let Err(e) = tokenizer.with_truncation(Some(tokenizers::TruncationParams {
        max_length: MAX_LEN,
        strategy: tokenizers::TruncationStrategy::LongestFirst,
        stride: 0,
        direction: tokenizers::TruncationDirection::Right,
    })) {
        tracing::warn!("nlu_local: truncation config failed: {e}");
        return None;
    }

    let mut raw_model = match tract_onnx::onnx().model_for_path(&onnx_path) {
        Ok(m) => m,
        Err(e) => {
            tracing::warn!("nlu_local: ONNX parse failed at {}: {e}", onnx_path.display());
            return None;
        }
    };
    // Concretize both inputs to a fixed [1, MAX_LEN] i64 shape — batch is
    // always 1 (single utterance), and nailing the shape down before
    // optimization avoids leaving symbolic dims for tract to juggle
    // through the attention-mask broadcasting ops.
    if raw_model
        .set_input_fact(0, InferenceFact::dt_shape(DatumType::I64, tvec!(1, MAX_LEN)))
        .is_err()
        || raw_model
            .set_input_fact(1, InferenceFact::dt_shape(DatumType::I64, tvec!(1, MAX_LEN)))
            .is_err()
    {
        tracing::warn!("nlu_local: failed to set input facts on ONNX graph");
        return None;
    }
    let plan = match raw_model.into_optimized().and_then(|m| m.into_runnable()) {
        Ok(p) => p,
        Err(e) => {
            tracing::warn!("nlu_local: ONNX optimize/runnable failed: {e}");
            return None;
        }
    };

    tracing::info!(
        "nlu_local: in-process model loaded from {} ({} intents, {} slots, no sidecar)",
        dir.display(),
        intents.len(),
        slots.len()
    );

    Some(Arc::new(NluModel { plan, tokenizer, intents, slots }))
}

/// Serialises the first load: a parse that arrives while the wake-time prewarm is still loading waits for
/// it (0.3-0.7 s) instead of seeing "no model" and silently skipping the local classifier for that turn.
static LOAD_LOCK: parking_lot::Mutex<()> = parking_lot::Mutex::new(());

/// Start loading the command model in the background if it is not resident (called when a capture starts, so
/// the model is ready by the time the transcript exists and the first command pays no load time).
pub fn prewarm() {
    if MODEL.read().is_some() || INIT_ATTEMPTED.load(Ordering::SeqCst) {
        return;
    }
    let _ = std::thread::Builder::new().name("nlu-prewarm".into()).spawn(|| {
        let _ = get_model();
    });
}

fn get_model() -> Option<Arc<NluModel>> {
    {
        let guard = MODEL.read();
        if let Some(m) = guard.as_ref() {
            return Some(m.clone());
        }
    }
    let _load = LOAD_LOCK.lock();
    if let Some(m) = MODEL.read().as_ref() {
        return Some(m.clone()); // another thread finished the load while we waited
    }
    // Only attempt a fresh load once per "model absent" state — avoids
    // re-parsing a missing/broken model on every single transcript.
    if INIT_ATTEMPTED.swap(true, Ordering::SeqCst) {
        return MODEL.read().as_ref().cloned();
    }
    let loaded = load_model();
    *MODEL.write() = loaded.clone();
    loaded
}

/// True when the in-process model is loaded and classifying (triggers a
/// load attempt if one hasn't happened yet). Used by the health/diagnostics
/// panel — the in-process equivalent of the old Python sidecar's port-open
/// check, except "responsive" now means "loaded", not "process reachable".
pub fn is_loaded() -> bool {
    get_model().is_some()
}

/// Force a fresh load from disk, replacing whatever is cached. Called by
/// the OTA model updater (`nlu_update.rs`) after a successful download —
/// the in-process equivalent of the old Python server's `POST /reload`.
pub fn reload() -> bool {
    let loaded = load_model();
    let ok = loaded.is_some();
    *MODEL.write() = loaded;
    INIT_ATTEMPTED.store(true, Ordering::SeqCst);
    ok
}

/// Result of local in-process classification — same shape as the Python
/// server's `/parse` JSON response, so `nlu_client::nlu_to_parsed_intent`
/// (the downstream mapping to `ParsedIntent`) needs zero changes.
pub struct LocalNluResult {
    pub intent: String,
    pub slots: serde_json::Value,
    pub confidence: f32,
}

fn softmax(xs: &[f32]) -> Vec<f32> {
    let max = xs.iter().cloned().fold(f32::MIN, f32::max);
    let exps: Vec<f32> = xs.iter().map(|x| (x - max).exp()).collect();
    let sum: f32 = exps.iter().sum::<f32>().max(f32::EPSILON);
    exps.iter().map(|e| e / sum).collect()
}

fn argmax(xs: &[f32]) -> (usize, f32) {
    let mut best_i = 0usize;
    let mut best_v = f32::MIN;
    for (i, &v) in xs.iter().enumerate() {
        if v > best_v {
            best_v = v;
            best_i = i;
        }
    }
    (best_i, best_v)
}

/// Classify a transcript in-process. `None` when the model/tokenizer
/// files aren't present (e.g. a dev checkout with no trained model yet)
/// or inference fails — callers fall back exactly as they did when the
/// Python sidecar was unreachable.
pub fn parse_local(text: &str) -> Option<LocalNluResult> {
    let model = get_model()?;
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return None;
    }

    let encoding = model.tokenizer.encode(trimmed, true).ok()?;
    let ids: Vec<i64> = encoding.get_ids().iter().map(|&x| x as i64).collect();
    let mask: Vec<i64> = encoding.get_attention_mask().iter().map(|&x| x as i64).collect();
    let tokens: Vec<String> = encoding.get_tokens().to_vec();
    if ids.len() != MAX_LEN || mask.len() != MAX_LEN {
        tracing::warn!(
            "nlu_local: unexpected token length {} (want {MAX_LEN}) — tokenizer padding/truncation misconfigured",
            ids.len()
        );
        return None;
    }

    let ids_tensor = Tensor::from_shape(&[1, MAX_LEN], &ids).ok()?;
    let mask_tensor = Tensor::from_shape(&[1, MAX_LEN], &mask).ok()?;

    let outputs = model.plan.run(tvec!(ids_tensor.into(), mask_tensor.into())).ok()?;
    if outputs.len() < 2 {
        return None;
    }

    let intent_logits = outputs[0].clone().into_tensor().into_plain_array::<f32>().ok()?;
    let intent_logits = intent_logits.as_slice()?;
    let num_intents = model.intents.len();
    if intent_logits.len() < num_intents {
        return None;
    }
    let probs = softmax(&intent_logits[..num_intents]);
    let (intent_id, confidence) = argmax(&probs);
    let intent = model.intents.get(intent_id)?.clone();

    let slot_tensor = outputs[1].clone().into_tensor();
    let slot_shape = slot_tensor.shape().to_vec(); // [1, seq_len, num_slots]
    let slot_logits = slot_tensor.into_plain_array::<f32>().ok()?;
    let slot_logits = slot_logits.as_slice()?;
    let num_slots = model.slots.len();
    let seq_len = *slot_shape.get(1).unwrap_or(&tokens.len());

    let mut tag_ids = Vec::with_capacity(seq_len);
    for pos in 0..seq_len {
        let base = pos * num_slots;
        let Some(row) = slot_logits.get(base..base + num_slots) else { break };
        tag_ids.push(argmax(row).0);
    }

    let slots = extract_slots(&tag_ids, &tokens, &model.slots);

    Some(LocalNluResult { intent, slots, confidence })
}

/// Port of `server/nlu_server.py`'s `extract_slots`/`_store_slot`/
/// `_join_parts` — BIO-tag decoding over WordPiece subword tokens.
fn extract_slots(tag_ids: &[usize], tokens: &[String], slot_names: &[String]) -> serde_json::Value {
    let mut raw: HashMap<String, Vec<String>> = HashMap::new();
    let mut current_slot: Option<String> = None;
    let mut current_parts: Vec<(String, bool)> = Vec::new();

    for (i, token) in tokens.iter().enumerate() {
        if token == "[CLS]" || token == "[SEP]" || token == "[PAD]" {
            flush_slot(&mut current_slot, &mut current_parts, &mut raw);
            continue;
        }
        let tag_id = tag_ids.get(i).copied().unwrap_or(0);
        let tag = slot_names.get(tag_id).map(String::as_str).unwrap_or("O");
        let is_subword = token.starts_with("##");
        let clean_token = token.trim_start_matches("##").to_string();

        if let Some(name) = tag.strip_prefix("B-") {
            flush_slot(&mut current_slot, &mut current_parts, &mut raw);
            current_slot = Some(name.to_string());
            current_parts.push((clean_token, is_subword));
        } else if let Some(name) = tag.strip_prefix("I-") {
            if current_slot.as_deref() == Some(name) {
                current_parts.push((clean_token, is_subword));
            } else {
                flush_slot(&mut current_slot, &mut current_parts, &mut raw);
            }
        } else {
            flush_slot(&mut current_slot, &mut current_parts, &mut raw);
        }
    }
    flush_slot(&mut current_slot, &mut current_parts, &mut raw);

    let mut out = serde_json::Map::new();
    for (key, values) in raw {
        let cleaned: Vec<String> = values
            .into_iter()
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty())
            .collect();
        if cleaned.is_empty() {
            continue;
        }
        if key == "keys" {
            out.insert(key, serde_json::Value::Array(cleaned.into_iter().map(serde_json::Value::String).collect()));
        } else {
            out.insert(key, serde_json::Value::String(cleaned.last().cloned().unwrap_or_default()));
        }
    }
    serde_json::Value::Object(out)
}

fn flush_slot(
    current_slot: &mut Option<String>,
    current_parts: &mut Vec<(String, bool)>,
    raw: &mut HashMap<String, Vec<String>>,
) {
    if let Some(slot) = current_slot.take() {
        if !current_parts.is_empty() {
            store_slot(raw, &slot, join_parts(current_parts));
        }
    }
    current_parts.clear();
}

fn store_slot(raw: &mut HashMap<String, Vec<String>>, key: &str, value: String) {
    raw.entry(key.to_string()).or_default().push(value);
}

/// Join subword token parts into one string: `##` subwords and punctuation
/// glue without a space, everything else joins with a space.
fn join_parts(parts: &[(String, bool)]) -> String {
    const PUNCTUATION: [char; 7] = ['/', '-', '.', ':', '_', '@', '#'];
    let mut result = String::new();
    for (text, is_subword) in parts {
        let starts_punct = text.chars().next().map(|c| PUNCTUATION.contains(&c)).unwrap_or(false);
        let last_is_punct = result.chars().last().map(|c| PUNCTUATION.contains(&c)).unwrap_or(false);
        if *is_subword || starts_punct || last_is_punct {
            result.push_str(text);
        } else {
            if !result.is_empty() {
                result.push(' ');
            }
            result.push_str(text);
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_join_parts_subwords_glue_without_space() {
        let parts = vec![("what".to_string(), false), ("sapp".to_string(), true)];
        assert_eq!(join_parts(&parts), "whatsapp");
    }

    #[test]
    fn test_join_parts_regular_words_join_with_space() {
        let parts = vec![("open".to_string(), false), ("chrome".to_string(), false)];
        assert_eq!(join_parts(&parts), "open chrome");
    }

    #[test]
    fn test_join_parts_punctuation_glues() {
        let parts = vec![
            ("owner".to_string(), false),
            ("/".to_string(), false),
            ("repo".to_string(), false),
        ];
        assert_eq!(join_parts(&parts), "owner/repo");
    }

    #[test]
    fn test_extract_slots_basic_bio_span() {
        // tokens: [CLS] open what ##sapp [SEP] [PAD]...
        let tokens = vec![
            "[CLS]".to_string(),
            "open".to_string(),
            "what".to_string(),
            "##sapp".to_string(),
            "[SEP]".to_string(),
            "[PAD]".to_string(),
        ];
        let slot_names = vec!["O".to_string(), "B-app_name".to_string(), "I-app_name".to_string()];
        // O, O, B-app_name, I-app_name, O, O
        let tag_ids = vec![0, 0, 1, 2, 0, 0];
        let slots = extract_slots(&tag_ids, &tokens, &slot_names);
        assert_eq!(slots["app_name"], "whatsapp");
    }

    #[test]
    fn test_extract_slots_empty_when_all_outside() {
        let tokens = vec!["[CLS]".to_string(), "hello".to_string(), "[SEP]".to_string()];
        let slot_names = vec!["O".to_string()];
        let tag_ids = vec![0, 0, 0];
        let slots = extract_slots(&tag_ids, &tokens, &slot_names);
        assert_eq!(slots, serde_json::json!({}));
    }

    #[test]
    fn test_extract_slots_keys_slot_collects_list() {
        let tokens = vec![
            "[CLS]".to_string(),
            "ctrl".to_string(),
            "shift".to_string(),
            "[SEP]".to_string(),
        ];
        let slot_names = vec!["O".to_string(), "B-keys".to_string(), "I-keys".to_string()];
        let tag_ids = vec![0, 1, 2, 0];
        let slots = extract_slots(&tag_ids, &tokens, &slot_names);
        assert_eq!(slots["keys"], serde_json::json!(["ctrl shift"]));
    }

    #[test]
    fn test_softmax_sums_to_one() {
        let probs = softmax(&[1.0, 2.0, 3.0]);
        let sum: f32 = probs.iter().sum();
        assert!((sum - 1.0).abs() < 1e-5);
        // Highest logit gets the highest probability.
        assert!(probs[2] > probs[1] && probs[1] > probs[0]);
    }

    #[test]
    fn test_argmax_picks_max() {
        assert_eq!(argmax(&[0.1, 0.9, 0.3]).0, 1);
        assert_eq!(argmax(&[5.0]).0, 0);
    }

    #[test]
    fn test_resolve_nlu_dir_none_without_model_files() {
        // In this test process CARGO_MANIFEST_DIR points at src-tauri, and
        // the bundled model IS present in this repo (committed per
        // AGENTS.md), so this just confirms resolution doesn't panic and
        // returns a dir containing the onnx file when one exists.
        if let Some(dir) = resolve_nlu_dir() {
            assert!(dir.join("nexus_nlu.onnx").exists());
        }
    }

    #[test]
    fn test_parse_local_empty_text_is_none() {
        assert!(parse_local("").is_none());
        assert!(parse_local("   ").is_none());
    }

    /// End-to-end smoke test against the REAL bundled model (not a mock) —
    /// catches tensor-layout/shape mistakes that pure-logic unit tests
    /// above can't (e.g. swapped input order, wrong output indexing).
    /// Doesn't assert a specific intent label (that's the model's call,
    /// and changes across retrains) — only that inference runs cleanly
    /// end-to-end and produces a well-formed, confident result for an
    /// unambiguous command.
    #[test]
    fn test_parse_local_end_to_end_smoke() {
        // "whatsapp_open" is its own trained intent (no slot) — confirmed
        // working at 0.986 confidence. Use "open chrome" here instead so
        // this test also exercises the BIO slot-extraction path.
        let Some(result) = parse_local("open chrome") else {
            // Only acceptable reason: no model bundled in this checkout.
            assert!(resolve_nlu_dir().is_none(), "model present but inference failed");
            return;
        };
        eprintln!(
            "[smoke] intent={} confidence={:.3} slots={}",
            result.intent, result.confidence, result.slots
        );
        assert!(!result.intent.is_empty());
        assert!(result.confidence >= 0.0 && result.confidence <= 1.0);
        assert_eq!(result.intent, "open_app");
        assert_eq!(result.slots.get("app_name").and_then(|v| v.as_str()), Some("chrome"));
        assert!(result.confidence > 0.5, "confidence suspiciously low: {}", result.confidence);
    }

    /// Dev helper (ignored): resident memory before / after the in-process NLU model loads and after it
    /// is dropped (docs/features/132 RAM budget).
    #[test]
    #[ignore]
    fn mem_nlu_load_parse_unload() {
        use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System};
        let ws = || -> f64 {
            let pid = sysinfo::get_current_pid().expect("pid");
            let mut sys = System::new();
            sys.refresh_processes_specifics(ProcessesToUpdate::Some(&[pid]), true, ProcessRefreshKind::new().with_memory());
            sys.process(pid).expect("self").memory() as f64 / 1_048_576.0
        };
        println!("MEMNLU 0 baseline                ws={:.0} MB", ws());
        let t0 = std::time::Instant::now();
        let first = parse_local("open chrome");
        println!("MEMNLU 1 after load + 1st parse  ws={:.0} MB  ({} ms) ok={}", ws(), t0.elapsed().as_millis(), first.is_some());
        let t1 = std::time::Instant::now();
        for t in ["open notepad", "send a message to mom", "what is the weather", "close brave", "start hand mode"] {
            let _ = parse_local(t);
        }
        println!("MEMNLU 2 after 5 more parses     ws={:.0} MB  ({} ms total)", ws(), t1.elapsed().as_millis());
        *MODEL.write() = None;
        println!("MEMNLU 3 after dropping the model ws={:.0} MB", ws());
    }
}
