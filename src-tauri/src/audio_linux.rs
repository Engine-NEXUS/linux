//! Linux audio-platform helpers: PipeWire/ALSA capture-device selection.
//!
//! A Linux audio server publishes far more "input" devices than there are
//! microphones, and they are not equally useful:
//!
//!   - every output sink also exposes a `Monitor of …` loopback source, which
//!     carries **speaker output** — giving it a probe window wastes time and
//!     can false-trigger the wake word on our own TTS playback;
//!   - ALSA's `default` often resolves to a PipeWire dummy sink that emits
//!     electrical static instead of speech;
//!   - ALSA's **plugin** devices (`lavrate` = the "Linear Converter" plugin,
//!     `samplerate`, `speexrate`, `upmix`, `vdownmix`, `jack`, …) are rate
//!     converters and mixers, not microphones: they open happily and deliver
//!     **silence**, so accepting one leaves the wake word listening to
//!     nothing at all;
//!   - WirePlumber's EasyEffects sink shows up as a capture device too.
//!
//! On the target machine (Pop!_OS 24.04, COSMIC, PipeWire) the device list
//! was, in ALSA order: `lavrate`, `jack`, `pipewire`, `pulse`, `default`,
//! `hw:CARD=sofhdadsp,DEV=6`, `hw:CARD=sofhdadsp,DEV=7`. Probing that order
//! meant the real capture path (PipeWire's default source) was tried late and
//! the `lavrate` plugin was accepted instead — the wake word then never heard
//! anything.
//!
//! The Linux build therefore probes in a fixed order ([`probe_priority`]):
//! the audio server's own capture path first (`pipewire`, `pulse`,
//! `default`), then the real hardware (`hw:`, `plughw:`, `sysdefault:`), then
//! everything else, and only last the ALSA plugin devices and the
//! loopback/monitor sources. On a PipeWire system `pipewire`, `pulse` and
//! `default` are three names for the same source, so only `pipewire` is
//! probed when it is present — probing all three would spend the long probe
//! window three times on one microphone ([`probe_order`]).
//!
//! When every device turns out to be silent, [`silent_fallback_index`]
//! reopens the best **real** capture path rather than a plugin or a loopback:
//! a plugin will never become a microphone, while a real device may start
//! working as soon as the input is unmuted.
//!
//! Everything in this module is pure so it can be unit-tested without a
//! sound card.

/// Name fragments that identify an output loopback ("monitor") source.
/// A monitor source carries speaker output — never microphone input.
const LOOPBACK_MARKERS: &[&str] = &["monitor", "loopback"];

/// Name fragments of the virtual sinks WirePlumber/PipeWire publish as capture
/// devices ("EasyEffects Sink", "Dummy Output", "Spotify", "Null").
const VIRTUAL_SINK_MARKERS: &[&str] = &["easyeffects", "spotify", "dummy", "null"];

/// ALSA plugin / virtual capture devices.
///
/// These are rate converters and mixers, not microphones. `lavrate` is ALSA's
/// "Linear Converter" plugin: it opens without error and delivers silence,
/// so it must never be preferred over a real capture path.
const ALSA_PLUGIN_MARKERS: &[&str] = &[
    "lavrate",
    "samplerate",
    "speexrate",
    "speex",
    "upmix",
    "vdownmix",
    "jack",
    "oss",
    "null",
    "dmix",
    "dsnoop",
];

// ─── Probe tiers (lower is probed first) ─────────────────────────────────────

/// The PipeWire capture device — the audio server's own default source.
const TIER_PIPEWIRE: u8 = 0;
/// The PulseAudio capture device (the compatibility layer).
const TIER_PULSE: u8 = 1;
/// ALSA's `default` capture device (resolves to the server's default source).
const TIER_DEFAULT: u8 = 2;
/// A real hardware capture device (`hw:`, `plughw:`, `sysdefault:`).
const TIER_HARDWARE: u8 = 3;
/// Anything else that is not a plugin and not a loopback.
const TIER_OTHER: u8 = 4;
/// An ALSA plugin / virtual device — opens, but delivers silence.
const TIER_PLUGIN: u8 = 5;
/// An output loopback ("Monitor of …") — carries speaker output.
const TIER_LOOPBACK: u8 = 6;

/// The normal probe window: how long a device gets to deliver its first
/// audio callback before it is called silent.
pub const PROBE_WAIT_MS: u64 = 500;

/// The probe window for the audio server's own capture path.
///
/// PipeWire's ALSA bridge starts slowly: the stream is created immediately
/// but the first callback can take more than a second. Probing it with the
/// normal 500 ms window reports "no audio callback fired" for a perfectly
/// good microphone and moves on to the next device.
pub const SLOW_PROBE_WAIT_MS: u64 = 2000;

