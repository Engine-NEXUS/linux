//! Ghost Mode command center. The main command center owns the turn and its
//! ownership/validity gates; this center owns only foreground desktop actions.
//! Each sub-center returns an honest receipt. No vision/OCR path exists here.

use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

use crate::shortcuts::{map::Action, router};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum GhostStep {
    Shortcut { action: String, arg: Option<u32> },
    AccessibleClick { name: String },
    Learn { alias: String },
    Forget { alias: String },
    LearnedClick { alias: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GhostTask {
    pub steps: Vec<GhostStep>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Route {
    KeyboardCenter,
    AccessibilityCenter,
    CursorCenter,
    LearningCenter,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    Verified,
    Unconfirmed,
    Blocked,
    Stopped,
}

#[derive(Debug, Clone, Serialize)]
pub struct Receipt {
    pub step: usize,
    pub route: Route,
    pub outcome: Outcome,
    pub detail: String,
    pub elapsed_ms: u64,
}

impl Receipt {
    pub fn spoken(&self) -> String {
        match self.outcome {
            Outcome::Verified => format!("{}, sir.", self.detail),
            Outcome::Unconfirmed => format!(
                "I tried that, sir, but couldn't verify the result. {}",
                self.detail
            ),
            Outcome::Blocked => format!("I couldn't do that safely, sir. {}", self.detail),
            Outcome::Stopped => "Stopped, sir. Ghost mode is still on.".into(),
        }
    }
}

/// Strict, local grammar for low-risk desktop actions. A compound command is
/// accepted only if EVERY step parses; unknown text never becomes a click.
pub fn parse(transcript: &str) -> Option<GhostTask> {
    let mut text = transcript
        .trim()
        .trim_end_matches(['.', '!', '?'])
        .to_lowercase();
    loop {
        let next = ["nexus, ", "nexus ", "please ", "can you ", "could you "]
            .iter()
            .find_map(|prefix| text.strip_prefix(prefix).map(|rest| rest.to_string()));
        match next {
            Some(rest) => text = rest,
            None => break,
        }
    }
    let parts: Vec<&str> = text.split(" then ").collect();
    if parts.is_empty() || parts.len() > 5 {
        return None;
    }
    let steps = parts
        .into_iter()
        .map(parse_step)
        .collect::<Option<Vec<_>>>()?;
    Some(GhostTask { steps })
}

fn parse_step(raw: &str) -> Option<GhostStep> {
    let t = raw.trim().trim_start_matches("please ").trim();
    let t = t.trim_start_matches("the ");
    for prefix in [
        "learn this as ",
        "remember this button as ",
        "remember this control as ",
    ] {
        if let Some(alias) = t.strip_prefix(prefix) {
            return valid_alias(alias).then(|| GhostStep::Learn {
                alias: alias.into(),
            });
        }
    }
    if let Some(alias) = t.strip_prefix("forget learned ") {
        return valid_alias(alias).then(|| GhostStep::Forget {
            alias: alias.into(),
        });
    }
    if let Some(alias) = t.strip_prefix("click learned ") {
        return valid_alias(alias).then(|| GhostStep::LearnedClick {
            alias: alias.into(),
        });
    }
    let action = match t {
        "save" | "save this" | "save file" | "save this file" | "save the file" => {
            Some(Action::Save)
        }
        "copy" | "copy this" | "copy selection" => Some(Action::Copy),
        "paste" | "paste this" => Some(Action::Paste),
        "cut" | "cut this" => Some(Action::Cut),
        "undo" | "undo that" => Some(Action::Undo),
        "redo" | "redo that" => Some(Action::Redo),
        "select all" | "select everything" => Some(Action::SelectAll),
        "refresh" | "reload" | "refresh page" | "reload page" => Some(Action::Refresh),
        "go back" | "back" => Some(Action::GoBack),
        "go forward" | "forward" => Some(Action::GoForward),
        "new tab" | "open new tab" | "open a new tab" => Some(Action::NewTab),
        "close tab" | "close this tab" => Some(Action::CloseTab),
        "reopen tab" | "reopen closed tab" => Some(Action::ReopenTab),
        "next tab" | "switch to next tab" => Some(Action::NextTab),
        "previous tab" | "prev tab" | "switch to previous tab" => Some(Action::PrevTab),
        "address bar" | "focus address bar" => Some(Action::AddressBar),
        "find" | "find in page" | "search in page" => Some(Action::FindInPage),
        "scroll down" => Some(Action::ScrollDown),
        "scroll up" => Some(Action::ScrollUp),
        "zoom in" => Some(Action::ZoomIn),
        "zoom out" => Some(Action::ZoomOut),
        "switch window" => Some(Action::SwitchWindow),
        _ => None,
    };
    if let Some(action) = action {
        return Some(GhostStep::Shortcut {
            action: action.id().into(),
            arg: None,
        });
    }
    for prefix in ["switch to tab ", "go to tab ", "tab "] {
        if let Some(n) = t.strip_prefix(prefix).and_then(|v| v.parse::<u32>().ok()) {
            if (1..=9).contains(&n) {
                return Some(GhostStep::Shortcut {
                    action: Action::GotoTab.id().into(),
                    arg: Some(n),
                });
            }
        }
    }
    let name = t
        .strip_prefix("click ")
        .or_else(|| t.strip_prefix("press "))?
        .trim_start_matches("the ")
        .trim();
    let name = name
        .strip_suffix(" button")
        .or_else(|| name.strip_suffix(" tab"))
        .or_else(|| name.strip_suffix(" option"))
        .unwrap_or(name)
        .trim();
    if name.len() < 2 || name.len() > 80 || is_risky_target(name) || is_ambiguous_target(name) {
        return None;
    }
    Some(GhostStep::AccessibleClick { name: name.into() })
}

pub(crate) fn is_risky_target(name: &str) -> bool {
    let lower = name.to_lowercase();
    if ["sign in", "log in", "close account", "place order"]
        .iter()
        .any(|phrase| lower.contains(phrase))
    {
        return true;
    }
    [
        "send",
        "submit",
        "delete",
        "remove",
        "buy",
        "purchase",
        "pay",
        "transfer",
        "confirm",
        "allow",
        "authorize",
        "install",
        "password",
        "erase",
        "discard",
        "publish",
        "post",
        "withdraw",
        "approve",
        "reject",
        "unsubscribe",
        "disconnect",
        "archive",
        "reset",
        "clear",
        "revoke",
        "grant",
        "share",
    ]
    .iter()
    .any(|word| {
        lower
            .split(|c: char| !c.is_alphanumeric())
            .any(|part| part == *word)
    })
}

fn valid_alias(alias: &str) -> bool {
    let alias = alias.trim();
    (2..=40).contains(&alias.len())
        && alias
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == ' ' || c == '-' || c == '_')
        && !is_risky_target(alias)
        && !is_ambiguous_target(alias)
}

pub(crate) fn is_ambiguous_target(name: &str) -> bool {
    let name = name.to_lowercase();
    let base = name
        .strip_suffix(" button")
        .or_else(|| name.strip_suffix(" option"))
        .or_else(|| name.strip_suffix(" tab"))
        .unwrap_or(&name);
    matches!(
        base,
        "here"
            | "there"
            | "this"
            | "that"
            | "it"
            | "one"
            | "button"
            | "option"
            | "ok"
            | "yes"
            | "continue"
            | "next"
            | "accept"
            | "agree"
            | "red"
            | "blue"
            | "green"
            | "yellow"
            | "black"
            | "white"
    ) || name.ends_with(" one")
        || name.ends_with(" thing")
}

pub fn validate(task: &GhostTask) -> bool {
    !task.steps.is_empty()
        && task.steps.len() <= 5
        && task.steps.iter().all(|step| match step {
            GhostStep::Shortcut { action, arg } => Action::from_id(action).is_some_and(|a| {
                a.takes_number() == arg.is_some() && arg.is_none_or(|n| (1..=9).contains(&n))
            }),
            GhostStep::AccessibleClick { name } => {
                name.len() >= 2
                    && name.len() <= 80
                    && !is_risky_target(name)
                    && !is_ambiguous_target(name)
            }
            GhostStep::Learn { alias }
            | GhostStep::Forget { alias }
            | GhostStep::LearnedClick { alias } => valid_alias(alias),
        })
}

/// Sequential execution is intentional: after an unverified step, the next
/// command may target a different UI state and must not be sent blindly.
pub fn execute_with_store(
    task: &GhostTask,
    cancel: &AtomicBool,
    store_dir: Option<&Path>,
) -> Vec<Receipt> {
    if !validate(task) {
        return vec![Receipt {
            step: 0,
            route: Route::KeyboardCenter,
            outcome: Outcome::Blocked,
            detail: "The Ghost command is incomplete or unsafe.".into(),
            elapsed_ms: 0,
        }];
    }
    let mut receipts = Vec::with_capacity(task.steps.len());
    for (index, step) in task.steps.iter().enumerate() {
        if cancel.load(Ordering::Relaxed)
            || crate::ghost::stop_requested()
            || !crate::ghost::session_active()
        {
            receipts.push(Receipt {
                step: index,
                route: route_for(step),
                outcome: Outcome::Stopped,
                detail: "The task was stopped.".into(),
                elapsed_ms: 0,
            });
            break;
        }
        let start = Instant::now();
        let (route, outcome, detail) = match step {
            GhostStep::Shortcut { action, arg } => keyboard_subcenter(action, *arg, cancel),
            GhostStep::AccessibleClick { name } => accessibility_subcenter(name, cancel),
            GhostStep::Learn { alias } => learning_subcenter(alias, store_dir, true),
            GhostStep::Forget { alias } => learning_subcenter(alias, store_dir, false),
            GhostStep::LearnedClick { alias } => learned_click_subcenter(alias, store_dir, cancel),
        };
        let receipt = Receipt {
            step: index,
            route,
            outcome,
            detail,
            elapsed_ms: start.elapsed().as_millis() as u64,
        };
        tracing::info!(
            "[GHOST-CENTER] step={} route={route:?} outcome={outcome:?} elapsed_ms={}",
            index,
            receipt.elapsed_ms
        );
        receipts.push(receipt);
        if outcome != Outcome::Verified {
            break;
        }
    }
    receipts
}

fn route_for(step: &GhostStep) -> Route {
    match step {
        GhostStep::Shortcut { .. } => Route::KeyboardCenter,
        GhostStep::AccessibleClick { .. } => Route::AccessibilityCenter,
        GhostStep::Learn { .. } | GhostStep::Forget { .. } | GhostStep::LearnedClick { .. } => {
            Route::LearningCenter
        }
    }
}

fn learning_subcenter(
    alias: &str,
    store_dir: Option<&Path>,
    teach: bool,
) -> (Route, Outcome, String) {
    let Some(dir) = store_dir else {
        return (
            Route::LearningCenter,
            Outcome::Blocked,
            "I can't access the Ghost learning store.".into(),
        );
    };
    let result = if teach {
        crate::ghost_learning::teach(dir, alias)
    } else {
        crate::ghost_learning::forget(dir, alias)
    };
    match result {
        Ok(detail) => (Route::LearningCenter, Outcome::Verified, detail),
        Err(detail) => (Route::LearningCenter, Outcome::Blocked, detail),
    }
}

fn learned_click_subcenter(
    alias: &str,
    store_dir: Option<&Path>,
    cancel: &AtomicBool,
) -> (Route, Outcome, String) {
    let Some(dir) = store_dir else {
        return (
            Route::LearningCenter,
            Outcome::Blocked,
            "I can't access the Ghost learning store.".into(),
        );
    };
    match crate::ghost_learning::resolve(dir, alias) {
        Ok((name, id)) => accessibility_subcenter_with_id(&name, Some(&id), cancel),
        Err(reason) => (Route::LearningCenter, Outcome::Blocked, reason),
    }
}

fn keyboard_subcenter(
    action_id: &str,
    arg: Option<u32>,
    cancel: &AtomicBool,
) -> (Route, Outcome, String) {
    if cancel.load(Ordering::Relaxed) {
        return (
            Route::KeyboardCenter,
            Outcome::Stopped,
            "The task was stopped.".into(),
        );
    }
    let Some(action) = Action::from_id(action_id) else {
        return (
            Route::KeyboardCenter,
            Outcome::Blocked,
            "Unknown keyboard action.".into(),
        );
    };
    if matches!(
        action,
        Action::Copy
            | Action::Paste
            | Action::Cut
            | Action::Undo
            | Action::Redo
            | Action::SelectAll
    ) && focused_field_is_secure()
    {
        return (
            Route::KeyboardCenter,
            Outcome::Blocked,
            "I can't safely edit a protected or unreadable field.".into(),
        );
    }
    if !crate::shortcuts::enabled() {
        return (
            Route::KeyboardCenter,
            Outcome::Blocked,
            "Keyboard routing is disabled.".into(),
        );
    }
    let out = crate::shortcuts::exec::do_action(action, arg);
    match &out {
        router::Outcome::Sent {
            verified: router::Verified::Yes,
            ..
        } => (
            Route::KeyboardCenter,
            Outcome::Verified,
            format!("{} completed", action.label()),
        ),
        router::Outcome::Sent { .. } => (
            Route::KeyboardCenter,
            Outcome::Unconfirmed,
            format!("The {} shortcut was sent once.", action.label()),
        ),
        router::Outcome::Aborted(router::Abort::Stopped) => (
            Route::KeyboardCenter,
            Outcome::Stopped,
            "The task was stopped.".into(),
        ),
        other => (
            Route::KeyboardCenter,
            Outcome::Blocked,
            router::reason(action, other).unwrap_or_else(|| "The shortcut was refused.".into()),
        ),
    }
}

#[cfg(target_os = "windows")]
fn focused_field_is_secure() -> bool {
    let Ok(automation) = uiautomation::UIAutomation::new() else {
        return true;
    };
    let Ok(focused) = automation.get_focused_element() else {
        return true;
    };
    focused.is_password().unwrap_or(true)
}

#[cfg(not(target_os = "windows"))]
fn focused_field_is_secure() -> bool {
    true
}

fn accessibility_subcenter(name: &str, cancel: &AtomicBool) -> (Route, Outcome, String) {
    accessibility_subcenter_with_id(name, None, cancel)
}

fn accessibility_subcenter_with_id(
    name: &str,
    expected_id: Option<&str>,
    cancel: &AtomicBool,
) -> (Route, Outcome, String) {
    #[cfg(target_os = "windows")]
    {
        match windows_accessibility::act(name, expected_id, cancel) {
            Ok((route, outcome, detail)) => (route, outcome, detail),
            Err(reason) => (Route::AccessibilityCenter, Outcome::Blocked, reason),
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = (name, expected_id, cancel);
        (
            Route::AccessibilityCenter,
            Outcome::Blocked,
            "Accessible Ghost control currently requires Windows.".into(),
        )
    }
}

#[cfg(target_os = "windows")]
mod windows_accessibility {
    use super::{Outcome, Route};
    use std::sync::atomic::{AtomicBool, Ordering};
    use uiautomation::controls::ControlType;
    use uiautomation::patterns::{UIInvokePattern, UISelectionItemPattern, UITogglePattern};
    use uiautomation::types::{Handle, PropertyConditionFlags, TreeScope, UIProperty};
    use uiautomation::variants::Variant;
    use uiautomation::UIAutomation;
    use windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow;

    pub fn act(
        name: &str,
        expected_id: Option<&str>,
        cancel: &AtomicBool,
    ) -> Result<(Route, Outcome, String), String> {
        let automation = UIAutomation::new().map_err(|e| e.to_string())?;
        let hwnd = unsafe { GetForegroundWindow() };
        if hwnd.0 == 0 {
            return Err("I can't identify the foreground window.".into());
        }
        let exe = crate::browser_url::foreground_process_name()
            .ok_or_else(|| "I can't identify the foreground app.".to_string())?;
        let title = crate::browser_url::foreground_window_title().unwrap_or_default();
        if crate::shortcuts::exec::is_protected_window(&exe, &title) {
            return Err("That window is protected.".into());
        }
        if crate::live::commands::window::window_process_elevated(hwnd)
            && !crate::live::commands::window::our_process_elevated()
        {
            return Err("That app runs as administrator.".into());
        }
        let root = automation
            .element_from_handle(Handle::from(hwnd.0))
            .map_err(|e| e.to_string())?;
        let cond = automation
            .create_property_condition(
                UIProperty::Name,
                Variant::from(name.to_string()),
                Some(PropertyConditionFlags::IgnoreCase),
            )
            .map_err(|e| e.to_string())?;
        let matches = root
            .find_all(TreeScope::Descendants, &cond)
            .map_err(|e| e.to_string())?;
        let eligible = matches
            .into_iter()
            .filter(|el| {
                el.get_name().is_ok_and(|n| n.eq_ignore_ascii_case(name))
                    && expected_id
                        .is_none_or(|id| el.get_automation_id().is_ok_and(|current| current == id))
                    && el.get_control_type().is_ok_and(|kind| {
                        matches!(
                            kind,
                            ControlType::Button
                                | ControlType::Hyperlink
                                | ControlType::TabItem
                                | ControlType::MenuItem
                                | ControlType::ListItem
                                | ControlType::CheckBox
                                | ControlType::RadioButton
                                | ControlType::SplitButton
                                | ControlType::ComboBox
                                | ControlType::Edit
                        )
                    })
                    && el.is_enabled().unwrap_or(false)
                    && !el.is_offscreen().unwrap_or(true)
                    && !el.is_password().unwrap_or(true)
            })
            .collect::<Vec<_>>();
        if eligible.len() != 1 {
            return Err(if eligible.is_empty() {
                format!("I couldn't find an accessible control named {name}.")
            } else {
                format!("More than one control is named {name}; please be more specific.")
            });
        }
        let el = &eligible[0];
        let same_window = || unsafe { GetForegroundWindow() }.0 == hwnd.0
            && crate::ghost::session_active() && !crate::ghost::stop_requested()
            && !cancel.load(Ordering::Relaxed);
        if !same_window() {
            return Err("The window changed before I could act.".into());
        }
        if let Ok(toggle) = el.get_pattern::<UITogglePattern>() {
            let before = toggle.get_toggle_state().map_err(|e| e.to_string())?;
            toggle.toggle().map_err(|e| e.to_string())?;
            let after = toggle.get_toggle_state().ok();
            return Ok((
                Route::AccessibilityCenter,
                if after.is_some_and(|state| state != before) {
                    Outcome::Verified
                } else {
                    Outcome::Unconfirmed
                },
                format!("Toggled {name}."),
            ));
        }
        if let Ok(selection) = el.get_pattern::<UISelectionItemPattern>() {
            selection.select().map_err(|e| e.to_string())?;
            return Ok((
                Route::AccessibilityCenter,
                if selection.is_selected().unwrap_or(false) {
                    Outcome::Verified
                } else {
                    Outcome::Unconfirmed
                },
                format!("Selected {name}."),
            ));
        }
        if let Ok(invoke) = el.get_pattern::<UIInvokePattern>() {
            invoke.invoke().map_err(|e| e.to_string())?;
            // Invoke returning success proves dispatch, not the app's effect.
            return Ok((
                Route::AccessibilityCenter,
                Outcome::Unconfirmed,
                format!("Activated {name}, but the app did not expose a verifiable result."),
            ));
        }
        let point = el
            .get_clickable_point()
            .map_err(|e| e.to_string())?
            .ok_or_else(|| format!("{name} has no safe clickable point."))?;
        let x = point.get_x();
        let y = point.get_y();
        let intended_id = el.get_runtime_id().map_err(|e| e.to_string())?;
        let checked = || {
            if !same_window() {
                return Err("The foreground window changed.".into());
            }
            let hit = automation
                .element_from_point(point)
                .map_err(|e| e.to_string())?;
            if hit.get_runtime_id().map_err(|e| e.to_string())? != intended_id {
                return Err("Another control covers the target point.".into());
            }
            Ok(())
        };
        checked()?;
        crate::live::commands::mouse::click_at_checked(x, y, || !same_window(), checked)?;
        Ok((
            Route::CursorCenter,
            Outcome::Unconfirmed,
            format!("Clicked {name}, but the app did not expose a verifiable result."),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_shortcuts_and_ordered_steps() {
        assert_eq!(
            parse("Nexus save then open a new tab"),
            Some(GhostTask {
                steps: vec![
                    GhostStep::Shortcut {
                        action: "save".into(),
                        arg: None
                    },
                    GhostStep::Shortcut {
                        action: "new_tab".into(),
                        arg: None
                    },
                ]
            })
        );
        assert_eq!(
            parse("switch to tab 3").unwrap().steps[0],
            GhostStep::Shortcut {
                action: "goto_tab_n".into(),
                arg: Some(3)
            }
        );
        assert_eq!(
            parse("Nexus, can you please click the Settings button")
                .unwrap()
                .steps[0],
            GhostStep::AccessibleClick {
                name: "settings".into()
            }
        );
    }

    #[test]
    fn parser_abstains_on_ambiguous_or_risky_text() {
        for text in [
            "I wish this was saved",
            "click the blue one",
            "click the blue button",
            "click Send",
            "click Delete",
            "save then maybe buy it",
            "tab 10",
        ] {
            assert!(parse(text).is_none(), "{text}");
        }
        assert!(!validate(&GhostTask {
            steps: vec![GhostStep::AccessibleClick {
                name: "Blue button".into()
            }],
        }));
    }

    #[test]
    fn learned_controls_use_explicit_alias_commands() {
        assert_eq!(
            parse("remember this button as settings").unwrap().steps[0],
            GhostStep::Learn {
                alias: "settings".into()
            }
        );
        assert_eq!(
            parse("click learned settings").unwrap().steps[0],
            GhostStep::LearnedClick {
                alias: "settings".into()
            }
        );
        assert_eq!(
            parse("forget learned settings").unwrap().steps[0],
            GhostStep::Forget {
                alias: "settings".into()
            }
        );
        assert!(parse("remember this button as send").is_none());
    }

    #[test]
    fn receipt_never_claims_unverified_success() {
        let r = Receipt {
            step: 0,
            route: Route::KeyboardCenter,
            outcome: Outcome::Unconfirmed,
            detail: "Save shortcut was sent.".into(),
            elapsed_ms: 1,
        };
        assert!(r.spoken().contains("couldn't verify"));
        assert!(!r.spoken().starts_with("Done"));
    }
}
