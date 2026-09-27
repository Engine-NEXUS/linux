# 10 — GitHub Sub-Command System Architecture

> **Status**: Implemented 2026-09-04
> **Files**: `src-tauri/src/github_cmd.rs`, `src-tauri/src/intent_parser.rs`,
>   `src-tauri/src/orchestrator.rs`, `frontend/src/net/orchestrator.ts`,
>   `frontend/src/sidebar/GitHubConflictPanel.tsx`
> **Tests**: 243 Rust lib tests (114 new), 28 Worker tests, 10 offline tests
> **Dependencies**: `octocrab = "0.42"`, `async-trait = "0.1"`

---

## 1. Problem Statement

The NEXUS GitHub control surface was a collection of regex patterns in the
Cloudflare Worker's `handleGitHubWrite()` function. This approach had
fundamental architectural problems:

1. **No typed command model** — every operation was a regex match against
   free text. There was no validation, no type safety, no exhaustive
   matching, and no way to discover available commands.

2. **No centralized confirmation** — destructive operations (merge, close,
   delete) had ad-hoc confirmation logic scattered across handler
   functions. A "yes" from one confirmation could theoretically apply to
   a different command.

3. **No conflict detection** — merge attempts returned generic failure
   messages without explaining which files conflicted, what the conflict
   markers were, or how to fix them. The user had to go to GitHub's web
   UI to understand the conflict.

4. **Token exposure** — the Worker returned raw GitHub OAuth tokens to the
   frontend, which then made GitHub API calls directly. This increased the
   attack surface and made token rotation difficult.

5. **Unmaintainable regex soup** — each new GitHub operation added another
   regex block to the Worker's `handleGitHubWrite()` function. The function
   was growing without bound and becoming increasingly fragile.

6. **No dry-run/preflight** — operations executed without checking
   preconditions. A merge would fail with a 405 from GitHub instead of
   being blocked before the attempt with a meaningful message.

7. **No structured results** — the Worker returned text strings, not typed
   results. The frontend couldn't distinguish a merge conflict from a
   permission error from a successful operation.

### The Old Flow (Before Phase 2A)

```
User speaks: "merge PR 23 in owner/repo"
  → STT → transcript
  → orchestrator::process_transcript()
  → intent_parser::parse_deterministic()
  → ParsedIntent::Unknown { raw: "merge PR 23 in owner/repo" }
  → route_intent() → Subsystem::WorkerBackend
  → dispatch_to_worker()
  → POST to Worker /main
  → Worker handleGitHubWrite():
      1. Regex match "merge" + "PR" + number + repo
      2. Fetch token from D1
      3. fetch("https://api.github.com/repos/.../pulls/.../merge")
      4. If 405: return "Merge conflict" (generic)
      5. If success: return "Merged" (text string)
  → Worker returns text
  → Frontend speaks text
```

Problems with this flow:
- The regex could fail on STT mishearings ("merge pull request" vs "merge PR")
- No conflict details — just "Merge conflict" text
- No confirmation before destructive merge
- Token traveled through the frontend
- No structured result for UI rendering

### The New Flow (After Phase 2A)

```
User speaks: "squash merge PR 23 in owner/repo"
  → STT → transcript
  → orchestrator::process_transcript()
  → intent_parser::parse_deterministic()
  → parse_github_command()
  → ParsedIntent::GitHubCommand {
      command: GitHubCommand::MergePr {
        repo: "owner/repo",
        pr_number: 23,
        method: MergeMethod::Squash
      }
    }
  → route_intent() → Subsystem::GitHub
  → Subsystem::GitHub dispatch arm:
      1. Emit Ack ("On it sir")
      2. Emit Loading (visible: true)
      3. Get session info (worker_url, user_id)
      4. github_cmd::execute_command(worker_url, user_id, &cmd, confirmed=false)
         a. build_octocrab_client()
            → get_github_token() → check cache → fetch from Worker if stale
            → construct octocrab::Octocrab with personal_token
         b. command.pre_check(&client)
            → Fetch PR, check mergeable_state
            → If Dirty: return MergeConflict with file/block details
         c. command.is_destructive() && !confirmed
            → return NeedsConfirmation { prompt, command }
         d. command.execute(&client)
            → octocrab API call
            → return Text / Error / MergeConflict
      5. Match result type → emit appropriate event
      6. Emit Done
```

