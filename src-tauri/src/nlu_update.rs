//! NLU model update checker — family devices pull admin-trained BERT-Mini.
//!
//! The admin retrains the NLU model and publishes it via
//! `server/nlu/publish_nlu.py` (files → R2, manifest → Worker KV).
//! Family devices call `check_nlu_update()` once on startup:
//!
//!   1. GET {worker}/models/nlu/latest  → manifest {version, files{sha256,size}}
//!   2. Compare manifest version vs local version.txt
//!   3. For each file whose sha256 differs: GET /models/nlu/download?name=
//!   4. Verify sha256 of each downloaded file
//!   5. Write to app data `nlu_model/` dir
//!   6. If NLU server is running: POST /reload_model (hot-swap, no restart)
//!
//! Everything is additive and non-blocking: if the Worker has no manifest,
//! R2 is unconfigured, or the download fails, the bundled model is used
//! exactly as before.
//!
//! A file-level manifest (not a zip) is used so updates resume per-file
//! and no archive dependency is needed.

use serde::Deserialize;
use sha2::{Digest, Sha256};
use tauri::Manager;
use std::collections::BTreeMap;
use std::path::PathBuf;

/// NLU server port — must match lazy_nlu.rs.
const NLU_PORT: u16 = 39218;

/// Manifest returned by GET /models/nlu/latest.
#[derive(Debug, Deserialize)]
struct ModelManifest {
    version: String,
    #[allow(dead_code)]
    updated_at: Option<String>,
    files: BTreeMap<String, FileMeta>,
}

#[derive(Debug, Deserialize)]
struct FileMeta {
    sha256: String,
    #[allow(dead_code)]
    size: u64,
}

/// Directory where downloaded model files live (under app data).
fn model_dir(app: &tauri::AppHandle) -> Option<PathBuf> {
    app.path()
        .app_data_dir()
        .ok()
        .map(|d| d.join("nlu_model"))
}

/// Whether a candidate dir holds a usable downloaded model.
fn usable_model_dir(dir: PathBuf) -> Option<PathBuf> {
    if dir.join("nexus_nlu.onnx").exists() && installed_version(&dir).is_some() {
        Some(dir)
    } else {
        None
    }
}

/// Same as `model_dir` but without an `AppHandle` — derives the app
/// data dir from environment the same way `lazy_stt.rs` does. Used by
/// `lazy_nlu` at spawn time (no AppHandle in that call path).
pub fn downloaded_model_dir_envless() -> Option<PathBuf> {
    let base = if let Ok(appdata) = std::env::var("APPDATA") {
        PathBuf::from(appdata).join("com.nexus.assistant")
    } else if let Ok(home) = std::env::var("HOME") {
        PathBuf::from(home).join(".config").join("com.nexus.assistant")
    } else {
        return None;
    };
    usable_model_dir(base.join("nlu_model"))
}

/// The installed model version, if an update was previously downloaded.
fn installed_version(dir: &PathBuf) -> Option<String> {
    let v = std::fs::read_to_string(dir.join("version.txt")).ok()?;
    let v = v.trim().to_string();
    if v.is_empty() {
        None
    } else {
        Some(v)
    }
}

/// Whether the downloaded model dir is usable (ONNX present).
pub fn downloaded_model_dir(app: &tauri::AppHandle) -> Option<PathBuf> {
    usable_model_dir(model_dir(app)?)
}

/// Resolve the Worker base URL from the active session.
/// Session is auto-opened from nexus-config.json at startup, so this
/// works without a settings round-trip.
fn worker_url() -> Option<String> {
    crate::network::get_session_info()
        .map(|(url, _, _)| url.trim_end_matches('/').to_string())
        .filter(|u| !u.is_empty())
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(bytes);
    format!("{:x}", h.finalize())
}

/// Spawn a background update check. Call once at app startup.
/// Non-blocking: runs on the tokio runtime, never delays the pipeline.
pub fn spawn_update_check(app: tauri::AppHandle) {
    tauri::async_runtime::spawn(async move {
        if let Err(e) = check_and_update(&app).await {
            tracing::debug!("[nlu_update] check skipped/failed: {}", e);
        }
    });
}

