# Research: 06 — Pebble Wakeup Design & Organic Bezel Geometry

**Date**: 2026-10-09  
**Status**: Research & Mathematical Exploration  
**Focus**: Organic Pebble Curvature, Catenary Bezel Drops, Squircle Stone Physics, and Wakeup Transitions for the Voice Orb

---

## 1. Context & User Directives

The user provided an annotated screen design (`media_1791559638603_e232f274.png`) requesting an organic **"Pebble Wakeup Design"** for the Voice Orb background.

### Key User Constraints:
1. **Option, Not a Replacement**: The current "Classic Capsule" (the rectangular pitch-black dock with $10\text{px}$ rounded corners) **must not be removed**. This new design must exist as an optional choice.
2. **Pebble Aesthetics**: Transition from rigid box geometry to organic, fluid, pebble-inspired contours.
3. **Wakeup Dynamics**: The pebble must feel alive upon wakeup—emerging gracefully from the screen boundary or expanding with fluid tactile physics.
4. **Localhost Preview First**: Create an interactive preview in `localhost` so the user can inspect, test, and tune the design prior to finalizing it in the Command Hub settings.

---

## 2. Visual Deconstruction of the User Sketch

Analysis of the user's hand-drawn markings on the screen capture reveals two key zones:

```
Screen Top Edge (Bezel)
═══════════════════════════════════════════════════════════════════════════════
        \                                                 /
         \                   ╭────────╮                  /
          ╰──────╮          │   Orb  │          ╭───────╯
                 ╰──────────│ (Circle)│──────────╯
                            ╰────────╯
                     Fluid Organic Pebble Scoop
```

1. **Top Dock (The Fluid Bezel Scoop)**:
   - The user drew a continuous curve initiating flush along the top screen bezel.
   - Instead of sharp $90^\circ$ corners, the line forms a gentle inflection ($S$-curve) that smoothly dips down into a rounded organic pouch cradling the Voice Orb.
   - The curve then rises symmetrically back to tangentially meet the top bezel.
2. **Bottom Dock (The Inverted Fluid Scoop)**:
   - At the bottom taskbar / input area, a mirrored upward arc cradles the bottom orb, maintaining geometric consistency.

---

## 3. Mathematical Models of Pebble Morphologies

To provide a comprehensive set of choices for the user on localhost, we formulate four distinct pebble paradigms:

### Paradigm 1: The Continuous Bezel Catenary Scoop (Direct Sketch Translation)
A mathematically continuous $C^1$ / $G^2$ curve dropping from the screen bezel.

Let the screen edge lie at $y = 0$, the orb center at $(x_c, y_{\text{depth}})$, and scoop half-width be $W$.
The boundary is parameterized via composite cubic Bézier curves:

$$\vec{P}(t) = (1-t)^3 \vec{P}_0 + 3(1-t)^2 t \vec{P}_1 + 3(1-t) t^2 \vec{P}_2 + t^3 \vec{P}_3, \quad t \in [0, 1]$$

- **Bezel Tangent**: $P_0 = (x_c - W, 0)$ with horizontal tangent vector $(1, 0)$, ensuring zero creasing against the screen bezel ($y'(0) = 0$).
- **Inflection Point**: $P_1 = (x_c - W + k_1, 0)$ and $P_2 = (x_c - R_{\text{bowl}}, y_{\text{depth}} - k_2)$.
- **Pebble Base**: A smooth circular arc of radius $R_{\text{bowl}}$ centered under the orb.
- **Symmetric Exit**: Exits at $P_3 = (x_c + W, 0)$ with horizontal tangent $(1, 0)$.

### Paradigm 2: Polished Obsidian River Pebble (Continuous Squircle)
A standalone, free-floating river-stone morphology based on the superellipse (Lamé curve):

$$\left| \frac{x}{a} \right|^p + \left| \frac{y}{b} \right|^p = 1, \quad p \approx 3.8$$

Unlike standard CSS `border-radius` (which introduces sharp second-derivative curvature jumps at straight edges), a high-order squircle provides continuous curvature curvature ($G^2$ continuity), creating a tactile, water-polished pebble appearance.
- **Fill**: Pure solid black (`#000000 !important`).
- **Rim**: $1\text{px}$ perimeter specular highlight (`rgba(255, 255, 255, 0.16)`).
- **Shadow**: Deep ambient occlusion drop shadow ($0\ 20\text{px}\ 52\text{px}\ \text{rgba}(0,0,0,0.95)$).

### Paradigm 3: Liquid Dynamic Island Pebble
An elongated organic pebble pod with semicircular pill caps that docks flush to the bezel:
- **Geometry**: `border-radius: 9999px` with top bezel fillet radius of $16\text{px}$.
- **Wakeup Physics**: Expands fluidly from a compact $8\text{px}$ dot into the full voice interaction pod.

### Paradigm 4: Asymmetric Natural Basalt Stone
A naturally sculpted stone with subtle organic asymmetry:
$$\text{border-radius: } 46\text{px}\ 38\text{px}\ 42\text{px}\ 48\text{px}\ /\ 40\text{px}\ 48\text{px}\ 44\text{px}\ 36\text{px}$$
Evokes natural black river stones, providing an ultra-bespoke, premium feel.

---

## 4. Wakeup Dynamics & Physics

The user specifically requested a **"pebble wakeup design"**. We identify three distinct wakeup animation mechanics:

1. **Fluid Bezel Pour (Droplet Extrusion)**:
   - Initial state: A subtle $3\text{px}$ horizontal accent resting on the bezel edge.
   - Wakeup trigger: The curve fluidly distorts downward with an elastic overshoot curve:
     $$f(t) = 1 - e^{-\gamma t} \cos(\omega t), \quad \text{CSS: } \text{cubic-bezier}(0.34, 1.56, 0.64, 1)$$
   - Simultaneously, the Voice Orb particles assemble within the expanding bowl.

2. **Obsidian Stone Depth Emergence**:
   - The pebble scales up from $0.80$ to $1.0$ with a subtle $3\text{D}$ tilt and depth blur dissolution.
   - A soft rim-light shimmer sweeps around the perimeter as the microphone activates.

3. **Organic Breathing Idle**:
   - While in `listening` or `thinking` state, the pebble container undergoes a very slow, sub-perceptual dimensional pulse ($0.5\%$ scale modulation at $0.25\text{Hz}$), making the container feel alive rather than static.

---

## 5. Architectural Non-Destructive Integration

To preserve the classic capsule with zero regressions:
```
┌────────────────────────────────────────────────────────┐
│               Settings / Store Option                  │
│       capsuleStyle: "classic" | "pebble"               │
└──────────────────────────┬─────────────────────────────┘
                           │
             ┌─────────────┴─────────────┐
             ▼                           ▼
   ["classic" (Default)]         ["pebble" (New)]
   • .orb-capsule (10px box)     • .orb-pebble (Organic scoop / stone)
   • 100% existing CSS           • Flowing SVG / Squircle geometry
   • Fully preserved             • Fluid wakeup transitions
```

---

## 6. Next Steps
1. Create implementation plan (`docs/features/106-pebble-wakeup-orb-capsule-plan.md`).
2. Build interactive localhost showcase (`frontend/pebble-showcase.html`) enabling side-by-side comparison, interactive wakeups, and real-time parameter tweaking.
