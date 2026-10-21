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
//! ALSA's `default` first — it is the audio server's own route — then
//! `pulse`, then the real hardware (`hw:`, `plughw:`, `sysdefault:`), then
//! the raw ALSA `pipewire` PCM, then everything else, and only last the ALSA
//! plugin devices and the loopback/monitor sources.
//!
//! The raw `pipewire` PCM deliberately comes after the hardware: on the
//! target machine it blocked **forever** inside `default_input_config()`
//! (CPAL never returned from the ALSA PCM), so probing it first stopped the
//! wake word from starting at all. Every Linux probe therefore runs on its
//! own thread behind a hard deadline ([`run_probe_with_deadline`]): a device
//! that does not answer within its probe window plus a grace period is
//! abandoned — never joined — and the next device is tried.
//!
//! When every device turns out to be silent, [`silent_fallback_index`]
//! reopens the best **real** capture path rather than a plugin or a loopback:
//! a plugin will never become a microphone, while a real device may start
//! working as soon as the input is unmuted. On equal RMS the first device
//! probed wins, so an all-silent machine falls back to `default` instead of
//! the raw `pipewire` PCM that is known to hang.
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

/// ALSA's `default` capture device — the audio server's own route.
const TIER_DEFAULT: u8 = 0;
/// The PulseAudio capture device (the compatibility layer).
const TIER_PULSE: u8 = 1;
/// A real hardware capture device (`hw:`, `plughw:`, `sysdefault:`).
const TIER_HARDWARE: u8 = 2;
/// The raw ALSA PipeWire PCM — observed hanging inside
/// `default_input_config()`, so it is never probed before real hardware.
const TIER_PIPEWIRE: u8 = 3;
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
/// 0. `default` — ALSA's default device, the audio server's own route;
/// 1. `pulse` — the PulseAudio compatibility layer;
/// 2. `hw:` / `plughw:` / `sysdefault:` — real hardware;
/// 3. `pipewire` — the raw ALSA PipeWire PCM, which was observed hanging
///    inside `default_input_config()`; only tried after real hardware;
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
    // Exact match only: "sysdefault:…" is a hardware device, not ALSA's
    // "default" device.
    if lower == "default" {
        return TIER_DEFAULT;
    }
    if lower.starts_with("pulse") {
        return TIER_PULSE;
    }
    if lower.starts_with("hw:") || lower.starts_with("plughw:") || lower.starts_with("sysdefault:")
    {
        return TIER_HARDWARE;
    }
    if lower.starts_with("pipewire") {
        return TIER_PIPEWIRE;
    }
    if ALSA_PLUGIN_MARKERS.iter().any(|m| lower.contains(m)) {
        return TIER_PLUGIN;
    }
    TIER_OTHER
}

/// Is this the raw ALSA PipeWire capture device?
pub fn is_pipewire(name: &str) -> bool {
    probe_priority(name) == TIER_PIPEWIRE
}

/// The capture devices in the order they should be probed.
///
/// Pure: the same names always come back in the same order, and the order is
/// stable (equal tiers keep the caller's order).
///
/// Every device stays in the list — including `pulse` and `default` when the
/// raw `pipewire` PCM is present. They are not skipped as aliases: `default`
/// is the route that works on the target machine, while the raw `pipewire`
/// PCM is the one that can hang, so it must never displace them
/// ([`probe_priority`] puts it after real hardware).
pub fn probe_order<'a>(names: &[&'a str]) -> Vec<&'a str> {
    let mut ordered: Vec<&'a str> = names.to_vec();
    ordered.sort_by_key(|n| probe_priority(n));
    ordered
}

