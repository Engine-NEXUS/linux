//! Lazy NLU server manager — starts the Python NLU server (BERT-Mini ONNX)
//! on-demand when the deterministic parser can't handle a command, and kills
//! it after idle to save RAM.
//!
//! The NLU server (server/nlu_server.py) uses a BERT-Mini ONNX model and
//! takes ~50-100 MB of RAM. Instead of running it constantly, we:
//!   1. Spawn it when the deterministic parser returns None
//!   2. Kill it after 60 seconds of no parse requests
//!
//! This keeps RAM low at idle while providing ML-based intent classification
//! as a fallback for commands the deterministic parser can't handle.

use std::process::{Child, Command};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

static NLU_CHILD: Mutex<Option<Child>> = Mutex::new(None);
static NLU_RUNNING: AtomicBool = AtomicBool::new(false);
static LAST_REQUEST: Mutex<Option<Instant>> = Mutex::new(None);
/// Cooldown after a failed NLU startup — don't retry for 60s to avoid
/// blocking every command that misses the deterministic parser.
static NLU_LAST_FAILURE: Mutex<Option<Instant>> = Mutex::new(None);

const NLU_IDLE_TIMEOUT: Duration = Duration::from_secs(300); // 5 minutes (was 60s)
const NLU_PORT: u16 = 39218;
/// How long to wait for the NLU server to become responsive (was 30s —
/// too long, caused "loading non stop" when the ONNX model was missing).
/// The BERT-Mini ONNX model loads in ~8-12s; 15s is enough.
const NLU_STARTUP_TIMEOUT_SECS: u64 = 15;
/// Cooldown after a failed startup attempt (60s).
const NLU_FAILURE_COOLDOWN: Duration = Duration::from_secs(60);

/// Get the NLU server script path.
fn nlu_script_path() -> Option<std::path::PathBuf> {
    let candidates = [
        // Development: src-tauri/target/release/../../../server/nlu_server.py
        std::env::current_exe()
            .ok()?
            .parent()?          // target/release
            .parent()?          // target
            .parent()?          // src-tauri
            .parent()?          // project root (ULTRON)
            .join("server")
            .join("nlu_server.py"),
        // Fallback: src-tauri/target/release/../../server/nlu_server.py
        std::env::current_exe()
            .ok()?
            .parent()?          // target/release
            .parent()?          // target
            .parent()?          // src-tauri
            .join("server")
            .join("nlu_server.py"),
        // Production: installed directory/resources/server/nlu_server.py
        std::env::current_exe()
            .ok()?
            .parent()?
            .join("resources")
            .join("server")
            .join("nlu_server.py"),
    ];

    for candidate in &candidates {
        if candidate.exists() {
            return Some(candidate.clone());
        }
    }

    tracing::warn!("[lazy_nlu] nlu_server.py not found in any candidate location");
    None
}

/// Check if the NLU server is already running (e.g. started externally or by a previous call).
fn is_nlu_responsive() -> bool {
    // Use a raw TCP connection — works from any thread (unlike tokio runtime checks)
    use std::net::TcpStream;
    use std::time::Duration as TcpDuration;
    let addr = format!("127.0.0.1:{}", NLU_PORT);
    TcpStream::connect_timeout(
        &addr.parse().unwrap_or_else(|_| "127.0.0.1:39218".parse().unwrap()),
        TcpDuration::from_millis(200),
    )
    .is_ok()
}

/// Find a working Python interpreter.
///
/// Search order:
/// 1. `python`, `python3`, `py` on PATH (fast, works if PATH is updated)
/// 2. Windows registry: HKCU/HKLM PythonCore\3.12\InstallPath, 3.11, 3.10
/// 3. Common per-user install: %LOCALAPPDATA%\Programs\Python\Python3XX\python.exe
///
/// This is needed because the NSIS installer installs Python with
/// PrependPath=1, but the PATH update doesn't reach processes spawned
/// from the installer process (the app is launched immediately after).
fn find_python() -> Option<String> {
    // 1. Try PATH-based commands — verify each actually works
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
                            tracing::info!("[lazy_nlu] found Python {} via registry: {}", ver, exe.display());
                            return Some(exe.to_string_lossy().to_string());
                        }
                    }
                }
            }
        }
    }

    // 3. Check common per-user install location
    #[cfg(windows)]
    {
        if let Ok(local_appdata) = std::env::var("LOCALAPPDATA") {
            for ver in &["Python313", "Python312", "Python311", "Python310"] {
                let exe = std::path::Path::new(&local_appdata)
                    .join("Programs")
                    .join("Python")
                    .join(ver)
                    .join("python.exe");
                if exe.exists() {
                    tracing::info!("[lazy_nlu] found Python at {}", exe.display());
                    return Some(exe.to_string_lossy().to_string());
                }
            }
        }
    }

    None
}

