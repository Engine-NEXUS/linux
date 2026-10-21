# 119 — Shortcut Map + Action Router, keyboard tier (phase E1, 2026-10-09)

Implements phase **E1** of `docs/features/103-ghost-command-understanding-and-keyboard-first-implementation-plan.md`
(design: `102-keyboard-first-shortcut-brain-plan.md`; evidence: `docs/research/ghost-mode/09-…`, `11-p0-baseline-report-…`).

## What it does
"Do ACTION in the application that is in front" → look up the documented key chord for **that app** → re-check the foreground window
**immediately before** sending → send ONE batched `SendInput` by **virtual key** (layout-independent) → optionally verify via a title change.

## New code — `src-tauri/src/shortcuts/`
| File | Role | Pure? |
|---|---|---|
| `keys.rs` | `Chord` parsing (`Ctrl+Shift+T`, `Alt+Left`, `Ctrl+=`, `F1-24`, digits…), VK table, extended-key flag, exact press/release `key_events` | yes |
| `seed_windows.json` | Hand-written, documented defaults: families **browser** (chrome, brave, msedge, firefox, opera, vivaldi), **vscode**, **explorer**, **notepad**, **terminal** (Windows Terminal), **console** (cmd/powershell/pwsh/conhost…), plus a `*` table of near-universal Windows chords | data |
| `map.rs` | `Action` (23 closed ids shared with the understanding track), `ShortcutMap::from_json` **with validation**, `lookup` precedence **exe → family → global**, `no_global` rule, `{n}` tab templates | yes |
| `router.rs` | `run(env, map, action, arg)`: foreground → protected? → elevated? → binding → stop? → **pre-send same-window gate** → send → verify; `Outcome`, `reason()`/`spoken()` | yes (over an `Env` trait) |
| `exec.rs` | Windows adapter `WinEnv` (foreground probe, `SendInput`, ghost stop flag), `do_action`, `do_action_spoken`, `run_or_legacy`, IPC `shortcut_do`, `is_protected_window` | thin |

Safety properties (each has a test): sends **nothing** when the foreground changed between decision and send (different exe *or* different window of the same exe);
protected windows (credential managers/wallets/finance exes; title match limited to credential tools) get no keys; elevated targets are refused unless NEXUS is elevated;
**terminals/consoles never inherit the global edit chords** (Ctrl+C = interrupt, Ctrl+Z = suspend, Ctrl+S = XOFF); a pending ghost stop wins; bad tab numbers rejected before sending.

## Rewired call sites (Windows; `NEXUS_SHORTCUTS_LEGACY=1` restores every previous implementation)
- `command_executor::browser_key` (new tab, close tab, next tab, back) — was a **detached PowerShell `SendKeys` spawn**: hundreds of ms, no window check, reported success regardless.
- `screen::switch_browser_tab` (tab N), used by "open tab 3" and by "delete tab N" (a refused switch still prevents the close).
- `live/commands/browser.rs`: `new_tab`, and the address-bar step of `navigate` / `search` / `focus_search_bar` via `address_bar_hotkey()`.
- Unknown executables with no binding **fall back to the old behaviour** (e.g. a browser not in the seed); known apps without that shortcut and protected/changed windows are refused with a spoken reason. Success wording is unchanged ("New tab sir.", "Tab closed sir.", "Switched to tab 3, sir.").
- IPC `shortcut_do({action, arg?})` (dev + the upcoming grammar track): `invoke("shortcut_do", {action:"refresh"})`.
- Dev harness `src-tauri/dev_tools/shortcut_live.rs` (`cargo build --example shortcut_live`): sends one action **only if** the foreground exe equals `--expect-exe`.

## Verify
- **Rust: 36 new tests** (keys 6, map 10, router 19, exec 2 — includes the corpus gate: 25 actions × 4 browsers send the exact documented chord once, and a window swap before the send sends nothing: 100/100). Full main lib group **1224 passed, 0 failed, 8 ignored**; `cargo check --features custom-protocol,admin-brain` clean, zero warnings.
- **Mutation check:** with the pre-send foreground gate disabled, 3 tests fail (incl. the corpus gate); restored → green.
- **Live, on a real Notepad launched for the test** (every step guarded by `--expect-exe notepad.exe`): `zoom_in` → `Ctrl+=`, 3 ms, not checked · `new_tab` → `Ctrl+N`, **verified**, 42 ms · `prev_tab` / `next_tab` / `close_tab` → `Ctrl+Shift+Tab` / `Ctrl+Tab` / `Ctrl+W`, **verified**, 42–44 ms · `refresh` → NoBinding (known app: Notepad has no refresh) · `goto_tab_n(12)` → BadArg, nothing sent · Notepad cleaned up. For comparison the legacy PowerShell path pays a process start per key press.

## Not done / not verified
- **Not exercised against a real browser** or inside a live Ghost session end-to-end (voice → orchestrator → router). The browser chords for the four rewired actions are identical to the legacy ones, and the corpus gate covers all browser exes, but only Notepad was driven live.
- **No voice phrases yet** for the new actions (reopen tab, refresh, forward, next/previous tab, save, copy, paste, undo, redo, select all, scroll, zoom, find, switch window): the parser has no intents for them (research 10 §3.4). They are reachable via `shortcut_do` now and via the grammar in phase U2.
- `minimize`/`maximize` stay with the existing system intents; harvesting/teaching/menu-crawl sources (E4) not built; verification is title-change only (no UIA-based verify yet, E2).
- Windows only; macOS/Linux call sites unchanged. Release binary not rebuilt.
