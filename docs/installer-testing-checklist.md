# NEXUS Installer Testing Checklist

**Purpose:** Verify NEXUS works perfectly when installed from the GitHub Actions installer artifact on a fresh laptop.

**Prerequisites on target laptop:**
- Windows 10/11 64-bit
- 16 GB RAM
- Python 3.10+ installed (`python --version` works in Command Prompt)
- Microphone (built-in or USB)
- Internet connection (for first-run model downloads + Cloudflare Worker)

**Installer artifact:** Download from GitHub Actions → `phase-1` branch → `windows-installer` job → `nexus-windows-exe` artifact

---

## Pre-Install Checks

- [ ] Download `nexus-windows-exe` artifact from GitHub Actions
- [ ] Verify the `.exe` file exists and is ~50-100 MB
- [ ] Check Python is installed: `python --version` (needs 3.10+)
- [ ] Check microphone is connected and not muted

## Installation

- [ ] Run `NEXUS_0.1.0_x64-setup.exe`
- [ ] Installer shows white theme with NEXUS branding
- [ ] Install completes without errors
- [ ] NEXUS launches automatically after install (setup wizard)
- [ ] NEXUS tray icon appears in system tray

## First-Run Setup

- [ ] Setup wizard appears
- [ ] Voice selection works (Sky, Adam, or Emma)
- [ ] Click "Launch Assistant" — orb appears at bottom-center
- [ ] Check logs for pre-warm messages:
  - `tts: startup pre-warm starting...`
  - `stt: startup pre-warm starting...`
  - `nlu: startup pre-warm starting...`
- [ ] Wait for pre-warm to complete (check logs):
  - `tts: startup pre-warm complete — engine ready for instant ack`
  - `stt: startup pre-warm complete`
  - `nlu: startup pre-warm complete`
- [ ] Note: First launch may take 30-60s for model downloads (Kokoro 337 MB from GitHub, Whisper ~40 MB from HuggingFace)

## Voice Command Tests

### Test 1: Wake Word + Architecture Mapper
- [ ] Say "Nexus" — orb illuminates
- [ ] Say "open architecture mapper"
- [ ] Hear "On it sir" within 2-3 seconds
- [ ] Orb disappears after "On it sir"
- [ ] Loading animation appears at top-right corner
- [ ] Architecture Mapper window opens with completed map
- [ ] Loading animation disappears after map is shown
- [ ] Map shows correct repository (from active browser tab)

### Test 2: Hotkey + Architecture Mapper
- [ ] Press `Ctrl+Space` — orb illuminates
- [ ] Say "open architecture mapper"
- [ ] Same flow as Test 1

### Test 3: Truncated Transcript Recovery
- [ ] If mic cuts out and STT returns "open-" or "open"
- [ ] Check logs for: `truncated 'open-' → open_architect (mic silence recovery)`
- [ ] Architecture mapper should still open (not Microsoft Office)

### Test 4: App Launch Command
- [ ] Say "Nexus" → "open Chrome" (or any installed app)
- [ ] Hear "On it sir" within 1-2 seconds
- [ ] Chrome launches

### Test 5: Invalid Command
- [ ] Say "Nexus" → "blah blah blah"
- [ ] Hear "Didn't catch that sir" or "Didn't understand that sir"
- [ ] No window opens, no loading animation

### Test 6: Close Command
- [ ] Say "Nexus" → "close Chrome"
- [ ] Chrome closes

## Architecture Mapper Tests

### Test 7: Phase 1 Architecture Map
- [ ] Architecture Mapper shows 5+ layers
- [ ] AI enrichment labels are visible
- [ ] Summary text is shown
- [ ] Graph is interactive (zoom, pan, click nodes)

### Test 8: Phase 2 Deep Scan
- [ ] Check logs for: `Phase 2: Starting deep graph scan`
- [ ] Hotspots appear on the map
- [ ] Circular dependencies are detected
- [ ] No errors in logs

### Test 9: Blast Radius
- [ ] Click a file in the architecture map
- [ ] Ask "What breaks if I change this file?"
- [ ] Blast radius highlights in red
- [ ] Dependency paths are shown

## Latency Measurements

### Test 10: First Command Latency
- [ ] Time from end of speech to "On it sir": _____ seconds (target: ≤3s)
- [ ] Time from "On it sir" to architecture window: _____ seconds (target: ≤10s)

### Test 11: Subsequent Command Latency
- [ ] Time from end of speech to "On it sir": _____ seconds (target: ≤2s)
- [ ] Should be faster than first command (models already warm)

## RAM Verification

### Test 12: Idle RAM
- [ ] Open Task Manager after 5 minutes idle
- [ ] `nexus.exe` RSS: _____ MB (target: ~350 MB)
- [ ] `python.exe` (STT) RSS: _____ MB (target: ~150 MB)
- [ ] `python.exe` (NLU) RSS: _____ MB (target: ~100 MB)
- [ ] Total: _____ MB (target: ~800 MB with all pre-warmed)

## Log Inspection

### Test 13: Log File Location
- [ ] Logs at: `%APPDATA%\com.nexus.assistant\`
- [ ] Latest log file has today's date
- [ ] No `ERROR` lines in log (warnings are OK)

### Test 14: Pre-warm Timeline
- [ ] TTS pre-warm completed in: _____ seconds
- [ ] STT pre-warm completed in: _____ seconds
- [ ] NLU pre-warm completed in: _____ seconds
- [ ] All three completed before first voice command

## Edge Cases

### Test 15: No Browser Open
- [ ] Close all browsers
- [ ] Say "Nexus" → "open architecture mapper"
- [ ] Architecture mapper should show manual URL entry (not crash)

### Test 16: No Internet
- [ ] Disconnect internet after first launch
- [ ] Say "Nexus" → "open Chrome"
- [ ] Should still work (local command, no internet needed)
- [ ] Say "Nexus" → "analyse PR 5 in owner/repo"
- [ ] Should show error (needs internet for GitHub)

### Test 17: Rapid Commands
- [ ] Say "Nexus" → "open Chrome"
- [ ] Immediately say "Nexus" → "close Chrome"
- [ ] Both commands should be handled (no race condition)

## Cleanup

- [ ] Right-click tray icon → Quit
- [ ] Verify no `nexus.exe` process in Task Manager
- [ ] Right-click tray icon → Settings → check all settings persist
- [ ] Uninstall via Add/Remove Programs — verify clean uninstall
