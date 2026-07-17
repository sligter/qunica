# Workspace Git Review Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use subagent-driven-development when workers are available, otherwise execute the tasks inline. Do not use Trellis. Implement each task before adding its regression tests; this plan intentionally does not use TDD.

**Goal:** Upgrade the group workspace right-side Git tab into a LiveAgent-capability-aligned Git review panel with diffs, history, branches, remotes, discard/stash/init, while keeping backend-owned Git execution and ag-swarmer UI patterns.

**Architecture:** Extend the existing `/api/v2/groups/:group_id/workspace-git/*` surface and `backend-rs/.../git` modules. Frontend keeps the current `Files | Git` right panel, but splits `WorkspaceGitTab` into toolbar/changes/diff/history/branch components driven by React Query hooks.

**Tech Stack:** Rust, Axum, Tokio, git CLI, React 19, TypeScript, TanStack Query, Vitest, existing shadcn dialog/sheet/confirm primitives.

## Global Constraints

- Capability alignment with LiveAgent, not a visual clone.
- Extend existing `workspace-git` APIs; do not invent a second Git client path.
- Backend owns all Git process execution under the group workspace root.
- Path inputs must stay inside the workspace root via existing validators.
- Keep AI commit-message generation.
- No commit-graph swimlanes, no full context-menu framework, no Tauri direct Git invokes.
- Confirm only destructive actions: discard, discard all, delete branch, stash pop when dirty, init repository.
- Existing untracked `.superpowers/` and `.tmp-liveagent-git/` content must remain untouched.
- Implement first, then add focused regression tests. No TDD.

## File Map

### Backend
- Modify: `backend-rs/crates/backend/src/git/status.rs`
- Modify: `backend-rs/crates/backend/src/git/ops.rs`
- Modify: `backend-rs/crates/backend/src/git/mod.rs`
- Create: `backend-rs/crates/backend/src/git/diff.rs`
- Create: `backend-rs/crates/backend/src/git/log.rs`
- Create: `backend-rs/crates/backend/src/git/branches.rs`
- Modify: `backend-rs/crates/backend/src/api/groups.rs`
- Modify: `backend-rs/crates/backend/src/api/mod.rs`
- Modify: `backend-rs/crates/backend/tests/groups.rs`

### Frontend
- Modify: `frontend/src/types/api.ts`
- Create: `frontend/src/hooks/useWorkspaceGit.ts`
- Modify: `frontend/src/hooks/useGroupFiles.ts` (re-export compatibility shims if needed)
- Modify: `frontend/src/components/chat/WorkspaceGitTab.tsx`
- Create: `frontend/src/components/chat/WorkspaceGitToolbar.tsx`
- Create: `frontend/src/components/chat/WorkspaceGitChangesView.tsx`
- Create: `frontend/src/components/chat/WorkspaceGitDiffPanel.tsx`
- Create: `frontend/src/components/chat/WorkspaceGitHistoryView.tsx`
- Create: `frontend/src/components/chat/WorkspaceGitBranchSheet.tsx`
- Create: `frontend/src/components/chat/WorkspaceGitTab.test.tsx`
- Create: `frontend/src/components/chat/WorkspaceGitChangesView.test.tsx`
- Create: `frontend/src/components/chat/WorkspaceGitHistoryView.test.tsx`

---

### Task 1: Enrich Git Status Model

**Files:**
- Modify: `backend-rs/crates/backend/src/git/status.rs`
- Modify: `backend-rs/crates/backend/src/git/ops.rs`
- Modify: `backend-rs/crates/backend/src/git/mod.rs`
- Modify: `backend-rs/crates/backend/tests/groups.rs`

**Interfaces:**
- Produces richer `WorkspaceGitStatus` and `WorkspaceGitFileStatus` JSON.
- Keeps `status(root) -> WorkspaceGitStatus`.
- Existing stage/unstage/commit callers continue to work with additive fields.

- [ ] **Step 1: Expand status structs**

Update `status.rs` to:

```rust
#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct WorkspaceGitDirtyCounts {
    pub staged: usize,
    pub unstaged: usize,
    pub untracked: usize,
    pub conflicted: usize,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct WorkspaceGitFileStatus {
    pub path: String,
    pub old_path: Option<String>,
    pub status: String,
    pub staged: bool,
    pub unstaged: bool,
    pub untracked: bool,
    pub conflicted: bool,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct WorkspaceGitStatus {
    pub available: bool,
    pub status: String, // ready | not_repo | error
    pub branch: Option<String>,
    pub upstream: Option<String>,
    pub remote_name: Option<String>,
    pub remote_url: Option<String>,
    pub ahead: Option<i64>,
    pub behind: Option<i64>,
    pub stash_count: usize,
    pub clean: bool,
    pub dirty_counts: WorkspaceGitDirtyCounts,
    pub files: Vec<WorkspaceGitFileStatus>,
    pub message: Option<String>,
}
```

