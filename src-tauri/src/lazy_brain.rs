//! Lazy brain server manager — starts the Qwen brain server (admin-only).
#![allow(dead_code)]
//!
//! Mirrors lazy_nlu.rs but with key differences:
//!   - Only starts when is_admin() is true (runtime + compile-time gate)
//!   - No idle timeout (always loaded, per admin's requirement)
//!   - Uses port 39219 (separate from NLU 39218 and STT 39217)
//!   - Finds brain_server.py in server/admin/ (gitignored, admin-only)
//!
//! The brain server runs Qwen2.5-0.5B-Instruct (398MB GGUF) and provides:
//!   - Intent classification (fallback when deterministic + BERT-Mini miss)
//!   - Pronunciation learning (zys → zync)
//!   - Phrasing generation (for training BERT-Mini)

#![cfg(feature = "admin-brain")]

use std::process::{Child, Command};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

static BRAIN_CHILD: Mutex<Option<Child>> = Mutex::new(None);
static BRAIN_RUNNING: AtomicBool = AtomicBool::new(false);

/// Get the brain server script path.
fn brain_script_path() -> Option<std::path::PathBuf> {
    let candidates = [
        // Development: src-tauri/target/debug/../../../server/admin/brain_server.py
        std::env::current_exe()
            .ok()?
            .parent()?          // target/debug or target/release
            .parent()?          // target
            .parent()?          // src-tauri
            .parent()?          // project root (ULTRON)
            .join("server")
            .join("admin")
            .join("brain_server.py"),
        // Fallback: src-tauri/target/release/../../server/admin/brain_server.py
        std::env::current_exe()
            .ok()?
            .parent()?          // target/release
            .parent()?          // target
            .parent()?          // src-tauri
            .join("server")
            .join("admin")
            .join("brain_server.py"),
        // Production: installed directory/resources/server/admin/brain_server.py
        std::env::current_exe()
            .ok()?
            .parent()?
            .join("resources")
            .join("server")
            .join("admin")
            .join("brain_server.py"),
    ];

    for candidate in &candidates {
        if candidate.exists() {
            tracing::info!("[lazy_brain] found brain_server.py at {:?}", candidate);
            return Some(candidate.clone());
        }
    }

    tracing::warn!("[lazy_brain] brain_server.py not found in any candidate location");
    tracing::warn!("[lazy_brain] Admin-only feature — expected at server/admin/brain_server.py");
    None
}

/// Check if the brain server is already running on the configured port.
fn is_brain_responsive() -> bool {
    use std::net::TcpStream;
    use std::time::Duration as TcpDuration;
    let port = crate::admin_config::brain_port();
    let addr = format!("127.0.0.1:{}", port);
    TcpStream::connect_timeout(
        &addr.parse().unwrap_or_else(|_| "127.0.0.1:39219".parse().unwrap()),
        TcpDuration::from_millis(500),
    )
    .is_ok()
}

/// Find a working Python interpreter (same logic as lazy_nlu.rs).
fn find_python() -> Option<String> {
    // 1. Try PATH-based commands
    for cmd in &["python", "python3", "py"] {
        if let Ok(output) = std::process::Command::new(cmd).arg("--version").output() {
            if output.status.success() {
                return Some(cmd.to_string());
            }
        }
    }

    // 2. Check Windows registry
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
                            return Some(exe.to_string_lossy().to_string());
                        }
                    }
                }
            }
        }
    }

    // 3. Check common per-user install
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
                    return Some(exe.to_string_lossy().to_string());
                }
            }
        }
    }

    None
}

/// Ensure the brain server is running. Spawns it if not.
///
/// ADMIN-ONLY: returns immediately if is_admin() is false.
/// NO IDLE TIMEOUT: the brain stays loaded for the entire session.
/// NON-BLOCKING: spawns the server and a background thread to wait for
/// readiness. Does NOT block the caller — if the brain isn't ready yet,
/// the brain_client will fall back to NLU/deterministic. This prevents
/// the first command from blocking for 12s while Qwen loads.
pub fn ensure_brain_running() {
    // Admin gate — no-op if not admin
    if !crate::admin_config::is_admin() {
        return;
    }

    // Already running (or spawning)?
    if BRAIN_RUNNING.load(Ordering::Relaxed) {
        return;
    }

    // Check if an external brain server is already running
    if is_brain_responsive() {
        BRAIN_RUNNING.store(true, Ordering::Relaxed);
        tracing::info!("[lazy_brain] external brain server detected on port {}", crate::admin_config::brain_port());
        return;
    }

    let script = match brain_script_path() {
        Some(p) => p,
        None => {
            tracing::warn!("[lazy_brain] cannot start brain server — script not found");
            tracing::warn!("[lazy_brain] admin-only: ensure server/admin/brain_server.py exists");
            return;
        }
    };

    let python_cmd = match find_python() {
        Some(p) => p,
        None => {
            tracing::error!(
                "[lazy_brain] no Python interpreter found. Install Python 3.12+ and run: \
                 pip install -r server/admin/requirements-brain.txt"
            );
            return;
        }
    };

    tracing::info!("[lazy_brain] starting brain server: {:?}", script);

    let mut cmd = Command::new(&python_cmd);
    cmd.arg(&script);

    // Hide console window on Windows
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }

    match cmd.spawn() {
        Ok(child) => {
            let pid = child.id();
            {
                let mut guard = BRAIN_CHILD.lock().unwrap();
                *guard = Some(child);
            }
            // Mark as "running" immediately to prevent re-spawning.
            // The background thread will clear this if it fails to start.
            BRAIN_RUNNING.store(true, Ordering::Relaxed);
            tracing::info!("[lazy_brain] brain server spawned (PID {}) — loading in background", pid);

            // Spawn a background thread to wait for readiness.
            // This does NOT block the caller — the first command falls back
            // to NLU/deterministic while the brain loads (~12s).
            std::thread::spawn(move || {
                let start = std::time::Instant::now();
                let timeout = std::time::Duration::from_secs(60);
                while start.elapsed() < timeout {
                    if is_brain_responsive() {
                        tracing::info!("[lazy_brain] brain server ready ({:.1}s)", start.elapsed().as_secs_f64());
                        return;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(500));
                }
                tracing::error!("[lazy_brain] brain server did not become responsive within 60s");
                // Kill the failed process and clear the running flag
                BRAIN_RUNNING.store(false, Ordering::Relaxed);
                let mut guard = BRAIN_CHILD.lock().unwrap();
                if let Some(mut child) = guard.take() {
                    let _ = child.kill();
                    let _ = child.wait();
                }
            });
        }
        Err(e) => {
            tracing::error!("[lazy_brain] failed to spawn brain server: {}", e);
        }
    }
}

/// Check if the brain server is running.
pub fn is_brain_running() -> bool {
    BRAIN_RUNNING.load(Ordering::Relaxed) || is_brain_responsive()
}

/// Kill the brain server (called on app shutdown).
pub fn kill_brain() {
    let mut guard = BRAIN_CHILD.lock().unwrap();
    if let Some(mut child) = guard.take() {
        let _ = child.kill();
        let _ = child.wait();
        tracing::info!("[lazy_brain] brain server killed");
    }
    BRAIN_RUNNING.store(false, Ordering::Relaxed);
}
