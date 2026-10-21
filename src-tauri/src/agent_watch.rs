//! Agent-completion watcher — lazy Node.js sidecar manager.
//!
//! Watches an external coding-agent CLI (Claude Code, Gemini CLI, Codex,
//! Aider, Hermes Agent, opencode — and any future tool via its own small
//! adapter) and tells the user when it finishes, using only local process
//! output pattern-matching. Zero network calls, zero API cost, works
//! offline.
//!
//! The actual spawn-and-watch logic lives in the maintained, MIT-licensed
//! `pty-manager` + `coding-agent-adapters` npm packages
//! (resources/agent_watch_sidecar/), run as a lazily-started local sidecar
//! — exactly like `lazy_stt.rs` manages the Python STT server. This module
//! is the Rust-side process manager + HTTP client + keep-awake/notify glue;
//! it holds no tool-specific knowledge at all. Adding a tool later means
//! adding an adapter inside the sidecar's own node_modules dependency tree
//! (or a small custom adapter file next to `server.cjs`), never touching
//! this file.

use std::io::BufRead;
use std::process::{Child, Command};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use tauri::Emitter;

use crate::google::types::AlertUrgency;
use crate::keep_awake::KeepAwakeLease;

const AGENT_WATCH_PORT: u16 = 39222;
const SIDECAR_IDLE_TIMEOUT: Duration = Duration::from_secs(10 * 60);
/// How often Rust polls the sidecar for a given watch's status.
const POLL_INTERVAL: Duration = Duration::from_secs(3);
/// Safety cap so a watch can never leak a keep-awake lease forever if the
/// underlying agent truly never signals completion (e.g. it crashed in a
/// way the adapter doesn't recognize). This is generous on purpose.
const MAX_WATCH_DURATION: Duration = Duration::from_secs(6 * 60 * 60);

static SIDECAR_CHILD: Mutex<Option<Child>> = Mutex::new(None);
static SIDECAR_RUNNING: AtomicBool = AtomicBool::new(false);
static SIDECAR_STARTING: AtomicBool = AtomicBool::new(false);
static LAST_ACTIVITY: Mutex<Option<Instant>> = Mutex::new(None);

/// Active watches, keyed by the sidecar-issued watch id, holding the
/// keep-awake lease for as long as the watch is in flight.
static ACTIVE_WATCHES: Mutex<Vec<(String, Option<KeepAwakeLease>)>> = Mutex::new(Vec::new());

/// The six tools covered out of the box by `coding-agent-adapters`. Kept
/// here only as a UI fallback if the sidecar can't be reached yet (e.g.
/// Settings opened before any watch has ever started it) — the sidecar's
/// own `/adapters` response is the source of truth once it's up.
pub const KNOWN_ADAPTERS: &[&str] = &[
    "claude", "gemini", "codex", "aider", "hermes", "opencode",
    "antigravity", "agy", "cursor", "trae", "windsurf", "vscode", "terminal", "generic",
];

fn sidecar_dir() -> Option<std::path::PathBuf> {
    let candidates = [
        // Dev: .../src-tauri/target/release/../../../resources/agent_watch_sidecar
        std::env::current_exe()
            .ok()?
            .parent()? // target/release
            .parent()? // target
            .parent()? // src-tauri
            .join("resources")
            .join("agent_watch_sidecar"),
        // Production: <app_dir>/resources/agent_watch_sidecar
        std::env::current_exe()
            .ok()?
            .parent()?
            .join("resources")
            .join("agent_watch_sidecar"),
    ];
    for path in &candidates {
        if path.join("server.cjs").exists() {
            return Some(path.clone());
        }
    }
    None
}

/// Find a working Node.js interpreter on PATH. Node is not bundled with
/// NEXUS (unlike the Python STT sidecar's own `find_python`, there is no
/// Windows-registry fallback here yet) — a user running coding-agent CLIs
/// almost certainly already has Node installed for those tools themselves,
/// but this is a real, known gap for a user who somehow has agent CLIs
/// without Node. Reported honestly via `sidecar_unavailable_reason()`.
fn find_node() -> Option<String> {
    for cmd in &["node", "node.exe"] {
        if let Ok(output) = Command::new(cmd).arg("--version").output() {
            if output.status.success() {
                return Some(cmd.to_string());
            }
        }
    }
    None
}

