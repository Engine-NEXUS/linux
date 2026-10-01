# NEXUS — Quick Start (Linux)

## 1. Install

**Dev loop** (live code + hot reload):

```bash
./scripts/dev.sh
```

Checks system deps, installs frontend modules if missing, runs `cargo tauri dev`.

**Dev-local binary** (fast cargo build + symlink + `.desktop` + hotkey):

```bash
./install.sh
```

**Production** (latest `.deb` from the `linux` remote releases):

```bash
curl -fsSL https://raw.githubusercontent.com/Engine-NEXUS/linux/main/install-prod.sh | bash
```

No `.env` needed. Backend Worker URL is hardcoded (`src-tauri/src/commands.rs`
`WORKER_URL`).

## 2. Interact

- **Hotkey:** `Super+Space` (auto-registered via `scripts/register-hotkey.sh`;
  COSMIC fallback: Settings → Keyboard → Custom Shortcut → `nexus --wake`).
- **Wake word:** say `"NEXUS"`.
- **Manual:** `nexus --wake` (cold start or running), `nexus --setup` (wizard).

## 3. Test scenarios

- `"open firefox"` / `"close firefox"` — `.desktop` launch incl. Flatpak.
- `"take screenshot"` — `cosmic-screenshot` first, grim+slurp fallback
  (`sudo apt install grim slurp wtype`).
- `"mute"` — `wpctl` toggle.
- Architecture mapper / blast radius / PR review — via Worker backend.

## 4. Troubleshoot

| Issue | Fix |
| :--- | :--- |
| Mic silent (Linux) | `pavucontrol` → Input Devices → pick real mic, not `.monitor` |
| Hotkey dead (COSMIC) | Re-run `./scripts/register-hotkey.sh`, else manual keybind |
| No display in packaged app | Rebuild via `scripts/build-prod.sh`, never plain `cargo build` |
| **Microphone is silent or blocked** | Press hotkey to trigger listening directly. On Linux check `pavucontrol`; on Windows check *Settings → Privacy → Microphone*. |
| **Intel SST mic goes silent (RMS=0.000000)** | Intel SST driver stops delivering audio after 2-25 min (Linux + Windows). Linux: restart PipeWire / re-plug; Windows: **Device Manager → Intel SST → Disable → Enable** or `Restart-Service Audiosrv` (Admin). NEXUS auto-recovers via silence thread. |
| **STT returns truncated transcripts ("open-")** | Caused by mic silence above. NEXUS auto-corrects "open-" → "open architecture mapper". |
| **Want to change voice settings** | Tray icon → **Settings** → **Audio & Voice**. |
| **Need diagnostics report** | Tray icon → **Diagnostics** (checks STT, TTS, Worker, OAuth). |
