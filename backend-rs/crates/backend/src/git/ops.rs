use std::{fmt, fs, path::Path};

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
    code: Option<&'static str>,
}

impl fmt::Display for GitOperationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for GitOperationError {}

impl GitOperationError {
    pub(super) fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            code: None,
        }
    }

    pub(super) fn missing_remote() -> Self {
        Self {
            message: "git remote is not configured; set a remote URL before fetch, pull, or push"
                .to_string(),
            code: Some("missing_remote"),
        }
    }

    pub fn code(&self) -> Option<&'static str> {
        self.code
    }
}

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
    let targets = if paths.is_empty() {
        &[".".to_string()][..]
    } else {
        paths
    };
    let args = if has_head(root).await? {
        git_args_with_paths(&["restore", "--staged", "--"], targets)
    } else {
        git_args_with_paths(
            &["rm", "--cached", "-f", "-r", "--ignore-unmatch", "--"],
            targets,
        )
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
    .map_err(|err| GitOperationError::new(git_command_error_message(err)))?;
    if output.success {
        Ok(output.stdout)
    } else {
        Err(GitOperationError::new(format_git_failure(
            "git staged diff failed",
            &output,
        )))
    }
}

pub async fn pull(root: &Path) -> Result<(), GitOperationError> {
    ensure_remote(root).await?;
    run_git_or_error(root, &git_args(&["pull", "--ff-only"]), "git pull failed").await
}

pub async fn push(root: &Path) -> Result<(), GitOperationError> {
    push_with_mode(root, false).await
}

pub async fn force_push(root: &Path) -> Result<(), GitOperationError> {
    push_with_mode(root, true).await
}

pub async fn rebase(root: &Path) -> Result<(), GitOperationError> {
    ensure_remote(root).await?;
    run_git_or_error(
        root,
        &git_args(&["pull", "--rebase"]),
        "git rebase from upstream failed",
    )
    .await
}

pub async fn init(root: &Path, branch: Option<&str>) -> Result<(), GitOperationError> {
    let mut args = git_args(&["init"]);
    if let Some(branch) = branch {
        validate_branch_name(root, branch).await?;
        args.push("-b".to_string());
        args.push(branch.to_string());
    }
    run_git_or_error(root, &args, "git init failed").await
}

pub async fn fetch(root: &Path) -> Result<(), GitOperationError> {
    ensure_remote(root).await?;
    run_git_or_error(root, &git_args(&["fetch", "--prune"]), "git fetch failed").await
}

pub async fn set_remote(root: &Path, remote_url: &str) -> Result<(), GitOperationError> {
    let remote_url = remote_url.trim();
    if remote_url.is_empty() || remote_url.contains(['\r', '\n']) {
        return Err(GitOperationError::new("remote URL is invalid"));
    }
    let has_origin = run_git_command(root, &git_args(&["remote", "get-url", "origin"]))
        .await
        .map_err(|err| GitOperationError::new(git_command_error_message(err)))?
        .success;
    let args = if has_origin {
        vec![
            "remote".to_string(),
            "set-url".to_string(),
            "origin".to_string(),
            remote_url.to_string(),
        ]
    } else {
        vec![
            "remote".to_string(),
            "add".to_string(),
            "origin".to_string(),
            remote_url.to_string(),
        ]
    };
    run_git_or_error(root, &args, "git set remote failed").await
}

pub async fn discard(root: &Path, paths: &[String]) -> Result<(), GitOperationError> {
    let all = paths.is_empty();
    let tracked = if all {
        vec![".".to_string()]
    } else {
        tracked_paths(root, paths).await?
    };
    if !tracked.is_empty() && has_head(root).await? {
        let mut args = git_args(&["restore", "--source=HEAD", "--staged", "--worktree", "--"]);
        args.extend(tracked);
        run_git_or_error(root, &args, "git discard failed").await?;
    }
    let mut clean_args = git_args(&["clean", "-fd"]);
    if !all {
        clean_args.push("--".to_string());
        clean_args.extend(paths.iter().cloned());
    }
    run_git_or_error(root, &clean_args, "git discard untracked files failed").await
}

pub async fn ignore(root: &Path, path: &str) -> Result<(), GitOperationError> {
    let ignore_path = root.join(".gitignore");
    let existing = fs::read_to_string(&ignore_path).unwrap_or_default();
    if existing.lines().any(|line| line == path) {
        return Ok(());
    }
    let mut updated = existing;
    if !updated.is_empty() && !updated.ends_with('\n') {
        updated.push('\n');
    }
    updated.push_str(path);
    updated.push('\n');
    fs::write(ignore_path, updated)
        .map_err(|_| GitOperationError::new("failed to update .gitignore"))
}

pub async fn stash_push(root: &Path, message: Option<&str>) -> Result<(), GitOperationError> {
    let mut args = git_args(&["stash", "push", "-u"]);
    if let Some(message) = message.map(str::trim).filter(|message| !message.is_empty()) {
        args.push("-m".to_string());
        args.push(message.to_string());
    }
    run_git_or_error(root, &args, "git stash push failed").await
}

