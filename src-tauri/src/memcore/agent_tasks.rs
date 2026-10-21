//! Small, local record of agent tasks for return reviews.
//!
//! The task record deliberately contains no prompt, transcript, or terminal
//! output. It uses Memory Core metadata (sealed where the OS key store is
//! available) and is never included in the cloud context pack.

use serde::{Deserialize, Serialize};
use std::path::Path;

const META_KEY: &str = "agent_tasks_v1";
const MAX_TASKS: usize = 80;
const RETENTION_SECS: i64 = 30 * 24 * 60 * 60;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Working,
    ResponseReady,
    PossiblyReady,
    NeedsAttention,
    Failed,
    Stopped,
    Unconfirmed,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AgentTask {
    pub id: String,
    pub provider: String,
    pub title: String,
    pub project: Option<String>,
    pub owned_by_nexus: bool,
    pub status: Status,
    pub evidence: String,
    pub started_at: i64,
    pub observed_at: i64,
    #[serde(default)]
    pub last_announced_at: Option<i64>,
}

fn now() -> i64 {
    chrono::Utc::now().timestamp()
}

fn read_from(store: &super::store::Store) -> Vec<AgentTask> {
    store
        .meta_get(META_KEY)
        .and_then(|raw| serde_json::from_str::<Vec<AgentTask>>(&raw).ok())
        .unwrap_or_default()
}

fn write_to(store: &super::store::Store, tasks: &[AgentTask]) {
    if let Ok(raw) = serde_json::to_string(tasks) {
        store.meta_set(META_KEY, &raw);
    }
}

fn prune(tasks: &mut Vec<AgentTask>, timestamp: i64) {
    tasks.retain(|task| timestamp.saturating_sub(task.observed_at) <= RETENTION_SECS);
    tasks.sort_by(|a, b| b.observed_at.cmp(&a.observed_at));
    tasks.truncate(MAX_TASKS);
}

pub fn list(dir: &Path) -> Vec<AgentTask> {
    super::with_store(dir, read_from).unwrap_or_default()
}

pub fn add(dir: &Path, id: String, provider: String, project: Option<String>, owned_by_nexus: bool) {
    let timestamp = now();
    let title = if owned_by_nexus {
        format!("{provider} task")
    } else {
        format!("{provider} window")
    };
    let status = if owned_by_nexus { Status::Working } else { Status::Unconfirmed };
    let evidence = if owned_by_nexus { "nexus_spawned" } else { "foreground_only" };
    super::with_store(dir, |store| {
        let mut tasks = read_from(store);
        tasks.retain(|task| task.id != id);
        tasks.push(AgentTask {
            id,
            provider,
            title,
            project,
            owned_by_nexus,
            status,
            evidence: evidence.into(),
            started_at: timestamp,
            observed_at: timestamp,
            last_announced_at: None,
        });
        prune(&mut tasks, timestamp);
        write_to(store, &tasks);
    });
}

/// Record an observation. A finished response cannot become "working" again
/// because of an older sidecar poll.
pub fn observe(dir: &Path, id: &str, status: Status, evidence: &str) {
    let timestamp = now();
    super::with_store(dir, |store| {
        let mut tasks = read_from(store);
        let Some(task) = tasks.iter_mut().find(|task| task.id == id) else { return };
        if matches!(task.status, Status::ResponseReady | Status::PossiblyReady | Status::Failed | Status::Stopped)
            && matches!(status, Status::Working | Status::Unconfirmed)
        {
            return;
        }
        // A three-second sidecar poll is not a meaningful journal revision.
        // Keep the last-check timestamp reasonably fresh without sealing and
        // rewriting the entire metadata value on every poll.
        if task.status == status && task.evidence == evidence
            && timestamp.saturating_sub(task.observed_at) < 30
        {
            return;
        }
        let revision_time = timestamp.max(task.observed_at.saturating_add(1));
        task.status = status;
        task.evidence = evidence.chars().take(80).collect();
        task.observed_at = revision_time;
        prune(&mut tasks, timestamp);
        write_to(store, &tasks);
    });
}