/// What the wake-word init line prints as the device on Linux.
///
/// The acoustic profile that ships with the app was calibrated on a Windows
/// machine, so its device name ("Microphone Array (Intel® Smart …)") is not
/// this machine's microphone. The device NEXUS really listens to is only
/// known once the capture stream opens — the capture step logs it — so the
/// init line points there instead of printing a name that is not this
/// machine's.
pub const KWS_DEVICE_PENDING: &str =
    "pending (the device actually opened is logged by the capture step)";

/// The "the microphone is silent" hint in the log — Linux wording.
///
/// The Windows message is about the Intel SST driver, which does not exist
/// here: on Linux a silent microphone is almost always the wrong input
/// device, a muted input or a PipeWire that is not running.
pub const SILENT_MIC_HINT: &str =
    "On Linux: check the input device and its volume in Settings > Sound (or run `wpctl status`), \
     and make sure PipeWire is running.";

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

/// Probe-order key: lower is probed first.
///
/// The tiers are, in order:
/// 0. `pipewire` — the audio server's own capture path;
/// 1. `pulse` — the PulseAudio compatibility layer (skipped when `pipewire`
///    is present: same source);
/// 2. `default` — ALSA's default device, the server's default source
///    (likewise skipped when `pipewire` is present);
/// 3. `hw:` / `plughw:` / `sysdefault:` — real hardware;
/// 4. everything else that is neither a plugin nor a loopback;
/// 5. ALSA plugin / virtual devices (they open, but deliver silence);
/// 6. output loopbacks ("Monitor of …" — they carry speaker output).
///
/// The sort is stable, so devices in the same tier keep the order the
/// caller passed in.
pub fn probe_priority(name: &str) -> u8 {
    let lower = name.trim().to_lowercase();
    if lower.is_empty() {
        return TIER_OTHER;
    }
    // A loopback is never a microphone, whatever else it looks like.
    if is_loopback_device(&lower) {
        return TIER_LOOPBACK;
    }
    if lower.starts_with("pipewire") {
        return TIER_PIPEWIRE;
    }
    if lower.starts_with("pulse") {
        return TIER_PULSE;
    }
    // Exact match only: "sysdefault:…" is a hardware device, not ALSA's
    // "default" device.
    if lower == "default" {
        return TIER_DEFAULT;
    }
    if lower.starts_with("hw:") || lower.starts_with("plughw:") || lower.starts_with("sysdefault:")
    {
        return TIER_HARDWARE;
    }
    if ALSA_PLUGIN_MARKERS.iter().any(|m| lower.contains(m)) {
        return TIER_PLUGIN;
    }
    TIER_OTHER
}

/// Is this PipeWire's own capture device?
pub fn is_pipewire(name: &str) -> bool {
    probe_priority(name) == TIER_PIPEWIRE
}

/// Is this the PulseAudio / ALSA compatibility alias of the audio server's
/// capture device (`pulse`, `default`)?
///
/// On a PipeWire system all three names resolve to the same source, so the
/// aliases add nothing once `pipewire` is in the list.
pub fn is_server_alias(name: &str) -> bool {
    matches!(probe_priority(name), TIER_PULSE | TIER_DEFAULT)
}

/// The capture devices in the order they should be probed.
///
/// Pure: the same names always come back in the same order, and the order is
/// stable (equal tiers keep the caller's order).
///
/// When PipeWire is present its `pulse` and `default` aliases are dropped:
/// probing the same microphone under three names would spend the long probe
/// window ([`SLOW_PROBE_WAIT_MS`]) three times before any real hardware is
/// reached. A machine without PipeWire still probes `pulse` and `default`,
/// so the compatibility path is never lost.
pub fn probe_order<'a>(names: &[&'a str]) -> Vec<&'a str> {
    let pipewire_present = names.iter().any(|n| is_pipewire(n));
    let mut ordered: Vec<&'a str> = names
        .iter()
        .copied()
        .filter(|n| !(pipewire_present && is_server_alias(n)))
        .collect();
    ordered.sort_by_key(|n| probe_priority(n));
    ordered
}

