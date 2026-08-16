//! The guarded shell tool.
//!
//! Replaces the previous `bash` module, which ran every Windows command through
//! `cmd /C` while advertising itself to the model as `Bash`. Four things changed:
//!
//! * **Dialect honesty.** [`resolve`] picks PowerShell on Windows and exposes the
//!   tool under a name and description matching whatever will actually parse the
//!   command, so the model stops writing `ls -la` for `cmd.exe`.
//! * **Containment that holds.** The redirection guard lexes with the rules of
//!   the resolved dialect ([`lex`]) instead of POSIX rules, closing a bypass
//!   where `> ..\..\evil.txt` read as a workspace-relative path.
//! * **Readable output.** Captured bytes are decoded as UTF-8 with a code-page
//!   fallback ([`decode`]), and truncation keeps the **tail** — the part of a
//!   build log that says what broke — spilling the whole stream to a file.
//! * **Nothing left running.** Every child is spawned into a process tree that a
//!   timeout, a cancellation, or a [`jobs::Job::kill`] can terminate whole.

pub mod decode;
pub mod jobs;
pub mod lex;
pub mod policy;
pub mod resolve;

use std::{
    path::Path,
    process::{Command as StdCommand, Stdio},
    time::Duration,
};

use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use tokio::{io::AsyncReadExt, time::timeout};
use uuid::Uuid;

use crate::process::spawn_process_tree;

use self::{
    decode::decode_output,
    policy::CommandVerdict,
    resolve::{ResolvedShell, ShellDialect},
};

use super::{controlled, ApprovalGrants, ApprovalRequest, ToolError, ToolResult};

/// Default command timeout when the caller does not specify one.
pub const DEFAULT_SHELL_TIMEOUT_SECONDS: u64 = 600;
/// Largest timeout a caller may request.
pub const MAX_SHELL_TIMEOUT_SECONDS: u64 = 3_600;
/// Largest combined stdout+stderr length (in characters) returned to the model.
pub const MAX_SHELL_OUTPUT_CHARS: usize = 12_000;
/// Longest command accepted.
///
/// PowerShell commands are handed over as `-EncodedCommand`, which is bounded by
/// the ~32 KB Windows command-line limit once UTF-16 and base64 have inflated
/// them. A command anywhere near this length should be a script file the shell
/// runs, not a command line, so the limit is reported rather than silently
/// worked around.
pub const MAX_COMMAND_CHARS: usize = 8_000;

/// Workspace-relative directory holding spilled shell output.
pub const SPILL_DIR: &str = ".ag-swarmer/shell";

