# Workspace Git Review

## Goal

Upgrade the group workspace right-side `Git` tab from a basic status/commit panel into a Git review surface that covers LiveAgent's core capabilities, while keeping ag-swarmer's existing layout language and backend-owned Git execution model.

Success looks like:

- Users can inspect staged and unstaged changes, review diffs, discard safely, and commit from the right panel.
- Users can browse commit history, open commit details/diffs, and create a branch from a commit.
- Users can manage branches and remotes, fetch/pull/push, stash, and initialize a repository when the workspace is not yet a Git repo.
- All Git execution stays on the backend under the group's workspace root.

## Non-Goals

- Pixel-perfect or byte-for-byte port of LiveAgent's Git Review UI.
- Commit graph swimlanes / virtualized history canvas.
- Full desktop-style context-menu system.
- Direct browser/desktop Git client access that bypasses the backend.
- Deep GitHub integration (open PR, rich remote browsing beyond branch list and remote URL setup).

## Current Baseline

ag-swarmer already has:

- Right workspace panel tabs: `Files | Git`
- Backend workspace Git APIs for status, stage, unstage, commit, commit-message generation, pull, push
- Frontend `WorkspaceGitTab` with branch summary, change list, stage/unstage, commit, AI commit message, pull/push

Missing relative to LiveAgent's right Git panel:

- Staged/unstaged sectioned review and richer file metadata
- Worktree/staged/branch/commit diffs
- History mode
- Branch management
- Fetch / set-remote / init
- Discard / add-to-gitignore / stash

## Product Decisions

1. Scope: capability alignment with LiveAgent Git review, not visual clone.
2. Implementation strategy: extend existing `workspace-git` stack (Approach A).
3. UI strategy: keep ag-swarmer panel/tab/button/dialog patterns.
4. Delivery: design and implement as one coherent feature set; internal task split is allowed, but the shipped behavior should present as one Git Review panel.

## Information Architecture

The `Git` tab becomes a self-contained review panel.

### Toolbar

- Current branch control (opens branch sheet)
- Summary chips/text: ahead/behind, dirty counts, stash count
- Actions: Fetch, Pull, Push, Refresh
- Mode switch: `Changes` | `History`
- When repo is unavailable because the workspace is not a Git repository: show `Init repository`

### Changes mode

- Two sections:
  - `Staged`
  - `Changes` (unstaged tracked changes, untracked files, conflicted files)
- File row actions:
  - stage / unstage
  - discard (confirm)
  - add to `.gitignore` for untracked files
  - open in Files tab when useful
- Selecting a file loads its diff:
  - staged section defaults to staged diff
  - changes section defaults to worktree diff
- Optional whole-tree diffs:
  - worktree
  - staged
  - branch (against upstream/merge-base when available)
- Commit composer remains at the bottom:
  - message input
  - AI generate commit message
  - commit staged changes
- Bulk actions:
  - stage all
  - unstage all
  - discard all (confirm)

### History mode

- Commit list with:
  - short SHA
  - subject
  - author
  - date
  - local-only marker when known
- Selecting a commit shows:
  - subject/body
  - author/date
  - file list
  - commit-level or file-level diff
- Load more via `limit/skip`
- Action: create branch from commit

### Branch sheet

- Local and remote branch lists
- Current branch highlight
- Actions:
  - switch branch
  - create branch
  - rename local branch
  - delete local branch (confirm)
  - create local branch from remote/start point
- Remote setup:
  - if push/fetch/pull fails due to missing remote, open remote URL dialog
  - save remote then optionally continue the original action

## Backend Design

### Ownership boundary

Backend owns all Git process execution. Frontend never shells out to Git and never receives credentials beyond what is already stored server-side for the workspace environment.

All endpoints remain under:

```text
/api/v2/groups/:group_id/workspace-git/*
```

Every endpoint:

1. Authenticates the current user
2. Loads the owned active group
3. Resolves the group workspace root
4. Validates path inputs against that root
5. Runs the Git operation
6. Returns structured JSON

### Status model

Upgrade `WorkspaceGitStatus` to a richer repository state while remaining backward-compatible where practical:

```ts
type WorkspaceGitStatus = {
  available: boolean
  status: 'ready' | 'not_repo' | 'error'
  branch: string | null
  upstream: string | null
  remote_name: string | null
  remote_url: string | null
  ahead: number | null
  behind: number | null
  stash_count: number
  clean: boolean
  dirty_counts: {
    staged: number
    unstaged: number
    untracked: number
    conflicted: number
  }
  files: WorkspaceGitFileStatus[]
  message: string | null
}

type WorkspaceGitFileStatus = {
  path: string
  old_path: string | null
  status: string
  staged: boolean
  unstaged: boolean
  untracked: boolean
  conflicted: boolean
}
```

Write operations continue returning the latest `WorkspaceGitStatus` after mutation so the panel can refresh from the response and/or invalidate the status query.

### Query APIs

- `GET /status`
  - Full repository state
- `GET /diff?mode=worktree|staged|branch&path?=`
  - Returns patch, stat, truncated flag, binary file list, and resolved base/head labels
- `GET /log?limit=&skip=`
  - Commit summaries plus `has_more`
- `GET /commits/:sha`
  - Commit details: subject, body, author, dates, files, insertions/deletions/stat
- `GET /commits/:sha/diff?path?=`
  - Commit patch for whole commit or one path
- `GET /branches`
  - Local/remote branches with current/upstream/ahead/behind metadata

### Mutation APIs

Keep and enhance:

- `POST /stage`
- `POST /unstage`
- `POST /commit`
- `POST /commit-message`
- `POST /pull`
- `POST /push`

Add:

