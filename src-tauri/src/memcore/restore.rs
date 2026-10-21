//! Reopen what was open before the power-off (plan D), after the user said yes.
//!
//! `plan` is pure: it turns a snapshot plus the set of processes that are
//! already running into an ordered, capped list of things to open, and says
//! what it skipped. `run` is the thin launcher (staggered, never blocks the
//! UI thread). Nothing here runs without a spoken "yes".

use std::collections::HashSet;

use super::session::{SessionApp, Snapshot};

/// Reopen at most this many things in one go.
pub const MAX_ITEMS: usize = 8;
const STAGGER_MS: u64 = 400;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Item {
    /// A normal program with (sanitised) arguments.
    Exe { path: String, args: Vec<String>, label: String },
    /// A Store app, started by name through the app registry.
    Packaged { label: String },
    /// A browser, asked to restore its last session.
    Browser { path: String, label: String },
    Folder { path: String },
}

impl Item {
    pub fn label(&self) -> String {
        match self {
            Item::Exe { label, .. } | Item::Packaged { label } | Item::Browser { label, .. } => label.clone(),
            Item::Folder { path } => path.rsplit(['\\', '/']).next().unwrap_or(path).to_string(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Plan {
    pub items: Vec<Item>,
    /// Already running, so left alone ("Brave").
    pub already_open: Vec<String>,
    /// Beyond the cap or without a usable path.
    pub skipped: usize,
}

impl Plan {
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}

fn item_for(a: &SessionApp, path_ok: &dyn Fn(&str) -> bool) -> Option<Item> {
    if a.packaged {
        return Some(Item::Packaged { label: a.label.clone() });
    }
    if a.path.is_empty() || !path_ok(&a.path) {
        return None;
    }
    if a.browser {
        return Some(Item::Browser { path: a.path.clone(), label: a.label.clone() });
    }
    Some(Item::Exe { path: a.path.clone(), args: a.args.clone(), label: a.label.clone() })
}

/// Build the restore plan. `running` holds lower-case exe names. Pure.
pub fn plan(s: &Snapshot, running: &HashSet<String>, path_ok: &dyn Fn(&str) -> bool) -> Plan {
    let mut p = Plan::default();
    let mut counted_running: HashSet<String> = HashSet::new();
    let mut candidates: Vec<Item> = vec![];

    // Browsers first (they take longest to load), then the rest, then folders.
    let mut apps: Vec<&SessionApp> = s.apps.iter().collect();
    apps.sort_by_key(|a| if a.browser { 0 } else { 1 });
    for a in apps {
        if running.contains(&a.exe.to_lowercase()) {
            if counted_running.insert(a.exe.to_lowercase()) {
                p.already_open.push(a.label.split(" on ").next().unwrap_or(&a.label).to_string());
            }
            continue;
        }
        match item_for(a, path_ok) {
            Some(i) => candidates.push(i),
            None => p.skipped += 1,
        }
    }
    for f in &s.folders {
        if path_ok(f) {
            candidates.push(Item::Folder { path: f.clone() });
        }
    }
    if candidates.len() > MAX_ITEMS {
        p.skipped += candidates.len() - MAX_ITEMS;
        candidates.truncate(MAX_ITEMS);
    }
    p.items = candidates;
    p
}

/// What a plan will open, as speech ("VS Code on ULTRON, Brave and 2 folders"). Pure.
pub fn describe(items: &[Item]) -> String {
    let names: Vec<String> = items.iter().filter(|i| !matches!(i, Item::Folder { .. })).map(Item::label).collect();
    let folders = items.iter().filter(|i| matches!(i, Item::Folder { .. })).count();
    super::session::names_phrase(names, folders)
}

/// "Reopened VS Code on ULTRON, Brave and 2 folders. Notepad was already open." Pure.
pub fn report(opened: &[String], failed: &[String], already_open: &[String], skipped: usize) -> String {
    let mut out = String::new();
    if opened.is_empty() {
        out.push_str("I couldn't reopen anything");
        if !already_open.is_empty() {
            out.push_str(", sir.");
        } else {
            out.push('.');
        }
    } else {
        out.push_str(&format!("Reopened {}.", join_names(opened)));
    }
    if !failed.is_empty() {
        out.push_str(&format!(" {} would not start.", join_names(failed)));
    }
    if !already_open.is_empty() {
        let verb = if already_open.len() == 1 { "was" } else { "were" };
        out.push_str(&format!(" {} {verb} already open.", join_names(already_open)));
    }
    if skipped > 0 {
        out.push_str(&format!(" I left {skipped} out."));
    }
    out.trim().to_string()
}

fn join_names(items: &[String]) -> String {
    let mut seen = HashSet::new();
    let uniq: Vec<&String> = items.iter().filter(|i| seen.insert(i.to_lowercase())).collect();
    match uniq.len() {
        0 => String::new(),
        1 => uniq[0].clone(),
        n => format!(
            "{} and {}",
            uniq[..n - 1].iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", "),
            uniq[n - 1]
        ),
    }
}

/// Names of the programs currently running (lower-case file names).
pub fn running_exes() -> HashSet<String> {
    use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System};
    let mut sys = System::new();
    sys.refresh_processes_specifics(ProcessesToUpdate::All, true, ProcessRefreshKind::new());
    sys.processes().values().map(|p| p.name().to_string_lossy().to_lowercase()).collect()
}

/// Launch one item. Blocking but quick (spawn only).
fn launch(item: &Item) -> Result<(), String> {
    match item {
        Item::Exe { path, args, .. } => {
            std::process::Command::new(path).args(args).spawn().map(|_| ()).map_err(|e| e.to_string())
        }
        Item::Browser { path, .. } => std::process::Command::new(path)
            .arg("--restore-last-session")
            .spawn()
            .map(|_| ())
            .map_err(|e| e.to_string()),
        Item::Packaged { label } => {
            let entry = crate::app_registry::lookup(label).ok_or_else(|| format!("{label} not found"))?;
            crate::app_registry::launch(&entry)
        }
        Item::Folder { path } => open::that(path).map_err(|e| e.to_string()),
    }
}

/// Open everything in `plan`, one item every 400 ms. Returns (opened, failed) labels.
pub async fn run(items: Vec<Item>) -> (Vec<String>, Vec<String>) {
    let mut opened = vec![];
    let mut failed = vec![];
    for it in items {
        let label = it.label();
        let it2 = it.clone();
        match tauri::async_runtime::spawn_blocking(move || launch(&it2)).await {
            Ok(Ok(())) => {
                println!("[SESSION] reopened {label}");
                opened.push(label);
            }
            other => {
                println!("[SESSION] could not reopen {label}: {other:?}");
                failed.push(label);
            }
        }
        tokio::time::sleep(std::time::Duration::from_millis(STAGGER_MS)).await;
    }
    (opened, failed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memcore::session::{Snapshot, REASON_PERIODIC};

    fn app(exe: &str, label: &str) -> SessionApp {
        SessionApp {
            exe: exe.into(),
            path: format!(r"C:\apps\{exe}"),
            args: vec![],
            label: label.into(),
            windows: 1,
            packaged: false,
            browser: false,
        }
    }

    fn snap(apps: Vec<SessionApp>, folders: &[&str]) -> Snapshot {
        Snapshot { ts: 1, reason: REASON_PERIODIC.into(), apps, folders: folders.iter().map(|s| s.to_string()).collect() }
    }

    fn running(names: &[&str]) -> HashSet<String> {
        names.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn running_programs_are_left_alone_and_reported() {
        let s = snap(vec![app("code.exe", "VS Code on ULTRON"), app("notepad.exe", "Notepad")], &[]);
        let p = plan(&s, &running(&["code.exe"]), &|_| true);
        assert_eq!(p.items.len(), 1);
        assert_eq!(p.already_open, vec!["VS Code".to_string()]);
    }

    #[test]
    fn browsers_go_first_and_use_the_restore_flag_item() {
        let mut b = app("brave.exe", "Brave");
        b.browser = true;
        let s = snap(vec![app("notepad.exe", "Notepad"), b], &[r"C:\Docs"]);
        let p = plan(&s, &running(&[]), &|_| true);
        assert!(matches!(p.items[0], Item::Browser { .. }), "{:?}", p.items);
        assert!(matches!(p.items.last(), Some(Item::Folder { .. })), "folders last");
    }

    #[test]
    fn at_most_eight_items_and_missing_paths_are_skipped() {
        let apps: Vec<SessionApp> = (0..10).map(|i| app(&format!("a{i}.exe"), &format!("A{i}"))).collect();
        let p = plan(&snap(apps, &[]), &running(&[]), &|_| true);
        assert_eq!(p.items.len(), MAX_ITEMS);
        assert_eq!(p.skipped, 2);
        let p2 = plan(&snap(vec![app("gone.exe", "Gone")], &[]), &running(&[]), &|path| !path.contains("gone"));
        assert!(p2.is_empty() && p2.skipped == 1);
    }

    #[test]
    fn store_apps_start_by_name() {
        let mut w = app("whatsapp.root.exe", "WhatsApp");
        w.packaged = true;
        w.path = String::new();
        let p = plan(&snap(vec![w], &[]), &running(&[]), &|_| false);
        assert_eq!(p.items, vec![Item::Packaged { label: "WhatsApp".into() }]);
    }

    #[test]
    fn report_wording() {
        assert_eq!(
            report(&["VS Code".into(), "Brave".into()], &[], &["Notepad".into()], 0),
            "Reopened VS Code and Brave. Notepad was already open."
        );
        assert_eq!(report(&[], &[], &["Brave".into(), "Code".into()], 0), "I couldn't reopen anything, sir. Brave and Code were already open.");
        assert_eq!(report(&["A".into()], &["B".into()], &[], 2), "Reopened A. B would not start. I left 2 out.");
    }
}
