# 31 — GitHub Sub-Command System (Phase 2A)

> **Date**: 2026-09-04
> **Type**: Architecture change — new subsystem
> **Impact**: High — adds typed GitHub operations beneath the central orchestrator
> **Phases**: 2A-1 through 2A-7 (7 phases, 14 test gates, all passing)

---

## Summary

Implemented a structured GitHub sub-command system in Rust that replaces the
Worker's regex-based `handleGitHubWrite()` with a typed `GitHubCommand` enum
of 28 variants, each implementing the `GitHubExecutable` trait. The system
fetches the user's GitHub OAuth token from the Worker, executes operations
directly via `octocrab`, and emits typed results (text, conflict reports,
confirmation prompts, errors) through the orchestrator's event channel.

The intent parser was extended with 26 natural-language patterns that parse
voice transcripts like "squash merge PR 23 in owner/repo" into structured
`GitHubCommand` values, routed to `Subsystem::GitHub`.

A frontend conflict UI panel displays merge conflicts with copy-paste options
for HEAD content, branch content, and full conflict blocks.

## What Changed

### New Files

1. **`src-tauri/src/github_cmd.rs`** (~2820 lines)
   - `GitHubCommand` enum — 28 typed variants across 4 categories
   - `GitHubExecutable` trait — `is_destructive()`, `confirmation_prompt()`,
     `required_scopes()`, `pre_check()`, `execute()`
   - `GitHubResult` enum — Text, NeedsConfirmation, MergeConflict, Error
   - `ConflictFile` / `ConflictBlock` structs for conflict reporting
   - `MergeMethod`, `CollaboratorPermission`, `OrgRole` enums
   - Token management: `get_github_token()`, `fetch_token_from_worker()`,
     `clear_github_token()` with in-memory caching and auto-refresh
   - `build_octocrab_client()` — constructs authenticated octocrab instance
   - `execute_command()` — main entry point (token → pre-check → confirm →
     execute → result)
   - 28 execution functions (one per command variant)
   - `fetch_conflict_details()` — retrieves PR files and extracts conflict
     blocks from diff patches
   - `extract_conflict_blocks_from_patch()` — parses `<<<<<<< ======= >>>>>>>`
     markers from git patches
   - `map_octocrab_error()` — maps octocrab errors to typed GitHubResult
     with auth-error detection (401/403)
   - 87 unit tests (serialization, destructive classification, confirmation
     prompts, conflict block extraction, token validity, serde round-trips)

2. **`frontend/src/sidebar/GitHubConflictPanel.tsx`** (158 lines)
   - React component displaying merge conflict details
   - Copy-paste buttons for HEAD content, branch content, full conflict blocks
   - File-level conflict count badges
   - Step-by-step fix instructions
   - Retry merge button

### Modified Files

3. **`src-tauri/Cargo.toml`**
   - Added `octocrab = "0.42"` — typed GitHub API client
   - Added `async-trait = "0.1"` — for the `GitHubExecutable` trait

4. **`src-tauri/src/lib.rs`**
   - Registered `github_cmd` module
   - Added 2 Tauri commands: `orchestrator_github_execute`,
     `orchestrator_github_clear_token`

5. **`src-tauri/src/orchestrator.rs`**
   - Added `Subsystem::GitHub` variant to the subsystem enum
   - Added 3 new `OrchestratorEvent` variants: `Confirm`, `ConflictReport`,
     `GitHubResult`
   - Added full `Subsystem::GitHub` dispatch arm in `process_transcript()`
     that extracts the `GitHubCommand` from the parsed intent, calls
     `github_cmd::execute_command()`, and emits the appropriate events
   - Made `route_intent()` `pub(crate)` for testing
   - Added `orchestrator_github_execute` Tauri command — allows frontend to
     execute GitHub commands directly with confirmation support
   - Added `orchestrator_github_clear_token` Tauri command — clears cached
     token after GitHub disconnect
   - Updated `is_long_running()` to include `Subsystem::GitHub`

6. **`src-tauri/src/intent_parser.rs`**
   - Added `ParsedIntent::GitHubCommand { command: GitHubCommand }` variant
   - Added `parse_github_command()` function — 26 regex patterns for natural
     language GitHub commands
   - Moved GitHub parser BEFORE open/close app parsers to prevent "close PR"
     and "show PR" from matching app commands
   - 27 new tests for GitHub command parsing and routing

