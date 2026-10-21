# Feature 87 — Screen Annotation & Live Architecture Drawing (Implemented 2026-10-02)

**Trigger:** *"annotate my screen"*, *"draw on the screen"*, *"add an arrow to the screen"*, *"create an architecture diagram"*
**Plan:** `docs/features/87-screen-annotation-architecture-plan.md`
**Constraint honored:** zero new windows, zero new WebView processes (stage + unified sidebar only).

## What was built

**Phase A (Rust):** `parse_screen_annotation_command` (`intent_parser.rs`, 3 anchored families + 11-phrase test + 4 misfire guards — architect gates verified safe, they require open/launch/show verbs); `run_screen_annotation` + pure `annotation_seed` (`orchestrator.rs`, normal + Ghost arms); `PENDING_ANNOTATION` + `get_pending_annotation` (`commands.rs`, `lib.rs`); `"Annotation ready, sir"` cached phrase (`tts.rs`).

**Phase B (Stage canvas):** `annotationTypes.ts` (pure reducer: add/toggle/move/setText/remove/seed/append/undo/redo/clear, 50-deep history; physical-px hitboxes with no dpr arg — regression guard for the double-dpr pin bug), `AnnotationCanvas.tsx` (capture mode = fullscreen hitbox, view mode = element bboxes only; select-drag-move, dbl-click text edit, Esc exits capture), `main.tsx` mount, `ghost.css` ink theme vars (`--ink-accent`/`--ink-primary`, transparent stage).

**Phase C (Sidebar palette):** `AnnotatePalette.tsx` (`"annotate"` view in `UnifiedSidebar.tsx`; tools/colors/undo/redo/clear/import-pins/commit/done; fresh-window `get_pending_annotation` + warm `sidebar:show_annotation` → `stage:annotation_begin`); `sidebar.css` liquid-glass rules.

**Phase D (Serializer):** `diagramSerializer.ts` (text→node, checkbox→diamond √/×, arrow→nearest-node edge, stroke→comment, sanitized labels) + 8 tests.

**86→87 bridge:** "Import pins" converts spatial cards to auto-laid-out text nodes via `stage:annotation_append` (id-reassigning, collision-free).

## Verify
- vitest **142/142** (19 new: 11 annotationTypes + 8 serializer); tsc 0.
- Rust **827/827** serial (2 new: parser phrases + seed shape); zero new warnings (34 pre-existing untouched).
- `node nexus.mjs build` → 51.2 MB binary.

## Known limits (by design)
- Primary monitor only (same as 86); session-only canvas (export is persistence); pointer events only.
- `stage_set_hitboxes` fullscreen capture while drawing = apps behind not clickable until Esc/Done (Snip-&-Sketch semantics).

## Live test script (user-run via `nexus start`)
1. *"annotate my screen"* → palette opens, stage captures.
2. Draw arrow + freehand, place text ("login button"), place checkbox, toggle it.
3. Select-drag an element; Undo/Redo; Esc → ink stays, clicks pass through; checkbox still toggles.
4. Commit → Mermaid tab shows `N1 --> N2`; Copy works; tab switch re-renders.
5. *"analyse the screen"* → spatial pins → Annotate view → Import pins → nodes appear.
