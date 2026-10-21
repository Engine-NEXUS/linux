//! Linux browser-meeting detection (PipeWire / PulseAudio).
//!
//! The meeting detector's process-name layer only sees a **native** meeting
//! client (zoom, teams, skypeforlinux, webex). A browser meeting — Google
//! Meet in Chromium, Zoom in Firefox — has no distinct process, so the only
//! cheap local signal is "a browser currently holds the microphone".
//!
//! PipeWire's PulseAudio-compatible layer exposes every recording stream
//! through `pactl list source-outputs`; each stream carries the owning
//! application's name in its properties. That is ONE cheap command, run at
//! most every 5 s, and it fails **quietly** when `pactl` is not installed.
//!
//! The AT-SPI route (reading a window/tab title) is out of scope here.

/// Application names (lower-cased) that count as "a browser is in a meeting".
/// Matched as substrings of `application.name` / `application.process.binary`.
const BROWSER_MARKERS: &[&str] = &[
    "chrome",
    "chromium",
    "brave",
    "firefox",
    "opera",
    "vivaldi",
    "msedge",
    "microsoft edge",
    "microsoft-edge",
    "microsoftedge",
    "thorium",
    "epic",
    "google meet",
];

/// Applications that must never count as a meeting: NEXUS's own capture,
/// the desktop audio stack, and the WebKitGTK helper the frontend uses for
/// `getUserMedia` (it holds the mic for the whole session on Linux).
const IGNORED_MARKERS: &[&str] = &[
    "nexus",
    "webkit",
    "pipewire",
    "pulseaudio",
    "speech-dispatcher",
    "wireplumber",
];

/// Extract the owning application names from `pactl list source-outputs`
/// output. Pure — the caller supplies the text.
///
/// Each recording stream looks like:
///
/// ```text
/// Source Output 3:
/// \tDriver: protocol-native.c
/// \tClient: 42
/// \tSource: 1
/// \tProperties:
/// \t\tmedia.name = "Audio Stream"
/// \t\tapplication.name = "Firefox"
/// \t\tapplication.process.binary = "firefox"
/// ```
pub fn parse_capture_clients(pactl_output: &str) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    for line in pactl_output.lines() {
        let line = line.trim();
        // "application.name = \"Firefox\"" / "application.process.binary = \"firefox\""
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim();
        if key != "application.name" && key != "application.process.binary" {
            continue;
        }
        let value = value.trim().trim_matches('"').trim();
        if value.is_empty() {
            continue;
        }
        if !names.iter().any(|n| n == value) {
            names.push(value.to_string());
        }
    }
    names
}

/// True when one of the capture-stream owners is a browser.
pub fn is_browser_capture(clients: &[String]) -> bool {
    clients.iter().any(|name| {
        let lower = name.to_lowercase();
        if IGNORED_MARKERS.iter().any(|m| lower.contains(m)) {
            return false;
        }
        BROWSER_MARKERS.iter().any(|m| lower.contains(m))
    })
}

/// How long `pactl` may take before it is killed. A wedged PipeWire/PulseAudio
/// server makes it block forever, and the caller is a tokio runtime worker —
/// one hung call would kill the meeting-detection task for the session.
#[cfg(not(test))]
const PACTL_TIMEOUT_MS: u64 = 2_000;

/// Ask the audio server whether a browser is currently recording.
///
/// Returns `false` for every failure mode (no `pactl`, a hung `pactl`, no
/// recording streams, a non-zero exit) — the caller treats "unknown" as
/// "no meeting", exactly like a missing meeting app.
#[cfg(not(test))]
pub fn browser_capture_active() -> bool {
    use crate::proc_linux::BoundedOutcome;

    let mut cmd = std::process::Command::new("pactl");
    cmd.args(["list", "source-outputs"]);
    match crate::proc_linux::run_bounded(&mut cmd, PACTL_TIMEOUT_MS) {
        BoundedOutcome::Finished { success: true, stdout } => {
            let clients = parse_capture_clients(&stdout);
            is_browser_capture(&clients)
        }
        BoundedOutcome::Finished { success: false, .. } => {
            tracing::debug!("meeting_linux: 'pactl list source-outputs' failed");
            false
        }
        BoundedOutcome::TimedOut => {
            tracing::debug!(
                "meeting_linux: 'pactl list source-outputs' timed out after {PACTL_TIMEOUT_MS} ms — browser meetings not detected"
            );
            false
        }
        BoundedOutcome::NotStarted => {
            tracing::debug!("meeting_linux: pactl not available — browser meetings not detected");
            false
        }
    }
}

/// Ask the audio server whether a browser is currently recording — test double.
///
/// A unit test must never spawn `pactl` against the user's real audio server (the same rule as
/// the D-Bus screen-saver inhibitor: tests never touch the real system). The pure parser above
/// is what the tests exercise.
#[cfg(test)]
pub fn browser_capture_active() -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "Source Output 1:\n\tDriver: protocol-native.c\n\tOwner Module: 13\n\tClient: 42\n\tSource: 1\n\tVolume: front-left: 65536 / 100% / 0.00 dB\n\tProperties:\n\t\tmedia.name = \"Audio Stream\"\n\t\tapplication.name = \"Google Meet\"\n\t\tapplication.process.binary = \"google-chrome\"\n\nSource Output 2:\n\tClient: 43\n\tProperties:\n\t\tapplication.name = \"Firefox\"\n";

    #[test]
    fn parses_the_owning_application_of_every_record_stream() {
        let clients = parse_capture_clients(SAMPLE);
        assert!(clients.iter().any(|c| c == "Google Meet"));
        assert!(clients.iter().any(|c| c == "google-chrome"));
        assert!(clients.iter().any(|c| c == "Firefox"));
    }

    #[test]
    fn a_browser_holding_the_mic_counts_as_a_meeting() {
        let clients = parse_capture_clients(SAMPLE);
        assert!(is_browser_capture(&clients));
    }

    #[test]
    fn our_own_capture_is_not_a_meeting() {
        let out = "Source Output 9:\n\tProperties:\n\t\tapplication.name = \"nexus\"\n\t\tapplication.process.binary = \"nexus\"\n";
        let clients = parse_capture_clients(out);
        assert!(!is_browser_capture(&clients));
    }

    #[test]
    fn the_webkitgtk_helper_is_not_a_meeting() {
        let out = "Source Output 9:\n\tProperties:\n\t\tapplication.name = \"WebKitWebProcess\"\n";
        let clients = parse_capture_clients(out);
        assert!(!is_browser_capture(&clients));
    }

    #[test]
    fn no_recording_streams_means_no_meeting() {
        assert!(!is_browser_capture(&parse_capture_clients("")));
        assert!(!is_browser_capture(&parse_capture_clients(
            "Source Output 1:\n\tClient: 42\n\tSource: 1\n"
        )));
    }

    #[test]
    fn a_non_browser_recorder_is_not_a_meeting() {
        let out = "Source Output 4:\n\tProperties:\n\t\tapplication.name = \"Zoom\"\n";
        assert!(!is_browser_capture(&parse_capture_clients(out)));
    }
}
