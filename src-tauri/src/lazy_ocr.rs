//! Lazy OCR server manager — starts the Python RapidOCR server on-demand
//! for text grounding (Phase-3b locator tier) and kills it after idle.
//!
//! Mirrors `lazy_nlu.rs`: spawn on first locate that needs OCR, kill
//! after 120s idle (the ONNX models hold ~100-300MB), 60s cooldown
//! after a failed start so a missing `rapidocr-onnxruntime` never
//! blocks the vision fallback path.

use std::process::{Child, Command};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

static OCR_CHILD: Mutex<Option<Child>> = Mutex::new(None);
static OCR_RUNNING: AtomicBool = AtomicBool::new(false);
static LAST_REQUEST: Mutex<Option<Instant>> = Mutex::new(None);
static OCR_LAST_FAILURE: Mutex<Option<Instant>> = Mutex::new(None);
static OCR_EVER_READY: AtomicBool = AtomicBool::new(false);

const OCR_IDLE_TIMEOUT: Duration = Duration::from_secs(120);
const OCR_DEFAULT_PORT: u16 = 39220;
const OCR_STARTUP_TIMEOUT_SECS: u64 = 20;
const OCR_FIRST_STARTUP_TIMEOUT_SECS: u64 = 60;
const OCR_FAILURE_COOLDOWN: Duration = Duration::from_secs(60);

/// Listen port (env override for tests): OCR_PORT, else 39220.
pub fn ocr_port() -> u16 {
    std::env::var("OCR_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(OCR_DEFAULT_PORT)
}

fn ocr_script_path() -> Option<std::path::PathBuf> {
    for candidate in [
        std::env::current_exe()
            .ok()?
            .parent()?
            .parent()?
            .parent()?
            .parent()?
            .join("server")
            .join("ocr_server.py"),
        std::env::current_exe()
            .ok()?
            .parent()?
            .parent()?
            .parent()?
            .join("server")
            .join("ocr_server.py"),
        std::env::current_exe()
            .ok()?
            .parent()?
            .join("resources")
            .join("server")
            .join("ocr_server.py"),
    ] {
        if candidate.exists() {
            return Some(candidate);
        }
    }
    tracing::warn!("[lazy_ocr] ocr_server.py not found in any candidate location");
    None
}

fn is_ocr_responsive() -> bool {
    use std::net::TcpStream;
    let addr = format!("127.0.0.1:{}", ocr_port());
    TcpStream::connect_timeout(
        &addr.parse().unwrap_or_else(|_| "127.0.0.1:39220".parse().unwrap()),
        Duration::from_millis(200),
    )
    .is_ok()
}

/// Mark an OCR request (resets the idle killer). Call before each use.
pub fn mark_ocr_request() {
    *LAST_REQUEST.lock().unwrap() = Some(Instant::now());
}

/// Ensure the OCR server is running. Spawns it if not. Never blocks
/// longer than the startup budget; on failure the caller falls back
/// to the vision grid tier.
pub fn ensure_ocr_running() {
    if OCR_RUNNING.load(Ordering::Relaxed) {
        return;
    }
    {
        let last_fail = OCR_LAST_FAILURE.lock().unwrap();
        if let Some(t) = *last_fail {
            if t.elapsed() < OCR_FAILURE_COOLDOWN {
                return;
            }
        }
    }
    if is_ocr_responsive() {
        OCR_RUNNING.store(true, Ordering::Relaxed);
        return;
    }
    let script = match ocr_script_path() {
        Some(p) => p,
        None => return,
    };
    // Reuse the NLU module's interpreter probe (PATH + Windows registry).
    let python_cmd = match crate::lazy_nlu::find_python() {
        Some(p) => p,
        None => {
            tracing::error!("[lazy_ocr] no Python interpreter found");
            return;
        }
    };
    tracing::info!("[lazy_ocr] starting OCR server: {:?}", script);
    let child = Command::new(&python_cmd)
        .arg(&script)
        .env("OCR_PORT", ocr_port().to_string())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .spawn();
    match child {
        Ok(c) => {
            *OCR_CHILD.lock().unwrap() = Some(c);
            OCR_RUNNING.store(true, Ordering::Relaxed);
            let budget = if OCR_EVER_READY.load(Ordering::Relaxed) {
                OCR_STARTUP_TIMEOUT_SECS
            } else {
                OCR_FIRST_STARTUP_TIMEOUT_SECS
            };
            let max_polls = (budget * 1000) / 500;
            for _ in 0..max_polls {
                std::thread::sleep(Duration::from_millis(500));
                if is_ocr_responsive() {
                    OCR_EVER_READY.store(true, Ordering::Relaxed);
                    *OCR_LAST_FAILURE.lock().unwrap() = None;
                    start_idle_killer();
                    return;
                }
            }
            tracing::warn!("[lazy_ocr] OCR server not responsive in {budget}s");
            OCR_RUNNING.store(false, Ordering::Relaxed);
            *OCR_LAST_FAILURE.lock().unwrap() = Some(Instant::now());
            let mut guard = OCR_CHILD.lock().unwrap();
            if let Some(mut child) = guard.take() {
                let _ = child.kill();
                let _ = child.wait();
                if let Some(stderr) = child.stderr.take() {
                    use std::io::Read;
                    let mut total = String::new();
                    let _ = stderr.take(4096).read_to_string(&mut total);
                    if !total.trim().is_empty() {
                        tracing::error!(
                            "[lazy_ocr] child stderr: {}",
                            total.chars().take(1500).collect::<String>()
                        );
                    }
                }
            }
        }
        Err(e) => {
            tracing::error!("[lazy_ocr] spawn failed: {e}");
            *OCR_LAST_FAILURE.lock().unwrap() = Some(Instant::now());
        }
    }
}

fn start_idle_killer() {
    std::thread::spawn(|| loop {
        std::thread::sleep(Duration::from_secs(15));
        let idle = LAST_REQUEST
            .lock()
            .unwrap()
            .map_or(true, |t| t.elapsed() >= OCR_IDLE_TIMEOUT);
        if idle {
            let mut guard = OCR_CHILD.lock().unwrap();
            if let Some(mut child) = guard.take() {
                let _ = child.kill();
                let _ = child.wait();
                OCR_RUNNING.store(false, Ordering::Relaxed);
                tracing::info!("[lazy_ocr] idle timeout — server killed");
            }
            return;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ocr_port_defaults_and_env_override() {
        std::env::remove_var("OCR_PORT");
        assert_eq!(ocr_port(), 39220);
        std::env::set_var("OCR_PORT", "39299");
        assert_eq!(ocr_port(), 39299);
        std::env::set_var("OCR_PORT", "bogus");
        assert_eq!(ocr_port(), 39220);
        std::env::remove_var("OCR_PORT");
    }
}