/// Human-readable reason the feature is unavailable, or `None` if it's
/// ready to use. Checked by the frontend before showing the watch UI.
pub fn sidecar_unavailable_reason() -> Option<String> {
    if sidecar_dir().is_none() {
        return Some("agent-watch sidecar files not found".to_string());
    }
    if find_node().is_none() {
        return Some(
            "Node.js not found on PATH — install Node.js to use agent-completion watching"
                .to_string(),
        );
    }
    None
}

/// Blocking raw-socket health check — callable from any thread, no tokio
/// runtime required. Mirrors `lazy_stt.rs::is_stt_responsive`.
fn is_sidecar_responsive() -> bool {
    use std::io::{Read, Write};
    use std::net::TcpStream;

    let addr = format!("127.0.0.1:{AGENT_WATCH_PORT}");
    let timeout = Duration::from_millis(500);
    let Ok(parsed) = addr.parse() else { return false };
    match TcpStream::connect_timeout(&parsed, timeout) {
        Ok(mut stream) => {
            stream.set_read_timeout(Some(timeout)).ok();
            stream.set_write_timeout(Some(timeout)).ok();
            let request = "GET /health HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n";
            if stream.write_all(request.as_bytes()).is_err() {
                return false;
            }
            let mut response = Vec::new();
            if stream.read_to_end(&mut response).is_err() {
                return false;
            }
            let response_str = String::from_utf8_lossy(&response);
            response_str.contains("200") && response_str.contains("\"ok\":true")
        }
        Err(_) => false,
    }
}

/// Install the sidecar's npm dependencies (`pty-manager` +
/// `coding-agent-adapters`, ~68 MB incl. node-pty's native prebuilt
/// binary) the first time this feature is used. Not bundled in the
/// installer — mirrors how Kokoro's model/voice files are downloaded on
/// first use rather than shipped, to keep the installer small for the
/// large majority of users who never touch this feature. Blocking; call
/// via `spawn_blocking` from async code.
fn ensure_deps_installed(dir: &std::path::Path) -> Result<(), String> {
    if dir.join("node_modules").join("pty-manager").exists() {
        return Ok(());
    }
    tracing::info!("agent_watch: installing sidecar dependencies (one-time, ~68 MB)...");
    let npm_cmd = if cfg!(windows) { "npm.cmd" } else { "npm" };
    let mut cmd = Command::new(npm_cmd);
    cmd.arg("install")
        .arg("--no-audit")
        .arg("--no-fund")
        .current_dir(dir);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    let output = cmd
        .output()
        .map_err(|e| format!("could not run npm install: {e} (is Node.js/npm installed?)"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("npm install failed: {stderr}"));
    }
    tracing::info!("agent_watch: sidecar dependencies installed");
    Ok(())
}

/// Start the sidecar if it's not already running. Safe to call repeatedly;
/// a compare-and-swap guard prevents concurrent spawns, exactly like
/// `lazy_stt.rs::ensure_stt_running`.
fn ensure_sidecar_running() {
    if SIDECAR_RUNNING.load(Ordering::Relaxed) {
        return;
    }
    if SIDECAR_STARTING
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return;
    }

    if is_sidecar_responsive() {
        SIDECAR_RUNNING.store(true, Ordering::Relaxed);
        SIDECAR_STARTING.store(false, Ordering::SeqCst);
        *LAST_ACTIVITY.lock().unwrap() = Some(Instant::now());
        return;
    }

    let Some(dir) = sidecar_dir() else {
        SIDECAR_STARTING.store(false, Ordering::SeqCst);
        tracing::warn!("agent_watch: sidecar files not found, skipping");
        return;
    };
    let Some(node_cmd) = find_node() else {
        SIDECAR_STARTING.store(false, Ordering::SeqCst);
        tracing::warn!("agent_watch: Node.js not found on PATH, skipping");
        return;
    };

    let mut cmd = Command::new(&node_cmd);
    cmd.arg("server.cjs")
        .current_dir(&dir)
        .env("AGENT_WATCH_PORT", AGENT_WATCH_PORT.to_string())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }

    match cmd.spawn() {
        Ok(mut child) => {
            let pid = child.id();
            if let Some(out) = child.stdout.take() {
                spawn_drain("OUT", out);
            }
            if let Some(err) = child.stderr.take() {
                spawn_drain("ERR", err);
            }
            *SIDECAR_CHILD.lock().unwrap() = Some(child);
            SIDECAR_RUNNING.store(true, Ordering::Relaxed);
            SIDECAR_STARTING.store(false, Ordering::SeqCst);
            *LAST_ACTIVITY.lock().unwrap() = Some(Instant::now());
            tracing::info!("agent_watch: sidecar started (PID {pid})");

            std::thread::spawn(move || {
                for _ in 0..30 {
                    std::thread::sleep(Duration::from_millis(500));
                    if is_sidecar_responsive() {
                        tracing::info!("agent_watch: sidecar ready");
                        return;
                    }
                }
                tracing::warn!("agent_watch: sidecar did not become ready in 15s");
            });
        }
        Err(e) => {
            SIDECAR_STARTING.store(false, Ordering::SeqCst);
            tracing::error!("agent_watch: failed to start sidecar: {e}");
        }
    }
}

