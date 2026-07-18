use std::path::Path;

use serde::Serialize;

use super::{
    ops::GitOperationError,
    runner::{format_git_failure, git_command_error_message, run_git_command},
};

#[derive(Debug, Serialize)]
pub struct WorkspaceGitBranch {
    pub name: String,
    pub full_name: String,
    pub kind: String,
    pub current: bool,
    pub upstream: Option<String>,
    pub ahead: i64,
    pub behind: i64,
}

#[derive(Debug, Serialize)]
pub struct WorkspaceGitBranches {
    pub branches: Vec<WorkspaceGitBranch>,
}

pub async fn branches(root: &Path) -> Result<WorkspaceGitBranches, GitOperationError> {
    let output = run_git(
        root,
        vec![
            "for-each-ref".to_string(),
            "--format=%(refname)%1f%(refname:short)%1f%(HEAD)%1f%(upstream:short)%1f%(upstream:track)".to_string(),
            "refs/heads".to_string(),
            "refs/remotes".to_string(),
        ],
        "git branch list failed",
    )
    .await?;
    Ok(WorkspaceGitBranches {
        branches: parse_branches(&output.stdout),
    })
}

pub async fn create_branch(
    root: &Path,
    name: &str,
    start_point: Option<&str>,
) -> Result<(), GitOperationError> {
    validate_branch_name(root, name).await?;
    let mut args = vec!["branch".to_string(), name.to_string()];
    if let Some(start_point) = start_point {
        args.push(resolve_start_point(root, start_point).await?);
    }
    run_git(root, args, "git create branch failed").await?;
    Ok(())
}

pub async fn switch_branch(
    root: &Path,
    name: &str,
    kind: Option<&str>,
) -> Result<(), GitOperationError> {
    let branches = branches(root).await?.branches;
    let kind = kind.unwrap_or("local");
    let target = branches
        .iter()
        .find(|branch| branch.kind == kind && (branch.name == name || branch.full_name == name))
        .ok_or_else(|| GitOperationError::new("branch does not exist"))?;

    if target.kind == "local" {
        run_git(
            root,
            vec!["switch".to_string(), target.name.clone()],
            "git switch branch failed",
        )
        .await?;
        return Ok(());
    }

    let local_name = target
        .name
        .split_once('/')
        .map(|(_, name)| name)
        .filter(|name| !name.is_empty())
        .ok_or_else(|| GitOperationError::new("remote branch name is invalid"))?;
    if let Some(local) = branches
        .iter()
        .find(|branch| branch.kind == "local" && branch.name == local_name)
    {
        run_git(
            root,
            vec!["switch".to_string(), local.name.clone()],
            "git switch branch failed",
        )
        .await?;
    } else {
        run_git(
            root,
            vec![
                "switch".to_string(),
                "--track".to_string(),
                target.name.clone(),
            ],
            "git switch remote branch failed",
        )
        .await?;
    }
    Ok(())
}

pub async fn rename_branch(root: &Path, old: &str, new: &str) -> Result<(), GitOperationError> {
    validate_branch_name(root, new).await?;
    let old = resolve_local_branch(root, old).await?;
    run_git(
        root,
        vec!["branch".to_string(), "-m".to_string(), old, new.to_string()],
        "git rename branch failed",
    )
    .await?;
    Ok(())
}

pub async fn delete_branch(root: &Path, name: &str, force: bool) -> Result<(), GitOperationError> {
    let name = resolve_local_branch(root, name).await?;
    run_git(
        root,
        vec![
            "branch".to_string(),
            if force { "-D" } else { "-d" }.to_string(),
            name,
        ],
        "git delete branch failed",
    )
    .await?;
    Ok(())
}

async fn resolve_local_branch(root: &Path, value: &str) -> Result<String, GitOperationError> {
    branches(root)
        .await?
        .branches
        .into_iter()
        .find(|branch| {
            branch.kind == "local" && (branch.name == value || branch.full_name == value)
        })
        .map(|branch| branch.name)
        .ok_or_else(|| GitOperationError::new("local branch does not exist"))
}

async fn resolve_start_point(root: &Path, value: &str) -> Result<String, GitOperationError> {
    let value = value.trim();
    if is_strict_sha(value) {
        let output = run_git(
            root,
            vec![
                "rev-parse".to_string(),
                "--verify".to_string(),
                format!("{value}^{{commit}}"),
            ],
            "git start point lookup failed",
        )
        .await?;
        return Ok(output.stdout.trim().to_string());
    }
    branches(root)
        .await?
        .branches
        .into_iter()
        .find(|branch| branch.full_name == value || branch.name == value)
        .map(|branch| branch.full_name)
        .ok_or_else(|| {
            GitOperationError::new(
                "branch start point must be an existing branch or full commit SHA",
            )
        })
}

async fn validate_branch_name(root: &Path, name: &str) -> Result<(), GitOperationError> {
    if name.trim().is_empty() || name.starts_with('-') || name != name.trim() {
        return Err(GitOperationError::new("branch name is invalid"));
    }
    run_git(
        root,
        vec![
            "check-ref-format".to_string(),
            "--branch".to_string(),
            name.to_string(),
        ],
        "git branch validation failed",
    )
    .await?;
    Ok(())
}

fn is_strict_sha(value: &str) -> bool {
    matches!(value.len(), 40 | 64) && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn parse_branches(stdout: &str) -> Vec<WorkspaceGitBranch> {
    stdout
        .lines()
        .filter_map(|record| {
            let fields = record.split('\x1f').collect::<Vec<_>>();
            if fields.len() != 5 {
                return None;
            }
            let full_name = fields[0];
            let name = fields[1];
            let kind = if full_name.starts_with("refs/heads/") {
                "local"
            } else if full_name.starts_with("refs/remotes/") && !name.ends_with("/HEAD") {
                "remote"
            } else {
                return None;
            };
            let (ahead, behind) = parse_track(fields[4]);
            Some(WorkspaceGitBranch {
                name: name.to_string(),
                full_name: full_name.to_string(),
                kind: kind.to_string(),
                current: fields[2] == "*",
                upstream: (!fields[3].is_empty()).then(|| fields[3].to_string()),
                ahead,
                behind,
            })
        })
        .collect()
}

fn parse_track(value: &str) -> (i64, i64) {
    let mut ahead = 0;
    let mut behind = 0;
    for part in value.trim_matches(['[', ']']).split(',') {
        let part = part.trim();
        if let Some(value) = part.strip_prefix("ahead ") {
            ahead = value.parse().unwrap_or(0);
        } else if let Some(value) = part.strip_prefix("behind ") {
            behind = value.parse().unwrap_or(0);
        }
    }
    (ahead, behind)
}

async fn run_git(
    root: &Path,
    args: Vec<String>,
    context: &'static str,
) -> Result<super::runner::GitCommandOutput, GitOperationError> {
    let output = run_git_command(root, &args)
        .await
        .map_err(|err| GitOperationError::new(git_command_error_message(err)))?;
    if output.success {
        Ok(output)
    } else {
        Err(GitOperationError::new(format_git_failure(context, &output)))
    }
}