7. **`frontend/src/net/orchestrator.ts`**
   - Extended `OrchestratorEvent` interface with `confirm`, `conflict_report`,
     `github_result` event types
   - Added `ConflictFile`, `ConflictBlock`, `GitHubResultPayload` TypeScript
     interfaces
   - Added `confirm` event handler — speaks confirmation prompt
   - Added `conflict_report` event handler — speaks conflict summary, logs
     conflict details for UI display
   - Added `github_result` event handler — logs raw structured result

8. **`frontend/src/sidebar/sidebar.css`**
   - Added 181 lines of styles for `.github-conflict-panel` and all child
     elements (conflict files, blocks, copy buttons, instructions, retry)

## Why

Before this change, GitHub operations were handled by the Worker's
`handleGitHubWrite()` function — a growing collection of regex patterns
over the raw transcript. This approach had fundamental problems:

1. **No typed command model** — every operation was a regex match against
   free text, with no validation, no type safety, and no exhaustive
   matching.
2. **No centralized confirmation** — destructive operations (merge, close,
   delete) had ad-hoc confirmation logic scattered across handlers.
3. **No conflict detection** — merge attempts returned generic failures
   without explaining what conflicted or how to fix it.
4. **No command discovery** — the user couldn't ask "what GitHub commands
   are available" and get a structured answer.
5. **Token exposure** — the Worker returned raw tokens to the frontend for
   GitHub API calls, increasing the attack surface.
6. **Unmaintainable regex soup** — each new GitHub operation added another
   regex block to the Worker, making the code increasingly fragile.
7. **No dry-run/preflight** — operations executed without checking
   preconditions (e.g., mergeable state, permissions, branch protection).

## How It Works

### Command Flow

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
      4. Extract GitHubCommand from parsed intent
      5. github_cmd::execute_command(worker_url, user_id, &cmd, confirmed=false)
         a. build_octocrab_client()
            → get_github_token()
            → check cache (is_valid?)
            → if stale: fetch_token_from_worker()
            → construct octocrab::Octocrab with personal_token
         b. command.pre_check(&client)
            → for MergePr: fetch PR, check mergeable_state
            → if Dirty: return MergeConflict result
         c. command.is_destructive() && !confirmed
            → return NeedsConfirmation { prompt, command }
         d. command.execute(&client)
            → octocrab API call
            → return Text / Error / MergeConflict
      6. Match result type:
         - NeedsConfirmation → emit Confirm event
         - MergeConflict → emit ConflictReport event
         - Text → emit Result event
         - Error → emit Error event
      7. Emit GitHubResult (raw structured data)
      8. Emit Done
      9. Clear active request
```

### Token Management

```
get_github_token(worker_url, user_id)
  → Check GITHUB_TOKEN static (Arc<RwLock<Option<CachedToken>>>)
  → If cached and is_valid(): return cached token
  → If stale or missing:
      → fetch_token_from_worker()
      → GET {worker_url}/oauth/github-token?user_id={user_id}
      → Worker calls getValidGithubToken() (handles refresh)
      → Returns { token: "gho_..." }
      → Cache in memory (expires_at = now + 3600s)
      → Return token
```

The token is **never**:
- Written to disk
- Logged (tracing only logs the fetch URL, never the token)
- Exposed to the frontend (the frontend only sees structured results)

### Conflict Detection Flow

```
MergePr pre_check:
  1. Fetch PR via octocrab::pulls().get()
  2. Check pr.mergeable (bool) and pr.mergeable_state (MergeableState enum)
  3. If !mergeable || mergeable_state == Dirty:
     → fetch_conflict_details()
       → GET /repos/{owner}/{repo}/pulls/{pr_number}/files
       → For each file: check patch for <<<<<<< and >>>>>>> markers
       → extract_conflict_blocks_from_patch()
         → Parse +<<<<<<< HEAD / +======= / +>>>>>>> markers
         → Build ConflictBlock { start_line, head_content, branch_content }
     → Return MergeConflict { pr_number, repo, conflict_files, message }
  4. If mergeable && mergeable_state == Clean:
     → Return Ok(()) — proceed to confirmation + merge