fn spawn_drain<R: std::io::Read + Send + 'static>(label: &'static str, pipe: R) {
    std::thread::Builder::new()
        .name(format!("agent-watch-{label}"))
        .spawn(move || {
            for line in std::io::BufReader::new(pipe).lines().map_while(Result::ok) {
                tracing::info!("[AGENT-WATCH-{label}] {line}");
            }
        })
        .ok();
}

fn mark_activity() {
    *LAST_ACTIVITY.lock().unwrap() = Some(Instant::now());
}

/// Kill the sidecar if nothing has watched anything for a while. Called
/// periodically from a background thread, mirroring `lazy_stt.rs`'s idle
/// monitor shape (this feature's idle RAM cost is small — a bare Node
/// process with no PTY children — but the convention is kept for parity
/// and because a PTY session that somehow outlives its watch record should
/// still be reaped by killing the whole sidecar).
pub fn check_sidecar_idle() {
    if !SIDECAR_RUNNING.load(Ordering::Relaxed) {
        return;
    }
    if !ACTIVE_WATCHES.lock().unwrap().is_empty() {
        mark_activity();
        return;
    }
    let should_kill = {
        let last = LAST_ACTIVITY.lock().unwrap();
        matches!(*last, Some(t) if Instant::now().duration_since(t) > SIDECAR_IDLE_TIMEOUT)
    };
    if should_kill {
        tracing::info!("agent_watch: sidecar idle, stopping");
        let mut guard = SIDECAR_CHILD.lock().unwrap();
        if let Some(mut child) = guard.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
        SIDECAR_RUNNING.store(false, Ordering::Relaxed);
        *LAST_ACTIVITY.lock().unwrap() = None;
    }
}

pub fn start_idle_monitor() {
    std::thread::spawn(|| loop {
        std::thread::sleep(Duration::from_secs(30));
        check_sidecar_idle();
    });
}

fn sidecar_url(path: &str) -> String {
    format!("http://127.0.0.1:{AGENT_WATCH_PORT}{path}")
}

/// List adapters known to the sidecar (falls back to `KNOWN_ADAPTERS` if
/// the sidecar isn't up yet — starting it just to answer this would add
/// needless latency to opening a settings panel).
pub async fn list_adapters() -> Vec<String> {
    if !is_sidecar_responsive() {
        return KNOWN_ADAPTERS.iter().map(|s| s.to_string()).collect();
    }
    let client = reqwest::Client::new();
    match client.get(sidecar_url("/adapters")).send().await {
        Ok(resp) => match resp.json::<serde_json::Value>().await {
            Ok(json) => json
                .get("adapters")
                .and_then(|v| v.as_array())
                .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect())
                .unwrap_or_else(|| KNOWN_ADAPTERS.iter().map(|s| s.to_string()).collect()),
            Err(_) => KNOWN_ADAPTERS.iter().map(|s| s.to_string()).collect(),
        },
        Err(_) => KNOWN_ADAPTERS.iter().map(|s| s.to_string()).collect(),
    }
}

/// Start watching an agent. `adapter_type` must be one returned by
/// `list_adapters()`. `prompt`, if given, is sent as the task once the
/// session is up (works for both the adapters' non-interactive automation
/// mode and their interactive mode — either way it's the one line the
/// caller asked the agent to do). Returns the sidecar-issued watch id.
pub async fn start_watch<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    adapter_type: String,
    workdir: Option<String>,
    prompt: Option<String>,
) -> Result<String, String> {
    if let Some(reason) = sidecar_unavailable_reason() {
        return Err(reason);
    }
    let Some(dir) = sidecar_dir() else {
        return Err("agent-watch sidecar files not found".to_string());
    };
    tokio::task::spawn_blocking(move || ensure_deps_installed(&dir))
        .await
        .map_err(|e| format!("install task panicked: {e}"))??;
    ensure_sidecar_running();
    for _ in 0..30 {
        if is_sidecar_responsive() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    if !is_sidecar_responsive() {
        return Err("agent-watch sidecar did not start in time".to_string());
    }

    let client = reqwest::Client::new();
    let body = serde_json::json!({
        "type": adapter_type,
        "workdir": workdir,
        "prompt": prompt,
    });
    let resp = client
        .post(sidecar_url("/watch"))
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("failed to reach agent-watch sidecar: {e}"))?;
    let json: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("bad response from agent-watch sidecar: {e}"))?;
    let id = json
        .get("id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "agent-watch sidecar returned no watch id".to_string())?
        .to_string();

    mark_activity();
    let lease = crate::keep_awake::acquire_lease();
    ACTIVE_WATCHES.lock().unwrap().push((id.clone(), lease));

    let poll_id = id.clone();
    let poll_adapter = adapter_type.clone();
    tauri::async_runtime::spawn(async move {
        poll_watch_until_done(app, poll_id, poll_adapter).await;
    });

    Ok(id)
}

