# Computer Use: Vision-to-Action Loop (Astra-style)

*Date: 2026-09-04*
*Status: research doc. Not wired into NEXUS. NEXUS stays text-only HTTP Worker today.*

How "call Astra" demos work under the hood: model looks at screenshots,
emits one OS-level action per turn, harness executes it, new screenshot
returns. Loop until goal done. No private app APIs. Same buttons a human
presses.

---

## 1. Big picture

```
goal -> screenshot -> model picks ONE action -> harness executes ->
new screenshot -> model verifies -> repeat -> done / fail
```

Roles:

- Model = brain. Reads pixels, decides next action.
- Harness = hands. Injects input, captures screen, enforces gates.
- Tools = narrow surface: `screenshot`, `computer_action`, `done`.

Each cycle = one model call. Long tasks = hundreds of cycles.

---

## 2. Observation packet (model input each turn)

| Field | Content | Notes |
|---|---|---|
| `screenshot` | PNG, downscaled (~1024px wide) | Primary input. Full-res wastes tokens. |
| `display` | Width x height, scale factor | Needed to map normalized coords to pixels. |
| `os` | `windows` / `macos` / `linux` / `browser` | Changes key names, window controls. |
| `goal` | Original user request | Never drops from context. |
| `history` | Prior actions + results | Short summaries, not full frames. |
| `a11y` (optional) | Button labels + bounding boxes | Helper only. Pixels stay source of truth. |

Coordinates normalized 0-1000, resolution-independent:

```
pixel_x = x / 1000 * display_width
pixel_y = y / 1000 * display_height
```

---

## 3. Action space (model output, one per turn)

| Action | Params | Use |
|---|---|---|
| `click` | `x, y` | Buttons, fields, links. |
| `double_click` | `x, y` | Select word, open item. |
| `right_click` | `x, y` | Context menus. |
| `drag` | `x1, y1, x2, y2` | Sliders, selections, window move. |
| `scroll` | `x, y, dx, dy` | Lists, pages. |
| `type` | `text` | Field input. Assumes field focused. |
| `keypress` | `key`, optional modifiers | Enter, Tab, Escape, Ctrl+C. |
| `wait` | `ms` | Loading spinners, transitions. |
| `screenshot` | — | Request fresh frame. |
| `done` / `fail` | summary | Terminal states. |

Example turn:

```json
{ "action": "click", "x": 512, "y": 348 }
```

Rules: one action per turn. `type` only after focus verified. No multi-step
scripts from model — harness sequences them.

---

## 4. Harness (dumb hands, strict order)

Pseudocode:

```
frame = capture_screenshot()
loop:
  action = model(goal, frame, history)
  if action == done/fail: break
  execute(action)            // OS input injection, see below
  sleep(settle_ms)           // 200-500ms, let UI redraw
  frame = capture_screenshot()
  history.append(action, frame_diff_summary)
```

OS input injection:

| Platform | Mechanism |
|---|---|
| Windows | `SendInput` |
| macOS | `CGEvent` |
| Linux X11 | `xdotool` / `wtype` |
| Browser | Playwright / CDP `mouse.click`, `keyboard.type` |

Harness owns: coordinate scaling, focus checks, settle waits, screenshot
capture, confirmation gates (section 8), action log. Model never touches OS
directly.

---

## 5. Worked trace ("fill name + date, submit")

```
screenshot              -> click(name field @ 512,348)
screenshot (caret in field) -> type("John")
screenshot ("John" shown)   -> keypress(Tab)
screenshot (date focused)   -> type("1990-01-01")
screenshot (date shown)     -> click(Submit @ 600,720)
screenshot (confirmation)   -> done("form submitted")
```

Failure branch: confirmation missing, red error visible -> model reads error
from pixels, corrects field, retries. No OS success signal exists; the next
screenshot IS the result.

---

## 6. Grounding (why Astra fast: first click lands)

Grounding = text ("Submit button") -> correct `x,y`.

- Benchmark for this: ScreenSpot-Pro. Vendor-reported: Astra 92.7% vs
  Sol 76.9%.
- Misses compound: wrong click -> wrong screen -> correction chain -> many
  extra model calls. High grounding accuracy shortens whole tasks.
- Training inputs: screenshots labeled with bounding boxes across many apps,
  plus reinforcement on task success with fewer steps rewarded (which also
  pushes fewer output tokens per task).

NEXUS takeaway: cache `label -> x,y + layout_hash`. Reuse until hash shifts.
Re-ground only on layout change.

---

## 7. Verification (no success API)

OS gives no "form submitted OK" callback. Model verifies visually:

