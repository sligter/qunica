use std::{fmt, path::Path};

use super::{
    runner::{
        format_git_failure, git_command_error_message, git_output_is_not_repository,
        run_git_command,
    },
    status::{not_repo_status, parse_status, unavailable_status},
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
        Ok(output) if output.success => enrich_status(root, parse_status(&output.stdout)).await,
        Ok(output) if git_output_is_not_repository(&output) => {
            not_repo_status("workspace is not a Git repository")
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

async fn enrich_status(root: &Path, mut status: WorkspaceGitStatus) -> WorkspaceGitStatus {
    let (remote_name, remote_url) = resolve_remote(root).await;
    status.remote_name = remote_name;
    status.remote_url = remote_url;
    status.stash_count = resolve_stash_count(root).await;
    status
}

async fn resolve_remote(root: &Path) -> (Option<String>, Option<String>) {
    let remotes = match run_git_command(root, &git_args(&["remote"])).await {
        Ok(output) if output.success => output
            .stdout
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .map(str::to_string)
            .collect::<Vec<_>>(),
        _ => return (None, None),
    };
    if remotes.is_empty() {
        return (None, None);
    }

    let remote_name = remotes
        .iter()
        .find(|name| name.as_str() == "origin")
        .cloned()
        .or_else(|| remotes.first().cloned());
    let Some(remote_name) = remote_name else {
        return (None, None);
    };

    let remote_url = match run_git_command(
        root,
        &git_args(&["remote", "get-url", remote_name.as_str()]),
    )
    .await
    {
        Ok(output) if output.success => {
            let url = output.stdout.trim();
            if url.is_empty() {
                None
            } else {
                Some(url.to_string())
            }
        }
        _ => None,
    };

    (Some(remote_name), remote_url)
}

async fn resolve_stash_count(root: &Path) -> usize {
    match run_git_command(
        root,
        &git_args(&["rev-list", "--walk-reflogs", "--count", "refs/stash"]),
    )
    .await
    {
        Ok(output) if output.success => output.stdout.trim().parse::<usize>().unwrap_or(0),
        _ => 0,
    }
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
