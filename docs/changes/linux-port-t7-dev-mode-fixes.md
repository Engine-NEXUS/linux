# Linux port T7 — dev-mode fixes found on the target machine

Scope: `src-tauri/src/gdk_backend_linux.rs`, `src-tauri/src/audio_linux.rs`,
and the small Linux-gated call-outs in `src-tauri/src/wakeword_oww.rs`.

## Why

The coordinator ran `tauri dev` on the target machine (Pop!_OS 24.04, COSMIC,
Wayland + XWayland, PipeWire). Three things were broken in the app's own log.

| Log line | What it means | Cause |
|---|---|---|
| `[GDK] GDK_BACKEND=wayland,x11 was set — keeping the user's GDK_BACKEND` | the overlay ran on the **native Wayland** backend, so it was not kept above the other windows and the orb was invisible behind VS Code | COSMIC exports `GDK_BACKEND=wayland,x11` for the **whole session**; that is not a choice for NEXUS, but the bootstrap treated any inherited value as the user's own decision |
| `audio: device produced 0 samples in 500ms — no audio callback fired` … then the `lavrate` device was **accepted** | the wake word listened to an ALSA rate-converter plugin that delivers silence — the microphone was never heard | the probe tried ALSA's `default` first, gave up after 500 ms, and the probe order put the plugin devices (`lavrate`, `jack`) before the audio server's real capture path |
| `audio: mic has been silent … Intel SST driver may need a restart. Try: 1) Unmute mic in Windows settings` / `device: 'Microphone Array (Intel® Smart '` | the log described a **Windows** microphone that does not exist on this machine | shared log wording, and the acoustic profile that ships with the app was calibrated on a Windows machine |

## What changed

### `src-tauri/src/gdk_backend_linux.rs`

The pure decision `choose()` now follows one rule, and the module doc says
why:

1. **`NEXUS_GDK_BACKEND` set** → use exactly that value as `GDK_BACKEND`
   (an explicit, NEXUS-scoped override — the escape hatch for a user who
   really wants the native Wayland backend).
2. **otherwise `DISPLAY` set** (XWayland is available) → `GDK_BACKEND=x11`,
   **replacing** whatever the session exported. The replaced value is kept in
   the decision (`ForceX11 { replaced }`) so the startup log can name it.
3. **otherwise** → leave `GDK_BACKEND` alone: there is no X server, so GTK
   keeps whatever it finds (native Wayland).

`Backend::RespectUser` is gone — an inherited session value is no longer
mistaken for a user decision. The Windows and macOS code paths are untouched
(the module is Linux-only).

### `src-tauri/src/audio_linux.rs`

* **`probe_priority()` is now a tier, not a flag.** The tiers, lowest first:
  `pipewire` → `pulse` → `default` → real hardware (`hw:`, `plughw:`,
  `sysdefault:`) → everything else → ALSA plugin / virtual devices
  (`lavrate`, `samplerate`, `speexrate`, `speex`, `upmix`, `vdownmix`,
  `jack`, `oss`, `null`, `dmix`, `dsnoop`) → output loopbacks
  ("Monitor of …"). The loopback exclusion is unchanged — a monitor source
  still carries speaker output and is never a microphone.
  * `default` is matched **exactly**: `sysdefault:…` is a hardware device,
    not ALSA's `default` device.
* **`probe_order()`** — the pure ordering function (stable: equal tiers keep
  the caller's order). On a PipeWire system `pulse` and `default` are the
  same source under other names, so they are **dropped from the order when
  `pipewire` is present**; without PipeWire they are still probed. This also
  means the 2 s window is not paid three times for one microphone.
* **`silent_fallback_index()`** — when every device probe reports silence,
  the fallback device is the best **real** capture path
  (`pipewire`/`pulse`/`default`, `hw:` …), never an ALSA plugin or a loopback,
  even when the plugin reports a higher RMS. Plugins and loopbacks are not
  microphones, so reopening one can never let the wake word hear the user;
  a real device may start working once the input is unmuted. The `>=`
  tie-break is kept, so an all-silent machine still has a fallback (and a
  plugin-only machine still falls back to the best plugin rather than none).
* **`probe_wait_ms()`** — the audio server's own capture path
  (`pipewire` / `pulse` / `default`) gets a **2000 ms** window instead of
  500 ms: PipeWire's ALSA bridge delivers its first callback late, and the
  short window reported a perfectly good microphone as silent. Every other
  device keeps the 500 ms window. `pulse`/`default` only reach the probe when
  PipeWire is absent, so their long window is never paid twice for the same
  source.
* **`capture_permission_state()`** probes in the same order as the wake-word
  capture (it used to keep its own two-bucket split).
* **`KWS_DEVICE_PENDING`** and **`SILENT_MIC_HINT`** — the two Linux wordings
  the shared log lines need (see below).

### `src-tauri/src/wakeword_oww.rs` (Linux-gated call-outs only)

* The device probe passes the per-device window into `try_device()`; the
  "0 samples" log line now names the window it actually waited
  (`0 samples in 2000ms`).
* The device list is de-duplicated before probing: when `pipewire` is
  present, its `pulse` and `default` aliases are skipped, so one source is
  not probed with the long window three times.
* When every probe reports silence, the fallback reopens the best **real**
  device (`audio_linux::silent_fallback_index`), not the last-probed ALSA
  plugin: on the target machine's list that is
  `hw:CARD=sofhdadsp,DEV=7`, not `jack`/`lavrate`.
* The wake-word init line prints the device as **pending** on Linux instead of
  the acoustic profile's device name (the shipped profile was calibrated on a
  Windows machine: *"Microphone Array (Intel® Smart …)"*). The device that is
  really in use is named by the capture step, which on Linux says so
  explicitly: *"stream started on 'pipewire', OWW KWS listening for 'nexus'
  (this is the capture device actually in use)…"*.
* The silent-microphone warning uses the Linux hint on Linux (check the input
  device and its volume in Settings > Sound or `wpctl status`, and make sure
  PipeWire is running). The Windows wording is byte-for-byte unchanged.
* Two more log lines in the same probe path named the Intel SST driver, which
  does not exist on this machine: *"device RMS … is zero — Intel SST driver
  lockout"* and *"ALL devices produced silence … the Intel SST driver may be
  broken"*. They now name the Linux causes (muted input, wrong source,
  PipeWire not running). The Windows wording is byte-for-byte unchanged.

## Not done / known gaps

* A device that opens but delivers only electrical noise (RMS just above the
  1e-6 floor) is still accepted as a working microphone. The probe order and
  the all-silent fallback now prefer real devices, but a noisy plugin/loopback
  that exceeds the floor before a real device is reached would still be
  accepted. A minimum-RMS floor per tier would need a measured threshold.
* The acoustic profile that ships with the app (highpass 128.3 Hz, pre-gain
  2.5x, threshold 0.68) was calibrated on a Windows laptop. Its DSP values are
  still used on Linux; only the (wrong) device name is ignored. A Linux
  calibration profile is a separate piece of work.
* The silent-microphone warning is a log line only. It is not spoken, and it
  is not raised in the UI.