1. Predict expected next screen ("confirmation page").
2. Compare against actual screenshot.
3. Match -> continue. Mismatch -> read visible error, backtrack one step.

Keep history as short summaries plus last 1-2 frames. Full frame history
explodes token cost and rarely helps.

---

## 8. Speed math (three levers, not one)

Per-task time ~ `steps x (tokens_per_step / decode_speed + harness_overhead)`.

1. Fewer tokens per step. Vendor-reported: ~65% fewer output tokens than
   Opus 5 at top Agents Last Exam scores. Less decode per cycle.
2. Fewer steps per task. Better grounding = fewer misses/corrections.
   Vendor-reported OSWorld 2.0: 72.6% in ~40 min vs Sol 65.7% in ~75 min.
3. Faster harness. Codex harness update alone: 1.9x on Mind2Web vs Sol.
   Plus persistent notes across context windows (no re-explore), async
   clarification (work continues on independent sub-steps).

Paid lever: Fast mode = up to 2x speed at 2x Standard price. Standard
vendor list: $10 per million input tokens, $50 per million output tokens.
(All figures vendor-reported; verify before quoting externally.)

---

## 9. Safety gates (required before any real deployment)

Computer-use agents act on real UI: delete, send, pay, authenticate.

1. Confirm before writes. Sends, deletes, submits, auth flows need explicit
   user approval. Reads and navigation do not.
2. Reviewer watches stream. Second model (Auto-Review) flags off-goal or
   destructive actions. Flagged task pauses, asks user.
3. Log every action with its screenshot. Reproducible audit trail.

These gates sometimes pause legitimate work. That is intended behavior, not
a bug. Never expose unrestricted computer actions through NEXUS without
confirmation policy + logging.

---

## 10. NEXUS mapping (current vs gap)

Today (text-only):

- Wake: OWW `nexus.onnx` via `tract-onnx` (`src-tauri/src/wakeword_oww.rs`).
- Trigger: `CommandOrControl+Space` (`src-tauri/src/hotkey.rs`).
- STT: lazy faster-whisper sidecar, port 39217 (`src-tauri/src/lazy_stt.rs`).
- Intent: deterministic parser first, ONNX NLU fallback.
- Backend: HTTP POST transcript -> Cloudflare Worker (`src-tauri/src/network.rs`).
- Output: sidebar render + Kokoro TTS (`src-tauri/src/tts.rs`).

Gap for computer-use mode (additive, existing path untouched):

| Need | Fits in |
|---|---|
| `screenshot` tool | New Rust command next to `src-tauri/src/command_executor.rs` |
| `computer_action` tool | Same area; OS injection per section 4 |
| Grounding cache | Small map: label -> x, y, layout_hash |
| Confirm-before-write gate | Wrap `execute_command` path + destructive UI actions |
| Session notes | Append local file; search prior turns before re-scan |
| Render loop state | Existing sidebar window (`src-tauri/src/dyn_windows.rs`) |

Minimal slice: screenshot tool only, used when text intent fails. Full loop
only for explicit "take over screen" mode.

---

## 11. Copy checklist (ordered, cheapest first)

1. Diff frames. Send changed regions only.
2. Ground once. Cache coords by layout hash.
3. Batch independent actions per turn. One roundtrip.
4. Persist notes. Search prior turns. Skip re-scan.
5. Gate writes. Confirm send/delete/auth. Log screenshot per action.

---

## 12. Failure modes

| Symptom | Cause | Mitigation |
|---|---|---|
| Click misses | Stale frame, scaling bug | Fresh screenshot, verify scale factor |
| Loop on same screen | Action had no effect | Detect no-diff, try focus-then-act |
| Popup steals focus | OS dialog appeared | Screenshot, dismiss or ask user |
| Auth wall | Login required | Stop, ask user, never stuff credentials silently |
| Slow drift | Full frames every turn | Diff-only updates, shorter history |

---

## 13. Glossary

- Grounding: phrase -> screen coordinates.
- Settle wait: pause after action so UI redraws before next screenshot.
- Layout hash: fingerprint of window/element geometry; cache key.
- Barge-in: user interrupts TTS; loop must stop speaking, resume listening.
- Compaction loss: re-summarizing history until details vanish; fixed by
  persistent notes + searchable prior windows.

---

## 14. Deliberate simplifications

- Single-display assumed. Multi-monitor needs per-display coords.
- No video/camera stream covered here. Same loop extends with frame sampling.
- Token math uses vendor figures as given; re-measure on NEXUS harness.
- ponytail: ceiling = single-machine desktop control. Upgrade path = remote
  session driver (RDP/CDP grid) when cross-machine needed.
