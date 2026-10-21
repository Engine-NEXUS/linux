//! Feature 83 P3 — background offline-voice swap worker (Kokoro).
//!
//! When the user equips a persona the cloud voice switches instantly (tts.rs); this worker makes the
//! matching OFFLINE voice available in the background:
//!   * shared model `kokoro_model.onnx` (~88 MB) is downloaded ONCE (first sync);
//!   * a voice is a 522,240-byte pack — switching voice downloads only that file and replaces
//!     `active_voice.bin` atomically (≈ 1 s after the first sync).
//! Safety: every download goes to a `.tmp`, is checked (size, pinned SHA-256 when known, parse/trial
//! load) and only then renamed over the live file — a failed or interrupted download changes nothing.
//! **Latest wins**: equip A → B → C quickly and the slot ends on C (the previous implementation
//! dropped later requests while one was running).
//! Downloads come from an immutable Hugging Face revision (`KOKORO_REV`), not `main`.

use std::path::Path;
use std::sync::Mutex;
use tauri::{AppHandle, Emitter, Runtime};

/// Pinned commit of `onnx-community/Kokoro-82M-v1.0-ONNX` (Apache-2.0 per its model card).
pub const KOKORO_REV: &str = "1939ad2a8e416c0acfeecc08a694d14ef25f2231";
const HF_BASE: &str = "https://huggingface.co/onnx-community/Kokoro-82M-v1.0-ONNX/resolve";
const MODEL_REMOTE: &str = "onnx/model_quantized.onnx";
/// `model_quantized.onnx` (int8) as measured on 2026-10-05.
pub const MODEL_BYTES: u64 = 92_361_116;
pub const MODEL_SHA256: &str = "fbae9257e1e05ffc727e951ef9b9c98418e6d79f1c9b6b13bd59f5c9028a1478";
pub const VOICE_BYTES: u64 = crate::tts_kokoro::VOICE_PACK_BYTES as u64;
const TMP_MODEL: &str = "kokoro_model.tmp";
const TMP_VOICE: &str = "voice_download.tmp";

/// SHA-256 of every catalog voice pack. The five marked (local) were downloaded and hashed on this
/// machine; the others come from Hugging Face's `X-Linked-ETag` header (the LFS SHA-256) for the
/// pinned revision — that method was validated against the local hashes (model, af_heart,
/// bm_george matched exactly). Voices outside this table are still checked by exact size + a full
/// parse (finite f32 pack) but not by hash.
const VOICE_SHA256: &[(&str, &str)] = &[
    ("af_nicole", "cd2191ab31b914ed7b318416b0e4440fdf392ddad9106a060819aa600a64f59a"),
    ("am_michael", "1d1f21dd8da39c30705cd4c75d039d265e9bc4a2a93ed09bc9e1b1225eb95ba1"),
    ("af_sarah", "4409fbc125afabacc615d94db5398d847006a737b0247d6892b7a9a0007a2f0a"),
    ("af_sky", "4435255c9744f3f31659e0d714ab7689bf65d9e77ec1cce060f083912614f0b9"),
    ("af_heart", "d583ccff3cdca2f7fae535cb998ac07e9fcb90f09737b9a41fa2734ec44a8f0b"),
    ("af_bella", "f69d836209b78eb8c66e75e3cda491e26ea838a3674257e9d4e5703cbaf55c8b"),
    ("bf_emma", "669fe0647f9dd04fcab92f1439a40eeb4c8b4ab1f82e4996fe3d918ce4a63b73"),
    ("bm_fable", "f889083196807b4adb15e9204252165f503b8d33d3982e681c52443c49d798f1"),
    ("bm_george", "c4b235a4c1f2cd3b939fed08b899ce9385638b763f7b73a59616c4fc9bd6c9bc"),
];

pub fn expected_voice_sha(name: &str) -> Option<&'static str> {
    VOICE_SHA256.iter().find(|(n, _)| *n == name).map(|(_, h)| *h)
}

/// Download URL for the shared model at the pinned revision.
pub fn model_url() -> String {
    format!("{HF_BASE}/{KOKORO_REV}/{MODEL_REMOTE}")
}

