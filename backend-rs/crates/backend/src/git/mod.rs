mod diff;
mod ops;
mod runner;
mod status;

pub use diff::{diff, DiffMode, WorkspaceGitDiff};
pub use ops::{commit, pull, push, stage, staged_diff, status, unstage, GitOperationError};
pub use status::{WorkspaceGitDirtyCounts, WorkspaceGitFileStatus, WorkspaceGitStatus};
