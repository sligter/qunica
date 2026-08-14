mod branches;
mod diff;
mod log;
mod ops;
mod runner;
mod status;

pub use branches::{
    branches, create_branch, create_task_worktree, delete_branch, remove_task_worktree,
    rename_branch, switch_branch, TaskWorktree, WorkspaceGitBranch, WorkspaceGitBranches,
};
pub use diff::{diff, DiffMode, WorkspaceGitDiff};
pub use log::{
    commit_details, commit_diff, create_branch_from_commit, log, pagination_is_valid,
    WorkspaceGitCommitDetails, WorkspaceGitCommitFile, WorkspaceGitCommitSummary, WorkspaceGitLog,
};
pub use ops::{
    commit, discard, fetch, force_push, ignore, init, pull, push, rebase, set_remote, stage,
    staged_diff, stash_pop, stash_push, status, unstage, GitOperationError,
};
pub use status::{WorkspaceGitDirtyCounts, WorkspaceGitFileStatus, WorkspaceGitStatus};
