//! Lazy STT server manager ΓÇö starts the local faster-whisper STT server
//! on-demand when the wake word fires, and kills it after idle to save
//! ~340 MB of RAM at idle.
//!
//! The STT server (server/stt_server.py) uses faster-whisper tiny.en and
//! takes ~340 MB of RAM. Instead of running it constantly, we:
//!   1. Spawn it when the wake word is detected
//!   2. Kill it after 60 seconds of no transcription requests
//!
//! This saves ~340 MB of RAM at idle (the vast majority of the time).

use std::process::{Child, Command};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

static STT_CHILD: Mutex<Option<Child>> = Mutex::new(None);
static STT_RUNNING: AtomicBool = AtomicBool::new(false);
/// Guard against concurrent ensure_stt_running calls — set to true atomically
/// before spawning, so a second caller sees it and returns immediately.
static STT_STARTING: AtomicBool = AtomicBool::new(false);
static LAST_REQUEST: Mutex<Option<Instant>> = Mutex::new(None);

const STT_IDLE_TIMEOUT: Duration = Duration::from_secs(300); // 5 minutes (unused — STT kept alive permanently)
const STT_KEEP_ALIVE: bool = true; // Never kill STT — 128 MB idle cost is worth 0.5s response
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

/// Check if the STT server is already running (external or our child).
/// Uses a raw TCP connection to avoid tokio runtime dependency ΓÇö this
/// function is called from non-tokio threads (wake-word thread, hotkey handler).
fn is_stt_responsive() -> bool {
    // Use a simple TCP connection + HTTP GET instead of reqwest, which
    // requires a tokio runtime that may not be available on the calling thread.
    use std::io::{Read, Write};
    use std::net::TcpStream;

    let addr = format!("127.0.0.1:{STT_PORT}");
    let timeout = Duration::from_millis(500);

    let stream_result = TcpStream::connect_timeout(
        &addr.parse().unwrap_or_else(|_| "127.0.0.1:39217".parse().unwrap()),
        timeout,
    );

    match stream_result {
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
            // Check for HTTP 200
            response_str.contains("200 OK") || response_str.contains("\"ok\":true")
        }
        Err(_) => false,
    }
}

/// Find a working Python interpreter.
///
/// Search order:
/// 1. `python`, `python3`, `py` on PATH (fast, works if PATH is updated)
/// 2. Windows registry: HKCU/HKLM PythonCore\3.13, 3.12, 3.11, 3.10
/// 3. Common per-user and system install locations
pub fn find_python() -> Option<String> {
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

    // Atomically claim the "starting" slot — prevents concurrent spawns
    if STT_STARTING.compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst).is_err() {
        tracing::debug!("lazy_stt: another thread is already starting STT, skipping");
        return;
    }

    // Check if an external STT server is already running (e.g. started by run.ps1)
    if is_stt_responsive() {
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
            tracing::warn!("lazy_stt: stt_server.py not found — skipping (external server may be used)");
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
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .spawn();

    match child {
        Ok(c) => {
            let pid = c.id();
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
        return; // Never kill — keep STT always ready for zero delay
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
        tracing::info!("lazy_stt: STT server idle for {}s ΓÇö killing to save RAM", STT_IDLE_TIMEOUT.as_secs());
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
