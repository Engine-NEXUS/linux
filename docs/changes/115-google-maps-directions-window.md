# 115 - "Directions to X" opens the real Google Maps in its own window

## What it does
- Voice: "directions to Central Park", "how do I get to Heathrow Airport", "show me the route to San Francisco", "directions from London Bridge to Heathrow", "walking directions to the station", "directions to the airport by bus".
- Opens a window sized/placed like the Architecture Mapper (960 wide, centered, y = 40, height min(1040, monitor-40)) showing the authentic Google Maps app (steps, alternatives, live traffic, Street View).
- Speaks "Finding the best route to {dest}, sir." If a Routes API key is saved AND you said an origin, it first asks Routes API v2 for a traffic-aware ETA (3 s cap) and says "Finding route to {dest}. About 28 minutes via FDR Drive, sir."

## Why a separate window, not an iframe
google.com/maps sends X-Frame-Options / frame-ancestors, so it cannot render inside an iframe (and the app CSP has no frame-src). `dyn_windows::open_external_window` opens a top-level webview on an https URL instead: same on WebView2, WKWebView and WebKitGTK. The `maps` label is in no capability file, so the remote page has no Tauri IPC. Native title bar provides the close button. Repeat requests re-point the same window.

## Files
`google/maps.rs` (build_directions_url with full percent-encoding and no empty origin, routes_request_body, parse_routes_json, compute_route, RouteEta), `dyn_windows.rs` (open_external_window), `commands.rs` (sidebar_geometry "maps" == architect, open_maps, open_maps_window IPC, parse_travel_mode, `routes_api_key` setting wired like deepgram), `api_keys.rs` (Command Hub row "Google Maps & Routes"), `intent_parser.rs` (Directions + guards), `center.rs`, `orchestrator.rs` (run_directions), `frontend/src/intent/parser.ts`.

## Limits (honest)
- NEXUS has no device location and does not use IP geolocation: the spoken ETA needs a spoken origin. Without one, Google Maps in the window resolves "your location" itself.
- "navigate to X" / "take me to X" are NOT claimed (they belong to browser navigation).
- Not run live: the window on a real display, the Routes API with the real key, macOS/Linux.
- Security: key is sent only in an HTTPS header. Restrict it to the Routes API in Google Cloud. `frontend/.env.local` holds `VITE_ROUTES_API_KEY` (unused; delete it - VITE_ vars can be bundled into the frontend).

## Verify
Rust main group 1173 passing; targeted maps/geometry/parser/orchestrator tests; `cargo check --features custom-protocol,admin-brain` clean; frontend tsc clean, vitest 215.

## Addendum: Places lookup + saved home/work
- New Command Hub key row "Google Places" (`placesApiKey`, `.env` `PLACES_API_KEY`, git-ignored; never in a URL/log/frontend).
- Voice: "nearest pharmacy", "coffee shops near me", "find a bank near Central Park" (`ParsedIntent::PlacesSearch`; needs a proximity marker so plain "find ..." is untouched). Places API (New) `places:searchText` (3 s cap) -> speaks "Top result for pharmacy near <anchor>: Boots, 5 Mill Rd, open now. Also Lloyds." and opens the top result in the Maps window. Without key/anchor/network it just opens a Maps search ("near me" is resolved by Google in the window).
- Anchor = spoken "near X", else the remembered home. "Directions to/from home|work|office" resolve via remembered facts ("remember that my home is ..."); unknown -> NEXUS asks you to say it, never guesses.
- Wording is "top result", not "closest": text search ranks by relevance, not exact distance (no coordinates without device location).
- Both keys were verified with one live request each (Places "pharmacy near Times Square" -> CVS results; Routes London Bridge -> Heathrow -> 4481 s via M4). Rust 1176 passing, vitest 215.
- Not run live: the spoken flow and the window on a real display. Pitfall: `places_api_key` must stay in `NexusSettings`. Restrict each key to its own API in Google Cloud.
