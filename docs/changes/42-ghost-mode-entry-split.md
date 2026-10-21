# 42 — "Ghost Mode" Voice Entry Split (2026-09-25)

Live bug: saying "ghost mode" opened the Ghostwriter dictation room
(log: `EnterGhostwriter { contact: None }` + dictation sidebar) instead
of cursor-control Ghost Mode. Root cause was a naming collision — the
old dictation entry owned every "ghost mode" phrase, and the new
cursor-control mode had no voice entry path at all (`ghost_enter`
existed only as a manually-invoked Tauri command).

---

## 1. The split

- `parse_ghostwriter_entry` surrendered the bare-mode family ("ghost
  mode", "go/start/open ghost mode", "open the ghost mode"). Dictation
  now requires a writer word ("ghostwriter", "ghost writer",
  "dictation", "take a letter", …). Its contact-tail parsing is
  untouched.
- New `parse_ghost_control_entry` (checked FIRST): "ghost mode",
  "go/start/open ghost mode", "take the mouse", "control my/your
  cursor", "ghost cursor", "take control", ghost-control openers.
  Strict exact match — "ghost mode is confusing" enters nothing.
- New `ParsedIntent::EnterGhostControl` (`enter_ghost_control`) +
  label arm; routes `LocalCommand`; handled explicitly in
  `process_transcript` via `run_ghost_control_enter` (ghost session +
  ring + spoken "Ghost mode on…" — no sidebar card; the ring is the
  UI). NLU mapping untouched (the old entry was deterministic-only too).
- Ghostwriter exit phrases ("exit/stop/close ghost mode") deliberately
  unchanged — they exit dictation, a separate concern.

## 2. Verification (twice)

Full lib **547/547 serial** (new: 10 control phrases → EnterGhostControl
and never Ghostwriter; trailing-word sentences enter neither room;
`EnterGhostControl` routes LocalCommand). `cargo check` clean, zero new
warnings. Live check for the built app: say "ghost mode" → ring +
"Ghost mode on" with NO dictation sidebar; say "ghostwriter" →
dictation room as before.
