# Screen Tour v1 — Research, Findings and Design (2026-10-05)

Request (user, verbatim intent): say "Nexus analyse my screen / explain what I can see / what am I seeing / what is on my screen". The full-screen transparent overlay must draw **pointers**: one at the thing on screen, one to a **callout** that explains it — **while TTS speaks**. **One callout at a time, synced to the TTS**; the next pointer is drawn only after the previous explanation finishes; nothing is drawn ahead of speech. TTS is an **overview only** (even for a 1000-word analysis); main points are drawn; **detail goes to the sidebar**. State flow: STT → thinking orb while the command is validated/routed → short ack ("On it sir", "Sure sir", "I'll get back to you sir") → orb disappears while fetching → orb shown while speaking and drawing callouts → orb disappears at the end.

Method: 3 parallel read-only explorations (screen/vision pipeline; stage overlay drawing; STT→orb/state flow), one design pass, targeted web research, then 4 user decisions (below).

## 1. What already existed (verified in code)
| Piece | Location | Status |
|---|---|---|
| Screen intent parse | `intent_parser.rs` `parse_screen_analysis_command` | worked for "analyse the screen", "what's on my screen"; **missed** "what am I seeing", "explain what I can see" |
| Runner | `orchestrator.rs` `run_screen_analysis` | spoke one canned line; **bypassed** State/Ack/Loading events; never checked cancel |
| Capture | `vision::capture_gridded_jpeg_base64` → `sidebar_backdrop::capture_region_bgra_public` (GDI BitBlt+CAPTUREBLT, primary monitor) | **includes the stage window** (orb/spinner/pins) and draws a red axis grid |
| Spatial VLM | `vision::analyze_screen_spatial` (Feature 86) | Gemini-only (Groq vision decommissioned 2026-10-01); `maxOutputTokens` 2048 |
| Overlay | `stage.rs`, `frontend/src/stage/main.tsx` | fullscreen transparent click-through window; **CSP drops React `style` props** (OrbFrame.tsx documents the nonce/`unsafe-inline` interaction) so the existing `SpatialAnnotationLayer` is unreliable in release; `stage_set_hitboxes` call signature mismatch (swallowed); `ghost:point` pointer is dead code; stage created 1920×1080 logical, never fitted to the monitor |
| TTS | `tts.rs`, `ttsPlayer.ts` | `speak()` calls `stopTts()` first (no queue); `speak_text` resolves ~500 ms after playback (late "ended"); meeting mute enforced only in the frontend |
| Orb flow | `net/orchestrator.ts` | **Ack handler parks the orb in `speaking`; `hideOrbAfterSpeech` only hides on `idle`** → orb would stay up through the fetch |
| Sidebar | `commands.rs` `set_pending_spatial`, `unified_show_sidebar("spatial")` | existing Spatial view with per-item cards — reusable with zero sidebar change |

## 2. Corrections to the first exploration (found when reading the code)
Two of the four user phrases did not parse; "what is on my screen" was routed to the free OCR path (`classify_screen_query` → ReadOnly) so it never reached a VLM; capture is not in `screen.rs` (that is UI Automation); Groq is not a fallback; there *is* a TTS-end signal but it is late; barge-in is `stop_tts()` + `cancel_active()` which bumps `TTS_GENERATION`; `WDA_EXCLUDEFROMCAPTURE` is already applied to the sidebar whose backdrop capture uses the same BitBlt path (strong precedent that GDI desktop-DC capture honours it).

## 3. Web research
* Gemini returns `box_2d` as `[ymin, xmin, ymax, xmax]` normalised 0–1000 (and `[y, x]` points) — matches the existing spatial parser. Source: Gemini spatial-understanding docs/tutorials.
* `SetWindowDisplayAffinity(WDA_EXCLUDEFROMCAPTURE)` (Win10 2004+) omits a window from captures with no black box; works only with DWM composition; not DRM. The BitBlt/desktop-DC interaction is not documented → verified only by precedent, flagged for a live check.
* Annotation/explainer products (Trupeer-style) time callouts to audio — confirms the pattern "audio position drives the visual", not "visual timers drive audio".

## 4. Design options for the hard requirement (one callout at a time, never ahead of speech)
| Option | Verdict | Why |
|---|---|---|
| (1) Frontend `speak()` per item, advance on playback end | **Rejected** | each `speak()` runs `stopTts()` (emits `tts-ended`, toggles `tts_playing` false between items → wake word un-suppresses, orb flaps); advance signal arrives after the 500 ms grace + synthesis latency → callout N+1 appears ~1 s late over silence; no queue |
| (2) One concatenated utterance, callouts anchored to word indices from `tts:caption` | **Rejected as primary** | Edge word boundaries ≠ whitespace words ("$1,200", "3.5"); Kokoro offline timings are evenly-distributed estimates; one miscount puts a callout on the wrong sentence |
| (3) Rust narration player: one rodio queue, one source per line, queue position == line index | **Chosen** | the audio queue itself is the clock; `sink.len()` tells exactly which line is at the head; gaps (synthesis underrun) naturally clear the callout |

## 5. Architecture decided
Pure core `screen_tour.rs`: `validate_script`, `narration_steps` (overview → item 1..n → closer), `TourEngine` (state machine, injected clock), `wants_narrated_tour`, `callout_json`. `tts::narrate` + `NarrationTracker` (queue → Started/Ended events). Runner `orchestrator::run_screen_tour`. Frontend `TourOverlay.tsx`, `tourGeometry.ts`, `tourState.ts`, `net/screenTour.ts`.

Invariants (tested): `Show` only after `Clear`; nothing after `End`; `End` exactly once; the frontend only ever receives the current callout.

State flow: `State{Thinking}` → `Ack` → `screen:tour_phase{fetching}` + spinner → context/capture/VLM (stage excluded from capture) → wait for ack audio → `screen:tour_start` (orb returns) → `narrate` → `screen:tour_end` → sidebar (at the END) → `Done`. Any failure before narration → `screen:tour_phase{fallback}` and the legacy chain (spatial → OCR → UIA) continues unchanged.

## 6. User decisions
| Question | Decision |
|---|---|
| Orb during the fetch | hide it + show the spinner; orb returns at first narration |
| Meeting active | sidebar only: no overlay, no ack, no speech (the stage is not hidden from screen shares) |
| Sidebar timing | open at the END (it docks right, 520 px, and would cover targets); overlay clears afterwards |
| Scope | analyse/explain/describe/"what am I seeing"/"what's on my screen" → vision tour; "read/scan/copy my screen" stay on free OCR |

## 7. Deviations from the written plan
No Gemini `responseSchema` (a 400 would silently force permanent fallback; JSON mime type + tolerant validation instead). The ack-finish wait moved to just before narration (capture + VLM overlap the ack audio). Deferred: OCR/UIA snap-to-element, persistent numbered pins, `stage_fit_to_primary`, follow-ups ("tell me more about 2"), multi-monitor.
