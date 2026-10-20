//! Lazy STT server manager Î“Ã‡Ã¶ starts the local faster-whisper STT server
//! on-demand when the wake word fires, and kills it after idle to save
//! ~340 MB of RAM at idle.
//!
//! The STT server (server/stt_server.py) uses Moonshine Streaming local
//! takes ~340 MB of RAM. Instead of running it constantly, we:
//!   1. Spawn it when the wake word is detected
//!   2. Kill it after 60 seconds of no transcription requests
//!
//! This saves ~340 MB of RAM at idle (the vast majority of the time).

use std::io::BufRead;
use std::process::{Child, Command};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

static STT_CHILD: Mutex<Option<Child>> = Mutex::new(None);
static STT_RUNNING: AtomicBool = AtomicBool::new(false);
/// Guard against concurrent ensure_stt_running calls â€” set to true atomically
/// before spawning, so a second caller sees it and returns immediately.
static STT_STARTING: AtomicBool = AtomicBool::new(false);
static LAST_REQUEST: Mutex<Option<Instant>> = Mutex::new(None);

const STT_IDLE_TIMEOUT: Duration = Duration::from_secs(60); // 60s idle timeout
const STT_KEEP_ALIVE: bool = false; // Kill idle STT fallback after 60s to enforce strict <150MB RAM limit
const STT_PORT: u16 = 39217;

/// Get the STT server script path.
fn stt_script_path() -> Option<std::path::PathBuf> {
    // Try several locations relative to the executable and project root
    let candidates = [
        // Development: src-tauri/target/release/../../../server/stt_server.py
        //   current_exe = .../src-tauri/target/release/nexus.exe
        //   parent() = .../target/release
        //   parent() = .../target
        //   parent() = .../src-tauri
        //   parent() = .../ULTRON  (project root)
        //   join("server/stt_server.py") = .../ULTRON/server/stt_server.py
        std::env::current_exe()
            .ok()?
            .parent()?          // target/release
            .parent()?          // target
            .parent()?          // src-tauri
            .parent()?          // project root (ULTRON)
            .join("server")
            .join("stt_server.py"),
        // Fallback: src-tauri/target/release/../../server/stt_server.py
        //   (in case the project root is src-tauri itself)
        std::env::current_exe()
            .ok()?
            .parent()?          // target/release
            .parent()?          // target
            .parent()?          // src-tauri
            .join("server")
            .join("stt_server.py"),
        // Production: <app_dir>/resources/server/stt_server.py
        std::env::current_exe()
            .ok()?
            .parent()?
            .join("resources")
            .join("server")
            .join("stt_server.py"),
    ];

    for path in &candidates {
        if path.exists() {
            return Some(path.clone());
        }
    }
    None
}

/// Probe the local STT sidecar with a raw HTTP GET and return the response
/// body, or `None` when nothing is listening on the port.
///
/// Uses a raw TCP connection to avoid tokio runtime dependency — this
/// function is called from non-tokio threads (wake-word thread, hotkey handler).
fn stt_http_probe() -> Option<String> {
    use std::io::{Read, Write};
    use std::net::TcpStream;

    let addr = format!("127.0.0.1:{STT_PORT}");
    let timeout = Duration::from_millis(500);

    let stream_result = TcpStream::connect_timeout(
        &addr.parse().unwrap_or_else(|_| "127.0.0.1:39217".parse().unwrap()),
        timeout,
    );

    let mut stream = stream_result.ok()?;
    stream.set_read_timeout(Some(timeout)).ok();
    stream.set_write_timeout(Some(timeout)).ok();
    let request = "GET /health HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n";
    if stream.write_all(request.as_bytes()).is_err() {
        return None;
    }
    let mut response = Vec::new();
    if stream.read_to_end(&mut response).is_err() {
        return None;
    }
    Some(String::from_utf8_lossy(&response).to_string())
}

/// Check if the STT server is running AND healthy (it can transcribe).
pub(crate) fn is_stt_responsive() -> bool {
    let Some(response) = stt_http_probe() else {
        return false;
    };
    // "200 OK" / "ok":true = healthy.
    response.contains("200 OK") || response.contains("\"ok\":true")
}

