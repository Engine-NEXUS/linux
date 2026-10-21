//! API-key management for the Command Hub "API Keys" page: list (masked),
//! add / replace, delete. Keys live in the OS keychain (`auth_vault`) and are
//! mirrored into settings.json because several readers (STT sidecar launcher,
//! Telegram bridge, wake verifier) still read the file directly; delete clears
//! BOTH, so a deleted key cannot linger in either place. A full key is never
//! returned to the frontend — only a masked tail.

use serde::Serialize;
use tauri::{AppHandle, Manager, Runtime};

/// (service id, display label, settings.json camelCase field)
pub const KEY_SERVICES: &[(&str, &str, &str)] = &[
    ("groq", "Groq", "groqApiKey"),
    ("gemini", "Gemini", "geminiApiKey"),
    ("cerebras", "Cerebras", "cerebrasApiKey"),
    ("deepgram", "Deepgram", "deepgramApiKey"),
    ("google_maps", "Google Maps & Routes", "routesApiKey"),
    ("google_places", "Google Places", "placesApiKey"),
];

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ApiKeyInfo {
    pub service: String,
    pub label: String,
    pub has_key: bool,
    /// e.g. `••••••••a1b2` — never the key itself.
    pub masked: String,
}

pub fn service_known(service: &str) -> bool {
    KEY_SERVICES.iter().any(|(s, _, _)| *s == service)
}

fn field_for(service: &str) -> Option<&'static str> {
    KEY_SERVICES.iter().find(|(s, _, _)| *s == service).map(|(_, _, f)| *f)
}

/// Mask a key: bullets plus the last 4 chars (only when the key is long
/// enough that 4 chars reveal nothing useful). Pure.
pub fn mask(key: &str) -> String {
    let chars: Vec<char> = key.chars().collect();
    if chars.is_empty() {
        return String::new();
    }
    if chars.len() < 12 {
        return "••••••••".to_string();
    }
    let tail: String = chars[chars.len() - 4..].iter().collect();
    format!("••••••••{tail}")
}

/// Structural validation for a pasted key (providers change their formats,
/// so no prefix is enforced). Pure.
pub fn validate_key(key: &str) -> Result<String, String> {
    let k = key.trim();
    if k.is_empty() {
        return Err("The key is empty.".to_string());
    }
    if k.chars().any(|c| c.is_whitespace()) {
        return Err("A key cannot contain spaces or line breaks.".to_string());
    }
    if k.len() < 16 {
        return Err("That looks too short to be an API key.".to_string());
    }
    if k.len() > 300 {
        return Err("That looks too long to be an API key.".to_string());
    }
    Ok(k.to_string())
}

/// Set (or clear, with `None`) one key field in settings.json text. Unknown
/// fields are preserved. Pure over the JSON value.
pub fn apply_key_field(json: &mut serde_json::Value, service: &str, key: Option<&str>) {
    let Some(field) = field_for(service) else { return };
    if !json.is_object() {
        *json = serde_json::json!({});
    }
    json[field] = serde_json::Value::String(key.unwrap_or("").to_string());
    // Legacy snake_case copy must not keep a deleted key alive.
    let snake = match service {
        "gemini" => "gemini_api_key",
        "cerebras" => "cerebras_api_key",
        "deepgram" => "deepgram_api_key",
        _ => "groq_api_key",
    };
    if let Some(obj) = json.as_object_mut() {
        if key.is_none() {
            obj.remove(snake);
        } else {
            obj.insert(snake.to_string(), serde_json::Value::String(key.unwrap_or("").to_string()));
        }
    }
}

/// `save_settings` must never delete or resurrect keys: an EMPTY incoming key
/// keeps what is on disk (deletion happens only through `api_key_delete`).
/// Returns the value to persist for a field. Pure.
pub fn merge_key(existing: &serde_json::Value, field: &str, incoming: &str) -> String {
    if !incoming.is_empty() {
        return incoming.to_string();
    }
    existing
        .get(field)
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string()
}

fn settings_path<R: Runtime>(app: &AppHandle<R>) -> Result<std::path::PathBuf, String> {
    let dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir.join("settings.json"))
}

fn mutate_settings_file<R: Runtime>(
    app: &AppHandle<R>,
    f: impl FnOnce(&mut serde_json::Value),
) -> Result<(), String> {
    let path = settings_path(app)?;
    let mut json = std::fs::read_to_string(&path)
        .ok()
        .and_then(|c| serde_json::from_str::<serde_json::Value>(&c).ok())
        .unwrap_or_else(|| serde_json::json!({}));
    f(&mut json);
    std::fs::write(&path, serde_json::to_string_pretty(&json).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())
}

/// IPC: which keys exist (masked).
#[tauri::command]
pub fn api_keys_list<R: Runtime>(app: AppHandle<R>) -> Vec<ApiKeyInfo> {
    KEY_SERVICES
        .iter()
        .map(|(service, label, _)| {
            let key = crate::commands::read_api_key(&app, service);
            ApiKeyInfo {
                service: service.to_string(),
                label: label.to_string(),
                has_key: !key.is_empty(),
                masked: mask(&key),
            }
        })
        .collect()
}

