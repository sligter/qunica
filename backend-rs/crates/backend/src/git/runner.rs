use std::{
    io,
    path::Path,
    process::{Command as StdCommand, Stdio},
    time::Duration,
};

use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    process::Command,
    time::timeout,
};

use crate::process::tokio_command_no_window;

const MAX_GIT_OUTPUT_CHARS: usize = 8_000;
const GIT_COMMAND_TIMEOUT_SECONDS: u64 = 120;

#[derive(Debug)]
pub(super) struct GitCommandOutput {
    pub(super) success: bool,
    pub(super) stdout: String,
    pub(super) stderr: String,
}

#[derive(Debug)]
pub(super) enum GitCommandError {
    MissingGit,
    TimedOut,
    Io(&'static str),
}

pub(super) async fn run_git_command(
    root: &Path,
    args: &[String],
) -> Result<GitCommandOutput, GitCommandError> {
    run_git_command_with_output_limit(root, args, MAX_GIT_OUTPUT_CHARS).await
}

pub(super) async fn run_git_command_with_output_limit(
    root: &Path,
    args: &[String],
    max_output_chars: usize,
) -> Result<GitCommandOutput, GitCommandError> {
    run_git_command_inner(root, args, None, max_output_chars).await
}

pub(super) async fn run_git_command_with_input(
    root: &Path,
    args: &[String],
    input: &[u8],
    max_output_chars: usize,
) -> Result<GitCommandOutput, GitCommandError> {
    run_git_command_inner(root, args, Some(input), max_output_chars).await
}

async fn run_git_command_inner(
    root: &Path,
    args: &[String],
    input: Option<&[u8]>,
    max_output_chars: usize,
) -> Result<GitCommandOutput, GitCommandError> {
    let mut child = git_command(root, args, input.is_some())
        .spawn()
        .map_err(|err| {
            if err.kind() == io::ErrorKind::NotFound {
                GitCommandError::MissingGit
            } else {
                GitCommandError::Io("failed to start git command")
            }
        })?;

    let stdin_handle = child.stdin.take();
    let mut stdout_handle = child.stdout.take().expect("stdout was piped");
    let mut stderr_handle = child.stderr.take().expect("stderr was piped");
    let mut stdout_buf = Vec::new();
    let mut stderr_buf = Vec::new();
    let wait = async {
        let write_stdin = async {
            if let (Some(mut stdin), Some(input)) = (stdin_handle, input) {
                stdin.write_all(input).await?;
                stdin.shutdown().await?;
            }
            Ok::<(), io::Error>(())
        };
        let (stdin_result, stdout_result, stderr_result, status_result) = tokio::join!(
            write_stdin,
            stdout_handle.read_to_end(&mut stdout_buf),
            stderr_handle.read_to_end(&mut stderr_buf),
            child.wait(),
        );
        stdin_result.map_err(|_| GitCommandError::Io("failed to write git stdin"))?;
        stdout_result.map_err(|_| GitCommandError::Io("failed to read git stdout"))?;
        stderr_result.map_err(|_| GitCommandError::Io("failed to read git stderr"))?;
        status_result.map_err(|_| GitCommandError::Io("failed to wait for git command"))
    };

    match timeout(Duration::from_secs(GIT_COMMAND_TIMEOUT_SECONDS), wait).await {
        Ok(status) => {
            let status = status?;
            Ok(GitCommandOutput {
                success: status.success(),
                stdout: truncate_git_output(
                    &String::from_utf8_lossy(&stdout_buf),
                    max_output_chars,
                ),
                stderr: truncate_git_output(
                    &String::from_utf8_lossy(&stderr_buf),
                    max_output_chars,
                ),
            })
        }
        Err(_) => {
            let _ = child.start_kill();
            Err(GitCommandError::TimedOut)
        }
    }
}

fn git_command(root: &Path, args: &[String], pipe_stdin: bool) -> Command {
    let mut command = StdCommand::new("git");
    command
        .args(args)
        .current_dir(root)
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdin(if pipe_stdin {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut command = tokio_command_no_window(command);
    command.kill_on_drop(true);
    command
}

pub(super) fn git_command_error_message(err: GitCommandError) -> String {
    match err {
        GitCommandError::MissingGit => "git executable was not found".to_string(),
        GitCommandError::TimedOut => {
            format!("git command timed out after {GIT_COMMAND_TIMEOUT_SECONDS} seconds")
        }
        GitCommandError::Io(message) => message.to_string(),
    }
}

pub(super) fn git_output_is_not_repository(output: &GitCommandOutput) -> bool {
    let combined = format!("{}\n{}", output.stdout, output.stderr).to_lowercase();
    combined.contains("not a git repository")
}

pub(super) fn format_git_failure(context: &str, output: &GitCommandOutput) -> String {
    let details = [output.stderr.trim(), output.stdout.trim()]
        .into_iter()
        .find(|part| !part.is_empty())
        .unwrap_or("command exited with a non-zero status");
    format!("{context}: {details}")
}

fn truncate_git_output(output: &str, max_output_chars: usize) -> String {
    if output.chars().count() <= max_output_chars {
        return output.to_string();
    }
    let truncated: String = output.chars().take(max_output_chars).collect();
    format!("{truncated}\n[output truncated]")
}
