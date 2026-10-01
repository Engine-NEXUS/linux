//! NEXUS GitHub Sub-Command System (Phase 2A)
//!
//! This module is a sub-architecture under the central orchestrator. It owns
//! ALL GitHub operations that previously went through the Worker's regex-based
//! `handleGitHubWrite()`. Instead of regex soup, every operation is a typed
//! variant of `GitHubCommand` implementing the `GitHubExecutable` trait.
//!
//! Architecture:
//!
//! ```text
//!   Orchestrator
//!     └── Subsystem::GitHub
//!           └── github_cmd::process()
//!                 ├── parse_github_command()  → GitHubCommand enum
//!                 ├── validate()              → scope/permission check
//!                 ├── pre_check()             → conflict detection
//!                 ├── is_destructive()?       → confirmation flow
//!                 └── execute()               → octocrab API call
//! ```
//!
//! Token: fetched from the Worker via `GET /oauth/github-token?user_id=...`
//! and cached in memory for the session. The client_secret never leaves
//! the Worker.
//!
//! Why octocrab: 16.8M downloads, actively maintained, typed semantic API
//! + low-level HTTP fallback for endpoints not yet covered.

use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;

// ─── Token Management ──────────────────────────────────────────────────

/// Cached GitHub token. Fetched from the Worker on first use, refreshed
/// when expired. Stored in memory only — never written to disk.
static GITHUB_TOKEN: once_cell::sync::Lazy<Arc<RwLock<Option<CachedToken>>>> =
    once_cell::sync::Lazy::new(|| Arc::new(RwLock::new(None)));

#[allow(dead_code)]
struct CachedToken {
    token: String,
    /// When the token expires (unix timestamp). 0 = no expiry (classic OAuth).
    expires_at: f64,
    /// When we fetched the token (unix timestamp).
    fetched_at: f64,
}

impl CachedToken {
    /// Is the token still valid (or classic with no expiry)?
    fn is_valid(&self) -> bool {
        if self.expires_at == 0.0 {
            return true;
        }
        let now = chrono::Utc::now().timestamp() as f64;
        // Refresh 5 minutes before expiry
        now < self.expires_at - 300.0
    }
}

/// Fetch the GitHub token from the Worker's `/oauth/github-token` endpoint.
/// The Worker handles refresh logic internally — we just get a valid token.
async fn fetch_token_from_worker(
    worker_url: &str,
    user_id: &str,
) -> Result<String, String> {
    let url = format!(
        "{}/oauth/github-token?user_id={}",
        worker_url.trim_end_matches('/'),
        user_id
    );
    tracing::info!("github_cmd: fetching token from worker: {}", url);

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .map_err(|e| format!("http client: {e}"))?;

    let resp = client
        .get(&url)
        .send()
        .await
        .map_err(|e| format!("token fetch: {e}"))?;

    if resp.status() == 404 {
        return Err("GitHub not connected. Please connect GitHub in the NEXUS setup.".into());
    }
    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("token fetch error {status}: {body}"));
    }

    let data: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("token json: {e}"))?;

    let token = data["token"]
        .as_str()
        .ok_or("token field missing in worker response")?
        .to_string();

    Ok(token)
}

/// Get a valid GitHub token, fetching from the Worker if needed.
/// Uses the cached token if it's still valid.
pub async fn get_github_token(worker_url: &str, user_id: &str) -> Result<String, String> {
    // Check cache first
    {
        let guard = GITHUB_TOKEN.read().await;
        if let Some(ref cached) = *guard {
            if cached.is_valid() {
                return Ok(cached.token.clone());
            }
        }
    }

    // Fetch new token
    let token = fetch_token_from_worker(worker_url, user_id).await?;
    let now = chrono::Utc::now().timestamp() as f64;

    // Cache it (we don't know the exact expiry from the Worker response,
    // so we assume 1 hour. The Worker's getValidGithubToken() handles
    // refresh on its end, and we re-fetch if our cache is stale.)
    {
        let mut guard = GITHUB_TOKEN.write().await;
        *guard = Some(CachedToken {
            token: token.clone(),
            expires_at: now + 3600.0, // 1 hour
            fetched_at: now,
        });
    }

    Ok(token)
}

/// Clear the cached token (e.g., when GitHub is disconnected).
pub async fn clear_github_token() {
    let mut guard = GITHUB_TOKEN.write().await;
    *guard = None;
}

/// Build an octocrab client authenticated with the user's GitHub token.
async fn build_octocrab_client(worker_url: &str, user_id: &str) -> Result<octocrab::Octocrab, String> {
    let token = get_github_token(worker_url, user_id).await?;
    octocrab::Octocrab::builder()
        .personal_token(token)
        .build()
        .map_err(|e| format!("octocrab client: {e}"))
}

// ─── Command Enum ──────────────────────────────────────────────────────

/// Merge method for PR merges.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum MergeMethod {
    Merge,
    Squash,
    Rebase,
}

impl Default for MergeMethod {
    fn default() -> Self {
        MergeMethod::Squash
    }
}

/// Permission level for collaborators.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum CollaboratorPermission {
    Pull,
    Triage,
    Push,
    Maintain,
    Admin,
}

impl Default for CollaboratorPermission {
    fn default() -> Self {
        CollaboratorPermission::Push
    }
}

/// Organization role for members.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum OrgRole {
    Member,
    Admin,
}

impl Default for OrgRole {
    fn default() -> Self {
        OrgRole::Member
    }
}

/// The structured GitHub command. Every GitHub operation the user can
/// perform is a variant of this enum. The orchestrator routes to
/// `Subsystem::GitHub` and calls `github_cmd::execute_command()`.
///
/// This is the idiomatic Rust command pattern: a closed set of variants
/// with exhaustive matching, serializable for logging/replay, and each
/// carrying exactly the data it needs.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "command")]
#[serde(rename_all = "snake_case")]
pub enum GitHubCommand {
    // ─── PR Operations (Phase 2A-1: 5 core commands) ───────────────
    /// Merge a PR. Destructive — requires confirmation.
    /// Pre-checks mergeable_state; if "dirty", returns conflict info.
    MergePr {
        repo: String, // "owner/repo"
        pr_number: u64,
        #[serde(default)]
        method: MergeMethod,
    },
    /// Approve a PR (submit a review with APPROVE event).
    ApprovePr {
        repo: String,
        pr_number: u64,
    },
    /// Close a PR. Destructive — requires confirmation.
    ClosePr {
        repo: String,
        pr_number: u64,
    },
    /// List open PRs in a repo.
    ListPrs {
        repo: String,
        #[serde(default = "default_pr_state")]
        state: String, // "open", "closed", "all"
    },
    /// Get details of a specific PR.
    GetPr {
        repo: String,
        pr_number: u64,
    },

    // ─── PR Operations (Phase 2A-3: 5 more commands) ───────────────
    /// Create a new PR. Not destructive (creates, doesn't modify/delete).
    CreatePr {
        repo: String,
        title: String,
        head: String,  // source branch
        base: String,  // target branch
        #[serde(default)]
        body: Option<String>,
        #[serde(default)]
        draft: bool,
    },
    /// Update the PR branch with the latest changes from the base branch.
    /// Not destructive — triggers GitHub's "Update branch" operation.
    UpdateBranch {
        repo: String,
        pr_number: u64,
    },
    /// Revert a merged PR by creating a new PR that reverts the changes.
    /// Destructive (in the sense that it reverses merged work) — requires confirmation.
    RevertPr {
        repo: String,
        pr_number: u64,
        #[serde(default)]
        title: Option<String>,
    },
    /// List the files changed in a PR.
    ListPrFiles {
        repo: String,
        pr_number: u64,
    },
    /// Comment on a PR. Not destructive.
    CommentPr {
        repo: String,
        pr_number: u64,
        body: String,
    },

    // ─── Collaborator + Organization Operations (Phase 2A-4) ───────
    /// Add a user as a collaborator to a repo. Destructive (modifies permissions).
    AddCollaborator {
        repo: String,
        username: String,
        #[serde(default)]
        permission: CollaboratorPermission,
    },
    /// Remove a collaborator from a repo. Destructive.
    RemoveCollaborator {
        repo: String,
        username: String,
    },
    /// List collaborators on a repo.
    ListCollaborators {
        repo: String,
    },
    /// Add a member to an organization. Destructive (modifies org membership).
    AddOrgMember {
        org: String,
        username: String,
        #[serde(default)]
        role: OrgRole,
    },
    /// Remove a member from an organization. Destructive.
    RemoveOrgMember {
        org: String,
        username: String,
    },
    /// List members of an organization.
    ListOrgMembers {
        org: String,
    },
    /// Convert an org member to an outside collaborator. Destructive.
    ConvertToOutsideCollaborator {
        org: String,
        username: String,
    },
    /// List outside collaborators of an organization.
    ListOutsideCollaborators {
        org: String,
    },

    // ─── Branch Operations (Phase 2A-5) ────────────────────────────
    /// Set branch protection rules. Destructive (modifies protection settings).
    SetBranchProtection {
        repo: String,
        branch: String,
        /// Required status checks (empty = none required)
        #[serde(default)]
        required_status_checks: Vec<String>,
        /// Require PR reviews before merge
        #[serde(default)]
        require_pr_reviews: bool,
        /// Required number of approving reviews
        #[serde(default)]
        required_review_count: u8,
        /// Enforce admins (apply rules to admins too)
        #[serde(default)]
        enforce_admins: bool,
    },
    /// Delete a branch. Destructive.
    DeleteBranch {
        repo: String,
        branch: String,
    },
    /// List branches in a repo.
    ListBranches {
        repo: String,
    },

    // ─── Release Operations (Phase 2A-5) ───────────────────────────
    /// Create a release. Not destructive (creates new).
    CreateRelease {
        repo: String,
        tag: String,
        name: String,
        #[serde(default)]
        body: Option<String>,
        #[serde(default)]
        draft: bool,
        #[serde(default)]
        prerelease: bool,
        #[serde(default)]
        target_commitish: Option<String>,
    },
    /// List releases in a repo.
    ListReleases {
        repo: String,
    },
    /// Delete a release. Destructive.
    DeleteRelease {
        repo: String,
        release_id: u64,
    },

    // ─── Workflow Operations (Phase 2A-5) ──────────────────────────
    /// List GitHub Actions workflows in a repo.
    ListWorkflows {
        repo: String,
    },
    /// List workflow runs for a repo (optionally filtered by workflow file).
    ListWorkflowRuns {
        repo: String,
        #[serde(default)]
        workflow_file: Option<String>, // e.g. "ci.yml"
    },
    /// Rerun a failed workflow run. Not destructive (re-triggers).
    RerunWorkflow {
        repo: String,
        run_id: u64,
    },
    /// Cancel a running workflow. Destructive (stops in-progress work).
    CancelWorkflow {
        repo: String,
        run_id: u64,
    },
}

fn default_pr_state() -> String {
    "open".to_string()
}

// ─── Result Types ──────────────────────────────────────────────────────

/// The result of executing a GitHub command. This is what gets spoken
/// to the user and/or displayed in the frontend.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type")]
#[serde(rename_all = "snake_case")]
pub enum GitHubResult {
    /// Simple text result — spoken to the user.
    Text {
        text: String,
    },
    /// Structured PR list — rendered in the sidebar as cards with
    /// Merge and Analyse buttons. Not spoken via TTS (visual-only).
    PrList {
        repo: String,
        state: String,
        prs: Vec<PrSummary>,
    },
    /// Destructive operation needs confirmation.
    /// The orchestrator should ask the user to confirm, then re-execute
    /// with `confirmed: true`.
    NeedsConfirmation {
        prompt: String,
        command: GitHubCommand,
    },
    /// Merge conflict detected — don't attempt the merge.
    /// The frontend should display the conflict details with copy-paste options.
    MergeConflict {
        pr_number: u64,
        repo: String,
        conflict_files: Vec<ConflictFile>,
        message: String,
    },
    /// Error from the GitHub API or local validation.
    Error {
        message: String,
        /// GitHub API status code (if applicable)
        status: Option<u16>,
        /// Whether this is a token/permission error (user should reconnect)
        is_auth_error: bool,
    },
}

