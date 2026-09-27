# NEXUS Diagnostic Review — 2026-09-12

## Root Cause Analysis of "Loading Non Stop" and "False Wake Triggers"

### Issue 1: "Loading Non Stop" — NLU Server Failing to Start

**Symptom:** Every command that the deterministic parser can't handle causes a 30-second loading indicator, then falls back to the Cloudflare Worker.

**Root Cause:** The BERT-Mini retraining completed successfully (`best_model.pt` was created at 00:24), but the ONNX export step was never run. The NLU server (`nlu_server.py`) requires `nexus_nlu.onnx` to start. When it can't find the ONNX file, it exits immediately with `sys.exit(1)`.

**Secondary Cause:** The NLU server's model directory selection logic checked if the *directory* existed, not if the ONNX file existed. The training process created `server/nlu/model/` (with only `best_model.pt`), so the server picked this directory, found no ONNX, and exited.

**Timeline from logs:**
```
13:10:57  [lazy_nlu] starting NLU server
13:11:40  [lazy_nlu] NLU server did not become responsive in 30s  ← 30s wasted
13:11:41  [lazy_nlu] starting NLU server  ← immediate retry
13:12:24  [lazy_nlu] NLU server did not become responsive in 30s  ← another 30s wasted
13:12:24  orchestrator: parsed intent: Unknown  ← falls back to Worker
```

**Fix Applied:**
1. Ran `python export_onnx.py` to export the trained model to ONNX (16.8 MB)
2. Copied tokenizer to `server/nlu/model/tokenizer/`
3. Updated `nlu_server.py` model dir selection: now checks if `nexus_nlu.onnx` exists in the dir, not just if the dir exists
4. Updated `export_onnx.py` with the new 52-intent, 45-slot-type lists (was 41 intents)
5. Updated `nlu_server.py` INTENTS and SLOT_TYPES lists to include the 11 live-mode intents
6. Copied new ONNX model to `src-tauri/resources/server/nlu/model/` for production builds

**Files Changed:**
- `server/nlu/export_onnx.py` — updated INTENTS (41→52) and SLOT_TYPES (29→45)
- `server/nlu_server.py` — updated INTENTS (41→52), SLOT_TYPES (29→45), model dir selection
- `server/nlu/model/nexus_nlu.onnx` — NEW (16.8 MB, exported from best_model.pt)
- `server/nlu/model/tokenizer/` — NEW (copied from resources)
- `server/nlu/model/labels.json` — NEW
- `src-tauri/resources/server/nlu/model/nexus_nlu.onnx` — UPDATED
- `src-tauri/resources/server/nlu/model/labels.json` — UPDATED

---

### Issue 2: "Loading Non Stop" — NLU Retry Loop (30s per attempt)

**Symptom:** Even after the NLU server fails, every subsequent unparseable command triggers another 30-second wait.

**Root Cause:** `lazy_nlu.rs` had no cooldown after a failed startup. Each call to `ensure_nlu_running()` would:
1. Spawn the NLU server process (which exits immediately due to missing ONNX)
2. Wait 30 seconds (60 × 500ms polls) for it to become responsive
3. Fail, reset `NLU_RUNNING` to false
4. Next command: repeat from step 1

**Fix Applied:**
1. Reduced startup timeout from 30s to 15s (BERT-Mini loads in ~8-12s)
2. Added a 60-second cooldown after a failed startup attempt
3. On failure, kill the failed child process and record the failure time
4. During cooldown, `ensure_nlu_running()` returns immediately (no blocking)

**Files Changed:** `src-tauri/src/lazy_nlu.rs`

---

### Issue 3: False Wake Word Triggers After Stream Restarts

**Symptom:** NEXUS wakes up without the user saying "nexus" or pressing the hotkey.

**Root Cause:** The Intel SST mic driver stops delivering audio after 2-25 minutes of use. The silence-recovery thread restarts the audio stream, but the driver produces transient audio bursts 5-10 seconds after each restart. These bursts pass the silence gate (RMS > 0.002) and the wake word model classifies them as "nexus" with high probability (0.847+).

The grace period after stream restart was only 5 seconds. The false trigger in the logs happened 7 seconds after a restart — past the grace period.

