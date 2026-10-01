# NEXUS Meeting & Privacy Protection

This directory documents the multi-layered meeting detection and audio privacy protection system in NEXUS.

---

## 🛡️ Key Documentation

- **[01-meeting-detection.md](01-meeting-detection.md)** — Architectural specification for the 4-layer meeting privacy engine:
  - **Layer 0 (Manual Pause)**: System tray override toggle (`manual_pause`).
  - **Layer 1 (WASAPI Audio Session Probing)**: Real-time Windows audio session enumeration detecting background mic usage by communication apps (Zoom, Teams, Google Meet, Discord).
  - **Layer 2 (Process Scanning)**: Verification of running meeting client binaries.
  - **Layer 3 (TTS & Audio Drain)**: Instant 80ms audio ring-buffer draining and wake word suppression to guarantee zero call interruptions.