## 2. Architecture Overview

```
┌─────────────────────────────────────────────────────────────────┐
│                    Central Orchestrator                         │
│                   (orchestrator.rs)                             │
│                                                                 │
│  process_transcript()                                           │
│    → parse_deterministic()  ──────────────────────┐            │
│    → route_intent()  → Subsystem::GitHub           │            │
│    → Subsystem::GitHub dispatch arm                │            │
│        → github_cmd::execute_command()             │            │
│        → emit OrchestratorEvent                    │            │
│            → Confirm / ConflictReport /            │            │
│              Result / Error / GitHubResult         │            │
└────────────────────────────────────────────────────│───────────┘
                                                     │
┌────────────────────────────────────────────────────▼───────────┐
│                  github_cmd.rs                                  │
│              (GitHub Sub-Command System)                        │
│                                                                 │
│  ┌──────────────────────────────────────────────────────────┐  │
│  │  GitHubCommand enum (28 variants)                        │  │
│  │  ├── PR Operations (10)                                  │  │
│  │  ├── Collaborator + Org (8)                              │  │
│  │  └── Branch + Release + Workflow (10)                    │  │
│  └──────────────────────────────────────────────────────────┘  │
│                                                                 │
│  ┌──────────────────────────────────────────────────────────┐  │
│  │  GitHubExecutable trait                                  │  │
│  │  ├── is_destructive() → bool                             │  │
│  │  ├── confirmation_prompt() → Option<String>              │  │
│  │  ├── required_scopes() → &[&str]                         │  │
│  │  ├── pre_check() → Result<(), GitHubResult>              │  │
│  │  └── execute() → GitHubResult                            │  │
│  └──────────────────────────────────────────────────────────┘  │
│                                                                 │
│  ┌──────────────────────────────────────────────────────────┐  │
│  │  GitHubResult enum                                       │  │
│  │  ├── Text { text }                                       │  │
│  │  ├── NeedsConfirmation { prompt, command }               │  │
│  │  ├── MergeConflict { pr_number, repo, conflict_files }   │  │
│  │  └── Error { message, status, is_auth_error }            │  │
│  └──────────────────────────────────────────────────────────┘  │
│                                                                 │
│  ┌──────────────────────────────────────────────────────────┐  │
│  │  Token Management                                        │  │
│  │  ├── GITHUB_TOKEN: Lazy<Arc<RwLock<Option<CachedToken>>>>│  │
│  │  ├── get_github_token(worker_url, user_id)               │  │
│  │  ├── fetch_token_from_worker() → GET /oauth/github-token │  │
│  │  ├── build_octocrab_client() → Octocrab                  │  │
│  │  └── clear_github_token()                                │  │
│  └──────────────────────────────────────────────────────────┘  │
│                                                                 │
│  ┌──────────────────────────────────────────────────────────┐  │
│  │  Conflict Detection                                      │  │
│  │  ├── fetch_conflict_details() → GET /pulls/{n}/files     │  │
│  │  └── extract_conflict_blocks_from_patch()                │  │
│  │      → Parses <<<<<<< ======= >>>>>>> from diff patches  │  │
│  └──────────────────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────────────────┘
         │
         │ HTTP (token fetch only)
         ▼
┌─────────────────────────────────────────────────────────────────┐
│              Cloudflare Worker (server/worker)                  │
│                                                                 │
│  GET /oauth/github-token?user_id=<id>                           │
│    → getValidGithubToken(env, userId)                           │
│    → Returns { token: "gho_..." }                               │
│    → Handles token refresh internally                           │
│    → Returns 404 if GitHub not connected                        │
└─────────────────────────────────────────────────────────────────┘
         │
         │ HTTPS (GitHub API calls)
         ▼
┌─────────────────────────────────────────────────────────────────┐
│                   GitHub API (api.github.com)                   │
│                                                                 │
│  Called directly from Rust via octocrab                         │
│  Authorization: Bearer <token>                                  │
│  Accept: application/vnd.github+json                            │
│  X-GitHub-Api-Version: 2022-11-28                               │
└─────────────────────────────────────────────────────────────────┘
```