/// A valid English Kokoro voice name: `a|b` (US/British) + `f|m` + `_` + lowercase letters.
pub fn is_valid_voice_name(name: &str) -> bool {
    let b = name.as_bytes();
    b.len() >= 4
        && matches!(b[0], b'a' | b'b')
        && matches!(b[1], b'f' | b'm')
        && b[2] == b'_'
        && b[3..].iter().all(|c| c.is_ascii_lowercase())
}

/// Download URL for a voice pack at the pinned revision; None for anything that is not a valid
/// English voice name (no guessing, no path tricks).
pub fn voice_url(name: &str) -> Option<String> {
    if !is_valid_voice_name(name) {
        return None;
    }
    Some(format!("{HF_BASE}/{KOKORO_REV}/voices/{name}.bin"))
}

/// What a swap has to do, given the live slot. Pure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SwapPlan {
    /// The wanted voice is already installed.
    Nothing,
    /// Shared model present: fetch only the 0.5 MB voice pack.
    VoiceOnly,
    /// First sync: fetch the shared model, then the voice pack.
    BaseAndVoice,
}

pub fn plan_swap(base_present: bool, slot_voice: Option<&str>, voice_file_present: bool, wanted: &str) -> SwapPlan {
    if base_present && voice_file_present && slot_voice == Some(wanted) {
        SwapPlan::Nothing
    } else if base_present {
        SwapPlan::VoiceOnly
    } else {
        SwapPlan::BaseAndVoice
    }
}

/// Latest-wins request queue. `request` returns true when the caller must start a worker; while a
/// worker runs, a newer request simply replaces the pending target (A → B → C ends on C).
#[derive(Debug, Default)]
pub struct SwapQueue {
    pending: Option<String>,
    running: bool,
}

impl SwapQueue {
    pub const fn new() -> Self {
        SwapQueue { pending: None, running: false }
    }
    pub fn request(&mut self, key: &str) -> bool {
        self.pending = Some(key.to_string());
        if self.running {
            false
        } else {
            self.running = true;
            true
        }
    }
    pub fn take(&mut self) -> Option<String> {
        self.pending.take()
    }
    /// Worker calls this after each job: true => another target arrived, keep going; false => the
    /// worker is done (state reset so the next request starts a fresh worker).
    pub fn keep_going(&mut self) -> bool {
        if self.pending.is_some() {
            true
        } else {
            self.running = false;
            false
        }
    }
}

static QUEUE: Mutex<SwapQueue> = Mutex::new(SwapQueue::new());
static CURRENT: Mutex<Option<String>> = Mutex::new(None);

/// Persona key currently being synced (if any) — lets `get_voice_status` report "downloading".
pub fn swap_target() -> Option<String> {
    CURRENT.lock().ok()?.clone()
}

fn set_current(v: Option<String>) {
    if let Ok(mut c) = CURRENT.lock() {
        *c = v;
    }
}

/// SHA-256 hex digest of a file.
pub fn sha256_file(path: &Path) -> std::io::Result<String> {
    use sha2::Digest;
    let bytes = std::fs::read(path)?;
    let mut hasher = sha2::Sha256::new();
    hasher.update(&bytes);
    Ok(format!("{:x}", hasher.finalize()))
}

/// Verify `tmp` (exact size when known, SHA-256 when known) and atomically replace `dest` with it.
/// On any failure `tmp` is removed and `dest` is untouched. Returns the verified SHA-256.
pub fn verify_and_install(
    tmp: &Path,
    dest: &Path,
    expected_bytes: Option<u64>,
    expected_sha: Option<&str>,
) -> Result<String, String> {
    let fail = |msg: String| -> Result<String, String> {
        let _ = std::fs::remove_file(tmp);
        Err(msg)
    };
    let len = match std::fs::metadata(tmp) {
        Ok(m) => m.len(),
        Err(e) => return fail(format!("missing download: {e}")),
    };
    if let Some(want) = expected_bytes {
        if len != want {
            return fail(format!("size mismatch: got {len}, expected {want}"));
        }
    }
    let digest = match sha256_file(tmp) {
        Ok(d) => d,
        Err(e) => return fail(format!("hash failed: {e}")),
    };
    if let Some(want) = expected_sha {
        if !want.is_empty() && !want.eq_ignore_ascii_case(&digest) {
            return fail("checksum mismatch".to_string());
        }
    }
    // rename over an existing file is atomic on the same volume (Windows MoveFileEx REPLACE_EXISTING)
    if let Err(e) = std::fs::rename(tmp, dest) {
        return fail(format!("atomic replace failed: {e}"));
    }
    Ok(digest)
}

