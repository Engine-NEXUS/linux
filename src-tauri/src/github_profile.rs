//! GitHub identity (login, name, avatar) for the Command Hub account row.
//! NEXUS stores only the GitHub OAuth *token* (on the Worker); to show a
//! username and photo it reads `GET /user` once with that token and caches
//! the answer in `github_profile.json`. Offline → the cached copy.

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager, Runtime};

const FILE: &str = "github_profile.json";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GithubProfile {
    pub login: String,
    pub name: String,
    pub avatar_url: String,
}

/// Parse the GitHub `/user` response. Pure.
pub fn parse_user(json: &str) -> Option<GithubProfile> {
    let v: serde_json::Value = serde_json::from_str(json).ok()?;
    let login = v.get("login")?.as_str()?.trim().to_string();
    if login.is_empty() {
        return None;
    }
    let s = |k: &str| v.get(k).and_then(|x| x.as_str()).unwrap_or("").trim().to_string();
    let name = {
        let n = s("name");
        if n.is_empty() { login.clone() } else { n }
    };
    Some(GithubProfile { login, name, avatar_url: s("avatar_url") })
}

fn cache_path<R: Runtime>(app: &AppHandle<R>) -> Option<std::path::PathBuf> {
    app.path().app_data_dir().ok().map(|d| d.join(FILE))
}

fn read_cache<R: Runtime>(app: &AppHandle<R>) -> Option<GithubProfile> {
    let p = cache_path(app)?;
    serde_json::from_str(&std::fs::read_to_string(p).ok()?).ok()
}

/// IPC: the connected GitHub identity. Network first (fresh), cache when
/// offline. `Err` only when there is no token and no cache.
#[tauri::command]
pub async fn github_profile<R: Runtime>(
    app: AppHandle<R>,
    server_url: String,
    user_id: String,
) -> Result<GithubProfile, String> {
    let fetched = async {
        let token = crate::github_cmd::get_github_token(&server_url, &user_id).await?;
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(8))
            .build()
            .map_err(|e| e.to_string())?;
        let resp = client
            .get("https://api.github.com/user")
            .header("User-Agent", "nexus-assistant")
            .header("Accept", "application/vnd.github+json")
            .bearer_auth(token)
            .send()
            .await
            .map_err(|e| e.to_string())?;
        if !resp.status().is_success() {
            return Err(format!("GitHub /user returned {}", resp.status()));
        }
        parse_user(&resp.text().await.map_err(|e| e.to_string())?)
            .ok_or_else(|| "unexpected GitHub /user response".to_string())
    }
    .await;
    match fetched {
        Ok(p) => {
            if let Some(path) = cache_path(&app) {
                let _ = std::fs::write(path, serde_json::to_string(&p).unwrap_or_default());
            }
            Ok(p)
        }
        Err(e) => read_cache(&app).ok_or(e),
    }
}

/// IPC: forget the cached identity and token (GitHub disconnected).
#[tauri::command]
pub async fn github_profile_clear<R: Runtime>(app: AppHandle<R>) -> Result<(), String> {
    crate::github_cmd::clear_github_token().await;
    if let Some(p) = cache_path(&app) {
        let _ = std::fs::remove_file(p);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_user_response() {
        let p = parse_user(r#"{"login":"octocat","name":"The Octocat","avatar_url":"https://avatars.githubusercontent.com/u/583231?v=4","id":1}"#).unwrap();
        assert_eq!(p.login, "octocat");
        assert_eq!(p.name, "The Octocat");
        assert!(p.avatar_url.starts_with("https://avatars.githubusercontent.com/"));
    }

    #[test]
    fn missing_name_falls_back_to_login() {
        let p = parse_user(r#"{"login":"octocat","name":null,"avatar_url":""}"#).unwrap();
        assert_eq!(p.name, "octocat");
        assert_eq!(p.avatar_url, "");
    }

    #[test]
    fn rejects_bad_payloads() {
        assert!(parse_user("not json").is_none());
        assert!(parse_user(r#"{"message":"Bad credentials"}"#).is_none());
        assert!(parse_user(r#"{"login":"  "}"#).is_none());
    }
}
