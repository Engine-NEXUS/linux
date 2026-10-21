# Feature 84 — Liquid Glass Frosted Desktop Widgets & Backdrop Blur Engine

**Date:** 2026-10-01  
**Category:** Visual UX / Window Compositing / Liquid Glass Material  
**Status:** SPECIFIED & READY FOR REVIEW  
**Research Spec:** `docs/research/liquid-glass/01-windows-frosted-glass-widgets-and-blur-ecosystem-audit-2026-10-01.md`  

---

## 1. Feature Objective

Feature 84 brings the authentic **"Liquid Glass" / Frosted Glass** visual language (demonstrated in user reference Images 1 & 2) to NEXUS desktop windows and floating widgets:
- Soft, high-diffusion frosted blur of the underlying desktop wallpaper and apps.
- Specular gradient borders with directional light refraction (bright top-left bevel highlight, diffused bottom shadow).
- Translucent milky white/acrylic tint with luminance adaptation (auto-adjusts between light and dark wallpapers).
- Complete immunity to Windows DWM focus/activation drops, allowing floating widgets and the Command Hub to remain permanently frosted even when non-activating.

---

## 2. Anatomy of the Liquid Glass Material

Replicating the exact visual qualities of **Image 1** (Iris Flower Subnets Widget) and **Image 2** (Claude in Excel Modal):

```
┌─────────────────────────────────────────────────────────────┐
│ 1. Specular Top-Left Highlight (White 45% alpha hairline)   │
│ ┌─────────────────────────────────────────────────────────┐ │
│ │ 2. Milky Diffused Fill (rgba(255, 255, 255, 0.18))      │ │
│ │ 3. Deep Gaussian/Kawase Desktop Blur (Radius: 36px)     │ │
│ │                                                         │ │
│ │    Total Subnets                                        │ │
│ │    77                                                   │ │
│ │    +12 sn / 1m                                          │ │
│ │    [ |||||||||||||||||||||||||||| ]                     │ │
│ │    0             50             100                     │ │
│ │                                                         │ │
│ │ 4. Refractive Ambient Color Sampling                    │ │
│ └─────────────────────────────────────────────────────────┘ │
│ 5. Deep Ambient Drop Shadow (0 20px 50px rgba(0,0,0,0.20))  │
└─────────────────────────────────────────────────────────────┘
```

### Key Visual Tokens:
1. **Backdrop Blur Radius:** 36px–48px dual-pass Kawase blur.
2. **Surface Fill:** `rgba(255, 255, 255, 0.18)` on light wallpapers, `rgba(15, 23, 42, 0.40)` on dark wallpapers.
3. **Specular Border:** `linear-gradient(135deg, rgba(255, 255, 255, 0.55) 0%, rgba(255, 255, 255, 0.10) 100%)`.
4. **Inner Bevel Highlights:** `box-shadow: inset 0 1px 1px rgba(255, 255, 255, 0.65), inset 0 -1px 1px rgba(0, 0, 0, 0.08)`.
5. **Continuous Squircle Geometry:** `border-radius: 28px`.

---

## 3. Implementation Blueprint

### 3.1 Backend Engine: `src-tauri/src/liquid_glass.rs`
Evolving `sidebar_backdrop.rs` into a generalized backdrop sampler for any window rect:
- **`capture_desktop_rect(x, y, w, h)`**: Captures the exact physical monitor region via GDI `BitBlt` in <1.2ms without window capture indicators.
- **`fast_kawase_blur(bgra_data, w, h, radius, passes)`**: SIMD-accelerated downsample/upsample blur in <2.5ms.
- **`encode_to_jpeg_data_uri()`**: Emits a compact base64 JPEG URI directly to the target webview.

### 3.2 Frontend Reusable Component: `<LiquidGlassCard>`
A plug-and-play React component usable across all NEXUS overlays:

```tsx
interface LiquidGlassCardProps {
  children: React.ReactNode;
  className?: string;
  radius?: number;
  intensity?: "light" | "medium" | "deep";
}

export function LiquidGlassCard({ children, className, radius = 28, intensity = "medium" }: LiquidGlassCardProps) {
  return (
    <div 
      className={`liquid-glass-card liquid-glass-card--${intensity} ${className || ""}`}
      style={{ borderRadius: `${radius}px` }}
    >
      <div className="liquid-glass-sheen" />
      <div className="liquid-glass-content">{children}</div>
    </div>
  );
}
```

### 3.3 CSS Styling Rules: `liquid-glass.css`
```css
.liquid-glass-card {
  position: relative;
  background-image: var(--desktop-backdrop-image);
  background-size: cover;
  background-position: center;
  background-attachment: fixed;
  background-color: rgba(255, 255, 255, 0.18);
  border: 1.5px solid rgba(255, 255, 255, 0.40);
  box-shadow:
    0 24px 60px rgba(0, 0, 0, 0.18),
    inset 0 1px 1.5px rgba(255, 255, 255, 0.70),
    inset 0 -1px 1.5px rgba(0, 0, 0, 0.06);
  overflow: hidden;
  backdrop-filter: saturate(135%);
}

.liquid-glass-sheen {
  position: absolute;
  inset: 0;
  background: linear-gradient(
    135deg,
    rgba(255, 255, 255, 0.25) 0%,
    rgba(255, 255, 255, 0.00) 60%
  );
  pointer-events: none;
}
```

---

## 4. Verification & Comparison against Reference Images

1. **Iris Flower Comparison (Image 1):**
   - The blue petals beneath the card diffuse into a smooth, creamy sapphire bokeh.
   - The white text "Total Subnets 77" remains razor-sharp with 100% legibility.
2. **Claude Modal Comparison (Image 2):**
   - The landscape greens bleed naturally through the glass with subtle saturation lift.
   - Interactive buttons cast realistic internal and external shadows.
3. **Performance Gate:**
   - Total background capture + blur latency: **<4ms**.
   - Zero impact on 60+ FPS UI animations.
   - 100% operational on non-activating, borderless, floating desktop widgets without DWM deactivation glitches.