/// True when a sidecar is listening on the port at all — even when it reports
/// that it cannot transcribe (e.g. `moonshine-voice` is not installed, answer
/// /health with 503 + "ok":false). The manager must not spawn a SECOND copy
/// over a running-but-broken sidecar; the real reason travels with the
/// transcription error instead.
fn stt_server_present() -> bool {
    stt_http_probe().is_some()
}

/// Environment for the local STT child process. Moonshine language is locked
/// here rather than inherited from the operator shell: English-only operation
/// must not depend on ambient process configuration.
fn stt_child_env() -> Vec<(String, String)> {
    vec![
        ("MOONSHINE_MODEL".to_string(), read_moonshine_model()),
        ("MOONSHINE_LANG".to_string(), "en".to_string()),
    ]
}

/// Find a working Python interpreter.
///
/// Search order:
/// 1. `python`, `python3`, `py` on PATH (fast, works if PATH is updated)
/// 2. Windows registry: HKCU/HKLM PythonCore\3.13, 3.12, 3.11, 3.10
/// 3. Common per-user and system install locations
pub fn find_python() -> Option<String> {
    // 1. Try PATH-based commands â€” verify each actually works
    for cmd in &["python", "python3", "py"] {
        if let Ok(output) = std::process::Command::new(cmd).arg("--version").output() {
            if output.status.success() {
                return Some(cmd.to_string());
            }
        }
    }

    // 2. Check Windows registry for Python install paths
    #[cfg(windows)]
    {
        use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE};
        use winreg::RegKey;
        let hives = [HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE];
        for &hive in &hives {
            for ver in &["3.13", "3.12", "3.11", "3.10"] {
                let key_path = format!("SOFTWARE\\Python\\PythonCore\\{}\\InstallPath", ver);
                if let Ok(key) = RegKey::predef(hive).open_subkey(&key_path) {
                    if let Ok(install_dir) = key.get_value::<String, _>("") {
                        let exe = std::path::Path::new(&install_dir).join("python.exe");
                        if exe.exists() {
                            tracing::info!("lazy_stt: found Python {} via registry: {}", ver, exe.display());
                            return Some(exe.to_string_lossy().to_string());
                        }
                    }
                }
            }
        }
    }

    // 3. Check common per-user and system install locations
    #[cfg(windows)]
    {
        let mut candidates = Vec::new();
        if let Ok(local_appdata) = std::env::var("LOCALAPPDATA") {
            for ver in &["Python313", "Python312", "Python311", "Python310"] {
                candidates.push(std::path::Path::new(&local_appdata).join("Programs").join("Python").join(ver).join("python.exe"));
            }
        }
        if let Ok(prog_files) = std::env::var("ProgramFiles") {
            for ver in &["Python313", "Python312", "Python311", "Python310"] {
                candidates.push(std::path::Path::new(&prog_files).join("Python").join(ver).join("python.exe"));
            }
        }
        for ver in &["Python313", "Python312", "Python311", "Python310"] {
            candidates.push(std::path::PathBuf::from(format!(r"C:\{}\python.exe", ver)));
        }

        for path in candidates {
            if path.exists() {
                if let Ok(output) = std::process::Command::new(&path).arg("--version").output() {
                    if output.status.success() {
                        tracing::info!("lazy_stt: found Python at {}", path.display());
                        return Some(path.to_string_lossy().to_string());
                    }
                }
            }
        }
    }

    None
}

