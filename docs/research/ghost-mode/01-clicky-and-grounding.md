# Ghost Research 01 — Clicky Teardown: Pointing Theater vs Real Control

Sources: `farzaa/clicky` (open), Isaac Flath's teardown, `clicky_windows`
(.NET port), `clickyX` (Rust/Tauri), clicky.foo docs. Researched 2026-09-25.

## 1. What Clicky actually is

macOS menu-bar tutor: hold a key, speak, a blue triangle flies across
the screen, points at the UI element, explains out loud. **It never
moves the real cursor and clicks nothing.** The triangle is drawn in a
fullscreen transparent overlay (one per monitor, above everything);
the user's cursor works normally underneath. Quoted from the teardown:
*"You don't have to figure out how to move a cursor in another app."*

## 2. How it works (verified pipeline)

1. Push-to-talk → AssemblyAI streaming STT.
2. On release: screenshot every monitor (own windows filtered, ≤1280px,
   80% JPEG) + transcript + last 10 turns → Claude via SSE.
3. Claude answers conversationally + appends `[POINT:x,y:label:screenN]`
   (prompt-engineered contract, regex-parsed; dimensions labeled per
   screenshot so coordinates have a space).
4. Windows port re-captures and calls **Claude Computer Use API** for
   pixel precision; converts screenshot space → physical px → WPF DIPs
   (DPI math is the main custom work: Y-flip, per-monitor offsets,
   AppKit-vs-SwiftUI points).
5. Triangle flies a bezier arc (rotates to travel direction, scales
   mid-flight), highlight ring, ElevenLabs TTS. Springs back to
   cursor-trailing.
6. All API keys live in a **Cloudflare Worker proxy** — our Worker shape.

The "no delay" is stagecraft: spinner runs through the full HTTP fetch;
state goes Speaking only when audio starts; flight + streaming speech
mask seconds of latency.

## 3. Why fake-vs-real is a product decision, not a tech gap

| Fake triangle (Clicky) | Real cursor (Ghost) |
|---|---|
| Never fights the user; user keeps working | Steals the mouse; hands-off protocol required |
| No OS trust issues (draws pixels only) | UIPI blocks admin windows; needs announce/abort/restore |
| Can't hover/drag real UI | Full hover/drag/scroll; strictly more powerful |
| Stealth-compatible | Detectable by design (desired here) |

clickyX proves the bridge: same overlay pattern + `enigo` computer-use
engine (click/scroll/type/key, incl. no-cursor-warp background mode).

## 4. Takeaway for Ghost Mode

Take Clicky's overlay + Worker-proxy + flight animation vocabulary;
replace the triangle with a ring around the REAL cursor; replace
point-only with enigo acts wrapped in the takeover leash. Theater where
it informs (ring), control where it acts (enigo), never confused.
