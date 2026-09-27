# Feature 33 — Architecture Mapper Voice → Loading Indicator Flow

> **Files:** `frontend/src/audio/recorder.ts`, `frontend/src/loading/loadingController.ts`, `src-tauri/src/architect.rs`, `src-tauri/src/commands.rs`
> **Added in:** 2026-09-02
> **Status:** Working, verified with 2 end-to-end tests
> **Depends on:** [27-loading-indicator-overlay.md](27-loading-indicator-overlay.md), [16-architecture-mapper.md](16-architecture-mapper.md)

---

## TL;DR

When the user says **"open architecture mapper"**, NEXUS now:

1. **Immediately** says "On it sir" (fire-and-forget TTS)
2. **Immediately** hides the orb (wakeup.json) and shows the `loading.json`
   animation at the **top-right corner** of the screen
3. Runs repository detection + Phase 1 analysis + AI enrichment in the
   **background** (3–8 seconds) while the loading animation is visible
4. Opens the architecture mapper window **only after** all analysis and AI
   enrichment is complete
5. Hides the loading indicator and says "Here is the architecture, sir."

This matches the existing sidebar/worker long-running query pattern exactly.

---

## Problem

The architecture voice command (`open_architect` intent) did not follow the
same loading-indicator pattern as the general sidebar/worker flow.

### Old (broken) behavior

```
voice transcript recognized
→ orb enters "thinking" state (loading circles loop on the orb)
→ TTS says "On it sir"
→ orb STAYS VISIBLE the entire time (3–8 seconds)
→ architect window opens
→ TTS says "Here is the architecture"
→ orb hides
```

**Issues:**
- The orb didn't disappear after "On it sir" — it stayed visible showing
  loading circles for the entire duration.
- The `loading.json` animation at the top-right corner was **never
  triggered**.
- It didn't match the sidebar/worker pattern that the user had configured.

### Root cause

The architecture branch in `recorder.ts`:
1. Set state to `thinking`
2. Called `speak("On it sir")`
3. **Waited** for `waitForTtsIdle()` before doing anything else
4. Called `invoke("open_architect_with_auto_detect")`
5. Only hid the orb **after** the entire operation and final TTS finished
6. **Never** called `showLoadingIndicator()`

### First fix attempt (wrong)

The first fix used `speak("On it sir").then(async () => { ... })` —
showing the loading indicator **after** TTS finished. This was still wrong
because:

- If TTS was slow (Kokoro loading ~1.7s on first use), the loading
  indicator was delayed.
- If the state check `if (curState === "speaking" || curState === "thinking")`
  failed (e.g. state was reset by a race condition), the loading indicator
  never showed at all.
- The user reported: *"i still dont see the corner animation that runs
  when the worker is running in the background"*

### Final fix (correct)

Show the loading indicator **immediately** — don't wait for TTS at all.
This matches the sidebar/worker pattern exactly:

```typescript
// 1. Speak "On it sir" — fire and forget, don't block on TTS
void speak("On it sir");

// 2. IMMEDIATELY hide orb + show loading indicator
useAssistant.getState().setVisible(false);
setTimeout(() => useAssistant.getState().reset(), 550);
void showLoadingIndicator();

// 3. Start architect invoke in background (runs while TTS plays)
(async () => {
  const result = await invoke<number>("open_architect_with_auto_detect");
  void hideLoadingIndicator();
  if (result === 1) void speak("Here is the architecture, sir.");
})();
```

---

## Implementation Details

### Files Changed

#### `frontend/src/audio/recorder.ts`

Both architect blocks (there are two — one for each capture path) were
updated to use the same pattern:

```typescript
if (intent.action === "open_architect") {
  // 1. Speak "On it sir" — fire and forget
  useAssistant.getState().setState("speaking");
  useAssistant.getState().addAssistantMessage("On it sir, mapping the architecture...");
  void speak("On it sir");

  // 2. IMMEDIATELY hide orb + show loading indicator
  useAssistant.getState().setVisible(false);
  setTimeout(() => useAssistant.getState().reset(), 550);
  void showLoadingIndicator();

  // 3. Start architect invoke in background
  (async () => {
    try {
      const { invoke } = await import("@tauri-apps/api/core");
      const result = await invoke<number>("open_architect_with_auto_detect");
      void hideLoadingIndicator();

      if (result === 1) {
        void speak("Here is the architecture, sir.");
      } else if (result === 2) {
        void speak("Found the repository, but analysis failed, sir. Opening the mapper anyway.");
      } else {
        void speak("Couldn't detect a repository, sir. Opening the mapper anyway.");
      }
    } catch (err) {
      console.error("[NEXUS] failed to open architect window:", err);
      void hideLoadingIndicator();
      void speak("Couldn't open the architecture mapper, sir.");
    }
  })();

  // Release captureInProgress after the flow completes (30s timeout)
  const flowDeadline = Date.now() + 30000;
  const checkInterval = setInterval(() => {
    if (Date.now() > flowDeadline) {
      clearInterval(checkInterval);
      captureInProgress = false;
      return;
    }
    const st = useAssistant.getState().state;
    if (st === "idle") {
      clearInterval(checkInterval);
      captureInProgress = false;
    }
  }, 500);
  return;
}
```

**Import added:**
```typescript
import { showLoadingIndicator, hideLoadingIndicator } from "../loading/loadingController";
```

#### `frontend/src/loading/loadingController.ts`

No changes needed — already had the correct idempotent functions:
- `showLoadingIndicator()` — creates/shows the `loading-indicator` Tauri window
- `hideLoadingIndicator()` — destroys the window (frees ~250 MB WebView2 RAM)

