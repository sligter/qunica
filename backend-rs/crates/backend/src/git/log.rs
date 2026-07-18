use std::path::Path;

use serde::Serialize;

use super::{
    diff,
    ops::GitOperationError,
    runner::{
        format_git_failure, git_command_error_message, run_git_command,
        run_git_command_with_output_limit,
    },
    WorkspaceGitDiff,
};

const MAX_LOG_LIMIT: usize = 100;
const MAX_LOG_SKIP: usize = 10_000;
const MAX_LOG_OUTPUT_CHARS: usize = 64_000;

#[derive(Debug, Serialize)]
pub struct WorkspaceGitCommitSummary {
    pub sha: String,
    pub short_sha: String,
    pub subject: String,
    pub author_name: String,
    pub author_email: String,
    pub author_date: String,
    pub local_only: bool,
}

#[derive(Debug, Serialize)]
pub struct WorkspaceGitLog {
    pub commits: Vec<WorkspaceGitCommitSummary>,
    pub has_more: bool,
}

#[derive(Debug, Serialize)]
pub struct WorkspaceGitCommitFile {
    pub path: String,
    pub old_path: Option<String>,
    pub status: String,
}

#[derive(Debug, Serialize)]
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

pub async fn log(
    root: &Path,
    limit: usize,
    skip: usize,
) -> Result<WorkspaceGitLog, GitOperationError> {
    let limit = limit.clamp(1, MAX_LOG_LIMIT);
    let skip = skip.min(MAX_LOG_SKIP);
    let output = run_git_with_limit(root, log_args(limit + 1, skip), "git log failed").await?;
    let mut commits = parse_summaries(&output.stdout)?;
    let has_more = commits.len() > limit;
    commits.truncate(limit);

    let upstream = upstream(root).await;
    for commit in &mut commits {
        commit.local_only = is_local_only(root, &commit.sha, upstream.as_deref()).await;
    }

    Ok(WorkspaceGitLog { commits, has_more })
}

pub async fn commit_details(
    root: &Path,
    sha: &str,
) -> Result<WorkspaceGitCommitDetails, GitOperationError> {
    let sha = resolve_commit(root, sha).await?;
    let metadata = run_git(
        root,
        vec![
            "show".to_string(),
            "--no-patch".to_string(),
            "--format=%H%x00%h%x00%s%x00%b%x00%an%x00%ae%x00%aI%x00".to_string(),
            sha.clone(),
        ],
        "git show failed",
    )
    .await?;
    let fields = metadata.stdout.split('\0').collect::<Vec<_>>();
    if fields.len() < 7 {
        return Err(GitOperationError::new(
            "git show returned an invalid commit record",
        ));
    }

    let files_output = run_git(
        root,
        vec![
            "show".to_string(),
            "--format=".to_string(),
            "--name-status".to_string(),
            "-z".to_string(),
            sha.clone(),
        ],
        "git show files failed",
    )
    .await?;
    let numstat_output = run_git(
        root,
        vec![
            "show".to_string(),
            "--format=".to_string(),
            "--numstat".to_string(),
            "-z".to_string(),
            sha.clone(),
        ],
        "git show statistics failed",
    )
    .await?;
    let stat = run_git(
        root,
        vec![
            "show".to_string(),
            "--format=".to_string(),
            "--stat".to_string(),
            sha,
        ],
        "git show stat failed",
    )
    .await?;
    let (insertions, deletions) = parse_numstat(&numstat_output.stdout);

    Ok(WorkspaceGitCommitDetails {
        sha: fields[0].to_string(),
        short_sha: fields[1].to_string(),
        subject: fields[2].to_string(),
        body: fields[3].trim_end().to_string(),
        author_name: fields[4].to_string(),
        author_email: fields[5].to_string(),
        author_date: fields[6].to_string(),
        files: parse_files(&files_output.stdout),
        insertions,
        deletions,
        stat: stat.stdout,
    })
}

pub async fn commit_diff(
    root: &Path,
    sha: &str,
    path: Option<&str>,
) -> Result<WorkspaceGitDiff, GitOperationError> {
    let sha = resolve_commit(root, sha).await?;
    diff::commit_diff(root, &sha, path).await
}

pub async fn create_branch_from_commit(
    root: &Path,
    sha: &str,
    branch: &str,
) -> Result<(), GitOperationError> {
    let sha = resolve_commit(root, sha).await?;
    validate_branch(root, branch).await?;
    run_git(
        root,
        vec!["branch".to_string(), branch.to_string(), sha],
        "git create branch failed",
    )
    .await?;
    Ok(())
}

pub fn pagination_is_valid(limit: usize, skip: usize) -> bool {
    (1..=MAX_LOG_LIMIT).contains(&limit) && skip <= MAX_LOG_SKIP
}

