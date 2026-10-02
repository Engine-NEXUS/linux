//! Lazy AT-SPI server manager — starts the Python accessibility sidecar on demand
//! and kills it after idle.
//!
//! Mirrors `lazy_ocr.rs`. The AT-SPI tier is the only perception and actuation
//! path that works on a native Wayland session, so a failure to start it is worth
//! reporting clearly rather than degrading silently into the pixel tier.
//!
//! Why lazy at all: the sidecar holds an a11y bus connection and a tree cache.
//! It is only needed for screen understanding and semantic actuation, so paying
//! for it on every boot would be waste. The idle timeout reclaims it, and the
//! failure cooldown stops a missing `at-spi2-core` from stalling every locate.

use std::process::{Child, Command};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

static ATSPI_CHILD: Mutex<Option<Child>> = Mutex::new(None);
static ATSPI_RUNNING: AtomicBool = AtomicBool::new(false);
static LAST_REQUEST: Mutex<Option<Instant>> = Mutex::new(None);
static ATSPI_LAST_FAILURE: Mutex<Option<Instant>> = Mutex::new(None);
static ATSPI_EVER_READY: AtomicBool = AtomicBool::new(false);

const ATSPI_IDLE_TIMEOUT: Duration = Duration::from_secs(300);
const ATSPI_DEFAULT_PORT: u16 = 39221;
// The sidecar itself is light — the cost is `gi` + the a11y bus, not a model —
// so the first-start budget is short.
const ATSPI_STARTUP_TIMEOUT_SECS: u64 = 8;
const ATSPI_FAILURE_COOLDOWN: Duration = Duration::from_secs(60);

