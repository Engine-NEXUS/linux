# Feature 87 — Screen Annotation & Live Architecture Drawing (Implementation Plan)

**Voice trigger:** *"annotate my screen"*, *"draw on screen"*, *"mark up the screen"*, *"create architecture diagram"*
**Scope:** Interactive canvas on the existing fullscreen `stage` overlay + tool palette in the unified sidebar. **Zero new windows. Zero new WebView processes.**

---

## 1. Grounding (verified against tree)

| Existing asset | Location | Reused how |
|---|---|---|
| Stage fullscreen transparent HWND + 30ms cursor-hole loop | `src-tauri/src/stage.rs:177-211` | Ink layer rides the same loop |
| `stage_set_hitboxes` IPC (physical-px `StageRect`, `stage.rs:32-34`) | `stage.rs:137`, registered `lib.rs:960` | Checkbox/tool hitboxes register here |
| Race-free pending pattern (`PENDING_SPATIAL` Mutex + mount-fetch + event) | `commands.rs:948-959`, `lib.rs:1020`, `SpatialDashboard.tsx:25/:38` | Cloned as `PENDING_ANNOTATION` |
| Views kept mounted, switched by `sidebar:set_view` | `UnifiedSidebar.tsx:19-25,40,54` | New `"annotate"` view |
| Physical-px contract (CSS `n/dpr`) | `stage/main.tsx:88-89` | All canvas coords physical |
| Hitbox FFI from live_glass | `live_glass.rs:77` | Unchanged consumer |

---

## 2. Architecture

```
┌─ STAGE (fullscreen, existing HWND) ──────────────────────────┐
│  existing: pins / leader lines / ghost ring                  │
│  NEW: <svg id="ink-layer">  paths(t) arrows(t) boxes(t)      │
│  NEW: <g id="checkbox-layer"> per-element ☐/☑ (physical px)  │
│  NEW: <foreignObject> editable text boxes (dbl-click)        │
└──────────────────────────────────────────────────────────────┘
┌─ SIDEBAR "annotate" view (existing unified window) ──────────┐
│  tools: select│arrow│text│freehand│checkbox│arch│undo│clear  │
│  diagram panel: Mermaid/PlantUML/JSON export + copy          │
└──────────────────────────────────────────────────────────────┘
Rust: intent → orchestrator runner → stage_show + set_view("annotate")
      → canvas ops stay frontend-local (no IPC per stroke)
      → commit_diagram serializes → speak + sidebar render
```

**Theme:** stage stays fully transparent; ink uses `var(--ink-primary)` (`#FFF` dark / `#0D0D0D` light, luminance-probe driven) and `var(--ink-accent)` `#259ED6`. No new surfaces, no hardcoded grey.

**Fixed in this plan (known bugs):** pin/hitbox coords are physical px — canvas follows the same contract (no second `* dpr`, cf. the stage/main.tsx:57-63 defect).

---

## 3. Phase A — Intent + Runner (Rust)

1. `intent_parser.rs`: `parse_screen_annotation_command` (after `parse_screen_analysis_command`, before `parse_analyse_command`): families —
   `annotate|draw|mark ?up|sketch … screen|display|this`,
   `create|make|build … architecture|arch|diagram`,
   `add (an? )?(arrow|arrow pointer|text|checkbox|note)( on|to)? (the |my )?screen`.
   Emits `NluResult{intent:"screen_annotation", slots:{prompt}, confidence:1.0}` + phrase tests.
2. `orchestrator.rs`: `run_screen_annotation(app, prompt)` — speak *"Annotation ready, sir."* (`tts.rs` cached), `stage_show`, `sidebar:set_view("annotate")`, `set_pending_annotation` seed (empty canvas + tool=`select`). Ghost-session arm mirrors `:945`.
3. `commands.rs`: `PENDING_ANNOTATION` + `set_pending_annotation` / `get_pending_annotation` (clone of `:948-959`); register in `lib.rs`; `capabilities/stage-cap.json` + sidebar cap unchanged ( Tauri v2 app commands not ACL-gated — same as spatial).
4. Mis-fire guards in tests: "draw the curtains" → not annotation; "architect this repo" → `OpenArchitect`, not annotation.

## 4. Phase B — Canvas (stage frontend, `frontend/src/stage/`)