/// Start the STT server if it's not already running.
/// Called when the wake word fires or when a transcription is needed.
/// Uses STT_STARTING as a compare-and-swap guard to prevent two concurrent
/// callers from both spawning a child process.
pub fn ensure_stt_running() {
    // Already running?
    if STT_RUNNING.load(Ordering::Relaxed) {
        return;
    }

    // Atomically claim the "starting" slot â€” prevents concurrent spawns
    if STT_STARTING.compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst).is_err() {
        tracing::debug!("lazy_stt: another thread is already starting STT, skipping");
        return;
    }

    // Check if an external STT server is already running (e.g. started by run.ps1)
    if stt_server_present() {
        STT_RUNNING.store(true, Ordering::Relaxed);
        STT_STARTING.store(false, Ordering::SeqCst);
        tracing::info!("lazy_stt: external STT server already running on port {STT_PORT}");
        return;
    }

    // Find the script
    let script = match stt_script_path() {
        Some(p) => p,
        None => {
            STT_STARTING.store(false, Ordering::SeqCst);
            tracing::warn!("lazy_stt: stt_server.py not found â€” skipping (external server may be used)");
            return;
        }
    };

    tracing::info!("lazy_stt: starting STT server ({})", script.display());

    let python_cmd = match find_python() {
        Some(cmd) => cmd,
        None => {
            STT_STARTING.store(false, Ordering::SeqCst);
            tracing::error!(
                "lazy_stt: no Python interpreter found. Install Python 3.12+ and run: \
                 pip install moonshine-voice fastapi uvicorn python-multipart"
            );
            return;
        }
    };

    let child = Command::new(&python_cmd)
        .arg(&script)
        .envs(stt_child_env())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .spawn();

    match child {
        Ok(mut c) => {
            let pid = c.id();
            // Drain the child's stderr on a dedicated thread: the pipe is
            // otherwise never read (Moonshine/uvicorn tracebacks lost) and
            // a filled 64KB pipe can block the child mid-transcription with
            // zero log evidence. Lines surface as [STT-PY] in the unified
            // console; traceback/error lines escalate to warn.
            if let Some(stderr) = c.stderr.take() {
                std::thread::Builder::new()
                    .name("stt-stderr-drain".into())
                    .spawn(move || {
                        for line in std::io::BufReader::new(stderr).lines().map_while(Result::ok) {
                            let msg = format!("[STT-PY] {line}");
                            let low = line.to_lowercase();
                            if low.contains("traceback")
                                || low.contains("error")
                                || low.contains("exception")
                                || low.contains("warn")
                            {
                                tracing::warn!("{msg}");
                            } else {
                                tracing::info!("{msg}");
                            }
                        }
                    })
                    .ok();
            }
            *STT_CHILD.lock().unwrap() = Some(c);
            STT_RUNNING.store(true, Ordering::Relaxed);
            STT_STARTING.store(false, Ordering::SeqCst);
            *LAST_REQUEST.lock().unwrap() = Some(Instant::now());
            tracing::info!("lazy_stt: STT server started (PID {pid})");

            // Wait for it to be ready (up to 15 seconds)
            std::thread::spawn(move || {
                for _ in 0..30 {
                    std::thread::sleep(Duration::from_millis(500));
                    if is_stt_responsive() {
                        tracing::info!("lazy_stt: STT server is ready");
                        return;
                    }
                }
                tracing::warn!("lazy_stt: STT server did not become ready in 15s");
            });
        }
        Err(e) => {
            STT_STARTING.store(false, Ordering::SeqCst);
            tracing::error!("lazy_stt: failed to start STT server: {e}");
        }
    }
}

/// Mark that a transcription request was just made (resets the idle timer).
pub fn mark_stt_request() {
    *LAST_REQUEST.lock().unwrap() = Some(Instant::now());
}

/// Check if the STT server has been idle for too long and kill it.
/// Called periodically from a background thread.
/// NOTE: STT is now kept alive permanently (STT_KEEP_ALIVE=true) because
/// the idle cost is only ~128 MB and killing it adds 10-15s delay on the
/// next command (cold model load). The monitor thread still runs for
/// future use but is a no-op.
pub fn check_stt_idle() {
    if STT_KEEP_ALIVE {
        return; // Never kill â€” keep STT always ready for zero delay
    }

    if !STT_RUNNING.load(Ordering::Relaxed) {
        return;
    }

    let should_kill = {
        let last = LAST_REQUEST.lock().unwrap();
        match *last {
            Some(t) => Instant::now().duration_since(t) > STT_IDLE_TIMEOUT,
            None => false,
        }
    };

    if should_kill {
        tracing::info!("lazy_stt: STT server idle for {}s Î“Ã‡Ã¶ killing to save RAM", STT_IDLE_TIMEOUT.as_secs());
        let mut child_guard = STT_CHILD.lock().unwrap();
        if let Some(mut child) = child_guard.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
        STT_RUNNING.store(false, Ordering::Relaxed);
        *LAST_REQUEST.lock().unwrap() = None;
    }
}