## 3. The GitHubCommand Enum

The core of the system is a closed enum of 28 variants. Each variant
carries exactly the data it needs — no more, no less. The enum is
`#[serde(tag = "command")]` for tagged JSON serialization, making it
easy to log, replay, and pass across the Tauri IPC boundary.

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "command")]
#[serde(rename_all = "snake_case")]
pub enum GitHubCommand {
    // PR Operations
    MergePr { repo: String, pr_number: u64, method: MergeMethod },
    ApprovePr { repo: String, pr_number: u64 },
    ClosePr { repo: String, pr_number: u64 },
    ListPrs { repo: String, state: String },
    GetPr { repo: String, pr_number: u64 },
    CreatePr { repo: String, title: String, head: String, base: String, body: Option<String>, draft: bool },
    UpdateBranch { repo: String, pr_number: u64 },
    RevertPr { repo: String, pr_number: u64, title: Option<String> },
    ListPrFiles { repo: String, pr_number: u64 },
    CommentPr { repo: String, pr_number: u64, body: String },

    // Collaborator + Organization
    AddCollaborator { repo: String, username: String, permission: CollaboratorPermission },
    RemoveCollaborator { repo: String, username: String },
    ListCollaborators { repo: String },
    AddOrgMember { org: String, username: String, role: OrgRole },
    RemoveOrgMember { org: String, username: String },
    ListOrgMembers { org: String },
    ConvertToOutsideCollaborator { org: String, username: String },
    ListOutsideCollaborators { org: String },

    // Branch + Release + Workflow
    SetBranchProtection { repo: String, branch: String, required_status_checks: Vec<String>, require_pr_reviews: bool, required_review_count: u8, enforce_admins: bool },
    DeleteBranch { repo: String, branch: String },
    ListBranches { repo: String },
    CreateRelease { repo: String, tag: String, name: String, body: Option<String>, draft: bool, prerelease: bool, target_commitish: Option<String> },
    ListReleases { repo: String },
    DeleteRelease { repo: String, release_id: u64 },
    ListWorkflows { repo: String },
    ListWorkflowRuns { repo: String, workflow_file: Option<String> },
    RerunWorkflow { repo: String, run_id: u64 },
    CancelWorkflow { repo: String, run_id: u64 },
}
```

### Supporting Enums

```rust
pub enum MergeMethod { Merge, Squash, Rebase }  // default: Squash
pub enum CollaboratorPermission { Pull, Triage, Push, Maintain, Admin }  // default: Push
pub enum OrgRole { Member, Admin }  // default: Member
```

## 4. The GitHubExecutable Trait

Every command variant implements this trait, which defines the full
lifecycle of a GitHub operation:

```rust
#[async_trait]
pub trait GitHubExecutable {
    /// Does this command modify GitHub state?
    /// Destructive commands require user confirmation.
    fn is_destructive(&self) -> bool;

    /// The confirmation prompt for destructive commands.
    /// Returns None for non-destructive commands.
    fn confirmation_prompt(&self) -> Option<String>;

