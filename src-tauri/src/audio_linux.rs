//! Linux audio-platform helpers: PipeWire/ALSA capture-device selection.
//!
//! A Linux audio server publishes far more "input" devices than there are
//! microphones:
//!
//!   - every output sink also exposes a `Monitor of …` loopback source, which
//!     carries **speaker output** — probing it first costs 5 s per device and
//!     can false-trigger the wake word on our own TTS playback;
//!   - ALSA's `default` often resolves to a PipeWire dummy sink that emits
//!     electrical static instead of speech;
//!   - WirePlumber's EasyEffects sink shows up as a capture device too.
//!
//! The Linux build therefore probes **real microphones first** and the
//! loopback/dummy sources last (they are still probed, so a machine whose
//! only capture device is a loopback still gets a stream).
//!
//! Everything in this module is pure so it can be unit-tested without a
//! sound card.

/// Name fragments that identify an output loopback ("monitor") source.
/// A monitor source carries speaker output — never microphone input.
const LOOPBACK_MARKERS: &[&str] = &["monitor", "loopback"];

/// Name fragments of the virtual sinks WirePlumber/PipeWire publish as capture
/// devices ("EasyEffects Sink", "Dummy Output", "Spotify", "Null").
const VIRTUAL_SINK_MARKERS: &[&str] = &["easyeffects", "spotify", "dummy", "null"];

/// True when `name` identifies an output-loopback / virtual-sink capture
/// device rather than a real microphone.
pub fn is_loopback_device(name: &str) -> bool {
    let lower = name.trim().to_lowercase();
    if lower.is_empty() {
        return false;
    }
    LOOPBACK_MARKERS
        .iter()
        .chain(VIRTUAL_SINK_MARKERS.iter())
        .any(|m| lower.contains(m))
}

/// Probe-order key: real microphones first (0), loopbacks / virtual sinks
/// last (1). Used to sort the capture-device probe order; the sort is
/// stable, so the default device keeps its place among equals.
pub fn probe_priority(name: &str) -> u8 {
    if is_loopback_device(name) {
        1
    } else {
        0
    }
}

/// The Linux microphone permission verdict.
///
/// Linux has **no per-app microphone prompt** (PipeWire/PulseAudio grant
/// capture at the session level), so the only honest signal is whether a
/// capture device actually opens. Returns:
///
///   - `"granted"`   — a real capture device is present and openable
///   - `"denied"`    — capture devices exist, but none of them can be opened
///   - `"no_device"` — there is no capture device at all
#[cfg(not(test))]
pub fn capture_permission_state() -> &'static str {
    use cpal::traits::{DeviceTrait, HostTrait};

    let host = cpal::default_host();
    let devices: Vec<cpal::Device> = match host.input_devices() {
        Ok(iter) => iter.collect(),
        Err(e) => {
            tracing::warn!("audio_linux: could not enumerate capture devices: {e}");
            return "denied";
        }
    };
    if devices.is_empty() {
        tracing::warn!("audio_linux: no capture device found");
        return "no_device";
    }

    // A real microphone first; loopbacks are only accepted when nothing else
    // exists (a machine whose only "input" is a monitor would otherwise be
    // reported as broken).
    let mut real: Vec<&cpal::Device> = Vec::new();
    let mut loopback: Vec<&cpal::Device> = Vec::new();
    for d in &devices {
        let name = d.name().unwrap_or_default();
        if probe_priority(&name) == 0 {
            real.push(d);
        } else {
            loopback.push(d);
        }
    }

    for d in real.iter().chain(loopback.iter()) {
        let name = d.name().unwrap_or_else(|_| "unknown".into());
        match d.default_input_config() {
            Ok(_) => {
                tracing::info!("audio_linux: capture device '{name}' opened — microphone granted");
                return "granted";
            }
            Err(e) => {
                tracing::warn!("audio_linux: capture device '{name}' could not be opened: {e}");
            }
        }
    }

    tracing::warn!("audio_linux: no capture device could be opened");
    "denied"
}

/// The Linux microphone permission verdict — test double.
///
/// A unit test must never open a capture device (that is the user's sound
/// card). Nothing was opened, so the honest verdict is "no_device"; the pure
/// helpers above are what the tests exercise.
#[cfg(test)]
pub fn capture_permission_state() -> &'static str {
    "no_device"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pipewire_monitor_sources_are_loopbacks() {
        assert!(is_loopback_device("Monitor of Built-in Audio Analog Stereo"));
        assert!(is_loopback_device("alsa_output.pci-0000_00_1f.3.analog-stereo.monitor"));
        assert!(is_loopback_device("Monitor of Audio Analog Stereo"));
    }

    #[test]
    fn virtual_sinks_are_loopbacks() {
        assert!(is_loopback_device("easyeffects_sink"));
        assert!(is_loopback_device("EasyEffects Sink"));
        assert!(is_loopback_device("spotify"));
        assert!(is_loopback_device("Dummy Output"));
    }

    #[test]
    fn real_microphones_are_not_loopbacks() {
        for name in [
            "Built-in Audio Analog Stereo",
            "HDA Intel PCH: ALC295 Analog (hw:0,0)",
            "USB Audio Device",
            "Webcam Microphone",
            "pipewire-alsa-input",
        ] {
            assert!(
                !is_loopback_device(name),
                "'{name}' is a real capture device and must not be deprioritised"
            );
            assert_eq!(probe_priority(name), 0);
        }
    }

    #[test]
    fn probe_order_puts_real_mics_first() {
        let names = [
            "Monitor of Built-in Audio Analog Stereo",
            "Built-in Audio Analog Stereo",
            "easyeffects_sink",
        ];
        let mut ordered: Vec<&str> = names.iter().copied().collect();
        ordered.sort_by_key(|n| probe_priority(n));
        assert_eq!(
            ordered,
            vec!["Built-in Audio Analog Stereo", "Monitor of Built-in Audio Analog Stereo", "easyeffects_sink"]
        );
    }

    #[test]
    fn empty_name_is_not_a_loopback() {
        assert!(!is_loopback_device(""));
        assert_eq!(probe_priority(""), 0);
    }
}