- [ ] **Step 2: Parse richer porcelain + remote/stash metadata**

In `parse_status` and/or `status()`:

- Keep porcelain v2 parsing.
- Capture rename old path for `2` records into `old_path`.
- Mark `untracked` for `?` records and `conflicted` for `u` records.
- Parse `# branch.upstream`.
- After a successful status parse, gather:
  - `remote_name` / `remote_url` via `git remote` + `git remote get-url <name>` (default `origin` when present)
  - `stash_count` via `git rev-list --walk-reflogs --count refs/stash` (0 if missing)
- Set `status` field:
  - `ready` when repository available
  - `not_repo` for non-repo
  - `error` for other failures

- [ ] **Step 3: Compute dirty counts**

```rust
fn dirty_counts(files: &[WorkspaceGitFileStatus]) -> WorkspaceGitDirtyCounts {
    let mut counts = WorkspaceGitDirtyCounts {
        staged: 0,
        unstaged: 0,
        untracked: 0,
        conflicted: 0,
    };
    for file in files {
        if file.conflicted {
            counts.conflicted += 1;
        }
        if file.untracked {
            counts.untracked += 1;
        } else {
            if file.staged {
                counts.staged += 1;
            }
            if file.unstaged {
                counts.unstaged += 1;
            }
        }
    }
    counts
}
```

- [ ] **Step 4: Add unit/API regression coverage and commit**

Extend unit tests in `status.rs` for rename `old_path`, conflicted flags, and dirty counts. Extend `groups.rs` status assertions to accept the new fields without breaking old ones.

Run:

```powershell
cargo test -p ag-swarmer-backend --lib git::status -- --nocapture
cargo test -p ag-swarmer-backend --test groups workspace_git_status_stage_unstage_and_commit -- --nocapture
```

Commit:

```powershell
git add backend-rs/crates/backend/src/git backend-rs/crates/backend/tests/groups.rs
git commit -m "feat(git): enrich workspace git status metadata"
```

---

### Task 2: Diff APIs

**Files:**
- Create: `backend-rs/crates/backend/src/git/diff.rs`
- Modify: `backend-rs/crates/backend/src/git/mod.rs`
- Modify: `backend-rs/crates/backend/src/git/ops.rs` (keep `staged_diff` or route it through new helper)
- Modify: `backend-rs/crates/backend/src/api/groups.rs`
- Modify: `backend-rs/crates/backend/src/api/mod.rs`
- Modify: `backend-rs/crates/backend/tests/groups.rs`

**Interfaces:**
- Produces:

```rust
pub struct WorkspaceGitDiff {
    pub mode: String, // worktree | staged | branch | commit
    pub base_ref: Option<String>,
    pub head_ref: Option<String>,
    pub path: Option<String>,
    pub patch: String,
    pub stat: String,
    pub truncated: bool,
    pub binary_files: Vec<String>,
}

pub async fn diff(
    root: &Path,
    mode: DiffMode,
    path: Option<&str>,
) -> Result<WorkspaceGitDiff, GitOperationError>;
```

- API: `GET /api/v2/groups/:group_id/workspace-git/diff?mode=&path=`

- [ ] **Step 1: Implement mode-specific git diff commands**

```rust
pub enum DiffMode {
    Worktree,
    Staged,
    Branch,
}

// worktree: git diff --no-ext-diff --find-renames [-- path]
// staged:   git diff --cached --no-ext-diff --find-renames [-- path]
// branch:   git diff --no-ext-diff --find-renames <upstream_or_merge_base>...HEAD [-- path]
```

Also collect `--stat` separately or parse from combined output. Detect binary files from diff headers containing `Binary files` / `GIT binary patch`.

- [ ] **Step 2: Truncate large diffs**

If patch exceeds a bounded size (for example 200_000 chars), truncate and set `truncated: true`. Keep enough content for UI rendering.

- [ ] **Step 3: Wire API handler**

```rust
pub async fn get_group_workspace_git_diff(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(group_id): Path<String>,
    Query(query): Query<GroupWorkspaceGitDiffQuery>,
) -> Result<Json<WorkspaceGitDiff>, ApiError>
```