fn emit_status<R: Runtime>(app: &AppHandle<R>, status: &str, key: &str, progress: Option<u8>, phase: &str) {
    let _ = app.emit(
        "voice:status",
        serde_json::json!({ "status": status, "voice_key": key, "progress": progress, "phase": phase }),
    );
}

/// Stream `url` into `tmp`, reporting 0..=100 progress (throttled to ~10% steps).
async fn download<R: Runtime>(
    app: &AppHandle<R>,
    client: &reqwest::Client,
    url: &str,
    tmp: &Path,
    key: &str,
    phase: &str,
) -> Result<(), String> {
    use tokio::io::AsyncWriteExt;
    let mut resp = client.get(url).send().await.map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!("HTTP {}", resp.status()));
    }
    let total = resp.content_length().unwrap_or(0);
    let mut file = tokio::fs::File::create(tmp).await.map_err(|e| e.to_string())?;
    let (mut done, mut last) = (0u64, 0u8);
    while let Some(chunk) = resp.chunk().await.map_err(|e| e.to_string())? {
        file.write_all(&chunk).await.map_err(|e| e.to_string())?;
        done += chunk.len() as u64;
        if total > 0 {
            let pct = ((done * 100) / total) as u8;
            if pct >= last + 10 {
                last = pct;
                emit_status(app, "downloading", key, Some(pct), phase);
            }
        }
    }
    file.flush().await.map_err(|e| e.to_string())?;
    if done == 0 {
        return Err("download was empty".to_string());
    }
    Ok(())
}

/// Trial-load the model so a truncated-but-hash-matching or corrupt file is caught before install.
fn trial_load_model(path: &Path) -> Result<(), String> {
    ort::session::Session::builder()
        .map_err(|e| e.to_string())?
        .commit_from_file(path)
        .map(|_| ())
        .map_err(|e| format!("trial load failed: {e}"))
}