/// Index (into `candidates`) of the device to reopen when every probe
/// reported silence.
///
/// `candidates` are `(name, rms)` pairs in the order they were probed.
///
/// A real capture path (`default`, `pulse`, `hw:` …) always wins over an
/// ALSA plugin or a loopback, even when the plugin/loopback reports a
/// higher RMS: plugins and loopbacks are not microphones (see the module
/// docs), so reopening one can never let the wake word hear the user, while
/// a real device may start working as soon as the input is unmuted. Among
/// the real devices the highest RMS wins; on a tie the **first** device
/// probed wins (`>`), so an all-silent machine falls back to `default` — the
/// audio server's own route — rather than the raw `pipewire` PCM that is
/// known to hang. When no real device exists the overall highest-RMS device
/// is returned (again first-wins on ties), so a fallback always exists even
/// when every RMS is exactly 0.0.
pub fn silent_fallback_index(candidates: &[(&str, f32)]) -> Option<usize> {
    let mut best: Option<usize> = None;
    let mut best_rms = 0.0f32;
    let mut best_real: Option<usize> = None;
    let mut best_real_rms = 0.0f32;

    for (i, (name, rms)) in candidates.iter().enumerate() {
        // `is_none()` keeps even a device with RMS = 0.0 as a fallback —
        // otherwise an all-silent machine would have no fallback at all.
        // `>` (not `>=`) keeps the FIRST device probed on a tie.
        if best.is_none() || *rms > best_rms {
            best = Some(i);
            best_rms = *rms;
        }
        // Real = any tier before the plugin tier (plugins and loopbacks
        // are the last two tiers).
        if probe_priority(name) < TIER_PLUGIN && (best_real.is_none() || *rms > best_real_rms) {
            best_real = Some(i);
            best_real_rms = *rms;
        }
    }

    best_real.or(best)
}

/// How long to wait for the first audio callback when probing a device.
///
/// The audio server's own capture paths (`default`, `pulse` and the raw
/// `pipewire` PCM) need [`SLOW_PROBE_WAIT_MS`]: PipeWire's ALSA bridge
/// delivers its first callback late, and the normal window would report a
/// good microphone as silent. Every other device keeps the normal window.
///
/// `default` and `pulse` are always probed now — they are no longer skipped
/// when the raw `pipewire` PCM is present (see [`probe_order`]) — and all
/// three names reach the same slow-starting source, so all three get the
/// long window.
pub fn probe_wait_ms(name: &str) -> u64 {
    let lower = name.trim().to_lowercase();
    if is_pipewire(name) || lower.starts_with("pulse") || lower == "default" {
        SLOW_PROBE_WAIT_MS
    } else {
        PROBE_WAIT_MS
    }
}

// ─── Hard probe deadline (a hung device must never stop the wake word) ──────

/// Extra time a probe thread gets beyond the device's probe window before it
/// is declared hung.
///
/// `default_input_config()` and `build_input_stream()` open the ALSA PCM and
/// normally answer in milliseconds; the probe window itself is then spent
/// waiting for the first audio callback. A device that has not returned from
/// that call within the probe window plus this grace period is not going to
/// answer — on the target machine the raw `pipewire` PCM never did.
pub const PROBE_DEADLINE_GRACE_MS: u64 = 3000;

/// Deadline for the enumeration-only metadata query.
///
/// The device listing opens each PCM once just to print its sample rate and
/// channel count — it never waits for an audio callback. A hung device must
/// therefore not cost the full probe deadline *twice* (once while listing,
/// once while probing); this short fixed deadline bounds the listing, and the
/// caller remembers the device that timed out and skips it in the probe loop.
pub const LIST_QUERY_DEADLINE_MS: u64 = 1500;

/// Hard deadline for one Linux device probe: its probe window
/// ([`probe_wait_ms`]) plus [`PROBE_DEADLINE_GRACE_MS`].
pub fn probe_deadline_ms(name: &str) -> u64 {
    probe_wait_ms(name) + PROBE_DEADLINE_GRACE_MS
}

/// Why a probe that ran on its own thread did not deliver a result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProbeWaitError {
    /// The probe did not finish inside its deadline. The thread is left
    /// running and must be abandoned (never joined).
    TimedOut,
    /// The probe thread could not be started, panicked, or its result could
    /// not be received.
    Failed,
}