    /// Required OAuth scopes for this command.
    fn required_scopes(&self) -> &'static [&'static str] { &["repo"] }

    /// Pre-check before execution.
    /// For merge: checks mergeable_state, returns MergeConflict if dirty.
    /// Returns Ok(()) if safe to proceed.
    async fn pre_check(&self, _client: &Octocrab) -> Result<(), GitHubResult> { Ok(()) }

    /// Execute the command against the GitHub API.
    async fn execute(&self, client: &Octocrab) -> GitHubResult;
}
```

### Destructive Classification

11 of 28 commands are destructive and require confirmation:

| Command | Why Destructive |
|---------|-----------------|
| `MergePr` | Modifies the base branch — irreversible without revert |
| `ClosePr` | Closes a PR — may need re-creation |
| `RevertPr` | Creates a revert PR — reverses merged work |
| `AddCollaborator` | Grants repository access to a new user |
| `RemoveCollaborator` | Revokes repository access |
| `AddOrgMember` | Grants org membership |
| `RemoveOrgMember` | Revokes org membership |
| `ConvertToOutsideCollaborator` | Changes member role — may reduce access |
| `SetBranchProtection` | Modifies branch protection rules |
| `DeleteBranch` | Deletes a branch ref — cannot be undone |
| `DeleteRelease` | Deletes a release — assets are lost |
| `CancelWorkflow` | Stops in-progress CI work |

## 5. The GitHubResult Enum

Results are typed, not text strings. This allows the frontend to render
different UI for each result type:

```rust
pub enum GitHubResult {
    /// Simple text result — spoken to the user.
    Text { text: String },

    /// Destructive operation needs confirmation.
    /// Orchestrator emits a Confirm event; user must say "yes".
    NeedsConfirmation { prompt: String, command: GitHubCommand },

    /// Merge conflict detected — don't attempt the merge.
    /// Frontend displays conflict details with copy-paste options.
    MergeConflict {
        pr_number: u64,
        repo: String,
        conflict_files: Vec<ConflictFile>,
        message: String,
    },

    /// Error from the GitHub API or local validation.
    Error {
        message: String,
        status: Option<u16>,
        is_auth_error: bool,  // 401/403 → user should reconnect
    },
}
```

## 6. Token Management

### Architecture

```
┌──────────────┐     GET /oauth/github-token     ┌──────────────┐
│  github_cmd  │ ──────────────────────────────► │    Worker    │
│  (Rust)      │                                 │  (D1-backed) │
│              │ ◄────────────────────────────── │              │
│  GITHUB_TOKEN│     { token: "gho_..." }        │  oauth_tokens│
│  (in-memory) │                                 │  table       │
└──────────────┘                                 └──────────────┘
```

### Caching Logic

```rust
static GITHUB_TOKEN: Lazy<Arc<RwLock<Option<CachedToken>>>> = ...;

struct CachedToken {
    token: String,
    expires_at: f64,   // unix timestamp; 0 = no expiry (classic OAuth)
    fetched_at: f64,
}

impl CachedToken {
    fn is_valid(&self) -> bool {
        if self.expires_at == 0.0 { return true; }  // classic token
        let now = chrono::Utc::now().timestamp() as f64;
        now < self.expires_at - 300.0  // refresh 5 min before expiry
    }
}
```

### Security Properties

- Token is **never** written to disk
- Token is **never** logged (tracing logs the fetch URL only)
- Token is **never** exposed to the frontend
- Token cache auto-refreshes 5 minutes before expiry
- `clear_github_token()` clears the cache on disconnect
- The Worker's `getValidGithubToken()` handles refresh-token rotation

## 7. Conflict Detection

### When It Triggers

The `pre_check()` method on `MergePr` fetches the PR and inspects:
- `pr.mergeable` (bool) — whether the PR can be merged cleanly
- `pr.mergeable_state` (MergeableState enum) — the detailed state

If `!mergeable || mergeable_state == Dirty`, the merge is **not attempted**.
Instead, the system fetches conflict details and returns a `MergeConflict`
result.

### Conflict Detail Extraction

```
fetch_conflict_details(client, repo, pr_number)
  → GET /repos/{owner}/{repo}/pulls/{pr_number}/files
  → For each file in the PR:
      → Check if the patch contains <<<<<<< and >>>>>>> markers
      → If yes: extract_conflict_blocks_from_patch(patch)
        → Parse +<<<<<<< HEAD / +======= / +>>>>>>> markers
        → Build ConflictBlock { start_line, head_content, branch_content }
      → Build ConflictFile { filename, conflict_count, blocks }
  → Return Vec<ConflictFile>
