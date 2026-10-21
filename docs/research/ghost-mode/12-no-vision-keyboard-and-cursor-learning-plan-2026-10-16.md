# Ghost Mode: learning keyboard and cursor actions without a vision model

**Date:** 2026-10-16  
**Status:** research and plan only; no behavior changed by this document.  
**Scope:** Windows Ghost Mode first. No screenshot, OCR, local image model, or cloud vision model is part of the proposed execution path.

## 1. Answer to the product question

Yes: NEXUS can learn which supported tasks an application exposes as a keyboard shortcut, which controls can be operated through Windows UI Automation (UIA), and which remaining controls need the real cursor. It can locate most accessible controls again at action time, even after they move. This should be learned as **action + app/context + control identity + observed result**, not as a permanent `(x, y)` coordinate.

No: a promise of perfect control in *every* app without vision is not technically honest. A custom-drawn canvas may expose neither semantic controls nor useful keyboard commands. UIA can report a clickable point that does not perform the desired action, and apps can change their UI or shortcut bindings. In such a case Ghost Mode must explain the limitation, ask the user to identify the control, or leave the action to the user. It must never invent a coordinate or claim success because input was sent.

The desired user experience is still simple: say the task; NEXUS chooses the fastest **verified** route and keeps the Ghost session open for the next instruction.

## 2. Existing NEXUS work to preserve

| Existing piece | Verified repository state | Consequence |
|---|---|---|
| Shortcut Map / Action Router | `src-tauri/src/shortcuts/{map,router,exec,keys}.rs`, seed for browser, VS Code, Explorer, Notepad and console families; same-HWND foreground check before `SendInput`; stop/protected/elevated checks. Change 119 reports a live Notepad run, not a browser or live Ghost voice run. | Extend this router; do not build a second key injector. Its current `Verified::Unconfirmed` and `NotChecked` outcomes must not be counted as success. |
| Cached UIA lookup | `screen.rs::list_actionables` uses one window-rooted `FindAllBuildCache` call and reads Name/ControlType/bounds/enabled/offscreen. October 8 Notepad re-test: 24-44 ms, 23/23 controls. | Keep this fast lookup. Add identity and pattern metadata in a measured extension. |
| Mouse execution | `live/commands/mouse.rs` resolves by control name, takes the rectangle center, performs a 550-750 ms glide plus 100 ms dwell, and can call cloud vision on UIA miss. | A new no-vision policy must remove vision from the candidate list. Current midpoint is not a durable learned position. Keep stop/focus/fullscreen guards. |
| Voice understanding | `orchestrator.rs` still calls `parse_deterministic`; the proposed `cmdu` normalizer/grammar modules in plan 103 are absent. The October 8 generated-phrase study found substantial misses and wrong actions; it is a historical baseline, not a current live accuracy measurement. | The route selector cannot rescue a wrong intent. Improve recognition and clarification before claiming task reliability. |
| Ghost turns | `ghostHotMic.ts` relistens between turns, and Rust queues commands spoken during a drill. | A capability probe, teaching operation, or cursor action must participate in the same stop and queue lifecycle. |

Sources in this repository: [research 09](09-keyboard-first-shortcut-knowledge-and-zero-delay-mouse-fallback-2026-10-08.md), [baseline 11](11-p0-baseline-report-2026-10-08.md), [plan 103](../../features/103-ghost-command-understanding-and-keyboard-first-implementation-plan.md), [change 119](../../changes/119-shortcut-map-and-action-router-e1.md). The original plan 102 includes a cloud vision fallback; this document replaces that fallback for the requested no-vision mode.

## 3. Research findings that determine the design

