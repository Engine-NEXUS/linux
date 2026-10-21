# Ghost Mode: no-vision desktop control research and evidence

Date: 2026-10-16  
Scope: Windows Ghost Mode command execution. Primary platform documentation was checked on 2026-10-16. Repository files are the source for NEXUS-specific implementation claims.

## Research question

Can NEXUS choose a keyboard command or a cursor action for a spoken task, learn where a control is, and repeat it reliably without a vision model?

For supported tasks, yes, with a semantic action map, Windows UI Automation (UIA), current foreground checks, and observed results. “Learn where” should mean learning an app-scoped control identity and resolving its position at execution time. A fixed pixel coordinate cannot survive a window move, resize, scroll, DPI change, or application update. Universal perfection is not supported by the platform evidence: an application may expose no usable accessibility provider, a command may be remapped, and a click can have no observable effect.

The implemented first slice is described in [change 133](../../changes/133-ghost-command-center-implementation-deep-dive-2026-10-16.md). The broader design and gates are in [plan 129](../../features/129-ghost-mode-reliable-desktop-control-implementation-plan.md).

## Source and inference ledger

| Evidence from a primary source | Design consequence for Ghost | Boundary |
|---|---|---|
| Microsoft describes UIA control patterns as capabilities and lists Invoke, Toggle, SelectionItem, and other patterns. [Microsoft: Control Patterns Overview](https://learn.microsoft.com/en-us/windows/win32/winauto/uiauto-controlpatternsoverview) | Query a live control for a semantic action before relying on a mouse point. | Availability of a pattern does not establish that a task completed. |
| Microsoft recommends caching UIA properties and patterns to reduce cross-process calls; current data must be requested for current state. [Microsoft: Caching UIA Properties and Control Patterns](https://learn.microsoft.com/en-us/windows/win32/winauto/uiauto-cachingforclients) | Use cached discovery for speed and reacquire a live element immediately before acting. | A historical snapshot is not a valid click target. |
| Microsoft's GetClickablePoint documentation says a point may be unavailable and does not guarantee that clicking causes an action. [Microsoft: GetClickablePoint](https://learn.microsoft.com/en-us/dotnet/api/system.windows.automation.automationelement.getclickablepoint) | Require a fresh point, hit test, foreground check, guarded click, and separate postcondition. | Cursor dispatch can remain Unconfirmed. |
| Microsoft documents that SendInput is subject to User Interface Privilege Isolation (UIPI). [Microsoft: SendInput](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-sendinput) | Keep elevation and protected-window checks in the existing shortcut/mouse route. | Input acceptance is not application-level completion. |
| Microsoft describes providers as the means by which applications expose their controls to UIA. [Microsoft: UIA Providers Overview](https://learn.microsoft.com/en-us/windows/win32/winauto/uiauto-providersoverview) | Report unsupported controls when no usable accessible identity exists. | Custom drawing can reduce or remove usable semantics. |
| Microsoft says AutomationId need not be unique across the whole tree and recommends contextual lookup. [Microsoft: Use the AutomationId Property](https://learn.microsoft.com/en-us/dotnet/framework/ui-automation/use-the-automationid-property) | Require uniqueness in the current foreground window, app fingerprint, type and name hash; add ancestry/context in the next phase. | Current strict uniqueness prevents some teachable controls from being saved. |
| VS Code documents user-defined bindings and conditional when clauses. [VS Code: Keyboard Shortcuts](https://code.visualstudio.com/docs/configure/keybindings) | A seeded shortcut needs app and focus-context validation; user overrides should be considered before broad support claims. | A chord can be correct in one state and wrong in another. |
| Electron describes accessibility support for its apps. [Electron: Accessibility](https://www.electronjs.org/docs/latest/tutorial/accessibility) | Test Electron apps individually rather than assuming every control is accessible. | Framework support is not proof for a particular app/version/window. |
| Chromium documents its Windows UIA support rollout. [Chromium: Windows UIA Support Update](https://developer.chrome.com/blog/windows-uia-support-update) | Browser controls and web content are promising audit candidates. | Browser support does not establish successful control of a particular page. |

The “design consequence” column is an engineering inference from each linked source, not a quotation or guarantee by the platform vendor.

## Local repository evidence

1. [Research 09](09-keyboard-first-shortcut-knowledge-and-zero-delay-mouse-fallback-2026-10-08.md) and [change 119](../../changes/119-shortcut-map-and-action-router-e1.md) document the earlier Shortcut Map and Action Router. The implementation in src-tauri/src/shortcuts/ is reused by Ghost's KeyboardCenter.
2. [Baseline 11](11-p0-baseline-report-2026-10-08.md) contains October 8 UIA probes in disposable apps. Its lookup timings and control-pattern counts are historical measurements, not a live acceptance result for the current implementation.
3. [Research 10](10-command-understanding-phrasing-space-and-measured-accuracy-2026-10-08.md) found understanding errors in generated phrases. It explains why route selection must follow strict intent/slot validation. Those figures do not measure today's live speech accuracy.
4. [Research 12](12-no-vision-keyboard-and-cursor-learning-plan-2026-10-16.md) gives the original no-vision decision ladder, privacy boundary, and staged proposal. It is explicitly a research/plan document; [change 133](../../changes/133-ghost-command-center-implementation-deep-dive-2026-10-16.md) records which parts have since been built.
5. Current implementation evidence comes from [ghost_center.rs](../../../src-tauri/src/ghost_center.rs), [ghost_learning.rs](../../../src-tauri/src/ghost_learning.rs), [center.rs](../../../src-tauri/src/center.rs), [orchestrator.rs](../../../src-tauri/src/orchestrator.rs), and [mouse.rs](../../../src-tauri/src/live/commands/mouse.rs). Source code demonstrates the guards and routing; focused tests demonstrate only their covered cases.

## Decision model justified by the evidence

The intended order for an understood, approved task is:

1. Resolve a canonical action, target app/window, control, and risk class. Clarify if any are ambiguous.
2. Use an app-native action where an existing supported command API provides it.
3. Use a known shortcut only for the matching app and currently valid focus context.
4. Use a fresh UIA semantic pattern on exactly one enabled control.
5. Use a fresh UIA clickable point and a guarded cursor action when semantic action is unavailable.
6. If no route exists, or the target changes, stop and explain the reason.
7. Observe an action-specific postcondition before saying completion was verified.

The first implementation supplies a strict subset of this model. It has no image interpretation, screenshot/OCR fallback, or vision-model targeting in its new Ghost action path. It does not yet discover every app-native action, check every user keybinding override, or verify all Invoke/cursor effects.

## Why a saved coordinate is insufficient

A saved (x, y) says nothing about which app owns the window or which control moved there. A learned semantic locator can be re-evaluated in the current foreground window. The current store uses executable identity, an app build fingerprint, AutomationId, control type, and accessible-name hash. It rejects duplicate or changed identities. This is deliberately conservative. Microsoft's AutomationId guidance motivates adding parent/ancestor context in the next phase, after live measurement shows where strict uniqueness loses useful coverage.

A saved location may be useful only as a short-lived candidate within one unchanged screen state. It must never be the authority for a learned rule. This is an inference from UIA's identity model and from ordinary layout changes, and is encoded as the no-coordinate policy in ghost_learning.rs.

## Evidence quality and gaps

| Question | Current evidence | Needed for release |
|---|---|---|
| Does the main center own Ghost turns? | Code path and focused routing tests. | Live interruption and queued-turn trace. |
| Does a shortcut target the right window? | Existing router checks and unit coverage. | Adversarial focus-switch trial in each supported app. |
| Does an accessible click find the right control? | Exact/unique UIA matching and current-point hit test in code. | Duplicate-label, occlusion, scroll, mixed-DPI and app-update trials. |
| Did the application do the requested thing? | Toggle/selection observation for supported cases; other dispatches marked Unconfirmed. | Per-action postconditions for Invoke, cursor and more shortcuts. |
| Does learning survive layout movement? | Semantic identity and no saved coordinates in code. | Teach, resize/scroll/restart, then replay in disposable apps. |
| Can every app be controlled? | No such evidence. Platform docs explain provider and privilege limits. | Publish supported-task matrix and unsupported rate; do not make a universal claim. |
| Is the path free of vision-model cost? | New Ghost command path and older direct click delegation contain no vision fallback. | Runtime trace of representative actions; other explicit vision features are outside this claim. |

## Proposed controlled research run

Use disposable data in Notepad, Explorer, a browser test page, VS Code test workspace, and one intentionally selected communication-app test view. Include an inaccessible custom canvas. For each action record transcript, parsed task, active app/HWND, selected route, UIA identity, action dispatch, observed postcondition, outcome, and latency. Exercise duplicate labels, hidden/disabled controls, changed foreground window at the last moment, moving windows, scrolling, mixed DPI, app update or remapped shortcut, cancellation, and ambiguous speech. Do not record private text-field values or personal chats.

Publish counts per app and action: verified completion, unconfirmed dispatch, blocked/unsupported, wrong-target prevention, wrong action, and latency distribution. The safety gate is zero observed actions in the wrong foreground window, zero unconfirmed automatic replay, and no external/destructive act without its existing confirmation. A proposed high completion percentage only becomes meaningful after supported-task scope and a held-out trial set are specified.

## Source list

Official platform and application references, checked 2026-10-16:

- [Microsoft UI Automation Control Patterns Overview](https://learn.microsoft.com/en-us/windows/win32/winauto/uiauto-controlpatternsoverview)
- [Microsoft Caching UI Automation Properties and Control Patterns](https://learn.microsoft.com/en-us/windows/win32/winauto/uiauto-cachingforclients)
- [Microsoft AutomationElement.GetClickablePoint](https://learn.microsoft.com/en-us/dotnet/api/system.windows.automation.automationelement.getclickablepoint)
- [Microsoft SendInput](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-sendinput)
- [Microsoft UI Automation Providers Overview](https://learn.microsoft.com/en-us/windows/win32/winauto/uiauto-providersoverview)
- [Microsoft Use the AutomationId Property](https://learn.microsoft.com/en-us/dotnet/framework/ui-automation/use-the-automationid-property)
- [VS Code Keyboard Shortcuts](https://code.visualstudio.com/docs/configure/keybindings)
- [Electron Accessibility](https://www.electronjs.org/docs/latest/tutorial/accessibility)
- [Chromium Windows UIA Support Update](https://developer.chrome.com/blog/windows-uia-support-update)
