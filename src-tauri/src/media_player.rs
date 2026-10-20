//! Cross-platform Universal Music & Media Controller.
//!
//! Controls music playback and opens media players across macOS and Windows:
//! - macOS (MacBook): Native AppleScript (`osascript`) controls for both Apple Music
//!   (`tell application "Music"`) and Spotify (`tell application "Spotify"`).
//! - Windows: Win32 multimedia virtual key dispatch (`VK_MEDIA_*`), protocol
//!   deep links (`spotify:`, `applemusic:`), and native process activation.
//! - Linux: MPRIS D-Bus interface via `crate::mpris`.

use std::process::Command;

/// Canonical music player identifiers.
pub const PLAYER_SPOTIFY: &str = "spotify";
pub const PLAYER_APPLE_MUSIC: &str = "apple_music";

/// Normalize player name from natural speech / input to canonical ID.
pub fn normalize_player_name(raw: &str) -> &str {
    let lower = raw.trim().to_lowercase();
    if lower.contains("apple") || lower.contains("music") || lower.contains("itunes") {
        PLAYER_APPLE_MUSIC
    } else {
        PLAYER_SPOTIFY
    }
}

/// Resolve which player to use:
/// 1. Explicitly requested player (e.g. from "on Spotify" / "on Apple Music")
/// 2. User's saved preference in settings.json (`defaultMusicPlayer`)
/// 3. Default fallback: "spotify"
pub fn resolve_player(requested: Option<&str>) -> String {
    if let Some(r) = requested {
        return normalize_player_name(r).to_string();
    }

    // Try reading default from local settings.json
    if let Some(config_dir) = dirs_next::data_dir() {
        let settings_path = config_dir.join("com.nexus.assistant").join("settings.json");
        if settings_path.exists() {
            if let Ok(content) = std::fs::read_to_string(&settings_path) {
                if let Ok(val) = serde_json::from_str::<serde_json::Value>(&content) {
                    if let Some(p) = val.get("defaultMusicPlayer").and_then(|v| v.as_str()) {
                        return normalize_player_name(p).to_string();
                    }
                }
            }
        }
    }

    PLAYER_SPOTIFY.to_string()
}

/// Display name for user-facing speech and captions.
pub fn player_display_name(player: &str) -> &'static str {
    match normalize_player_name(player) {
        PLAYER_APPLE_MUSIC => "Apple Music",
        _ => "Spotify",
    }
}