/// A summary of a single PR for list display in the sidebar.
#[derive(Debug, Clone, Serialize)]
pub struct PrSummary {
    pub number: u64,
    pub title: String,
    pub author: String,
    pub repo: String,
    pub state: String,
    /// ISO 8601 timestamp of PR creation
    pub created_at: String,
    /// Whether the PR is mergeable (null = unknown, true/false = GitHub's answer)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mergeable: Option<bool>,
}

/// A file with merge conflicts.
#[derive(Debug, Clone, Serialize)]
pub struct ConflictFile {
    pub filename: String,
    /// Number of conflict blocks in this file.
    pub conflict_count: usize,
    /// The conflict blocks with HEAD and branch versions.
    pub blocks: Vec<ConflictBlock>,
}

/// A single conflict block (<<<<<<< ======= >>>>>>>).
#[derive(Debug, Clone, Serialize)]
pub struct ConflictBlock {
    /// Line number where the conflict starts.
    pub start_line: usize,
    /// Content from the HEAD (base) branch.
    pub head_content: String,
    /// Content from the feature branch.
    pub branch_content: String,
}

// ─── Trait: GitHubExecutable ───────────────────────────────────────────

/// Every GitHubCommand variant implements this trait.
/// The trait provides:
///   - `is_destructive()` — does this command modify state?
///   - `confirmation_prompt()` — what to ask the user before executing
///   - `required_scopes()` — what OAuth scopes are needed
///   - `pre_check()` — validate before executing (e.g., check mergeable state)
///   - `execute()` — the actual API call via octocrab
#[async_trait::async_trait]
pub trait GitHubExecutable {
    /// Does this command modify GitHub state? Destructive commands
    /// require user confirmation before execution.
    fn is_destructive(&self) -> bool;

    /// The confirmation prompt for destructive commands.
    /// Returns None for non-destructive commands.
    fn confirmation_prompt(&self) -> Option<String>;

    /// Required OAuth scopes for this command.
    fn required_scopes(&self) -> &'static [&'static str] {
        &["repo"]
    }

    /// Pre-check before execution. For merge commands, this checks
    /// mergeable_state and returns a MergeConflict result if dirty.
    /// Returns Ok(()) if safe to proceed, Err(GitHubResult) if not.
    async fn pre_check(
        &self,
        _client: &octocrab::Octocrab,
    ) -> Result<(), GitHubResult> {
        Ok(())
    }

    /// Execute the command against the GitHub API.
    async fn execute(
        &self,
        client: &octocrab::Octocrab,
    ) -> GitHubResult;
}

// ─── Trait Implementations ─────────────────────────────────────────────

#[async_trait::async_trait]
impl GitHubExecutable for GitHubCommand {
    fn is_destructive(&self) -> bool {
        match self {
            GitHubCommand::MergePr { .. } => true,
            GitHubCommand::ClosePr { .. } => true,
            GitHubCommand::RevertPr { .. } => true,
            GitHubCommand::AddCollaborator { .. } => true,
            GitHubCommand::RemoveCollaborator { .. } => true,
            GitHubCommand::AddOrgMember { .. } => true,
            GitHubCommand::RemoveOrgMember { .. } => true,
            GitHubCommand::ConvertToOutsideCollaborator { .. } => true,
            GitHubCommand::ApprovePr { .. } => false,
            GitHubCommand::ListPrs { .. } => false,
            GitHubCommand::GetPr { .. } => false,
            GitHubCommand::CreatePr { .. } => false,
            GitHubCommand::UpdateBranch { .. } => false,
            GitHubCommand::ListPrFiles { .. } => false,
            GitHubCommand::CommentPr { .. } => false,
            GitHubCommand::ListCollaborators { .. } => false,
            GitHubCommand::ListOrgMembers { .. } => false,
            GitHubCommand::ListOutsideCollaborators { .. } => false,
            GitHubCommand::SetBranchProtection { .. } => true,
            GitHubCommand::DeleteBranch { .. } => true,
            GitHubCommand::ListBranches { .. } => false,
            GitHubCommand::CreateRelease { .. } => false,
            GitHubCommand::ListReleases { .. } => false,
            GitHubCommand::DeleteRelease { .. } => true,
            GitHubCommand::ListWorkflows { .. } => false,
            GitHubCommand::ListWorkflowRuns { .. } => false,
            GitHubCommand::RerunWorkflow { .. } => false,
            GitHubCommand::CancelWorkflow { .. } => true,
        }
    }

    fn confirmation_prompt(&self) -> Option<String> {
        match self {
            GitHubCommand::MergePr { repo, pr_number, method } => Some(format!(
                "Are you sure you want to {} merge PR #{} in {}? Say yes to confirm.",
                match method {
                    MergeMethod::Merge => "merge",
                    MergeMethod::Squash => "squash",
                    MergeMethod::Rebase => "rebase",
                },
                pr_number,
                repo
            )),
            GitHubCommand::ClosePr { repo, pr_number } => Some(format!(
                "Are you sure you want to close PR #{} in {}? Say yes to confirm.",
                pr_number, repo
            )),
            GitHubCommand::RevertPr { repo, pr_number, .. } => Some(format!(
                "Are you sure you want to revert PR #{} in {}? This will create a new PR that undoes the changes. Say yes to confirm.",
                pr_number, repo
            )),
            GitHubCommand::AddCollaborator { repo, username, permission } => Some(format!(
                "Are you sure you want to add {} as a collaborator to {} with {} permission? Say yes to confirm.",
                username, repo, serde_json::to_string(permission).unwrap_or_default().trim_matches('"')
            )),
            GitHubCommand::RemoveCollaborator { repo, username } => Some(format!(
                "Are you sure you want to remove {} as a collaborator from {}? Say yes to confirm.",
                username, repo
            )),
            GitHubCommand::AddOrgMember { org, username, role } => Some(format!(
                "Are you sure you want to add {} to the {} organization as {}? Say yes to confirm.",
                username, org, serde_json::to_string(role).unwrap_or_default().trim_matches('"')
            )),
            GitHubCommand::RemoveOrgMember { org, username } => Some(format!(
                "Are you sure you want to remove {} from the {} organization? Say yes to confirm.",
                username, org
            )),
            GitHubCommand::ConvertToOutsideCollaborator { org, username } => Some(format!(
                "Are you sure you want to convert {} to an outside collaborator in {}? Say yes to confirm.",
                username, org
            )),
            GitHubCommand::SetBranchProtection { repo, branch, .. } => Some(format!(
                "Are you sure you want to set branch protection on {} in {}? Say yes to confirm.",
                branch, repo
            )),
            GitHubCommand::DeleteBranch { repo, branch } => Some(format!(
                "Are you sure you want to delete branch {} in {}? This cannot be undone. Say yes to confirm.",
                branch, repo
            )),
            GitHubCommand::DeleteRelease { repo, release_id } => Some(format!(
                "Are you sure you want to delete release {} in {}? Say yes to confirm.",
                release_id, repo
            )),
            GitHubCommand::CancelWorkflow { repo, run_id } => Some(format!(
                "Are you sure you want to cancel workflow run {} in {}? Say yes to confirm.",
                run_id, repo
            )),
            _ => None,
        }
    }

    async fn pre_check(
        &self,
        client: &octocrab::Octocrab,
    ) -> Result<(), GitHubResult> {
        match self {
            GitHubCommand::MergePr { repo, pr_number, .. } => {
                // Check mergeable_state before attempting merge
                let (owner, repo_name) = split_repo(repo);
                let pr = client
                    .pulls(owner, repo_name)
                    .get(*pr_number)
                    .await
                    .map_err(|e| GitHubResult::Error {
                        message: format!("Failed to fetch PR for conflict check: {e}"),
                        status: None,
                        is_auth_error: false,
                    })?;

                let mergeable = pr.mergeable.unwrap_or(false);
                let mergeable_state = pr.mergeable_state.unwrap_or(octocrab::models::pulls::MergeableState::Unknown);

                if !mergeable || matches!(mergeable_state, octocrab::models::pulls::MergeableState::Dirty) {
                    // Fetch conflict details
                    let conflict_files = fetch_conflict_details(client, repo, *pr_number).await;
                    return Err(GitHubResult::MergeConflict {
                        pr_number: *pr_number,
                        repo: repo.clone(),
                        conflict_files,
                        message: format!(
                            "PR #{} has merge conflicts and cannot be merged automatically.",
                            pr_number
                        ),
                    });
                }

                Ok(())
            }
            _ => Ok(()),
        }
    }

    async fn execute(
        &self,
        client: &octocrab::Octocrab,
    ) -> GitHubResult {
        match self {
            GitHubCommand::MergePr { repo, pr_number, method } => {
                execute_merge_pr(client, repo, *pr_number, method).await
            }
            GitHubCommand::ApprovePr { repo, pr_number } => {
                execute_approve_pr(client, repo, *pr_number).await
            }
            GitHubCommand::ClosePr { repo, pr_number } => {
                execute_close_pr(client, repo, *pr_number).await
            }
            GitHubCommand::ListPrs { repo, state } => {
                execute_list_prs(client, repo, state).await
            }
            GitHubCommand::GetPr { repo, pr_number } => {
                execute_get_pr(client, repo, *pr_number).await
            }
            GitHubCommand::CreatePr {
                repo,
                title,
                head,
                base,
                body,
                draft,
            } => {
                execute_create_pr(client, repo, title, head, base, body.as_deref(), *draft).await
            }
            GitHubCommand::UpdateBranch { repo, pr_number } => {
                execute_update_branch(client, repo, *pr_number).await
            }
            GitHubCommand::RevertPr {
                repo,
                pr_number,
                title,
            } => {
                execute_revert_pr(client, repo, *pr_number, title.as_deref()).await
            }
            GitHubCommand::ListPrFiles { repo, pr_number } => {
                execute_list_pr_files(client, repo, *pr_number).await
            }
            GitHubCommand::CommentPr {
                repo,
                pr_number,
                body,
            } => {
                execute_comment_pr(client, repo, *pr_number, body).await
            }
            GitHubCommand::AddCollaborator {
                repo,
                username,
                permission,
            } => {
                execute_add_collaborator(client, repo, username, permission).await
            }
            GitHubCommand::RemoveCollaborator { repo, username } => {
                execute_remove_collaborator(client, repo, username).await
            }
            GitHubCommand::ListCollaborators { repo } => {
                execute_list_collaborators(client, repo).await
            }
            GitHubCommand::AddOrgMember {
                org,
                username,
                role,
            } => {
                execute_add_org_member(client, org, username, role).await
            }
            GitHubCommand::RemoveOrgMember { org, username } => {
                execute_remove_org_member(client, org, username).await
            }
            GitHubCommand::ListOrgMembers { org } => {
                execute_list_org_members(client, org).await
            }
            GitHubCommand::ConvertToOutsideCollaborator { org, username } => {
                execute_convert_to_outside_collaborator(client, org, username).await
            }
            GitHubCommand::ListOutsideCollaborators { org } => {
                execute_list_outside_collaborators(client, org).await
            }
            GitHubCommand::SetBranchProtection {
                repo,
                branch,
                required_status_checks,
                require_pr_reviews,
                required_review_count,
                enforce_admins,
            } => {
                execute_set_branch_protection(
                    client,
                    repo,
                    branch,
                    required_status_checks,
                    *require_pr_reviews,
                    *required_review_count,
                    *enforce_admins,
                )
                .await
            }
            GitHubCommand::DeleteBranch { repo, branch } => {
                execute_delete_branch(client, repo, branch).await
            }
            GitHubCommand::ListBranches { repo } => {
                execute_list_branches(client, repo).await
            }
            GitHubCommand::CreateRelease {
                repo,
                tag,
                name,
                body,
                draft,
                prerelease,
                target_commitish,
            } => {
                execute_create_release(
                    client,
                    repo,
                    tag,
                    name,
                    body.as_deref(),
                    *draft,
                    *prerelease,
                    target_commitish.as_deref(),
                )
                .await
            }
            GitHubCommand::ListReleases { repo } => {
                execute_list_releases(client, repo).await
            }
            GitHubCommand::DeleteRelease { repo, release_id } => {
                execute_delete_release(client, repo, *release_id).await
            }
            GitHubCommand::ListWorkflows { repo } => {
                execute_list_workflows(client, repo).await
            }
            GitHubCommand::ListWorkflowRuns {
                repo,
                workflow_file,
            } => {
                execute_list_workflow_runs(client, repo, workflow_file.as_deref()).await
            }
            GitHubCommand::RerunWorkflow { repo, run_id } => {
                execute_rerun_workflow(client, repo, *run_id).await
            }
            GitHubCommand::CancelWorkflow { repo, run_id } => {
                execute_cancel_workflow(client, repo, *run_id).await
            }
        }
    }
}