async fn check_and_update(app: &tauri::AppHandle) -> Result<(), String> {
    let base = worker_url().ok_or("no active Worker session")?;
    let dir = model_dir(app).ok_or("no app data dir")?;

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(120))
        .build()
        .map_err(|e| e.to_string())?;

    // 1. Fetch manifest.
    let manifest: ModelManifest = client
        .get(format!("{}/models/nlu/latest", base))
        .send()
        .await
        .map_err(|e| format!("manifest fetch: {}", e))?
        .json()
        .await
        .map_err(|e| format!("manifest parse: {}", e))?;

    // 2. Compare against installed version.
    if installed_version(&dir).as_deref() == Some(manifest.version.as_str()) {
        tracing::debug!("[nlu_update] already at version {}", manifest.version);
        return Ok(());
    }

    tracing::info!(
        "[nlu_update] new NLU model available: {} ({} files)",
        manifest.version,
        manifest.files.len()
    );

    // 3. Download each file to a staging dir, verify sha256.
    let staging = dir.join(".staging");
    std::fs::create_dir_all(&staging).map_err(|e| e.to_string())?;

    for (name, meta) in &manifest.files {
        let bytes = client
            .get(format!("{}/models/nlu/download", base))
            .query(&[("name", name.as_str())])
            .send()
            .await
            .map_err(|e| format!("download {}: {}", name, e))?
            .bytes()
            .await
            .map_err(|e| format!("read {}: {}", name, e))?;

        let actual = sha256_hex(&bytes);
        if actual != meta.sha256 {
            let _ = std::fs::remove_dir_all(&staging);
            return Err(format!("sha256 mismatch for {} ({} != {})", name, actual, meta.sha256));
        }

        let dest = staging.join(name);
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        std::fs::write(&dest, &bytes).map_err(|e| e.to_string())?;
    }

    // 4. Verify required files, then swap staging → live atomically-ish.
    if !staging.join("nexus_nlu.onnx").exists() {
        let _ = std::fs::remove_dir_all(&staging);
        return Err("manifest missing nexus_nlu.onnx".to_string());
    }
    std::fs::write(staging.join("version.txt"), &manifest.version)
        .map_err(|e| e.to_string())?;

    // Move staged files into the live dir (file-level swap keeps the old
    // model loadable if the process is killed mid-swap).
    for entry in std::fs::read_dir(&staging).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let name = entry.file_name();
        let dest = dir.join(&name);
        if entry.path().is_dir() {
            // tokenizer/ — merge file by file
            let dest_sub = dir.join(&name);
            std::fs::create_dir_all(&dest_sub).map_err(|e| e.to_string())?;
            for sub in std::fs::read_dir(entry.path()).map_err(|e| e.to_string())? {
                let sub = sub.map_err(|e| e.to_string())?;
                std::fs::rename(sub.path(), dest_sub.join(sub.file_name()))
                    .map_err(|e| e.to_string())?;
            }
        } else {
            if dest.exists() {
                std::fs::remove_file(&dest).map_err(|e| e.to_string())?;
            }
            std::fs::rename(entry.path(), &dest).map_err(|e| e.to_string())?;
        }
    }
    let _ = std::fs::remove_dir_all(&staging);

    tracing::info!("[nlu_update] NLU model updated to {}", manifest.version);

    // 5. Hot-swap if the NLU server is already running.
    if nlu_server_running() {
        match client
            .post(format!("http://127.0.0.1:{}/reload_model", NLU_PORT))
            .send()
            .await
        {
            Ok(resp) if resp.status().is_success() => {
                tracing::info!("[nlu_update] hot-swapped model in running NLU server");
            }
            Ok(resp) => {
                tracing::warn!("[nlu_update] reload_model returned {}", resp.status());
            }
            Err(e) => {
                tracing::warn!("[nlu_update] reload_model failed: {}", e);
            }
        }
    }

    Ok(())
}

fn nlu_server_running() -> bool {
    use std::net::TcpStream;
    use std::time::Duration as TcpDuration;
    let addr = format!("127.0.0.1:{}", NLU_PORT);
    TcpStream::connect_timeout(
        &addr.parse().unwrap(),
        TcpDuration::from_millis(200),
    )
    .is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sha256_hex_known() {
        // sha256("abc") is a well-known digest
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn test_manifest_deserialize() {
        let json = r#"{
            "version": "2026-09-17-01",
            "updated_at": "2026-09-17T10:00:00Z",
            "files": {
                "nexus_nlu.onnx": {"sha256": "abc", "size": 17176},
                "labels.json": {"sha256": "def", "size": 4}
            }
        }"#;
        let m: ModelManifest = serde_json::from_str(json).unwrap();
        assert_eq!(m.version, "2026-09-17-01");
        assert_eq!(m.files.len(), 2);
        assert!(m.files.contains_key("nexus_nlu.onnx"));
        assert_eq!(m.files["labels.json"].size, 4);
    }

    #[test]
    fn test_installed_version_missing_dir() {
        let dir = PathBuf::from("C:/nonexistent-nlu-test-dir");
        assert_eq!(installed_version(&dir), None);
    }

    #[test]
    fn test_installed_version_roundtrip() {
        let dir = std::env::temp_dir().join(format!("nlu-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("version.txt"), "v1.2.3\n").unwrap();
        assert_eq!(installed_version(&dir), Some("v1.2.3".to_string()));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
