# Stage preview harness (`stage.html?eyeball`)

A dev-only page that renders the REAL stage — the WebGL orb, the aura, the pointer marker — in a normal browser with fixture states, so the look can be checked without a voice turn or the desktop app.

## Use it

1. Start the dev server: `npm run dev` in `frontend/` (or `tauri dev`, which starts it).
2. Open `http://localhost:5173/stage.html?eyeball`.
3. The control bar (bottom left) switches state (idle / listening / thinking / speaking), orb style (aura / classic / pebble), top/bottom, the pointer marker, and the level. The URL follows the controls, so a view can be shared.

URL parameters: `state`, `style`, `level` (0..1), `pos` (`top`|`bottom`), `pointer` (`1`|`0`), `panel` (top inset in px standing in for a desktop panel, default 32). Unknown values keep the default.

Listening and speaking add a speech-like wobble around the level, through the same store fields the orb and aura read (`micLevel` while listening, the playback proxy while speaking).

## Screenshots without a window

```bash
google-chrome --headless=new --user-data-dir=/tmp/eyeball-profile \
  --use-angle=swiftshader --enable-unsafe-swiftshader --hide-scrollbars \
  --window-size=1280,560 --virtual-time-budget=5000 \
  --screenshot=listening.png \
  "http://localhost:5173/stage.html?eyeball&state=listening&style=aura&level=0.7"
```

SwiftShader renders WebGL in software, so the orb and the aura draw headless. Headless Chrome may report a smaller viewport than `--window-size`; positions follow `innerWidth`/`innerHeight`.

## How it stays out of production

- `stage/main.tsx` lazy-loads `eyeball/EyeballPanel` only when `import.meta.env.DEV` is true and the URL has `?eyeball`; Vite folds the constant and drops the import.
- `OrbFrame.tsx` listens for the harness's `nexus:eyeball-orb-rect` event only under `import.meta.env.DEV`.
- Check after changing either: `vite build`, then `grep -rlE 'eyeball|EyeballPanel' dist/` must print nothing.

## What it does not show

Tauri events (wake, captions from Rust, the real orb rect) — the harness drives the store directly. WebKitGTK differences (the desktop app's engine on Linux) — confirm the final look on the desktop.