Validate optional path with `validate_git_paths` when present. Register route in `api/mod.rs`.

- [ ] **Step 4: Tests and commit**

Cover:

- worktree path diff contains changed hunk
- staged diff empty before stage, non-empty after stage
- unsafe path rejected
- truncation flag when forced with a large fixture if practical

Run:

```powershell
cargo test -p ag-swarmer-backend --test groups workspace_git_diff -- --nocapture
```

Commit:

```powershell
git add backend-rs/crates/backend/src/git backend-rs/crates/backend/src/api backend-rs/crates/backend/tests/groups.rs
git commit -m "feat(git): add workspace git diff endpoint"
```

---

### Task 3: History and Commit Details

**Files:**
- Create: `backend-rs/crates/backend/src/git/log.rs`
- Modify: `backend-rs/crates/backend/src/git/mod.rs`
- Modify: `backend-rs/crates/backend/src/api/groups.rs`
- Modify: `backend-rs/crates/backend/src/api/mod.rs`
- Modify: `backend-rs/crates/backend/tests/groups.rs`

**Interfaces:**
- Produces:

```rust
pub struct WorkspaceGitCommitSummary {
    pub sha: String,
    pub short_sha: String,
    pub subject: String,
    pub author_name: String,
    pub author_email: String,
    pub author_date: String,
    pub local_only: bool,
}

pub struct WorkspaceGitLog {
    pub commits: Vec<WorkspaceGitCommitSummary>,
    pub has_more: bool,
}

pub struct WorkspaceGitCommitDetails {
    pub sha: String,
    pub short_sha: String,
    pub subject: String,
    pub body: String,
    pub author_name: String,
    pub author_email: String,
    pub author_date: String,
    pub files: Vec<WorkspaceGitCommitFile>,
    pub insertions: usize,
    pub deletions: usize,
    pub stat: String,
}

pub async fn log(root: &Path, limit: usize, skip: usize) -> Result<WorkspaceGitLog, GitOperationError>;
pub async fn commit_details(root: &Path, sha: &str) -> Result<WorkspaceGitCommitDetails, GitOperationError>;
pub async fn commit_diff(root: &Path, sha: &str, path: Option<&str>) -> Result<WorkspaceGitDiff, GitOperationError>;
pub async fn create_branch_from_commit(root: &Path, sha: &str, branch: &str) -> Result<(), GitOperationError>;
```

- APIs:
  - `GET /log?limit=&skip=`
  - `GET /commits/:sha`
  - `GET /commits/:sha/diff?path=`
  - `POST /commits/:sha/create-branch` body `{ "name": "..." }`

- [ ] **Step 1: Implement log parsing**

Use a stable pretty format, for example:

```text
git log --skip=N -n LIMIT --format=%H%x00%h%x00%s%x00%an%x00%ae%x00%aI%x00
```

Mark `local_only` when commit is not reachable from upstream (`git merge-base --is-ancestor` or set membership from `git rev-list upstream..HEAD`) when upstream exists.

- [ ] **Step 2: Implement commit details and commit diff**

Details via `git show --stat --format=... --name-status`.
Diff via `git show --no-ext-diff --format= --patch <sha> [-- path]`.

- [ ] **Step 3: Create branch from commit**

```rust
// git branch <name> <sha>
// or git switch -c <name> <sha>
```

Validate branch name rejects spaces and `..` / leading `-`.

- [ ] **Step 4: Tests and commit**

Cover:

- log returns newest commit after a commit
- pagination `has_more`
- commit details include subject/files
- create-branch makes the branch listable later

Run:

```powershell
cargo test -p ag-swarmer-backend --test groups workspace_git_log -- --nocapture
cargo test -p ag-swarmer-backend --test groups workspace_git_commit -- --nocapture
```

Commit:

```powershell
git add backend-rs/crates/backend/src/git backend-rs/crates/backend/src/api backend-rs/crates/backend/tests/groups.rs
git commit -m "feat(git): add workspace git history endpoints"
```

---

### Task 4: Branch, Remote, Init, Discard, Ignore, Stash Ops

**Files:**
- Create: `backend-rs/crates/backend/src/git/branches.rs`
- Modify: `backend-rs/crates/backend/src/git/ops.rs`
- Modify: `backend-rs/crates/backend/src/git/mod.rs`
- Modify: `backend-rs/crates/backend/src/api/groups.rs`
- Modify: `backend-rs/crates/backend/src/api/mod.rs`
- Modify: `backend-rs/crates/backend/tests/groups.rs`

