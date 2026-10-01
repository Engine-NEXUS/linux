# 48 — GitHub Sub-Command System (Phase 2A)

> **Date**: 2026-09-04
> **Status**: Implemented and tested
> **Phases**: 2A-1 through 2A-7 (7 phases, 14 test gates)
> **Tests**: 114 new tests (87 in github_cmd.rs, 27 in intent_parser.rs)
> **Dependencies**: `octocrab = "0.42"`, `async-trait = "0.1"`

---

## Overview

The GitHub Sub-Command System is a typed Rust subsystem beneath the central
orchestrator that handles all GitHub operations through 28 structured
command variants. It replaces the Worker's regex-based `handleGitHubWrite()`
with a type-safe, testable, conflict-aware command system.

The system supports:
- **28 GitHub commands** across PR, collaborator, org, branch, release, and
  workflow operations
- **Natural language parsing** — 26 regex patterns parse voice transcripts
  into typed commands
- **Conflict detection** — merge operations pre-check `mergeable_state` and
  return detailed conflict reports with copy-paste options
- **Centralized confirmation** — 11 destructive commands require explicit
  user confirmation before executing
- **Token security** — GitHub OAuth token fetched from Worker, cached in
  memory, never exposed to frontend or disk
- **Typed results** — Text, NeedsConfirmation, MergeConflict, Error events
  enable structured frontend rendering

## Architecture

```
Voice: "squash merge PR 23 in owner/repo"
  │
  ▼
┌─────────────────────────────────────────────────────────┐
│  intent_parser.rs                                       │
│  parse_github_command()                                 │
│  → 26 regex patterns                                    │
│  → ParsedIntent::GitHubCommand {                        │
│      command: GitHubCommand::MergePr {                  │
│        repo: "owner/repo",                              │
│        pr_number: 23,                                   │
│        method: MergeMethod::Squash                      │
│      }                                                  │
│    }                                                    │
└─────────────────────────────────────────────────────────┘
  │
  ▼
┌─────────────────────────────────────────────────────────┐
│  orchestrator.rs                                        │
│  route_intent() → Subsystem::GitHub                     │
│  process_transcript() → Subsystem::GitHub dispatch:     │
│    1. Emit Ack ("On it sir")                            │
│    2. Emit Loading (visible: true)                      │
│    3. github_cmd::execute_command()                     │
│    4. Emit result event                                 │
│    5. Emit Done                                         │
└─────────────────────────────────────────────────────────┘
  │
  ▼
┌─────────────────────────────────────────────────────────┐
│  github_cmd.rs                                          │
│  execute_command(worker_url, user_id, &cmd, confirmed)  │
│                                                         │
│  Step 1: build_octocrab_client()                        │
│    → get_github_token()                                 │
│    → Check in-memory cache                              │
│    → If stale: fetch_token_from_worker()                │
│      → GET {worker_url}/oauth/github-token?user_id=...  │
│    → Construct Octocrab with personal_token             │
│                                                         │
│  Step 2: command.pre_check(&client)                     │
│    → For MergePr: fetch PR, check mergeable_state       │
│    → If Dirty: return MergeConflict result              │
│                                                         │
│  Step 3: if is_destructive() && !confirmed              │
│    → return NeedsConfirmation { prompt, command }       │
│                                                         │
│  Step 4: command.execute(&client)                       │
│    → octocrab API call (PUT/POST/PATCH/DELETE/GET)      │
│    → Return Text / Error / MergeConflict                │
└─────────────────────────────────────────────────────────┘
  │
  ▼
┌─────────────────────────────────────────────────────────┐
│  Frontend (orchestrator.ts)                             │
│                                                         │
│  Event: "confirm"                                       │
│    → Speak prompt: "Are you sure you want to squash     │
│      merge PR #23 in owner/repo?"                       │
│    → User says "yes"                                    │
│    → Call orchestrator_github_execute(confirmed=true)   │
│                                                         │
│  Event: "conflict_report"                               │
│    → Speak: "PR #23 has merge conflicts. 2 files..."   │
│    → Display GitHubConflictPanel with copy-paste        │
│                                                         │
│  Event: "result"                                        │
│    → Speak: "PR #23 has been squash merged"             │
│                                                         │
│  Event: "error"                                         │
│    → Speak: "Your GitHub token has expired..."          │
└─────────────────────────────────────────────────────────┘
```