/// Ensure the NLU server is running. Spawns it if not.
pub fn ensure_nlu_running() {
    // Already running?
    if NLU_RUNNING.load(Ordering::Relaxed) {
        return;
    }

    // Cooldown: if the last startup attempt failed recently, don't retry.
    // This prevents blocking every command for 15s when the NLU server
    // can't start (e.g. missing ONNX model, missing Python deps).
    {
        let last_fail = NLU_LAST_FAILURE.lock().unwrap();
        if let Some(t) = *last_fail {
            if t.elapsed() < NLU_FAILURE_COOLDOWN {
                tracing::debug!(
                    "[lazy_nlu] skipping startup — in cooldown ({:.0}s remaining)",
                    (NLU_FAILURE_COOLDOWN - t.elapsed()).as_secs_f64()
                );
                return;
            }
        }
    }

    // Check if an external NLU server is already running on the port
    if is_nlu_responsive() {
        NLU_RUNNING.store(true, Ordering::Relaxed);
        tracing::info!("[lazy_nlu] external NLU server detected on port {}", NLU_PORT);
        return;
    }

    let script = match nlu_script_path() {
        Some(p) => p,
        None => {
            tracing::warn!("[lazy_nlu] cannot start NLU server — script not found");
            return;
        }
    };

    tracing::info!("[lazy_nlu] starting NLU server: {:?}", script);

    let python_cmd = match find_python() {
        Some(p) => p,
        None => {
            tracing::error!(
                "[lazy_nlu] no Python interpreter found. Install Python 3.12+ and run: \
                 pip install numpy onnxruntime fastapi uvicorn pydantic transformers"
            );
            return;
        }
    };

    // Spawn: python nlu_server.py
    let child = Command::new(&python_cmd)
        .arg(&script)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .spawn();

    match child {
        Ok(c) => {
            *NLU_CHILD.lock().unwrap() = Some(c);
            NLU_RUNNING.store(true, Ordering::Relaxed);
            tracing::info!("[lazy_nlu] NLU server spawned, waiting for it to be ready...");
            // Wait for the server to be responsive (up to 15 seconds — was 30s).
            // The BERT-Mini ONNX model + transformers tokenizer takes ~8-12s
            // to load on first spawn. 15s gives a small margin.
            let poll_interval = Duration::from_millis(500);
            let max_polls = (NLU_STARTUP_TIMEOUT_SECS * 1000) / poll_interval.as_millis() as u64;
            for _ in 0..max_polls {
                std::thread::sleep(poll_interval);
                if is_nlu_responsive() {
                    tracing::info!("[lazy_nlu] NLU server is ready");
                    // Clear any previous failure time
                    *NLU_LAST_FAILURE.lock().unwrap() = None;
                    // Start the idle killer thread
                    start_idle_killer();
                    return;
                }
            }
            tracing::warn!("[lazy_nlu] NLU server did not become responsive in {}s", NLU_STARTUP_TIMEOUT_SECS);
            NLU_RUNNING.store(false, Ordering::Relaxed);
            // Record the failure time so we don't retry for 60s
            *NLU_LAST_FAILURE.lock().unwrap() = Some(Instant::now());
            // Kill the failed child process
            let mut child_guard = NLU_CHILD.lock().unwrap();
            if let Some(mut child) = child_guard.take() {
                let _ = child.kill();
                let _ = child.wait();
            }
        }
        Err(e) => {
            tracing::error!("[lazy_nlu] failed to spawn NLU server: {}", e);
            *NLU_LAST_FAILURE.lock().unwrap() = Some(Instant::now());
        }
    }
}

/// Mark that an NLU request was just made (resets the idle timer).
pub fn mark_nlu_request() {
    *LAST_REQUEST.lock().unwrap() = Some(Instant::now());
}

/// Start a background thread that kills the NLU server after idle timeout.
fn start_idle_killer() {
    std::thread::spawn(move || loop {
        std::thread::sleep(Duration::from_secs(10));
        let should_kill = {
            let last = LAST_REQUEST.lock().unwrap();
            if let Some(t) = *last {
                t.elapsed() > NLU_IDLE_TIMEOUT
            } else {
                // No request ever made — kill after timeout from start
                true
            }
        };
        if should_kill && NLU_RUNNING.load(Ordering::Relaxed) {
            tracing::info!("[lazy_nlu] idle timeout reached, killing NLU server");
            let mut child_guard = NLU_CHILD.lock().unwrap();
            if let Some(mut child) = child_guard.take() {
                let _ = child.kill();
                let _ = child.wait();
            }
            NLU_RUNNING.store(false, Ordering::Relaxed);
            return;
        }
    });
}
