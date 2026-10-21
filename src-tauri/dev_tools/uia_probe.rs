//! UIA probe — measurement tool for the keyboard-first plan (docs/features/102, 103; research doc 09 §5).
//!
//! Answers, per foreground app, with numbers instead of assumptions:
//!   M1  how many actionable controls report a non-empty AcceleratorKey / AccessKey / support Invoke
//!   M2  how long today's `screen::list_actionables()` (10 sequential queries) takes
//!   M3  whether that query, rooted at the *focused element*, even sees the window's controls
//!   M4  how long ONE cached, window-rooted query takes (cold and warm) and how big the tree is
//!
//! Usage (focus the app you want to measure during the countdown):
//!   cargo run --example uia_probe -- --label notepad --delay 6 --out probe_notepad.json
//!   --repeat N   warm repeats of the cached query (default 3)
//!   --no-baseline  skip the legacy list_actionables() timing (it can take seconds on big trees)
//!
//! Privacy: window titles are never recorded; element names are written ONLY for controls that report a
//! shortcut/mnemonic (static UI labels such as "New tab"). Output stays on this machine.
//! Read-only: nothing is invoked, clicked or typed.

#[cfg(not(target_os = "windows"))]
fn main() {
    eprintln!("uia_probe is Windows-only");
}

#[cfg(target_os = "windows")]
fn main() {
    if let Err(e) = win::run() {
        eprintln!("uia_probe failed: {e}");
        std::process::exit(1);
    }
}

#[cfg(target_os = "windows")]
mod win {
    use std::collections::{BTreeMap, HashSet};
    use std::time::{Duration, Instant};

    use uiautomation::controls::ControlType;
    use uiautomation::patterns::{
        UIExpandCollapsePattern, UIInvokePattern, UIPatternType, UISelectionItemPattern, UITogglePattern,
    };
    use uiautomation::types::{Handle, TreeScope, UIProperty};
    use uiautomation::UIAutomation;

    /// Same set `screen.rs` treats as clickable, so the numbers are comparable.
    const CLICKABLE: &[ControlType] = &[
        ControlType::Button,
        ControlType::Hyperlink,
        ControlType::TabItem,
        ControlType::MenuItem,
        ControlType::ListItem,
        ControlType::CheckBox,
        ControlType::RadioButton,
        ControlType::SplitButton,
        ControlType::Edit,
        ControlType::ComboBox,
    ];

    struct Args {
        label: String,
        delay: u64,
        out: Option<String>,
        repeat: usize,
        baseline: bool,
        /// Refuse to measure unless the foreground process is this exe (case-insensitive).
        expect_exe: Option<String>,
    }

    fn parse_args() -> Args {
        let mut a = Args { label: "app".into(), delay: 5, out: None, repeat: 3, baseline: true, expect_exe: None };
        let v: Vec<String> = std::env::args().skip(1).collect();
        let mut i = 0;
        while i < v.len() {
            match v[i].as_str() {
                "--label" => { a.label = v.get(i + 1).cloned().unwrap_or_default(); i += 1; }
                "--delay" => { a.delay = v.get(i + 1).and_then(|s| s.parse().ok()).unwrap_or(5); i += 1; }
                "--out" => { a.out = v.get(i + 1).cloned(); i += 1; }
                "--repeat" => { a.repeat = v.get(i + 1).and_then(|s| s.parse().ok()).unwrap_or(3); i += 1; }
                "--no-baseline" => a.baseline = false,
                "--expect-exe" => { a.expect_exe = v.get(i + 1).cloned(); i += 1; }
                _ => {}
            }
            i += 1;
        }
        a
    }

    fn exe_for_pid(pid: u32) -> String {
        let out = std::process::Command::new("tasklist")
            .args(["/FI", &format!("PID eq {pid}"), "/FO", "CSV", "/NH"])
            .output();
        match out {
            Ok(o) => {
                let s = String::from_utf8_lossy(&o.stdout).to_string();
                s.split(',').next().unwrap_or("").trim_matches('"').to_string()
            }
            Err(_) => String::new(),
        }
    }