1. `AnnotationCanvas.tsx` (new): `<svg>` ink layer (paths/arrows/text-boxes) + checkbox `<g>`; pointer events only inside registered canvas bounds; coords physical px throughout.
2. `annotationStore.ts` (new, zustand): elements `[{id, kind: stroke|arrow|text|checkbox, points|box, text?, checked?, color}]`, tool state, undo/redo stacks; listeners `stage:tool_change`, `stage:annotation_clear`, `stage:commit_request` → emits `stage:annotation_commit {elements}`.
3. `main.tsx`: mount canvas under pins; register canvas + checkbox rects via `stage_set_hitboxes` (clone of `:57-63` pattern **minus** the double-dpr bug); cleanup on unmount (`:75` pattern); `Escape` → `stage:annotation_done` (also fixes the "no dismissal" gap for this view).
4. Text editing: dbl-click canvas → `<input>` in `<foreignObject>`; Enter commits; Esc cancels.

## 5. Phase C — Sidebar palette (`frontend/src/sidebar/`)

1. `AnnotatePalette.tsx` (new view): tool buttons (select/arrow/text/freehand/checkbox/architecture/undo/clear), element count, stroke color swatches (ink-primary/accent/red/green), diagram export tabs (Mermaid/PlantUML/JSON + Copy).
2. `UnifiedSidebar.tsx`: add `"annotate"` to view union + mount list.
3. `sidebar.css`: `.annotate-*` styles, liquid-glass buttons (clone `.spatial-*` patterns `:279-443`).
4. Commit flow: palette "Commit" → `stage:commit_request` → canvas serializes → `stage:annotation_commit` → palette renders diagram tabs (`diagramSerializer.ts`, new) → speak summary via existing TTS.

## 6. Phase D — Diagram serialization (pure, test-first)

`diagramSerializer.ts` (new): elements → Mermaid flowchart (`checkbox`→decision diamonds, `arrow`→edges, `text`→nodes), PlantUML, JSON. Schema:

```ts
type El = { id:string; kind:'stroke'|'arrow'|'text'|'checkbox';
            points?:[x,y][]; box?:[x,y,w,h]; text?:string; checked?:boolean; color:string }
```

Pinned spatial items (Feature 86) importable as `text` nodes via `sidebar:focus_spatial_card`-adjacent action ("Pin to canvas").

## 7. Testing matrix

| Layer | Cases | Harness |
|---|---|---|
| Intent | ~15 phrases + 5 misfire guards | `intent_parser` tests (mirror `:7275`) |
| Runner | seed payload shape, ghost arm, P0/P1 regression pins | `orchestrator` tests |
| Serializer | strokes→Mermaid, checkbox→diamond, arrow→edge, empty→empty-diagram, round-trip JSON | new `diagramSerializer.test.ts` |
| Canvas store | add/move/toggle/undo/redo/clear, commit payload shape | new `annotationStore.test.ts` |
| Hitboxes | checkbox rect physical-px at dpr 100/150/200 (regression for the double-dpr bug) | new `annotationHitbox.test.ts` |
| E2E (manual) | "annotate my screen" → palette opens; draw arrow+text+checkbox; Esc closes; commit → Mermaid renders | user live run |
| Gates | `tsc` 0, `vitest` full, `cargo check` 0-new, `node nexus.mjs build` | existing |

## 8. File touch list (exact)

**Rust:** `intent_parser.rs` (parser + tests), `orchestrator.rs` (runner + tests), `commands.rs` (`PENDING_ANNOTATION` + get/set), `lib.rs` (register 1 command), `tts.rs` (1 cached phrase).
**Frontend:** stage: `AnnotationCanvas.tsx`, `annotationStore.ts`, `annotationHitbox.test.ts`, `main.tsx` (mount + hitbox + Esc). sidebar: `AnnotatePalette.tsx`, `diagramSerializer.ts` + tests, `UnifiedSidebar.tsx`, `sidebar.css`.
**Docs:** this file; AGENTS.md entry on completion; features README index row 87.

## 9. Out of scope

Multi-monitor canvas (primary only, same as 86 — flag, don't fix), voice-driven element placement ("put arrow on the login button" needs UIA grounding — future), persistence across reboots (session-only canvas; export is the persistence), touch/pen pressure (pointer events only).