// ─── Command Implementations ───────────────────────────────────────────

/// Split "owner/repo" into ("owner", "repo").
/// If the string doesn't contain "/", returns ("", full_string).
fn split_repo(repo: &str) -> (&str, &str) {
    match repo.split_once('/') {
        Some((owner, name)) => (owner, name),
        None => ("", repo),
    }
}

/// Map an octocrab error to a GitHubResult::Error.
fn map_octocrab_error(e: octocrab::Error, context: &str) -> GitHubResult {
    let (status, is_auth) = match &e {
        octocrab::Error::GitHub { source, .. } => {
            let code = source.status_code.as_u16();
            let is_auth = code == 401 || code == 403;
            (Some(code), is_auth)
        }
        octocrab::Error::Hyper { .. } => (None, false),
        _ => (None, false),
    };

    let message = if is_auth {
        format!("Your GitHub token has expired or lacks permissions. Please reconnect GitHub in the NEXUS setup to {}.", context)
    } else {
        format!("Error trying to {}: {}", context, e)
    };

    GitHubResult::Error {
        message,
        status,
        is_auth_error: is_auth,
    }
}

// ─── Merge PR ──────────────────────────────────────────────────────────

async fn execute_merge_pr(
    client: &octocrab::Octocrab,
    repo: &str,
    pr_number: u64,
    method: &MergeMethod,
) -> GitHubResult {
    let (owner, repo_name) = split_repo(repo);

    // Fetch PR info for the commit title
    let pr = match client.pulls(owner, repo_name).get(pr_number).await {
        Ok(pr) => pr,
        Err(e) => return map_octocrab_error(e, &format!("fetch PR #{} for merge", pr_number)),
    };

    let commit_title = pr
        .title
        .clone()
        .unwrap_or_else(|| format!("Merge PR #{}", pr_number));

    // Perform the merge using the low-level API (octocrab's typed merge
    // API may not support all options, so we use the HTTP method directly).
    let merge_method_str = match method {
        MergeMethod::Merge => "merge",
        MergeMethod::Squash => "squash",
        MergeMethod::Rebase => "rebase",
    };

    let url = format!("/repos/{}/{}/pulls/{}/merge", owner, repo_name, pr_number);
    let body = serde_json::json!({
        "commit_title": commit_title,
        "merge_method": merge_method_str,
    });

    let result: Result<serde_json::Value, octocrab::Error> = client
        .put(&url, Some(&body))
        .await;

    match result {
        Ok(_) => GitHubResult::Text {
            text: format!(
                "PR #{} has been {} merged into {}, sir.",
                pr_number,
                merge_method_str,
                repo
            ),
        },
        Err(e) => {
            // Check if it's a 405 (merge conflict) — shouldn't happen
            // because pre_check should catch this, but handle it anyway.
            if let octocrab::Error::GitHub { source, .. } = &e {
                if source.status_code.as_u16() == 405 {
                    let conflict_files = fetch_conflict_details(client, repo, pr_number).await;
                    return GitHubResult::MergeConflict {
                        pr_number,
                        repo: repo.to_string(),
                        conflict_files,
                        message: format!(
                            "PR #{} has merge conflicts and cannot be merged.",
                            pr_number
                        ),
                    };
                }
            }
            map_octocrab_error(e, &format!("merge PR #{}", pr_number))
        }
    }
}

// ─── Approve PR ────────────────────────────────────────────────────────

async fn execute_approve_pr(
    client: &octocrab::Octocrab,
    repo: &str,
    pr_number: u64,
) -> GitHubResult {
    let (owner, repo_name) = split_repo(repo);

    // Use the low-level API to submit an APPROVE review
    let url = format!("/repos/{}/{}/pulls/{}/reviews", owner, repo_name, pr_number);
    let body = serde_json::json!({
        "event": "APPROVE",
    });

    let result: Result<serde_json::Value, octocrab::Error> = client
        .post(&url, Some(&body))
        .await;

    match result {
        Ok(_) => GitHubResult::Text {
            text: format!("PR #{} in {} has been approved, sir.", pr_number, repo),
        },
        Err(e) => map_octocrab_error(e, &format!("approve PR #{}", pr_number)),
    }
}

// ─── Close PR ──────────────────────────────────────────────────────────

async fn execute_close_pr(
    client: &octocrab::Octocrab,
    repo: &str,
    pr_number: u64,
) -> GitHubResult {
    let (owner, repo_name) = split_repo(repo);

    // Use the low-level API to patch the PR state
    let url = format!("/repos/{}/{}/pulls/{}", owner, repo_name, pr_number);
    let body = serde_json::json!({
        "state": "closed",
    });

    let result: Result<serde_json::Value, octocrab::Error> = client
        .patch(&url, Some(&body))
        .await;

    match result {
        Ok(_) => GitHubResult::Text {
            text: format!("PR #{} in {} has been closed, sir.", pr_number, repo),
        },
        Err(e) => map_octocrab_error(e, &format!("close PR #{}", pr_number)),
    }
}

// ─── List PRs ──────────────────────────────────────────────────────────

/// Fetch ALL PRs across every repository the authenticated user has access to.
///
/// This covers repos where the user is:
///   - Owner (their own repos)
///   - Collaborator (invited to someone else's repo)
///   - Organization member (repos owned by orgs the user belongs to)
///
/// Uses a two-step approach:
///   1. List all repos via `GET /user/repos?affiliation=owner,collaborator,organization_member`
///   2. Fetch PRs from each repo in parallel via `GET /repos/{owner}/{repo}/pulls`
///   3. Aggregate, sort by created_at desc, cap at 100
///
/// This replaces the old `author:{username}` search which only found PRs the
/// user *created*. The new approach finds ALL PRs in ALL repos the user has
/// access to — including PRs opened by teammates, contributors, etc.
async fn execute_list_prs_account_wide(
    client: &octocrab::Octocrab,
    state: &str,
) -> GitHubResult {
    // 1. Get the authenticated user's login (for logging)
    let current_user = match client.current().user().await {
        Ok(u) => u,
        Err(e) => return map_octocrab_error(e, "fetch current user for account-wide PR list"),
    };
    let username = &current_user.login;
    tracing::info!("[github_cmd] account-wide PR list for user '{}', state='{}'", username, state);

    // 2. List ALL repos the user has access to (owner + collaborator + org member)
    let repo_page = match client
        .current()
        .list_repos_for_authenticated_user()
        .affiliation("owner,collaborator,organization_member")
        .per_page(100)
        .send()
        .await
    {
        Ok(page) => page,
        Err(e) => return map_octocrab_error(e, "list repos for account-wide PR list"),
    };

    // Collect (owner, repo_name) pairs from the first page.
    // We cap at 100 repos to avoid excessive API calls.
    let repo_list: Vec<(String, String)> = repo_page
        .items
        .iter()
        .filter_map(|repo| {
            let owner = repo.owner.as_ref()?.login.clone();
            let name = repo.name.clone();
            Some((owner, name))
        })
        .collect();

    let repo_count = repo_list.len();
    tracing::info!("[github_cmd] account-wide PR list: fetching from {} repos", repo_count);

    if repo_list.is_empty() {
        return GitHubResult::Text {
            text: "You don't have any accessible repositories, sir.".to_string(),
        };
    }

    // 3. Fetch PRs from each repo in parallel (capped concurrency to avoid
    //    hitting GitHub's rate limit). We use tokio JoinSet with chunks of 10.
    let mut all_prs: Vec<PrSummary> = Vec::new();
    const CONCURRENCY: usize = 10;

    for chunk in repo_list.chunks(CONCURRENCY) {
        let mut set = tokio::task::JoinSet::new();
        for (owner, name) in chunk {
            let client = client.clone();
            let owner = owner.clone();
            let name = name.clone();
            let state_str = state.to_string();
            set.spawn(async move {
                match client
                    .pulls(&owner, &name)
                    .list()
                    .state(match state_str.to_lowercase().as_str() {
                        "closed" => octocrab::params::State::Closed,
                        "all" => octocrab::params::State::All,
                        _ => octocrab::params::State::Open,
                    })
                    .per_page(20)
                    .send()
                    .await
                {
                    Ok(page) => {
                        let repo_full = format!("{}/{}", owner, name);
                        let prs: Vec<PrSummary> = page
                            .items
                            .iter()
                            .map(|pr| PrSummary {
                                number: pr.number,
                                title: pr.title.clone().unwrap_or_default(),
                                author: pr.user.as_ref().map(|u| u.login.clone()).unwrap_or_default(),
                                repo: repo_full.clone(),
                                state: match &pr.state {
                                    Some(octocrab::models::IssueState::Open) => "open".to_string(),
                                    Some(octocrab::models::IssueState::Closed) => "closed".to_string(),
                                    _ => state_str.clone(),
                                },
                                created_at: pr.created_at.unwrap_or_default().to_rfc3339(),
                                mergeable: None,
                            })
                            .collect();
                        prs
                    }
                    Err(e) => {
                        tracing::warn!("[github_cmd] failed to fetch PRs from {}/{}: {}", owner, name, e);
                        Vec::new()
                    }
                }
            });
        }
        while let Some(res) = set.join_next().await {
            if let Ok(prs) = res {
                all_prs.extend(prs);
            }
        }
    }

    if all_prs.is_empty() {
        return GitHubResult::Text {
            text: format!("You have no {} pull requests across your {} repositories.", state, repo_count),
        };
    }

    // 5. Sort by created_at descending (latest first)
    all_prs.sort_by(|a, b| b.created_at.cmp(&a.created_at));

    // 6. Cap at 100 PRs to keep the sidebar manageable
    if all_prs.len() > 100 {
        all_prs.truncate(100);
    }

    let count = all_prs.len();
    tracing::info!("[github_cmd] account-wide PR list: {} PRs from {} repos for user '{}'", count, repo_count, username);

    GitHubResult::PrList {
        repo: "all repositories".to_string(),
        state: state.to_string(),
        prs: all_prs,
    }
}