1. **UIA describes abilities, not just rectangles.** `Invoke`, `Toggle`, `SelectionItem`, `ExpandCollapse`, `Value`, and `Scroll` patterns can operate controls without moving the cursor. Microsoft's [control-pattern overview](https://learn.microsoft.com/en-us/windows/win32/winauto/uiauto-controlpatternsoverview) describes their roles. In the NEXUS October 8 probe, 52-91% of actionable controls in Notepad, Explorer, and Calculator exposed `Invoke`, but that availability was not an end-to-end action-success test.
2. **Bulk reads matter.** Microsoft's [UIA caching guidance](https://learn.microsoft.com/en-us/windows/win32/winauto/uiauto-cachingforclients) says one cache request avoids many cross-process property calls. Cached pattern objects cannot perform actions; before `Invoke` NEXUS needs a fresh live element. A cache is a discovery aid, never permission to act on stale state.
3. **Shortcut metadata is incomplete.** The NEXUS probe found non-empty `AcceleratorKey` on only 0-3% of sampled controls, while `AccessKey`, control names and patterns helped more. Seeded app shortcuts, application keybinding files, and user teaching must carry much of the knowledge. UIA's [element property list](https://learn.microsoft.com/en-us/windows/win32/winauto/uiauto-automation-element-propids) defines these properties. [VS Code documents](https://code.visualstudio.com/docs/configure/keybindings) user overrides and context-dependent `when` clauses, so a chord is not necessarily valid in every focused control.
4. **Identifiers have scope and age limits.** Microsoft says `AutomationId` may only be unique among siblings and that playback in volatile UIs can break; [its AutomationId guidance](https://learn.microsoft.com/dotnet/framework/ui-automation/use-the-automationid-property) recommends parent context. Do not persist a UIA runtime ID or screen coordinate as the sole identity of a control.
5. **A clickable point is not proof of action.** Microsoft's [GetClickablePoint documentation](https://learn.microsoft.com/en-us/dotnet/api/system.windows.automation.automationelement.getclickablepoint) says it may be unavailable when obscured, and explicitly does not guarantee that clicking it does anything. Use a fresh point, a foreground check, a hit test, and a post-action observation.
6. **App coverage varies.** Chromium's [native Windows UIA support](https://developer.chrome.com/blog/windows-uia-support-update) improves its accessibility surface, but it does not prove a particular Brave page or Electron app exposes the requested control. [Electron's accessibility documentation](https://www.electronjs.org/docs/latest/tutorial/accessibility) says tree exposure can depend on accessibility activation. Microsoft's [provider guidance](https://learn.microsoft.com/en-us/windows/win32/winauto/uiauto-providersoverview) explains why custom controls without providers can be opaque.
7. **Events help, but are not a guarantee.** UIA focus, structure and property events can invalidate a warm snapshot; Microsoft's [event guidance](https://learn.microsoft.com/en-us/windows/win32/winauto/uiauto-eventsforclients) warns that providers may omit events. Refresh immediately before an action and inspect the intended effect afterward.
8. **Windows security boundaries remain.** Microsoft's [UIPI guidance](https://learn.microsoft.com/en-us/troubleshoot/power-platform/power-automate/desktop-flows/ui-automation/uipi-issues) describes blocked input to elevated apps and other unusable desktop states. Ghost Mode should report those conditions; learning a shortcut does not bypass them.

## 4. A single no-vision decision ladder

```
spoken request
  -> identify canonical action + target app/window + object + risk
  -> reject/clarify if intent or target is ambiguous
  -> inspect capabilities for that exact app, UI state and focus context
  -> app-native command or documented/learned keyboard shortcut, if valid
  -> live UIA pattern on the exact control, if supported
  -> fresh UIA clickable point + guarded real-cursor action, if needed
  -> no safe local route: ask the user or report unsupported
  -> verify the intended effect; record only verified evidence
```

An app-native action (for example an existing supported command API) can be an even stronger route than a shortcut, but must keep the same target, risk and verification contract. **No model or screenshot fallback is implied by a UIA miss.** OCR is also excluded from this plan so the cost and privacy boundary is unambiguous.

### Route choice

- **Keyboard:** known chord for the executable **and** current focus state; foreground window must still match immediately before sending. A shortcut with unknown `when` context is only a candidate, not a guaranteed action.
- **UIA pattern:** use the control's live semantic action when a stable locator finds exactly one matching enabled target. This can outperform both keyboard and cursor for a named button.
- **Cursor:** only when no reliable command/pattern exists and the exact control is present, visible, enabled, within the intended window, and has a fresh clickable point. `GetClickablePoint` is preferable to rectangle center; still verify the point and result.
- **Unsupported/ambiguous:** if there are zero or multiple plausible targets, no usable accessibility provider, an inaccessible canvas, an elevated/protected window, or no observable effect, stop and say what was missing. A user may manually show a control, but NEXUS must not silently extrapolate to other layouts.

## 5. What “learn where to move” should mean

Persist **semantic locators**, never a naked coordinate:

```
LearnedControl {
  app_exe, app_version_or_build, window_role,
  action_id, control_type,
  automation_id_if_present, normalized_name,
  parent/ancestor identity, nearby stable label,
  required_focus_or_tab_context,
  preferred_route, source, last_verified,
  verified_successes, observed_failures
}
```

On every use, NEXUS re-finds the control in the current foreground window, requires a unique match, retrieves its current pattern or clickable point, checks the same window still owns focus, then acts. The pixel location exists only for that single execution. A DPI change, scroll, dialog, resize, app update, or different document invalidates any cached rectangle.

Learning sources, in order of confidence:

1. Bundled, versioned action maps and the application's own command/keybinding data; respect user overrides and context.
2. Current UIA tree: control type, AutomationId plus ancestry, name/label, supported patterns, AccessKey/AcceleratorKey, enabled/offscreen state.
3. User teaching: “When I say X in this app, use this shortcut” or a deliberate demonstration. For a demonstrated click, `ElementFromPointBuildCache` can identify the element under the user's click without image analysis; store its semantic locator only if one exists. If no semantic element exists, mark the teaching as *layout-specific and unverified*, not a general rule.
4. Execution feedback: promote a route after the intended state change is observed on representative runs; demote or invalidate on failure, app version change, shortcut remap, or target ambiguity. Do not learn from `SendInput` returning success alone.

Private typed text, message content, passwords, entire accessibility trees and screenshots are not part of the learned record. User-taught rules need a way to inspect and delete them. For an inaccessible custom canvas, ask the user to act manually or use an app-specific supported command; no coordinate-learning claim is made.

## 6. Verification contract

Each canonical action defines its *own* observable postcondition, for example: selected tab changed, URL changed to the requested value, checkbox state changed, named dialog opened, field focus/value changed, or expected window appeared. A title change is not a universal proof. UIA events can wake the verifier, but the verifier reads current state because an event may be missed or spurious.

Outcomes are `Verified`, `NoEffectProven`, `Unconfirmed`, `WrongTargetPrevented`, `Blocked`, or `Stopped`:

- Only `Verified` updates learned success and permits “Done.”
- `Unconfirmed` reports uncertainty. It must not trigger a second click/keypress on a possibly non-idempotent action such as Send, Delete, Purchase, or Toggle.
- One alternative route may be tried only when the first attempt was **proven to have no effect** and the action is safe to repeat. Reacquire the foreground and control before the alternative.
- Destructive and external actions keep their existing confirmation policy regardless of whether keyboard, pattern or cursor is chosen.

## 7. Plan, ordered from the current codebase

| Phase | Research/implementation work for a later task | Exit evidence |
|---|---|---|
| P0: real-app capability audit | Reuse `uia_probe` read-only on a throwaway Brave page, VS Code test workspace, WhatsApp test chat, Explorer and Notepad. Record cold/warm tree time, pattern coverage, unique locator rate, clickable-point availability and foreground stability. Include a custom canvas case. Do not inspect personal chats/windows without a deliberate test setup. | Per-app table with raw counts, p50/p95 and unsupported examples; no behavior change. |
| P1: intent contract | Finish the normalizer/grammar and slot-validation work from plan 103 so “save,” “open the next tab,” and “click the Search button” become distinct canonical actions. Add an explicit `Clarify` outcome when the app, target or verb is unclear. | Held-out user phrasings, real-log replay and ambient false-action measurements; no new wrong-action regressions. |
| P2: capability inventory | Extend the cached UIA query with AutomationId, ancestry, AccessKey/AcceleratorKey, pattern availability and `IsPassword`; add a per-app capability store using the existing Shortcut Map as the key source. Use bounded foreground-only discovery, event invalidation plus pre-action refresh. | Measured overhead, unique-target coverage and no sensitive-value persistence. |
| P3: pattern route | Reacquire a unique live element and use Invoke/Select/Toggle/Expand as appropriate. Define action-specific postconditions. | Real-app pattern attempts classified as Verified/NoEffect/Unconfirmed; no duplicate action after Unconfirmed. |
| P4: guarded cursor route | On pattern/shortcut absence, re-resolve a unique control, obtain a fresh clickable point, hit-test against the intended target/window, check DPI/display and foreground, then move/click once. Remove the vision fallback for the no-vision Ghost policy. | 0 wrong-window/wrong-control actions in adversarial focus and moving-layout tests; measured latency. |
| P5: learning and teaching | Read VS Code/default/user keybindings, harvest UIA hints, add explicit teach/forget commands, and persist successful semantic routes with version/context. Demote on failure. | A taught control still works after resize/scroll, or declines safely if identity changes; user can view/erase the rule. |
| P6: rollout | Compare route choice and completion in throwaway real apps, first in preview, then opt-in live, then default only for proven app/action pairs. | Publish per-app supported-task success, wrong-action, ambiguous-decline and p50/p95 latency; do not aggregate them into a “perfect” claim. |

P0 and P1 can be prepared independently. P2 must precede P3/P4. Existing Shortcut Map E1 and window-rooted UIA E0.5 are already built and should not be reimplemented.

## 8. Test matrix and acceptance criteria

- **Tasks:** open/close/switch tab, search, save, focus field, select list item, toggle checkbox, click a named button, open a WhatsApp chat, prepare a WhatsApp draft, and stop mid-action. Include repeated labels, hidden/disabled elements, menus, scrolling, resize, mixed DPI, window switch at the final millisecond, and app update/remapped shortcut.
- **Negative set:** conversational room speech containing “open/close/type,” misheard app names, inaccessible canvas, elevated/protected window, lost UIA provider, UIA timeout, stale handle, stale rectangle and provider that reports Invoke but performs nothing.
- **Primary release gate:** zero observed actions on a different foreground window; zero sends/deletes/purchases without their confirmation; zero automatic replays after an unconfirmed effect. A failure to understand must clarify or decline, never choose a plausible but unverified target.
- **Coverage gate:** report success **per app and action** on held-out tasks, plus unsupported rate. A proposed ≥98% supported-task completion target is meaningful only after P0 defines supported tasks and live tests measure it; it is not a current NEXUS result.
- **Speed gate:** measure `understand`, `capability lookup`, `act`, `verify` separately. Existing 40/80/250 ms keyboard/pattern/mouse figures in plan 102 are targets, not measurements; retain or revise them after real-app p95 runs.

## 9. Explicit limits and product wording

“I can use this app's shortcut,” “I found its Save button,” and “I need you to show me this custom control” are honest status messages. “I know exactly where every button will be” is not. The accessibility provider, app configuration and current UI state are the source of position at the moment of use. Where those sources are absent, a no-vision Ghost Mode must stop safely.

This plan requires no new LLM tokens for simple desktop actions. It does **not** remove the cost of speech recognition already in the voice pipeline; it removes screenshot interpretation and vision-model calls from the action-selection and targeting path.