// ─── Probe state: RUNNING → CANCELLED, or RUNNING → INSTALLED ────────────────
//
// The deadline races against the worker's install. If the worker stores its
// stream just before the caller's `recv_timeout` expires, a plain
// "cancelled" flag would make the caller report a timeout and move on while
// the stream it just installed stays live. A three-state cell resolves the
// race: whichever side wins the compare-exchange settles the probe, and the
// loser can see what happened.

const PROBE_RUNNING: u8 = 0;
const PROBE_CANCELLED: u8 = 1;
const PROBE_INSTALLED: u8 = 2;

/// What [`ProbeState::try_cancel`] found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProbeCancelOutcome {
    /// The probe is now cancelled; its result must be discarded.
    Cancelled,
    /// The probe was already cancelled (two timeout paths raced).
    AlreadyCancelled,
    /// The probe already installed its stream: the device was accepted.
    AlreadyInstalled,
}

/// The shared state of one device probe.
///
/// Cloned into the worker thread (it is an `Arc`); the caller keeps one clone.
/// The worker claims the install slot with [`try_install`](Self::try_install)
/// inside the critical section that stores the stream; the caller claims the
/// cancellation with [`try_cancel`](Self::try_cancel) when its deadline
/// expires. Both transitions are one-time, so exactly one of them wins.
#[derive(Clone, Default)]
pub struct ProbeState {
    state: std::sync::Arc<std::sync::atomic::AtomicU8>,
}

impl ProbeState {
    /// True once the probe has been abandoned.
    pub fn is_cancelled(&self) -> bool {
        self.state.load(std::sync::atomic::Ordering::SeqCst) == PROBE_CANCELLED
    }

    /// Claim the install slot (`RUNNING -> INSTALLED`).
    ///
    /// Returns `false` when the probe was cancelled (or already installed), in
    /// which case the worker must not store its stream.
    pub fn try_install(&self) -> bool {
        self.state
            .compare_exchange(
                PROBE_RUNNING,
                PROBE_INSTALLED,
                std::sync::atomic::Ordering::SeqCst,
                std::sync::atomic::Ordering::SeqCst,
            )
            .is_ok()
    }

    /// Abandon the probe (`RUNNING -> CANCELLED`).
    ///
    /// A cancelled probe cannot install afterwards. If the install won the
    /// race first, the caller learns it through
    /// [`ProbeCancelOutcome::AlreadyInstalled`] and must treat the device as
    /// accepted instead of probing another one over it.
    pub fn try_cancel(&self) -> ProbeCancelOutcome {
        match self.state.compare_exchange(
            PROBE_RUNNING,
            PROBE_CANCELLED,
            std::sync::atomic::Ordering::SeqCst,
            std::sync::atomic::Ordering::SeqCst,
        ) {
            Ok(_) => ProbeCancelOutcome::Cancelled,
            Err(current) if current == PROBE_INSTALLED => ProbeCancelOutcome::AlreadyInstalled,
            Err(_) => ProbeCancelOutcome::AlreadyCancelled,
        }
    }
}

/// Run `probe` on its own thread and wait at most `deadline` for it.
///
/// On Linux, CPAL's `default_input_config()` and `build_input_stream()` can
/// block **forever** inside ALSA (the raw `pipewire` PCM did exactly that),
/// so the capture setup must never call them on the thread that starts the
/// wake word. The probe gets a worker thread; on timeout the caller receives
/// [`ProbeWaitError::TimedOut`] and is expected to abandon the handle and
/// try the next device.
///
/// The worker handle is deliberately dropped, never joined: joining a hung
/// ALSA call would block for exactly as long as the call does, which is what
/// this helper exists to prevent.
///
/// The probe closure must be self-contained (`'static`): it may outlive its
/// deadline. Callers that install shared state (a capture stream) must give
/// the closure a [`ProbeState`] so a late return cannot overwrite the device
/// the caller settled on, and so a stream installed just before the deadline
/// is reported as accepted rather than as a timeout.
pub fn run_probe_with_deadline<T, F>(
    deadline: std::time::Duration,
    probe: F,
) -> Result<T, ProbeWaitError>
where
    T: Send + 'static,
    F: FnOnce() -> T + Send + 'static,
{
    let (tx, rx) = std::sync::mpsc::channel();
    // A failed send only means the caller already gave up (timeout) — the
    // result is simply dropped.
    let _worker = match std::thread::Builder::new()
        .name("nexus-device-probe".into())
        .spawn(move || {
            let _ = tx.send(probe());
        }) {
        Ok(handle) => handle,
        Err(_) => return Err(ProbeWaitError::Failed),
    };
    match rx.recv_timeout(deadline) {
        Ok(value) => Ok(value),
        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => Err(ProbeWaitError::TimedOut),
        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => Err(ProbeWaitError::Failed),
    }
}