async fn resolve_commit(root: &Path, sha: &str) -> Result<String, GitOperationError> {
    if !is_strict_sha(sha) {
        return Err(GitOperationError::new(
            "commit sha must be a full hexadecimal object id",
        ));
    }
    let output = run_git(
        root,
        vec![
            "rev-parse".to_string(),
            "--verify".to_string(),
            format!("{sha}^{{commit}}"),
        ],
        "git commit lookup failed",
    )
    .await?;
    Ok(output.stdout.trim().to_string())
}

fn is_strict_sha(sha: &str) -> bool {
    matches!(sha.len(), 40 | 64) && sha.bytes().all(|byte| byte.is_ascii_hexdigit())
}

async fn validate_branch(root: &Path, branch: &str) -> Result<(), GitOperationError> {
    if branch.is_empty() || branch.starts_with('-') {
        return Err(GitOperationError::new("branch name is invalid"));
    }
    run_git(
        root,
        vec![
            "check-ref-format".to_string(),
            "--branch".to_string(),
            branch.to_string(),
        ],
        "git branch validation failed",
    )
    .await?;
    Ok(())
}

async fn upstream(root: &Path) -> Option<String> {
    let output = run_git_command(
        root,
        &[
            "rev-parse".to_string(),
            "--verify".to_string(),
            "@{upstream}".to_string(),
        ],
    )
    .await
    .ok()?;
    output.success.then(|| output.stdout.trim().to_string())
}

async fn is_local_only(root: &Path, sha: &str, upstream: Option<&str>) -> bool {
    let Some(upstream) = upstream else {
        return false;
    };
    match run_git_command(
        root,
        &[
            "merge-base".to_string(),
            "--is-ancestor".to_string(),
            sha.to_string(),
            upstream.to_string(),
        ],
    )
    .await
    {
        Ok(output) => !output.success,
        Err(_) => false,
    }
}

fn log_args(limit: usize, skip: usize) -> Vec<String> {
    vec![
        "log".to_string(),
        format!("--skip={skip}"),
        "-n".to_string(),
        limit.to_string(),
        "--format=%H%x00%h%x00%s%x00%an%x00%ae%x00%aI%x00".to_string(),
    ]
}

fn parse_summaries(stdout: &str) -> Result<Vec<WorkspaceGitCommitSummary>, GitOperationError> {
    let fields = stdout
        .trim_end_matches(['\r', '\n'])
        .split('\0')
        .filter(|field| !field.is_empty())
        .collect::<Vec<_>>();
    if fields.len() % 6 != 0 {
        return Err(GitOperationError::new(
            "git log returned an invalid commit record",
        ));
    }
    Ok(fields
        .chunks_exact(6)
        .map(|fields| WorkspaceGitCommitSummary {
            sha: fields[0].to_string(),
            short_sha: fields[1].to_string(),
            subject: fields[2].to_string(),
            author_name: fields[3].to_string(),
            author_email: fields[4].to_string(),
            author_date: fields[5].to_string(),
            local_only: false,
        })
        .collect())
}

fn parse_files(stdout: &str) -> Vec<WorkspaceGitCommitFile> {
    let fields = stdout
        .split('\0')
        .filter(|field| !field.is_empty())
        .collect::<Vec<_>>();
    let mut files = Vec::new();
    let mut index = 0;
    while let Some(status) = fields.get(index) {
        index += 1;
        let is_rename = status.starts_with('R') || status.starts_with('C');
        let Some(first_path) = fields.get(index) else {
            break;
        };
        index += 1;
        let (path, old_path) = if is_rename {
            let Some(path) = fields.get(index) else { break };
            index += 1;
            ((*path).to_string(), Some((*first_path).to_string()))
        } else {
            ((*first_path).to_string(), None)
        };
        files.push(WorkspaceGitCommitFile {
            path,
            old_path,
            status: (*status).to_string(),
        });
    }
    files
}

fn parse_numstat(stdout: &str) -> (usize, usize) {
    stdout
        .split('\0')
        .fold((0, 0), |(insertions, deletions), record| {
            let mut fields = record.split('\t');
            let additions = fields
                .next()
                .and_then(|value| value.parse::<usize>().ok())
                .unwrap_or(0);
            let removals = fields
                .next()
                .and_then(|value| value.parse::<usize>().ok())
                .unwrap_or(0);
            (insertions + additions, deletions + removals)
        })
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

async fn run_git_with_limit(
    root: &Path,
    args: Vec<String>,
    context: &'static str,
) -> Result<super::runner::GitCommandOutput, GitOperationError> {
    let output = run_git_command_with_output_limit(root, &args, MAX_LOG_OUTPUT_CHARS)
        .await
        .map_err(|err| GitOperationError::new(git_command_error_message(err)))?;
    if output.success {
        Ok(output)
    } else {
        Err(GitOperationError::new(format_git_failure(context, &output)))
    }
}