/// Extract "owner/repo" from a GitHub API repository_url.
/// Input: "https://api.github.com/repos/owner/repo"
/// Output: "owner/repo"
#[allow(dead_code)]
fn extract_repo_from_url(url: &str) -> String {
    // Parse the URL string to extract the path after /repos/
    // URL format: https://api.github.com/repos/{owner}/{repo}
    if let Some(pos) = url.find("/repos/") {
        let after_repos = &url[pos + 7..]; // skip "/repos/"
        // after_repos = "owner/repo" (possibly with trailing slash or query)
        // Take everything up to the first '?' or end
        let repo_part = after_repos.split('?').next().unwrap_or(after_repos);
        // Remove trailing slash
        let repo_part = repo_part.trim_end_matches('/');
        // Now we have "owner/repo" — take the first two path segments
        let parts: Vec<&str> = repo_part.splitn(3, '/').collect();
        if parts.len() >= 2 {
            return format!("{}/{}", parts[0], parts[1]);
        }
    }
    // Fallback: use the full URL as the repo identifier
    url.to_string()
}

async fn execute_list_prs(
    client: &octocrab::Octocrab,
    repo: &str,
    state: &str,
) -> GitHubResult {
    // If no repo is specified, fetch ALL PRs across the user's entire account
    // using the GitHub Search API. This handles "Open PR list" without a repo.
    if repo.is_empty() {
        return execute_list_prs_account_wide(client, state).await;
    }

    let (owner, repo_name) = split_repo(repo);

    let state_enum = match state.to_lowercase().as_str() {
        "closed" => octocrab::params::State::Closed,
        "all" => octocrab::params::State::All,
        _ => octocrab::params::State::Open,
    };

    let prs = match client
        .pulls(owner, repo_name)
        .list()
        .state(state_enum)
        .per_page(20)
        .send()
        .await
    {
        Ok(page) => page,
        Err(e) => return map_octocrab_error(e, &format!("list PRs in {}", repo)),
    };

    if prs.items.is_empty() {
        return GitHubResult::Text {
            text: format!("There are no {} pull requests in {}.", state, repo),
        };
    }

    // Build structured PR summaries for the sidebar.
    // Sort by created_at descending (latest first).
    let mut pr_summaries: Vec<PrSummary> = prs
        .items
        .iter()
        .map(|pr| PrSummary {
            number: pr.number,
            title: pr.title.clone().unwrap_or_else(|| "(no title)".to_string()),
            author: pr.user.as_ref().map(|u| u.login.clone()).unwrap_or_else(|| "unknown".to_string()),
            repo: repo.to_string(),
            state: state.to_string(),
            created_at: pr.created_at.map(|d| d.to_rfc3339()).unwrap_or_default(),
            mergeable: None, // not available in list view; fetched on demand
        })
        .collect();

    // Sort by created_at descending (latest first)
    pr_summaries.sort_by(|a, b| b.created_at.cmp(&a.created_at));

    GitHubResult::PrList {
        repo: repo.to_string(),
        state: state.to_string(),
        prs: pr_summaries,
    }
}

// ─── Get PR ────────────────────────────────────────────────────────────

async fn execute_get_pr(
    client: &octocrab::Octocrab,
    repo: &str,
    pr_number: u64,
) -> GitHubResult {
    let (owner, repo_name) = split_repo(repo);

    let pr = match client.pulls(owner, repo_name).get(pr_number).await {
        Ok(pr) => pr,
        Err(e) => return map_octocrab_error(e, &format!("fetch PR #{}", pr_number)),
    };

    let mergeable_state = format!("{:?}", pr.mergeable_state.unwrap_or(octocrab::models::pulls::MergeableState::Unknown));
    let author = pr
        .user
        .as_ref()
        .map(|u| u.login.as_str())
        .unwrap_or("unknown");
    let state_str = match pr.state {
        Some(octocrab::models::IssueState::Open) => "open",
        Some(octocrab::models::IssueState::Closed) => "closed",
        Some(_) => "unknown",
        None => "unknown",
    };

    GitHubResult::Text {
        text: format!(
            "PR #{}: {}\nState: {}\nAuthor: {}\nMergeable: {}\nChanges: +{} -{} across {} files\n\n{}",
            pr.number,
            pr.title.as_deref().unwrap_or("(no title)"),
            state_str,
            author,
            mergeable_state,
            pr.additions.unwrap_or(0),
            pr.deletions.unwrap_or(0),
            pr.changed_files.unwrap_or(0),
            pr.body.as_deref().unwrap_or("(no description)").chars().take(500).collect::<String>()
        ),
    }
}

// ─── Create PR ─────────────────────────────────────────────────────────

async fn execute_create_pr(
    client: &octocrab::Octocrab,
    repo: &str,
    title: &str,
    head: &str,
    base: &str,
    body: Option<&str>,
    draft: bool,
) -> GitHubResult {
    let (owner, repo_name) = split_repo(repo);

    let result = client
        .pulls(owner, repo_name)
        .create(title, head, base)
        .body(body.unwrap_or(""))
        .draft(draft)
        .send()
        .await;

    match result {
        Ok(pr) => GitHubResult::Text {
            text: format!(
                "Created PR #{}: {} in {}. {}{}",
                pr.number,
                pr.title.as_deref().unwrap_or(title),
                repo,
                if draft { "(draft) " } else { "" },
                pr.html_url
                    .as_ref()
                    .map(|u| format!("View at: {}", u))
                    .unwrap_or_default()
            ),
        },
        Err(e) => map_octocrab_error(e, &format!("create PR in {}", repo)),
    }
}

// ─── Update Branch ─────────────────────────────────────────────────────

async fn execute_update_branch(
    client: &octocrab::Octocrab,
    repo: &str,
    pr_number: u64,
) -> GitHubResult {
    let (owner, repo_name) = split_repo(repo);

    // PUT /repos/{owner}/{repo}/pulls/{pull_number}/update-branch
    let url = format!(
        "/repos/{}/{}/pulls/{}/update-branch",
        owner, repo_name, pr_number
    );

    let result: Result<serde_json::Value, octocrab::Error> = client.put(&url, None::<&()>).await;

    match result {
        Ok(_) => GitHubResult::Text {
            text: format!(
                "Updated branch for PR #{} in {} with the latest changes from the base branch, sir.",
                pr_number, repo
            ),
        },
        Err(e) => map_octocrab_error(e, &format!("update branch for PR #{}", pr_number)),
    }
}

// ─── Revert PR ─────────────────────────────────────────────────────────

async fn execute_revert_pr(
    client: &octocrab::Octocrab,
    repo: &str,
    pr_number: u64,
    custom_title: Option<&str>,
) -> GitHubResult {
    let (owner, repo_name) = split_repo(repo);

    // Step 1: Fetch the PR to get the merge commit SHA
    let pr = match client.pulls(owner, repo_name).get(pr_number).await {
        Ok(pr) => pr,
        Err(e) => return map_octocrab_error(e, &format!("fetch PR #{} for revert", pr_number)),
    };

    let merge_commit_sha = match pr.merge_commit_sha {
        Some(sha) => sha,
        None => {
            return GitHubResult::Error {
                message: format!(
                    "PR #{} has not been merged yet — cannot revert an unmerged PR.",
                    pr_number
                ),
                status: None,
                is_auth_error: false,
            }
        }
    };

    // Step 2: Create a revert branch via the API
    // POST /repos/{owner}/{repo}/pulls/{pull_number}/revert
    let url = format!("/repos/{}/{}/pulls/{}/revert", owner, repo_name, pr_number);
    let title = custom_title
        .map(|t| t.to_string())
        .unwrap_or_else(|| format!("Revert \"{}\"", pr.title.as_deref().unwrap_or("PR")));

    let body = serde_json::json!({
        "title": title,
        "body": format!("This reverts PR #{}.", pr_number),
        "commit_sha": merge_commit_sha,
    });

    let result: Result<serde_json::Value, octocrab::Error> = client.post(&url, Some(&body)).await;

    match result {
        Ok(v) => {
            let new_pr_number = v["number"].as_u64().unwrap_or(0);
            let html_url = v["html_url"].as_str().unwrap_or("");
            GitHubResult::Text {
                text: format!(
                    "Created revert PR #{} for PR #{} in {}. View at: {}",
                    new_pr_number, pr_number, repo, html_url
                ),
            }
        }
        Err(e) => map_octocrab_error(e, &format!("revert PR #{}", pr_number)),
    }
}

// ─── List PR Files ─────────────────────────────────────────────────────

async fn execute_list_pr_files(
    client: &octocrab::Octocrab,
    repo: &str,
    pr_number: u64,
) -> GitHubResult {
    let (owner, repo_name) = split_repo(repo);

    let url = format!("/repos/{}/{}/pulls/{}/files", owner, repo_name, pr_number);
    let result: Result<serde_json::Value, octocrab::Error> =
        client.get(&url, None::<&str>).await;

    match result {
        Ok(v) => {
            let files = v.as_array().cloned().unwrap_or_default();
            if files.is_empty() {
                return GitHubResult::Text {
                    text: format!("PR #{} in {} has no changed files.", pr_number, repo),
                };
            }

            let file_list: Vec<String> = files
                .iter()
                .enumerate()
                .map(|(i, f)| {
                    let filename = f["filename"].as_str().unwrap_or("(unknown)");
                    let status = f["status"].as_str().unwrap_or("modified");
                    let additions = f["additions"].as_u64().unwrap_or(0);
                    let deletions = f["deletions"].as_u64().unwrap_or(0);
                    format!(
                        "{}. {} ({}): +{} -{}",
                        i + 1,
                        filename,
                        status,
                        additions,
                        deletions
                    )
                })
                .collect();

            GitHubResult::Text {
                text: format!(
                    "PR #{} in {} changes {} files:\n{}",
                    pr_number,
                    repo,
                    files.len(),
                    file_list.join("\n")
                ),
            }
        }
        Err(e) => map_octocrab_error(e, &format!("list files for PR #{}", pr_number)),
    }
}

// ─── Comment on PR ─────────────────────────────────────────────────────

async fn execute_comment_pr(
    client: &octocrab::Octocrab,
    repo: &str,
    pr_number: u64,
    body: &str,
) -> GitHubResult {
    let (owner, repo_name) = split_repo(repo);

    // POST /repos/{owner}/{repo}/issues/{issue_number}/comments
    // (PRs are issues in GitHub's API — comments go to the issues endpoint)
    let url = format!(
        "/repos/{}/{}/issues/{}/comments",
        owner, repo_name, pr_number
    );
    let json_body = serde_json::json!({ "body": body });

    let result: Result<serde_json::Value, octocrab::Error> =
        client.post(&url, Some(&json_body)).await;

    match result {
        Ok(_) => GitHubResult::Text {
            text: format!("Comment added to PR #{} in {}, sir.", pr_number, repo),
        },
        Err(e) => map_octocrab_error(e, &format!("comment on PR #{}", pr_number)),
    }
}

// ─── Add Collaborator ──────────────────────────────────────────────────

