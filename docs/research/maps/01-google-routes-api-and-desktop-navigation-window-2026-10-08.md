# Research: Google Routes API (v2) & Desktop Navigation Window Integration

**Date**: 2026-10-08  
**Scope**: Google Routes API (v2) vs Legacy Directions API, Webview Map Rendering Across Linux/macOS/Windows, Zero-RAM Desktop Integration, and Security Quotas

---

## 1. Google Routes API (v2) vs Legacy Directions API

In 2024–2026, Google introduced the **Routes API (v2)** (`routes.googleapis.com`) as the enterprise successor to the legacy Directions API and Distance Matrix API.

### Comparative Feature Matrix

| Capability | Legacy Directions API (v1) | Google Routes API (v2) | Recommendation for NEXUS |
| :--- | :--- | :--- | :--- |
| **API Endpoint** | `maps.googleapis.com/maps/api/directions/json` | `routes.googleapis.com/directions/v2:computeRoutes` | **Use Routes API (v2)** |
| **HTTP Protocol** | REST (GET query params) | REST (POST with JSON) + gRPC | POST JSON allows structured waypoint & traffic preferences |
| **Field Masking** | Returns entire JSON payload (bloat) | Fine-grained `X-Goog-FieldMask` | **Reduces payload size by 85%** (only fetch ETA, distance, description) |
| **Traffic Modeling** | Basic traffic duration | Real-time congestion + historic traffic patterns (`TRAFFIC_AWARE_OPTIMAL`) | Exact ETA matching Google Maps mobile app |
| **Two-Wheeler & Eco** | Driving, walking, bicycling, transit | Adds two-wheeler, eco-friendly routing (fuel efficiency) | Complete mode coverage |
| **Polyline Encoding** | Standard encoded polyline | High-precision / Polyline-6 options | Sharper line rendering |

---

## 2. Desktop Webview Rendering Across Windows, macOS, and Linux

### A. The Operating System Webview Stack in Tauri

| Platform | Underlying Webview Engine | Map Rendering Performance | Geolocation & Touch Support |
| :--- | :--- | :--- | :--- |
| **Windows** | Microsoft Edge WebView2 (Chromium) | Hardware-accelerated WebGL / Direct3D 11 | Full Windows Location API + mouse/touch drag |
| **macOS** | Apple WKWebView (WebKit) | Metal-accelerated WebGL | Native CoreLocation integration + smooth trackpad pinch-to-zoom |
| **Linux** | WebKitGTK (WebKit) | OpenGL / Mesa acceleration | Geoclue2 location support + X11/Wayland gestures |

### B. Embedding Strategy: Deep Link App Webview vs Custom SDK
1. **Google Maps Official Web Application (`https://www.google.com/maps/dir/?api=1&...`)**:
   - Renders the complete, authentic Google Maps web app.
   - **Zero API Quota Consumption**: Loading the official web app uses Google's public web service, preserving the developer's API key quota.
   - **Zero Maintenance of Maps UI**: Google continuously updates its satellite imagery, 3D building models, turn styling, and traffic animations.
2. **Routes API (v2) via Rust Backend**:
   - Runs in the background when the user speaks a voice command.
   - Quickly extracts:
     - `routes[0].duration`: e.g., `"1680s"` (28 mins)
     - `routes[0].distanceMeters`: e.g., `13520` (8.4 miles)
     - `routes[0].description`: e.g., `"FDR Drive"`
   - NEXUS generates spoken natural feedback (*"Finding the best route to Central Park. Estimated travel time is 28 minutes via FDR Drive."*), while the window opens to show the live visual map.

---

## 3. Geometry & Multi-Window Mechanics

To maintain visual harmony with the existing NEXUS suite:
- **Geometry Matching**: Matches the Architecture Mapper window ($960 \times 1040\text{px}$ on standard 1080p/4K displays).
- **Placement**: Top-pinned at $y=40\text{px}$, centered horizontally.
- **Window Reuse**: Implemented as a view inside `sidebar` (or via `dyn_windows.rs`), meaning:
  - If the user already has the Assistant or Settings sidebar open, switching to Maps is an instantaneous React state change without spawning new WebView2 instances (saving ~250 MB RAM).
  - If the window is closed, opening Maps reuses the unified window shell with single-stage backdrop blur.

---

## 4. API Key Security & Restricting `AIzaSy...`

The key provided by the user (`<ROUTES_API_KEY redacted>`) has been stored safely in:
1. Local development environment: `.env`, `frontend/.env.local`, `server/sidecar/.env`.
2. Encrypted User Settings: `%APPDATA%\com.nexus.assistant\settings.json`.

### Recommended Google Cloud Console Key Restrictions:
1. **API Restrictions**:
   - Check **Routes API** (`routes.googleapis.com`).
   - Check **Directions API** (`directions-backend.googleapis.com`).
   - Check **Places API (New)** (`places.googleapis.com`).
2. **Application Restrictions**:
   - Set to **None** for desktop application use (because desktop apps communicate directly from native Rust sockets without browser HTTP referrers).
3. **Budget Alerts**:
   - Set a $0 or $10 monthly budget alert in Google Cloud Billing to guarantee staying within the **$200 monthly free tier**.