/// Index (into `candidates`) of the device to reopen when every probe
/// reported silence.
///
/// `candidates` are `(name, rms)` pairs in the order they were probed.
///
/// A real capture path (`pipewire`, `pulse`, `default`, `hw:` …) always wins
/// over an ALSA plugin or a loopback, even when the plugin/loopback reports
/// a higher RMS: plugins and loopbacks are not microphones (see the module
/// docs), so reopening one can never let the wake word hear the user, while
/// a real device may start working as soon as the input is unmuted. Among
/// the real devices the highest RMS wins; the `>=` tie-break keeps the last
/// one, which is also what guarantees a fallback exists when every RMS is
/// exactly 0.0. When no real device exists the overall highest-RMS device is
/// returned.
pub fn silent_fallback_index(candidates: &[(&str, f32)]) -> Option<usize> {
    let mut best: Option<usize> = None;
    let mut best_rms = 0.0f32;
    let mut best_real: Option<usize> = None;
    let mut best_real_rms = 0.0f32;

    for (i, (name, rms)) in candidates.iter().enumerate() {
        // `>=` (not `>`) so that even a device with RMS = 0.0 is kept —
        // otherwise an all-silent machine would have no fallback at all.
        if best.is_none() || *rms >= best_rms {
            best = Some(i);
            best_rms = *rms;
        }
        // Real = any tier before the plugin tier (plugins and loopbacks
        // are the last two tiers).
        if probe_priority(name) < TIER_PLUGIN && (best_real.is_none() || *rms >= best_real_rms) {
            best_real = Some(i);
            best_real_rms = *rms;
        }
    }

    best_real.or(best)
}