```

### Confirmation Flow

```
Destructive command (e.g., MergePr, ClosePr, DeleteBranch):
  1. execute_command() called with confirmed=false
  2. pre_check() passes (no conflicts)
  3. is_destructive() returns true
  4. confirmation_prompt() returns Some("Are you sure...")
  5. Returns NeedsConfirmation { prompt, command }
  6. Orchestrator emits Confirm event
  7. Frontend speaks prompt: "Are you sure you want to squash merge PR #23?"
  8. User says "yes"
  9. Frontend calls orchestrator_github_execute with confirmed=true
  10. execute_command() called with confirmed=true
  11. Skips confirmation check, executes directly
  12. Returns Text result ("PR #23 has been squash merged")
```

## The 28 GitHub Commands

### PR Operations (10 commands)

| Command | Destructive | Description |
|---------|-------------|-------------|
| `MergePr` | Yes | Merge a PR with merge/squash/rebase method |
| `ApprovePr` | No | Submit an APPROVE review on a PR |
| `ClosePr` | Yes | Close a PR without merging |
| `ListPrs` | No | List open/closed/all PRs in a repo |
| `GetPr` | No | Get details of a specific PR |
| `CreatePr` | No | Create a new PR from head to base branch |
| `UpdateBranch` | No | Update PR branch with latest base changes |
| `RevertPr` | Yes | Create a revert PR for a merged PR |
| `ListPrFiles` | No | List files changed in a PR |
| `CommentPr` | No | Add a comment to a PR |

### Collaborator + Organization (8 commands)

| Command | Destructive | Description |
|---------|-------------|-------------|
| `AddCollaborator` | Yes | Add user as repo collaborator with permission |
| `RemoveCollaborator` | Yes | Remove collaborator from repo |
| `ListCollaborators` | No | List repo collaborators with permissions |
| `AddOrgMember` | Yes | Add user to org as member/admin |
| `RemoveOrgMember` | Yes | Remove user from org |
| `ListOrgMembers` | No | List org members |
| `ConvertToOutsideCollaborator` | Yes | Convert org member to outside collaborator |
| `ListOutsideCollaborators` | No | List outside collaborators of org |

### Branch + Release + Workflow (10 commands)

| Command | Destructive | Description |
|---------|-------------|-------------|
| `SetBranchProtection` | Yes | Set branch protection rules |
| `DeleteBranch` | Yes | Delete a branch ref |
| `ListBranches` | No | List branches with protection status |
| `CreateRelease` | No | Create a new release |
| `ListReleases` | No | List releases |
| `DeleteRelease` | Yes | Delete a release |
| `ListWorkflows` | No | List GitHub Actions workflows |
| `ListWorkflowRuns` | No | List workflow runs (optionally filtered) |
| `RerunWorkflow` | No | Rerun a failed workflow run |
| `CancelWorkflow` | Yes | Cancel a running workflow |

## Natural Language Patterns (26 regex patterns)

The `parse_github_command()` function in `intent_parser.rs` recognizes these
voice/text patterns:

### PR Operations
- `merge PR <num> in <repo>` → MergePr (merge)
- `squash merge PR <num> in <repo>` → MergePr (squash)
- `rebase merge PR <num> in <repo>` → MergePr (rebase)
- `approve PR <num> in <repo>` → ApprovePr
- `close PR <num> in <repo>` → ClosePr
- `get PR <num> in <repo>` → GetPr
- `show PR <num> in <repo>` → GetPr
- `tell me about PR <num> in <repo>` → GetPr
- `list PRs in <repo>` → ListPrs (open)
- `list open PRs in <repo>` → ListPrs (open)
- `list closed PRs in <repo>` → ListPrs (closed)
- `revert PR <num> in <repo>` → RevertPr
- `list PR files for PR <num> in <repo>` → ListPrFiles
- `update branch for PR <num> in <repo>` → UpdateBranch

### Collaborator + Org
- `add <user> as collaborator to <repo>` → AddCollaborator (push)
- `add <user> as admin collaborator to <repo>` → AddCollaborator (admin)
- `add <user> as push collaborator to <repo>` → AddCollaborator (push)
- `add <user> as pull collaborator to <repo>` → AddCollaborator (pull)
- `remove <user> as collaborator from <repo>` → RemoveCollaborator
- `list collaborators in <repo>` → ListCollaborators
- `add <user> to org <org>` → AddOrgMember (member)
- `add <user> as admin to org <org>` → AddOrgMember (admin)
- `remove <user> from org <org>` → RemoveOrgMember
- `list members of org <org>` → ListOrgMembers

### Branch + Release + Workflow
- `list branches in <repo>` → ListBranches
- `delete branch <name> in <repo>` → DeleteBranch
- `list releases in <repo>` → ListReleases
- `list workflows in <repo>` → ListWorkflows
- `list workflow runs in <repo>` → ListWorkflowRuns
- `rerun workflow <id> in <repo>` → RerunWorkflow
- `cancel workflow <id> in <repo>` → CancelWorkflow

## Test Results

| Gate | Tests | Result |
|------|-------|--------|
| Phase 2A-1 Gate 1 (cargo check + cargo test --lib) | 173 passed | PASS |
| Phase 2A-1 Gate 2 (mock-wake + frontend build) | Clean | PASS |
| Phase 2A-2 Gate 1 | 181 passed | PASS |
| Phase 2A-2 Gate 2 | Clean | PASS |
| Phase 2A-3 Gate 1 | 191 passed | PASS |
| Phase 2A-3 Gate 2 | Clean | PASS |
| Phase 2A-4 Gate 1 | 204 passed | PASS |
| Phase 2A-4 Gate 2 | Clean | PASS |
| Phase 2A-5 Gate 1 | 217 passed | PASS |
| Phase 2A-5 Gate 2 | Clean | PASS |
| Phase 2A-6 Gate 1 | 243 passed | PASS |
| Phase 2A-6 Gate 2 | Clean | PASS |
| Phase 2A-7 Gate 1 (cargo + worker tests) | 243 + 28 passed | PASS |
| Phase 2A-7 Gate 2 (full build + frontend) | Clean | PASS |

**Total new tests added: 87 in github_cmd.rs + 27 in intent_parser.rs = 114 new tests**

## Security Considerations

### Token Handling
- GitHub OAuth token is fetched from the Worker's `/oauth/github-token`
  endpoint, which calls `getValidGithubToken()` (handles token refresh)
- Token is cached in process memory only (`Arc<RwLock<Option<CachedToken>>>`)
- Token is **never** written to disk
- Token is **never** logged (tracing logs the fetch URL only)
- Token is **never** exposed to the frontend (frontend sees only structured
  `GitHubResult` events)
- Token cache auto-refreshes 5 minutes before expiry
- `clear_github_token()` clears the cache on disconnect

### Destructive Operation Safety
- 11 commands are classified as destructive (see table above)
- Destructive commands return `NeedsConfirmation` before executing
- Confirmation prompt includes the exact command, repo, and parameters
- The frontend must re-submit the command with `confirmed=true` to execute
- This prevents accidental destructive actions from ambiguous voice transcripts

### Error Handling
- 401/403 errors are detected and flagged as `is_auth_error: true`
- Auth errors produce a user-friendly message: "Your GitHub token has expired
  or lacks permissions. Please reconnect GitHub in the NEXUS setup."
- 404 errors produce "not found" messages
- Rate limit errors are passed through with status codes
- Network errors produce generic error messages

### Future Security Considerations
The current design returns the raw GitHub token from the Worker to the Rust
client. This is a deliberate tradeoff for Phase 2A — it enables direct
GitHub API access from Rust without proxying every call through the Worker.
Future improvements to consider:

1. **Worker-proxied GitHub operations** — the Worker makes the GitHub API
   call, the token never reaches the desktop client.
2. **Short-lived scoped capability tokens** — the Worker issues a
   narrowly-scoped, short-lived token to the desktop client.
3. **GitHub App installation/user tokens** — instead of long-lived OAuth
   tokens, use GitHub App user-to-server tokens with fine-grained
   repository selection and short expiry.

These are documented as future work, not implemented in Phase 2A.

## Files Changed Summary

| File | Lines Added | Type |
|------|-------------|------|
| `src-tauri/src/github_cmd.rs` | ~2820 | NEW |
| `frontend/src/sidebar/GitHubConflictPanel.tsx` | 158 | NEW |
| `src-tauri/src/intent_parser.rs` | ~870 | Modified |
| `src-tauri/src/orchestrator.rs` | ~250 | Modified |
| `frontend/src/net/orchestrator.ts` | ~110 | Modified |
| `frontend/src/sidebar/sidebar.css` | 181 | Modified |
| `src-tauri/Cargo.toml` | 8 | Modified |
| `src-tauri/src/lib.rs` | 4 | Modified |
