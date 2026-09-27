# Feature 34 — Architecture Mapper Layout Redesign

> **Files:** `frontend/src/architect/ArchitectApp.tsx`, `frontend/src/architect/architect.css`, `frontend/src/architect/ArchitectSidebar.tsx` (deprecated), `frontend/src/architect/architectStore.ts`
> **Added in:** 2026-09-02
> **Status:** Working, verified with 2 end-to-end tests
> **Depends on:** [16-architecture-mapper.md](16-architecture-mapper.md), [33-architecture-mapper-voice-loading-flow.md](33-architecture-mapper-voice-loading-flow.md)

---

## TL;DR

The architecture mapper window was redesigned from a 2-column bottom
section + floating sidebar overlay to a cleaner **2-row bottom section**
with **3 view tabs** in the header.

```
┌─────────────────────────────────────────────────────────────┐
│ [search bar] [Analyze]   [Files] [Hotspots (25)] [Cycles (0)] [Deep Scan] │
├─────────────────────────────────────────────────────────────┤
│                                                             │
│              Architecture Map (full width)                  │
│              (no floating sidebar overlay)                  │
│                                                             │
├─────────────────────────────────────────────────────────────┤
│ Row 1: File Inspector (110px)                               │
│  src/components/ui/button.tsx  CRITICAL                     │
│  Dependents (Imported By): 21 files                         │
│  Dependencies (Imports): 1 files                            │
│  [Run Reverse BFS Impact Analysis]                          │
│  Imported By: src/pages/Index.backup.tsx  Dashboard...      │
│  Imports: src/lib/utils.ts                                  │
├─────────────────────────────────────────────────────────────┤
│ Row 2: Analytics (changes based on selected view)           │
│  Files view → Top Coupling Hubs list                        │
│  Hotspots view → Full hotspots list                         │
│  Cycles view → Circular dependency chains                   │
└─────────────────────────────────────────────────────────────┘
```

---

## Problem

The previous layout had several issues:

1. **Floating FILE INSPECTOR overlay** — The `ArchitectSidebar`
   component floated over the architecture map as an overlay. It was
   visually cluttered and covered part of the graph.

2. **2-column bottom section** — Hotspots and Cycles were always shown
   side-by-side in two columns, regardless of which view the user wanted
   to focus on.

3. **No view switching** — There was no way to switch between Files,
   Hotspots, and Cycles views. The user wanted "3 buttons neat and
   simple" at the top.

4. **Stat pills were redundant** — The header had `Files <number>`,
   `Hotspots <number>`, `Cycles <number>` stat pills that were
   redundant with the view tabs.

5. **The inspector content was in the wrong place** — The user wanted
   the entire FILE INSPECTOR section (dependents, dependencies, imported
   by, imports, impact analysis) to be in the bottom section, not as a
   floating overlay.

---

## Implementation

### Header: 3 View Tabs

Replaced the stat pills with 3 clean view tabs:

```tsx
<div className="architect-view-tabs">
  <button
    className={`architect-view-tab ${viewMode === "layers" || viewMode === "files" ? "active" : ""}`}
    onClick={() => setViewMode("layers")}
  >
    Files
  </button>
  <button
    className={`architect-view-tab ${viewMode === "hotspots" ? "active" : ""}`}
    onClick={() => setViewMode("hotspots")}
    disabled={!phase2Data}
  >
    Hotspots {phase2Data ? `(${phase2Data.hotspots.length})` : ""}
  </button>
  <button
    className={`architect-view-tab ${viewMode === "cycles" ? "active" : ""}`}
    onClick={() => setViewMode("cycles")}
    disabled={!phase2Data}
  >
    Cycles {phase2Data ? `(${phase2Data.circular_deps.length})` : ""}
  </button>
</div>
```

- **Files** — Default view. Shows the layer/file graph + coupling hubs
  in Row 2.
- **Hotspots (N)** — Shows the hotspot list in Row 2. Disabled until
  deep scan completes.
- **Cycles (N)** — Shows cycle chains in Row 2. Disabled until deep
  scan completes.

### Main Area: Full-Width Map (No Sidebar)

Removed the `ArchitectSidebar` component from the render tree:

```tsx
// Before:
<ArchitectureMap />
<ArchitectSidebar />  // floating overlay — REMOVED

// After:
<ArchitectureMap />  // full width, no overlay
```

The `ArchitectSidebar.tsx` file still exists but is no longer imported
or rendered.

### Row 1: File Inspector (110px)

The file inspector content that was previously in the floating sidebar
is now in Row 1 of the bottom section:

```tsx
<div className="architect-inspector-row">
  {selectedFile ? (
    <div className="architect-inspector-content">
      {/* File path + risk badge */}
      <code>{selectedFile.file_path}</code>
      <span className="architect-inspector-risk">{selectedFile.risk_level.toUpperCase()}</span>

      {/* Dependents + Dependencies counts */}
      <div className="architect-inspector-stat">
        <span>Dependents (Imported By)</span>
        <span>{selectedFile.in_degree} files</span>
      </div>
      <div className="architect-inspector-stat">
        <span>Dependencies (Imports)</span>
        <span>{selectedFile.out_degree} files</span>
      </div>

      {/* Impact analysis button */}
      <button onClick={() => runImpactAnalysis(selectedFile.file_path)}>
        Run Reverse BFS Impact Analysis
      </button>

      {/* Imported By list (clickable) */}
      {selectedFile.imported_by.slice(0, 8).map(f => (
        <span onClick={() => setSelectedNodeId(f)}>{f}</span>
      ))}

      {/* Imports list (clickable) */}
      {selectedFile.imports.slice(0, 8).map(f => (
        <span onClick={() => setSelectedNodeId(f)}>{f}</span>
      ))}

      {/* Blast radius (after impact analysis) */}
      {impactResult && (
        <span>{impactResult.direct_count} direct, {impactResult.transitive_count} transitive</span>
      )}
    </div>
  ) : selectedLayer ? (
    /* Layer info: tech stack, file count, root dirs */
  ) : (
    /* Repo summary when nothing selected */
  )}
</div>
```