/// Open ONE capture device by name, without keeping any other device open.
///
/// cpal's ALSA enumeration opens every listed PCM (both directions) and keeps
/// it open for as long as the `Device` lives. A held raw DMIC (`hw:…`) keeps
/// PipeWire's own microphone source from opening the hardware, so capture
/// through `default` / `pulse` then stalls inside `snd_pcm_prepare` and
/// delivers nothing (reproduced on the target machine: PipeWire's `Mic1`
/// source is `hw:sofhdadsp,6`). Probing therefore keeps only names and
/// reopens each candidate right before its probe; `Iterator::find` drops
/// (closes) every other device of this enumeration as it goes. `default`
/// needs no enumeration at all: cpal opens it lazily.
pub fn reopen_input_device(host: &cpal::Host, name: &str) -> Option<cpal::Device> {
    use cpal::traits::{DeviceTrait, HostTrait};

    if name == "default" {
        if let Some(d) = host.default_input_device() {
            if d.name().map(|n| n == "default").unwrap_or(false) {
                return Some(d);
            }
        }
    }
    host.input_devices()
        .ok()?
        .find(|d| d.name().map(|n| n == name).unwrap_or(false))
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

    // Keep only the names: the enumerated devices hold every PCM open (see
    // `reopen_input_device`), which would keep PipeWire off the microphone
    // while `default` is being opened.
    let ordered_names: Vec<String> = ordered
        .iter()
        .map(|d| d.name().unwrap_or_else(|_| "unknown".into()))
        .collect();
    drop(ordered);
    drop(remaining);
    drop(devices);

    for name in ordered_names {
        let Some(d) = reopen_input_device(&host, &name) else {
            continue;
        };
        // Opening a device is a probe too: `default_input_config()` and
        // `build_input_stream()` can block forever inside ALSA (the raw
        // `pipewire` PCM did), and this runs during app setup. Bound it with
        // the same deadline the wake-word probe uses; a device that does not
        // answer is treated as unusable instead of freezing startup.
        let device = d.clone();
        let deadline = std::time::Duration::from_millis(probe_deadline_ms(&name));
        let opened = match run_probe_with_deadline(deadline, move || device_opens(&device)) {
            Ok(opened) => opened,
            Err(_) => {
                tracing::warn!(
                    "audio_linux: capture device '{name}' did not answer within {}ms — treating it as unusable",
                    deadline.as_millis()
                );
                false
            }
        };
        if opened {
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
    fn default_comes_first_then_pulse_then_hardware_then_pipewire() {
        // `default` is the audio server's own route and the one that works on
        // the target machine: it must be probed before everything else.
        assert_eq!(probe_priority("default"), TIER_DEFAULT);
        assert_eq!(probe_priority("pulse"), TIER_PULSE);
        // Real hardware comes after the server's first two routes.
        assert_eq!(probe_priority("hw:CARD=sofhdadsp,DEV=6"), TIER_HARDWARE);
        assert_eq!(probe_priority("plughw:CARD=PCH,DEV=0"), TIER_HARDWARE);
        assert_eq!(probe_priority("sysdefault:CARD=PCH,DEV=0"), TIER_HARDWARE);
        // "sysdefault:…" is a hardware device, not ALSA's "default" device.
        assert_ne!(probe_priority("sysdefault:CARD=PCH,DEV=0"), TIER_DEFAULT);
        // The raw ALSA PipeWire PCM — the device that hung — comes last of
        // the real routes, after hardware.
        assert_eq!(probe_priority("pipewire"), TIER_PIPEWIRE);
        assert!(TIER_PIPEWIRE > TIER_HARDWARE);
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
        // `default` is the route that works on this machine; the raw
        // `pipewire` PCM — which hung inside `default_input_config()` — is
        // probed only after `default`, `pulse` and the real hardware.
        assert_eq!(
            probe_order(&names),
            vec![
                "default",
                "pulse",
                "hw:CARD=sofhdadsp,DEV=6",
                "hw:CARD=sofhdadsp,DEV=7",
                "pipewire",
                "lavrate",
                "jack",
            ]
        );
    }

    #[test]
    fn pipewire_does_not_hide_the_default_and_pulse_routes() {
        // The T7 alias dedupe skipped `default` and `pulse` whenever the raw
        // `pipewire` PCM was present — which skipped the only route that
        // worked. Every name must stay in the probe list.
        let names = ["pipewire", "pulse", "default"];
        assert_eq!(probe_order(&names), vec!["default", "pulse", "pipewire"]);
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
                "default",
                "pulse",
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
        // word listening to nothing; a real route must win.
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

        // The first-wins tie-break (`>`) keeps the FIRST real device probed,
        // which is `default` — the route that works on this machine — not the
        // raw `pipewire` PCM that is known to hang.
        assert_eq!(chosen, Some("default"));
        assert_ne!(chosen, Some("pipewire"));
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
        // a fallback device (the first-wins tie-break keeps the first one).
        let candidates = [("lavrate", 0.0), ("jack", 0.0)];
        assert_eq!(silent_fallback_index(&candidates), Some(0));
        assert_eq!(silent_fallback_index(&[]), None);
    }

    #[test]
    fn the_probe_order_is_stable() {
        // Two devices of the same tier keep the order they were listed in.
        // The raw `pipewire` PCM is a later tier than hardware, so it sorts
        // after both hardware devices even though it was listed first.
        let names = ["hw:CARD=B,DEV=0", "hw:CARD=A,DEV=0", "pipewire"];
        assert_eq!(
            probe_order(&names),
            vec!["hw:CARD=B,DEV=0", "hw:CARD=A,DEV=0", "pipewire"]
        );
    }

    #[test]
    fn the_server_capture_path_gets_the_long_probe_window() {
        // PipeWire's ALSA bridge delivers its first callback late. `default`
        // and `pulse` are always probed (they are no longer skipped when the
        // raw `pipewire` PCM is present) and reach the same slow source, so
        // all three get the long window.
        assert_eq!(probe_wait_ms("pipewire"), SLOW_PROBE_WAIT_MS);
        assert_eq!(probe_wait_ms("pulse"), SLOW_PROBE_WAIT_MS);
        assert_eq!(probe_wait_ms("default"), SLOW_PROBE_WAIT_MS);
        // Everything else keeps the normal window.
        assert_eq!(probe_wait_ms("hw:CARD=sofhdadsp,DEV=6"), PROBE_WAIT_MS);
        assert_eq!(probe_wait_ms("lavrate"), PROBE_WAIT_MS);
        assert_eq!(probe_wait_ms(""), PROBE_WAIT_MS);
    }

    #[test]
    fn the_probe_deadline_is_the_probe_window_plus_the_grace_period() {
        // A device that has not answered within its probe window plus the
        // grace period is hung (the raw `pipewire` PCM never answered at all).
        assert_eq!(
            probe_deadline_ms("default"),
            SLOW_PROBE_WAIT_MS + PROBE_DEADLINE_GRACE_MS
        );
        assert_eq!(
            probe_deadline_ms("pulse"),
            SLOW_PROBE_WAIT_MS + PROBE_DEADLINE_GRACE_MS
        );
        assert_eq!(
            probe_deadline_ms("pipewire"),
            SLOW_PROBE_WAIT_MS + PROBE_DEADLINE_GRACE_MS
        );
        assert_eq!(
            probe_deadline_ms("hw:CARD=sofhdadsp,DEV=6"),
            PROBE_WAIT_MS + PROBE_DEADLINE_GRACE_MS
        );
        assert_eq!(
            probe_deadline_ms("lavrate"),
            PROBE_WAIT_MS + PROBE_DEADLINE_GRACE_MS
        );
    }

    #[test]
    fn a_fast_probe_returns_its_value() {
        let value = run_probe_with_deadline(std::time::Duration::from_secs(5), || 42u32);
        assert_eq!(value, Ok(42));
    }

    #[test]
    fn a_probe_that_never_returns_times_out_instead_of_blocking() {
        // The raw `pipewire` PCM blocked forever inside `default_input_config()`.
        // The deadline helper must give up on schedule and leave the worker
        // thread abandoned (never joined) so the wake word can try the next
        // device.
        let deadline = std::time::Duration::from_millis(100);
        let started = std::time::Instant::now();
        // The probe blocks on a channel that is never sent to — a stand-in
        // for a hung ALSA call. It outlives the call by design.
        let (_tx, rx) = std::sync::mpsc::channel::<()>();
        let result: Result<(), ProbeWaitError> = run_probe_with_deadline(deadline, move || {
            let _ = rx.recv();
        });
        let elapsed = started.elapsed();

        assert_eq!(result, Err(ProbeWaitError::TimedOut));
        // It waited out the deadline...
        assert!(
            elapsed >= deadline,
            "returned before the deadline ({elapsed:?} < {deadline:?})"
        );
        // ...but did not block anywhere near as long as the hung device would.
        assert!(
            elapsed < deadline + std::time::Duration::from_secs(5),
            "did not return promptly after the deadline ({elapsed:?})"
        );
    }

    #[test]
    fn the_list_query_deadline_is_shorter_than_every_probe_deadline() {
        // The listing query only opens the PCM (no audio wait), so it gets a
        // short fixed deadline. If it were as long as a probe deadline, a
        // hung device would cost a full deadline in the listing AND another
        // one in the probe loop.
        for name in [
            "default",
            "pulse",
            "pipewire",
            "hw:CARD=sofhdadsp,DEV=6",
            "plughw:CARD=PCH,DEV=0",
            "sysdefault:CARD=PCH,DEV=0",
            "lavrate",
            "Monitor of Built-in Audio Analog Stereo",
            "",
        ] {
            assert!(
                LIST_QUERY_DEADLINE_MS < probe_deadline_ms(name),
                "the listing deadline must be shorter than the probe deadline of '{name}'"
            );
        }
    }

    #[test]
    fn a_cancelled_probe_cannot_install() {
        let state = ProbeState::default();
        assert_eq!(state.try_cancel(), ProbeCancelOutcome::Cancelled);
        assert!(state.is_cancelled());
        // The caller gave up first: a late worker must not store its stream.
        assert!(!state.try_install());
        assert!(state.is_cancelled());
    }

    #[test]
    fn an_installed_probe_reports_installed_when_cancelled() {
        let state = ProbeState::default();
        assert!(state.try_install());
        // The worker won the race: the caller must keep this device instead
        // of probing another one over the live stream.
        assert_eq!(state.try_cancel(), ProbeCancelOutcome::AlreadyInstalled);
        assert!(!state.is_cancelled());
        // The install slot is one-shot.
        assert!(!state.try_install());
    }

    #[test]
    fn double_cancel_is_fine() {
        let state = ProbeState::default();
        assert_eq!(state.try_cancel(), ProbeCancelOutcome::Cancelled);
        assert_eq!(state.try_cancel(), ProbeCancelOutcome::AlreadyCancelled);
        assert!(!state.try_install());
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
