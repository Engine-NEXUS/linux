# Ghost Mode Research — Index

Current records: [first Ghost command-center implementation](../../changes/133-ghost-command-center-implementation-deep-dive-2026-10-16.md), [no-vision research with primary sources](13-no-vision-control-evidence-sources-2026-10-16.md), and [release plan](../../features/129-ghost-mode-reliable-desktop-control-implementation-plan.md).

Understand-first docs for the Ghost Mode program (Windows-only AI
cursor/keyboard control). Implementation record lives in
`docs/changes/37–46`; the live plan in `docs/features/64`.

| File | What it answers |
|---|---|
| `01-clicky-and-grounding.md` | How Clicky AI actually works (teardown): fake triangle, point-tag contract, Computer Use coordinates, Worker-proxy pattern — and why its "no delay" is stagecraft |
| `02-costs-and-limits.md` | Grounding options (UIA → OmniParser → hosted), perfection benchmark limits, RAM/cost ledger, hard OS limits (UIPI, fullscreen, DPI) |
| `03-irregularities-and-fixes-timeline.md` | Every live defect hit during voice-mode bring-up, its evidence, root cause, fix, and standing guard — plus the lessons distilled from those fixes |