**Interfaces:**
- Produces branch listing/mutation helpers and new ops:

```rust
pub struct WorkspaceGitBranch {
    pub name: String,
    pub full_name: String,
    pub kind: String, // local | remote
    pub current: bool,
    pub upstream: Option<String>,
    pub ahead: i64,
    pub behind: i64,
}

pub struct WorkspaceGitBranches {
    pub branches: Vec<WorkspaceGitBranch>,
}

pub async fn branches(root: &Path) -> Result<WorkspaceGitBranches, GitOperationError>;
pub async fn create_branch(root: &Path, name: &str, start_point: Option<&str>) -> Result<(), GitOperationError>;
pub async fn switch_branch(root: &Path, name: &str, kind: Option<&str>) -> Result<(), GitOperationError>;
pub async fn rename_branch(root: &Path, old: &str, new: &str) -> Result<(), GitOperationError>;
pub async fn delete_branch(root: &Path, name: &str, force: bool) -> Result<(), GitOperationError>;

pub async fn init(root: &Path, branch: Option<&str>) -> Result<(), GitOperationError>;
pub async fn fetch(root: &Path) -> Result<(), GitOperationError>;
pub async fn set_remote(root: &Path, remote_url: &str) -> Result<(), GitOperationError>;
pub async fn discard(root: &Path, paths: &[String]) -> Result<(), GitOperationError>; // empty => all
pub async fn ignore(root: &Path, path: &str) -> Result<(), GitOperationError>;
pub async fn stash_push(root: &Path, message: Option<&str>) -> Result<(), GitOperationError>;
pub async fn stash_pop(root: &Path) -> Result<(), GitOperationError>;
```

- APIs:
  - `GET /branches`
  - `POST /branches`
  - `POST /branches/switch`
  - `POST /branches/rename`
  - `POST /branches/delete`
  - `POST /init`
  - `POST /fetch`
  - `POST /set-remote`
  - `POST /discard`
  - `POST /ignore`
  - `POST /stash/push`
  - `POST /stash/pop`

- [ ] **Step 1: Implement branch list/mutations**

Use `git for-each-ref` / `git branch -vv` style parsing. Switching a remote branch should create/switch a local tracking branch when needed (`git switch --track` / `git switch -c`).

- [ ] **Step 2: Implement init/fetch/set-remote**

```rust
// init: git init -b <branch?>
// set-remote: git remote add origin <url> or git remote set-url origin <url>
// fetch: git fetch --prune
```

Missing-remote failures must remain readable for frontend remote setup.

- [ ] **Step 3: Implement discard/ignore/stash with path safety**

```rust
// discard tracked: git restore --source=HEAD --staged --worktree -- <path>
// discard untracked: git clean -f -- <path>
// discard all: restore + clean carefully
// ignore: append relative path to .gitignore if not already present
// stash push/pop: git stash push -u [-m msg], git stash pop
```

Reject path escape attempts. Empty `paths` means all only for discard when body explicitly sets `all: true` to avoid accidents:

```rust
struct GroupWorkspaceGitDiscardRequest {
    paths: Vec<String>,
    all: bool,
}
```

- [ ] **Step 4: Tests and commit**

Cover:

- init turns not_repo into ready
- create/switch/rename/delete branch
- discard one file and all
- ignore appends gitignore
- stash push increments stash_count and pop restores
- set-remote then fetch does not 500 on empty remote repo fixtures when applicable

Run:

```powershell
cargo test -p ag-swarmer-backend --test groups workspace_git_ -- --nocapture
```

Commit:

```powershell
git add backend-rs/crates/backend/src/git backend-rs/crates/backend/src/api backend-rs/crates/backend/tests/groups.rs
git commit -m "feat(git): add branch remote discard and stash operations"
```

---

### Task 5: Frontend Types and Hooks

**Files:**
- Modify: `frontend/src/types/api.ts`
- Create: `frontend/src/hooks/useWorkspaceGit.ts`
- Modify: `frontend/src/hooks/useGroupFiles.ts` as needed for re-exports/compat
- Optional test: `frontend/src/hooks/useWorkspaceGit.test.ts`

**Interfaces:**
- Mirror backend DTOs exactly as specified in the design doc.
- Query keys:

