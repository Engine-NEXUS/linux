//! Action leases background watcher and manager.
//!
//! Checks periodically for expired action leases (e.g. 5-day block of a contact)
//! in `Store`. When a lease expires:
//! 1. Executes the unblock on the designated platform (WhatsApp / Instagram).
//! 2. Marks the lease status as 'completed'.
//! 3. Proactively notifies the user via the proactive policy.

use std::path::Path;
use std::time::Duration;
use tauri::{AppHandle, Manager, Runtime};

use super::store::{ActionLease, Store};

pub const TICK_SECS: u64 = 30;

/// List currently active leases.
pub fn list_active(dir: &Path) -> Vec<ActionLease> {
    super::with_store(dir, |s| {
        s.list_active_action_leases().unwrap_or_default()
    })
    .unwrap_or_default()
}

/// Cancel active leases for a specific person and platform.
pub fn cancel_leases_for(dir: &Path, target_name: &str, platform: &str) -> usize {
    super::with_store(dir, |s| {
        s.cancel_action_leases_for(target_name, platform).unwrap_or(0)
    })
    .unwrap_or(0)
}

/// Process any due action leases at the current timestamp. `exec` performs
/// the action and says whether it really worked; the lease is marked
/// `completed` or `failed` accordingly (a failed one is not retried every
/// tick, and the user is told).
pub fn process_due_leases(
    store: &Store,
    now_ts: i64,
    exec: impl Fn(&ActionLease) -> bool,
) -> Vec<(ActionLease, bool)> {
    let Ok(due) = store.get_due_action_leases(now_ts) else {
        return Vec::new();
    };

    let mut processed = Vec::new();
    for lease in due {
        let ok = exec(&lease);
        let _ = store.mark_action_lease_status(&lease.id, if ok { "completed" } else { "failed" });
        processed.push((lease, ok));
    }
    processed
}

/// Resolve a path to a project script, supporting both project-root and src-tauri working directories.
pub fn resolve_script_path(script_rel: &str) -> std::path::PathBuf {
    let p = std::path::PathBuf::from(script_rel);
    if p.exists() {
        return p;
    }
    let parent_p = std::path::PathBuf::from("..").join(script_rel);
    if parent_p.exists() {
        return parent_p;
    }
    if let Ok(manifest_dir) = std::env::var("CARGO_MANIFEST_DIR") {
        let root = std::path::Path::new(&manifest_dir).parent().unwrap_or(std::path::Path::new(&manifest_dir));
        let candidate = root.join(script_rel);
        if candidate.exists() {
            return candidate;
        }
    }
    p
}

/// Production executor for one due lease.
pub fn run_lease(lease: &ActionLease) -> bool {
    match lease.action_type.as_str() {
        "unblock" => execute_unblock(lease),
        "reminder" => execute_reminder(lease),
        other => {
            tracing::warn!("leases: unknown action_type '{other}'");
            false
        }
    }
}

/// Execute a voice reminder (the spoken part happens in `start`).
pub fn execute_reminder(lease: &ActionLease) -> bool {
    tracing::info!("leases: voice reminder triggered for '{}'", lease.target_name);
    true
}

/// Execute the unblock on the specific platform. Returns whether it worked.
pub fn execute_unblock(lease: &ActionLease) -> bool {
    match lease.platform.as_str() {
        "instagram" => {
            let script = resolve_script_path("scripts/instagram_client.py");
            if !script.exists() {
                tracing::warn!("leases: instagram helper script missing at {}", script.display());
                return false;
            }
            let out = std::process::Command::new("python")
                .args([script.to_string_lossy().as_ref(), "unblock", "--user", &lease.target_name])
                .output();
            match out {
                Ok(o) if o.status.success() => true,
                Ok(o) => {
                    tracing::warn!(
                        "leases: instagram unblock for '{}' failed: {}",
                        lease.target_name,
                        String::from_utf8_lossy(&o.stdout).trim()
                    );
                    false
                }
                Err(e) => {
                    tracing::warn!("leases: could not run python: {e}");
                    false
                }
            }
        }
        other => {
            tracing::warn!("leases: no unblock executor for platform '{other}'");
            false
        }
    }
}

/// Start background lease checker loop.
pub fn start<R: Runtime>(app: AppHandle<R>) {
    tauri::async_runtime::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_secs(TICK_SECS)).await;
            let Ok(dir) = app.path().app_data_dir() else { continue };
            if !super::enabled(&dir) {
                continue;
            }
            let now = chrono::Utc::now().timestamp();
            let dir2 = dir.clone();
            // Python can take seconds: keep it off the async worker thread.
            let processed = tokio::task::spawn_blocking(move || {
                super::with_store(&dir2, |s| process_due_leases(s, now, run_lease)).unwrap_or_default()
            })
            .await
            .unwrap_or_default();

            for (lease, ok) in processed {
                let text = if lease.action_type == "reminder" {
                    format!("Sir, here is your reminder: {}.", lease.target_name)
                } else if ok {
                    format!("Sir, the temporary block for {} on {} has expired. I have unblocked them.", lease.target_name, lease.platform)
                } else {
                    format!("Sir, the block for {} on {} was due to end, but I couldn't unblock them. Please check it.", lease.target_name, lease.platform)
                };
                crate::proactive_policy::submit(&app, format!("lease_expired_{}", lease.id), text, crate::google::types::AlertUrgency::Medium);
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn process_due_leases_handles_expiry() {
        let store = Store::open_in_memory().unwrap();
        let now = 1_700_000_000i64;

        let lease = ActionLease {
            id: "lease_test_due".into(),
            action_type: "unblock".into(),
            target_name: "Prem Sai".into(),
            platform: "instagram".into(),
            target_identifier: "premsai_ig".into(),
            expires_at: now - 10,
            created_at: now - 100,
            status: "active".into(),
        };

        store.create_action_lease(&lease).unwrap();

        // Injected executor: the test must never touch a real Instagram session.
        let processed = process_due_leases(&store, now, |_| true);
        assert_eq!(processed.len(), 1);
        assert_eq!(processed[0].0.id, "lease_test_due");
        assert!(processed[0].1);

        // After processing, status is completed and no longer due
        let remaining = store.get_due_action_leases(now).unwrap();
        assert!(remaining.is_empty());
    }

    #[test]
    fn failed_unblock_is_reported_and_not_retried_every_tick() {
        let store = Store::open_in_memory().unwrap();
        let now = 1_700_000_000i64;
        let lease = ActionLease {
            id: "lease_fail".into(),
            action_type: "unblock".into(),
            target_name: "Someone".into(),
            platform: "instagram".into(),
            target_identifier: "someone".into(),
            expires_at: now - 10,
            created_at: now - 100,
            status: "active".into(),
        };
        store.create_action_lease(&lease).unwrap();
        let processed = process_due_leases(&store, now, |_| false);
        assert_eq!(processed.len(), 1);
        assert!(!processed[0].1, "a failed unblock must not be reported as done");
        assert!(store.get_due_action_leases(now).unwrap().is_empty());
        assert!(process_due_leases(&store, now, |_| true).is_empty());
    }

    #[test]
    fn unsupported_platform_unblock_is_not_a_success() {
        let lease = ActionLease {
            id: "x".into(),
            action_type: "unblock".into(),
            target_name: "A".into(),
            platform: "whatsapp".into(),
            target_identifier: "a".into(),
            expires_at: 0,
            created_at: 0,
            status: "active".into(),
        };
        assert!(!run_lease(&lease));
    }
}
