//! Linux Native D-Bus & MPRIS Media Control + Desktop Notifications.
//! Communicates directly with the D-Bus system/session bus using `zbus`.
//! Zero-latency, zero-fork — no `pkill` or sub-shell spawning.

/// The MPRIS interface (and path) used for transport control.
const MPRIS_PATH: &str = "/org/mpris/MediaPlayer2";
/// The MPRIS2 player interface (Play/Pause/Next/…).
const MPRIS_PLAYER_IFACE: &str = "org.mpris.MediaPlayer2.Player";
/// The MPRIS2 root interface — `Activate` raises the player's window.
const MPRIS_ROOT_IFACE: &str = "org.mpris.MediaPlayer2";

/// Map a NEXUS media action to the MPRIS interface + method that performs it.
///
/// Pure — the mapping is platform-independent (only the transport is
/// Linux-only), so it is unit-tested everywhere.
#[allow(dead_code)] // live on Linux (the only platform with an MPRIS transport)
pub fn mpris_method_for(command: &str) -> Option<(&'static str, &'static str)> {
    match command {
        "play_pause" | "toggle" => Some((MPRIS_PLAYER_IFACE, "PlayPause")),
        "play" => Some((MPRIS_PLAYER_IFACE, "Play")),
        "pause" => Some((MPRIS_PLAYER_IFACE, "Pause")),
        "next" => Some((MPRIS_PLAYER_IFACE, "Next")),
        "previous" | "prev" => Some((MPRIS_PLAYER_IFACE, "Previous")),
        "stop" => Some((MPRIS_PLAYER_IFACE, "Stop")),
        // Raise the player window (MPRIS2 root interface).
        "open" | "launch" | "activate" => Some((MPRIS_ROOT_IFACE, "Activate")),
        _ => None,
    }
}

/// How long a single D-Bus call may take before it is abandoned. The desktop
/// is another program and it may never answer — a media command must not wait
/// for it forever.
#[cfg(all(target_os = "linux", not(test)))]
const DBUS_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(2);

#[cfg(all(target_os = "linux", not(test)))]
pub async fn send_mpris_command(command: &str) -> Result<String, String> {
    use zbus::fdo::DBusProxy;
    use zbus::Connection;

    let Some((interface, method)) = mpris_method_for(command) else {
        return Err(format!("Unknown MPRIS command: {command}"));
    };

    let connection = match tokio::time::timeout(DBUS_TIMEOUT, Connection::session()).await {
        Ok(Ok(c)) => c,
        Ok(Err(e)) => return Err(format!("Failed to connect to D-Bus session: {e}")),
        Err(_) => return Err("The D-Bus session did not answer in time.".to_string()),
    };

    let dbus_proxy = match tokio::time::timeout(DBUS_TIMEOUT, DBusProxy::new(&connection)).await {
        Ok(Ok(p)) => p,
        Ok(Err(e)) => return Err(format!("Failed to create D-Bus proxy: {e}")),
        Err(_) => return Err("The D-Bus session did not answer in time.".to_string()),
    };
    let names = match tokio::time::timeout(DBUS_TIMEOUT, dbus_proxy.list_names()).await {
        Ok(Ok(n)) => n,
        Ok(Err(e)) => return Err(format!("Failed to list D-Bus names: {e}")),
        Err(_) => return Err("The D-Bus session did not answer in time.".to_string()),
    };

    let mpris_players: Vec<String> = names
        .into_iter()
        .filter(|n| n.as_str().starts_with("org.mpris.MediaPlayer2."))
        .map(|n| n.to_string())
        .collect();

    if mpris_players.is_empty() {
        return Err("No active media players found on D-Bus, sir.".to_string());
    }

    let mut sent = 0;
    for player in &mpris_players {
        let res = tokio::time::timeout(
            DBUS_TIMEOUT,
            connection.call_method(
                Some(player.as_str()),
                MPRIS_PATH,
                Some(interface),
                method,
                &(),
            ),
        )
        .await;
        if matches!(res, Ok(Ok(_))) {
            sent += 1;
        }
    }

    if sent > 0 {
        tracing::info!("MPRIS D-Bus: sent {} to {} player(s)", method, sent);
        Ok(format!("Executed {} on active media player.", method.to_lowercase()))
    } else {
        Err(format!("Failed to send {} to media players.", method))
    }
}

/// Unit-test double: a unit test must never open the real session bus (the
/// same rule as the screen-saver inhibitor — tests never touch the real
/// system). The pure mapping above is what the tests exercise.
#[cfg(all(target_os = "linux", test))]
pub async fn send_mpris_command(_command: &str) -> Result<String, String> {
    Err("MPRIS D-Bus is disabled in unit tests".to_string())
}

#[allow(dead_code)]
#[cfg(not(target_os = "linux"))]
pub async fn send_mpris_command(_command: &str) -> Result<String, String> {
    Err("MPRIS D-Bus is only available on Linux".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transport_actions_map_to_the_player_interface() {
        assert_eq!(
            mpris_method_for("play_pause"),
            Some(("org.mpris.MediaPlayer2.Player", "PlayPause"))
        );
        assert_eq!(
            mpris_method_for("toggle"),
            Some(("org.mpris.MediaPlayer2.Player", "PlayPause"))
        );
        assert_eq!(
            mpris_method_for("play"),
            Some(("org.mpris.MediaPlayer2.Player", "Play"))
        );
        assert_eq!(
            mpris_method_for("pause"),
            Some(("org.mpris.MediaPlayer2.Player", "Pause"))
        );
        assert_eq!(
            mpris_method_for("next"),
            Some(("org.mpris.MediaPlayer2.Player", "Next"))
        );
        assert_eq!(
            mpris_method_for("previous"),
            Some(("org.mpris.MediaPlayer2.Player", "Previous"))
        );
        assert_eq!(
            mpris_method_for("stop"),
            Some(("org.mpris.MediaPlayer2.Player", "Stop"))
        );
    }

    #[test]
    fn opening_a_player_activates_its_window() {
        assert_eq!(
            mpris_method_for("open"),
            Some(("org.mpris.MediaPlayer2", "Activate"))
        );
        assert_eq!(
            mpris_method_for("launch"),
            Some(("org.mpris.MediaPlayer2", "Activate"))
        );
    }

    #[test]
    fn an_unknown_action_has_no_mpris_method() {
        assert!(mpris_method_for("").is_none());
        assert!(mpris_method_for("search").is_none());
        assert!(mpris_method_for("volume up").is_none());
    }
}
