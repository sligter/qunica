use std::path::Path;

use serde::{Deserialize, Serialize};

use super::{
    ops::GitOperationError,
    runner::{
        format_git_failure, git_command_error_message, run_git_command,
        run_git_command_with_output_limit,
    },
};

const MAX_DIFF_PATCH_CHARS: usize = 200_000;

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DiffMode {
    Worktree,
    Staged,
    Branch,
}

pub(super) async fn commit_diff(
    root: &Path,
    sha: &str,
    path: Option<&str>,
) -> Result<WorkspaceGitDiff, GitOperationError> {
    let mut patch_args = vec![
        "--no-optional-locks".to_string(),
        "-c".to_string(),
        "core.quotePath=false".to_string(),
        "show".to_string(),
        "--no-ext-diff".to_string(),
        "--find-renames".to_string(),
        "--format=".to_string(),
        "--patch".to_string(),
        sha.to_string(),
    ];
    append_path(&mut patch_args, path);
    let patch = run_diff(root, &patch_args, "git commit diff failed").await?;

    let mut stat_args = vec![
        "--no-optional-locks".to_string(),
        "-c".to_string(),
        "core.quotePath=false".to_string(),
        "show".to_string(),
        "--format=".to_string(),
        "--stat".to_string(),
        sha.to_string(),
    ];
    append_path(&mut stat_args, path);
    let stat = run_diff(root, &stat_args, "git commit diff stat failed").await?;

    let binary_files = binary_files(&patch);
    let (patch, truncated) = truncate_patch(patch);
    Ok(WorkspaceGitDiff {
        mode: "commit".to_string(),
        base_ref: Some(format!("{sha}^")),
        head_ref: Some(sha.to_string()),
        path: path.map(str::to_string),
        patch,
        stat,
        truncated,
        binary_files,
    })
}

impl DiffMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Worktree => "worktree",
            Self::Staged => "staged",
            Self::Branch => "branch",
        }
    }
}

#[derive(Debug, Serialize)]
pub struct WorkspaceGitDiff {
    pub mode: String,
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
) -> Result<WorkspaceGitDiff, GitOperationError> {
    let (base_ref, head_ref) = match mode {
        DiffMode::Worktree => (Some("index".to_string()), Some("worktree".to_string())),
        DiffMode::Staged => (Some("HEAD".to_string()), Some("index".to_string())),
        DiffMode::Branch => (
            Some(resolve_branch_base(root).await?),
            Some("HEAD".to_string()),
        ),
    };

    let mut patch_args = diff_args(mode, base_ref.as_deref());
    append_path(&mut patch_args, path);
    let patch = run_diff(root, &patch_args, "git diff failed").await?;

    let mut stat_args = diff_args(mode, base_ref.as_deref());
    stat_args.push("--stat".to_string());
    append_path(&mut stat_args, path);
    let stat = run_diff(root, &stat_args, "git diff stat failed").await?;

    let (patch, truncated) = truncate_patch(patch);
    let binary_files = binary_files(&patch);
    Ok(WorkspaceGitDiff {
        mode: mode.as_str().to_string(),
        base_ref,
        head_ref,
        path: path.map(str::to_string),
        patch,
        stat,
        truncated,
        binary_files,
    })
}

fn diff_args(mode: DiffMode, base_ref: Option<&str>) -> Vec<String> {
    let mut args = vec![
        "--no-optional-locks".to_string(),
        "-c".to_string(),
        "core.quotePath=false".to_string(),
        "diff".to_string(),
        "--no-ext-diff".to_string(),
        "--find-renames".to_string(),
    ];
    match mode {
        DiffMode::Worktree => {}
        DiffMode::Staged => args.push("--cached".to_string()),
        DiffMode::Branch => args.push(format!("{}...HEAD", base_ref.unwrap_or("HEAD"))),
    }
    args
}

fn append_path(args: &mut Vec<String>, path: Option<&str>) {
    if let Some(path) = path {
        args.push("--".to_string());
        args.push(path.to_string());
    }
}

async fn run_diff(
    root: &Path,
    args: &[String],
    context: &'static str,
) -> Result<String, GitOperationError> {
    let output = run_git_command_with_output_limit(root, args, MAX_DIFF_PATCH_CHARS + 1)
        .await
        .map_err(|err| GitOperationError::new(git_command_error_message(err)))?;
    if output.success {
        Ok(output.stdout)
    } else {
        Err(GitOperationError::new(format_git_failure(context, &output)))
    }
}

async fn resolve_branch_base(root: &Path) -> Result<String, GitOperationError> {
    let upstream = run_git_command(
        root,
        &[
            "rev-parse".to_string(),
            "--verify".to_string(),
            "@{upstream}".to_string(),
        ],
    )
    .await
    .map_err(|err| GitOperationError::new(git_command_error_message(err)))?;
    if upstream.success {
        return Ok(upstream.stdout.trim().to_string());
    }

    let merge_base = run_git_command(
        root,
        &[
            "rev-parse".to_string(),
            "--verify".to_string(),
            "HEAD^".to_string(),
        ],
    )
    .await
    .map_err(|err| GitOperationError::new(git_command_error_message(err)))?;
    if merge_base.success {
        return Ok(merge_base.stdout.trim().to_string());
    }

    Err(GitOperationError::new(
        "git branch diff failed: branch has no upstream or base commit",
    ))
}

fn truncate_patch(patch: String) -> (String, bool) {
    let runner_truncated = patch.ends_with("\n[output truncated]");
    if !runner_truncated && patch.chars().count() <= MAX_DIFF_PATCH_CHARS {
        return (patch, false);
    }
    let patch: String = patch.chars().take(MAX_DIFF_PATCH_CHARS).collect();
    (format!("{patch}\n[diff truncated]"), true)
}

fn binary_files(patch: &str) -> Vec<String> {
    let mut files = Vec::new();
    let mut current_path = None;
    for line in patch.lines() {
        if let Some(path) = line.strip_prefix("diff --git a/") {
            current_path = path.split(" b/").nth(1).map(str::to_string);
        }
        if line.starts_with("Binary files ") || line == "GIT binary patch" {
            if let Some(path) = current_path.as_ref() {
                if !files.contains(path) {
                    files.push(path.clone());
                }
            }
        }
    }
    files
}
