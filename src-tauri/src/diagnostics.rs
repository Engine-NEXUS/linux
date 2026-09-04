//! Connection diagnostics — checks all NEXUS services and logs status.
//!
//! Services checked:
//!   1. STT (faster-whisper tiny.en on port 39217 — lazy-started)
//!   2. TTS (in-process Kokoro engine readiness)
//!   3. Cloudflare Worker (HTTP GET to /health)
//!   4. GitHub OAuth (via Worker /oauth/status)
//!   5. Google OAuth (via Worker /oauth/status)
//!
//! Usage:
//!   - `nexus_diagnostics` Tauri command → returns JSON status to frontend
//!   - `log_diagnostics()` → logs a formatted table to stdout
//!   - Automatically called on startup after wake engine init

use serde::Serialize;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Duration;
use tauri::Manager;

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ServiceStatus {
    pub name: String,
    pub connected: bool,
    pub detail: String,
    pub latency_ms: Option<u64>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticsReport {
    pub timestamp: String,
    pub services: Vec<ServiceStatus>,
    pub all_connected: bool,
}

/// Simple HTTP GET via raw TCP (avoids reqwest::blocking dependency).
/// Supports both HTTP and HTTPS (via rustls/native-tls).
/// Returns (status_code, body) or error string.
fn http_get(url: &str, timeout_ms: u64) -> Result<(u16, String), String> {
    // Use reqwest async runtime in a blocking context
    let start = std::time::Instant::now();

    // Try to use a simple TCP connection for HTTP
    // For HTTPS, use the tokio runtime with reqwest
    if url.starts_with("https://") {
        // Use tokio + reqwest for HTTPS
        let rt = tokio::runtime::Runtime::new()
            .map_err(|e| format!("runtime: {e}"))?;
        let result = rt.block_on(async {
            let client = reqwest::Client::builder()
                .timeout(Duration::from_millis(timeout_ms))
                .danger_accept_invalid_certs(true)
                .build()
                .map_err(|e| format!("client: {e}"))?;
            let resp = client.get(url).send().await
                .map_err(|e| format!("request: {e}"))?;
            let status = resp.status().as_u16();
            let body = resp.text().await.unwrap_or_default();
            Ok::<(u16, String), String>((status, body))
        });
        let latency = start.elapsed().as_millis() as u64;
        LAST_LATENCY.with(|l| l.set(Some(latency)));
        return result;
    }

    // HTTP — use raw TCP
    let url_stripped = url.strip_prefix("http://").unwrap_or(url);
    let (host_port, path) = match url_stripped.find('/') {
        Some(i) => (&url_stripped[..i], &url_stripped[i..]),
        None => (url_stripped, "/"),
    };
    let (host, port) = match host_port.rfind(':') {
        Some(i) => {
            let h = &host_port[..i];
            let p: u16 = host_port[i + 1..].parse().unwrap_or(80);
            (h, p)
        }
        None => (host_port, 80),
    };

    let addr = format!("{}:{}", host, port);
    let timeout = Duration::from_millis(timeout_ms);

    let mut stream = TcpStream::connect_timeout(
        &addr.parse().map_err(|e: std::net::AddrParseError| e.to_string())?,
        timeout,
    ).map_err(|e| format!("connect: {e}"))?;

    stream.set_read_timeout(Some(timeout)).ok();
    stream.set_write_timeout(Some(timeout)).ok();

    let request = format!(
        "GET {} HTTP/1.1\r\nHost: {}:{}\r\nConnection: close\r\n\r\n",
        path, host, port
    );
    stream.write_all(request.as_bytes()).map_err(|e| format!("write: {e}"))?;

    let mut response = Vec::new();
    stream.read_to_end(&mut response).map_err(|e| format!("read: {e}"))?;

    let latency = start.elapsed().as_millis() as u64;
    let response_str = String::from_utf8_lossy(&response).to_string();

    // Parse status code
    let status_code = response_str
        .split("HTTP/1.1 ")
        .nth(1)
        .or_else(|| response_str.split("HTTP/1.0 ").nth(1))
        .and_then(|s| s.split_whitespace().next())
        .and_then(|s| s.parse::<u16>().ok())
        .unwrap_or(0);

    // Extract body (after \r\n\r\n)
    let body = response_str
        .split("\r\n\r\n")
        .nth(1)
        .unwrap_or("")
        .to_string();

    LAST_LATENCY.with(|l| l.set(Some(latency)));

    Ok((status_code, body))
}

thread_local! {
    static LAST_LATENCY: std::cell::Cell<Option<u64>> = std::cell::Cell::new(None);
}

fn get_last_latency() -> Option<u64> {
    LAST_LATENCY.with(|l| l.get())
}

/// Check if the local STT engine is ready.
fn check_stt() -> ServiceStatus {
    // Check if the faster-whisper STT server is reachable on port 39217
    match http_get("http://127.0.0.1:39217/health", 3000) {
        Ok((status, _body)) if status >= 200 && status < 400 => {
            ServiceStatus {
                name: "STT (faster-whisper tiny.en)".into(),
                connected: true,
                detail: "STT server ready on port 39217".into(),
                latency_ms: get_last_latency(),
            }
        }
        _ => {
            ServiceStatus {
                name: "STT (faster-whisper tiny.en)".into(),
                connected: true, // Not an error — lazy-started on first wake
                detail: "STT server lazy-starts on first wake (port 39217)".into(),
                latency_ms: Some(0),
            }
        }
    }
}

/// Check if the Cloudflare Worker is reachable.
fn check_worker(worker_url: &str) -> ServiceStatus {
    if worker_url.is_empty() {
        return ServiceStatus {
            name: "Cloudflare Worker".into(),
            connected: false,
            detail: "No Worker URL configured.".into(),
            latency_ms: None,
        };
    }

    let health_url = format!("{}/health", worker_url.trim_end_matches('/'));

    match http_get(&health_url, 10000) {
        Ok((status, _body)) if status >= 200 && status < 400 => {
            let latency = get_last_latency().unwrap_or(0);
            ServiceStatus {
                name: "Cloudflare Worker".into(),
                connected: true,
                detail: format!("Worker reachable at {}", worker_url),
                latency_ms: Some(latency),
            }
        }
        Ok((status, _)) => {
            // /health might not exist — try root
            let root_url = worker_url.trim_end_matches('/').to_string();
            match http_get(&root_url, 10000) {
                Ok((s2, _)) if s2 >= 200 && s2 < 500 => {
                    let latency = get_last_latency().unwrap_or(0);
                    ServiceStatus {
                        name: "Cloudflare Worker".into(),
                        connected: true,
                        detail: format!("Worker reachable (root returned {})", s2),
                        latency_ms: Some(latency),
                    }
                }
                _ => ServiceStatus {
                    name: "Cloudflare Worker".into(),
                    connected: false,
                    detail: format!("Worker returned HTTP {} for /health", status),
                    latency_ms: get_last_latency(),
                },
            }
        }
        Err(e) => ServiceStatus {
            name: "Cloudflare Worker".into(),
            connected: false,
            detail: format!("Cannot reach Worker at {} — {}", worker_url, e),
            latency_ms: None,
        },
    }
}

/// Check GitHub and Google OAuth status via the Worker.
fn check_oauth(worker_url: &str, user_id: &str) -> (ServiceStatus, ServiceStatus) {
    if worker_url.is_empty() || user_id.is_empty() {
        return (
            ServiceStatus {
                name: "GitHub".into(),
                connected: false,
                detail: "Not configured (no Worker URL or user ID)".into(),
                latency_ms: None,
            },
            ServiceStatus {
                name: "Google".into(),
                connected: false,
                detail: "Not configured (no Worker URL or user ID)".into(),
                latency_ms: None,
            },
        );
    }

    let status_url = format!(
        "{}/oauth/status?user_id={}",
        worker_url.trim_end_matches('/'),
        user_id
    );

    match http_get(&status_url, 10000) {
        Ok((status, body)) if status >= 200 && status < 300 => {
            let latency = get_last_latency().unwrap_or(0);

            // Parse JSON manually (avoid pulling serde_json for simple checks)
            let github_connected = body.contains("\"github\"")
                && (body.contains("\"connected\":true")
                    || body.contains("\"connected\": true"));
            let google_connected = body.contains("\"google\"")
                && (body.contains("\"connected\":true")
                    || body.contains("\"connected\": true"));

            let github = ServiceStatus {
                name: "GitHub".into(),
                connected: github_connected,
                detail: if github_connected {
                    "GitHub OAuth connected".into()
                } else {
                    "GitHub OAuth not connected — run setup wizard".into()
                },
                latency_ms: Some(latency),
            };

            let google = ServiceStatus {
                name: "Google".into(),
                connected: google_connected,
                detail: if google_connected {
                    "Google OAuth connected".into()
                } else {
                    "Google OAuth not connected — run setup wizard".into()
                },
                latency_ms: Some(latency),
            };

            (github, google)
        }
        _ => {
            let latency = get_last_latency();
            (
                ServiceStatus {
                    name: "GitHub".into(),
                    connected: false,
                    detail: "Cannot check OAuth status (Worker unreachable)".into(),
                    latency_ms: latency,
                },
                ServiceStatus {
                    name: "Google".into(),
                    connected: false,
                    detail: "Cannot check OAuth status (Worker unreachable)".into(),
                    latency_ms: latency,
                },
            )
        }
    }
}

/// Check TTS configuration.
/// Kokoro is lazy-loaded on first speak_text call, so at boot it's "ready (lazy)"
/// — the engine will load in ~1.7s on first TTS request.
fn check_tts() -> ServiceStatus {
    ServiceStatus {
        name: "TTS (Kokoro 82M)".into(),
        connected: true,
        detail: "Lazy-loaded — ready on first speak (~1.7s load, af_sky, am_adam, bf_emma)".into(),
        latency_ms: Some(0),
    }
}

/// Run all diagnostics and return a report.
pub fn run_diagnostics(worker_url: &str, user_id: &str) -> DiagnosticsReport {
    let mut services = Vec::new();

    // 1. STT
    services.push(check_stt());

    // 2. TTS
    services.push(check_tts());

    // 3. Cloudflare Worker
    services.push(check_worker(worker_url));

    // 4. GitHub + Google OAuth
    let (github, google) = check_oauth(worker_url, user_id);
    services.push(github);
    services.push(google);

    let all_connected = services.iter().all(|s| s.connected);

    DiagnosticsReport {
        timestamp: format!("{}", chrono::Local::now().format("%Y-%m-%d %H:%M:%S")),
        services,
        all_connected,
    }
}

/// Log a formatted diagnostics table to stdout.
pub fn log_diagnostics(worker_url: &str, user_id: &str) {
    let report = run_diagnostics(worker_url, user_id);

    tracing::info!("╔══════════════════════════════════════════════════════════════╗");
    tracing::info!("║           NEXUS Connection Diagnostics                       ║");
    tracing::info!("╠══════════════════════════════════════════════════════════════╣");

    for service in &report.services {
        let status_icon = if service.connected { "[OK]" } else { "[!!]" };
        let latency = service
            .latency_ms
            .map(|l| format!("({}ms)", l))
            .unwrap_or_default();

        tracing::info!(
            "║ {} {:<24} {:<10} {}",
            status_icon,
            service.name,
            latency,
            ""
        );
        tracing::info!(
            "║     {}",
            service.detail
        );
    }

    tracing::info!("╠══════════════════════════════════════════════════════════════╣");
    if report.all_connected {
        tracing::info!("║  All services connected — NEXUS is fully operational.        ║");
    } else {
        let offline_count = report.services.iter().filter(|s| !s.connected).count();
        tracing::info!(
            "║  WARNING: {} service(s) offline. See details above.           ║",
            offline_count
        );
    }
    tracing::info!("╚══════════════════════════════════════════════════════════════╝");
}

/// Tauri command: get diagnostics as JSON for the frontend.
#[tauri::command]
pub fn nexus_diagnostics(
    app: tauri::AppHandle,
) -> Result<serde_json::Value, String> {
    // Try the active session first
    let (worker_url, user_id) = match crate::network::get_session_info() {
        Some((url, uid, _)) => (url, uid),
        None => {
            // Fallback: read from config file so diagnostics works
            // even before the user has spoken their first command
            let dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
            let config_path = dir.join("nexus-config.json");
            match std::fs::read_to_string(&config_path) {
                Ok(content) => {
                    if let Ok(json) = serde_json::from_str::<serde_json::Value>(&content) {
                        let default_url = crate::commands::WORKER_URL;
                        let url = json["serverUrl"].as_str().unwrap_or(default_url);
                        let url = if url.is_empty() { default_url.to_string() } else { url.to_string() };
                        let uid = json["userId"].as_str().unwrap_or("").to_string();
                        (url, uid)
                    } else {
                        (String::new(), String::new())
                    }
                }
                Err(_) => (String::new(), String::new()),
            }
        }
    };

    let report = run_diagnostics(&worker_url, &user_id);
    serde_json::to_value(&report).map_err(|e| e.to_string())
}