/// A local process from the previous NEXUS run cannot be called running until
/// a live adapter confirms its identity and status again.
pub fn mark_previous_run_unconfirmed(dir: &Path) {
    let timestamp = now();
    super::with_store(dir, |store| {
        let mut tasks = read_from(store);
        for task in &mut tasks {
            if task.owned_by_nexus && matches!(task.status, Status::Working | Status::NeedsAttention) {
                task.status = Status::Unconfirmed;
                task.evidence = "nexus_restarted".into();
                task.observed_at = timestamp;
            }
        }
        prune(&mut tasks, timestamp);
        write_to(store, &tasks);
    });
}

pub fn spoken_review(tasks: &[AgentTask]) -> Option<String> {
    let mut ordered = tasks.to_vec();
    ordered.sort_by_key(|task| match task.status {
        Status::NeedsAttention => 0,
        Status::ResponseReady => 1,
        Status::PossiblyReady => 2,
        Status::Failed => 3,
        Status::Working => 4,
        Status::Unconfirmed => 5,
        Status::Stopped => 6,
    });
    let lines: Vec<String> = ordered.iter().take(3).map(|task| {
        let name = task.provider.chars().take(32).collect::<String>();
        match task.status {
            Status::ResponseReady => format!("{name} has replied"),
            Status::PossiblyReady => format!("{name} may have replied; please verify"),
            Status::NeedsAttention => format!("{name} needs your attention"),
            Status::Failed => format!("{name} ran into an error"),
            Status::Working => format!("{name} was working when I last checked"),
            Status::Unconfirmed => format!("I cannot confirm {name}'s current status"),
            Status::Stopped => format!("{name} was stopped"),
        }
    }).collect();
    if lines.is_empty() { None } else { Some(format!("{}.", lines.join(". "))) }
}

/// Active tasks remain visible on every return. A terminal result is spoken
/// only when its latest observation has not yet been announced.
pub fn due_for_review(tasks: &[AgentTask]) -> Vec<AgentTask> {
    tasks.iter().filter(|task| {
        matches!(task.status, Status::Working | Status::NeedsAttention | Status::Unconfirmed)
            || task.last_announced_at.map(|at| at < task.observed_at).unwrap_or(true)
    }).cloned().collect()
}

/// A speech receipt acknowledges only the exact revisions used to build it.
/// A task that changed while NEXUS was speaking stays due for the next review.
pub(crate) fn mark_announced_in(store: &super::store::Store, revisions: &[(String, i64)]) {
    if revisions.is_empty() { return; }
    let mut tasks = read_from(store);
    let mut changed = false;
    for task in &mut tasks {
        if revisions.iter().any(|(id, observed)| id == &task.id && *observed == task.observed_at) {
            task.last_announced_at = Some(task.observed_at);
            changed = true;
        }
    }
    if changed { write_to(store, &tasks); }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn review_never_calls_unconfirmed_task_finished() {
        let task = AgentTask {
            id: "1".into(), provider: "Antigravity".into(), title: "task".into(), project: None,
            owned_by_nexus: false, status: Status::Unconfirmed, evidence: "foreground_only".into(),
            started_at: 1, observed_at: 1, last_announced_at: None,
        };
        assert_eq!(spoken_review(&[task]), Some("I cannot confirm Antigravity's current status.".into()));
    }

    #[test]
    fn terminal_revision_is_spoken_only_once_but_working_remains_visible() {
        let mut task = AgentTask {
            id: "1".into(), provider: "Claude".into(), title: "task".into(), project: None,
            owned_by_nexus: true, status: Status::ResponseReady, evidence: "sidecar_done".into(),
            started_at: 1, observed_at: 5, last_announced_at: None,
        };
        assert_eq!(due_for_review(&[task.clone()]).len(), 1);
        task.last_announced_at = Some(5);
        assert!(due_for_review(&[task.clone()]).is_empty());
        task.status = Status::Working;
        assert_eq!(due_for_review(&[task]).len(), 1);
    }

    #[test]
    fn adapter_pattern_is_only_a_possible_response() {
        let task = AgentTask {
            id: "2".into(), provider: "Codex".into(), title: "task".into(), project: None,
            owned_by_nexus: true, status: Status::PossiblyReady, evidence: "adapter_pattern".into(),
            started_at: 1, observed_at: 2, last_announced_at: None,
        };
        assert_eq!(spoken_review(&[task]), Some("Codex may have replied; please verify.".into()));
    }
}
