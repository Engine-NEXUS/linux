# NEXUS — Change 121: Pebble Wakeup Design & Elastic Thread Stretch Animations

**Date**: 2026-10-09  
**Status**: Completed & Live on Localhost  
**Focus**: Organic Fluid Bezel Scoop, Catenary Elastic Thread Wakeup Physics, Specular Rim & Wing Highlight Opacity Tuning, WebGL Orb Interior Genesis Modes, Command Hub De-Glow & Style Selector

---

## 1. Context & User Directives

Following the Pure Black Caption baseline fix, the user requested an organic **Pebble Wakeup Design** based on their hand-drawn concept sketch:
1. **Pebble Background Choice**: An organic container option for the Voice Orb, strictly preserving the existing Classic Capsule (`10px` pitch-black `#000000 !important` box).
2. **Elastic Thread Stretching Wakeup**:
   - The entrance should not be a static linear slide; it should animate neatly like a **thread stretching down from the screen border bezel**.
   - The thread forms from a taut horizontal line along the screen bezel, stretching into a catenary pocket with the circle (Voice Orb) nestled **inside** the thread.
   - The parts touching the screen border (the wings) must show configurable/animatable **rim highlight specular opacity**.
3. **Multiple Physical Animation Variations**:
   - Provide multiple distinct animations for the fluid bezel scoop on `localhost` so the user can test, compare, and choose their preferred animation before setting it as a permanent option in Command Hub.
4. **Command Hub UI Styling**:
   - Strip any custom purple glow gradients from the Orb Customization section in Command Hub, styling it cleanly with standard glass styling (`settings-btn settings-btn--glass`) matching API Keys, Memory, and Sidebar Docking.

---

## 2. Mathematical Modeling: The Catenary Thread Stretch

The Fluid Bezel Scoop is modeled as an elastic string under tension pinned at the screen bezel coordinates $(0, 0)$ and $(W, 0)$ for a top dock (or $(0, H)$ and $(W, H)$ for a bottom dock).

### 2.1 Catenary Bézier Pouch Equation
At any normalized progression $\tau \in [0, 1]$ and stretch factor $s(\tau)$:
$$\text{depth}(\tau) = H \cdot s(\tau)$$
The boundary curve is parameterized as a continuous cubic Bézier with horizontal wing contact tangents ($dy/dx = 0$):
- Pinned left wing: $(X_{\text{left}}, 0)$ with control point $(X_{\text{left}} + W_{\text{wing}}, 0)$
- Sagging central bowl: $(X_c, \text{depth}(\tau))$ with control points $(X_c \mp \frac{W_{\text{eff}}}{2} \cdot 0.52, \text{depth}(\tau))$
- Pinned right wing: $(X_{\text{right}}, 0)$ with control point $(X_{\text{right}} - W_{\text{wing}}, 0)$

### 2.2 Nested Voice Orb Kinematics
The circle (Voice Orb) is dynamically cradled inside the stretching thread pocket:
$$Y_{\text{orb}}(\tau) = Y_{\text{bezel}} + \text{depth}(\tau) \cdot 0.48 - 50\text{px}$$
$$\text{Scale}_{\text{orb}}(\tau) = \min(1.0, \max(0.1, \tau \cdot 1.3))$$
The circle is never floating disconnectedly; it is visibly birthed and nestled inside the stretching thread.

---

## 3. The 5 Wakeup Thread Animations Built

Implemented in `frontend/pebble-showcase.html`:

| Mode | Name | Mathematical Formulation | Physical Behavior |
|:---|:---|:---|:---|
| **1** | **🧵 Elastic Thread Stretch & Snap** (Primary Directive) | $s(\tau) = 1.0 - e^{-4.6 \tau k} \cos(7.2 \tau \sqrt{k}) (1 - \tau^{3})$ | Taut thread pulls down under elastic mass, overshoots to $+15\%$, rebounds once with harmonic spring damping, and settles firmly into the scoop. |
| **2** | **🏹 Bowstring Pluck & Rebound** (High Tension) | $\tau < \tau_0: (\frac{\tau}{\tau_0})^{1.7} \cdot 1.25$; $\tau \ge \tau_0: 1 + 0.25 e^{-6.8 \tau' k} \cos(15 \tau' \sqrt{k})$ | Rapid initial plunge ($160\text{ms}$), sharp mechanical snap past equilibrium, and rapid physical ring-down. |
| **3** | **💧 Liquid Meniscus Extrusion** (Surface Tension) | $s(\tau) = 1.0 - (1 + 3.6 k \tau) e^{-3.6 k \tau}$ | Fluid mercury dripping down smoothly with critical damping ($\zeta = 1.0$), zero overshoot, like molten glass forming a pouch. |
| **4** | **🌅 Horizon Split & Wing Flare** (Magnetic Unroll) | $W_{\text{eff}}(\tau) = W \min(1.0, 1.85 \tau)$, $s(\tau) = 1 - e^{-5.2 \tau k} \cos(5.8 \tau \sqrt{k})$ | Originates at center point and unrolls horizontally toward screen borders while stretching downward; wings flash with specular light on docking. |
| **5** | **〰️ Acoustic Harmonic Ripple** (Vocal Resonance) | $s(\tau) = s_{\text{base}}(\tau) + 0.08 e^{-5 \tau} \sin(26 \tau \sqrt{k})$ | High-frequency standing wave harmonics vibrate along the string in sync with acoustic wakeword speech before tension stabilizes. |