## The 28 Commands

### PR Operations (10)

#### MergePr
```rust
MergePr { repo: String, pr_number: u64, method: MergeMethod }
```
- **Destructive**: Yes — requires confirmation
- **Pre-check**: Fetches PR, checks `mergeable_state`. If `Dirty`, returns
  `MergeConflict` with file-level conflict details.
- **API**: `PUT /repos/{owner}/{repo}/pulls/{pr_number}/merge`
- **Voice patterns**:
  - "merge PR 23 in owner/repo" → MergePr (merge)
  - "squash merge PR 23 in owner/repo" → MergePr (squash)
  - "rebase merge PR 23 in owner/repo" → MergePr (rebase)
- **Confirmation**: "Are you sure you want to squash merge PR #23 in
  owner/repo? Say yes to confirm."

#### ApprovePr
```rust
ApprovePr { repo: String, pr_number: u64 }
```
- **Destructive**: No
- **API**: `POST /repos/{owner}/{repo}/pulls/{pr_number}/reviews` with
  `event: "APPROVE"`
- **Voice**: "approve PR 5 in owner/repo"

#### ClosePr
```rust
ClosePr { repo: String, pr_number: u64 }
```
- **Destructive**: Yes — requires confirmation
- **API**: `PATCH /repos/{owner}/{repo}/pulls/{pr_number}` with
  `state: "closed"`
- **Voice**: "close PR 10 in owner/repo"

#### ListPrs
```rust
ListPrs { repo: String, state: String }  // "open", "closed", "all"
```
- **Destructive**: No
- **API**: `octocrab::pulls().list().state(...).send()`
- **Voice**: "list PRs in owner/repo", "list closed PRs in owner/repo"

#### GetPr
```rust
GetPr { repo: String, pr_number: u64 }
```
- **Destructive**: No
- **API**: `octocrab::pulls().get(pr_number)`
- **Voice**: "get PR 42 in owner/repo", "show PR 42 in owner/repo"

