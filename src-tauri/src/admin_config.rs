//! Admin config — runtime check for admin-only features.
//!
//! The admin config file lives at `%APPDATA%/com.nexus.assistant/admin.json`
//! and is created manually by the admin. If it doesn't exist or
//! `is_admin` is false, all brain calls are no-ops.
//!
//! This is the runtime gate. The compile-time gate is the `admin-brain`
//! Cargo feature. Both must be active for the brain to run:
//!   1. `cargo build --features admin-brain` (compile-time)
//!   2. `admin.json` with `is_admin: true` (runtime)

#![allow(dead_code)]

use serde::Deserialize;
use std::path::PathBuf;
use std::sync::OnceLock;

/// Admin config file format.
#[derive(Debug, Deserialize, Clone)]
pub struct AdminConfig {
    pub is_admin: bool,
    pub brain_enabled: bool,
    #[serde(default = "default_brain_port")]
    pub brain_port: u16,
    #[serde(default = "default_brain_model")]
    pub brain_model: String,
    #[serde(default = "default_auto_train")]
    pub auto_train: bool,
    #[serde(default = "default_retrain_threshold")]
    pub retrain_threshold: u32,
    #[serde(default = "default_min_confidence")]
    pub min_confidence: f32,
}

fn default_brain_port() -> u16 { 39219 }
fn default_brain_model() -> String { "qwen2.5-0.5b-instruct-q4_k_m.gguf".to_string() }
fn default_auto_train() -> bool { true }
fn default_retrain_threshold() -> u32 { 50 }
fn default_min_confidence() -> f32 { 0.90 }

/// Cached admin config (loaded once, reused).
static ADMIN_CONFIG: OnceLock<Option<AdminConfig>> = OnceLock::new();

/// Get the path to the admin config file.
/// Checks two locations:
///   1. %APPDATA%/com.nexus.assistant/admin.json (production)
///   2. server/admin/admin_config.json (dev fallback)
fn admin_config_path() -> PathBuf {
    let base = dirs_next::data_dir().unwrap_or_else(|| PathBuf::from("."));
    let prod_path = base.join("com.nexus.assistant").join("admin.json");
    if prod_path.exists() {
        return prod_path;
    }
    // Dev fallback: look for server/admin/admin_config.json relative to CWD
    let dev_path = PathBuf::from("server").join("admin").join("admin_config.json");
    if dev_path.exists() {
        tracing::info!("[admin] using dev admin config at {:?}", dev_path);
        return dev_path;
    }
    // Return the prod path even if it doesn't exist (so the error message is helpful)
    prod_path
}

/// Load the admin config from disk. Returns None if the file doesn't
/// exist or can't be parsed.
fn load_admin_config() -> Option<AdminConfig> {
    let path = admin_config_path();
    if !path.exists() {
        return None;
    }
    let contents = std::fs::read_to_string(&path).ok()?;
    serde_json::from_str(&contents).ok()
}

/// Check if this machine is the admin's machine.
///
/// Returns true only if:
///   1. The `admin-brain` Cargo feature is enabled (compile-time)
///   2. The admin.json config file exists with `is_admin: true` (runtime)
///
/// This is checked once and cached for the lifetime of the process.
pub fn is_admin() -> bool {
    // Compile-time gate: if admin-brain feature is not enabled,
    // this function always returns false.
    #[cfg(not(feature = "admin-brain"))]
    {
        return false;
    }

    #[cfg(feature = "admin-brain")]
    {
        let config = ADMIN_CONFIG.get_or_init(|| {
            let cfg = load_admin_config();
            if let Some(ref c) = cfg {
                tracing::info!(
                    "[admin] Admin config loaded: is_admin={}, brain_enabled={}",
                    c.is_admin,
                    c.brain_enabled
                );
            } else {
                tracing::debug!("[admin] No admin config found — brain disabled");
            }
            cfg
        });
        config.as_ref().map(|c| c.is_admin && c.brain_enabled).unwrap_or(false)
    }
}

/// Get the admin config (if loaded). Returns None if not admin.
pub fn get_admin_config() -> Option<&'static AdminConfig> {
    if !is_admin() {
        return None;
    }
    ADMIN_CONFIG.get().and_then(|c| c.as_ref())
}

/// Get the brain port from admin config (default 39219).
pub fn brain_port() -> u16 {
    get_admin_config()
        .map(|c| c.brain_port)
        .unwrap_or(39219)
}

/// Get the retrain threshold from admin config (default 50).
pub fn retrain_threshold() -> u32 {
    get_admin_config()
        .map(|c| c.retrain_threshold)
        .unwrap_or(50)
}

/// Get the minimum confidence for auto-approval (default 0.90).
pub fn min_confidence() -> f32 {
    get_admin_config()
        .map(|c| c.min_confidence)
        .unwrap_or(0.90)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_admin_without_config() {
        // Without admin.json, is_admin should be false
        // (or true if admin-brain feature is enabled AND config exists)
        // This test just verifies it doesn't panic.
        let _ = is_admin();
    }

    #[test]
    fn test_brain_port_default() {
        let port = brain_port();
        assert_eq!(port, 39219);
    }

    #[test]
    fn test_retrain_threshold_default() {
        let threshold = retrain_threshold();
        assert_eq!(threshold, 50);
    }

    #[test]
    fn test_min_confidence_default() {
        let conf = min_confidence();
        assert!((conf - 0.90).abs() < 0.001);
    }
}