/// Listen port (env override for tests): `ATSPI_PORT`, else 39221.
pub fn atspi_port() -> u16 {
    std::env::var("ATSPI_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(ATSPI_DEFAULT_PORT)
}

fn atspi_script_path() -> Option<std::path::PathBuf> {
    for candidate in [
        std::env::current_exe()
            .ok()?
            .parent()?
            .parent()?
            .parent()?
            .parent()?
            .join("server")
            .join("atspi_server.py"),
        std::env::current_exe()
            .ok()?
            .parent()?
            .parent()?
            .parent()?
            .join("server")
            .join("atspi_server.py"),
        std::env::current_exe()
            .ok()?
            .parent()?
            .join("resources")
            .join("server")
            .join("atspi_server.py"),
        // `cargo test` runs from src-tauri/, and dev invocations from the repo root.
        std::path::PathBuf::from("server/atspi_server.py"),
        std::path::PathBuf::from("../server/atspi_server.py"),
    ] {
        if candidate.exists() {
            return Some(candidate);
        }
    }
    None
}

fn is_atspi_responsive() -> bool {
    use std::io::{Read, Write};
    use std::net::TcpStream;
    let addr = format!("127.0.0.1:{}", atspi_port());
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

pub fn mark_atspi_request() {
    if let Ok(mut t) = LAST_REQUEST.lock() {
        *t = Some(Instant::now());
    }
}

/// Start the sidecar if it is not already up. Never blocks for long.
pub fn ensure_atspi_running() {
    if is_atspi_responsive() {
        ATSPI_RUNNING.store(true, Ordering::Relaxed);
        mark_atspi_request();
        return;
    }

    // Back off after a failure so a missing dependency cannot stall every
    // locate attempt for a minute each time.
    if let Ok(t) = ATSPI_LAST_FAILURE.lock() {
        if let Some(last) = *t {
            if last.elapsed() < ATSPI_FAILURE_COOLDOWN {
                return;
            }
        }
    }

    // Reap a dead child so we do not accumulate zombies.
    {
        let mut slot = match ATSPI_CHILD.lock() {
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

    let Some(script) = atspi_script_path() else {
        tracing::warn!("atspi: atspi_server.py not found — semantic tier unavailable");
        record_failure();
        return;
    };

    let mut cmd = Command::new(python_bin());
    cmd.arg(&script)
        .env("ATSPI_PORT", atspi_port().to_string())
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    if let Some(dir) = script.parent() {
        cmd.current_dir(dir);
    }

    match cmd.spawn() {
        Ok(child) => {
            if let Ok(mut slot) = ATSPI_CHILD.lock() {
                *slot = Some(child);
            }
            ATSPI_RUNNING.store(true, Ordering::Relaxed);
            mark_atspi_request();
            start_idle_killer();

            // Wait briefly so the first caller does not race the sidecar.
            let deadline = Instant::now() + Duration::from_secs(ATSPI_STARTUP_TIMEOUT_SECS);
            while Instant::now() < deadline {
                if is_atspi_responsive() {
                    ATSPI_EVER_READY.store(true, Ordering::Relaxed);
                    tracing::info!("atspi: sidecar ready on port {}", atspi_port());
                    return;
                }
                std::thread::sleep(Duration::from_millis(120));
            }
            tracing::warn!(
                "atspi: sidecar did not become responsive within {ATSPI_STARTUP_TIMEOUT_SECS}s"
            );
            record_failure();
        }
        Err(e) => {
            tracing::warn!("atspi: failed to spawn sidecar: {e}");
            record_failure();
        }
    }
}

fn record_failure() {
    ATSPI_RUNNING.store(false, Ordering::Relaxed);
    if let Ok(mut t) = ATSPI_LAST_FAILURE.lock() {
        *t = Some(Instant::now());
    }
}

fn python_bin() -> String {
    std::env::var("NEXUS_PYTHON")
        .or_else(|_| std::env::var("PYTHON"))
        .unwrap_or_else(|_| "python3".to_string())
}

/// Whether the tier has ever been observed working this session. Used to decide
/// whether a failure is worth surfacing to the user or silently degrading.
pub fn ever_ready() -> bool {
    ATSPI_EVER_READY.load(Ordering::Relaxed)
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
        if idle < ATSPI_IDLE_TIMEOUT {
            continue;
        }
        let mut slot = match ATSPI_CHILD.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        if let Some(mut child) = slot.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
        ATSPI_RUNNING.store(false, Ordering::Relaxed);
        tracing::debug!("atspi: sidecar killed after idle");
        return;
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn port_defaults_to_39221_and_honours_override() {
        // The override is what lets tests run sidecars side by side.
        assert_eq!(atspi_port(), 39221);
        std::env::set_var("ATSPI_PORT", "39399");
        assert_eq!(atspi_port(), 39399);
        std::env::remove_var("ATSPI_PORT");
        assert_eq!(atspi_port(), 39221);
    }

    #[test]
    fn port_ignores_nonsense_override() {
        std::env::set_var("ATSPI_PORT", "not-a-port");
        assert_eq!(atspi_port(), 39221);
        std::env::remove_var("ATSPI_PORT");
    }

    #[test]
    fn script_is_discoverable_from_the_repo() {
        // Guard against the resources copy and the dev path drifting apart.
        let p = atspi_script_path().expect("atspi_server.py should be discoverable");
        assert!(p.exists(), "resolved path does not exist: {}", p.display());
        assert!(
            p.file_name().unwrap() == "atspi_server.py",
            "resolved a non-AT-SPI script: {}",
            p.display()
        );
    }

    #[test]
    fn bundled_copy_matches_the_source() {
        // tauri.conf.json bundles the resources copy, so the two must not drift.
        let dev = std::path::Path::new("server/atspi_server.py");
        let bundled = std::path::Path::new("src-tauri/resources/server/atspi_server.py");
        if !dev.exists() || !bundled.exists() {
            return; // running from a different cwd; not a failure
        }
        let a = std::fs::read_to_string(dev).expect("read dev copy");
        let b = std::fs::read_to_string(bundled).expect("read bundled copy");
        assert_eq!(
            a, b,
            "atspi_server.py has drifted from its bundled copy — re-copy before building"
        );
    }
}
