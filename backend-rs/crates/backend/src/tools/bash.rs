//! Guarded `Bash` tool.
//!
//! [`run_bash`] runs a shell command in the workspace root with a conservative
//! safety envelope: destructive commands are blocked, shell redirection targets
//! must stay inside the workspace, the command runs with a bounded timeout, and
//! the combined output is capped. Execution uses `tokio::process` so it never
//! blocks the async executor.

use std::{path::Path, process::Stdio, sync::OnceLock, time::Duration};

use regex::Regex;
use tokio::{io::AsyncReadExt, process::Command, time::timeout};

use super::{resolve_workspace_path, ToolError, ToolResult};

/// Default command timeout when the caller does not specify one.
pub const DEFAULT_BASH_TIMEOUT_SECONDS: u64 = 600;
/// Largest timeout a caller may request.
pub const MAX_BASH_TIMEOUT_SECONDS: u64 = 3_600;
/// Largest combined stdout+stderr length (in characters) returned to the model.
pub const MAX_BASH_OUTPUT_CHARS: usize = 12_000;

/// Compiled destructive-command patterns. Matched against the lowercased
/// command so they catch case variants.
fn destructive_patterns() -> &'static [Regex] {
    static PATTERNS: OnceLock<Vec<Regex>> = OnceLock::new();
    PATTERNS.get_or_init(|| {
        [
            r"(^|[;&|])\s*(?:sudo\s+|command\s+|builtin\s+|env\s+)*(?:[\w./-]*[/\\])?(rm|del|rmdir|format|shutdown|erase|rd)\b",
            r"\b(powershell|pwsh)\b[^\n]*(remove-item|clear-content|stop-computer)\b",
            r"\bgit\s+reset\s+--hard\b",
            r"\bgit\s+clean\b",
            r"\bgit\s+push\b[^\n]*\s--force(?:\b|-with-lease\b)",
        ]
        .iter()
        .map(|pattern| Regex::new(pattern).expect("static destructive command pattern must compile"))
        .collect()
    })
}

/// Redirection operators whose target file must stay inside the workspace.
const REDIRECT_OPERATORS: [&str; 6] = [">", ">>", "1>", "1>>", "2>", "2>>"];

/// Reject a command that is empty, destructive, or redirects output outside the
/// workspace root.
fn guard_command(command: &str, root: &Path) -> Result<(), ToolError> {
    if command.trim().is_empty() {
        return Err(ToolError::invalid("command must be non-empty"));
    }

    let lowered = command.to_lowercase();
    for pattern in destructive_patterns() {
        if pattern.is_match(&lowered) {
            return Err(ToolError::invalid(
                "command is blocked by workspace safety policy",
            ));
        }
    }

    // Split shell words so a redirection target written as a separate token
    // (`> out`) or attached (`2>out`) is validated against the workspace root.
    let tokens = shlex::split(command)
        .ok_or_else(|| ToolError::invalid("command could not be parsed safely"))?;
    for (index, token) in tokens.iter().enumerate() {
        if REDIRECT_OPERATORS.contains(&token.as_str()) {
            if let Some(target) = tokens.get(index + 1) {
                resolve_workspace_path(root, target)?;
            }
        } else if REDIRECT_OPERATORS
            .iter()
            .any(|operator| token.starts_with(operator))
        {
            let target = token.trim_start_matches(|c: char| c.is_ascii_digit() || c == '>');
            if !target.is_empty() {
                resolve_workspace_path(root, target)?;
            }
        }
    }

    Ok(())
}

/// Run `command` in `root` with a bounded timeout and bounded output.
///
/// `timeout_seconds` must be in `1..=MAX_BASH_TIMEOUT_SECONDS`. On success the
/// output is `exit_code=<n>` followed by the trimmed stdout and stderr; on
/// timeout it reports the elapsed limit plus any partial output. Both forms are
/// truncated to [`MAX_BASH_OUTPUT_CHARS`] characters.
pub async fn run_bash(
    root: &Path,
    command: &str,
    timeout_seconds: u64,
) -> Result<ToolResult, ToolError> {
    if !(1..=MAX_BASH_TIMEOUT_SECONDS).contains(&timeout_seconds) {
        return Err(ToolError::invalid(format!(
            "timeout_seconds must be between 1 and {MAX_BASH_TIMEOUT_SECONDS} when provided"
        )));
    }
    guard_command(command, root)?;

    #[cfg(windows)]
    let (shell, flag) = ("cmd", "/C");
    #[cfg(not(windows))]
    let (shell, flag) = ("sh", "-c");

    let mut child = Command::new(shell)
        .arg(flag)
        .arg(command)
        .current_dir(root)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|_| ToolError::invalid("failed to start command"))?;

    let mut stdout_handle = child.stdout.take().expect("stdout was piped");
    let mut stderr_handle = child.stderr.take().expect("stderr was piped");
    let mut stdout_buf = Vec::new();
    let mut stderr_buf = Vec::new();

    // Read both pipes while waiting so a chatty command cannot deadlock on a
    // full pipe buffer. The buffers retain whatever was read if the wait is
    // cancelled by the timeout below.
    let wait = async {
        let (_out, _err, status) = tokio::join!(
            stdout_handle.read_to_end(&mut stdout_buf),
            stderr_handle.read_to_end(&mut stderr_buf),
            child.wait(),
        );
        status
    };

    match timeout(Duration::from_secs(timeout_seconds), wait).await {
        Ok(status) => {
            let code = match status {
                Ok(status) => status
                    .code()
                    .map(|code| code.to_string())
                    .unwrap_or_else(|| "signal".to_string()),
                Err(_) => "unknown".to_string(),
            };
            let stdout = String::from_utf8_lossy(&stdout_buf);
            let stderr = String::from_utf8_lossy(&stderr_buf);
            let mut parts = vec![format!("exit_code={code}")];
            let trimmed_out = stdout.trim();
            if !trimmed_out.is_empty() {
                parts.push(trimmed_out.to_string());
            }
            let trimmed_err = stderr.trim();
            if !trimmed_err.is_empty() {
                parts.push(trimmed_err.to_string());
            }
            Ok(ToolResult::completed(truncate_output(&parts.join("\n"))))
        }
        Err(_elapsed) => {
            // The wait future is dropped here, releasing its borrow of `child`,
            // so the process can be killed and the partial buffers reported.
            let _ = child.start_kill();
            let stdout = String::from_utf8_lossy(&stdout_buf);
            let stderr = String::from_utf8_lossy(&stderr_buf);
            let combined = format!("{}{}", stdout.trim(), stderr.trim());
            let mut summary = format!("Command timed out after {timeout_seconds}s.");
            if !combined.trim().is_empty() {
                summary = format!("{summary}\n{}", combined.trim());
            }
            Ok(ToolResult::completed(truncate_output(&summary)))
        }
    }
}

/// Cap `output` to [`MAX_BASH_OUTPUT_CHARS`] characters, appending a marker when
/// it was shortened. Counts characters (not bytes) so a multibyte boundary is
/// never split.
fn truncate_output(output: &str) -> String {
    if output.chars().count() <= MAX_BASH_OUTPUT_CHARS {
        return output.to_string();
    }
    let truncated: String = output.chars().take(MAX_BASH_OUTPUT_CHARS).collect();
    format!("{truncated}\n[output truncated]")
}
