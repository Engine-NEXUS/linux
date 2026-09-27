# Feature 35 — Custom-Protocol Build Requirement (Frontend → Rust Binary)

> **Files:** `src-tauri/tauri.conf.json`, `frontend/vite.config.ts`, `scripts/run.ps1`, `frontend/dist/`
> **Added in:** 2026-09-02
> **Status:** Documented — this is a build process requirement, not a feature
> **Affects:** All frontend changes

---

## TL;DR

**After any frontend change, you MUST rebuild the Rust binary.** Running
`npm run build` alone is NOT enough — the Rust binary embeds the frontend
files at compile time via Tauri's `custom-protocol` feature.

```bash
# CORRECT — both steps required:
cd frontend && npm run build
cd src-tauri && cargo build --release --features custom-protocol

# Or use the build script:
pwsh ./scripts/run.ps1 -Build
```

```bash
# WRONG — only updates dist/, Rust binary still serves old frontend:
cd frontend && npm run build
```

---

## Problem

After making frontend changes (new layout, new components, CSS changes,
etc.), the user reported:

> "this new layout is not at all being used cross check what is causing it"

Investigation showed:
- `frontend/dist/` contained the **new** built JS/CSS (confirmed via
  `Select-String` for `architect-view-tab` — found in built files)
- The running NEXUS binary was serving the **old** frontend
- The Rust binary had NOT been rebuilt after the frontend build

### Root Cause

NEXUS uses Tauri's `custom-protocol` feature for production builds:

```json
// src-tauri/tauri.conf.json
{
  "build": {
    "frontendDist": "../frontend/dist",
    "devUrl": "http://localhost:5173",
    "beforeBuildCommand": ""
  }
}
```

When built with `--features custom-protocol`:
1. Tauri reads `frontendDist` (`../frontend/dist`)
2. **Embeds all files** from `dist/` into the Rust binary at compile time
3. The binary serves these embedded files via the `tauri://localhost`
   protocol (not from disk)

When built WITHOUT `--features custom-protocol` (dev mode):
1. Tauri uses `devUrl` (`http://localhost:5173`)
2. The Vite dev server serves files live from disk
3. Hot reload works — changes appear instantly

### Why this was confusing

- `npm run build` succeeds and updates `frontend/dist/` — no errors
- The built files in `dist/` contain the new code — verified via grep
- But the Rust binary was compiled **before** the frontend build
- The binary serves the **old embedded frontend** — new code is invisible
- No error message warns about this

---

## Build Process

### Development (hot reload)

```bash
# Terminal 1: Start Vite dev server
cd frontend && npm run dev

# Terminal 2: Start Tauri dev mode (uses devUrl, no embedding)
cd src-tauri && cargo tauri dev
```

In dev mode, Tauri connects to `http://localhost:5173` (Vite dev server).
Frontend changes appear instantly via hot module replacement. No Rust
rebuild needed.

### Production (embedded)

```bash
# Step 1: Build frontend
cd frontend && npm run build
# → outputs to frontend/dist/

# Step 2: Build Rust binary with custom-protocol
cd src-tauri && cargo build --release --features custom-protocol
# → embeds frontend/dist/ into the binary
# → outputs to src-tauri/target/release/nexus.exe
```

The `custom-protocol` feature tells Tauri to embed `frontendDist` into
the binary. Without it, the binary would try to connect to the dev server
(which isn't running in production).

### Using the build script

```bash
pwsh ./scripts/run.ps1 -Build
```

This runs both steps:
1. `npm --prefix frontend run build`
2. `cargo build --release --features custom-protocol`

Then starts NEXUS with the new binary.

---

## Vite Multi-Page Build

The frontend uses Vite's multi-page build mode. Each Tauri window has a
corresponding HTML entry point:

```typescript
// frontend/vite.config.ts
build: {
  rollupOptions: {
    input: {
      main: resolve(__dirname, "index.html"),       // orb window
      setup: resolve(__dirname, "setup.html"),       // setup wizard
      settings: resolve(__dirname, "settings.html"), // settings window
      sidebar: resolve(__dirname, "sidebar.html"),   // response sidebar
      architect: resolve(__dirname, "architect.html"), // architecture mapper
      loading: resolve(__dirname, "loading.html"),   // loading indicator
    },
  },
},
```

Every window declared in `tauri.conf.json` (or created dynamically via
`dyn_windows.rs`) must have a matching rollup input here, otherwise the
HTML file is missing from `dist/` and the window shows a WebView2 error
page in production. (Dev mode hides this: the Vite server serves any HTML
file on demand.)

---

## Dynamic Windows

NEXUS uses dynamic window creation (`src-tauri/src/dyn_windows.rs`).
Only the `main` (orb) window is declared in `tauri.conf.json`. All other
windows are created on demand:

| Window Label | URL | Created When |
|---|---|---|
| `main` | `index.html` | At startup (tauri.conf.json) |
| `setup` | `setup.html` | First run / when needed |
| `settings` | `settings.html` | User opens settings |
| `sidebar` | `sidebar.html` | Worker response arrives |
| `architect-sidebar` | `architect.html` | Architecture mapper opened |
| `loading-indicator` | `loading.html` | Long-running operation starts |

All windows use `WebviewUrl::App(config.url.into())` which serves from
the embedded frontend (production) or the dev server (development).

---

## Verification

After building, verify the new frontend is embedded:

```bash
# Check the built JS contains your new code
Select-String -Path "frontend/dist/assets/architect-*.js" -Pattern "your-new-class-name"

# Check the built CSS contains your new styles
Select-String -Path "frontend/dist/assets/architect-*.css" -Pattern "your-new-class-name"
```

If found in `dist/` but not visible in the running app, the Rust binary
was not rebuilt.

---

## Common Mistakes

1. **Only running `npm run build`** — Updates `dist/` but not the binary.
2. **Only running `cargo build`** — Embeds the old `dist/` if frontend
   wasn't rebuilt first.
3. **Running `cargo build` without `--features custom-protocol`** —
   Binary tries to connect to dev server, fails in production.
4. **Forgetting to restart NEXUS** — The old binary is still running.
   Kill it first: `taskkill /F /IM nexus.exe`

---

## Checklist After Frontend Changes

- [ ] `cd frontend && npm run build` — succeeds, no errors
- [ ] `cd src-tauri && cargo build --release --features custom-protocol` — succeeds
- [ ] `taskkill /F /IM nexus.exe` — kill old binary
- [ ] Start NEXUS with new binary
- [ ] Verify new UI is visible
- [ ] Check CDP logs for console errors