#### CreatePr
```rust
CreatePr { repo: String, title: String, head: String, base: String, body: Option<String>, draft: bool }
```
- **Destructive**: No (creates, doesn't modify/delete)
- **API**: `octocrab::pulls().create(title, head, base).body(...).draft(...).send()`

#### UpdateBranch
```rust
UpdateBranch { repo: String, pr_number: u64 }
```
- **Destructive**: No
- **API**: `PUT /repos/{owner}/{repo}/pulls/{pr_number}/update-branch`

#### RevertPr
```rust
RevertPr { repo: String, pr_number: u64, title: Option<String> }
```
- **Destructive**: Yes — requires confirmation
- **API**: `POST /repos/{owner}/{repo}/pulls/{pr_number}/revert`
- **Pre-check**: Fetches PR, checks for `merge_commit_sha`. If not merged,
  returns error: "PR #N has not been merged yet — cannot revert."

#### ListPrFiles
```rust
ListPrFiles { repo: String, pr_number: u64 }
```
- **Destructive**: No
- **API**: `GET /repos/{owner}/{repo}/pulls/{pr_number}/files`

#### CommentPr
```rust
CommentPr { repo: String, pr_number: u64, body: String }
```
- **Destructive**: No
- **API**: `POST /repos/{owner}/{repo}/issues/{pr_number}/comments`
  (PRs are issues in GitHub's API)

### Collaborator + Organization (8)

#### AddCollaborator
```rust
AddCollaborator { repo: String, username: String, permission: CollaboratorPermission }
```
- **Destructive**: Yes — requires confirmation
- **API**: `PUT /repos/{owner}/{repo}/collaborators/{username}` with
  `permission: "pull"|"push"|"admin"|"triage"|"maintain"`
- **Voice**: "add user1 as admin collaborator to owner/repo"

#### RemoveCollaborator
```rust
RemoveCollaborator { repo: String, username: String }
```
- **Destructive**: Yes — requires confirmation
- **API**: `DELETE /repos/{owner}/{repo}/collaborators/{username}`

#### ListCollaborators
```rust
ListCollaborators { repo: String }
```
- **Destructive**: No
- **API**: `GET /repos/{owner}/{repo}/collaborators`

#### AddOrgMember
```rust
AddOrgMember { org: String, username: String, role: OrgRole }
```
- **Destructive**: Yes — requires confirmation
- **API**: `PUT /orgs/{org}/memberships/{username}` with
  `role: "member"|"admin"`
- **Note**: If the user is not already a member, GitHub sends an
  invitation. The response includes `state: "pending"`.

#### RemoveOrgMember
```rust
RemoveOrgMember { org: String, username: String }
```
- **Destructive**: Yes — requires confirmation
- **API**: `DELETE /orgs/{org}/memberships/{username}`

#### ListOrgMembers
```rust
ListOrgMembers { org: String }
```
- **Destructive**: No
- **API**: `GET /orgs/{org}/members`

#### ConvertToOutsideCollaborator
```rust
ConvertToOutsideCollaborator { org: String, username: String }
```
- **Destructive**: Yes — requires confirmation
- **API**: `PUT /orgs/{org}/outside_collaborators/{username}`

#### ListOutsideCollaborators
```rust
ListOutsideCollaborators { org: String }
```
- **Destructive**: No
- **API**: `GET /orgs/{org}/outside_collaborators`

### Branch + Release + Workflow (10)

#### SetBranchProtection
```rust
SetBranchProtection {
    repo: String,
    branch: String,
    required_status_checks: Vec<String>,
    require_pr_reviews: bool,
    required_review_count: u8,
    enforce_admins: bool,
}
```
- **Destructive**: Yes — requires confirmation
- **API**: `PUT /repos/{owner}/{repo}/branches/{branch}/protection`

#### DeleteBranch
```rust
DeleteBranch { repo: String, branch: String }
```
- **Destructive**: Yes — requires confirmation
- **API**: `DELETE /repos/{owner}/{repo}/git/refs/heads/{branch}`

#### ListBranches
```rust
ListBranches { repo: String }
```
- **Destructive**: No
- **API**: `GET /repos/{owner}/{repo}/branches`

#### CreateRelease
```rust
CreateRelease {
    repo: String, tag: String, name: String,
    body: Option<String>, draft: bool, prerelease: bool,
    target_commitish: Option<String>,
}
```
- **Destructive**: No (creates new)
- **API**: `POST /repos/{owner}/{repo}/releases`

#### ListReleases
```rust
ListReleases { repo: String }
```
- **Destructive**: No
- **API**: `GET /repos/{owner}/{repo}/releases`

#### DeleteRelease
```rust
DeleteRelease { repo: String, release_id: u64 }
```
- **Destructive**: Yes — requires confirmation
- **API**: `DELETE /repos/{owner}/{repo}/releases/{release_id}`

#### ListWorkflows
```rust
ListWorkflows { repo: String }
```
- **Destructive**: No
- **API**: `GET /repos/{owner}/{repo}/actions/workflows`

#### ListWorkflowRuns
```rust
ListWorkflowRuns { repo: String, workflow_file: Option<String> }
```
- **Destructive**: No
- **API**: `GET /repos/{owner}/{repo}/actions/runs` or
  `GET /repos/{owner}/{repo}/actions/workflows/{workflow_file}/runs`

#### RerunWorkflow
```rust
RerunWorkflow { repo: String, run_id: u64 }
```
- **Destructive**: No (re-triggers, doesn't destroy)
- **API**: `POST /repos/{owner}/{repo}/actions/runs/{run_id}/rerun`

#### CancelWorkflow
```rust
CancelWorkflow { repo: String, run_id: u64 }
```
- **Destructive**: Yes — requires confirmation
- **API**: `POST /repos/{owner}/{repo}/actions/runs/{run_id}/cancel`

## Conflict Detection

### How It Works

When the user says "merge PR 23 in owner/repo":

1. **Pre-check**: `MergePr::pre_check()` fetches the PR via
   `octocrab::pulls().get()`.
2. **Mergeable state**: Checks `pr.mergeable` (bool) and
   `pr.mergeable_state` (enum: `Clean`, `Dirty`, `Blocked`, `Behind`,
   `Draft`, `Unstable`, `Unknown`, `HasHooks`).
3. **If clean**: Proceeds to confirmation → merge.
4. **If dirty**: Does **not** attempt the merge. Instead:
   - Calls `fetch_conflict_details()` which fetches
     `GET /repos/{owner}/{repo}/pulls/{pr_number}/files`
   - For each file, checks the `patch` field for `<<<<<<<` and `>>>>>>>`
     conflict markers
   - Extracts conflict blocks using
     `extract_conflict_blocks_from_patch()` which parses:
     ```
     +<<<<<<< HEAD
     +<base content>
     +=======
     +<branch content>
     +>>>>>>> branch-name
     ```
   - Returns `MergeConflict { pr_number, repo, conflict_files, message }`

### What the User Hears

> "PR #23 in owner/repo has merge conflicts. 2 files have conflicts.
> Please fix the conflicts and push, then try merging again."

### What the Frontend Shows

The `GitHubConflictPanel` component displays:

```
┌─────────────────────────────────────────────────┐
│  Merge Conflict — PR #23                        │
│  owner/repo                                     │
├─────────────────────────────────────────────────┤
│  PR #23 has merge conflicts.                    │
├─────────────────────────────────────────────────┤
│  src/main.rs                          [2 conflicts] │
│  ┌─────────────────────────────────────────────┐   │
│  │  Line 10                                    │   │
│  │  HEAD (base)                    [Copy]      │   │
│  │  fn auth() -> Result<Token> {              │   │
│  │      token::generate()                      │   │
│  │  }                                          │   │
│  │  Branch (feature)               [Copy]      │   │
│  │  fn auth() -> Result<Token> {              │   │
│  │      oauth::exchange(code)                  │   │
│  │  }                                          │   │
│  │  [Copy Full Conflict Block]                 │   │
│  └─────────────────────────────────────────────┘   │
├─────────────────────────────────────────────────┤
│  How to fix:                                    │
│  1. Copy the HEAD or branch content             │
│  2. Resolve the conflicts in your local clone   │
│  3. Push the resolved changes to the PR branch  │
│  4. Ask NEXUS to merge the PR again             │
│  [Retry Merge]                                  │
└─────────────────────────────────────────────────┘
```

### Limitations

- GitHub's API provides conflict markers only in the diff patch from
  `/pulls/{n}/files`. For large PRs, the patch may be truncated.
- If no patch is available, the system reports the conflict without
  file-level details and directs the user to GitHub's web UI.
- The system **never invents** conflict files or markers.

## Confirmation Flow

### Destructive Commands (11)

These commands return `NeedsConfirmation` on first execution:

| Command | Confirmation Prompt |
|---------|-------------------|
| MergePr | "Are you sure you want to {method} merge PR #{n} in {repo}? Say yes to confirm." |
| ClosePr | "Are you sure you want to close PR #{n} in {repo}? Say yes to confirm." |
| RevertPr | "Are you sure you want to revert PR #{n} in {repo}? This will create a new PR that undoes the changes. Say yes to confirm." |
| AddCollaborator | "Are you sure you want to add {user} as a collaborator to {repo} with {perm} permission? Say yes to confirm." |
| RemoveCollaborator | "Are you sure you want to remove {user} as a collaborator from {repo}? Say yes to confirm." |
| AddOrgMember | "Are you sure you want to add {user} to the {org} organization as {role}? Say yes to confirm." |
| RemoveOrgMember | "Are you sure you want to remove {user} from the {org} organization? Say yes to confirm." |
| ConvertToOutsideCollaborator | "Are you sure you want to convert {user} to an outside collaborator in {org}? Say yes to confirm." |
| SetBranchProtection | "Are you sure you want to set branch protection on {branch} in {repo}? Say yes to confirm." |
| DeleteBranch | "Are you sure you want to delete branch {branch} in {repo}? This cannot be undone. Say yes to confirm." |
| DeleteRelease | "Are you sure you want to delete release {id} in {repo}? Say yes to confirm." |
| CancelWorkflow | "Are you sure you want to cancel workflow run {id} in {repo}? Say yes to confirm." |

### Flow

```
1. User: "merge PR 23 in owner/repo"
2. NEXUS: "On it sir." (ack)
3. NEXUS: "Are you sure you want to merge PR #23 in owner/repo? Say yes to confirm."
4. User: "yes"
5. NEXUS: "PR #23 has been merge merged into owner/repo, sir."
```

## Token Security

### How Tokens Are Handled

```
┌──────────────┐    GET /oauth/github-token     ┌──────────────┐
│  Rust client │ ─────────────────────────────► │    Worker    │
│  (github_cmd)│                                │  (D1-backed) │
│              │ ◄───────────────────────────── │              │
│  In-memory   │    { token: "gho_..." }        │  oauth_tokens│
│  cache only  │                                │  table       │
└──────────────┘                                └──────────────┘
```

### Security Properties

| Property | Implementation |
|----------|---------------|
| Never written to disk | `static GITHUB_TOKEN: Lazy<Arc<RwLock<Option<CachedToken>>>>` — process memory only |
| Never logged | `tracing::info!("github_cmd: fetching token from worker: {}", url)` — logs URL only, never the token |
| Never exposed to frontend | Frontend receives only `GitHubResult` events (Text, Conflict, Error) — never the token |
| Auto-refreshed | Cache checks `is_valid()` — refreshes 5 minutes before expiry |
| Clearable | `clear_github_token()` clears cache on disconnect |
| Worker handles refresh | Worker's `getValidGithubToken()` handles refresh-token rotation |

### Future Security Improvements

The current design returns the raw token from the Worker to the Rust client.
This is a deliberate tradeoff for Phase 2A. Future options:

1. **Worker-proxied operations** — Worker makes the GitHub API call, token
   never leaves the Worker.
2. **Short-lived scoped capability tokens** — Worker issues a narrowly-scoped,
   short-lived token to the desktop client.
3. **GitHub App installation tokens** — use GitHub App user-to-server tokens
   with fine-grained repository selection and short expiry.

## Error Handling

### Error Types

```rust
GitHubResult::Error {
    message: String,
    status: Option<u16>,     // HTTP status code
    is_auth_error: bool,     // true for 401/403
}
```

### Error Messages

| Status | is_auth_error | Message |
|--------|--------------|---------|
| 401 | true | "Your GitHub token has expired or lacks permissions. Please reconnect GitHub in the NEXUS setup to {context}." |
| 403 | true | "Your GitHub token has expired or lacks permissions. Please reconnect GitHub in the NEXUS setup to {context}." |
| 404 | false | "Error trying to {context}: {error}" |
| 405 | false | (For merge) Returns MergeConflict instead of Error |
| Other | false | "Error trying to {context}: {error}" |

### Error Mapping

```rust
fn map_octocrab_error(e: octocrab::Error, context: &str) -> GitHubResult {
    match &e {
        octocrab::Error::GitHub { source, .. } => {
            let code = source.status_code.as_u16();
            let is_auth = code == 401 || code == 403;
            (Some(code), is_auth)
        }
        octocrab::Error::Hyper { .. } => (None, false),
        _ => (None, false),
    }
    // ... build user-friendly message
}
```

## Test Results

### Test Count by Phase

| Phase | New Tests | Cumulative | Gate 1 | Gate 2 |
|-------|-----------|------------|--------|--------|
| 2A-1 (Foundation) | 17 | 173 | PASS | PASS |
| 2A-2 (Conflict + Confirm) | 8 | 181 | PASS | PASS |
| 2A-3 (Full PR ops) | 10 | 191 | PASS | PASS |
| 2A-4 (Collaborator + Org) | 13 | 204 | PASS | PASS |
| 2A-5 (Branch + Release + Workflow) | 13 | 217 | PASS | PASS |
| 2A-6 (Intent parser) | 26 | 243 | PASS | PASS |
| 2A-7 (Final integration) | 0 (integration) | 243 | PASS | PASS |

### Test Categories

**github_cmd.rs (87 tests)**:
- `split_repo()` — 3 tests
- Enum serde round-trips — 15 tests
- `is_destructive()` classification — 28 tests (one per command)
- `confirmation_prompt()` — 15 tests
- `required_scopes()` — 1 test
- `GitHubResult` serialization — 4 tests
- `ConflictFile` / `ConflictBlock` serialization — 2 tests
- `extract_conflict_blocks_from_patch()` — 3 tests
- `CachedToken::is_valid()` — 4 tests
- `default_pr_state()` — 1 test
- `MergeMethod` / `CollaboratorPermission` / `OrgRole` serde — 6 tests

**intent_parser.rs (27 tests)**:
- PR command parsing — 10 tests
- Collaborator/org parsing — 8 tests
- Branch/release/workflow parsing — 7 tests
- Routing — 2 tests

### Additional Tests (existing, still passing)

- Worker tests: 28 vitest tests (quota, cache, search, dedup)
- Offline command tests: 10 tests (close_app, whatsapp_chat)

## Files Changed

| File | Lines | Type | Description |
|------|-------|------|-------------|
| `src-tauri/src/github_cmd.rs` | ~2820 | NEW | Core module: 28 commands, trait, token mgmt, conflict detection |
| `frontend/src/sidebar/GitHubConflictPanel.tsx` | 158 | NEW | React component for conflict UI |
| `src-tauri/src/intent_parser.rs` | ~870 | Modified | `parse_github_command()` + 26 patterns + 27 tests |
| `src-tauri/src/orchestrator.rs` | ~250 | Modified | `Subsystem::GitHub`, 3 new events, dispatch arm, 2 Tauri commands |
| `frontend/src/net/orchestrator.ts` | ~110 | Modified | New event handlers + TypeScript types |
| `frontend/src/sidebar/sidebar.css` | 181 | Modified | Conflict panel styles |
| `src-tauri/Cargo.toml` | 8 | Modified | Added `octocrab`, `async-trait` |
| `src-tauri/src/lib.rs` | 4 | Modified | Registered module + commands |

## Phase Breakdown

### Phase 2A-1: Foundation
- Added `octocrab = "0.42"` to Cargo.toml
- Created `github_cmd.rs` with `GitHubCommand` enum (5 PR commands)
- Implemented token fetch from Worker with in-memory caching
- Added `Subsystem::GitHub` to orchestrator
- 17 new tests

### Phase 2A-2: Conflict Detection + Confirmation
- Implemented `pre_check()` for MergePr (mergeable_state check)
- Implemented `fetch_conflict_details()` + `extract_conflict_blocks_from_patch()`
- Added centralized confirmation flow (`is_destructive()` + `confirmation_prompt()`)
- Added 3 new `OrchestratorEvent` variants (Confirm, ConflictReport, GitHubResult)
- Added 2 new Tauri commands (`orchestrator_github_execute`, `orchestrator_github_clear_token`)
- 8 new tests

### Phase 2A-3: Full PR Operations
- Added `CreatePr`, `UpdateBranch`, `RevertPr`, `ListPrFiles`, `CommentPr`
- 10 new tests

### Phase 2A-4: Collaborator + Organization
- Added 8 collaborator/org commands
- 13 new tests

### Phase 2A-5: Branch + Release + Workflow
- Added 10 branch/release/workflow commands
- 13 new tests

### Phase 2A-6: Intent Parser Integration
- Added `ParsedIntent::GitHubCommand` variant
- Implemented `parse_github_command()` with 26 regex patterns
- Updated `route_intent()` to route to `Subsystem::GitHub`
- Updated `Subsystem::GitHub` dispatch arm to use parsed command
- Moved GitHub parser before open/close app parsers
- 27 new tests

### Phase 2A-7: Final Integration
- Verified Worker `/oauth/github-token` endpoint
- Created `GitHubConflictPanel.tsx` frontend component
- Added conflict panel CSS styles
- Extended `orchestrator.ts` with new event handlers + TypeScript types
- Ran all test gates (Rust + Worker + offline + frontend build)