/// IPC: add or replace a key.
#[tauri::command]
pub fn api_key_set<R: Runtime>(app: AppHandle<R>, service: String, key: String) -> Result<ApiKeyInfo, String> {
    if !service_known(&service) {
        return Err(format!("unknown service: {service}"));
    }
    let clean = validate_key(&key)?;
    crate::auth_vault::set_api_key(&service, &clean);
    mutate_settings_file(&app, |j| apply_key_field(j, &service, Some(&clean)))?;
    tracing::info!("api_keys: {service} key saved");
    let label = KEY_SERVICES.iter().find(|(s, _, _)| *s == service).map(|(_, l, _)| *l).unwrap_or("");
    Ok(ApiKeyInfo {
        service,
        label: label.to_string(),
        has_key: true,
        masked: mask(&clean),
    })
}

/// IPC: delete a key from the keychain AND settings.json.
#[tauri::command]
pub fn api_key_delete<R: Runtime>(app: AppHandle<R>, service: String) -> Result<(), String> {
    if !service_known(&service) {
        return Err(format!("unknown service: {service}"));
    }
    crate::auth_vault::clear_api_key(&service);
    mutate_settings_file(&app, |j| apply_key_field(j, &service, None))?;
    tracing::info!("api_keys: {service} key deleted");
    Ok(())
}

/// Number of configured keys (Command Hub insight).
pub fn count_keys<R: Runtime>(app: &AppHandle<R>) -> usize {
    KEY_SERVICES
        .iter()
        .filter(|(s, _, _)| !crate::commands::read_api_key(app, s).is_empty())
        .count()
}

/// IPC: number of configured API keys.
#[tauri::command]
pub fn api_keys_count<R: Runtime>(app: AppHandle<R>) -> usize {
    count_keys(&app)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mask_never_reveals_more_than_four_chars() {
        assert_eq!(mask(""), "");
        assert_eq!(mask("short"), "••••••••");
        assert_eq!(mask("AIzaSyA-1234567890abcdefXYZ9"), "••••••••XYZ9");
        assert!(!mask("gsk_1234567890abcdef").contains("1234"));
    }

    #[test]
    fn validation() {
        assert!(validate_key("").is_err());
        assert!(validate_key("   ").is_err());
        assert!(validate_key("has space inside 1234567890").is_err());
        assert!(validate_key("short").is_err());
        assert!(validate_key(&"x".repeat(301)).is_err());
        assert_eq!(validate_key("  AIzaSyA-1234567890abcdef  ").unwrap(), "AIzaSyA-1234567890abcdef");
    }

    #[test]
    fn set_and_delete_edit_only_their_field() {
        let mut j = serde_json::json!({"orbColor": "#fff", "groqApiKey": "old", "groq_api_key": "old", "geminiApiKey": "g"});
        apply_key_field(&mut j, "groq", Some("NEWKEY0123456789"));
        assert_eq!(j["groqApiKey"], "NEWKEY0123456789");
        assert_eq!(j["geminiApiKey"], "g");
        assert_eq!(j["orbColor"], "#fff");
        apply_key_field(&mut j, "groq", None);
        assert_eq!(j["groqApiKey"], "");
        assert!(j.get("groq_api_key").is_none()); // legacy copy cannot keep it alive
        assert_eq!(j["geminiApiKey"], "g");
        // Unknown service is a no-op.
        let before = j.clone();
        apply_key_field(&mut j, "nope", Some("x"));
        assert_eq!(j, before);
        // Non-object JSON heals into an object.
        let mut bad = serde_json::json!(null);
        apply_key_field(&mut bad, "gemini", Some("K0123456789012345"));
        assert_eq!(bad["geminiApiKey"], "K0123456789012345");
    }

    #[test]
    fn save_settings_cannot_delete_or_resurrect_via_empty_payload() {
        let disk = serde_json::json!({"groqApiKey": "ondisk"});
        // Empty incoming keeps the disk value (no accidental wipe)…
        assert_eq!(merge_key(&disk, "groqApiKey", ""), "ondisk");
        // …a non-empty incoming key (setup wizard) wins…
        assert_eq!(merge_key(&disk, "groqApiKey", "fromwizard"), "fromwizard");
        // …and after api_key_delete the disk is empty, so an empty payload stays empty.
        let deleted = serde_json::json!({"groqApiKey": ""});
        assert_eq!(merge_key(&deleted, "groqApiKey", ""), "");
        assert_eq!(merge_key(&serde_json::json!({}), "geminiApiKey", ""), "");
    }

    #[test]
    fn known_services() {
        assert!(service_known("groq") && service_known("gemini") && service_known("cerebras") && service_known("deepgram"));
        assert!(!service_known("openai"));
    }
}