### Row 2: Analytics (View-Dependent)

Row 2 changes content based on the selected view tab:

```tsx
<div className="architect-analytics-row">
  {viewMode === "hotspots" && (
    /* Full hotspots list (up to 10, clickable) */
  )}
  {viewMode === "cycles" && (
    /* Circular dependency chains (clickable file nodes) */
  )}
  {(viewMode === "layers" || viewMode === "files") && (
    /* Top Coupling Hubs list (up to 8, clickable) */
  )}
</div>
```

### CSS Changes

Added new CSS classes:

- `.architect-view-tabs` — flex container for the 3 view buttons
- `.architect-view-tab` — individual view button (11px font, 5px 12px padding)
- `.architect-view-tab.active` — active state (pink accent)
- `.architect-view-tab:disabled` — disabled state (40% opacity)
- `.architect-bottom-section` — 240px tall, 2-row flex column
- `.architect-inspector-row` — 110px tall, file inspector content
- `.architect-inspector-content` — flex column with 6px gap
- `.architect-inspector-file-header` — file path + risk badge
- `.architect-inspector-stats` — dependents/dependencies counts
- `.architect-inspector-stat` — individual stat (label + value)
- `.architect-inspector-list-section` — imported by / imports lists
- `.architect-inspector-file-link` — clickable file path (purple accent)
- `.architect-inspector-impact` — blast radius display
- `.architect-inspector-empty` — repo summary when nothing selected
- `.architect-analytics-row` — fills remaining space, analytics list
- `.architect-analytics-list` — scrollable list container
- `.architect-analytics-list-title` — section title

### Store Usage

The `architectStore.ts` already had the required state:
- `selectedNodeId` — currently selected file/layer
- `viewMode` — `"layers" | "files" | "hotspots" | "cycles"`
- `impactResult` — result of reverse BFS impact analysis
- `highlightedPaths` — paths to highlight in the graph
- `setSelectedNodeId()` — select a file/layer
- `setViewMode()` — switch view
- `setImpactResult()` — store impact analysis result
- `setHighlightedPaths()` — store paths to highlight

### Impact Analysis

The `runImpactAnalysis` function calls the Rust `query_impact` command:

```typescript
const runImpactAnalysis = useCallback(async (filePath: string) => {
  try {
    const impact = await invoke<ImpactResult>("query_impact", {
      targetFile: filePath,
      maxDepth: 6,
    });
    setImpactResult(impact);
    setHighlightedPaths(impact.dependency_paths);
  } catch (err: any) {
    console.warn("Impact analysis error:", err);
  }
}, [setImpactResult, setHighlightedPaths]);
```

---

## What Was Removed

1. **`ArchitectSidebar` component** — No longer rendered. The file still
   exists but is not imported by `ArchitectApp.tsx`.
2. **Floating sidebar overlay** — The inspector is now in Row 1 of the
   bottom section, not floating over the map.
3. **Stat pills** — `Files <number>`, `Hotspots <number>`,
   `Cycles <number>` pills removed from header. Replaced by view tab
   counts.
4. **2-column bottom section** — Replaced by 2-row bottom section.
5. **Old analytics section CSS** — `.architect-analytics-section` and
   `.architect-analytics-col` replaced by `.architect-bottom-section`,
   `.architect-inspector-row`, and `.architect-analytics-row`.

---

## What Was Retained

- Full-width architecture graph (no sidebar overlay covering it)
- Clickable cycle-chain files (select graph nodes)
- Clickable hotspot items (select graph nodes)
- Clickable imported-by / imports files (select graph nodes)
- Impact analysis (reverse BFS) via Rust `query_impact` command
- Layer inspector (tech stack, file count, root directories)
- Repo summary when nothing is selected
- Deep scan button (only shown when Phase 2 data is not yet available)
- Progress bar during loading/deep scanning
- Error banner for errors

---

## Critical Build Note

**The Rust binary must be rebuilt after frontend changes.** See
[Feature 33](33-architecture-mapper-voice-loading-flow.md#critical-build-note)
for details.

---

## Test Results

Both tests confirmed the new layout loads correctly:

### Test 1 (2026-09-02 19:03)

```
[log] [TEST] Step 1: Showing loading indicator
[log] [TEST] Step 2: Loading indicator shown, calling architect
[log] === CDP attached to: NEXUS Loading ===
[log] [TEST] Step 3: Architect result: 0
[log] [TEST] Step 4: Loading indicator hidden, done
[log] === CDP attached to: NEXUS Architecture Mapper ===
[log] [architect] main.tsx loaded, mounting React app...
[log] [architect] ArchitectApp mounted, fetching pending repo...
```

### Test 2 (2026-09-02 19:04)

```
[log] [TEST] Step 1: Showing loading indicator
[log] [TEST] Step 2: Loading indicator shown, calling architect
[log] === CDP attached to: NEXUS Loading ===
[log] [TEST] Step 3: Architect result: 0
[log] [TEST] Step 4: Loading indicator hidden, done
[log] === CDP attached to: NEXUS Architecture Mapper ===
[log] [architect] main.tsx loaded, mounting React app...
[log] [architect] ArchitectApp mounted, fetching pending repo...
```

No console errors. New layout code confirmed in built JS:
- `architect-view-tab` found in built JS + CSS
- `architect-bottom-section` found in built JS
- `architect-inspector-row` found in built JS
- `architect-analytics-row` found in built JS