async fn execute_add_collaborator(
    client: &octocrab::Octocrab,
    repo: &str,
    username: &str,
    permission: &CollaboratorPermission,
) -> GitHubResult {
    let (owner, repo_name) = split_repo(repo);
    let perm_str = match permission {
        CollaboratorPermission::Pull => "pull",
        CollaboratorPermission::Triage => "triage",
        CollaboratorPermission::Push => "push",
        CollaboratorPermission::Maintain => "maintain",
        CollaboratorPermission::Admin => "admin",
    };

    let url = format!(
        "/repos/{}/{}/collaborators/{}",
        owner, repo_name, username
    );
    let body = serde_json::json!({ "permission": perm_str });

    let result: Result<serde_json::Value, octocrab::Error> = client.put(&url, Some(&body)).await;

    match result {
        Ok(_) => GitHubResult::Text {
            text: format!(
                "Added {} as a collaborator to {} with {} permission, sir. An invitation has been sent.",
                username, repo, perm_str
            ),
        },
        Err(e) => map_octocrab_error(e, &format!("add collaborator {} to {}", username, repo)),
    }
}

// ─── Remove Collaborator ───────────────────────────────────────────────

async fn execute_remove_collaborator(
    client: &octocrab::Octocrab,
    repo: &str,
    username: &str,
) -> GitHubResult {
    let (owner, repo_name) = split_repo(repo);
    let url = format!(
        "/repos/{}/{}/collaborators/{}",
        owner, repo_name, username
    );

    let result: Result<serde_json::Value, octocrab::Error> =
        client.delete(&url, None::<&()>).await;

    match result {
        Ok(_) => GitHubResult::Text {
            text: format!("Removed {} as a collaborator from {}, sir.", username, repo),
        },
        Err(e) => map_octocrab_error(e, &format!("remove collaborator {} from {}", username, repo)),
    }
}

// ─── List Collaborators ────────────────────────────────────────────────

async fn execute_list_collaborators(
    client: &octocrab::Octocrab,
    repo: &str,
) -> GitHubResult {
    let (owner, repo_name) = split_repo(repo);
    let url = format!("/repos/{}/{}/collaborators", owner, repo_name);

    let result: Result<serde_json::Value, octocrab::Error> =
        client.get(&url, None::<&str>).await;

    match result {
        Ok(v) => {
            let users = v.as_array().cloned().unwrap_or_default();
            if users.is_empty() {
                return GitHubResult::Text {
                    text: format!("{} has no collaborators.", repo),
                };
            }
            let list: Vec<String> = users
                .iter()
                .enumerate()
                .map(|(i, u)| {
                    let login = u["login"].as_str().unwrap_or("(unknown)");
                    let perms = u["permissions"].as_object().map(|p| {
                        if p.get("admin").and_then(|v| v.as_bool()).unwrap_or(false) {
                            "admin"
                        } else if p.get("push").and_then(|v| v.as_bool()).unwrap_or(false) {
                            "push"
                        } else if p.get("pull").and_then(|v| v.as_bool()).unwrap_or(false) {
                            "pull"
                        } else {
                            "unknown"
                        }
                    }).unwrap_or("unknown");
                    format!("{}. {} ({})", i + 1, login, perms)
                })
                .collect();
            GitHubResult::Text {
                text: format!("Collaborators of {}:\n{}", repo, list.join("\n")),
            }
        }
        Err(e) => map_octocrab_error(e, &format!("list collaborators of {}", repo)),
    }
}

// ─── Add Org Member ────────────────────────────────────────────────────

async fn execute_add_org_member(
    client: &octocrab::Octocrab,
    org: &str,
    username: &str,
    role: &OrgRole,
) -> GitHubResult {
    let role_str = match role {
        OrgRole::Member => "member",
        OrgRole::Admin => "admin",
    };

    let url = format!("/orgs/{}/memberships/{}", org, username);
    let body = serde_json::json!({ "role": role_str });

    let result: Result<serde_json::Value, octocrab::Error> = client.put(&url, Some(&body)).await;

    match result {
        Ok(v) => {
            let state = v["state"].as_str().unwrap_or("unknown");
            if state == "pending" {
                GitHubResult::Text {
                    text: format!(
                        "Invitation sent to {} to join {} as {}. They must accept the invitation.",
                        username, org, role_str
                    ),
                }
            } else {
                GitHubResult::Text {
                    text: format!("Added {} to {} as {}, sir.", username, org, role_str),
                }
            }
        }
        Err(e) => map_octocrab_error(e, &format!("add {} to org {}", username, org)),
    }
}

// ─── Remove Org Member ─────────────────────────────────────────────────

async fn execute_remove_org_member(
    client: &octocrab::Octocrab,
    org: &str,
    username: &str,
) -> GitHubResult {
    let url = format!("/orgs/{}/memberships/{}", org, username);

    let result: Result<serde_json::Value, octocrab::Error> =
        client.delete(&url, None::<&()>).await;

    match result {
        Ok(_) => GitHubResult::Text {
            text: format!("Removed {} from the {} organization, sir.", username, org),
        },
        Err(e) => map_octocrab_error(e, &format!("remove {} from org {}", username, org)),
    }
}

// ─── List Org Members ──────────────────────────────────────────────────

async fn execute_list_org_members(
    client: &octocrab::Octocrab,
    org: &str,
) -> GitHubResult {
    let url = format!("/orgs/{}/members", org);

    let result: Result<serde_json::Value, octocrab::Error> =
        client.get(&url, None::<&str>).await;

    match result {
        Ok(v) => {
            let users = v.as_array().cloned().unwrap_or_default();
            if users.is_empty() {
                return GitHubResult::Text {
                    text: format!("{} has no members.", org),
                };
            }
            let list: Vec<String> = users
                .iter()
                .enumerate()
                .map(|(i, u)| {
                    let login = u["login"].as_str().unwrap_or("(unknown)");
                    format!("{}. {}", i + 1, login)
                })
                .collect();
            GitHubResult::Text {
                text: format!("Members of {}:\n{}", org, list.join("\n")),
            }
        }
        Err(e) => map_octocrab_error(e, &format!("list members of org {}", org)),
    }
}

// ─── Convert to Outside Collaborator ───────────────────────────────────

async fn execute_convert_to_outside_collaborator(
    client: &octocrab::Octocrab,
    org: &str,
    username: &str,
) -> GitHubResult {
    // PUT /orgs/{org}/outside_collaborators/{username}
    let url = format!("/orgs/{}/outside_collaborators/{}", org, username);

    let result: Result<serde_json::Value, octocrab::Error> =
        client.put(&url, None::<&()>).await;

    match result {
        Ok(_) => GitHubResult::Text {
            text: format!(
                "Converted {} to an outside collaborator in {}, sir.",
                username, org
            ),
        },
        Err(e) => map_octocrab_error(e, &format!("convert {} to outside collaborator in {}", username, org)),
    }
}

// ─── List Outside Collaborators ────────────────────────────────────────

async fn execute_list_outside_collaborators(
    client: &octocrab::Octocrab,
    org: &str,
) -> GitHubResult {
    let url = format!("/orgs/{}/outside_collaborators", org);

    let result: Result<serde_json::Value, octocrab::Error> =
        client.get(&url, None::<&str>).await;

    match result {
        Ok(v) => {
            let users = v.as_array().cloned().unwrap_or_default();
            if users.is_empty() {
                return GitHubResult::Text {
                    text: format!("{} has no outside collaborators.", org),
                };
            }
            let list: Vec<String> = users
                .iter()
                .enumerate()
                .map(|(i, u)| {
                    let login = u["login"].as_str().unwrap_or("(unknown)");
                    format!("{}. {}", i + 1, login)
                })
                .collect();
            GitHubResult::Text {
                text: format!("Outside collaborators of {}:\n{}", org, list.join("\n")),
            }
        }
        Err(e) => map_octocrab_error(e, &format!("list outside collaborators of {}", org)),
    }
}

// ─── Set Branch Protection ─────────────────────────────────────────────

async fn execute_set_branch_protection(
    client: &octocrab::Octocrab,
    repo: &str,
    branch: &str,
    required_status_checks: &[String],
    require_pr_reviews: bool,
    required_review_count: u8,
    enforce_admins: bool,
) -> GitHubResult {
    let (owner, repo_name) = split_repo(repo);
    let url = format!(
        "/repos/{}/{}/branches/{}/protection",
        owner, repo_name, branch
    );

    let mut body = serde_json::json!({
        "required_status_checks": {
            "strict": true,
            "contexts": required_status_checks,
        },
        "enforce_admins": enforce_admins,
        "restrictions": null,
    });

    if require_pr_reviews {
        body["required_pull_request_reviews"] = serde_json::json!({
            "required_approving_review_count": required_review_count,
            "dismiss_stale_reviews": true,
            "require_code_owner_reviews": false,
        });
    } else {
        body["required_pull_request_reviews"] = serde_json::Value::Null;
    }

    let result: Result<serde_json::Value, octocrab::Error> = client.put(&url, Some(&body)).await;

    match result {
        Ok(_) => GitHubResult::Text {
            text: format!(
                "Branch protection set for {} in {}, sir. Reviews required: {}, Status checks: {}.",
                branch,
                repo,
                if require_pr_reviews { format!("{}", required_review_count) } else { "none".into() },
                if required_status_checks.is_empty() { "none".into() } else { required_status_checks.join(", ") }
            ),
        },
        Err(e) => map_octocrab_error(e, &format!("set branch protection on {} in {}", branch, repo)),
    }
}

// ─── Delete Branch ──────────────────────────────────────────────────────

async fn execute_delete_branch(
    client: &octocrab::Octocrab,
    repo: &str,
    branch: &str,
) -> GitHubResult {
    let (owner, repo_name) = split_repo(repo);
    let url = format!("/repos/{}/{}/git/refs/heads/{}", owner, repo_name, branch);

    let result: Result<serde_json::Value, octocrab::Error> =
        client.delete(&url, None::<&()>).await;

    match result {
        Ok(_) => GitHubResult::Text {
            text: format!("Deleted branch {} in {}, sir.", branch, repo),
        },
        Err(e) => map_octocrab_error(e, &format!("delete branch {} in {}", branch, repo)),
    }
}

// ─── List Branches ──────────────────────────────────────────────────────

async fn execute_list_branches(
    client: &octocrab::Octocrab,
    repo: &str,
) -> GitHubResult {
    let (owner, repo_name) = split_repo(repo);
    let url = format!("/repos/{}/{}/branches", owner, repo_name);

    let result: Result<serde_json::Value, octocrab::Error> =
        client.get(&url, None::<&str>).await;

    match result {
        Ok(v) => {
            let branches = v.as_array().cloned().unwrap_or_default();
            if branches.is_empty() {
                return GitHubResult::Text {
                    text: format!("{} has no branches.", repo),
                };
            }
            let list: Vec<String> = branches
                .iter()
                .enumerate()
                .map(|(i, b)| {
                    let name = b["name"].as_str().unwrap_or("(unknown)");
                    let protected = b["protected"].as_bool().unwrap_or(false);
                    format!("{}. {}{}", i + 1, name, if protected { " (protected)" } else { "" })
                })
                .collect();
            GitHubResult::Text {
                text: format!("Branches in {}:\n{}", repo, list.join("\n")),
            }
        }
        Err(e) => map_octocrab_error(e, &format!("list branches in {}", repo)),
    }
}

// ─── Create Release ─────────────────────────────────────────────────────