async fn poll_watch_until_done<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    id: String,
    adapter_type: String,
) {
    let client = reqwest::Client::new();
    let started = Instant::now();
    loop {
        tokio::time::sleep(POLL_INTERVAL).await;
        mark_activity();

        if started.elapsed() > MAX_WATCH_DURATION {
            tracing::warn!("agent_watch: watch {id} exceeded max duration, giving up on it");
            break;
        }

        let Ok(resp) = client.get(sidecar_url(&format!("/watch/{id}"))).send().await else {
            continue; // sidecar hiccup — keep polling, the lease stays held
        };
        let Ok(json) = resp.json::<serde_json::Value>().await else {
            continue;
        };
        let status = json.get("status").and_then(|v| v.as_str()).unwrap_or("working");

        match status {
            "done" => {
                crate::proactive_policy::submit(
                    &app,
                    format!("agent_watch_done_{id}"),
                    format!("Your {adapter_type} task just finished, sir."),
                    AlertUrgency::High,
                );
                let _ = app.emit("agent_watch:done", &json);
                break;
            }
            "error" => {
                let err = json.get("error").and_then(|v| v.as_str()).unwrap_or("unknown error");
                crate::proactive_policy::submit(
                    &app,
                    format!("agent_watch_error_{id}"),
                    format!("Your {adapter_type} session ran into trouble, sir: {err}"),
                    AlertUrgency::High,
                );
                let _ = app.emit("agent_watch:error", &json);
                break;
            }
            "login_required" => {
                crate::proactive_policy::submit(
                    &app,
                    format!("agent_watch_login_{id}"),
                    format!("{adapter_type} needs you to log in before it can continue, sir."),
                    AlertUrgency::High,
                );
                let _ = app.emit("agent_watch:login_required", &json);
                // Not terminal — keep polling, the user may sign in and it resumes.
            }
            "stopped" => {
                let _ = app.emit("agent_watch:stopped", &json);
                break;
            }
            _ => {
                let _ = app.emit("agent_watch:progress", &json);
            }
        }
    }

    ACTIVE_WATCHES.lock().unwrap().retain(|(wid, _)| wid != &id);
}

/// Watch any desktop agent, IDE (Antigravity, Cursor, Trae, VS Code), or running terminal task.
/// Holds a KeepAwakeLease until the task finishes, then proactively alerts the user via TTS.
pub async fn start_desktop_or_ide_watch<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    target_name: String,
    workdir: Option<String>,
) -> Result<String, String> {
    let id = format!(
        "ide_watch_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0)
    );

    let lease = crate::keep_awake::acquire_lease();
    ACTIVE_WATCHES.lock().unwrap().push((id.clone(), lease));

    let watch_id = id.clone();
    let poll_app = app.clone();
    let poll_target = target_name.clone();

    tauri::async_runtime::spawn(async move {
        poll_desktop_task_quiescence(poll_app, watch_id, poll_target, workdir).await;
    });

    Ok(id)
}

