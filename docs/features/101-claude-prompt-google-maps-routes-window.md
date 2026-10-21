# One-Time Claude Prompt: Google Maps & Routes Navigation Window Integration in NEXUS

> **Instructions for User**: Copy and paste the entire prompt block below directly into Claude. It contains complete context, architectural specifications, exact file locations, and step-by-step instructions.

```markdown
You are working on NEXUS, a high-performance cross-platform desktop AI assistant built with Tauri v2 (Rust backend) + React 18 / TypeScript + WebGL frontend.

### Context & Objective:
The user wants to integrate Google Maps & Routes Navigation into NEXUS:
1. When the user asks for directions (e.g., "Directions to Central Park", "How do I get to Heathrow Airport?", "Show route to San Francisco"):
   - NEXUS speaks a natural voice acknowledgement: "Finding the best route to {destination}, sir." (with ETA / distance if Routes API is queried).
   - Simultaneously, NEXUS opens a dedicated window matching the exact size and screen position of the Architecture Mapper window (960px wide, centered horizontally, top-pinned at y=40px).
   - Inside the window, display the authentic Google Maps application with full directions, turn-by-turn navigation list, alternative routes, and real-time colored traffic (NO custom mockups or dumbed-down maps).
2. The user has already provided their Google Routes API key (`<ROUTES_API_KEY redacted>`), which has been stored safely in `.env`, `frontend/.env.local`, `server/sidecar/.env`, and `%APPDATA%\com.nexus.assistant\settings.json`.
3. The solution must work identically on Windows (WebView2), macOS (WKWebView), and Linux (WebKitGTK).

---

### Reference Documentation & Research Files:
Review the following files in the codebase before implementing:
- `docs/features/101-google-maps-routes-window-plan.md`: Full architecture, window geometry, and implementation checklist.
- `docs/research/maps/01-google-routes-api-and-desktop-navigation-window-2026-10-08.md`: Deep research into Google Routes API v2, payload field masking, and cross-platform webview map rendering.
- `src-tauri/src/google/maps.rs`: Existing Google Maps domain service (`MapsService`), URL builders (`build_maps_nav_url`), and parser methods.
- `src-tauri/src/commands.rs`: `sidebar_geometry` function (lines 1056-1065) defining the exact 960x1040 centered geometry of the "architect" window.
- `src-tauri/src/dyn_windows.rs`: Dynamic window creation mechanics.
- `src-tauri/src/api_keys.rs`: `KEY_SERVICES` array where API keys are registered for Command Hub.
- `frontend/src/sidebar/UnifiedSidebar.tsx`: Unified dynamic sidebar shell hosting view switching (`SidebarView`).
- `frontend/src/settings-sidebar/hub/HubHome.tsx`: Command Hub home page.

---

### Implementation Tasks:

#### 1. Register API Key in Command Hub (`src-tauri/src/api_keys.rs`)
- Add `("google_maps", "Google Maps & Routes", "routesApiKey")` to `KEY_SERVICES`.
- Ensure it surfaces in Command Hub -> API Keys with masking (`••••••••BHemccU`) and save/delete capabilities wired to OS Keychain / `settings.json`.

#### 2. Window Sizing & IPC (`src-tauri/src/commands.rs`)
- In `sidebar_geometry(view, dock, monitor_w, monitor_h)`:
  - Add `"maps"` to match `"architect"`:
    ```rust
    "architect" | "maps" => {
        let h = (monitor_h - 40.0).min(1040.0).max(400.0);
        if docked {
            dock_x(960.0, h)
        } else {
            let x = ((monitor_w - 960.0) / 2.0).max(0.0);
            (x, 40.0, 960.0, h)
        }
    }
    ```
- Add a Tauri command `open_maps_window(destination: String, origin: Option<String>, mode: Option<String>)` that prepares the sidebar window with view `"maps"` and emits `maps:navigate` payload.

#### 3. Frontend Maps Component (`frontend/src/maps/GoogleMapsApp.tsx` & `maps.css`)
- Create `GoogleMapsApp`:
  - Renders a liquid glass header bar (window close button, destination title, travel mode pill).
  - Below the header, renders a full-height `<iframe>` loading the official Google Maps direction URL:
    `https://www.google.com/maps/dir/?api=1&origin=${origin}&destination=${destination}&travelmode=${mode}`
  - Listens for `maps:navigate` events to update destination and origin dynamically without reloading the entire window.
- In `frontend/src/sidebar/UnifiedSidebar.tsx`:
  - Add `"maps"` to `SidebarView` type and `VIEWS` array.
  - Mount `<GoogleMapsApp />` inside the view switch.

#### 4. Voice Intent & Orchestrator Routing
- In `src-tauri/src/intent_parser.rs` & `orchestrator.rs`:
  - When user speech resolves to `action: "directions"` or `action: "navigate"`:
    - Extract `destination` from slots/transcript.
    - Invoke `open_maps_window` with the target destination.
    - If `routesApiKey` is present, call Google Routes API v2 (`computeRoutes`) to fetch duration and road name, then synthesize speech: `"Finding route to {destination}. ETA is {minutes} minutes via {road_name}."`.
    - If offline or no key, speak: `"Finding the best route to {destination}, sir."` and open the map window.

#### 5. Verification & Testing
- Run `cargo test --lib -- commands::sidebar_geometry_tests` to ensure geometry tests pass with the new `"maps"` view.
- Run `npm test -- --run` in `frontend/` to ensure 0 test regressions.
- Run `npx tsc --noEmit` and `cargo check --features custom-protocol,admin-brain`.
```