/// One sync job. Never panics; any failure emits `error`, cleans its `.tmp` files and leaves the
/// previously installed voice/model fully intact.
async fn run_one<R: Runtime>(app: &AppHandle<R>, voice_key: &str, engine: &crate::tts_kokoro::KokoroEngine) {
    let Some(persona) = crate::voice_catalog::find_by_key(voice_key) else { return };
    let voice = persona.kokoro_voice;
    let Some(dir) = crate::voice_catalog::managed_voices_dir() else {
        emit_status(app, "error", voice_key, None, "voice");
        return;
    };
    if std::fs::create_dir_all(&dir).is_err() {
        emit_status(app, "error", voice_key, None, "voice");
        return;
    }
    let removed = crate::voice_catalog::cleanup_legacy_slot_at(&dir);
    if removed > 0 {
        tracing::info!("tts-swap: removed {removed} legacy Piper slot file(s)");
    }

    let manifest = crate::voice_catalog::read_manifest_at(&dir);
    let plan = plan_swap(
        crate::voice_catalog::base_present_at(&dir),
        manifest.as_ref().map(|m| m.kokoro_voice.as_str()),
        dir.join(crate::voice_catalog::VOICE_FILE).is_file(),
        voice,
    );
    if plan == SwapPlan::Nothing {
        emit_status(app, "ready", voice_key, Some(100), "voice");
        return;
    }
    let Some(voice_url) = voice_url(voice) else {
        tracing::warn!("tts-swap: '{voice}' is not a valid Kokoro voice name");
        emit_status(app, "error", voice_key, None, "voice");
        return;
    };

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(600))
        .build()
        .unwrap_or_default();
    let (tmp_model, tmp_voice) = (dir.join(TMP_MODEL), dir.join(TMP_VOICE));
    let cleanup = || {
        let _ = std::fs::remove_file(&tmp_model);
        let _ = std::fs::remove_file(&tmp_voice);
    };

    emit_status(app, "downloading", voice_key, Some(0), if plan == SwapPlan::BaseAndVoice { "base" } else { "voice" });

    // 1. shared model (first sync only)
    if plan == SwapPlan::BaseAndVoice {
        if let Err(e) = download(app, &client, &model_url(), &tmp_model, voice_key, "base").await {
            tracing::warn!("tts-swap: model download failed: {e}");
            cleanup();
            emit_status(app, "error", voice_key, None, "base");
            return;
        }
        // trial-load the temp file first, then verify + atomically install it
        let t = tmp_model.clone();
        let trial = tokio::task::spawn_blocking(move || trial_load_model(&t)).await;
        if !matches!(trial, Ok(Ok(()))) {
            tracing::warn!("tts-swap: model trial load failed: {trial:?}");
            cleanup();
            emit_status(app, "error", voice_key, None, "base");
            return;
        }
        if let Err(e) = verify_and_install(
            &tmp_model,
            &dir.join(crate::voice_catalog::MODEL_FILE),
            Some(MODEL_BYTES),
            Some(MODEL_SHA256),
        ) {
            tracing::warn!("tts-swap: model verify failed: {e}");
            cleanup();
            emit_status(app, "error", voice_key, None, "base");
            return;
        }
    }

    // 2. the voice pack (always)
    if let Err(e) = download(app, &client, &voice_url, &tmp_voice, voice_key, "voice").await {
        tracing::warn!("tts-swap: voice download failed for '{voice}': {e}");
        cleanup();
        emit_status(app, "error", voice_key, None, "voice");
        return;
    }
    // parse check = trial load for a voice pack
    let parsed = std::fs::read(&tmp_voice)
        .map_err(|e| e.to_string())
        .and_then(|b| crate::tts_kokoro::parse_voice_pack(&b).map(|_| ()));
    if let Err(e) = parsed {
        tracing::warn!("tts-swap: voice pack invalid for '{voice}': {e}");
        cleanup();
        emit_status(app, "error", voice_key, None, "voice");
        return;
    }
    let voice_sha = match verify_and_install(
        &tmp_voice,
        &dir.join(crate::voice_catalog::VOICE_FILE),
        Some(VOICE_BYTES),
        expected_voice_sha(voice),
    ) {
        Ok(h) => h,
        Err(e) => {
            tracing::warn!("tts-swap: voice verify failed for '{voice}': {e}");
            cleanup();
            emit_status(app, "error", voice_key, None, "voice");
            return;
        }
    };

    // 3. manifest (records exactly what is now installed)
    let model_sha = manifest
        .as_ref()
        .map(|m| m.model_sha256.clone())
        .filter(|s| !s.is_empty() && plan == SwapPlan::VoiceOnly)
        .unwrap_or_else(|| MODEL_SHA256.to_string());
    let new_manifest = crate::voice_catalog::VoiceManifest {
        voice_key: voice_key.to_string(),
        kokoro_voice: voice.to_string(),
        model_sha256: model_sha,
        voice_sha256: voice_sha,
        version: 2,
    };
    if crate::voice_catalog::write_manifest_at(&dir, &new_manifest).is_err() {
        tracing::warn!("tts-swap: manifest write failed");
        emit_status(app, "error", voice_key, None, "voice");
        return;
    }

    // 4. hot-swap the voice in a loaded engine (a not-yet-loaded engine picks it up lazily). Applied
    // between utterances: a reply that is being spoken right now finishes in its own voice.
    if crate::tts_kokoro::is_loaded(engine).await {
        wait_for_utterance_boundary().await;
        let pack = dir.join(crate::voice_catalog::VOICE_FILE);
        if let Err(e) = crate::tts_kokoro::set_voice(engine, &pack, voice).await {
            tracing::warn!("tts-swap: engine hot-swap failed for '{voice}': {e}");
        }
    }
    tracing::info!("tts-swap: offline voice '{voice}' ready for persona '{voice_key}'");
    emit_status(app, "ready", voice_key, Some(100), "voice");
}

/// Request that the offline voice for `key` be installed in the background. Latest request wins;
/// returns immediately. Safe to call repeatedly (e.g. at startup and when the cloud returns).
pub fn request_swap<R: Runtime>(app: AppHandle<R>, key: String, engine: crate::tts_kokoro::KokoroEngine) {
    let start_worker = QUEUE.lock().map(|mut q| q.request(&key)).unwrap_or(false);
    if !start_worker {
        tracing::info!("tts-swap: worker busy — '{key}' queued (latest wins)");
        return;
    }
    tauri::async_runtime::spawn(async move {
        loop {
            let next = QUEUE.lock().ok().and_then(|mut q| q.take());
            if let Some(k) = next {
                set_current(Some(k.clone()));
                run_one(&app, &k, &engine).await;
            }
            let again = QUEUE.lock().map(|mut q| q.keep_going()).unwrap_or(false);
            if !again {
                set_current(None);
                break;
            }
        }
    });
}