---

## 4. The 4 Orb Interior Genesis Modes Built

The WebGL vertex shader in `pebble-showcase.html` was upgraded with uniforms `u_genesis` ($0.0 \to 1.0$) and `u_genesis_mode` ($0..3$):
1. **🌸 Core Bloom (Mode 0)**: Particles expand radially outward from a central stardust seed point to fill the 100px orb as the thread stretches.
2. **⚪ Droplet Roll (Mode 1)**: Particles slide down from the screen bezel along the thread into the bottom of the scoop.
3. **💥 Spark Burst (Mode 2)**: Supernova explosion of bright particle sparks that gets pulled inward by gravitational suction into the rotating sphere.
4. **🌀 Trefoil Helix Weave (Mode 3)**: Braided helical strands spiral into form and weave into the Celtic Trefoil knot cage.

---

## 5. Screen Border Rim & Wing Highlight Opacity Controls

A dedicated SVG multi-stop gradient `<linearGradient id="rimGradient">` and dual `<path>` overlays with Gaussian blur filters `<filter id="wingGlowFilter">` provide full specular control:
- **Wing Contact Highlight Opacity**: Slider ($0\%$ to $100\%$) controlling the exact opacity of the thread where it touches the screen border bezel.
- **Perimeter Rim Base Opacity**: Slider ($0\%$ to $60\%$) controlling the subtle continuous edge highlight.
- **Wing Contact Glow Width**: Slider ($20\text{px}$ to $120\text{px}$) controlling the fillet flare width along the screen border.
- **Dynamic Wing Flash on Wakeup**: Toggle enabling an illuminated specular gleam ($+35\%$ opacity) that travels to the wings at maximum elastic stretch.

---

## 6. Interactive Scrubber & Simulation Tools

- **Timeline Scrubber**: High-resolution slider ($0 \to 1000$) allowing frame-by-frame scrubbing to freeze and inspect the thread at any fraction of its stretch.
- **Playback Speed Selector**: $1.0\times$ (Realtime), $0.5\times$ (Slow-Mo), $0.25\times$ (Ultra Slow-Mo).
- **Elastic Spring Stiffness Slider**: $0.5$ to $2.0$ spring constant modifier.
- **Live HUD**: Displays active wakeup mode, tension state (`Stretching`, `Elastic Overshoot (+15%)`, `Settled`), and wing opacity.

---

## 7. Command Hub Cleanups & Production Wiring

1. **Glow Removal**: Stripped the glowing purple gradient and glowing drop-shadow on the "✦ Open Orb Customization Studio" button in `SettingsSidebarApp.tsx`, replacing it with clean, unified `settings-btn settings-btn--glass` styling matching all other cards.
2. **Capsule Style Selector in Command Hub**: Added a clean two-card selector under `Orb Capsule & Scoop Style`:
   - `[⬝ Classic Capsule]` (10px pitch-black box)
   - `[◗ Fluid Pebble Scoop]` (Catenary bezel scoop)
3. **Dynamic Wakeup Thread & Genesis Controls**:
   - Integrated full Wakeup Thread Animation selector directly within Command Hub Settings: Mode 1 (Elastic Thread Stretch & Snap), Mode 2 (Bowstring Pluck), Mode 3 (Liquid Meniscus), Mode 4 (Horizon Split), Mode 5 (Harmonic Ripple).
   - Added Orb Interior Genesis selectors (Spark Burst, Core Bloom, Droplet Roll, Helix Weave).
   - Added "✦ Test Wakeup on Desktop" action button invoking `test_wake_orb` for real-time live previewing on the desktop stage overlay.
4. **Orb Customization Studio Integration**:
   - Added `Wakeup` tab to `orb-studio.html` alongside Thinking, Speaking, and Listening.
   - Live clicking cards in Orb Studio triggers the desktop preview and persists configuration upon saving.
5. **Stage Overlay Physics in `OrbFrame.tsx`**:
   - Implemented `computeStretch(tau, wakeupMode, k)` supporting all 5 mathematical equations dynamically.
   - Pinned Fluid Pebble Scoop to screen bezel (`W=301px`, `H=105px`, $k=1.03$) with real-time SVG catenary Bézier curve morphing.
6. **Rust IPC Commands**:
   - `set_orb_capsule_style(style, mode, genesis)` in `src-tauri/src/commands.rs`, emitting `orb:capsule_style` across windows.
   - `test_wake_orb` IPC command triggering `wake_orb_src(&app, "preview-wake")`.
   - Registered in `tauri::generate_handler![...]` in `src-tauri/src/lib.rs`.

---

## 8. Verification Results

- **Vite Dev Server**: Active and running on `http://localhost:5173/pebble-showcase.html` with real-time hot reload.
- **TypeScript**: `npx tsc --noEmit` clean (0 errors).
- **Vitest**: 224/224 tests passed across 30 test files in 3.94s.
- **Frontend Production Build**: `npm run build` compiled cleanly in 4.76s with all HTML views and assets generated.
- **Rust Compilation**: `cargo check --features custom-protocol,admin-brain` compiled with zero errors.
- **Rust Unit Tests**: `cargo test --lib -- commands::` 36/36 tests passed.
- **Laptop Configuration**: Persisted to `settings.json` with user choices (`pebble`, Mode 1 Elastic Thread Stretch $k=1.03$, Spark Burst genesis, Top screen dock).