/// Run `command` in `root` under `shell`.
///
/// With `run_in_background`, the command is registered as a [`jobs::Job`] and the
/// call returns its id immediately; otherwise it runs to completion or to
/// `timeout_seconds`, whichever comes first. A timeout terminates the whole
/// process tree, not just the shell, and still reports whatever was produced.
///
/// `shell` arrives from the caller rather than being read from a process-wide
/// value here: the account's shell preference decides the tool name and dialect
/// guidance the model was given for this turn, and the interpreter that parses
/// the command has to be the same one.
///
/// `grants` holds the policy rules the user has already approved for this
/// thread. A command the policy wants a decision on returns
/// [`ToolStatus::ApprovalRequired`] rather than running — the gate lives here,
/// not only in the runtime, so a caller that forgets to supply grants fails
/// closed instead of silently running unapproved work. Grants carrying
/// [`ApprovalGrants::bypass_all`] skip the review outright.
///
/// `tool_name` is the name the model actually called, which is not always the
/// dialect's own name: every shell alias routes here, so a model that reaches
/// for `Bash` on a PowerShell host would otherwise be shown an approval card
/// naming a tool it never called, next to an activity row naming the one it did.
pub async fn run_shell(
    shell: &ResolvedShell,
    tool_name: &str,
    root: &Path,
    command: &str,
    timeout_seconds: u64,
    run_in_background: bool,
    grants: &ApprovalGrants,
) -> Result<ToolResult, ToolError> {
    if !(1..=MAX_SHELL_TIMEOUT_SECONDS).contains(&timeout_seconds) {
        return Err(ToolError::invalid(format!(
            "timeout_seconds must be between 1 and {MAX_SHELL_TIMEOUT_SECONDS} when provided"
        )));
    }
    if command.chars().count() > MAX_COMMAND_CHARS {
        return Err(ToolError::invalid(format!(
            "command must be at most {MAX_COMMAND_CHARS} characters; write it to a script file \
             with the Write tool and run that file instead"
        )));
    }

    // An agent in unattended mode skips the review entirely, `Deny` rules
    // included. That is the whole point of the mode and it is not a subtle
    // setting: formatting a volume or powering off the host will run. It is
    // reachable only by an owner who switched this agent into it.
    if !grants.bypass_all() {
        match policy::review(command, shell.dialect, root, &|rule| grants.contains(rule)) {
            CommandVerdict::Allow => {}
            CommandVerdict::Deny { reason } => return Err(ToolError::invalid(reason)),
            CommandVerdict::Ask {
                rule,
                capability,
                detail,
            } => {
                return Ok(controlled::approval_required(ApprovalRequest {
                    rule: rule.to_string(),
                    capability: capability.to_string(),
                    reason: detail,
                    tool_name: tool_name.to_string(),
                    subject: command.to_string(),
                }))
            }
        }
    }

    let id = format!("shell_{}", &Uuid::new_v4().simple().to_string()[..12]);
    let spill_path = format!("{SPILL_DIR}/{id}.log");
    let (mut child, tree) = spawn_process_tree(shell_command(shell, command, root))
        .map_err(|_| ToolError::invalid("failed to start command"))?;

    if run_in_background {
        let job = jobs::start(
            id,
            command.to_string(),
            root.to_path_buf(),
            spill_path,
            child,
            tree,
        );
        return Ok(ToolResult::completed(format!(
            "Started in the background.\njob_id={}\nRead new output with ShellOutput \
             (job_id=\"{}\"), stop it with ShellKill. The complete log is written to {}.",
            job.id, job.id, job.spill_path
        )));
    }

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

    let outcome = timeout(Duration::from_secs(timeout_seconds), wait).await;
    let stdout = decode_output(&stdout_buf);
    let stderr = decode_output(&stderr_buf);

    let header = match outcome {
        Ok(status) => {
            let code = match status {
                Ok(status) => status
                    .code()
                    .map(|code| code.to_string())
                    .unwrap_or_else(|| "signal".to_string()),
                Err(_) => "unknown".to_string(),
            };
            format!("exit_code={code}")
        }
        Err(_elapsed) => {
            // The wait future has been dropped, releasing its borrow of `child`,
            // so the tree can be killed. Terminating the tree rather than the
            // child is the point: `pwsh -Command "npm run build"` leaves `node`
            // running otherwise, holding these pipes open.
            tree.terminate();
            let _ = child.start_kill();
            format!(
                "Command timed out after {timeout_seconds}s and its process tree was terminated."
            )
        }
    };

    let mut parts = vec![header];
    if !stdout.trim().is_empty() {
        parts.push(stdout.trim_end().to_string());
    }
    if !stderr.trim().is_empty() {
        parts.push(stderr.trim_end().to_string());
    }
    Ok(ToolResult::completed(
        bound_output(root, &spill_path, &parts.join("\n")).await,
    ))
}

/// Read whatever a background job has produced since the last read.
pub fn read_job_output(root: &Path, job_id: &str) -> Result<ToolResult, ToolError> {
    let job = jobs::registry()
        .get(job_id, root)
        .ok_or_else(|| ToolError::invalid(unknown_job(job_id)))?;
    let read = job.read();

    let mut lines = vec![format!(
        "job_id={} status={} elapsed={}s",
        job.id,
        read.status.label(),
        job.elapsed_seconds()
    )];
    if read.dropped > 0 {
        lines.push(format!(
            "[{} characters were dropped before this read: output outran the reader. The complete \
             log is at {}.]",
            read.dropped, job.spill_path
        ));
    }
    if read.withheld > 0 {
        lines.push(format!(
            "[{} more characters are queued; call ShellOutput again to continue.]",
            read.withheld
        ));
    }
    if read.text.trim().is_empty() {
        lines.push(if read.status.is_running() {
            "[no new output since the last read]".to_string()
        } else {
            "[no further output]".to_string()
        });
    } else {
        lines.push(read.text.trim_end().to_string());
    }
    Ok(ToolResult::completed(lines.join("\n")))
}

/// Terminate a background job and everything it started.
pub fn kill_job(root: &Path, job_id: &str) -> Result<ToolResult, ToolError> {
    let job = jobs::registry()
        .get(job_id, root)
        .ok_or_else(|| ToolError::invalid(unknown_job(job_id)))?;
    if !job.status().is_running() {
        return Ok(ToolResult::completed(format!(
            "job_id={} had already finished ({}).",
            job.id,
            job.status().label()
        )));
    }
    job.kill();
    Ok(ToolResult::completed(format!(
        "job_id={} and its process tree were terminated. Its output so far is at {}.",
        job.id, job.spill_path
    )))
}

