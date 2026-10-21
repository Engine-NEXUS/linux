# Feature 86: Spatial Vision & Screen OCR Command Center

**Status**: SPECIFICATION & IMPLEMENTATION BLUEPRINT  
**Corpus**: `c:\PROJECTS\ULTRON`  
**Dependencies**: `src-tauri/src/vision.rs`, `src-tauri/src/orchestrator.rs`, `src-tauri/src/stage.rs`, `frontend/src/stage/main.tsx`, `frontend/src/sidebar/SidebarApp.tsx`

---

## 1. Feature Overview

Enables spatial visual decomposition of the user's active monitor when triggered by voice command (*"Analyse the screen for me"*, *"Deconstruct this image"*, *"Explain the diagram on my screen"*).

- **Transparent Stage Overlay (`stage` window)**:
  - Renders glowing liquid-glass bounding boxes over identified visual elements.
  - Draws dynamic floating pins (`[1]`, `[2]`, `[3]`) with SVG callout lines pointing directly at objects on screen.
  - 100% click-through by default; selective hit-testing allows clicking pins.
- **Docked Sidebar (`sidebar` window)**:
  - Opens full-height on the right monitor edge (`x = monitor_w - w + 12`, `y = 20`, `h = monitor_h - 40`).
  - Renders card-by-card detailed breakdowns.
  - Synchronized hover & click states between on-screen boxes and sidebar cards.
- **Zero-RAM Cloud Architecture**:
  - **Primary**: Google Gemini Flash-Lite via Cloud API (native 2D spatial coordinate recognition `[ymin, xmin, ymax, xmax]`).
  - **Fallback**: Groq LPU Vision (Llama-4-Scout) triggered automatically if Gemini hits HTTP 429 or quota limit.
  - **Local Text Aligner**: Windows WinRT OCR (`windows::Media::Ocr`) for word-level bounding rects.
  - **Local RAM footprint**: **0 MB**.

---

## 2. File Modification & Creation Inventory

| File | Type | Changes to Make |
| :--- | :--- | :--- |
| `src-tauri/src/vision.rs` | Rust (Edit) | Add `SpatialBoundingBox`, `SpatialAnalysisItem`, `SpatialAnalysisPayload`. Add `analyze_screen_spatial()`. Set Gemini Flash-Lite as primary and Groq as fallback. |
| `src-tauri/src/orchestrator.rs` | Rust (Edit) | In `run_screen_analysis()`, replace plain text VLM call with `analyze_screen_spatial()`. Emit `stage:spatial_annotations` and `sidebar:show_spatial`. |
| `frontend/src/stage/main.tsx` | React (Edit) | Add `SpatialAnnotationLayer` component listening to `stage:spatial_annotations`. Render SVG leader lines, glowing bounding boxes, and numbered pin badges. |
| `frontend/src/stage/ghost.css` | CSS (Edit) | Add styles for `.spatial-box`, `.spatial-pin`, `.spatial-leader-line`, and pulsing highlight keyframes. |
| `frontend/src/sidebar/SidebarApp.tsx` | React (Edit) | Add `SpatialDashboard` view mode when `sidebar:set_view("spatial_analysis")` or `sidebar:spatial` event arrives. |
| `frontend/src/sidebar/SpatialDashboard.tsx` | React (New) | Render detailed item cards (nutrition, specs, code, health facts). Emit `stage:highlight_item` on card hover. |
| `frontend/src/sidebar/sidebar.css` | CSS (Edit) | Add liquid-glass card styles for `.spatial-card`, `.spatial-badge`, `.spatial-detail-grid`. |

---

## 3. Data Flow & Communication Contract

### A. Stage Window Event (`stage:spatial_annotations`)
Emitted by Rust when analysis completes:
```typescript
interface SpatialPin {
  id: number;
  label: string;
  x: number;      // CSS Logical X (top-left)
  y: number;      // CSS Logical Y (top-left)
  width: number;  // CSS Logical Width
  height: number; // CSS Logical Height
  pinX: number;   // Anchor X for pointer pin
  pinY: number;   // Anchor Y for pointer pin
}

interface SpatialAnnotationPayload {
  title: string;
  pins: SpatialPin[];
}
```

### B. Sidebar Window Event (`sidebar:spatial_data`)
Emitted by Rust concurrently with the stage event:
```typescript
interface SpatialDetailRow {
  title: string;
  value: string;
}

interface SpatialItemCard {
  id: number;
  label: string;
  category: string;
  summary: string;
  details: SpatialDetailRow[];
}

interface SpatialAnalysisPayload {
  title: string;
  overview: string;
  items: SpatialItemCard[];
  providerUsed: "gemini" | "groq";
}
```

---

## 4. Implementation Steps

1. **Step 1: Rust Spatial VLM Engine (`src-tauri/src/vision.rs`)**:
   - Update `provider_order` to default to `["gemini", "groq"]`.
   - Implement `analyze_screen_spatial(app)` using Gemini Flash-Lite with structured schema:
     ```json
     {
       "title": "Screen Content Analysis",
       "overview": "Overview of visual items",
       "items": [
         {
           "id": 1,
           "label": "California Almonds",
           "category": "Nuts",
           "box_2d": [180, 240, 420, 510],
           "summary": "160 kcal, high vitamin E",
           "details": [{"title": "Protein", "value": "6g"}]
         }
       ]
     }
     ```
   - If Gemini returns HTTP 429, call `mark_exhausted(app_data_dir, "gemini")` and fall back to Groq Llama-4-Scout.
   - Denormalize coordinates using the monitor's scale factor and physical dimensions.

2. **Step 2: Orchestrator Integration (`src-tauri/src/orchestrator.rs`)**:
   - In `run_screen_analysis()`, call `analyze_screen_spatial()`.
   - Speak a high-level sentence via TTS (e.g. *"I've highlighted 3 nut varieties on your screen, sir. Detailed breakdown is in the sidebar."*).
   - Emit `stage:spatial_annotations` to the `stage` window and `sidebar:spatial_data` to the `sidebar` window.
   - Ensure both windows are visible and positioned.

3. **Step 3: Stage Window Floating Layer (`frontend/src/stage/main.tsx`)**:
   - Create `<SpatialOverlay pins={pins} activeId={activePinId} onPinClick={handlePinClick} />`.
   - Render SVG `<rect>` for glowing bounding boxes and `<line>` / `<path>` for architectural leader lines.
   - Add pointer pin badges `<div className="spatial-pin">{pin.id} {pin.label}</div>`.

4. **Step 4: Sidebar Visual Dashboard (`frontend/src/sidebar/SpatialDashboard.tsx`)**:
   - Create cards for each item with badge `[1]`, `[2]`, `[3]`.
   - Hovering a card emits `stage:highlight_item(id)` via Tauri IPC or window message.
   - The stage overlay pulses the corresponding box when hovered.

---

## 5. Verification Checklist

- [ ] Rust compiles cleanly with 0 new warnings: `cargo check`.
- [ ] Tests pass: `cargo test`.
- [ ] Frontend compiles without TypeScript errors: `npx tsc --noEmit`.
- [ ] Vitest tests pass: `npm run test`.
- [ ] Release binary builds with custom protocol: `npm run build && cargo build --release --features custom-protocol`.
- [ ] Zero local RAM overhead: Task Manager verifies 0 MB added to background RAM during vision analysis.
- [ ] Failover test: Simulating HTTP 429 on Gemini seamlessly routes to Groq within the same command without user interruption.