/// Longest a swap waits for a reply to finish before applying anyway (a stuck flag must not block it).
const BOUNDARY_WAIT_MAX_MS: u64 = 120_000;

/// Wait until no speech is in flight (bounded). The swap itself is a few microseconds of pointer
/// replacement under the engine lock, so applying it in the gap between two replies is invisible.
async fn wait_for_utterance_boundary() {
    let started = std::time::Instant::now();
    while crate::tts::speech_active() && (started.elapsed().as_millis() as u64) < BOUNDARY_WAIT_MAX_MS {
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
}

/// Make sure the CURRENTLY SELECTED persona's offline voice is installed (startup, cloud restored).
/// No-op when it already is.
pub fn sync_selected_voice<R: Runtime>(app: &AppHandle<R>, engine: crate::tts_kokoro::KokoroEngine) {
    let key = crate::commands::read_selected_voice(app);
    if let Some(p) = crate::voice_catalog::find_by_key(&key) {
        crate::tts_kokoro::set_preferred_voice(p.kokoro_voice);
    }
    if crate::voice_catalog::sync_state_for(&key) == crate::voice_catalog::VoiceSyncState::Ready {
        return;
    }
    request_swap(app.clone(), key, engine);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(tag: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("nexus_swap_{tag}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn urls_are_pinned_to_the_revision_not_main() {
        let m = model_url();
        assert!(m.contains(KOKORO_REV) && !m.contains("/main/"));
        assert!(m.ends_with("onnx/model_quantized.onnx"));
        assert_eq!(
            voice_url("bm_george").unwrap(),
            format!("{HF_BASE}/{KOKORO_REV}/voices/bm_george.bin")
        );
    }

    #[test]
    fn voice_name_validation_blocks_path_tricks() {
        for ok in ["af_heart", "bm_george", "am_michael", "bf_emma"] {
            assert!(is_valid_voice_name(ok) && voice_url(ok).is_some(), "{ok}");
        }
        for bad in ["", "af_", "zf_xiaoxiao", "../af_heart", "af_heart/../x", "AF_HEART", "af-heart", "af_heart.bin", "a", "af_h3art"] {
            assert!(!is_valid_voice_name(bad) && voice_url(bad).is_none(), "{bad}");
        }
    }

    #[test]
    fn every_catalog_voice_has_a_valid_url() {
        for p in crate::voice_catalog::VOICE_CATALOG {
            assert!(voice_url(p.kokoro_voice).is_some(), "no URL for {}", p.kokoro_voice);
        }
    }

    #[test]
    fn pinned_hashes_are_wellformed_and_unique() {
        let mut seen = std::collections::HashSet::new();
        for (n, h) in VOICE_SHA256 {
            assert!(is_valid_voice_name(n));
            assert_eq!(h.len(), 64, "{n}");
            assert!(h.chars().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()), "{n}");
            assert!(seen.insert(*h), "duplicate hash for {n}");
        }
        assert_eq!(MODEL_SHA256.len(), 64);
        assert_eq!(expected_voice_sha("af_heart").map(str::len), Some(64));
        // a voice outside the catalog has no pin (size + parse checks still apply)
        assert_eq!(expected_voice_sha("am_adam"), None);
    }

    #[test]
    fn every_catalog_voice_is_hash_pinned() {
        for p in crate::voice_catalog::VOICE_CATALOG {
            assert!(
                expected_voice_sha(p.kokoro_voice).is_some(),
                "catalog voice {} has no pinned SHA-256",
                p.kokoro_voice
            );
        }
    }

    #[test]
    fn plan_matrix() {
        // wanted voice already installed
        assert_eq!(plan_swap(true, Some("bm_george"), true, "bm_george"), SwapPlan::Nothing);
        // other voice installed, model present => 0.5 MB swap only
        assert_eq!(plan_swap(true, Some("af_heart"), true, "bm_george"), SwapPlan::VoiceOnly);
        // manifest says it but the file vanished => refetch the voice
        assert_eq!(plan_swap(true, Some("bm_george"), false, "bm_george"), SwapPlan::VoiceOnly);
        // empty manifest / legacy slot
        assert_eq!(plan_swap(true, None, false, "bm_george"), SwapPlan::VoiceOnly);
        // first-ever sync
        assert_eq!(plan_swap(false, None, false, "bm_george"), SwapPlan::BaseAndVoice);
        assert_eq!(plan_swap(false, Some("bm_george"), true, "bm_george"), SwapPlan::BaseAndVoice);
    }

    #[test]
    fn latest_wins_a_b_c() {
        let mut q = SwapQueue::new();
        assert!(q.request("a"), "first request starts the worker");
        assert_eq!(q.take().as_deref(), Some("a")); // worker begins A
        // while A downloads the user clicks B then C
        assert!(!q.request("b"), "worker already running");
        assert!(!q.request("c"), "worker already running");
        assert!(q.keep_going(), "a newer target arrived");
        assert_eq!(q.take().as_deref(), Some("c"), "B was superseded; the slot ends on C");
        assert!(!q.keep_going(), "queue drained");
        // worker has exited: the next request starts a new one
        assert!(q.request("d"));
    }

    #[test]
    fn queue_idle_take_is_none() {
        let mut q = SwapQueue::new();
        assert_eq!(q.take(), None);
        assert!(!q.keep_going());
    }

    #[test]
    fn verify_and_install_replaces_atomically_on_success() {
        let dir = tmp("ok");
        let dest = dir.join("active_voice.bin");
        std::fs::write(&dest, b"OLD VOICE").unwrap();
        let t = dir.join("voice_download.tmp");
        std::fs::write(&t, b"abc").unwrap();
        let sha = verify_and_install(
            &t,
            &dest,
            Some(3),
            Some("ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"),
        )
        .unwrap();
        assert_eq!(sha, "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
        assert_eq!(std::fs::read(&dest).unwrap(), b"abc");
        assert!(!t.exists(), "tmp consumed");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn verify_and_install_failure_keeps_old_file_and_leaves_no_tmp() {
        let dir = tmp("fail");
        let dest = dir.join("active_voice.bin");
        let t = dir.join("voice_download.tmp");
        for (bytes, sha, label) in [
            (Some(99u64), None, "wrong size"),
            (Some(3), Some("0000000000000000000000000000000000000000000000000000000000000000"), "wrong hash"),
        ] {
            std::fs::write(&dest, b"OLD VOICE").unwrap();
            std::fs::write(&t, b"abc").unwrap();
            assert!(verify_and_install(&t, &dest, bytes, sha).is_err(), "{label}");
            assert_eq!(std::fs::read(&dest).unwrap(), b"OLD VOICE", "{label}: old voice must survive");
            assert!(!t.exists(), "{label}: orphan tmp left behind");
        }
        // missing download
        assert!(verify_and_install(&t, &dest, None, None).is_err());
        assert_eq!(std::fs::read(&dest).unwrap(), b"OLD VOICE");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn sha256_known_vector() {
        let dir = tmp("sha");
        let p = dir.join("abc.txt");
        std::fs::write(&p, b"abc").unwrap();
        assert_eq!(
            sha256_file(&p).unwrap(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The assets we pinned must match what is on disk in the dev download (skipped when absent).
    #[test]
    fn pinned_hashes_match_the_downloaded_dev_assets() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("resources/kokoro");
        let model = root.join("model_quantized.onnx");
        if !model.exists() {
            eprintln!("dev kokoro assets absent — skipping");
            return;
        }
        assert_eq!(std::fs::metadata(&model).unwrap().len(), MODEL_BYTES);
        assert_eq!(sha256_file(&model).unwrap(), MODEL_SHA256);
        for (name, sha) in VOICE_SHA256 {
            let p = root.join("voices").join(format!("{name}.bin"));
            if p.exists() {
                assert_eq!(std::fs::metadata(&p).unwrap().len(), VOICE_BYTES, "{name}");
                assert_eq!(sha256_file(&p).unwrap(), *sha, "{name}");
            }
        }
    }
}