/// List the background jobs started in this workspace.
pub fn list_jobs(root: &Path) -> ToolResult {
    let jobs = jobs::registry().list(root);
    if jobs.is_empty() {
        return ToolResult::completed("No background shell jobs have been started.");
    }
    let lines = jobs
        .iter()
        .map(|job| {
            format!(
                "- {} status={} elapsed={}s produced={} chars log={} command={}",
                job.id,
                job.status().label(),
                job.elapsed_seconds(),
                job.total_chars(),
                job.spill_path,
                summarize_command(&job.command),
            )
        })
        .collect::<Vec<_>>();
    ToolResult::completed(lines.join("\n"))
}

fn unknown_job(job_id: &str) -> String {
    format!(
        "no background job '{job_id}' exists in this workspace. List the running jobs with \
         ShellJobs."
    )
}

fn summarize_command(command: &str) -> String {
    let single_line = command.split_whitespace().collect::<Vec<_>>().join(" ");
    if single_line.chars().count() <= 80 {
        return single_line;
    }
    format!("{}…", single_line.chars().take(79).collect::<String>())
}

/// Build the child process for `command` under `shell`, rooted at `root`.
fn shell_command(shell: &ResolvedShell, command: &str, root: &Path) -> StdCommand {
    let mut process = StdCommand::new(&shell.program);
    match shell.dialect {
        ShellDialect::PowerShell => {
            // `-EncodedCommand` carries the script as base64 UTF-16LE, so no
            // quoting layer sits between the model's command and the parser.
            // Windows PowerShell 5.1 does not re-parse `-Command` with
            // `CommandLineToArgvW` rules, and quoting for both hosts at once is
            // not reliably possible; encoding sidesteps the question entirely.
            process
                .arg("-NoLogo")
                .arg("-NoProfile")
                .arg("-NonInteractive")
                .arg("-EncodedCommand")
                .arg(encode_command(&powershell_script(command)));
        }
        ShellDialect::Cmd => {
            // `cmd.exe` does not follow `CommandLineToArgvW` quoting, so the
            // arguments are written to the command line verbatim. `/D` skips
            // AutoRun registry commands; `chcp 65001` makes the console emit
            // UTF-8 rather than the host's OEM code page.
            #[cfg(windows)]
            {
                use std::os::windows::process::CommandExt;
                process.raw_arg("/D");
                process.raw_arg("/C");
                process.raw_arg(format!("chcp 65001>nul & {command}"));
            }
            #[cfg(not(windows))]
            {
                process.arg("/D").arg("/C").arg(command);
            }
        }
        ShellDialect::Posix => {
            process.arg("-c").arg(command);
        }
    }
    process
        .current_dir(root)
        // No stdin: an agent cannot answer an interactive prompt, and an
        // immediate EOF makes a program that asks fail fast instead of blocking
        // until the timeout. Commands needing input use a heredoc or a pipe.
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    process
}

/// Wrap `command` with the preamble and exit-code trailer PowerShell needs.
///
/// Three problems are handled here, all of which reached the model as corrupted
/// or ambiguous output:
///
/// * **Output encoding.** `[Console]::OutputEncoding` decides how PowerShell
///   interprets a native program's bytes and how its own output is written.
/// * **Input encoding.** Windows PowerShell 5.1 reads files with the host's ANSI
///   code page, so `Get-Content` on a UTF-8 file returns mojibake on a
///   Simplified-Chinese host — `第一行` comes back as `绗竴琛`. PowerShell 7
///   already defaults to UTF-8, so the blanket default is only widened there,
///   where `utf8` also means *without* a BOM. Setting it on 5.1 would start
///   writing BOMs into source files.
/// * **Exit codes.** PowerShell leaves `$LASTEXITCODE` untouched when the last
///   statement was a cmdlet, so a stale value from an earlier native command
///   would be reported. It is cleared first, and `$?` decides the code when no
///   native program ran.
///
/// The progress renderer and ANSI styling are disabled too; both otherwise reach
/// the model as escape-sequence noise.
fn powershell_script(command: &str) -> String {
    format!(
        "$ErrorActionPreference = 'Continue'\n\
         $ProgressPreference = 'SilentlyContinue'\n\
         try {{ [Console]::OutputEncoding = [Text.UTF8Encoding]::new($false); \
         $OutputEncoding = [Console]::OutputEncoding }} catch {{ }}\n\
         $PSDefaultParameterValues['Get-Content:Encoding'] = 'utf8'\n\
         $PSDefaultParameterValues['Select-String:Encoding'] = 'utf8'\n\
         $PSDefaultParameterValues['Import-Csv:Encoding'] = 'utf8'\n\
         if ($PSVersionTable.PSVersion.Major -ge 6) \
         {{ $PSDefaultParameterValues['*:Encoding'] = 'utf8' }}\n\
         if (Get-Variable -Name PSStyle -ErrorAction SilentlyContinue) \
         {{ $PSStyle.OutputRendering = 'PlainText' }}\n\
         $global:LASTEXITCODE = $null\n\
         {command}\n\
         if ($null -ne $LASTEXITCODE) {{ exit $LASTEXITCODE }}\n\
         if ($?) {{ exit 0 }} else {{ exit 1 }}\n"
    )
}

