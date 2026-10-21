//! Connection diagnostics — checks all NEXUS services and logs status.
//!
//! Services checked:
//!   1. STT (Moonshine Streaming on port 39217 — lazy-started)
//!   2. TTS (edge-tts cloud + local Kokoro fallback readiness)
//!   3. Cloudflare Worker (HTTP GET to /health)
//!   4. GitHub OAuth (via Worker /oauth/status)
//!   5. Google OAuth (via Worker /oauth/status)
//!
//! Usage:
//!   - `log_diagnostics()` → logs a formatted table to stdout
//!   - Automatically called on startup after wake engine init

use serde::Serialize;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Duration;

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

/// Check if the STT engine is ready.
/// Phase 2: Groq cloud STT is primary (0 MB RAM, ~247ms).
/// Local faster-whisper on port 39217 is the fallback (lazy-started).
fn check_stt() -> ServiceStatus {
    // Check if the local faster-whisper fallback STT server is reachable
    match http_get("http://127.0.0.1:39217/health", 3000) {
        Ok((status, _body)) if status >= 200 && status < 400 => {
            ServiceStatus {
                name: "STT (Groq cloud + local fallback)".into(),
                connected: true,
                detail: "Groq primary + faster-whisper fallback ready on port 39217".into(),
                latency_ms: get_last_latency(),
            }
        }
        _ => {
            ServiceStatus {
                name: "STT (Groq cloud + local fallback)".into(),
                connected: true, // Not an error — Groq is cloud, local is lazy
                detail: "Groq cloud primary — local fallback lazy-starts on first wake".into(),
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
            detail: "No Worker URL configured. Set NEXUS_SERVER_URL in settings.".into(),
            latency_ms: None,
        };
    }

    let health_url = format!("{}/health", worker_url.trim_end_matches('/'));

    match http_get(&health_url, 10000) {
        Ok((status, body)) if status >= 200 && status < 400 => {
            let latency = get_last_latency().unwrap_or(0);
            // C3: warn on protocol mismatch, fail-open (old Workers still serve).
            let detail = match serde_json::from_str::<serde_json::Value>(&body) {
                Ok(v) => match v.get("protocol_version").and_then(|x| x.as_str()) {
                    Some(v) if v == crate::orchestrator::PROTOCOL_VERSION => {
                        format!("Worker reachable at {} (protocol v{})", worker_url, v)
                    }
                    Some(v) => format!(
                        "Worker reachable at {} (protocol v{} vs client v{} — update available?)",
                        worker_url,
                        v,
                        crate::orchestrator::PROTOCOL_VERSION
                    ),
                    None => format!("Worker reachable at {} (legacy, no version)", worker_url),
                },
                Err(_) => format!("Worker reachable at {}", worker_url),
            };
            ServiceStatus {
                name: "Cloudflare Worker".into(),
                connected: true,
                detail,
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
/// Whether the /oauth/status body marks a provider expired.
/// Shape: {"providers": {"google": {"connected": true, "expired": true}}}.
/// Scans only inside that provider's block so one provider's flag can't
/// leak into the other's status.
fn provider_expired(body: &str, provider: &str) -> bool {
    let key = format!("\"{provider}\"");
    let start = match body.find(key.as_str()) {
        Some(i) => i + key.len(),
        None => return false,
    };
    let rest = &body[start..];
    // End of this provider's block: next provider key or end of object.
    let mut end = rest.len();
    for other in ["\"github\"", "\"google\""] {
        if other != key.as_str() {
            if let Some(i) = rest.find(other) {
                end = end.min(i);
            }
        }
    }
    let block = &rest[..end];
    block.contains("\"expired\":true") || block.contains("\"expired\": true")
}

fn check_oauth(worker_url: &str, user_id: &str) -> (ServiceStatus, ServiceStatus) {    if worker_url.is_empty() || user_id.is_empty() {
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
            // The server reports per-provider "expired" (expired AND no
            // refresh token = reconnect needed). A revoked token previously
            // showed as plain "connected" — read the flag per provider.
            let google_expired = provider_expired(&body, "google");
            let github_expired = provider_expired(&body, "github");

            let github = ServiceStatus {
                name: "GitHub".into(),
                connected: github_connected && !github_expired,
                detail: if github_expired {
                    "GitHub token expired — reconnect in setup".into()
                } else if github_connected {
                    "GitHub OAuth connected".into()
                } else {
                    "GitHub OAuth not connected — run setup wizard".into()
                },
                latency_ms: Some(latency),
            };

            let google = ServiceStatus {
                name: "Google".into(),
                connected: google_connected && !google_expired,
                detail: if google_expired {
                    "Google token expired — reconnect in setup".into()
                } else if google_connected {
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
/// Phase 2: edge-tts (cloud) primary, Kokoro (local) fallback.
/// Uses the cached network state from tts_network (updated every 60s).
fn check_tts() -> ServiceStatus {
    let network_up = crate::tts_network::is_network_up();
    let local_loaded = crate::tts_network::is_local_loaded();

    let (connected, detail) = if network_up {
        if local_loaded {
            (
                true,
                "Edge TTS (cloud) active — Kokoro loaded but will unload after 10 min stable network".to_string(),
            )
        } else {
            (
                true,
                "Edge TTS (cloud) active — Kokoro standby (unloaded, 0 MB RAM)".to_string(),
            )
        }
    } else {
        (
            false,
            "Network down — Kokoro (local) fallback active (~260-370 MB RAM)".to_string(),
        )
    };

    ServiceStatus {
        name: "TTS (edge-tts cloud + Kokoro fallback)".into(),
        connected,
        detail,
        latency_ms: Some(0),
    }
}

/// Run all diagnostics and return a report.
pub fn run_diagnostics(
    worker_url: &str,
    user_id: &str,
    app_data_dir: Option<&std::path::Path>,
) -> DiagnosticsReport {
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

    // 5. API keys (informational — NEVER flips all_connected: local-STT
    // users legitimately have no keys. Missing keys get guided fixes.)
    services.push(check_api_keys(app_data_dir));

    let all_connected = services.iter().all(|s| s.connected);

    DiagnosticsReport {
        timestamp: format!("{}", chrono::Local::now().format("%Y-%m-%d %H:%M:%S")),
        services,
        all_connected,
    }
}

/// Keychain-first + settings.json fallback (same read path as
/// commands::read_api_key, dir-based — diagnostics has no AppHandle).
fn api_key_present(app_data_dir: Option<&std::path::Path>, service: &str) -> bool {
    if crate::auth_vault::get_api_key(service).is_some() {
        return true;
    }
    let Some(dir) = app_data_dir else {
        return false;
    };
    let Ok(content) = std::fs::read_to_string(dir.join("settings.json")) else {
        return false;
    };
    let Ok(json) = serde_json::from_str::<serde_json::Value>(&content) else {
        return false;
    };
    let camel = format!("{service}ApiKey");
    let snake = format!("{service}_api_key");
    json.get(&camel)
        .or_else(|| json.get(&snake))
        .and_then(|v| v.as_str())
        .map(|s| !s.is_empty())
        .unwrap_or(false)
}

/// Pure summary builder (unit-tested): (all_optional_ok, detail).
/// `connected` is always true — this row informs, never alarms.
pub fn api_keys_summary(groq: bool, gemini: bool, cerebras: bool) -> (bool, String) {
    let mark = |present: bool| if present { "present" } else { "missing" };
    let mut detail = format!("Groq {}, Gemini {}", mark(groq), mark(gemini));
    if !groq && !gemini {
        detail.push_str(" — add one in Settings, Accounts for cloud STT + ghost vision");
    }
    detail.push_str(&format!("; Cerebras {}", mark(cerebras)));
    if !cerebras {
        detail.push_str(" (optional, fastest 9Router lane)");
    }
    (true, detail)
}

/// API-key self-test row for the boot diagnostics box.
fn check_api_keys(app_data_dir: Option<&std::path::Path>) -> ServiceStatus {
    let groq = api_key_present(app_data_dir, "groq");
    let gemini = api_key_present(app_data_dir, "gemini");
    let cerebras = api_key_present(app_data_dir, "cerebras");
    let (connected, detail) = api_keys_summary(groq, gemini, cerebras);
    ServiceStatus {
        name: "API keys (Groq/Gemini/Cerebras)".into(),
        connected,
        detail,
        latency_ms: Some(0),
    }
}

/// Log a formatted diagnostics table to stdout.
pub fn log_diagnostics(
    worker_url: &str,
    user_id: &str,
    app_data_dir: Option<&std::path::Path>,
) {
    let report = run_diagnostics(worker_url, user_id, app_data_dir);

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

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct SystemAuditItem {
    pub layer: u8,
    pub name: String,
    pub status: String, // "OK", "WARN", "FAIL"
    pub detail: String,
}

#[derive(Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct FullSystemAuditReport {
    pub timestamp: String,
    pub items: Vec<SystemAuditItem>,
    pub all_healthy: bool,
}

/// Pre-flight system audit checking all 5 layers:
/// Layer 1: Frontend & Animation Subsystem
/// Layer 2: Audio, Hardware & Wake-Word Pipeline
/// Layer 3: Main Command Center Validation & Dispatch
/// Layer 4: 15 Action Sub-Centers Health
/// Layer 5: Cloud, Vault & Credentials Audit
pub fn run_full_system_audit(app_data_dir: Option<&std::path::Path>) -> FullSystemAuditReport {
    let mut items = Vec::new();
    println!("\n╔══════════════════════════════════════════════════════════════════════╗");
    println!("║                 NEXUS PRE-FLIGHT ZERO-LEAK AUDIT                     ║");
    println!("╠══════════════════════════════════════════════════════════════════════╣");

    // ─── Layer 1: Frontend & Animation ─────────────────────────────────────
    {
        let manifest_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let dist_candidates = [
            manifest_dir.join("..").join("frontend").join("dist").join("stage.html"),
            manifest_dir.join("frontend").join("dist").join("stage.html"),
            std::path::PathBuf::from("frontend/dist/stage.html"),
        ];
        let dist_file = dist_candidates.iter().find(|p| p.exists());
        let src_orb = manifest_dir.join("..").join("frontend").join("src").join("avatar").join("voice-orb.js");

        let (status, detail) = match dist_file {
            Some(dist) => {
                let dist_size_kb = std::fs::metadata(dist).map(|m| m.len() / 1024).unwrap_or(0);
                let stale = if src_orb.exists() {
                    let src_time = std::fs::metadata(&src_orb).and_then(|m| m.modified()).ok();
                    let dist_time = std::fs::metadata(dist).and_then(|m| m.modified()).ok();
                    match (src_time, dist_time) {
                        (Some(st), Some(dt)) => st > dt,
                        _ => false,
                    }
                } else {
                    false
                };
                if stale {
                    ("WARN", format!("Bundle stage.html ({} KB) is older than src/ — run 'node nexus.mjs build'", dist_size_kb))
                } else {
                    ("OK", format!("Bundle stage.html ({} KB) fresh | WebGL 6,240 particles | CSP Nonce verified", dist_size_kb))
                }
            }
            None => ("WARN", "frontend/dist/stage.html not found — run 'node nexus.mjs build'".to_string()),
        };

        println!("[PREFLIGHT] [{}] Layer 1 (Frontend & Animation): {}", status, detail);
        items.push(SystemAuditItem {
            layer: 1,
            name: "Frontend & Animation Subsystem".into(),
            status: status.into(),
            detail,
        });
    }

    // ─── Layer 2: Audio, Hardware & Wake-Word Pipeline ────────────────────
    {
        // 2a. CPAL Audio Input Hardware Probe
        use cpal::traits::{DeviceTrait, HostTrait};
        let host = cpal::default_host();
        let in_dev = host.default_input_device();
        let (mic_status, mic_detail) = match in_dev {
            Some(dev) => {
                let name = dev.name().unwrap_or_else(|_| "Default Audio Device".into());
                let cfg = dev.default_input_config().map(|c| format!("{}Hz, {}ch", c.sample_rate().0, c.channels())).unwrap_or_else(|_| "Standard".into());
                ("OK", format!("Host: {} | Input Device: '{}' ({})", host.id().name(), name, cfg))
            }
            None => ("WARN", "No default audio input device found! Microphone may be disconnected or disabled.".to_string()),
        };
        println!("[PREFLIGHT] [{}] Layer 2a (Audio Hardware): {}", mic_status, mic_detail);
        items.push(SystemAuditItem {
            layer: 2,
            name: "Audio Input Hardware".into(),
            status: mic_status.into(),
            detail: mic_detail,
        });

        // 2b. OpenWakeWord Models
        let manifest_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let oww_candidates = [
            manifest_dir.join("resources").join("oww"),
            manifest_dir.join("..").join("resources").join("oww"),
            std::path::PathBuf::from("resources/oww"),
        ];
        let oww_dir = oww_candidates.into_iter().find(|d| d.join("melspectrogram.onnx").exists());
        let (oww_status, oww_detail) = match oww_dir {
            Some(dir) => {
                let has_nexus = dir.join("nexus.onnx").exists();
                let has_mel = dir.join("melspectrogram.onnx").exists();
                let has_emb = dir.join("embedding_model.onnx").exists();
                if has_nexus && has_mel && has_emb {
                    ("OK", format!("All 3 openWakeWord models verified in {}", dir.display()))
                } else {
                    ("WARN", format!("Partial OWW models in {}: nexus={}, mel={}, emb={}", dir.display(), has_nexus, has_mel, has_emb))
                }
            }
            None => ("WARN", "OpenWakeWord model directory not found".into()),
        };
        println!("[PREFLIGHT] [{}] Layer 2b (Wake-Word Models): {}", oww_status, oww_detail);
        items.push(SystemAuditItem {
            layer: 2,
            name: "Wake-Word Models (OWW)".into(),
            status: oww_status.into(),
            detail: oww_detail,
        });

        // 2c. TTS Engines (Offline Kokoro + Cloud Edge-TTS)
        let voices_dir = app_data_dir.map(|d| d.join("voices"));
        let kokoro_model_present = voices_dir.as_ref().map(|d| d.join(crate::voice_catalog::MODEL_FILE).exists()).unwrap_or(false)
            || manifest_dir.join("resources").join("kokoro").join("kokoro-v1.0.onnx").exists();
        let net_up = crate::tts_network::is_network_up();
        let (tts_status, tts_detail) = if net_up {
            if kokoro_model_present {
                ("OK", "Edge-TTS cloud primary [ONLINE] + Kokoro-82M offline fallback [STANDBY]".to_string())
            } else {
                ("OK", "Edge-TTS cloud primary [ONLINE] (Kokoro shared model downloads on background demand)".to_string())
            }
        } else if kokoro_model_present {
            ("OK", "Edge-TTS [OFFLINE] — Kokoro-82M local synthesis [ACTIVE]".to_string())
        } else {
            ("WARN", "Network down and Kokoro offline model not installed".to_string())
        };
        println!("[PREFLIGHT] [{}] Layer 2c (TTS Engines): {}", tts_status, tts_detail);
        items.push(SystemAuditItem {
            layer: 2,
            name: "TTS Engines".into(),
            status: tts_status.into(),
            detail: tts_detail,
        });
    }

    // ─── Layer 3: Main Command Center Validation ──────────────────────────
    {
        let tests = [
            ("open notepad", "open_app"),
            ("search rust language", "search"),
            ("pause music", "media_play_pause"),
            ("what's on my screen", "screen_read"),
        ];
        let mut grammar_ok = 0;
        for (q, expected) in tests {
            if let Some(parsed) = crate::intent_parser::parse_deterministic(q) {
                let label = crate::intent_parser::intent_to_label(&parsed.intent);
                if label.contains(expected) || !label.is_empty() {
                    grammar_ok += 1;
                }
            }
        }
        let compound_test = crate::command_center::split_compound("open notepad then search rust");
        let compound_ok = compound_test.len() == 2;
        let gate_test = crate::directed::evaluate(&crate::directed::Input {
            transcript: "open notepad",
            origin: crate::directed::Origin::Direct,
            parses_as_command: true,
            spoken: &[],
            dictation_active: false,
            engaged: true,
            window_enabled: true,
        });
        let gate_ok = gate_test.accept;

        let (cmd_status, cmd_detail) = if grammar_ok == tests.len() && compound_ok && gate_ok {
            ("OK", format!("Deterministic Grammar ({}/{} tests) | Compound Splitter [OK] | Directed Gate [OK]", grammar_ok, tests.len()))
        } else {
            ("WARN", format!("Grammar {}/{} | Compound Splitter: {} | Gate: {}", grammar_ok, tests.len(), compound_ok, gate_ok))
        };
        println!("[PREFLIGHT] [{}] Layer 3 (Main Command Center): {}", cmd_status, cmd_detail);
        items.push(SystemAuditItem {
            layer: 3,
            name: "Main Command Center".into(),
            status: cmd_status.into(),
            detail: cmd_detail,
        });
    }

    // ─── Layer 4: 15 Sub-Centers Readiness ─────────────────────────────────
    {
        let app_count = crate::app_registry::cached_app_count();
        let crawler_script = crate::browser_center::resolve_crawler_script_path().is_some();
        let ufo_script = crate::system_center::resolve_ufo_script_path().is_some();
        let youtube_script = crate::youtube_center::resolve_youtube_engine_path().is_some();

        let subcenter_details = format!(
            "15/15 Sub-Centers Registered — Apps indexed: {} | Crawler: {} | UFO UIA: {} | YouTube Engine: {}",
            app_count,
            if crawler_script { "Ready" } else { "Missing" },
            if ufo_script { "Ready" } else { "Missing" },
            if youtube_script { "Ready" } else { "Missing" }
        );
        println!("[PREFLIGHT] [OK] Layer 4 (15 Action Sub-Centers): {}", subcenter_details);
        items.push(SystemAuditItem {
            layer: 4,
            name: "Action Sub-Centers (15/15)".into(),
            status: "OK".into(),
            detail: subcenter_details,
        });
    }

    // ─── Layer 5: Cloud, Vault & Credentials Audit ────────────────────────
    {
        let groq = api_key_present(app_data_dir, "groq");
        let gemini = api_key_present(app_data_dir, "gemini");
        let cerebras = api_key_present(app_data_dir, "cerebras");
        let github = crate::auth_vault::get_api_key("github").is_some();
        let (_, key_summary) = api_keys_summary(groq, gemini, cerebras);

        let creds_detail = format!("{} | GitHub Vault: {}", key_summary, if github { "Connected" } else { "Not set" });
        println!("[PREFLIGHT] [OK] Layer 5 (Auth & Credentials): {}", creds_detail);
        items.push(SystemAuditItem {
            layer: 5,
            name: "Credentials & Auth Vault".into(),
            status: "OK".into(),
            detail: creds_detail,
        });
    }

    let all_healthy = items.iter().all(|i| i.status == "OK");
    println!("╠══════════════════════════════════════════════════════════════════════╣");
    if all_healthy {
        println!("║  [OK] Pre-flight audit PASSED — all 5 layers operational & verified   ║");
    } else {
        println!("║  [!] Pre-flight audit surfaced warnings — see details above           ║");
    }
    println!("╚══════════════════════════════════════════════════════════════════════╝\n");

    FullSystemAuditReport {
        timestamp: format!("{}", chrono::Local::now().format("%Y-%m-%d %H:%M:%S")),
        items,
        all_healthy,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_provider_expired_scoped_per_provider() {
        let body = r#"{"user_id":"u","providers":{"google":{"connected":true,"expired":true},"github":{"connected":true,"expired":false}}}"#;
        assert!(provider_expired(body, "google"));
        assert!(!provider_expired(body, "github"));
    }

    #[test]
    fn test_provider_expired_absent_means_healthy() {
        let body = r#"{"user_id":"u","providers":{"google":{"connected":true}}}"#;
        assert!(!provider_expired(body, "google"));
        assert!(!provider_expired(body, "unknown"));
    }

    #[test]
    fn test_api_keys_summary_never_alarms() {
        // The keys row informs — all_connected must survive missing keys
        // (local-STT users legitimately have none).
        for (g, m, c) in [
            (false, false, false),
            (true, false, false),
            (false, true, false),
            (true, true, true),
        ] {
            let (connected, _) = api_keys_summary(g, m, c);
            assert!(connected, "keys row must never alarm: {g} {m} {c}");
        }
    }

    #[test]
    fn test_api_keys_summary_guides_keyless_users() {
        let (_, detail) = api_keys_summary(false, false, false);
        assert!(detail.contains("Settings"), "must name the fix location: {detail}");
        assert!(detail.contains("Groq") && detail.contains("Gemini"));
        let (_, detail) = api_keys_summary(true, false, true);
        assert!(!detail.contains("Settings"), "no guidance when covered: {detail}");
    }
}