```ts
workspaceGitQueryKey(groupId)
workspaceGitDiffQueryKey(groupId, mode, path)
workspaceGitLogQueryKey(groupId)
workspaceGitCommitQueryKey(groupId, sha)
workspaceGitCommitDiffQueryKey(groupId, sha, path)
workspaceGitBranchesQueryKey(groupId)
```

- Hooks:
  - status/diff/log/commit/branches queries
  - mutations for every write endpoint
  - invalidation:
    - all mutations -> status
    - branch mutations -> branches + log
    - stage/unstage/discard/commit/stash -> diff queries
    - pull/stash/discard -> workspace-files when content may change

- [ ] **Step 1: Add TS types**

Extend `GroupWorkspaceGitStatus` and add:

```ts
GroupWorkspaceGitDiff
GroupWorkspaceGitLog
GroupWorkspaceGitCommitSummary
GroupWorkspaceGitCommitDetails
GroupWorkspaceGitBranches
GroupWorkspaceGitBranch
GroupWorkspaceGitDiscardRequest
GroupWorkspaceGitRemoteRequest
GroupWorkspaceGitBranchCreateRequest
// etc.
```

- [ ] **Step 2: Move/expand Git hooks into `useWorkspaceGit.ts`**

Keep existing export names used by current UI (`useGroupWorkspaceGitStatus`, `useStageGroupWorkspaceGit`, ...) either in the new file or re-exported from `useGroupFiles.ts` so intermediate refactors do not break imports.

- [ ] **Step 3: Smoke-check type generation and commit**

Run:

```powershell
pnpm --filter @ag-swarmer/frontend type-check
```

Commit:

```powershell
git add frontend/src/types/api.ts frontend/src/hooks/useWorkspaceGit.ts frontend/src/hooks/useGroupFiles.ts
git commit -m "feat(frontend): add workspace git review data hooks"
```

---

### Task 6: Git Review UI Shell (Toolbar + Changes + Diff)

**Files:**
- Modify: `frontend/src/components/chat/WorkspaceGitTab.tsx`
- Create: `frontend/src/components/chat/WorkspaceGitToolbar.tsx`
- Create: `frontend/src/components/chat/WorkspaceGitChangesView.tsx`
- Create: `frontend/src/components/chat/WorkspaceGitDiffPanel.tsx`
- Create: `frontend/src/components/chat/WorkspaceGitTab.test.tsx`
- Create: `frontend/src/components/chat/WorkspaceGitChangesView.test.tsx`

**Interfaces:**
- `WorkspaceGitTab` owns:
  - mode: `changes | history`
  - selected path + selected section
  - commit message
  - error banner
  - branch sheet open state
- Changes view consumes status files and emits stage/unstage/discard/select.
- Diff panel consumes `GroupWorkspaceGitDiff`.

- [ ] **Step 1: Rebuild tab shell**

```tsx
// toolbar
// error banners
// not_repo init CTA
// mode content:
//   changes -> WorkspaceGitChangesView + DiffPanel
//   history -> placeholder until Task 7
// branch sheet host
```

Use existing button/input/tabs/confirm-dialog primitives. Detect wide layout with a simple width threshold (~500px) via container measurement.

- [ ] **Step 2: Implement Changes sections**

Split files into:

```ts
const staged = files.filter(f => f.staged && !f.untracked)
const changes = files.filter(f => f.untracked || f.unstaged || f.conflicted)
```

Row actions:

- stage / unstage
- discard (ConfirmDialog)
- ignore when untracked
- select for diff

Bulk actions: stage all / unstage all / discard all(confirm).

- [ ] **Step 3: Implement Diff panel**

Render:

- path/mode header
- stat summary if present
- truncated notice
- binary placeholder
- patch in monospace pre/code with basic added/removed line coloring if cheap

Selected staged file uses `mode=staged`; selected changes file uses `mode=worktree`.

- [ ] **Step 4: Keep commit composer + AI message**

Preserve current commit form behavior and sparkles generate action.

- [ ] **Step 5: Frontend tests and commit**

Tests:

- renders staged and changes sections separately
- selecting a file requests the expected diff mode
- discard confirm gates mutation
- not_repo shows init CTA

Run:

```powershell
pnpm --filter @ag-swarmer/frontend test -- WorkspaceGit
pnpm --filter @ag-swarmer/frontend type-check
```

Commit:

```powershell
git add frontend/src/components/chat/WorkspaceGit*.tsx frontend/src/components/chat/WorkspaceGit*.test.tsx
git commit -m "feat(frontend): add workspace git changes review ui"
```

---

### Task 7: History UI + Branch Sheet + Remote/Stash/Init Flows

