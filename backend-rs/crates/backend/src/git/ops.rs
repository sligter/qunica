use std::{fmt, path::Path};

use super::{
    runner::{
        format_git_failure, git_command_error_message, git_output_is_not_repository,
        run_git_command,
    },
    status::{parse_status, unavailable_status},
    WorkspaceGitStatus,
};

#[derive(Debug)]
pub struct GitOperationError {
    message: String,
}

impl fmt::Display for GitOperationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for GitOperationError {}

pub async fn status(root: &Path) -> WorkspaceGitStatus {
    let args = git_args(&[
        "--no-optional-locks",
        "-c",
        "core.quotePath=false",
        "status",
        "--porcelain=v2",
        "-z",
        "-b",
    ]);
    match run_git_command(root, &args).await {
        Ok(output) if output.success => parse_status(&output.stdout),
        Ok(output) if git_output_is_not_repository(&output) => {
            unavailable_status("workspace is not a Git repository")
        }
        Ok(output) => unavailable_status(format_git_failure("git status failed", &output)),
        Err(err) => unavailable_status(git_command_error_message(err)),
    }
}

pub async fn stage(root: &Path, paths: &[String]) -> Result<(), GitOperationError> {
    let args = if paths.is_empty() {
        git_args(&["add", "-A"])
    } else {
        git_args_with_paths(&["add", "--"], paths)
    };
    run_git_or_error(root, &args, "git stage failed").await
}

pub async fn unstage(root: &Path, paths: &[String]) -> Result<(), GitOperationError> {
    let args = if paths.is_empty() {
        git_args(&["reset", "--", "."])
    } else {
        git_args_with_paths(&["reset", "--"], paths)
    };
    run_git_or_error(root, &args, "git unstage failed").await
}

pub async fn commit(root: &Path, message: String) -> Result<(), GitOperationError> {
    let args = vec!["commit".to_string(), "-m".to_string(), message];
    run_git_or_error(root, &args, "git commit failed").await
}

pub async fn staged_diff(root: &Path) -> Result<String, GitOperationError> {
    let output = run_git_command(
        root,
        &git_args(&[
            "--no-optional-locks",
            "-c",
            "core.quotePath=false",
            "diff",
            "--cached",
            "--no-ext-diff",
            "--find-renames",
            "--stat",
            "--patch",
        ]),
    )
    .await
    .map_err(|err| GitOperationError {
        message: git_command_error_message(err),
    })?;
    if output.success {
        Ok(output.stdout)
    } else {
        Err(GitOperationError {
            message: format_git_failure("git staged diff failed", &output),
        })
    }
}

pub async fn pull(root: &Path) -> Result<(), GitOperationError> {
    run_git_or_error(root, &git_args(&["pull", "--ff-only"]), "git pull failed").await
}

pub async fn push(root: &Path) -> Result<(), GitOperationError> {
    run_git_or_error(root, &git_args(&["push"]), "git push failed").await
}

fn git_args(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|part| (*part).to_string()).collect()
}

fn git_args_with_paths(prefix: &[&str], paths: &[String]) -> Vec<String> {
    let mut args = git_args(prefix);
    args.extend(paths.iter().cloned());
    args
}

async fn run_git_or_error(
    root: &Path,
    args: &[String],
    context: &'static str,
) -> Result<(), GitOperationError> {
    let output = run_git_command(root, args)
        .await
        .map_err(|err| GitOperationError {
            message: git_command_error_message(err),
        })?;
    if output.success {
        Ok(())
    } else {
        Err(GitOperationError {
            message: format_git_failure(context, &output),
        })
    }
}