async fn poll_desktop_task_quiescence<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    id: String,
    target_name: String,
    workdir: Option<String>,
) {
    let started = Instant::now();
    let work_path = workdir.map(std::path::PathBuf::from);

    // Initial grace delay so the user or agent has started processing
    tokio::time::sleep(Duration::from_secs(3)).await;

    let mut last_activity = Instant::now();
    let mut check_count = 0;

    loop {
        tokio::time::sleep(POLL_INTERVAL).await;
        check_count += 1;

        if started.elapsed() > MAX_WATCH_DURATION {
            tracing::warn!("agent_watch: desktop watch {id} exceeded max duration");
            break;
        }

        // Check if watch was canceled
        let still_active = {
            let active = ACTIVE_WATCHES.lock().unwrap();
            active.iter().any(|(wid, _)| wid == &id)
        };
        if !still_active {
            break;
        }

        // Check workspace file activity if path is available
        let mut had_file_activity = false;
        if let Some(ref path) = work_path {
            if let Ok(entries) = std::fs::read_dir(path) {
                for entry in entries.flatten() {
                    if let Ok(meta) = entry.metadata() {
                        if let Ok(mtime) = meta.modified() {
                            if let Ok(elapsed) = mtime.elapsed() {
                                if elapsed < Duration::from_secs(3) {
                                    had_file_activity = true;
                                    break;
                                }
                            }
                        }
                    }
                }
            }
        }

        if had_file_activity {
            last_activity = Instant::now();
        } else if check_count >= 2 && last_activity.elapsed() >= Duration::from_secs(6) {
            // Settled! The flurry of changes ceased.
            crate::proactive_policy::submit(
                &app,
                format!("agent_watch_done_{id}"),
                format!("Your {target_name} response is ready, sir. It has finished running."),
                AlertUrgency::High,
            );
            let _ = app.emit(
                "agent_watch:done",
                serde_json::json!({
                    "id": id,
                    "type": target_name,
                    "status": "done"
                }),
            );
            break;
        }
    }

    ACTIVE_WATCHES.lock().unwrap().retain(|(wid, _)| wid != &id);
}

/// Stop a watch and release its keep-awake lease immediately.
pub async fn stop_watch(id: String) -> Result<(), String> {
    if is_sidecar_responsive() {
        let client = reqwest::Client::new();
        let _ = client.post(sidecar_url(&format!("/watch/{id}/stop"))).send().await;
    }
    ACTIVE_WATCHES.lock().unwrap().retain(|(wid, _)| wid != &id);
    Ok(())
}

/// Current status + buffered output for one watch, for a UI that wants to
/// show live progress without waiting for completion.
pub async fn get_watch(id: String) -> Result<serde_json::Value, String> {
    let client = reqwest::Client::new();
    let resp = client
        .get(sidecar_url(&format!("/watch/{id}")))
        .send()
        .await
        .map_err(|e| format!("failed to reach agent-watch sidecar: {e}"))?;
    resp.json().await.map_err(|e| format!("bad response: {e}"))
}

/// Every watch the sidecar knows about (id/type/status/timestamps, no
/// output), for a Settings panel to list. Empty, not an error, when the
/// sidecar isn't running — a UI polling this while idle shouldn't need to
/// distinguish "never started" from "nothing running".
pub async fn list_watches() -> Result<serde_json::Value, String> {
    if !is_sidecar_responsive() {
        return Ok(serde_json::json!({ "watches": [] }));
    }
    let client = reqwest::Client::new();
    let resp = client
        .get(sidecar_url("/watch"))
        .send()
        .await
        .map_err(|e| format!("failed to reach agent-watch sidecar: {e}"))?;
    resp.json().await.map_err(|e| format!("bad response: {e}"))
}

// --- Tauri commands --------------------------------------------------

#[tauri::command]
pub async fn agent_watch_list_adapters() -> Vec<String> {
    list_adapters().await
}

#[tauri::command]
pub fn agent_watch_unavailable_reason() -> Option<String> {
    sidecar_unavailable_reason()
}

#[tauri::command]
pub async fn agent_watch_start<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    adapter_type: String,
    workdir: Option<String>,
    prompt: Option<String>,
) -> Result<String, String> {
    start_watch(app, adapter_type, workdir, prompt).await
}

#[tauri::command]
pub async fn agent_watch_stop(id: String) -> Result<(), String> {
    stop_watch(id).await
}

#[tauri::command]
pub async fn agent_watch_status(id: String) -> Result<serde_json::Value, String> {
    get_watch(id).await
}

#[tauri::command]
pub async fn agent_watch_list_watches() -> Result<serde_json::Value, String> {
    list_watches().await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_adapters_includes_cli_and_ides() {
        assert!(KNOWN_ADAPTERS.contains(&"claude"));
        assert!(KNOWN_ADAPTERS.contains(&"antigravity"));
        assert!(KNOWN_ADAPTERS.contains(&"codex"));
        assert!(KNOWN_ADAPTERS.contains(&"cursor"));
    }

    #[test]
    fn sidecar_url_builds_against_the_fixed_local_port() {
        assert_eq!(sidecar_url("/health"), "http://127.0.0.1:39222/health");
    }
}
