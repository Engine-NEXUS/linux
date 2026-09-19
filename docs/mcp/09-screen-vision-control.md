# Screen Vision + Control (2026-09-19)

User asks in many ways ("click the 3rd option", "move to the 4th tab",
"what's that button", "press the second link") — one grounding engine
serves all phrasings.

## Methods compared

| Method | Accuracy | Cost | Verdict |
|---|---|---|---|
| Windows UIA tree (already a dependency) | Exact names + boxes for native/browser UI | free, ~50ms | **primary** — no model, no download |
| OmniParser-style vision model | Handles canvas/custom UIs UIA can't see | GPU + 100s MB download | phase 2 fallback only |
| Windows OCR | Text without structure | free, slower | phase 2 for canvas text |
| Tab hotkeys (Ctrl+1..8) | 100% for browser tabs | free, instant | **always prefer over grounding for tabs** |

## Ordinal resolution rule

Visible + enabled actionables (Button, Hyperlink, TabItem, MenuItem,
ListItem, Checkbox, RadioButton) in the foreground window, sorted
top-to-bottom then left-to-right. "3rd option" = 3rd actionable;
"4th tab" = Ctrl+4 first, UIA TabItem fallback. Never act on
password/secure fields without explicit confirm; destructive targets
(delete/close/remove) always confirm.

## Question forms (all map to the same engine)

click/press/choose/tap + [the] + ordinal + [option/button/link/tab];
move/go/switch + to + ordinal + tab; "what's the Nth X" (read back name
instead of clicking); "click on X" by visible name (substring match).