async fn execute_create_release(
    client: &octocrab::Octocrab,
    repo: &str,
    tag: &str,
    name: &str,
    body: Option<&str>,
    draft: bool,
    prerelease: bool,
    target_commitish: Option<&str>,
) -> GitHubResult {
    let (owner, repo_name) = split_repo(repo);
    let url = format!("/repos/{}/{}/releases", owner, repo_name);

    let mut json_body = serde_json::json!({
        "tag_name": tag,
        "name": name,
        "body": body.unwrap_or(""),
        "draft": draft,
        "prerelease": prerelease,
    });

    if let Some(tc) = target_commitish {
        json_body["target_commitish"] = serde_json::Value::String(tc.to_string());
    }

    let result: Result<serde_json::Value, octocrab::Error> =
        client.post(&url, Some(&json_body)).await;

    match result {
        Ok(v) => {
            let html_url = v["html_url"].as_str().unwrap_or("");
            GitHubResult::Text {
                text: format!(
                    "Created release {} in {}. {}",
                    name,
                    repo,
                    if !html_url.is_empty() { format!("View at: {}", html_url) } else { String::new() }
                ),
            }
        }
        Err(e) => map_octocrab_error(e, &format!("create release {} in {}", name, repo)),
    }
}

// ─── List Releases ──────────────────────────────────────────────────────

async fn execute_list_releases(
    client: &octocrab::Octocrab,
    repo: &str,
) -> GitHubResult {
    let (owner, repo_name) = split_repo(repo);
    let url = format!("/repos/{}/{}/releases", owner, repo_name);

    let result: Result<serde_json::Value, octocrab::Error> =
        client.get(&url, None::<&str>).await;

    match result {
        Ok(v) => {
            let releases = v.as_array().cloned().unwrap_or_default();
            if releases.is_empty() {
                return GitHubResult::Text {
                    text: format!("{} has no releases.", repo),
                };
            }
            let list: Vec<String> = releases
                .iter()
                .take(10)
                .enumerate()
                .map(|(i, r)| {
                    let name = r["name"].as_str().or(r["tag_name"].as_str()).unwrap_or("(unknown)");
                    let prerelease = r["prerelease"].as_bool().unwrap_or(false);
                    let draft = r["draft"].as_bool().unwrap_or(false);
                    let tag = r["tag_name"].as_str().unwrap_or("");
                    format!(
                        "{}. {} (tag: {}){}",
                        i + 1,
                        name,
                        tag,
                        if prerelease { " [prerelease]" } else if draft { " [draft]" } else { "" }
                    )
                })
                .collect();
            GitHubResult::Text {
                text: format!("Releases in {}:\n{}", repo, list.join("\n")),
            }
        }
        Err(e) => map_octocrab_error(e, &format!("list releases in {}", repo)),
    }
}

// ─── Delete Release ─────────────────────────────────────────────────────

async fn execute_delete_release(
    client: &octocrab::Octocrab,
    repo: &str,
    release_id: u64,
) -> GitHubResult {
    let (owner, repo_name) = split_repo(repo);
    let url = format!("/repos/{}/{}/releases/{}", owner, repo_name, release_id);

    let result: Result<serde_json::Value, octocrab::Error> =
        client.delete(&url, None::<&()>).await;

    match result {
        Ok(_) => GitHubResult::Text {
            text: format!("Deleted release {} in {}, sir.", release_id, repo),
        },
        Err(e) => map_octocrab_error(e, &format!("delete release {} in {}", release_id, repo)),
    }
}

// ─── List Workflows ─────────────────────────────────────────────────────

async fn execute_list_workflows(
    client: &octocrab::Octocrab,
    repo: &str,
) -> GitHubResult {
    let (owner, repo_name) = split_repo(repo);
    let url = format!("/repos/{}/{}/actions/workflows", owner, repo_name);

    let result: Result<serde_json::Value, octocrab::Error> =
        client.get(&url, None::<&str>).await;

    match result {
        Ok(v) => {
            let workflows = v["workflows"]
                .as_array()
                .cloned()
                .unwrap_or_default();
            if workflows.is_empty() {
                return GitHubResult::Text {
                    text: format!("{} has no GitHub Actions workflows.", repo),
                };
            }
            let list: Vec<String> = workflows
                .iter()
                .enumerate()
                .map(|(i, w)| {
                    let name = w["name"].as_str().unwrap_or("(unknown)");
                    let state = w["state"].as_str().unwrap_or("unknown");
                    let path = w["path"].as_str().unwrap_or("");
                    format!("{}. {} ({}) — {}", i + 1, name, path, state)
                })
                .collect();
            GitHubResult::Text {
                text: format!("Workflows in {}:\n{}", repo, list.join("\n")),
            }
        }
        Err(e) => map_octocrab_error(e, &format!("list workflows in {}", repo)),
    }
}

// ─── List Workflow Runs ─────────────────────────────────────────────────

async fn execute_list_workflow_runs(
    client: &octocrab::Octocrab,
    repo: &str,
    workflow_file: Option<&str>,
) -> GitHubResult {
    let (owner, repo_name) = split_repo(repo);

    let url = if let Some(wf) = workflow_file {
        format!(
            "/repos/{}/{}/actions/workflows/{}/runs",
            owner, repo_name, wf
        )
    } else {
        format!("/repos/{}/{}/actions/runs", owner, repo_name)
    };

    let result: Result<serde_json::Value, octocrab::Error> =
        client.get(&url, None::<&str>).await;

    match result {
        Ok(v) => {
            let runs = v["workflow_runs"]
                .as_array()
                .cloned()
                .unwrap_or_default();
            if runs.is_empty() {
                return GitHubResult::Text {
                    text: format!("No workflow runs found in {}.", repo),
                };
            }
            let list: Vec<String> = runs
                .iter()
                .take(10)
                .enumerate()
                .map(|(i, r)| {
                    let id = r["id"].as_u64().unwrap_or(0);
                    let status = r["status"].as_str().unwrap_or("unknown");
                    let conclusion = r["conclusion"].as_str().unwrap_or("—");
                    let branch = r["head_branch"].as_str().unwrap_or("?");
                    let display_title = r["display_title"].as_str().unwrap_or("(no title)");
                    format!(
                        "{}. #{} [{} / {}] {} ({})",
                        i + 1, id, status, conclusion, display_title, branch
                    )
                })
                .collect();
            GitHubResult::Text {
                text: format!("Recent workflow runs in {}:\n{}", repo, list.join("\n")),
            }
        }
        Err(e) => map_octocrab_error(e, &format!("list workflow runs in {}", repo)),
    }
}

// ─── Rerun Workflow ─────────────────────────────────────────────────────

async fn execute_rerun_workflow(
    client: &octocrab::Octocrab,
    repo: &str,
    run_id: u64,
) -> GitHubResult {
    let (owner, repo_name) = split_repo(repo);
    let url = format!(
        "/repos/{}/{}/actions/runs/{}/rerun",
        owner, repo_name, run_id
    );

    let result: Result<serde_json::Value, octocrab::Error> =
        client.post(&url, None::<&()>).await;

    match result {
        Ok(_) => GitHubResult::Text {
            text: format!("Rerun workflow {} in {}, sir.", run_id, repo),
        },
        Err(e) => map_octocrab_error(e, &format!("rerun workflow {} in {}", run_id, repo)),
    }
}

// ─── Cancel Workflow ────────────────────────────────────────────────────

async fn execute_cancel_workflow(
    client: &octocrab::Octocrab,
    repo: &str,
    run_id: u64,
) -> GitHubResult {
    let (owner, repo_name) = split_repo(repo);
    let url = format!(
        "/repos/{}/{}/actions/runs/{}/cancel",
        owner, repo_name, run_id
    );

    let result: Result<serde_json::Value, octocrab::Error> =
        client.post(&url, None::<&()>).await;

    match result {
        Ok(_) => GitHubResult::Text {
            text: format!("Cancelled workflow {} in {}, sir.", run_id, repo),
        },
        Err(e) => map_octocrab_error(e, &format!("cancel workflow {} in {}", run_id, repo)),
    }
}

// ─── Conflict Detection ────────────────────────────────────────────────

/// Fetch conflict details for a PR. This retrieves the files changed
/// in the PR and identifies which ones have conflict markers.
async fn fetch_conflict_details(
    client: &octocrab::Octocrab,
    repo: &str,
    pr_number: u64,
) -> Vec<ConflictFile> {
    let (owner, repo_name) = split_repo(repo);
    let url = format!("/repos/{}/{}/pulls/{}/files", owner, repo_name, pr_number);

    let result: Result<serde_json::Value, octocrab::Error> = client.get(&url, None::<&str>).await;

    let files = match result {
        Ok(v) => v.as_array().cloned().unwrap_or_default(),
        Err(e) => {
            tracing::warn!("github_cmd: failed to fetch PR files for conflict details: {}", e);
            return Vec::new();
        }
    };

    let mut conflict_files = Vec::new();

    for file in files {
        let filename = file["filename"]
            .as_str()
            .unwrap_or("(unknown)")
            .to_string();

        // Check if the file has merge conflict markers in the patch
        let patch = file["patch"].as_str().unwrap_or("");

        // Conflict markers in a diff patch appear as:
        // +<<<<<<< HEAD
        // +=======
        // +>>>>>>> branch
        let has_conflicts = patch.contains("<<<<<<<") && patch.contains(">>>>>>>");

        if has_conflicts {
            let blocks = extract_conflict_blocks_from_patch(patch);
            conflict_files.push(ConflictFile {
                filename,
                conflict_count: blocks.len(),
                blocks,
            });
        }
    }

    conflict_files
}

/// Extract conflict blocks from a git patch string.
/// Conflict markers in patches appear as added lines starting with
/// +<<<<<<<, +=======, +>>>>>>>.
fn extract_conflict_blocks_from_patch(patch: &str) -> Vec<ConflictBlock> {
    let mut blocks = Vec::new();
    let mut current_head = String::new();
    let mut current_branch = String::new();
    let mut in_head = false;
    let mut in_branch = false;
    let mut start_line = 0;
    let mut line_num = 0;

    for line in patch.lines() {
        line_num += 1;

        // Strip the leading "+" for added lines in a patch
        let content = if line.starts_with('+') {
            &line[1..]
        } else if line.starts_with('-') {
            continue;
        } else {
            continue;
        };

        if content.starts_with("<<<<<<<") {
            in_head = true;
            in_branch = false;
            start_line = line_num;
            current_head.clear();
            current_branch.clear();
        } else if content.starts_with("=======") {
            in_head = false;
            in_branch = true;
        } else if content.starts_with(">>>>>>>") {
            in_head = false;
            in_branch = false;
            blocks.push(ConflictBlock {
                start_line,
                head_content: current_head.trim().to_string(),
                branch_content: current_branch.trim().to_string(),
            });
            current_head.clear();
            current_branch.clear();
        } else if in_head {
            current_head.push_str(content);
            current_head.push('\n');
        } else if in_branch {
            current_branch.push_str(content);
            current_branch.push('\n');
        }
    }

    blocks
}

// ─── Public API: execute_command ───────────────────────────────────────

/// Execute a GitHub command. This is the main entry point called by
/// the orchestrator when routing to `Subsystem::GitHub`.
///
/// Steps:
///   1. Build octocrab client (fetches token from Worker)
///   2. Run pre-check (e.g., conflict detection for merge)
///   3. If destructive and not confirmed → return NeedsConfirmation
///   4. Execute the command
///   5. Return the result
pub async fn execute_command(
    worker_url: &str,
    user_id: &str,
    command: &GitHubCommand,
    confirmed: bool,
) -> GitHubResult {
    // Build the octocrab client
    let client = match build_octocrab_client(worker_url, user_id).await {
        Ok(c) => c,
        Err(e) => {
            return GitHubResult::Error {
                message: e,
                status: None,
                is_auth_error: true,
            }
        }
    };

    // Pre-check (e.g., conflict detection for merge)
    if let Err(conflict_result) = command.pre_check(&client).await {
        return conflict_result;
    }

    // If destructive and not confirmed, ask for confirmation
    if command.is_destructive() && !confirmed {
        if let Some(prompt) = command.confirmation_prompt() {
            return GitHubResult::NeedsConfirmation {
                prompt,
                command: command.clone(),
            };
        }
    }

    // Execute
    command.execute(&client).await
}

