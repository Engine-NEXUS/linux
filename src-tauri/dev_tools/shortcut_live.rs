//! Live harness for the shortcut brain (phase E1): runs ONE action through the real Windows adapter
//! (`WinEnv`: foreground probe + batched SendInput + title-change verification) and prints the outcome.
//!
//!   cargo run --example shortcut_live -- --expect-exe notepad.exe --action new_tab [--arg 2] [--delay 1]
//!
//! SAFETY: nothing is sent unless the foreground executable is exactly `--expect-exe` (exit code 3 and
//! nothing sent otherwise), so a stray window cannot receive keys from a test. The router's own pre-send
//! foreground re-check still applies on top. Use harmless actions on apps you launched for the test.

#[cfg(not(target_os = "windows"))]
fn main() {
    eprintln!("shortcut_live is Windows-only");
}

#[cfg(target_os = "windows")]
fn main() {
    use nexus_lib::shortcuts::router::Env;
    use nexus_lib::shortcuts::{do_action, exec::WinEnv, Action, Outcome};

    let v: Vec<String> = std::env::args().skip(1).collect();
    let get = |name: &str| v.iter().position(|a| a == name).and_then(|i| v.get(i + 1)).cloned();
    let Some(expect) = get("--expect-exe") else {
        eprintln!("--expect-exe <name.exe> is required (safety)");
        std::process::exit(2);
    };
    let Some(action_id) = get("--action") else {
        eprintln!("--action <id> is required");
        std::process::exit(2);
    };
    let arg = get("--arg").and_then(|s| s.parse::<u32>().ok());
    let delay = get("--delay").and_then(|s| s.parse::<u64>().ok()).unwrap_or(1);
    let Some(action) = Action::from_id(&action_id) else {
        eprintln!("unknown action '{action_id}'");
        std::process::exit(2);
    };

    std::thread::sleep(std::time::Duration::from_secs(delay));
    match WinEnv.foreground() {
        Some(f) if f.exe.eq_ignore_ascii_case(&expect) => {}
        other => {
            eprintln!(
                "foreground is {:?}, expected '{expect}' - NOTHING SENT",
                other.map(|f| f.exe)
            );
            std::process::exit(3);
        }
    }

    let t = std::time::Instant::now();
    let out = do_action(action, arg);
    let wall = t.elapsed().as_millis();
    let line = match &out {
        Outcome::Sent { chord, scope, verified, ms } => format!(
            "SENT chord={chord} scope={} verified={verified:?} router_ms={ms} wall_ms={wall}",
            scope.describe()
        ),
        other => format!("NOT_SENT {other:?} wall_ms={wall}"),
    };
    println!("{line}");
}
