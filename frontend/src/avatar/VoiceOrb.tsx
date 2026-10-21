import React, { useEffect, useRef } from "react";
import "./voice-orb.js";
import { getEnvelopeLevel } from "../audio/captionScheduler";

export type VoiceOrbState = "idle" | "listening" | "thinking" | "speaking" | "text";

export interface VoiceOrbElement extends HTMLElement {
  state: VoiceOrbState;
  particles: number;
  setLevel: (vol: number) => void;
  play: () => void;
  pause: () => void;
  assemble: () => void;
  disperse: () => void;
  /** Particle-generated text: the SAME cloud particles converge into glyphs. */
  setText: (text: string, holdMs?: number) => boolean;
  connect: (source: MediaStream | AudioNode | HTMLMediaElement) => Promise<VoiceOrbElement>;
  disconnect: () => void;
  renderAt: (time: number, opts?: unknown) => void;
}

export interface VoiceOrbProps {
  state: VoiceOrbState;
  particles?: number;
  level?: number;
  visible?: boolean;
  color?: string;
  thinkMode?: number;
  speakMode?: number;
  listenMode?: number;
  /** Light = black ink particles on a light capsule; dark = white particles. */
  theme?: "light" | "dark";
  /** Ghost enter: particles snap scattered then assemble into the circle. */
  entered?: boolean;
  /** No-slide exit (plan §1.3): particles scatter outward instead of a CSS slide-away. */
  dispersing?: boolean;
  /** Particle-generated text (overrides the state visual until it dissolves). */
  text?: string | null;
  className?: string;
  style?: React.CSSProperties;
}

// Augment JSX IntrinsicElements for the <voice-orb> Web Component
declare global {
  namespace JSX {
    interface IntrinsicElements {
      "voice-orb": React.DetailedHTMLProps<
        React.HTMLAttributes<HTMLElement> & {
          state?: string;
          particles?: number | string;
          recording?: boolean;
          color?: string;
          theme?: string;
          "think-mode"?: number | string;
          "speak-mode"?: number | string;
          "listen-mode"?: number | string;
        },
        HTMLElement
      >;
    }
  }
}

/**
 * High-performance WebGL 3D Voice Orb Component (Ship Notes)
 *
 * ONE continuous particle simulation morphing across 5 states:
 * - 'idle': Calm, breathing grey sphere
 * - 'listening': Grainy warm amber/brown sphere, calm and voice-reactive
 * - 'thinking': 3D rotating purple beaded starburst — 64 radial rays with
 *   dense white-magenta nucleus, concentric beaded steps, dual-axis 3D
 *   tumbling rotation, and dynamic outward energy waves
 * - 'speaking': Dense irregular potato/pebble blob, molded by NEXUS's
 *   real voice-amplitude beat (sub-phase C2), with particle sparks
 * - 'text': The SAME particles detach, converge into readable glyphs,
 *   hold, then dissolve back into the previous state
 * A brief glitch/tear burst plays whenever the dominant state changes.
 */
export const VoiceOrb: React.FC<VoiceOrbProps> = ({
  state,
  particles = 5000,
  level = 0,
  visible = true,
  color,
  thinkMode = 6,
  speakMode = 2,
  listenMode = 1,
  theme = "dark",
  entered = false,
  dispersing = false,
  text: _text,
  className,
  style,
}) => {
  const orbRef = useRef<VoiceOrbElement | null>(null);

  useEffect(() => {
    if (orbRef.current) {
      orbRef.current.state = state;
    }
  }, [state]);

  useEffect(() => {
    if (orbRef.current) {
      if (visible) {
        orbRef.current.play();
      } else {
        const t = setTimeout(() => {
          orbRef.current?.pause();
        }, 500);
        return () => clearTimeout(t);
      }
    }
  }, [visible, state]);

  useEffect(() => {
    orbRef.current?.setAttribute("theme", theme);
  }, [theme]);

  useEffect(() => {
    if (orbRef.current && thinkMode) {
      orbRef.current.setAttribute("think-mode", String(thinkMode));
    }
  }, [thinkMode]);

  useEffect(() => {
    if (orbRef.current && speakMode) {
      orbRef.current.setAttribute("speak-mode", String(speakMode));
    }
  }, [speakMode]);

  useEffect(() => {
    if (orbRef.current && listenMode) {
      orbRef.current.setAttribute("listen-mode", String(listenMode));
    }
  }, [listenMode]);

  useEffect(() => {
    if (entered && orbRef.current) {
      orbRef.current.assemble();
    }
  }, [entered]);

  useEffect(() => {
    if (dispersing && orbRef.current) {
      orbRef.current.disperse();
    }
  }, [dispersing]);

  useEffect(() => {
    if (orbRef.current && typeof level === "number" && level > 0) {
      orbRef.current.setLevel(level);
    }
  }, [level]);

  // Real TTS-audio beat sync (sub-phase C2): while speaking, pump NEXUS's
  // own voice-amplitude envelope (Rust's `tts:caption.envelope`, scheduled
  // by captionScheduler.ts) into setLevel() every frame — this takes
  // priority over the `level` prop's mic-volume-proxy effect above
  // whenever a real envelope value is available for the current instant.
  // Falls back to doing nothing (letting the `level` prop keep driving)
  // the moment no envelope is available, so speaking visuals never go
  // flat just because this signal is momentarily missing.
  useEffect(() => {
    if (state !== "speaking") return;
    let raf = 0;
    const pump = () => {
      const v = getEnvelopeLevel();
      if (v !== null && orbRef.current) orbRef.current.setLevel(v);
      raf = requestAnimationFrame(pump);
    };
    raf = requestAnimationFrame(pump);
    return () => {
      if (raf) cancelAnimationFrame(raf);
    };
  }, [state]);

  return (
    <voice-orb
      ref={(el: VoiceOrbElement | null) => {
        orbRef.current = el;
        // Base sizing lives in the `.voice-orb-el` CSS class (styles.css),
        // not here, for the same reason as Avatar.tsx's wrapper: the stage
        // window's CSP makes React's `style` prop a no-op (see OrbFrame.tsx's
        // note). Any caller-supplied `style` override (no current caller
        // passes one, but it's part of this component's public props) is
        // still honored, applied imperatively instead.
        if (el && style) {
          Object.assign(el.style, style);
        }
      }}
      state={state}
      particles={particles}
      color={color}
      think-mode={thinkMode}
      speak-mode={speakMode}
      listen-mode={listenMode}
      className={`voice-orb-el${className ? ` ${className}` : ""}`}
    />
  );
};