**Files:**
- Create: `frontend/src/components/chat/WorkspaceGitHistoryView.tsx`
- Create: `frontend/src/components/chat/WorkspaceGitBranchSheet.tsx`
- Modify: `frontend/src/components/chat/WorkspaceGitTab.tsx`
- Modify: `frontend/src/components/chat/WorkspaceGitToolbar.tsx`
- Create: `frontend/src/components/chat/WorkspaceGitHistoryView.test.tsx`
- Extend: `frontend/src/components/chat/WorkspaceGitTab.test.tsx`

**Interfaces:**
- History view:
  - infinite/load-more list from log query
  - selected commit details + file list + commit diff
  - create branch from commit dialog
- Branch sheet:
  - local/remote lists
  - create/switch/rename/delete
  - remote URL setup dialog when fetch/pull/push fail for missing remote
- Toolbar actions:
  - fetch/pull/push/refresh
  - stash push/pop
  - open branch sheet

- [ ] **Step 1: History view**

```tsx
// left/top: commit list
// right/bottom: details + files + DiffPanel(mode=commit)
// load more button when has_more
```

- [ ] **Step 2: Branch sheet**

Use existing `Sheet` primitive. Include:

- create branch input
- local/remote sections
- delete confirm
- rename inline/dialog

- [ ] **Step 3: Remote setup + init + stash**

- Init button on not_repo status opens confirm then `POST /init`
- Fetch/pull/push catch missing-remote style errors and open remote dialog; on save call `set-remote` and retry original action
- Stash push always available when dirty; stash pop confirms if dirty_counts indicate local changes

- [ ] **Step 4: Tests and commit**

Tests:

- history selection renders commit subject/diff
- branch sheet switch/create handlers fire
- missing remote opens setup dialog
- init CTA path works in not_repo state

Run:

```powershell
pnpm --filter @ag-swarmer/frontend test -- WorkspaceGit
pnpm --filter @ag-swarmer/frontend type-check
```

Commit:

```powershell
git add frontend/src/components/chat/WorkspaceGit*.tsx frontend/src/components/chat/WorkspaceGit*.test.tsx frontend/src/hooks/useWorkspaceGit.ts
git commit -m "feat(frontend): add git history branch and remote flows"
```

---

### Task 8: End-to-End Verification

**Files:**
- No new production files required unless fixes surface.

- [ ] **Step 1: Backend full git-related tests**

```powershell
cargo test -p ag-swarmer-backend --test groups workspace_git_ -- --nocapture
cargo check -p ag-swarmer-backend
```

Expected: pass.

- [ ] **Step 2: Frontend tests + typecheck**

```powershell
pnpm --filter @ag-swarmer/frontend test -- WorkspaceGit
pnpm --filter @ag-swarmer/frontend type-check
```

Expected: pass.

- [ ] **Step 3: Manual smoke checklist**

In a group workspace that is a git repo:

1. Changes shows staged/unstaged sections
2. File selection shows diff
3. Discard requires confirm and restores file
4. Commit + AI message still works
5. History lists commits and shows commit diff
6. Branch create/switch works
7. Fetch/pull/push surface errors cleanly; missing remote opens setup
8. Stash push/pop updates status
9. Non-repo workspace shows init and becomes ready after init

- [ ] **Step 4: Final commit only if verification fixes were needed**

```powershell
git status --short
# commit only source fixes, never .tmp-liveagent-git / build logs / .superpowers
```

---

## Spec Coverage Check

| Spec requirement | Task |
|---|---|
| Rich status + dirty counts + conflict/untracked metadata | Task 1 |
| Worktree/staged/branch diffs | Task 2 |
| History list/details/commit diff/create branch from commit | Task 3 + Task 7 |
| Branch management | Task 4 + Task 7 |
| Fetch / set-remote / init | Task 4 + Task 7 |
| Discard / ignore / stash | Task 4 + Task 6/7 |
| Changes/History UI in existing right panel | Task 6 + Task 7 |
| Confirm destructive actions | Task 6 + Task 7 |
| Keep AI commit message | Task 6 |
| No graph/virtualized swimlanes / no Tauri git client | All tasks |

## Placeholder / Consistency Review

- No TBD/TODO left in task steps.
- Endpoint names and DTO field names are consistent across backend and frontend tasks.
- Destructive request body uses explicit `all: true` for discard-all to avoid accidental wipes.
- Existing status/stage/unstage/commit/pull/push flows remain and are extended, not replaced.
