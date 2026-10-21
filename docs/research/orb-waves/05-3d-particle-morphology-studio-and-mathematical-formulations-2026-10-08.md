# Research: 3D Particle Morphology Studio & Mathematical Formulations

**Date**: 2026-10-08  
**Scope**: 3D Particle Dynamics, WebGL Vertex Shader Parametric Geometry, State-Driven Morphing, and Real-Time Acoustic Reactivity

---

## 1. Executive Summary & Studio Architecture

The NEXUS Voice Orb is driven by a custom WebGL particle engine running 6,240 persistent particles with zero GC allocation.
To give the user complete control over visual aesthetics without degrading 60+ FPS performance or consuming local model RAM, the system implements an isolated **Orb Customization Studio** with mathematical parametric morphologies across all assistant states.

### Studio Window Architecture
- **Window**: Frameless liquid glass window ($800 \times 980$px) created dynamically via `open_orb_studio_window`.
- **Docking Geometry**: Positioned exactly to the left of the Settings sidebar ($x = \text{monitor\_w} - 1222.0$) with a 12px margin, maintaining full visibility of Settings.
- **2-Pane Interface**:
  - Left Pane: Minimal numeric selectors for Thinking ($1..12$), Speaking ($1..6$), and Listening ($1..6$) with instant save.
  - Right Pane: Real-time WebGL preview canvas with interactive audio reactivity simulation slider.

---

## 2. Mathematical Formulations of Key Morphologies

### A. Thinking Option #6: Trefoil Celtic Helix Cage (User Choice & Production Default)
A braided 3-strand helical cage consisting of three intersecting rings rotated at $120^\circ$ offsets:
$$\begin{aligned}
\text{strand} &= \lfloor 3.0 \cdot w \rfloor \in \{0, 1, 2\} \\
\theta &= 2\pi \cdot \text{fract}(3.0 \cdot w) \\
\phi &= \text{strand} \cdot \frac{2\pi}{3} \\
R(\theta) &= 0.95 + 0.15 \cos(3\theta + 1.2 t) \\
\vec{p}_0(\theta) &= \begin{pmatrix} R(\theta)\cos(\theta + 0.5t) \\ R(\theta)\sin(\theta + 0.5t) \\ 0.35\sin(2\theta + \phi) \end{pmatrix}
\end{aligned}$$

For strands 1 and 2, coordinate rotations are applied around the coordinate axes:
$$\begin{aligned}
\text{Strand 1}: \quad & \vec{p}_1 = \mathbf{R}_X(1.05) \vec{p}_0 \\
\text{Strand 2}: \quad & \vec{p}_2 = \mathbf{R}_Y(1.05) \vec{p}_0
\end{aligned}$$

The assembly is rotated around the Y-axis by $\omega t = 0.35t$ and scaled by the entrance expansion factor $\text{thinkIn} \in [0.70, 1.0]$:
$$\vec{P}_{\text{thought}} = \mathbf{R}_Y(0.35t) \cdot (\vec{p} + 0.06 \vec{n}_{\text{seed}}) \cdot \text{mix}(0.70, 1.0, \text{thinkIn})$$

---

### B. Speaking Option #2: 64-Strand Fibonacci Blossom (Production Default)
An organic fluid droplet that blossoms into 64 petal strands modulated by acoustic sound energy ($E_{\text{sound}} = 0.75 \max(low, mid, high) + 0.85 \cdot \text{onset}$):
$$\begin{aligned}
\text{lobeNoise} &= \mathcal{N}(0.92 \vec{n} + \vec{v}_{\text{drift}} t) \\
\text{speechBulge} &= (0.18 \max(low, mid) + 0.32 \cdot \text{onset}) \cdot (1.0 - 0.75 \cdot \text{textProg}) \\
\text{dropSag} &= -0.08 \max(0, -n_y) \cdot (1.0 - 0.4 n_x^2) \\
\text{bloom} &= \sin(6.0 \|\vec{n}_{xy}\| - 3.0t) \cdot 0.22 \cdot E_{\text{sound}} \\
R_{\text{speak}} &= 1.0 + 0.12 \cdot \text{lobeNoise} + \text{speechBulge} \cdot \text{lobeNoise} + \text{dropSag} + \text{bloom} \\
\vec{P}_{\text{speak}} &= \mathbf{R}_Y(0.18t) \cdot (\vec{n} \cdot R_{\text{speak}})
\end{aligned}$$

---

### C. Listening Option #1: Acoustic Inward Suction Vortex / Chladni Cymatics
50% of particles form a rigid anchor shell ($r=1.0$), while the remaining 50% act as acoustic sand grains on a Chladni plate that scatter and bounce proportionally to voice beats:
$$\begin{aligned}
\text{isFixed} &= \left( \lfloor 6240 \cdot w + 0.5 \rfloor \pmod 2 \right) < 0.5 \\
\text{voiceLevel} &= \text{clamp}\left( \frac{E_{\text{sound}} - 0.02}{0.98}, 0, 1 \right) \\
\text{bounce} &= \sin(t(18.0 + 34.0 h_1) + 2\pi h_2) \\
\text{dynR} &= 1.0 + \text{voiceLevel} \cdot (0.35 \cdot \text{bounce} + 2(h_1 - 0.5) \cdot 0.45 \cdot \text{onset} + (h_3 - 0.5) \cdot 0.16 \cdot mid) \\
R_{\text{listen}} &= \begin{cases} 1.0 & \text{if } \text{isFixed} \\ \text{dynR} & \text{otherwise} \end{cases}
\end{aligned}$$

---

## 3. Parametric State Morphing & Lerp Dynamics

State transitions are governed by exponential weight decay:
$$\mathbf{w}_{k+1} = \mathbf{w}_k + (\mathbf{w}_{\text{target}} - \mathbf{w}_k) \cdot \left( 1 - e^{-\Delta t / 0.24} \right)$$
where $\mathbf{w} = (w_{\text{idle}}, w_{\text{listen}}, w_{\text{think}}, w_{\text{speak}})^T$ and $\sum_{i=1}^4 w_i = 1$.

The final vertex position $\vec{P}_{\text{final}}$ is the linear superposition of the active state morphologies:
$$\vec{P}_{\text{final}} = \mathbf{R}_Y(0.14t) \cdot \vec{n} \cdot R_{\text{listen}} (w_{\text{idle}} + w_{\text{listen}}) + \vec{P}_{\text{thought}} w_{\text{think}} + \vec{P}_{\text{speak}} w_{\text{speak}}$$

---

## 4. Performance & Memory Impact

| Profile Metric | Canvas 2D Fallback | WebGL Native Particle Engine | Impact |
| :--- | :--- | :--- | :--- |
| **FPS in Idle** | 25 FPS (throttled) | 25 FPS (throttled) | **-60% GPU power draw** |
| **FPS in Active States** | 60 FPS | 60–144 FPS (monitor native) | **Fluid motion** |
| **Draw Calls / Frame** | $N$ arc/fill calls | **1 `gl.drawArrays(gl.POINTS, 0, count)`** | **$O(1)$ GPU dispatch** |
| **Particle Allocation** | Array recreate per frame | Static Float32Buffer | **0 bytes/sec GC pressure** |
| **RAM Footprint** | ~175 MB baseline | ~177 MB baseline | **Zero local model RAM** |