/// Start a background thread that periodically checks if STT should be killed.
pub fn start_idle_monitor() {
    std::thread::spawn(move || {
        loop {
            std::thread::sleep(Duration::from_secs(10));
            check_stt_idle();
        }
    });
}

/// Pre-warm the local STT server shortly after boot so the first voice
/// command has zero cold-start delay (cold model load is 10-15s).
///
/// Only spawns when cloud STT won't be used (no Groq key or localSttOnly),
/// mirroring the wake-word path's decision in wakeword_oww.rs â€” Groq users
/// pay no RAM. Runs on a background thread; never blocks startup or paint.
pub fn spawn_prewarm(app_data_dir: std::path::PathBuf) {
    std::thread::Builder::new()
        .name("stt-prewarm".into())
        .spawn(move || {
            // Let boot settle (wake engine + first paint win the race).
            std::thread::sleep(Duration::from_secs(20));
            let settings_path = app_data_dir.join("settings.json");
            let (groq_key, local_only) = match std::fs::read_to_string(&settings_path) {
                Ok(content) => {
                    let json: serde_json::Value =
                        serde_json::from_str(&content).unwrap_or_default();
                    let key = crate::auth_vault::get_api_key("groq").unwrap_or_else(|| {
                        let json: serde_json::Value =
                            serde_json::from_str(&content).unwrap_or_default();
                        json.get("groqApiKey")
                            .or_else(|| json.get("groq_api_key"))
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string()
                    });
                    let local = json
                        .get("localSttOnly")
                        .or_else(|| json.get("local_stt_only"))
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false);
                    (key, local)
                }
                Err(_) => (String::new(), false),
            };
            if groq_key.is_empty() || local_only {
                tracing::info!(
                    "lazy_stt: pre-warming local STT server for zero-delay first command"
                );
                ensure_stt_running();
            } else {
                tracing::info!(
                    "lazy_stt: Groq cloud STT configured, skipping pre-warm (saves RAM)"
                );
            }
        })
        .ok();
}

/// Read the Moonshine model name from settings.json. Falls back to
/// "medium_streaming" (245M, 6.65% WER) for admin accuracy. Family
/// members with 8GB laptops can set "small_streaming" (123M, 7.84% WER)
/// in Settings to reduce RAM usage.
fn read_moonshine_model() -> String {
    // Try %APPDATA%/com.nexus.assistant/settings.json
    let path = if let Ok(appdata) = std::env::var("APPDATA") {
        std::path::PathBuf::from(appdata)
            .join("com.nexus.assistant")
            .join("settings.json")
    } else if let Ok(home) = std::env::var("HOME") {
        std::path::PathBuf::from(home)
            .join(".config")
            .join("com.nexus.assistant")
            .join("settings.json")
    } else {
        return "medium_streaming".to_string();
    };

    let Ok(content) = std::fs::read_to_string(&path) else {
        return "medium_streaming".to_string();
    };
    let Ok(json) = serde_json::from_str::<serde_json::Value>(&content) else {
        return "medium_streaming".to_string();
    };

    json.get("moonshineModel")
        .or_else(|| json.get("moonshine_model"))
        .and_then(|v| v.as_str())
        .unwrap_or("medium_streaming")
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn test_local_child_locks_english_model() {
        let env: HashMap<String, String> = stt_child_env().into_iter().collect();
        assert_eq!(env.get("MOONSHINE_LANG"), Some(&"en".to_string()));
        assert!(!env
            .get("MOONSHINE_MODEL")
            .map(|model| model.is_empty())
            .unwrap_or(true));
    }
}