/// Encode a PowerShell script as base64 UTF-16LE for `-EncodedCommand`.
fn encode_command(script: &str) -> String {
    let utf16: Vec<u8> = script
        .encode_utf16()
        .flat_map(|unit| unit.to_le_bytes())
        .collect();
    BASE64.encode(utf16)
}

/// Cap `output` at [`MAX_SHELL_OUTPUT_CHARS`], keeping the **tail**.
///
/// The head was the wrong half to keep. A failing build prints its configuration
/// banner first and its error summary last, so head-truncation returned the
/// least informative 12 000 characters and discarded the reason. The complete
/// text is written to a workspace-relative spill file the model can `Read`, and
/// the marker leads the output so a later truncation cannot hide it.
async fn bound_output(root: &Path, spill_path: &str, output: &str) -> String {
    let total = output.chars().count();
    if total <= MAX_SHELL_OUTPUT_CHARS {
        return output.to_string();
    }
    let tail: String = output
        .chars()
        .skip(total - MAX_SHELL_OUTPUT_CHARS)
        .collect();
    let spilled = write_spill(root, spill_path, output).await;
    let location = if spilled {
        format!("the complete output is at {spill_path}")
    } else {
        "the complete output could not be written to disk".to_string()
    };
    format!(
        "[output truncated: kept the last {MAX_SHELL_OUTPUT_CHARS} of {total} characters; \
         {location}]\n{tail}"
    )
}