#### `src-tauri/src/architect.rs`

No changes needed — already had the correct sequential AI enrichment:
- `open_architect_with_auto_detect()` runs Phase 1, waits for Worker AI
  enrichment, merges enriched labels, then opens the window.
- `enrich_phase1_inline()` completes before the window opens.

#### `src-tauri/src/commands.rs`

No changes needed — `show_loading_indicator` and `hide_loading_indicator`
Tauri commands already existed and worked correctly.

### What was NOT changed

- The Rust-side sequential AI enrichment in `architect.rs` was already
  correct (added in a prior session).
- The loading indicator window creation/positioning in `dyn_windows.rs`
  was already correct.
- The `loadingController.ts` was already correct.

---

## Desired Sequence

```
voice transcript recognized
→ IMMEDIATELY speak "On it sir" (fire-and-forget)
→ IMMEDIATELY hide orb (setVisible(false) + reset())
→ IMMEDIATELY show loading.json at top-right corner
→ invoke open_architect_with_auto_detect (background)
  → Rust detects repository from active browser window
  → Rust runs Phase 1 (heuristic clustering, ~0.5–1s)
  → Rust waits for Worker AI enrichment (~2–8s)
  → Rust merges enriched summary + layer labels
  → Rust opens architecture window
→ hide loading indicator
→ speak "Here is the architecture, sir."
→ clean assistant state
```

The architecture operation does NOT block the immediate acknowledgement
or prevent the loading animation from appearing.

---

## Critical Build Note

**The Rust binary must be rebuilt after frontend changes.**

NEXUS uses Tauri's `custom-protocol` feature, which **embeds the frontend
files into the Rust binary at compile time**. Running `npm run build` only
updates `frontend/dist/` — the Rust binary still serves the **old embedded
frontend**.

After any frontend change:
```bash
cd frontend && npm run build          # updates frontend/dist/
cd src-tauri && cargo build --release --features custom-protocol  # embeds into binary
```

Or use `scripts/run.ps1 -Build` which does both.

**This was the root cause of the user reporting "the new layout is not at
all being used"** — the frontend was rebuilt but the Rust binary was not,
so NEXUS was serving the old embedded frontend.

---

## Test Results

### Test 1 (2026-09-02 19:03)

| Step | Timestamp | Event |
|------|-----------|-------|
| 1 | 19:03:15.742 | loading-indicator window created |
| 2 | 19:03:15.827 | loading-indicator positioned at (1833, 9) |
| 3 | 19:03:15.827 | loading-indicator window shown |
| 4 | 19:03:16.125 | architect-sidebar: auto-detect open (code=0) |
| 5 | 19:03:16.129 | loading-indicator window destroyed |

**Result:** Loading indicator showed immediately, then hidden after
architect window opened. No repo detected (Brave not focused).

### Test 2 (2026-09-02 19:04)

| Step | Timestamp | Event |
|------|-----------|-------|
| 1 | 19:04:36.136 | loading-indicator window created |
| 2 | 19:04:36.248 | loading-indicator positioned at (1833, 9) |
| 3 | 19:04:36.248 | loading-indicator window shown |
| 4 | 19:04:36.266 | window title: ULTRON - Devin |
| 5 | 19:04:36.556 | architect-sidebar: auto-detect open (code=0) |
| 6 | 19:04:36.560 | loading-indicator window destroyed |

**Result:** Loading indicator showed immediately, then hidden after
architect window opened. No repo detected (Brave not focused).

### Earlier tests (with repo detected)

From the prior session with `zync-meet/Zync`:

| Step | Test A | Test B |
|------|--------|--------|
| Loading indicator shown | 18:53:29.818 | 18:54:38.758 |
| Repo detected | 18:53:31.096 | 18:54:40.045 |
| Phase 1 complete | 18:53:33.194 | 18:54:40.572 |
| AI enrichment done | 18:53:35.856 | 18:54:43.085 |
| Architect window opens | 18:53:36.109 | 18:54:43.322 |
| Loading indicator hidden | 18:53:36.114 | 18:54:43.327 |
| **Total time** | **6.3s** | **4.6s** |
| **Loading visible for** | **6.3s** | **4.6s** |

---

## Edge Cases Handled

1. **No repo detected** (result=0): Loading indicator hidden, speaks
   "Couldn't detect a repository, sir. Opening the mapper anyway."
2. **Repo found but analysis failed** (result=2): Loading indicator
   hidden, speaks "Found the repository, but analysis failed, sir."
3. **Invoke error** (catch): Loading indicator hidden, speaks
   "Couldn't open the architecture mapper, sir."
4. **TTS slow/fails**: Loading indicator still shows immediately because
   TTS is fire-and-forget.
5. **30s timeout**: `captureInProgress` is released after 30 seconds
   even if the flow doesn't complete, preventing a stuck state.

---

## Voice Command Recognition

`intent_parser.rs` recognizes these variants (including STT mishearings):

- "open architecture mapper"
- "open the architecture mapper"
- "show architecture"
- "show me the architecture"
- "launch architecture mapper"
- "open architect"
- "open codebase mapper"
- "open dependency mapper"
- "open architecture diagram"
- "open octach at mapper" (STT mishearing)
- "open arcade mapper" (STT mishearing)
- "launch arch at mapper" (STT mishearing)
- "open up and remember" (STT mishearing)
- "open up and member" (STT mishearing)
- "open are cat map" (STT mishearing)