/// How long to wait for the first audio callback when probing a device.
///
/// The audio server's own capture path (`pipewire`, `pulse`, `default`)
/// needs [`SLOW_PROBE_WAIT_MS`]: PipeWire's ALSA bridge delivers its first
/// callback late, and the normal window would report a good microphone as
/// silent. Every other device keeps the normal window.
///
/// `pulse` and `default` only reach the probe when PipeWire is absent (see
/// [`probe_order`]), so their long window is never paid on a PipeWire
/// machine for the same source twice.
pub fn probe_wait_ms(name: &str) -> u64 {
    let lower = name.trim().to_lowercase();
    if lower.starts_with("pipewire") || lower.starts_with("pulse") || lower == "default" {
        SLOW_PROBE_WAIT_MS
    } else {
        PROBE_WAIT_MS
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

    // Probe in the same order the wake-word capture uses: the real capture
    // paths first, the ALSA plugin devices and the loopbacks last (a
    // loopback is only accepted when nothing else exists — a machine whose
    // only "input" is a monitor would otherwise be reported as broken).
    let mut remaining: Vec<(String, &cpal::Device)> = devices
        .iter()
        .map(|d| (d.name().unwrap_or_default(), d))
        .collect();
    let names: Vec<String> = remaining.iter().map(|(n, _)| n.clone()).collect();
    let name_refs: Vec<&str> = names.iter().map(|n| n.as_str()).collect();
    let mut ordered: Vec<&cpal::Device> = Vec::new();
    for wanted in probe_order(&name_refs) {
        let idx = remaining.iter().position(|(n, _)| n.as_str() == wanted);
        if let Some(idx) = idx {
            ordered.push(remaining.remove(idx).1);
        }
    }

    for d in ordered {
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
        }
    }

    #[test]
    fn the_server_capture_path_is_probed_first() {
        // PipeWire's own capture device is the real microphone on a modern
        // Linux desktop: it must be probed before everything else.
        assert_eq!(probe_priority("pipewire"), TIER_PIPEWIRE);
        assert_eq!(probe_priority("pulse"), TIER_PULSE);
        assert_eq!(probe_priority("default"), TIER_DEFAULT);
        // Real hardware comes after the server's own path.
        assert_eq!(probe_priority("hw:CARD=sofhdadsp,DEV=6"), TIER_HARDWARE);
        assert_eq!(probe_priority("plughw:CARD=PCH,DEV=0"), TIER_HARDWARE);
        assert_eq!(probe_priority("sysdefault:CARD=PCH,DEV=0"), TIER_HARDWARE);
        // "sysdefault:…" is a hardware device, not ALSA's "default" device.
        assert_ne!(probe_priority("sysdefault:CARD=PCH,DEV=0"), TIER_DEFAULT);
    }

    #[test]
    fn alsa_plugins_and_loopbacks_are_probed_last() {
        for name in [
            "lavrate",
            "samplerate",
            "speexrate",
            "speex",
            "upmix",
            "vdownmix",
            "jack",
            "oss",
            "null",
            "dmix",
            "dsnoop",
        ] {
            let p = probe_priority(name);
            assert!(
                p >= TIER_PLUGIN,
                "'{name}' is an ALSA plugin/virtual device and must be probed last (got {p})"
            );
            assert!(
                p > probe_priority("hw:CARD=sofhdadsp,DEV=6"),
                "'{name}' must come after real hardware (got {p})"
            );
        }
        // A loopback carries speaker output — never a microphone.
        assert!(probe_priority("Monitor of Built-in Audio Analog Stereo") >= TIER_LOOPBACK);
    }

    #[test]
    fn the_device_list_of_the_target_machine_sorts_into_the_right_order() {
        // The list the app logged on Pop!_OS / COSMIC, in ALSA order.
        let names = [
            "lavrate",
            "jack",
            "pipewire",
            "pulse",
            "default",
            "hw:CARD=sofhdadsp,DEV=6",
            "hw:CARD=sofhdadsp,DEV=7",
        ];
        // `pulse` and `default` are the same PipeWire source under other
        // names, so they are not probed a second and third time.
        assert_eq!(
            probe_order(&names),
            vec![
                "pipewire",
                "hw:CARD=sofhdadsp,DEV=6",
                "hw:CARD=sofhdadsp,DEV=7",
                "lavrate",
                "jack",
            ]
        );
    }

    #[test]
    fn pulse_and_default_are_probed_when_pipewire_is_absent() {
        // Without PipeWire the PulseAudio/ALSA compatibility path is the only
        // way to reach the microphone — it must still be probed.
        let names = [
            "lavrate",
            "jack",
            "pulse",
            "default",
            "hw:CARD=sofhdadsp,DEV=6",
        ];
        assert_eq!(
            probe_order(&names),
            vec![
                "pulse",
                "default",
                "hw:CARD=sofhdadsp,DEV=6",
                "lavrate",
                "jack",
            ]
        );
    }

    #[test]
    fn the_silent_fallback_prefers_a_real_device_over_a_plugin() {
        // Every device is silent — the exact list from the target machine,
        // in the order the probe really walks it. `lavrate` and `jack` open
        // happily and deliver silence, so reopening one would leave the wake
        // word listening to nothing; the physical microphone must win.
        let names = [
            "lavrate",
            "jack",
            "pipewire",
            "pulse",
            "default",
            "hw:CARD=sofhdadsp,DEV=6",
            "hw:CARD=sofhdadsp,DEV=7",
        ];
        let probed = probe_order(&names);
        let candidates: Vec<(&str, f32)> = probed.iter().map(|n| (*n, 0.0)).collect();
        let chosen = silent_fallback_index(&candidates).map(|i| candidates[i].0);

        // The `>=` tie-break keeps the last real device probed.
        assert_eq!(chosen, Some("hw:CARD=sofhdadsp,DEV=7"));
        assert_ne!(chosen, Some("jack"));
        assert_ne!(chosen, Some("lavrate"));
        assert!(probe_priority(chosen.unwrap()) < TIER_PLUGIN);
    }

    #[test]
    fn the_silent_fallback_ignores_a_noisy_plugin() {
        // A plugin's RMS is electrical noise, not speech: even when it is
        // higher than a silent microphone's, the microphone is the device
        // that may start working once it is unmuted.
        let candidates = [("hw:CARD=sofhdadsp,DEV=7", 0.0), ("jack", 0.004)];
        assert_eq!(silent_fallback_index(&candidates), Some(0));
    }

    #[test]
    fn the_silent_fallback_still_exists_without_a_real_device() {
        // A machine whose only "inputs" are plugins/loopbacks must still get
        // a fallback device (the `>=` tie-break keeps the last one).
        let candidates = [("lavrate", 0.0), ("jack", 0.0)];
        assert_eq!(silent_fallback_index(&candidates), Some(1));
        assert_eq!(silent_fallback_index(&[]), None);
    }

    #[test]
    fn the_probe_order_is_stable() {
        // Two devices of the same tier keep the order they were listed in.
        let names = ["hw:CARD=B,DEV=0", "hw:CARD=A,DEV=0", "pipewire"];
        assert_eq!(
            probe_order(&names),
            vec!["pipewire", "hw:CARD=B,DEV=0", "hw:CARD=A,DEV=0"]
        );
    }

    #[test]
    fn the_server_capture_path_gets_the_long_probe_window() {
        // PipeWire's ALSA bridge delivers its first callback late.
        assert_eq!(probe_wait_ms("pipewire"), SLOW_PROBE_WAIT_MS);
        assert_eq!(probe_wait_ms("pulse"), SLOW_PROBE_WAIT_MS);
        assert_eq!(probe_wait_ms("default"), SLOW_PROBE_WAIT_MS);
        // Everything else keeps the normal window.
        assert_eq!(probe_wait_ms("hw:CARD=sofhdadsp,DEV=6"), PROBE_WAIT_MS);
        assert_eq!(probe_wait_ms("lavrate"), PROBE_WAIT_MS);
        assert_eq!(probe_wait_ms(""), PROBE_WAIT_MS);
    }

    #[test]
    fn empty_name_is_not_a_loopback() {
        assert!(!is_loopback_device(""));
        assert_eq!(probe_priority(""), TIER_OTHER);
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