- `POST /init`
- `POST /fetch`
- `POST /set-remote`
- `POST /discard`
- `POST /ignore`
- `POST /branches` (create)
- `POST /branches/switch`
- `POST /branches/rename`
- `POST /branches/delete`
- `POST /stash/push`
- `POST /stash/pop`
- `POST /commits/:sha/create-branch`

### Diff response shape

```ts
type WorkspaceGitDiff = {
  mode: 'worktree' | 'staged' | 'branch' | 'commit'
  base_ref: string | null
  head_ref: string | null
  path: string | null
  patch: string
  stat: string
  truncated: boolean
  binary_files: string[]
}
```

Large diffs may be truncated server-side with a clear `truncated: true` flag. The UI must render partial diffs safely and show that more content was omitted.

### Safety rules

- Path arguments are validated with the existing workspace path sanitizer; reject escapes outside the workspace root.
- `discard` only operates on validated relative paths or explicit "all" mode.
- Destructive branch deletion requires an explicit API call and confirmation in the UI.
- `push`, `pull`, and `fetch` surface readable errors. Missing remote should be distinguishable so the frontend can open remote setup.
- Never return raw secrets, credential helper output, or unrestricted process dumps in API errors.
- Prefer `--ff-only` pull behavior already used by the current backend unless a later design revisits merge strategy.

### Backend module split

Extend `backend-rs/crates/backend/src/git/`:

- `status.rs` — porcelain parsing and dirty counts
- `diff.rs` — worktree/staged/branch/commit diffs
- `log.rs` — history and commit details
- `branches.rs` — branch listing and branch mutations
- `ops.rs` — stage/unstage/commit/fetch/pull/push/discard/ignore/stash/init/remote
- `runner.rs` — shared command execution

`api/groups.rs` remains the HTTP adapter: auth, request validation, DTO mapping, error translation.

## Frontend Design

### Component structure

Replace the current single-file Git tab responsibilities with focused components:

- `WorkspaceGitTab.tsx` — assembly, mode state, selection state, error/busy coordination
- `WorkspaceGitToolbar.tsx` — branch summary, remote actions, mode switch, init entry
- `WorkspaceGitChangesView.tsx` — staged/changes lists and commit composer
- `WorkspaceGitDiffPanel.tsx` — shared diff rendering
- `WorkspaceGitHistoryView.tsx` — commit list/details
- `WorkspaceGitBranchSheet.tsx` — branch management + remote setup dialog entry

Shared UI primitives stay on the existing design system:

- buttons, tabs, inputs, sheets/dialogs, confirm dialogs, scroll areas

### Data layer

Extend `frontend/src/hooks/useGroupFiles.ts` (or extract a dedicated `useWorkspaceGit.ts` if the file becomes unwieldy) with:

- status query
- diff query keyed by mode/path
- log query with pagination
- commit details/diff queries
- branches query
- mutations for all write operations

Invalidation rules:

- Any mutation invalidates status
- Branch mutations also invalidate branches and history
- Commit/stash/discard/stage mutations invalidate diff queries
- History pagination appends rather than replacing unless a full refresh is requested

### Layout behavior

- Narrow panel: stacked list-first layout; selecting a file/commit expands detail/diff below
- Wide panel (~500px+): side-by-side list + detail/diff
- Keep the panel inside the existing right workspace column; do not introduce a second dock system

### File navigation

- Primary action for a changed file in Git view is "inspect diff here"
- Secondary action can open the file in the Files tab via existing `fileNavStore`
- Deleted files should not attempt a useless Files-tab open

### Dangerous action confirmations

Use existing confirm/alert dialogs for:

- discard one file
- discard all
- delete branch
- stash pop when it may overwrite local changes
- init repository

No confirmation for ordinary stage/unstage/commit/fetch/pull/push/create/switch branch. Failures show inline errors.

### Explicit non-copies from LiveAgent

- No commit-graph SVG swimlanes
- No large mirrored right-dock registry abstraction
- No full context-menu framework; use row actions + overflow menus
- No Tauri invoke Git client; backend HTTP only

## Error Handling

- Status `not_repo`: show init CTA and disable review actions
- Status `error`: show message and keep refresh available
- Mutation failure: preserve user input (for example commit message), show error banner, leave selection intact when possible
- Missing remote: open remote setup dialog with the failed action context (`fetch` / `pull` / `push`)
- Diff truncation: render available patch and show truncated notice
- Binary files: show placeholder rather than raw binary patch noise

## Testing Strategy

### Backend

- Status parsing for staged/unstaged/untracked/conflict/rename and dirty counts
- Diff endpoints for worktree/staged/branch/commit and path filtering
- Log pagination and commit details
- Branch create/switch/rename/delete
- Discard path validation and "all" mode
- Init / set-remote / stash push-pop
- Existing stage/unstage/commit/pull/push regressions remain green

### Frontend

- Toolbar mode switching and disabled states
- Changes sections render staged vs unstaged correctly
- Selecting a file loads the expected diff mode
- Discard/delete confirmations gate the mutation
- History selection shows details/diff
- Branch sheet actions call the right endpoints and refresh state
- not_repo state shows init flow

No strict TDD requirement for this workstream; implement the feature, then cover the critical paths above with focused tests.

## Rollout Notes

- Feature ships behind the existing Git tab; no new navigation entry is required
- API additions are additive; old clients that only understand the previous status fields should continue to function if they ignore unknown JSON fields
- Desktop and web both consume the same backend APIs

## Open Follow-ups (Out of Scope Now)

- Commit graph visualization
- Merge/rebase conflict resolution UI beyond surfacing conflicted files
- GitHub PR creation / remote browsing
- Signed commits / advanced auth UX beyond current environment-provided credentials