**Timeline from logs:**
```
13:10:22  silence-recovery #6: audio stream restarted
13:10:22  wake: grace period reset (5s immunity)
13:10:27  audio: RMS=0.000016, silent_for~14s  ← still in grace period
13:10:29  wake: model probability=0.847  ← 7s after restart, PAST 5s grace
13:10:29  OWW wake confirmed! (raw RMS: 0.070261)
13:10:36  stt-groq: transcript 'Thanks. See you off.'  ← false wake
```

**Fix Applied:**
1. Increased grace period from 5s to 10s after stream restart
2. Updated the grace period reset log message
3. Updated the KWS engine init log to show the new grace period

**Files Changed:** `src-tauri/src/wakeword_oww.rs`

---

### Issue 4: Brain Server Blocking First Command for 12s

**Symptom:** The first command after startup takes 12+ seconds to process because the Qwen brain server is loading.

**Root Cause:** `lazy_brain.rs` `ensure_brain_running()` was synchronous — it spawned the brain server process and then blocked for up to 60 seconds waiting for it to become responsive. The Qwen 0.5B model takes ~12s to load from disk.

**Timeline from logs:**
```
13:10:37  [lazy_brain] starting brain server
13:10:37  [lazy_brain] brain server spawned (PID 13056)
13:10:37  [lazy_brain] waiting for brain server on port 39219...
13:10:49  [lazy_brain] brain server ready (12.1s)  ← 12s blocked!
```

**Fix Applied:**
1. Made `ensure_brain_running()` non-blocking: spawns the process and a background thread to wait for readiness
2. The background thread sets `BRAIN_RUNNING=true` when ready, or clears it and kills the process on failure
3. The first command falls back to NLU/deterministic while the brain loads in the background
4. Subsequent commands use the brain once it's ready

**Files Changed:** `src-tauri/src/lazy_brain.rs`

---

### Issue 5: Live-Mode Intents Not Mapped in NLU Client

**Symptom:** Even if the NLU server classifies a live-mode command correctly, the orchestrator can't route it because `nlu_client.rs` doesn't map the new intent labels.

**Root Cause:** `nlu_client.rs` `nlu_to_parsed_intent()` had no mapping for the 11 new live-mode intents (`type_text`, `press_key`, `press_hotkey`, `confirm_send`, `cancel_action`, `browser_new_tab`, `browser_navigate`, `browser_search`, `whatsapp_open`, `whatsapp_search`, `focus_app`).

**Fix Applied:**
1. Added a catch-all mapping for all 11 live-mode intents to `ParsedIntent::NluResult` (the generic wrapper used by the deterministic live-mode parser)
2. Updated `brain_server.py` `ALL_INTENTS` list to include the 11 live-mode intents

**Files Changed:**
- `src-tauri/src/nlu_client.rs`
- `server/admin/brain_server.py`

---

### Summary of All Fixes

| Issue | Root Cause | Fix | Impact |
|-------|-----------|-----|--------|
| Loading non stop | ONNX model not exported after training | Ran `export_onnx.py`, copied to resources | NLU server starts correctly |
| Loading non stop (retry) | 30s wait per failed NLU startup, no cooldown | 15s timeout + 60s cooldown | Failed NLU no longer blocks |
| False wake triggers | 5s grace period too short for Intel SST | Increased to 10s | Transients filtered |
| First command 12s delay | Brain server blocks while loading Qwen | Non-blocking background spawn | First command instant |
| Live intents not routed | nlu_client.rs missing 11 intent mappings | Added NluResult mappings | NLU live commands work |

### Test Results After Fixes

```
323 Rust unit tests passed
10 offline command tests passed
7 user command tests passed
8 integration tests (ignored — need network)
0 failures, 0 warnings
cargo check clean
```

### Remaining Intel SST Driver Issue

The silence-recovery loop (restarts every 5-60s when mic goes silent) is caused by the Intel Smart Sound Technology driver. This is a hardware driver bug, not a NEXUS bug. The silence-recovery thread mitigates it by restarting the stream, but the driver will continue to produce silent periods.

**Permanent fixes (require admin):**
1. Update the Intel SST driver from the laptop manufacturer (HP)
2. Disable "Audio Enhancements" in mic properties
3. Use a USB microphone instead of the built-in Intel SST mic
4. Restart the Windows Audio service when it gets stuck:
   ```powershell
   Restart-Service -Name "Audiosrv" -Force
   ```
