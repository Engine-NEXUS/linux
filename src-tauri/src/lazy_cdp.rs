//! Lazy CDP sidecar manager.
//!
//! Mirrors `lazy_atspi.rs`. Kept separate rather than merged because the two
//! tiers have genuinely different lifetimes: AT-SPI is needed the moment anything
//! asks about the screen, while the CDP tier is only useful when a Chromium or
//! Electron app happens to be running with a debug port — which is a property of
//! the user's launch flags, not of anything NEXUS controls.

use std::process::{Child, Command};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

static CDP_CHILD: Mutex<Option<Child>> = Mutex::new(None);
static LAST_REQUEST: Mutex<Option<Instant>> = Mutex::new(None);
static CDP_LAST_FAILURE: Mutex<Option<Instant>> = Mutex::new(None);
static CDP_EVER_READY: AtomicBool = AtomicBool::new(false);

const CDP_IDLE_TIMEOUT: Duration = Duration::from_secs(300);
const CDP_DEFAULT_PORT: u16 = 39222;
const CDP_STARTUP_TIMEOUT_SECS: u64 = 8;
const CDP_FAILURE_COOLDOWN: Duration = Duration::from_secs(120);

/// Listen port for the sidecar itself (env override for tests).
///
/// Not to be confused with the *browser* debug ports the sidecar probes — those
/// are configured through `CDP_PORT` in the sidecar's environment.
pub fn cdp_server_port() -> u16 {
    std::env::var("NEXUS_CDP_SERVER_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(CDP_DEFAULT_PORT)
}

fn cdp_script_path() -> Option<std::path::PathBuf> {
    for candidate in [
        std::env::current_exe()
            .ok()?
            .parent()?
            .parent()?
            .parent()?
            .parent()?
            .join("server")
            .join("cdp_server.py"),
        std::env::current_exe()
            .ok()?
            .parent()?
            .parent()?
            .parent()?
            .join("server")
            .join("cdp_server.py"),
        std::env::current_exe()
            .ok()?
            .parent()?
            .join("resources")
            .join("server")
            .join("cdp_server.py"),
        std::path::PathBuf::from("server/cdp_server.py"),
        std::path::PathBuf::from("../server/cdp_server.py"),
    ] {
        if candidate.exists() {
            return Some(candidate);
        }
    }
    None
}

fn is_cdp_responsive() -> bool {
    use std::io::{Read, Write};
    use std::net::TcpStream;
    let addr = format!("127.0.0.1:{}", cdp_server_port());
    let Ok(mut stream) = TcpStream::connect(&addr) else {
        return false;
    };
    let _ = stream.set_read_timeout(Some(Duration::from_millis(600)));
    if stream.write_all(b"GET /health HTTP/1.0\r\n\r\n").is_err() {
        return false;
    }
    let mut buf = [0u8; 64];
    stream.read(&mut buf).is_ok()
}

pub fn mark_cdp_request() {
    if let Ok(mut t) = LAST_REQUEST.lock() {
        *t = Some(Instant::now());
    }
}

/// Start the sidecar if needed. Cheap when already up.
pub fn ensure_cdp_running() {
    if is_cdp_responsive() {
        mark_cdp_request();
        return;
    }
    if let Ok(t) = CDP_LAST_FAILURE.lock() {
        if let Some(last) = *t {
            if last.elapsed() < CDP_FAILURE_COOLDOWN {
                return;
            }
        }
    }
    {
        let mut slot = match CDP_CHILD.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        if let Some(mut child) = slot.take() {
            match child.try_wait() {
                Ok(Some(_)) => {}
                _ => {
                    *slot = Some(child);
                }
            }
        }
    }

    let Some(script) = cdp_script_path() else {
        tracing::warn!("cdp: cdp_server.py not found — Chromium/Electron tier unavailable");
        record_failure();
        return;
    };

    let mut cmd = Command::new(python_bin());
    cmd.arg(&script)
        .env("NEXUS_CDP_SERVER_PORT", cdp_server_port().to_string())
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    if let Some(dir) = script.parent() {
        cmd.current_dir(dir);
    }

    match cmd.spawn() {
        Ok(child) => {
            if let Ok(mut slot) = CDP_CHILD.lock() {
                *slot = Some(child);
            }
            mark_cdp_request();
            start_idle_killer();
            let deadline = Instant::now() + Duration::from_secs(CDP_STARTUP_TIMEOUT_SECS);
            while Instant::now() < deadline {
                if is_cdp_responsive() {
                    CDP_EVER_READY.store(true, Ordering::Relaxed);
                    tracing::info!("cdp: sidecar ready on port {}", cdp_server_port());
                    return;
                }
                std::thread::sleep(Duration::from_millis(120));
            }
            tracing::warn!("cdp: sidecar did not become responsive in time");
            record_failure();
        }
        Err(e) => {
            tracing::warn!("cdp: failed to spawn sidecar: {e}");
            record_failure();
        }
    }
}

fn record_failure() {
    if let Ok(mut t) = CDP_LAST_FAILURE.lock() {
        *t = Some(Instant::now());
    }
}

fn python_bin() -> String {
    std::env::var("NEXUS_PYTHON")
        .or_else(|_| std::env::var("PYTHON"))
        .unwrap_or_else(|_| "python3".to_string())
}

pub fn ever_ready() -> bool {
    CDP_EVER_READY.load(Ordering::Relaxed)
}

fn start_idle_killer() {
    std::thread::spawn(|| loop {
        std::thread::sleep(Duration::from_secs(30));
        let idle = LAST_REQUEST
            .lock()
            .ok()
            .and_then(|t| *t)
            .map(|t| t.elapsed())
            .unwrap_or(Duration::ZERO);
        if idle < CDP_IDLE_TIMEOUT {
            continue;
        }
        let mut slot = match CDP_CHILD.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        if let Some(mut child) = slot.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
        tracing::debug!("cdp: sidecar killed after idle");
        return;
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn port_defaults_to_39222_and_honours_override() {
        assert_eq!(cdp_server_port(), 39222);
        std::env::set_var("NEXUS_CDP_SERVER_PORT", "39444");
        assert_eq!(cdp_server_port(), 39444);
        std::env::remove_var("NEXUS_CDP_SERVER_PORT");
        assert_eq!(cdp_server_port(), 39222);
    }

    #[test]
    fn port_ignores_nonsense() {
        std::env::set_var("NEXUS_CDP_SERVER_PORT", "nope");
        assert_eq!(cdp_server_port(), 39222);
        std::env::remove_var("NEXUS_CDP_SERVER_PORT");
    }

    #[test]
    fn sidecar_port_is_distinct_from_browser_debug_ports() {
        // Conflating these would make the sidecar probe its own port. The browser
        // ports are the sidecar's `CDP_PORT`; this one is the sidecar's own
        // listener.
        assert_ne!(cdp_server_port(), crate::lazy_atspi::atspi_port());
    }

    #[test]
    fn script_is_discoverable() {
        let p = cdp_script_path().expect("cdp_server.py should be discoverable");
        assert!(p.exists());
        assert_eq!(p.file_name().unwrap(), "cdp_server.py");
    }

    #[test]
    fn bundled_copy_matches_the_source() {
        let dev = std::path::Path::new("server/cdp_server.py");
        let bundled = std::path::Path::new("src-tauri/resources/server/cdp_server.py");
        if !dev.exists() || !bundled.exists() {
            return;
        }
        assert_eq!(
            std::fs::read_to_string(dev).expect("read dev"),
            std::fs::read_to_string(bundled).expect("read bundled"),
            "cdp_server.py has drifted from its bundled copy"
        );
    }
}