```

### What the User Sees

1. **Voice**: "PR #23 in owner/repo has merge conflicts. 2 files have
   conflicts. Please fix the conflicts and push, then try merging again."

2. **Frontend UI** (GitHubConflictPanel.tsx):
   - PR number and repo header
   - Each conflicted file with a conflict count badge
   - Each conflict block showing:
     - Line number
     - HEAD (base) content with a "Copy" button
     - Branch (feature) content with a "Copy" button
     - "Copy Full Conflict Block" button (copies the full
       `<<<<<<< HEAD ... ======= ... >>>>>>> branch` text)
   - Step-by-step fix instructions
   - "Retry Merge" button

### Limitations

GitHub's REST API provides conflict markers **only** in the diff patch
returned by the `/pulls/{n}/files` endpoint. This patch may be truncated
for large PRs. If the patch is not available, the system reports the
conflict without file-level details and directs the user to GitHub's web UI.

The system **never invents** conflict files or conflict markers. If the
data isn't available from the API, it says so.

## 8. Intent Parser Integration

### New ParsedIntent Variant

```rust
pub enum ParsedIntent {
    // ... existing variants ...
    GitHubCommand { command: crate::github_cmd::GitHubCommand },
    // ...
}
```

### Parser Ordering

The GitHub parser runs **before** the open/close app parsers in
`parse_deterministic()`. This is critical because:

- "close PR 10 in owner/repo" would match `parse_close_command()` as
  "close app: PR 10 in owner/repo"
- "show PR 42 in owner/repo" would match `parse_open_command()` as
  "open app: PR 42 in owner/repo"

By running the GitHub parser first, these commands are correctly
identified as GitHub operations.

### Pattern Matching

The `parse_github_command()` function uses 26 regex patterns to match
natural language GitHub commands. Each pattern extracts the relevant
parameters (repo, PR number, username, etc.) and constructs the
appropriate `GitHubCommand` variant.

The parser also handles the `extract_repo()` helper, which finds
"in <owner/repo>" or "for <owner/repo>" at the end of the text.

### Routing

```rust
fn route_intent(intent: &ParsedIntent) -> Subsystem {
    match intent {
        // ...
        ParsedIntent::GitHubCommand { .. } => Subsystem::GitHub,
        // ...
    }
}
```

## 9. Orchestrator Integration

### New Event Types

```rust
pub enum OrchestratorEvent {
    // ... existing variants ...

    /// GitHub: confirmation required for a destructive operation.
    Confirm { prompt: String, request_id: String, command: serde_json::Value },

    /// GitHub: merge conflict detected.
    ConflictReport {
        request_id: String,
        pr_number: u64,
        repo: String,
        conflict_files: serde_json::Value,
        message: String,
    },