/// Write the full output beside the workspace, best effort.
async fn write_spill(root: &Path, spill_path: &str, output: &str) -> bool {
    let path = root.join(spill_path);
    if let Some(parent) = path.parent() {
        if tokio::fs::create_dir_all(parent).await.is_err() {
            return false;
        }
    }
    tokio::fs::write(&path, output).await.is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::ToolStatus;

    #[test]
    fn powershell_scripts_are_encoded_as_utf16_base64() {
        let encoded = encode_command("echo 世界");
        let decoded = BASE64.decode(encoded).unwrap();
        let units: Vec<u16> = decoded
            .chunks_exact(2)
            .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
            .collect();
        assert_eq!(String::from_utf16(&units).unwrap(), "echo 世界");
    }

    #[test]
    fn the_powershell_wrapper_forces_utf8_and_a_defined_exit_code() {
        let script = powershell_script("cargo build");
        assert!(script.contains("[Console]::OutputEncoding"));
        // Windows PowerShell 5.1 reads files as ANSI, so the read cmdlets are
        // pinned to UTF-8 there; the blanket default is only safe on 7+, where
        // `utf8` means "no BOM".
        assert!(script.contains("$PSDefaultParameterValues['Get-Content:Encoding'] = 'utf8'"));
        assert!(script.contains("PSVersion.Major -ge 6"));
        assert!(script.contains("$global:LASTEXITCODE = $null"));
        assert!(script.contains("cargo build"));
        assert!(script
            .trim_end()
            .ends_with("if ($?) { exit 0 } else { exit 1 }"));
    }

    #[tokio::test]
    async fn truncation_keeps_the_tail_and_spills_the_rest() {
        let root = tempfile::tempdir().unwrap();
        let output: String = (0..MAX_SHELL_OUTPUT_CHARS + 500)
            .map(|index| if index < 500 { 'H' } else { 'T' })
            .collect();
        let spill = format!("{SPILL_DIR}/probe.log");
        let bounded = bound_output(root.path(), &spill, &output).await;

        assert!(bounded.starts_with("[output truncated"), "{bounded}");
        assert!(
            !bounded.contains('H'),
            "the discarded head must not survive truncation"
        );
        assert_eq!(
            bounded.lines().last().unwrap().chars().count(),
            MAX_SHELL_OUTPUT_CHARS,
            "the retained tail should be exactly the cap"
        );
        assert_eq!(
            std::fs::read_to_string(root.path().join(&spill)).unwrap(),
            output,
            "the spill file should hold the complete output"
        );
    }

    #[tokio::test]
    async fn output_within_the_cap_is_returned_verbatim_without_a_spill() {
        let root = tempfile::tempdir().unwrap();
        let spill = format!("{SPILL_DIR}/probe.log");
        assert_eq!(bound_output(root.path(), &spill, "small").await, "small");
        assert!(!root.path().join(SPILL_DIR).exists());
    }

    #[test]
    fn long_commands_are_refused_with_an_actionable_message() {
        let root = tempfile::tempdir().unwrap();
        let command = "a".repeat(MAX_COMMAND_CHARS + 1);
        let error = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(run_shell(
                resolve::process_shell(),
                "Shell",
                root.path(),
                &command,
                5,
                false,
                &ApprovalGrants::default(),
            ))
            .unwrap_err();
        assert!(
            error.model_safe_message().contains("script file"),
            "{}",
            error.model_safe_message()
        );
    }

    /// Run a real deletion of `victim.txt` inside a throwaway root and report
    /// what the tool did about it. Bare `rm <file>` is the one destructive form
    /// both a POSIX shell and PowerShell (where `rm` aliases `Remove-Item`)
    /// spell the same way, so the test exercises the host's actual interpreter.
    async fn delete_a_file(grants: &ApprovalGrants) -> (ToolResult, bool) {
        let root = tempfile::tempdir().unwrap();
        let victim = root.path().join("victim.txt");
        std::fs::write(&victim, "keep me").unwrap();

        let result = run_shell(
            resolve::process_shell(),
            "Shell",
            root.path(),
            "rm victim.txt",
            30,
            false,
            grants,
        )
        .await
        .unwrap();

        (result, victim.exists())
    }

    #[tokio::test]
    async fn without_a_bypass_a_destructive_command_pauses_instead_of_running() {
        let (result, survived) = delete_a_file(&ApprovalGrants::default()).await;

        assert_eq!(result.status, ToolStatus::ApprovalRequired);
        assert!(survived, "the file must still be there while we wait");
    }

    #[tokio::test]
    async fn the_card_names_the_call_the_model_made_not_the_hosts_dialect() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("victim.txt"), "keep me").unwrap();

        // Every shell alias routes here, so a model can reach a PowerShell host
        // through `Bash`. Naming the dialect instead of the call would put one
        // tool on the approval card and another on the activity row beside it,
        // for the same command.
        let result = run_shell(
            resolve::process_shell(),
            "Bash",
            root.path(),
            "rm victim.txt",
            30,
            false,
            &ApprovalGrants::default(),
        )
        .await
        .unwrap();

        assert_eq!(result.status, ToolStatus::ApprovalRequired);
        let payload: serde_json::Value = serde_json::from_str(&result.output).unwrap();
        assert_eq!(payload["approval_request"]["tool_name"], "Bash");
        assert!(root.path().join("victim.txt").exists());
    }

    #[tokio::test]
    async fn a_bypass_runs_the_destructive_command_without_asking() {
        let mut grants = ApprovalGrants::default();
        grants.set_bypass_all(true);

        let (result, survived) = delete_a_file(&grants).await;

        assert_ne!(
            result.status,
            ToolStatus::ApprovalRequired,
            "unattended mode asks nobody: {}",
            result.output
        );
        assert!(!survived, "the deletion should have happened for real");
    }

    #[test]
    fn a_bypass_also_stops_the_policy_refusing_outright() {
        let root = tempfile::tempdir().unwrap();
        let mut grants = ApprovalGrants::default();
        grants.set_bypass_all(true);
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();

        // `dd of=` is a `Deny` rule — normally refused with no approval offered.
        // Unattended mode is exactly as blunt as it sounds, so the command is
        // handed to the shell instead. `dd` is absent on the hosts this runs on,
        // which is what makes the assertion safe to write: reaching the shell at
        // all is the whole finding.
        let denied = runtime
            .block_on(run_shell(
                resolve::process_shell(),
                "Shell",
                root.path(),
                "dd if=/dev/zero of=probe.bin count=0",
                30,
                false,
                &ApprovalGrants::default(),
            ))
            .unwrap_err();
        assert!(
            denied.model_safe_message().contains("blocked"),
            "{}",
            denied.model_safe_message()
        );

        let bypassed = runtime
            .block_on(run_shell(
                resolve::process_shell(),
                "Shell",
                root.path(),
                "dd if=/dev/zero of=probe.bin count=0",
                30,
                false,
                &grants,
            ))
            .expect("the policy no longer refuses");
        assert_ne!(bypassed.status, ToolStatus::ApprovalRequired);
        assert!(
            bypassed.output.contains("exit_code="),
            "the command reached the shell: {}",
            bypassed.output
        );
    }
}