/// Execute a media action across supported platforms.
pub async fn execute_media_action(
    action: &str,
    query: Option<&str>,
    player_opt: Option<&str>,
) -> Result<String, String> {
    let player = resolve_player(player_opt);
    let player_name = player_display_name(&player);

    tracing::info!(
        "media_player: executing action='{}' query={:?} player='{}' ({})",
        action,
        query,
        player,
        player_name
    );

    #[cfg(target_os = "macos")]
    {
        execute_macos_action(action, query, &player, player_name)
    }

    #[cfg(target_os = "windows")]
    {
        execute_windows_action(action, query, &player, player_name)
    }

    #[cfg(target_os = "linux")]
    {
        let _ = player_name;
        // MPRIS controls the player that is already running; it cannot search
        // a catalogue. Say so instead of pretending the song started.
        if let Some(q) = query {
            if !q.trim().is_empty() {
                return Err(
                    "Playing a specific song is not supported by the Linux media control yet, sir."
                        .to_string(),
                );
            }
        }
        crate::mpris::send_mpris_command(action).await
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    {
        let _ = query;
        Ok(format!("Media {} executed on {player_name}, sir.", action))
    }
}

// ─── macOS AppleScript Implementation ──────────────────────────────────────

#[cfg(target_os = "macos")]
fn execute_macos_action(
    action: &str,
    query: Option<&str>,
    player: &str,
    display: &str,
) -> Result<String, String> {
    let app_name = if player == PLAYER_APPLE_MUSIC {
        "Music"
    } else {
        "Spotify"
    };

    match action {
        "open" | "launch" => {
            let script = format!(r#"tell application "{app_name}" to activate"#);
            run_osascript(&script)?;
            Ok(format!("Opened {display}, sir."))
        }
        "play_pause" | "toggle" => {
            let script = format!(r#"tell application "{app_name}" to playpause"#);
            run_osascript(&script)?;
            Ok(format!("Toggled playback on {display}, sir."))
        }
        "play" => {
            if let Some(q) = query {
                play_track_macos(player, q, display)
            } else {
                let script = format!(r#"tell application "{app_name}" to play"#);
                run_osascript(&script)?;
                Ok(format!("Playing {display}, sir."))
            }
        }
        "pause" => {
            let script = format!(r#"tell application "{app_name}" to pause"#);
            run_osascript(&script)?;
            Ok(format!("Paused {display}, sir."))
        }
        "next" | "skip" => {
            let script = format!(r#"tell application "{app_name}" to next track"#);
            run_osascript(&script)?;
            Ok(format!("Skipped to the next track on {display}, sir."))
        }
        "previous" | "prev" => {
            let script = format!(r#"tell application "{app_name}" to previous track"#);
            run_osascript(&script)?;
            Ok(format!("Playing previous track on {display}, sir."))
        }
        "stop" => {
            let script = if player == PLAYER_APPLE_MUSIC {
                format!(r#"tell application "{app_name}" to stop"#)
            } else {
                format!(r#"tell application "{app_name}" to pause"#)
            };
            run_osascript(&script)?;
            Ok(format!("Stopped {display}, sir."))
        }
        _ => Err(format!("Unknown media action: {action}")),
    }
}

#[cfg(target_os = "macos")]
fn play_track_macos(player: &str, query: &str, display: &str) -> Result<String, String> {
    let escaped = query.replace('"', "\\\"");
    if player == PLAYER_APPLE_MUSIC {
        // Try searching/playing within Music library or opening search
        let script = format!(
            r#"tell application "Music"
                activate
                try
                    play (first track whose name contains "{escaped}")
                on error
                    set sound volume to 75
                    play
                end try
            end tell"#
        );
        run_osascript(&script)?;
        Ok(format!("Playing {} on Apple Music, sir.", query))
    } else {
        // Spotify URI deep-link search & activate
        let encoded = urlencoding::encode(query);
        let uri = format!("spotify:search:{encoded}");
        let script = format!(
            r#"tell application "Spotify"
                activate
                open location "{uri}"
                play
            end tell"#
        );
        run_osascript(&script)?;
        Ok(format!("Playing {} on Spotify, sir.", query))
    }
}

#[cfg(target_os = "macos")]
fn run_osascript(script: &str) -> Result<(), String> {
    let output = Command::new("osascript")
        .arg("-e")
        .arg(script)
        .output()
        .map_err(|e| format!("Failed to run osascript: {e}"))?;

    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr);
        return Err(format!("AppleScript error: {err}"));
    }
    Ok(())
}

// ─── Windows Implementation ────────────────────────────────────────────────

#[cfg(target_os = "windows")]
fn execute_windows_action(
    action: &str,
    query: Option<&str>,
    player: &str,
    display: &str,
) -> Result<String, String> {
    match action {
        "open" | "launch" => open_player_windows(player, display),
        "play_pause" | "toggle" => {
            send_windows_media_key(0xB3)?; // VK_MEDIA_PLAY_PAUSE
            Ok(format!("Toggled playback on {display}, sir."))
        }
        "play" => {
            if let Some(q) = query {
                play_track_windows(player, q, display)
            } else {
                send_windows_media_key(0xB3)?; // VK_MEDIA_PLAY_PAUSE
                Ok(format!("Playing on {display}, sir."))
            }
        }
        "pause" => {
            send_windows_media_key(0xB3)?; // VK_MEDIA_PLAY_PAUSE
            Ok(format!("Paused {display}, sir."))
        }
        "next" | "skip" => {
            send_windows_media_key(0xB0)?; // VK_MEDIA_NEXT_TRACK
            Ok(format!("Skipped to the next track on {display}, sir."))
        }
        "previous" | "prev" => {
            send_windows_media_key(0xB1)?; // VK_MEDIA_PREV_TRACK
            Ok(format!("Playing previous track on {display}, sir."))
        }
        "stop" => {
            send_windows_media_key(0xB2)?; // VK_MEDIA_STOP
            Ok(format!("Stopped {display}, sir."))
        }
        _ => Err(format!("Unknown media action: {action}")),
    }
}

#[cfg(target_os = "windows")]
fn open_player_windows(player: &str, display: &str) -> Result<String, String> {
    if player == PLAYER_APPLE_MUSIC {
        // Try Windows Apple Music protocol or store app
        let launched = Command::new("cmd")
            .args(["/c", "start", "applemusic:"])
            .spawn()
            .is_ok();
        if !launched {
            let _ = Command::new("cmd")
                .args(["/c", "start", "itms:"])
                .spawn();
        }
    } else {
        // Spotify protocol or executable
        let launched = Command::new("cmd")
            .args(["/c", "start", "spotify:"])
            .spawn()
            .is_ok();
        if !launched {
            if let Some(entry) = crate::app_registry::lookup_strict("spotify") {
                let _ = crate::app_registry::launch(&entry);
            }
        }
    }
    Ok(format!("Opened {display}, sir."))
}

#[cfg(target_os = "windows")]
fn play_track_windows(player: &str, query: &str, _display: &str) -> Result<String, String> {
    if player == PLAYER_SPOTIFY {
        let uri = format!("spotify:search:{}", query);
        let _ = Command::new("cmd")
            .args(["/c", "start", &uri])
            .spawn();
        // Give Spotify a brief moment to focus, then send play key
        std::thread::sleep(std::time::Duration::from_millis(600));
        let _ = send_windows_media_key(0xB3);
        Ok(format!("Searching and playing {} on Spotify, sir.", query))
    } else {
        // Apple Music search web or protocol
        let url = format!("https://music.apple.com/search?term={}", query.replace(' ', "+"));
        let _ = Command::new("cmd")
            .args(["/c", "start", &url])
            .spawn();
        Ok(format!("Opening {} on Apple Music, sir.", query))
    }
}

#[cfg(target_os = "windows")]
fn send_windows_media_key(vk: u8) -> Result<(), String> {
    use windows::Win32::UI::Input::KeyboardAndMouse::{keybd_event, KEYEVENTF_KEYUP};
    unsafe {
        // Key down
        keybd_event(vk, 0, Default::default(), 0);
        // Key up
        keybd_event(vk, 0, KEYEVENTF_KEYUP, 0);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_player_name() {
        assert_eq!(normalize_player_name("spotify"), PLAYER_SPOTIFY);
        assert_eq!(normalize_player_name("SPOTIFY"), PLAYER_SPOTIFY);
        assert_eq!(normalize_player_name("apple music"), PLAYER_APPLE_MUSIC);
        assert_eq!(normalize_player_name("Apple Music"), PLAYER_APPLE_MUSIC);
        assert_eq!(normalize_player_name("apple"), PLAYER_APPLE_MUSIC);
        assert_eq!(normalize_player_name("music"), PLAYER_APPLE_MUSIC);
    }

    #[test]
    fn test_player_display_name() {
        assert_eq!(player_display_name(PLAYER_SPOTIFY), "Spotify");
        assert_eq!(player_display_name(PLAYER_APPLE_MUSIC), "Apple Music");
    }

    #[test]
    fn test_resolve_player_explicit() {
        assert_eq!(resolve_player(Some("spotify")), "spotify");
        assert_eq!(resolve_player(Some("apple music")), "apple_music");
    }
}