    fn ms(d: Duration) -> f64 {
        (d.as_secs_f64() * 1000.0 * 10.0).round() / 10.0
    }

    pub fn run() -> Result<(), String> {
        let args = parse_args();
        let e = |x: uiautomation::errors::Error| x.to_string();

        eprintln!(
            "uia_probe[{}]: focus the target app now — measuring in {}s (read-only, nothing is clicked)",
            args.label, args.delay
        );
        std::thread::sleep(Duration::from_secs(args.delay));

        let auto = UIAutomation::new().map_err(e)?;

        // Foreground window -> UIA element (window-rooted, NOT focused-element-rooted).
        let hwnd = unsafe { windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow() };
        if hwnd.0 == 0 {
            return Err("no foreground window".into());
        }
        let window = auto.element_from_handle(Handle::from(hwnd.0)).map_err(e)?;
        let pid = window.get_process_id().unwrap_or(0);
        let exe = exe_for_pid(pid);
        if let Some(want) = &args.expect_exe {
            if !exe.eq_ignore_ascii_case(want) {
                // Never read a window we were not asked to measure.
                return Err(format!("foreground is '{exe}', expected '{want}' - nothing measured"));
            }
        }
        let class = window.get_classname().unwrap_or_default();
        let framework = window.get_framework_id().unwrap_or_default();

        // ---- M3 context: what the legacy query is rooted at ----
        let true_cond = auto.create_true_condition().map_err(e)?;
        let focused = auto.get_focused_element().ok();
        let (focused_type, focused_children, focused_is_window) = match &focused {
            Some(f) => {
                let t = f.get_control_type().map(|c| format!("{c:?}")).unwrap_or_default();
                let n = f.find_all(TreeScope::Children, &true_cond).map(|v| v.len()).unwrap_or(0);
                let same = auto.compare_elements(f, &window).unwrap_or(false);
                (t, n, same)
            }
            None => (String::new(), 0, false),
        };

        // ---- M2: legacy approach (10 sequential find_all, rooted at the focused element) ----
        let mut baseline_ms = None;
        let mut baseline_count = None;
        let mut baseline_names: HashSet<String> = HashSet::new();
        // The shipped lookup (since 2026-10-08: window-rooted, one cached query). Always measured.
        let mut current_ms = Vec::new();
        let mut current_count = 0usize;
        let mut current_names: HashSet<String> = HashSet::new();
        for i in 0..=args.repeat {
            let t = Instant::now();
            let els = nexus_lib::screen::list_actionables();
            current_ms.push(ms(t.elapsed()));
            if i == 0 {
                current_count = els.len();
                for el in &els {
                    current_names.insert(el.name.to_lowercase());
                }
            }
        }
        if args.baseline {
            // The pre-fix lookup (10 sequential queries under the focused element), for comparison.
            let t = Instant::now();
            let els = nexus_lib::screen::list_actionables_legacy();
            baseline_ms = Some(ms(t.elapsed()));
            baseline_count = Some(els.len());
            for el in &els {
                baseline_names.insert(el.name.to_lowercase());
            }
        }

        // ---- M4: ONE cached query rooted at the window ----
        let cache = auto.create_cache_request().map_err(e)?;
        for p in [
            UIProperty::Name,
            UIProperty::ControlType,
            UIProperty::BoundingRectangle,
            UIProperty::IsEnabled,
            UIProperty::IsOffscreen,
            UIProperty::AutomationId,
            UIProperty::AcceleratorKey,
            UIProperty::AccessKey,
        ] {
            cache.add_property(p).map_err(e)?;
        }
        for p in [UIPatternType::Invoke, UIPatternType::Toggle, UIPatternType::ExpandCollapse, UIPatternType::SelectionItem] {
            cache.add_pattern(p).map_err(e)?;
        }

        let mut cold_ms = 0.0;
        let mut warm: Vec<f64> = Vec::new();
        let mut elements = Vec::new();
        for i in 0..=args.repeat {
            let t = Instant::now();
            let found = window
                .find_all_build_cache(TreeScope::Descendants, &true_cond, &cache)
                .map_err(e)?;
            let dt = ms(t.elapsed());
            if i == 0 {
                cold_ms = dt;
                elements = found;
            } else {
                warm.push(dt);
            }
        }
        let warm_min = warm.iter().cloned().fold(f64::INFINITY, f64::min);

        // ---- Read cached properties (no further cross-process calls) ----
        let t_read = Instant::now();
        let mut by_type: BTreeMap<String, usize> = BTreeMap::new();
        let mut actionable = 0usize;
        let (mut with_accel, mut with_access, mut with_invoke, mut with_toggle, mut with_expand, mut with_select) =
            (0usize, 0usize, 0usize, 0usize, 0usize, 0usize);
        let mut with_any_keyboard_or_pattern = 0usize;
        let mut actionable_names: HashSet<String> = HashSet::new();
        let mut examples: Vec<serde_json::Value> = Vec::new();
        let mut accel_strings: BTreeMap<String, usize> = BTreeMap::new();
        // Shortcut embedded in the control's NAME, e.g. "Bold (Ctrl+B)", "Refresh (F5)", "Up (Alt + Up Arrow)".
        let name_shortcut = regex::Regex::new(r"\((?:Ctrl|Alt|Shift|Win|Cmd)[^)]*\)|\(F\d{1,2}\)").unwrap();
        let mut with_name_shortcut = 0usize;
        let mut name_shortcut_examples: Vec<String> = Vec::new();

        for el in &elements {
            let ct = match el.get_cached_control_type() {
                Ok(c) => c,
                Err(_) => continue,
            };
            *by_type.entry(format!("{ct:?}")).or_insert(0) += 1;
            if !CLICKABLE.contains(&ct) {
                continue;
            }
            let enabled = el.is_cached_enabled().unwrap_or(false);
            let offscreen = el.is_cached_offscreen().unwrap_or(true);
            let name = el.get_cached_name().unwrap_or_default();
            let rect_ok = el
                .get_cached_bounding_rectangle()
                .map(|r| r.get_width() > 0 && r.get_height() > 0)
                .unwrap_or(false);
            if !enabled || offscreen || !rect_ok || name.trim().is_empty() {
                continue;
            }
            actionable += 1;
            actionable_names.insert(name.to_lowercase());

            let accel = el.get_cached_accelerator_key().unwrap_or_default();
            let access = el.get_cached_access_key().unwrap_or_default();
            let invoke = el.get_cached_pattern::<UIInvokePattern>().is_ok();
            let toggle = el.get_cached_pattern::<UITogglePattern>().is_ok();
            let expand = el.get_cached_pattern::<UIExpandCollapsePattern>().is_ok();
            let select = el.get_cached_pattern::<UISelectionItemPattern>().is_ok();

            if name_shortcut.is_match(&name) {
                with_name_shortcut += 1;
                if name_shortcut_examples.len() < 20 {
                    name_shortcut_examples.push(name.clone());
                }
            }
            if !accel.trim().is_empty() { with_accel += 1; *accel_strings.entry(accel.trim().to_string()).or_insert(0) += 1; }
            if !access.trim().is_empty() { with_access += 1; }
            if invoke { with_invoke += 1; }
            if toggle { with_toggle += 1; }
            if expand { with_expand += 1; }
            if select { with_select += 1; }
            if !accel.trim().is_empty() || !access.trim().is_empty() || invoke || toggle || expand || select || name_shortcut.is_match(&name) {
                with_any_keyboard_or_pattern += 1;
            }
            // Names only for controls that report a shortcut/mnemonic (static labels).
            if (!accel.trim().is_empty() || !access.trim().is_empty()) && examples.len() < 40 {
                examples.push(serde_json::json!({
                    "type": format!("{ct:?}"), "name": name, "accelerator": accel, "access": access, "invoke": invoke
                }));
            }
        }
        let read_ms = ms(t_read.elapsed());

        let baseline_recall = if args.baseline && !actionable_names.is_empty() {
            let hit = actionable_names.iter().filter(|n| baseline_names.contains(*n)).count();
            Some((hit as f64 / actionable_names.len() as f64 * 1000.0).round() / 10.0)
        } else {
            None
        };
        let pct = |n: usize| -> f64 {
            if actionable == 0 { 0.0 } else { (n as f64 / actionable as f64 * 1000.0).round() / 10.0 }
        };

        let current_recall = if !actionable_names.is_empty() {
            let hit = actionable_names.iter().filter(|n| current_names.contains(*n)).count();
            Some((hit as f64 / actionable_names.len() as f64 * 1000.0).round() / 10.0)
        } else {
            None
        };
        let foreground_stable = unsafe { windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow() }.0 == hwnd.0;
        let report = serde_json::json!({
            "foreground_stable": foreground_stable,
            "label": args.label,
            "process": { "pid": pid, "exe": exe, "class": class, "framework": framework },
            "tree": { "elements_total": elements.len(), "by_control_type": by_type },
            "current_list_actionables": {
                "first_ms": current_ms.first().cloned(), "later_ms": current_ms.iter().skip(1).cloned().collect::<Vec<_>>(),
                "count": current_count, "recall_vs_window_rooted_pct": current_recall
            },
            "legacy_list_actionables": {
                "ms": baseline_ms, "count": baseline_count,
                "rooted_at": { "focused_control_type": focused_type, "focused_children": focused_children, "focused_is_window": focused_is_window },
                "recall_vs_window_rooted_pct": baseline_recall
            },
            "window_rooted_cached": {
                "cold_ms": cold_ms, "warm_min_ms": if warm.is_empty() { serde_json::Value::Null } else { serde_json::json!(warm_min) },
                "warm_runs_ms": warm, "read_cached_props_ms": read_ms
            },
            "actionable": {
                "count": actionable,
                "with_accelerator_key": with_accel, "with_accelerator_pct": pct(with_accel),
                "with_access_key": with_access, "with_access_pct": pct(with_access),
                "with_invoke": with_invoke, "with_invoke_pct": pct(with_invoke),
                "with_shortcut_in_name": with_name_shortcut, "with_shortcut_in_name_pct": pct(with_name_shortcut),
                "with_toggle": with_toggle, "with_expand_collapse": with_expand, "with_selection_item": with_select,
                "with_any_keyboard_or_pattern": with_any_keyboard_or_pattern, "with_any_pct": pct(with_any_keyboard_or_pattern),
                "distinct_accelerator_strings": accel_strings
            },
            "examples_with_shortcut": examples,
            "examples_shortcut_in_name": name_shortcut_examples
        });

        let text = serde_json::to_string_pretty(&report).map_err(|x| x.to_string())?;
        match &args.out {
            Some(p) => {
                std::fs::write(p, &text).map_err(|x| x.to_string())?;
                eprintln!("uia_probe: wrote {p}");
            }
            None => println!("{text}"),
        }
        eprintln!(
            "uia_probe[{}] exe={} tree={} actionable={} | accel {:.1}% access {:.1}% invoke {:.1}% any {:.1}% | legacy {:?}ms ({:?} found) vs CURRENT {:?}ms ({} found, recall {:?}%) | raw cached cold {}ms warm {:.1}ms",
            args.label, exe, elements.len(), actionable, pct(with_accel), pct(with_access), pct(with_invoke),
            pct(with_any_keyboard_or_pattern), baseline_ms, baseline_count, current_ms, current_count, current_recall,
            cold_ms, if warm.is_empty() { f64::NAN } else { warm_min }
        );
        Ok(())
    }
}