    /// GitHub: raw structured result for advanced UI rendering.
    GitHubResult { request_id: String, result: serde_json::Value },
}
```

### Dispatch Arm

The `Subsystem::GitHub` dispatch arm in `process_transcript()`:

1. Emits Ack + Loading events
2. Gets session info (worker_url, user_id)
3. Extracts the `GitHubCommand` from the parsed intent
4. Calls `github_cmd::execute_command()`
5. Matches the result type and emits the appropriate event
6. Emits `GitHubResult` (raw structured data) + `Done`

If the intent somehow doesn't contain a `GitHubCommand` (backward
compatibility), it falls back to the Worker dispatch.

### New Tauri Commands

- `orchestrator_github_execute` — execute a GitHub command from the
  frontend with optional `confirmed` flag
- `orchestrator_github_clear_token` — clear the cached GitHub token

## 10. Frontend Integration

### Event Handling (orchestrator.ts)

The frontend orchestrator listener handles three new event types:

- **`confirm`**: Speaks the confirmation prompt. The user says "yes" to
  confirm, which triggers `orchestrator_github_execute` with `confirmed=true`.

- **`conflict_report`**: Speaks the conflict summary and logs conflict
  details for the `GitHubConflictPanel` to display.

- **`github_result`**: Logs the raw structured result for advanced UI
  rendering.

### Conflict UI (GitHubConflictPanel.tsx)

A React component that renders:
- PR number and repo header
- Each conflicted file with a conflict count badge
- Each conflict block with HEAD content, branch content, and copy buttons
- Full conflict block copy button
- Step-by-step fix instructions
- Retry merge button

### CSS (sidebar.css)

181 lines of styles for the conflict panel, including:
- Red-tinted background for conflict warning
- Monospace font for file names and conflict content
- Copy buttons with hover states
- Scrollable conflict content (max-height: 200px)
- Responsive layout

## 11. Testing Strategy

### Unit Tests (114 new tests)

**github_cmd.rs (87 tests)**:
- `split_repo()` — owner/repo parsing
- `MergeMethod`, `CollaboratorPermission`, `OrgRole` — serde round-trips
- `is_destructive()` — all 28 commands classified correctly
- `confirmation_prompt()` — all 11 destructive commands have prompts
- `required_scopes()` — default scope check
- Command serialization — all 28 variants serialize/deserialize correctly
- `GitHubResult` serialization — all 4 variants
- `ConflictFile` / `ConflictBlock` serialization
- `extract_conflict_blocks_from_patch()` — single, multiple, no conflicts
- `CachedToken::is_valid()` — classic, valid, expired, soon-to-expire

**intent_parser.rs (27 tests)**:
- `parse_merge_pr()` — merge, squash, rebase methods
- `parse_approve_pr()`, `parse_close_pr()`, `parse_get_pr()`
- `parse_list_prs()` — open, closed states
- `parse_revert_pr()`
- `parse_add_collaborator()` — with and without permission
- `parse_remove_collaborator()`, `parse_list_collaborators()`
- `parse_add_org_member()` — member and admin roles
- `parse_remove_org_member()`, `parse_list_org_members()`
- `parse_list_branches()`, `parse_delete_branch()`
- `parse_list_releases()`, `parse_list_workflows()`
- `parse_list_workflow_runs()`, `parse_rerun_workflow()`
- `parse_cancel_workflow()`
- `route_github_command_to_github_subsystem()`
- `non_github_not_routed_to_github()`

### Test Gates (14 gates, all passing)

Each phase had two test gates:
1. `cargo check` + `cargo test --lib`
2. `cargo check --features mock-wake` + `npm run build`

Phase 2A-7 additionally ran Worker tests (`npm test` in `server/worker/`)
and offline command tests (`cargo test --test offline_commands`).

## 12. Future Work

### Not Implemented in Phase 2A

1. **Worker-proxied GitHub operations** — proxy all GitHub API calls
   through the Worker so the token never reaches the desktop client.

2. **GitHub App installation tokens** — migrate from long-lived OAuth
   tokens to short-lived GitHub App user-to-server tokens with
   fine-grained repository selection.

3. **Pagination support** — `ListPrs`, `ListCollaborators`, `ListOrgMembers`,
   `ListBranches`, `ListReleases`, `ListWorkflows`, `ListWorkflowRuns`
   currently return only the first page (10-30 items). Add pagination
   for large repos.

4. **Rate limit handling** — GitHub API rate limits (5000 req/hour for
   authenticated users) are not currently tracked. Add rate limit
   monitoring and backoff.

5. **Command discovery** — add a "what GitHub commands are available"
   intent that lists all 28 commands with examples.

6. **Dry-run/preview mode** — for destructive commands, show what would
   happen before executing (e.g., "This will merge 15 files, +234 -56
   lines, into the main branch").

7. **Local clone conflict resolution** — for repos cloned locally,
   run `git diff` to get actual conflict markers instead of relying
   on the API's diff patch (which may be truncated).

8. **SSO/enterprise policy handling** — organization SAML SSO and
   enterprise managed policies may block certain operations. Add
   explicit handling for these cases.

9. **Webhook integration** — subscribe to GitHub webhooks for real-time
   PR/CI updates instead of polling.

10. **Batch operations** — "approve all PRs by user X" or "close all
    draft PRs older than 30 days".