// ─── Tests ─────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_split_repo() {
        assert_eq!(split_repo("owner/repo"), ("owner", "repo"));
        assert_eq!(split_repo("Engine-NEXUS/NEXUS-Agent"), ("Engine-NEXUS", "NEXUS-Agent"));
        assert_eq!(split_repo("single"), ("", "single"));
    }

    #[test]
    fn test_merge_method_default() {
        assert_eq!(MergeMethod::default(), MergeMethod::Squash);
    }

    #[test]
    fn test_collaborator_permission_default() {
        assert_eq!(CollaboratorPermission::default(), CollaboratorPermission::Push);
    }

    #[test]
    fn test_org_role_default() {
        assert_eq!(OrgRole::default(), OrgRole::Member);
    }

    #[test]
    fn test_command_is_destructive() {
        assert!(GitHubCommand::MergePr {
            repo: "owner/repo".into(),
            pr_number: 1,
            method: MergeMethod::Squash,
        }
        .is_destructive());

        assert!(GitHubCommand::ClosePr {
            repo: "owner/repo".into(),
            pr_number: 1,
        }
        .is_destructive());

        assert!(!GitHubCommand::ApprovePr {
            repo: "owner/repo".into(),
            pr_number: 1,
        }
        .is_destructive());

        assert!(!GitHubCommand::ListPrs {
            repo: "owner/repo".into(),
            state: "open".into(),
        }
        .is_destructive());

        assert!(!GitHubCommand::GetPr {
            repo: "owner/repo".into(),
            pr_number: 1,
        }
        .is_destructive());
    }

    #[test]
    fn test_confirmation_prompt_merge() {
        let cmd = GitHubCommand::MergePr {
            repo: "Engine-NEXUS/NEXUS-Agent".into(),
            pr_number: 25,
            method: MergeMethod::Squash,
        };
        let prompt = cmd.confirmation_prompt().unwrap();
        assert!(prompt.contains("PR #25"));
        assert!(prompt.contains("Engine-NEXUS/NEXUS-Agent"));
        assert!(prompt.contains("squash"));
    }

    #[test]
    fn test_confirmation_prompt_close() {
        let cmd = GitHubCommand::ClosePr {
            repo: "owner/repo".into(),
            pr_number: 42,
        };
        let prompt = cmd.confirmation_prompt().unwrap();
        assert!(prompt.contains("PR #42"));
        assert!(prompt.contains("close"));
    }

    #[test]
    fn test_confirmation_prompt_none_for_non_destructive() {
        let cmd = GitHubCommand::ApprovePr {
            repo: "owner/repo".into(),
            pr_number: 1,
        };
        assert!(cmd.confirmation_prompt().is_none());

        let cmd = GitHubCommand::ListPrs {
            repo: "owner/repo".into(),
            state: "open".into(),
        };
        assert!(cmd.confirmation_prompt().is_none());
    }

    #[test]
    fn test_command_serialization() {
        let cmd = GitHubCommand::MergePr {
            repo: "owner/repo".into(),
            pr_number: 42,
            method: MergeMethod::Rebase,
        };
        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains("\"command\":\"merge_pr\""));
        assert!(json.contains("\"repo\":\"owner/repo\""));
        assert!(json.contains("\"pr_number\":42"));
        assert!(json.contains("\"method\":\"rebase\""));

        // Deserialize back
        let deserialized: GitHubCommand = serde_json::from_str(&json).unwrap();
        match deserialized {
            GitHubCommand::MergePr {
                repo,
                pr_number,
                method,
            } => {
                assert_eq!(repo, "owner/repo");
                assert_eq!(pr_number, 42);
                assert_eq!(method, MergeMethod::Rebase);
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn test_extract_conflict_blocks_from_patch() {
        let patch = r#"+<<<<<<< HEAD
+fn auth() -> Result<Token> {
+    token::generate()
+}
+=======
+fn auth() -> Result<Token> {
+    oauth::exchange(code)
+}
+>>>>>>> feature-branch
"#;
        let blocks = extract_conflict_blocks_from_patch(patch);
        assert_eq!(blocks.len(), 1);
        assert!(blocks[0].head_content.contains("token::generate"));
        assert!(blocks[0].branch_content.contains("oauth::exchange"));
    }

    #[test]
    fn test_extract_conflict_blocks_multiple() {
        let patch = r#"+<<<<<<< HEAD
+let x = 1;
+=======
+let x = 2;
+>>>>>>> branch
+some other line
+<<<<<<< HEAD
+let y = 3;
+=======
+let y = 4;
+>>>>>>> branch
"#;
        let blocks = extract_conflict_blocks_from_patch(patch);
        assert_eq!(blocks.len(), 2);
        assert!(blocks[0].head_content.contains("x = 1"));
        assert!(blocks[1].head_content.contains("y = 3"));
    }

    #[test]
    fn test_extract_conflict_blocks_no_conflicts() {
        let patch = "+let x = 1;\n+let y = 2;\n";
        let blocks = extract_conflict_blocks_from_patch(patch);
        assert!(blocks.is_empty());
    }

    #[test]
    fn test_github_result_serialization() {
        let result = GitHubResult::Text {
            text: "PR merged".into(),
        };
        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("\"type\":\"text\""));
        assert!(json.contains("\"text\":\"PR merged\""));
    }

    #[test]
    fn test_github_result_needs_confirmation_serialization() {
        let cmd = GitHubCommand::MergePr {
            repo: "owner/repo".into(),
            pr_number: 1,
            method: MergeMethod::Squash,
        };
        let result = GitHubResult::NeedsConfirmation {
            prompt: "Are you sure?".into(),
            command: cmd,
        };
        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("\"type\":\"needs_confirmation\""));
        assert!(json.contains("\"prompt\":\"Are you sure?\""));
        assert!(json.contains("\"command\":\"merge_pr\""));
    }

    #[test]
    fn test_github_result_merge_conflict_serialization() {
        let result = GitHubResult::MergeConflict {
            pr_number: 25,
            repo: "owner/repo".into(),
            conflict_files: vec![ConflictFile {
                filename: "src/main.rs".into(),
                conflict_count: 1,
                blocks: vec![ConflictBlock {
                    start_line: 10,
                    head_content: "let x = 1;".into(),
                    branch_content: "let x = 2;".into(),
                }],
            }],
            message: "Has conflicts".into(),
        };
        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("\"type\":\"merge_conflict\""));
        assert!(json.contains("\"pr_number\":25"));
        assert!(json.contains("\"filename\":\"src/main.rs\""));
        assert!(json.contains("\"head_content\":\"let x = 1;\""));
    }

    #[test]
    fn test_cached_token_validity() {
        let now = chrono::Utc::now().timestamp() as f64;

        // Classic token (no expiry) — always valid
        let classic = CachedToken {
            token: "abc".into(),
            expires_at: 0.0,
            fetched_at: now,
        };
        assert!(classic.is_valid());

        // Token that expires in 1 hour — valid
        let valid = CachedToken {
            token: "abc".into(),
            expires_at: now + 3600.0,
            fetched_at: now,
        };
        assert!(valid.is_valid());

        // Token that expired 10 minutes ago — invalid
        let expired = CachedToken {
            token: "abc".into(),
            expires_at: now - 600.0,
            fetched_at: now - 4200.0,
        };
        assert!(!expired.is_valid());

        // Token that expires in 3 minutes — invalid (we refresh 5 min before)
        let soon_expire = CachedToken {
            token: "abc".into(),
            expires_at: now + 180.0,
            fetched_at: now - 3420.0,
        };
        assert!(!soon_expire.is_valid());
    }

    #[test]
    fn test_default_pr_state() {
        assert_eq!(default_pr_state(), "open");
    }

    #[test]
    fn test_github_result_error_serialization() {
        let result = GitHubResult::Error {
            message: "Token expired".into(),
            status: Some(401),
            is_auth_error: true,
        };
        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("\"type\":\"error\""));
        assert!(json.contains("\"status\":401"));
        assert!(json.contains("\"is_auth_error\":true"));
    }

    #[test]
    fn test_confirmation_prompt_includes_method() {
        let cmd = GitHubCommand::MergePr {
            repo: "owner/repo".into(),
            pr_number: 10,
            method: MergeMethod::Merge,
        };
        let prompt = cmd.confirmation_prompt().unwrap();
        assert!(prompt.contains("merge merge"));
    }

    #[test]
    fn test_confirmation_prompt_rebase() {
        let cmd = GitHubCommand::MergePr {
            repo: "owner/repo".into(),
            pr_number: 10,
            method: MergeMethod::Rebase,
        };
        let prompt = cmd.confirmation_prompt().unwrap();
        assert!(prompt.contains("rebase merge"));
    }

    #[test]
    fn test_required_scopes_default() {
        let cmd = GitHubCommand::MergePr {
            repo: "owner/repo".into(),
            pr_number: 1,
            method: MergeMethod::Squash,
        };
        assert_eq!(cmd.required_scopes(), &["repo"]);
    }

    #[test]
    fn test_conflict_file_serialization() {
        let file = ConflictFile {
            filename: "src/main.rs".into(),
            conflict_count: 2,
            blocks: vec![
                ConflictBlock {
                    start_line: 10,
                    head_content: "let x = 1;".into(),
                    branch_content: "let x = 2;".into(),
                },
                ConflictBlock {
                    start_line: 50,
                    head_content: "fn foo() {}".into(),
                    branch_content: "fn bar() {}".into(),
                },
            ],
        };
        let json = serde_json::to_string(&file).unwrap();
        assert!(json.contains("\"filename\":\"src/main.rs\""));
        assert!(json.contains("\"conflict_count\":2"));
        assert!(json.contains("\"start_line\":50"));
    }

    #[test]
    fn test_merge_method_serde() {
        let m = MergeMethod::Squash;
        let json = serde_json::to_string(&m).unwrap();
        assert_eq!(json, "\"squash\"");

        let m: MergeMethod = serde_json::from_str("\"rebase\"").unwrap();
        assert_eq!(m, MergeMethod::Rebase);
    }

    #[test]
    fn test_collaborator_permission_serde() {
        let p = CollaboratorPermission::Admin;
        let json = serde_json::to_string(&p).unwrap();
        assert_eq!(json, "\"admin\"");

        let p: CollaboratorPermission = serde_json::from_str("\"pull\"").unwrap();
        assert_eq!(p, CollaboratorPermission::Pull);
    }

    #[test]
    fn test_org_role_serde() {
        let r = OrgRole::Admin;
        let json = serde_json::to_string(&r).unwrap();
        assert_eq!(json, "\"admin\"");

        let r: OrgRole = serde_json::from_str("\"member\"").unwrap();
        assert_eq!(r, OrgRole::Member);
    }

    #[test]
    fn test_create_pr_is_not_destructive() {
        let cmd = GitHubCommand::CreatePr {
            repo: "owner/repo".into(),
            title: "New feature".into(),
            head: "feature".into(),
            base: "main".into(),
            body: None,
            draft: false,
        };
        assert!(!cmd.is_destructive());
        assert!(cmd.confirmation_prompt().is_none());
    }

    #[test]
    fn test_revert_pr_is_destructive() {
        let cmd = GitHubCommand::RevertPr {
            repo: "owner/repo".into(),
            pr_number: 42,
            title: None,
        };
        assert!(cmd.is_destructive());
        let prompt = cmd.confirmation_prompt().unwrap();
        assert!(prompt.contains("revert"));
        assert!(prompt.contains("PR #42"));
    }

    #[test]
    fn test_update_branch_is_not_destructive() {
        let cmd = GitHubCommand::UpdateBranch {
            repo: "owner/repo".into(),
            pr_number: 1,
        };
        assert!(!cmd.is_destructive());
    }

    #[test]
    fn test_list_pr_files_is_not_destructive() {
        let cmd = GitHubCommand::ListPrFiles {
            repo: "owner/repo".into(),
            pr_number: 1,
        };
        assert!(!cmd.is_destructive());
    }

    #[test]
    fn test_comment_pr_is_not_destructive() {
        let cmd = GitHubCommand::CommentPr {
            repo: "owner/repo".into(),
            pr_number: 1,
            body: "Nice work".into(),
        };
        assert!(!cmd.is_destructive());
    }

    #[test]
    fn test_create_pr_serialization() {
        let cmd = GitHubCommand::CreatePr {
            repo: "owner/repo".into(),
            title: "Add feature X".into(),
            head: "feature-x".into(),
            base: "main".into(),
            body: Some("This adds X".into()),
            draft: true,
        };
        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains("\"command\":\"create_pr\""));
        assert!(json.contains("\"title\":\"Add feature X\""));
        assert!(json.contains("\"head\":\"feature-x\""));
        assert!(json.contains("\"base\":\"main\""));
        assert!(json.contains("\"draft\":true"));

        let de: GitHubCommand = serde_json::from_str(&json).unwrap();
        match de {
            GitHubCommand::CreatePr {
                title, draft, ..
            } => {
                assert_eq!(title, "Add feature X");
                assert!(draft);
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn test_revert_pr_serialization() {
        let cmd = GitHubCommand::RevertPr {
            repo: "owner/repo".into(),
            pr_number: 99,
            title: Some("Custom revert title".into()),
        };
        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains("\"command\":\"revert_pr\""));
        assert!(json.contains("\"pr_number\":99"));
        assert!(json.contains("\"title\":\"Custom revert title\""));
    }

    #[test]
    fn test_comment_pr_serialization() {
        let cmd = GitHubCommand::CommentPr {
            repo: "owner/repo".into(),
            pr_number: 5,
            body: "LGTM".into(),
        };
        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains("\"command\":\"comment_pr\""));
        assert!(json.contains("\"body\":\"LGTM\""));
    }

    #[test]
    fn test_update_branch_serialization() {
        let cmd = GitHubCommand::UpdateBranch {
            repo: "owner/repo".into(),
            pr_number: 7,
        };
        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains("\"command\":\"update_branch\""));
        assert!(json.contains("\"pr_number\":7"));
    }

    #[test]
    fn test_list_pr_files_serialization() {
        let cmd = GitHubCommand::ListPrFiles {
            repo: "owner/repo".into(),
            pr_number: 3,
        };
        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains("\"command\":\"list_pr_files\""));
    }

    // ─── Phase 2A-4: Collaborator + Org tests ──────────────────────

    #[test]
    fn test_add_collaborator_is_destructive() {
        let cmd = GitHubCommand::AddCollaborator {
            repo: "owner/repo".into(),
            username: "newuser".into(),
            permission: CollaboratorPermission::Push,
        };
        assert!(cmd.is_destructive());
        let prompt = cmd.confirmation_prompt().unwrap();
        assert!(prompt.contains("newuser"));
        assert!(prompt.contains("push"));
    }

    #[test]
    fn test_remove_collaborator_is_destructive() {
        let cmd = GitHubCommand::RemoveCollaborator {
            repo: "owner/repo".into(),
            username: "olduser".into(),
        };
        assert!(cmd.is_destructive());
        assert!(cmd.confirmation_prompt().unwrap().contains("remove"));
    }

    #[test]
    fn test_list_collaborators_is_not_destructive() {
        let cmd = GitHubCommand::ListCollaborators {
            repo: "owner/repo".into(),
        };
        assert!(!cmd.is_destructive());
        assert!(cmd.confirmation_prompt().is_none());
    }

    #[test]
    fn test_add_org_member_is_destructive() {
        let cmd = GitHubCommand::AddOrgMember {
            org: "myorg".into(),
            username: "newmember".into(),
            role: OrgRole::Member,
        };
        assert!(cmd.is_destructive());
        let prompt = cmd.confirmation_prompt().unwrap();
        assert!(prompt.contains("newmember"));
        assert!(prompt.contains("myorg"));
        assert!(prompt.contains("member"));
    }

    #[test]
    fn test_remove_org_member_is_destructive() {
        let cmd = GitHubCommand::RemoveOrgMember {
            org: "myorg".into(),
            username: "oldmember".into(),
        };
        assert!(cmd.is_destructive());
        assert!(cmd.confirmation_prompt().unwrap().contains("remove"));
    }

    #[test]
    fn test_list_org_members_is_not_destructive() {
        let cmd = GitHubCommand::ListOrgMembers { org: "myorg".into() };
        assert!(!cmd.is_destructive());
    }

    #[test]
    fn test_convert_to_outside_collaborator_is_destructive() {
        let cmd = GitHubCommand::ConvertToOutsideCollaborator {
            org: "myorg".into(),
            username: "someone".into(),
        };
        assert!(cmd.is_destructive());
        assert!(cmd.confirmation_prompt().unwrap().contains("outside collaborator"));
    }

    #[test]
    fn test_list_outside_collaborators_is_not_destructive() {
        let cmd = GitHubCommand::ListOutsideCollaborators { org: "myorg".into() };
        assert!(!cmd.is_destructive());
    }

    #[test]
    fn test_add_collaborator_serialization() {
        let cmd = GitHubCommand::AddCollaborator {
            repo: "owner/repo".into(),
            username: "user1".into(),
            permission: CollaboratorPermission::Admin,
        };
        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains("\"command\":\"add_collaborator\""));
        assert!(json.contains("\"username\":\"user1\""));
        assert!(json.contains("\"permission\":\"admin\""));
    }

    #[test]
    fn test_remove_collaborator_serialization() {
        let cmd = GitHubCommand::RemoveCollaborator {
            repo: "owner/repo".into(),
            username: "user1".into(),
        };
        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains("\"command\":\"remove_collaborator\""));
    }

    #[test]
    fn test_add_org_member_serialization() {
        let cmd = GitHubCommand::AddOrgMember {
            org: "myorg".into(),
            username: "user1".into(),
            role: OrgRole::Admin,
        };
        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains("\"command\":\"add_org_member\""));
        assert!(json.contains("\"org\":\"myorg\""));
        assert!(json.contains("\"role\":\"admin\""));
    }

    #[test]
    fn test_remove_org_member_serialization() {
        let cmd = GitHubCommand::RemoveOrgMember {
            org: "myorg".into(),
            username: "user1".into(),
        };
        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains("\"command\":\"remove_org_member\""));
    }

    #[test]
    fn test_convert_to_outside_collaborator_serialization() {
        let cmd = GitHubCommand::ConvertToOutsideCollaborator {
            org: "myorg".into(),
            username: "user1".into(),
        };
        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains("\"command\":\"convert_to_outside_collaborator\""));
    }

    // ─── Phase 2A-5: Branch + Release + Workflow tests ─────────────

    #[test]
    fn test_set_branch_protection_is_destructive() {
        let cmd = GitHubCommand::SetBranchProtection {
            repo: "owner/repo".into(),
            branch: "main".into(),
            required_status_checks: vec!["ci".into()],
            require_pr_reviews: true,
            required_review_count: 2,
            enforce_admins: true,
        };
        assert!(cmd.is_destructive());
        let prompt = cmd.confirmation_prompt().unwrap();
        assert!(prompt.contains("main"));
        assert!(prompt.contains("owner/repo"));
    }

    #[test]
    fn test_delete_branch_is_destructive() {
        let cmd = GitHubCommand::DeleteBranch {
            repo: "owner/repo".into(),
            branch: "feature".into(),
        };
        assert!(cmd.is_destructive());
        assert!(cmd.confirmation_prompt().unwrap().contains("delete"));
    }

    #[test]
    fn test_list_branches_not_destructive() {
        let cmd = GitHubCommand::ListBranches {
            repo: "owner/repo".into(),
        };
        assert!(!cmd.is_destructive());
    }

    #[test]
    fn test_create_release_not_destructive() {
        let cmd = GitHubCommand::CreateRelease {
            repo: "owner/repo".into(),
            tag: "v1.0".into(),
            name: "Release 1.0".into(),
            body: None,
            draft: false,
            prerelease: false,
            target_commitish: None,
        };
        assert!(!cmd.is_destructive());
    }

    #[test]
    fn test_delete_release_is_destructive() {
        let cmd = GitHubCommand::DeleteRelease {
            repo: "owner/repo".into(),
            release_id: 123,
        };
        assert!(cmd.is_destructive());
        assert!(cmd.confirmation_prompt().unwrap().contains("123"));
    }

    #[test]
    fn test_list_workflows_not_destructive() {
        let cmd = GitHubCommand::ListWorkflows {
            repo: "owner/repo".into(),
        };
        assert!(!cmd.is_destructive());
    }

    #[test]
    fn test_list_workflow_runs_not_destructive() {
        let cmd = GitHubCommand::ListWorkflowRuns {
            repo: "owner/repo".into(),
            workflow_file: Some("ci.yml".into()),
        };
        assert!(!cmd.is_destructive());
    }

    #[test]
    fn test_rerun_workflow_not_destructive() {
        let cmd = GitHubCommand::RerunWorkflow {
            repo: "owner/repo".into(),
            run_id: 999,
        };
        assert!(!cmd.is_destructive());
    }

    #[test]
    fn test_cancel_workflow_is_destructive() {
        let cmd = GitHubCommand::CancelWorkflow {
            repo: "owner/repo".into(),
            run_id: 999,
        };
        assert!(cmd.is_destructive());
        assert!(cmd.confirmation_prompt().unwrap().contains("cancel"));
    }

    #[test]
    fn test_set_branch_protection_serialization() {
        let cmd = GitHubCommand::SetBranchProtection {
            repo: "owner/repo".into(),
            branch: "main".into(),
            required_status_checks: vec!["ci".into(), "lint".into()],
            require_pr_reviews: true,
            required_review_count: 1,
            enforce_admins: false,
        };
        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains("\"command\":\"set_branch_protection\""));
        assert!(json.contains("\"branch\":\"main\""));
        assert!(json.contains("\"require_pr_reviews\":true"));
    }

    #[test]
    fn test_delete_branch_serialization() {
        let cmd = GitHubCommand::DeleteBranch {
            repo: "owner/repo".into(),
            branch: "old".into(),
        };
        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains("\"command\":\"delete_branch\""));
    }

    #[test]
    fn test_create_release_serialization() {
        let cmd = GitHubCommand::CreateRelease {
            repo: "owner/repo".into(),
            tag: "v2.0".into(),
            name: "Release 2.0".into(),
            body: Some("Changes".into()),
            draft: true,
            prerelease: false,
            target_commitish: Some("main".into()),
        };
        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains("\"command\":\"create_release\""));
        assert!(json.contains("\"tag\":\"v2.0\""));
        assert!(json.contains("\"draft\":true"));
    }

    #[test]
    fn test_cancel_workflow_serialization() {
        let cmd = GitHubCommand::CancelWorkflow {
            repo: "owner/repo".into(),
            run_id: 42,
        };
        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains("\"command\":\"cancel_workflow\""));
        assert!(json.contains("\"run_id\":42"));
    }
}
