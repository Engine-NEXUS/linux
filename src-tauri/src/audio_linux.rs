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
///   - `"granted"`   — a real capture device is present and opens
///   - `"denied"`    — capture devices exist, but none of them can be opened
///   - `"no_device"` — there is no capture device at all
///
/// Pure decision — see [`permission_verdict`].
#[cfg(not(test))]
pub fn capture_permission_state() -> &'static str {
    use cpal::traits::{DeviceTrait, HostTrait};

    let host = cpal::default_host();
    let devices: Vec<cpal::Device> = match host.input_devices() {
        Ok(iter) => iter.collect(),
        Err(e) => {
            tracing::warn!("audio_linux: could not enumerate capture devices: {e}");
            return permission_verdict(false, true);
        }
    };
    if devices.is_empty() {
        tracing::warn!("audio_linux: no capture device found");
        return permission_verdict(false, false);
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
        if device_opens(d) {
            tracing::info!("audio_linux: capture device '{name}' opened — microphone granted");
            return permission_verdict(true, true);
        }
    }

    tracing::warn!("audio_linux: no capture device could be opened");
    permission_verdict(false, true)
}

/// The permission verdict for the capture devices found on this machine.
///
/// Pure — `opened` is the result of ACTUALLY opening a capture device (not
/// just asking it what it supports), `had_device` says whether a capture
/// device exists at all.
pub fn permission_verdict(opened: bool, had_device: bool) -> &'static str {
    if !had_device {
        "no_device"
    } else if opened {
        "granted"
    } else {
        "denied"
    }
}

/// Does this capture device actually open?
///
/// `default_input_config()` only asks the device what it *supports* — it
/// never opens it, so a microphone that is present but unusable (held
/// exclusively by another application, an ALSA dummy device) would still
/// "succeed" and the app would claim voice input works.
///
/// Building a short input stream is what actually opens the device; the
/// stream is dropped again immediately.
#[cfg(not(test))]
fn device_opens(device: &cpal::Device) -> bool {
    use cpal::traits::{DeviceTrait, StreamTrait};

    let config = match device.default_input_config() {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!("audio_linux: capture device has no usable input config: {e}");
            return false;
        }
    };
    let stream_config = cpal::StreamConfig {
        channels: config.channels(),
        sample_rate: config.sample_rate(),
        buffer_size: cpal::BufferSize::Default,
    };
    let built = match config.sample_format() {
        cpal::SampleFormat::I16 => device.build_input_stream::<i16, _, _>(
            &stream_config,
            |_: &[i16], _: &cpal::InputCallbackInfo| {},
            |_: cpal::StreamError| {},
            None,
        ),
        cpal::SampleFormat::I32 => device.build_input_stream::<i32, _, _>(
            &stream_config,
            |_: &[i32], _: &cpal::InputCallbackInfo| {},
            |_: cpal::StreamError| {},
            None,
        ),
        cpal::SampleFormat::U16 => device.build_input_stream::<u16, _, _>(
            &stream_config,
            |_: &[u16], _: &cpal::InputCallbackInfo| {},
            |_: cpal::StreamError| {},
            None,
        ),
        cpal::SampleFormat::F32 => device.build_input_stream::<f32, _, _>(
            &stream_config,
            |_: &[f32], _: &cpal::InputCallbackInfo| {},
            |_: cpal::StreamError| {},
            None,
        ),
        other => {
            tracing::warn!("audio_linux: unsupported capture sample format {other:?}");
            return false;
        }
    };
    match built {
        // The stream is dropped here — the device WAS opened, which is the
        // whole point of the probe.
        Ok(_stream) => true,
        Err(e) => {
            tracing::warn!("audio_linux: capture device could not be opened: {e}");
            false
        }
    }
}

/// The Linux microphone permission verdict — test double.
///
/// A unit test must never open a capture device (that is the user's sound
/// card). Nothing was opened, so the honest verdict is "no_device"; the pure
/// helpers above are what the tests exercise.
#[cfg(test)]
pub fn capture_permission_state() -> &'static str {
    permission_verdict(false, false)
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

    #[test]
    fn the_permission_verdict_needs_an_actually_opened_device() {
        // Only a device that OPENED counts as granted — a device that is
        // present but unusable (held by another app, an ALSA dummy) is
        // "denied", and no device at all is "no_device".
        assert_eq!(permission_verdict(true, true), "granted");
        assert_eq!(permission_verdict(false, true), "denied");
        assert_eq!(permission_verdict(false, false), "no_device");
        assert_eq!(permission_verdict(true, false), "no_device");
    }
}