pub async fn stash_pop(root: &Path) -> Result<(), GitOperationError> {
    run_git_or_error(root, &git_args(&["stash", "pop"]), "git stash pop failed").await
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

async fn ensure_remote(root: &Path) -> Result<(), GitOperationError> {
    remote_name(root).await.map(|_| ())
}

async fn remote_name(root: &Path) -> Result<String, GitOperationError> {
    let output = run_git_command(root, &git_args(&["remote"]))
        .await
        .map_err(|err| GitOperationError::new(git_command_error_message(err)))?;
    if !output.success {
        return Err(GitOperationError::new(format_git_failure(
            "git remote lookup failed",
            &output,
        )));
    }
    let remotes = output
        .stdout
        .lines()
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .collect::<Vec<_>>();
    remotes
        .iter()
        .find(|name| **name == "origin")
        .or_else(|| remotes.first())
        .map(|name| (*name).to_string())
        .ok_or_else(GitOperationError::missing_remote)
}

async fn push_with_mode(root: &Path, force: bool) -> Result<(), GitOperationError> {
    let remote = remote_name(root).await?;
    let mut args = git_args(&["push"]);
    if force {
        args.push("--force-with-lease".to_string());
    }
    if !has_upstream(root).await? {
        args.extend(["--set-upstream".to_string(), remote, "HEAD".to_string()]);
    }
    run_git_or_error(
        root,
        &args,
        if force {
            "git force push failed"
        } else {
            "git push failed"
        },
    )
    .await
}

async fn has_upstream(root: &Path) -> Result<bool, GitOperationError> {
    let output = run_git_command(root, &git_args(&["rev-parse", "--verify", "@{upstream}"]))
        .await
        .map_err(|err| GitOperationError::new(git_command_error_message(err)))?;
    Ok(output.success)
}

async fn has_head(root: &Path) -> Result<bool, GitOperationError> {
    let output = run_git_command(root, &git_args(&["rev-parse", "--verify", "HEAD"]))
        .await
        .map_err(|err| GitOperationError::new(git_command_error_message(err)))?;
    Ok(output.success)
}

async fn tracked_paths(root: &Path, paths: &[String]) -> Result<Vec<String>, GitOperationError> {
    let args = git_args_with_paths(&["ls-files", "-z", "--"], paths);
    let output = run_git_command(root, &args)
        .await
        .map_err(|err| GitOperationError::new(git_command_error_message(err)))?;
    if !output.success {
        return Err(GitOperationError::new(format_git_failure(
            "git tracked path lookup failed",
            &output,
        )));
    }
    Ok(output
        .stdout
        .split('\0')
        .filter(|path| !path.is_empty())
        .map(str::to_string)
        .collect())
}

async fn validate_branch_name(root: &Path, branch: &str) -> Result<(), GitOperationError> {
    if branch.trim().is_empty() || branch != branch.trim() || branch.starts_with('-') {
        return Err(GitOperationError::new("branch name is invalid"));
    }
    run_git_or_error(
        root,
        &git_args(&["check-ref-format", "--branch", branch]),
        "git branch validation failed",
    )
    .await
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
        .map_err(|err| GitOperationError::new(git_command_error_message(err)))?;
    if output.success {
        Ok(())
    } else {
        Err(GitOperationError::new(format_git_failure(context, &output)))
    }
}

#[cfg(test)]
mod tests {
    use std::{path::Path, process::Command};

    use super::{force_push, push};

    fn git(root: &Path, args: &[&str]) -> String {
        let output = Command::new("git")
            .arg("-C")
            .arg(root)
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&output.stderr),
        );
        String::from_utf8_lossy(&output.stdout).into_owned()
    }

    #[tokio::test]
    async fn push_publishes_a_branch_and_force_push_uses_the_tracking_ref() {
        let remote = tempfile::tempdir().unwrap();
        let local = tempfile::tempdir().unwrap();
        git(remote.path(), &["init", "--bare"]);
        git(local.path(), &["init", "-b", "feature/test"]);
        git(local.path(), &["config", "user.email", "test@example.com"]);
        git(local.path(), &["config", "user.name", "Test User"]);
        std::fs::write(local.path().join("readme.txt"), "one\n").unwrap();
        git(local.path(), &["add", "readme.txt"]);
        git(local.path(), &["commit", "-m", "initial"]);
        git(
            local.path(),
            &["remote", "add", "origin", remote.path().to_str().unwrap()],
        );

        push(local.path()).await.unwrap();
        assert_eq!(
            git(local.path(), &["rev-parse", "--abbrev-ref", "@{upstream}"]).trim(),
            "origin/feature/test",
        );

        std::fs::write(local.path().join("readme.txt"), "two\n").unwrap();
        git(local.path(), &["add", "readme.txt"]);
        git(local.path(), &["commit", "--amend", "--no-edit"]);
        force_push(local.path()).await.unwrap();
        assert_eq!(
            git(local.path(), &["rev-parse", "HEAD"]).trim(),
            git(remote.path(), &["rev-parse", "refs/heads/feature/test"]).trim(),
        );
    }
}
