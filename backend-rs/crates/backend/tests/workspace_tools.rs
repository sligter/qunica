//! Workspace tool safety and behavior tests.
//!
//! These exercise the path-safety resolver and the `Read`/`Write`/`Edit`/
//! `Glob`/`Grep` file tools directly against a temporary workspace. No network
//! or database is involved. Every test name contains `workspace_tools` so the
//! `--workspace workspace_tools` filter selects them.

use std::path::Path;

use qunica_backend::tools::{
    resolve_workspace_path, ApprovalGrants, FileEdit, ToolExecutor, ToolStatus, WorkspaceMount,
    WorkspaceTools, MAX_READ_LINES, SELF_MOUNT_NAME,
};
use serde_json::{json, Value};
use tempfile::tempdir;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

// ---------------------------------------------------------------------------
// Path-safety rejection (no filesystem traversal needed beyond an empty root)
// ---------------------------------------------------------------------------

#[test]
fn workspace_tools_rejects_parent_directory_segments() {
    let root = tempdir().unwrap();
    for unsafe_path in ["..", "../escape.txt", "a/../../b", "sub\\..\\..\\b"] {
        let result = resolve_workspace_path(root.path(), unsafe_path);
        assert!(
            result.is_err(),
            "expected `{unsafe_path}` to be rejected as a parent-escape path"
        );
    }
}

#[test]
fn workspace_tools_rejects_windows_drive_paths() {
    let root = tempdir().unwrap();
    // Drive prefixes must be rejected on every host, not just Windows.
    for unsafe_path in ["C:\\tmp\\x", "c:foo", "D:/data/file.txt"] {
        let result = resolve_workspace_path(root.path(), unsafe_path);
        assert!(
            result.is_err(),
            "expected drive path `{unsafe_path}` to be rejected"
        );
    }
}

#[test]
fn workspace_tools_rejects_unc_paths() {
    let root = tempdir().unwrap();
    // UNC and leading-separator (absolute) paths must be rejected everywhere.
    for unsafe_path in [
        "\\\\server\\share\\x",
        "//server/share/x",
        "/etc/passwd",
        "\\windows\\system32",
    ] {
        let result = resolve_workspace_path(root.path(), unsafe_path);
        assert!(
            result.is_err(),
            "expected UNC/absolute path `{unsafe_path}` to be rejected"
        );
    }
}

#[test]
fn workspace_tools_rejects_empty_and_home_paths() {
    let root = tempdir().unwrap();
    for unsafe_path in ["", "   ", "~", "~/secrets"] {
        let result = resolve_workspace_path(root.path(), unsafe_path);
        assert!(result.is_err(), "expected `{unsafe_path:?}` to be rejected");
    }
}

#[test]
fn workspace_tools_rejects_missing_root() {
    // A non-existent root cannot be canonicalized and must be rejected.
    let result = resolve_workspace_path(Path::new("/this/path/does/not/exist/anywhere"), "a.txt");
    assert!(result.is_err());
}

// ---------------------------------------------------------------------------
// Symlink escape
// ---------------------------------------------------------------------------

#[cfg(unix)]
fn make_dir_symlink(target: &Path, link: &Path) -> std::io::Result<()> {
    std::os::unix::fs::symlink(target, link)
}

#[cfg(windows)]
fn make_dir_symlink(target: &Path, link: &Path) -> std::io::Result<()> {
    std::os::windows::fs::symlink_dir(target, link)
}

#[tokio::test]
async fn workspace_tools_rejects_symlink_escape_outside_workspace() {
    let base = tempdir().unwrap();
    let outside = base.path().join("outside");
    let workspace = base.path().join("workspace");
    std::fs::create_dir(&outside).unwrap();
    std::fs::create_dir(&workspace).unwrap();
    std::fs::write(outside.join("secret.txt"), "top secret").unwrap();

    // A symlink inside the workspace pointing at the sibling `outside` dir.
    let link = workspace.join("escape");
    if let Err(err) = make_dir_symlink(&outside, &link) {
        eprintln!(
            "skipping symlink-escape assertion: symlink creation unavailable on this host ({err})"
        );
        return;
    }

    // The resolver must refuse to resolve through the escaping symlink.
    assert!(
        resolve_workspace_path(&workspace, "escape").is_err(),
        "symlink pointing outside the workspace must be rejected"
    );
    assert!(
        resolve_workspace_path(&workspace, "escape/secret.txt").is_err(),
        "a file reached through an escaping symlink must be rejected"
    );

    // And the file tools must refuse to read through it.
    let tools = WorkspaceTools::new(&workspace).unwrap();
    assert!(tools.read("escape/secret.txt", 1, MAX_READ_LINES).is_err());
}

// ---------------------------------------------------------------------------
// Read / Write / Edit / Glob / Grep happy path + containment
// ---------------------------------------------------------------------------

#[tokio::test]
async fn workspace_tools_read_write_edit_glob_and_grep_stay_inside_root() {
    let root = tempdir().unwrap();
    let tools = WorkspaceTools::new(root.path()).unwrap();

    // Write creates parent directories under the root.
    let written = tools
        .write("src/main.rs", "fn main() {\n    println!(\"hi\");\n}\n")
        .unwrap();
    assert!(written.output.contains("src/main.rs"));
    assert!(root.path().join("src/main.rs").is_file());

    // A second top-level file so glob/grep have more than one candidate.
    tools.write("README.md", "# Title\nfn note\n").unwrap();

    // Read returns plain text, matching the content the model may edit.
    let read = tools.read("src/main.rs", 1, MAX_READ_LINES).unwrap();
    assert_eq!(read.output, "fn main() {\n    println!(\"hi\");\n}\n");

    // Read window honors offset and limit and tells the model how to continue.
    let windowed = tools.read("src/main.rs", 2, 1).unwrap();
    assert_eq!(
        windowed.output,
        "    println!(\"hi\");\n\n[2 more lines in file. Use offset=3 to continue.]"
    );

    // Edit replaces an exact unique match.
    let edited = tools
        .edit(
            "src/main.rs",
            &[FileEdit::new("println!(\"hi\")", "println!(\"bye\")")],
        )
        .unwrap();
    assert!(edited.output.contains("replaced 1 block"));
    let after = std::fs::read_to_string(root.path().join("src/main.rs")).unwrap();
    assert!(after.contains("bye"));

    // Edit rejects a missing match and an empty oldText.
    assert!(tools
        .edit("src/main.rs", &[FileEdit::new("nope", "x")])
        .is_err());
    assert!(tools
        .edit("src/main.rs", &[FileEdit::new("", "x")])
        .is_err());

    // Glob over "**/*" finds both the nested and top-level files, sorted.
    let globbed = tools.glob("**/*", 200).unwrap();
    let listed: Vec<&str> = globbed.output.lines().collect();
    assert!(listed.contains(&"README.md"), "glob output: {listed:?}");
    assert!(listed.contains(&"src/main.rs"), "glob output: {listed:?}");

    // A narrower glob only matches under the chosen subtree.
    let scoped = tools.glob("src/**/*.rs", 200).unwrap();
    assert_eq!(scoped.output, "src/main.rs");

    // Grep returns path:line:text matches in sorted path order.
    let grepped = tools.grep("fn", "**/*", 200).unwrap();
    assert!(grepped.output.contains("src/main.rs:1:fn main() {"));
    assert!(grepped.output.contains("README.md:2:fn note"));

    // Containment: writing or reading outside the root is refused.
    assert!(tools.write("../escape.txt", "nope").is_err());
    assert!(tools.read("/etc/passwd", 1, MAX_READ_LINES).is_err());
    assert!(!root.path().parent().unwrap().join("escape.txt").exists());

    // Reading a missing file is an error, not a panic.
    assert!(tools.read("does/not/exist.txt", 1, MAX_READ_LINES).is_err());
}

#[test]
fn workspace_tools_edit_validates_every_block_before_writing() {
    let root = tempdir().unwrap();
    let tools = WorkspaceTools::new(root.path()).unwrap();
    let original = "alpha\nbeta\ngamma\n";
    tools.write("file.txt", original).unwrap();

    tools
        .edit(
            "file.txt",
            &[
                FileEdit::new("alpha", "one"),
                FileEdit::new("gamma", "three"),
            ],
        )
        .unwrap();
    assert_eq!(
        std::fs::read_to_string(root.path().join("file.txt")).unwrap(),
        "one\nbeta\nthree\n"
    );

    tools.write("file.txt", original).unwrap();

    let result = tools.edit(
        "file.txt",
        &[
            FileEdit::new("alpha", "one"),
            FileEdit::new("missing", "nope"),
        ],
    );

    assert!(result.is_err());
    assert_eq!(
        std::fs::read_to_string(root.path().join("file.txt")).unwrap(),
        original
    );

    tools.write("file.txt", "repeat\nrepeat\n").unwrap();
    assert!(tools
        .edit("file.txt", &[FileEdit::new("repeat", "once")])
        .is_err());
}

#[tokio::test]
async fn workspace_tools_glob_does_not_match_across_directory_separators() {
    let root = tempdir().unwrap();
    let tools = WorkspaceTools::new(root.path()).unwrap();
    tools.write("top.txt", "x").unwrap();
    tools.write("nested/inner.txt", "y").unwrap();

    // `*` must not cross `/`, so a single-segment pattern matches only top level.
    let shallow = tools.glob("*.txt", 200).unwrap();
    let listed: Vec<&str> = shallow.output.lines().collect();
    assert_eq!(listed, vec!["top.txt"], "shallow glob: {listed:?}");
}

// ---------------------------------------------------------------------------
// Bash guard and execution
// ---------------------------------------------------------------------------

#[tokio::test]
async fn workspace_tools_bash_gates_destructive_commands_on_a_human_decision() {
    let root = tempdir().unwrap();
    std::fs::write(root.path().join("file.txt"), "keep me").unwrap();
    let executor = ToolExecutor::new(Some(root.path().to_path_buf())).unwrap();

    // Destructive but ordinary development work: the tool asks rather than
    // refusing, and runs nothing until the answer arrives. Refusing outright
    // only moved the job to the human.
    for command in [
        "rm -rf build",
        "del file.txt",
        "rmdir target",
        "git reset --hard HEAD",
        "git clean -fd",
        "git push origin main --force",
        "powershell Remove-Item secret.txt",
    ] {
        let result = executor
            .execute("Bash", json!({ "command": command }))
            .await;
        assert_eq!(
            result.status,
            ToolStatus::ApprovalRequired,
            "command should need approval: {command}"
        );
        let payload: Value = serde_json::from_str(&result.output).unwrap();
        assert_eq!(payload["approval_request"]["subject"], command);
        assert!(
            !payload["approval_request"]["rule"]
                .as_str()
                .unwrap()
                .is_empty(),
            "the request must name the rule a remembered grant is keyed on: {}",
            result.output
        );
    }
    assert!(
        root.path().join("file.txt").exists(),
        "nothing may run before the user answers"
    );

    // An executor holding the grant runs the same command.
    let granted = ToolExecutor::new(Some(root.path().to_path_buf()))
        .unwrap()
        .with_approvals(ApprovalGrants::new(["delete-files".to_string()]));
    let deleted = granted
        .execute("Bash", json!({ "command": "rm file.txt" }))
        .await;
    assert_eq!(deleted.status, ToolStatus::Completed, "{}", deleted.output);
    assert!(!root.path().join("file.txt").exists());

    // Host-level operations are refused with no approval offered: no click makes
    // formatting a volume part of a workspace task.
    for command in ["shutdown /s /t 0", "mkfs.ext4 /dev/sda1"] {
        let result = executor
            .execute("Bash", json!({ "command": command }))
            .await;
        assert_eq!(
            result.status,
            ToolStatus::Failed,
            "command should be blocked outright: {command}"
        );
        assert!(result.output.contains("blocked"), "{}", result.output);
    }

    // Empty command is rejected.
    let empty = executor.execute("Bash", json!({ "command": "   " })).await;
    assert_eq!(empty.status, ToolStatus::Failed);

    // A redirection target escaping the workspace stops before running.
    let escape = executor
        .execute("Bash", json!({ "command": "echo hi > ../escape.txt" }))
        .await;
    assert_eq!(escape.status, ToolStatus::ApprovalRequired);
    assert!(!root.path().parent().unwrap().join("escape.txt").exists());
}

#[tokio::test]
async fn workspace_tools_bash_runs_in_workspace_with_bounded_output() {
    let root = tempdir().unwrap();
    let executor = ToolExecutor::new(Some(root.path().to_path_buf())).unwrap();

    // The command runs with the workspace root as its working directory: the
    // relative redirect target lands inside the root.
    let probe = executor
        .execute(
            "Bash",
            json!({ "command": "echo workspace_probe > probe.txt" }),
        )
        .await;
    assert_eq!(probe.status, ToolStatus::Completed, "{}", probe.output);
    assert!(
        probe.output.contains("exit_code=0"),
        "expected exit code, got: {}",
        probe.output
    );
    assert!(
        root.path().join("probe.txt").is_file(),
        "command should run in the workspace root"
    );

    // Output is bounded, and truncation keeps the TAIL: a failing build prints
    // its banner first and its error summary last, so the head is the wrong
    // half to retain.
    let mut big = "HEAD-MARKER\n".to_string();
    big.push_str(&"a\n".repeat(12_000));
    big.push_str("TAIL-MARKER\n");
    std::fs::write(root.path().join("big.txt"), &big).unwrap();
    let dump_command = if cfg!(windows) {
        "type big.txt"
    } else {
        "cat big.txt"
    };
    let dumped = executor
        .execute("Bash", json!({ "command": dump_command }))
        .await;
    assert_eq!(dumped.status, ToolStatus::Completed, "{}", dumped.output);
    assert!(
        dumped.output.starts_with("[output truncated"),
        "the truncation marker should lead the output: {}",
        &dumped.output[..dumped.output.len().min(200)]
    );
    assert!(
        dumped.output.contains("TAIL-MARKER"),
        "truncation should keep the tail"
    );
    assert!(
        !dumped.output.contains("HEAD-MARKER"),
        "truncation should drop the head"
    );

    // Nothing is lost: the complete output is spilled to a workspace-relative
    // path the model can read back with the `Read` tool.
    let spill = dumped
        .output
        .lines()
        .next()
        .unwrap()
        .split("complete output is at ")
        .nth(1)
        .and_then(|tail| tail.strip_suffix(']'))
        .expect("truncation marker should name the spill file");
    let spilled = executor
        .execute("Read", json!({ "path": spill, "limit": 5 }))
        .await;
    assert_eq!(spilled.status, ToolStatus::Completed, "{}", spilled.output);
    assert!(
        spilled.output.contains("HEAD-MARKER"),
        "the spill file should hold the discarded head: {}",
        spilled.output
    );
}

#[tokio::test]
async fn workspace_tools_shell_answers_to_every_dialect_name() {
    let root = tempdir().unwrap();
    let executor = ToolExecutor::new(Some(root.path().to_path_buf())).unwrap();

    // A model that has seen `Bash` elsewhere, or reads the host-specific name
    // off the tool list, reaches the same shell either way.
    for name in ["Bash", "Pwsh", "Cmd", "Shell"] {
        let result = executor
            .execute(name, json!({ "command": "echo dialect_probe" }))
            .await;
        assert_eq!(
            result.status,
            ToolStatus::Completed,
            "`{name}` should reach the shell: {}",
            result.output
        );
        assert!(
            result.output.contains("dialect_probe"),
            "`{name}` output: {}",
            result.output
        );
    }
}

#[tokio::test]
async fn workspace_tools_shell_runs_a_background_job_and_reads_it_incrementally() {
    let root = tempdir().unwrap();
    let executor = ToolExecutor::new(Some(root.path().to_path_buf())).unwrap();

    let started = executor
        .execute(
            "Bash",
            json!({ "command": "echo background_probe", "run_in_background": true }),
        )
        .await;
    assert_eq!(started.status, ToolStatus::Completed, "{}", started.output);
    let job_id = started
        .output
        .lines()
        .find_map(|line| line.strip_prefix("job_id="))
        .expect("background start should report a job id")
        .to_string();

    // The job runs detached, so poll until it reports output or finishes.
    let mut seen = String::new();
    for _ in 0..50 {
        let read = executor
            .execute("ShellOutput", json!({ "job_id": job_id }))
            .await;
        assert_eq!(read.status, ToolStatus::Completed, "{}", read.output);
        seen.push_str(&read.output);
        if seen.contains("background_probe") {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    assert!(
        seen.contains("background_probe"),
        "background output should surface: {seen}"
    );

    let listed = executor.execute("ShellJobs", json!({})).await;
    assert!(listed.output.contains(&job_id), "{}", listed.output);

    // A job id from another workspace is inert rather than readable.
    let other = tempdir().unwrap();
    let stranger = ToolExecutor::new(Some(other.path().to_path_buf())).unwrap();
    let denied = stranger
        .execute("ShellOutput", json!({ "job_id": job_id }))
        .await;
    assert_eq!(denied.status, ToolStatus::Failed, "{}", denied.output);
}

#[cfg(windows)]
#[tokio::test]
async fn workspace_tools_shell_closes_the_windows_redirect_bypass() {
    let root = tempdir().unwrap();
    let executor = ToolExecutor::new(Some(root.path().to_path_buf())).unwrap();

    // POSIX lexing collapsed this target to `....evil.txt`, which carried no
    // `..` segment: the guard passed while the shell wrote two levels up.
    let escaped = executor
        .execute("Bash", json!({ "command": r"echo pwned > ..\..\evil.txt" }))
        .await;
    // Still caught — now as a question, since the user can legitimately allow it.
    assert_eq!(
        escaped.status,
        ToolStatus::ApprovalRequired,
        "{}",
        escaped.output
    );
    let payload: Value = serde_json::from_str(&escaped.output).unwrap();
    assert_eq!(
        payload["approval_request"]["rule"],
        "write-outside-workspace"
    );
    let escape_target = root.path().parent().unwrap().parent().unwrap();
    assert!(!escape_target.join("evil.txt").exists());

    // A command with an apostrophe and no redirection is no longer rejected by
    // the lexer: POSIX `shlex` failed to parse it and refused the whole command.
    let apostrophe = executor
        .execute("Bash", json!({ "command": "echo it's fine" }))
        .await;
    assert_eq!(
        apostrophe.status,
        ToolStatus::Completed,
        "{}",
        apostrophe.output
    );
}

#[tokio::test]
async fn workspace_tools_shell_output_survives_a_non_ascii_round_trip() {
    // The previous implementation ran `cmd /C` and decoded with
    // `String::from_utf8_lossy`. On a Simplified-Chinese host the console code
    // page is CP936, so a `git log`, an `npm` error, or any other Chinese output
    // reached the model as a wall of U+FFFD with no recoverable content.
    let root = tempdir().unwrap();
    let executor = ToolExecutor::new(Some(root.path().to_path_buf())).unwrap();

    let echoed = executor
        .execute("Bash", json!({ "command": "echo 你好世界" }))
        .await;
    assert_eq!(echoed.status, ToolStatus::Completed, "{}", echoed.output);
    assert!(
        echoed.output.contains("你好世界"),
        "non-ASCII output should survive: {}",
        echoed.output
    );
    assert!(
        !echoed.output.contains('\u{FFFD}'),
        "no character should decode to the replacement character: {}",
        echoed.output
    );

    // The same holds for text the shell reads back off disk.
    std::fs::write(root.path().join("notes.txt"), "第一行\n第二行\n").unwrap();
    let dump_command = if cfg!(windows) {
        "Get-Content notes.txt"
    } else {
        "cat notes.txt"
    };
    let dumped = executor
        .execute("Bash", json!({ "command": dump_command }))
        .await;
    assert_eq!(dumped.status, ToolStatus::Completed, "{}", dumped.output);
    assert!(
        dumped.output.contains("第一行") && dumped.output.contains("第二行"),
        "file contents should decode: {}",
        dumped.output
    );
}

#[cfg(windows)]
#[tokio::test]
async fn workspace_tools_shell_speaks_powershell_on_windows() {
    let root = tempdir().unwrap();
    let executor = ToolExecutor::new(Some(root.path().to_path_buf())).unwrap();

    // A PowerShell-only construct proves the dialect the tool advertises is the
    // dialect that parses the command.
    let result = executor
        .execute(
            "Bash",
            json!({ "command": "$items = 2 + 3; Write-Output \"sum=$items\"" }),
        )
        .await;
    assert_eq!(result.status, ToolStatus::Completed, "{}", result.output);
    assert!(
        result.output.contains("sum=5"),
        "PowerShell should have parsed this: {}",
        result.output
    );
    assert!(result.output.contains("exit_code=0"), "{}", result.output);

    // A failing native command still reports a real exit code, not PowerShell's.
    let failed = executor
        .execute("Bash", json!({ "command": "cmd /c exit 7" }))
        .await;
    assert!(
        failed.output.contains("exit_code=7"),
        "the native exit code should survive the wrapper: {}",
        failed.output
    );
}

// ---------------------------------------------------------------------------
// Fetch
// ---------------------------------------------------------------------------

/// Spawn a single-shot local HTTP server that replies with a `text/plain` body.
async fn spawn_text_server(body: String) -> (std::net::SocketAddr, tokio::task::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let handle = tokio::spawn(async move {
        if let Ok((mut socket, _)) = listener.accept().await {
            // Drain the request headers (best effort) before responding.
            let mut buffer = [0u8; 1024];
            let _ = socket.read(&mut buffer).await;
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            let _ = socket.write_all(response.as_bytes()).await;
            let _ = socket.flush().await;
        }
    });
    (addr, handle)
}

#[tokio::test]
async fn workspace_tools_fetch_rejects_non_http_and_reads_local_text_server() {
    let executor = ToolExecutor::without_workspace();

    // Non-http(s) URLs and unparseable input are rejected without any request.
    for bad_url in ["ftp://example.com/data", "file:///etc/passwd", "not-a-url"] {
        let result = executor.execute("Fetch", json!({ "url": bad_url })).await;
        assert_eq!(
            result.status,
            ToolStatus::Failed,
            "url should be rejected: {bad_url}"
        );
    }

    // A local text server is fetched and summarized, no live network involved.
    let body = "hello workspace_tools fetch body".to_string();
    let (addr, server) = spawn_text_server(body.clone()).await;
    let url = format!("http://{addr}/");
    let result = executor.execute("Fetch", json!({ "url": url })).await;
    assert_eq!(result.status, ToolStatus::Completed, "{}", result.output);
    assert!(
        result.output.contains("Fetched http://"),
        "expected a fetch header, got: {}",
        result.output
    );
    assert!(
        result.output.contains("hello workspace_tools fetch body"),
        "expected the body snippet, got: {}",
        result.output
    );
    server.await.unwrap();
}

// ---------------------------------------------------------------------------
// Controlled (non-executing) tools
// ---------------------------------------------------------------------------

#[tokio::test]
async fn workspace_tools_controlled_tools_return_expected_statuses() {
    let executor = ToolExecutor::without_workspace();

    // AskUser: required vs optional.
    let required = executor
        .execute(
            "AskUser",
            json!({ "question": "Proceed?", "required": true }),
        )
        .await;
    assert_eq!(required.status, ToolStatus::WaitingForUser);
    let payload: Value = serde_json::from_str(&required.output).unwrap();
    assert_eq!(payload["status"], "WAITING_FOR_USER");

    let optional = executor
        .execute(
            "AskUser",
            json!({ "question": "Pick one", "required": false, "choices": ["a", " b ", ""] }),
        )
        .await;
    assert_eq!(optional.status, ToolStatus::InputRequested);
    let payload: Value = serde_json::from_str(&optional.output).unwrap();
    assert_eq!(payload["status"], "INPUT_REQUESTED");
    assert_eq!(payload["choices"], json!(["a", "b"]));

    // WebSearch with no provider configured is setup-required.
    let search = executor
        .execute("WebSearch", json!({ "query": "rust" }))
        .await;
    assert_eq!(search.status, ToolStatus::SetupRequired);
    let payload: Value = serde_json::from_str(&search.output).unwrap();
    assert_eq!(payload["status"], "SETUP_REQUIRED");

    // WebSearch validates its arguments.
    let bad_search = executor
        .execute("WebSearch", json!({ "query": "   " }))
        .await;
    assert_eq!(bad_search.status, ToolStatus::Failed);

    // Media stubs are setup-required.
    let image = executor
        .execute("GenerateImage", json!({ "prompt": "a cat" }))
        .await;
    assert_eq!(image.status, ToolStatus::SetupRequired);
    let video = executor
        .execute("GenerateVideo", json!({ "prompt": "a dog" }))
        .await;
    assert_eq!(video.status, ToolStatus::SetupRequired);

    // TodoWrite normalizes whatever shape the model sent into a bounded
    // checklist, so a status the model tracked is not silently dropped.
    let todos = executor
        .execute(
            "TodoWrite",
            json!({
                "todos": [
                    { "content": "one", "status": "in_progress" },
                    "two",
                ]
            }),
        )
        .await;
    assert_eq!(todos.status, ToolStatus::Completed);
    let payload: Value = serde_json::from_str(&todos.output).unwrap();
    assert_eq!(payload["status"], "COMPLETED");
    assert_eq!(
        payload["todos"],
        json!([
            { "content": "one", "status": "in_progress" },
            { "content": "two", "status": "pending" },
        ])
    );

    // ExitPlanMode needs approval and performs no side effect.
    let plan = executor
        .execute("ExitPlanMode", json!({ "plan": "do the thing" }))
        .await;
    assert_eq!(plan.status, ToolStatus::ApprovalRequired);
    let payload: Value = serde_json::from_str(&plan.output).unwrap();
    assert_eq!(payload["status"], "APPROVAL_REQUIRED");
}

// ---------------------------------------------------------------------------
// Executor dispatch
// ---------------------------------------------------------------------------

#[tokio::test]
async fn workspace_tools_executor_dispatches_file_and_non_file_tools_safely() {
    let root = tempdir().unwrap();
    let executor = ToolExecutor::new(Some(root.path().to_path_buf())).unwrap();

    // Write then read a file through the executor.
    let written = executor
        .execute(
            "Write",
            json!({ "path": "a/b.txt", "content": "line1\nline2\n" }),
        )
        .await;
    assert_eq!(written.status, ToolStatus::Completed, "{}", written.output);

    let read = executor.execute("Read", json!({ "path": "a/b.txt" })).await;
    assert_eq!(read.status, ToolStatus::Completed);
    assert!(read.output.starts_with("line1\nline2"));

    // Glob (default pattern) and Grep dispatch to the file tools.
    let globbed = executor.execute("Glob", json!({})).await;
    assert_eq!(globbed.status, ToolStatus::Completed);
    assert!(globbed.output.contains("a/b.txt"));

    let grepped = executor
        .execute("Grep", json!({ "pattern": "line2" }))
        .await;
    assert_eq!(grepped.status, ToolStatus::Completed);
    assert!(grepped.output.contains("a/b.txt:2:line2"));

    // Edit dispatches and succeeds.
    let edited = executor
        .execute(
            "Edit",
            json!({
                "path": "a/b.txt",
                "edits": [{ "oldText": "line1", "newText": "LINE1" }]
            }),
        )
        .await;
    assert_eq!(edited.status, ToolStatus::Completed);

    let deleted = executor
        .execute("DeleteFile", json!({ "path": "a/b.txt" }))
        .await;
    assert_eq!(deleted.status, ToolStatus::Completed, "{}", deleted.output);
    assert!(!root.path().join("a/b.txt").exists());
    assert_eq!(
        executor
            .execute("DeleteFile", json!({ "path": "a" }))
            .await
            .status,
        ToolStatus::Failed,
    );
    assert!(root.path().join("a").is_dir());

    // A non-file tool runs through the same executor.
    let todos = executor
        .execute("TodoWrite", json!({ "todos": ["x"] }))
        .await;
    assert_eq!(todos.status, ToolStatus::Completed);

    // Unknown tools do not panic.
    let unknown = executor.execute("Nonexistent", json!({})).await;
    assert_eq!(unknown.status, ToolStatus::Failed);
    assert!(unknown.output.contains("unavailable"));

    // A missing required argument is a model-safe failure.
    let missing = executor.execute("Read", json!({})).await;
    assert_eq!(missing.status, ToolStatus::Failed);

    // A path escape is rejected and never echoes the absolute local root.
    let escape = executor
        .execute("Read", json!({ "path": "../secret.txt" }))
        .await;
    assert_eq!(escape.status, ToolStatus::Failed);
    let absolute_root = root.path().to_string_lossy();
    assert!(
        !escape.output.contains(absolute_root.as_ref()),
        "error text must not leak the absolute workspace path"
    );

    // Without a workspace, file tools report WORKSPACE_REQUIRED.
    let no_workspace = ToolExecutor::without_workspace();
    let needs_ws = no_workspace
        .execute("Read", json!({ "path": "a.txt" }))
        .await;
    assert_eq!(needs_ws.status, ToolStatus::WorkspaceRequired);
    let payload: Value = serde_json::from_str(&needs_ws.output).unwrap();
    assert_eq!(payload["status"], "WORKSPACE_REQUIRED");
}

// ---------------------------------------------------------------------------
// Named mounts: the agent's own workspace alongside the conversation's
// ---------------------------------------------------------------------------

/// Build a primary root plus a `~self` mount, each with one marker file.
fn mounted_pair() -> (tempfile::TempDir, tempfile::TempDir, WorkspaceTools) {
    let primary = tempdir().unwrap();
    let own = tempdir().unwrap();
    std::fs::write(primary.path().join("shared.md"), "shared note\n").unwrap();
    std::fs::create_dir(own.path().join("templates")).unwrap();
    std::fs::write(own.path().join("templates/letter.md"), "private template\n").unwrap();
    let mount = WorkspaceMount::new(SELF_MOUNT_NAME, own.path()).unwrap();
    let tools = WorkspaceTools::with_mounts(primary.path(), vec![mount]).unwrap();
    (primary, own, tools)
}

#[test]
fn workspace_tools_mount_reads_both_roots_in_one_address_space() {
    let (_primary, _own, tools) = mounted_pair();

    let shared = tools.read("shared.md", 1, MAX_READ_LINES).unwrap();
    assert!(shared.output.contains("shared note"));

    let private = tools
        .read("~self/templates/letter.md", 1, MAX_READ_LINES)
        .unwrap();
    assert!(private.output.contains("private template"));
}

#[test]
fn workspace_tools_mount_writes_and_edits_report_the_mounted_address() {
    let (primary, own, tools) = mounted_pair();

    let written = tools.write("~self/notes/todo.md", "one\n").unwrap();
    assert!(
        written.output.contains("~self/notes/todo.md"),
        "write must echo the address the caller can read back, got: {}",
        written.output
    );
    assert!(own.path().join("notes/todo.md").is_file());
    assert!(
        !primary.path().join("notes").exists(),
        "a mounted write must not land in the primary root"
    );

    let edited = tools
        .edit("~self/notes/todo.md", &[FileEdit::new("one", "two")])
        .unwrap();
    assert!(edited.output.contains("~self/notes/todo.md"));
    assert_eq!(
        std::fs::read_to_string(own.path().join("notes/todo.md")).unwrap(),
        "two\n"
    );
}

#[test]
fn workspace_tools_mount_glob_and_grep_prefix_mounted_matches() {
    let (_primary, _own, tools) = mounted_pair();

    let all = tools.glob("**/*.md", 100).unwrap().output;
    assert!(all.contains("shared.md"), "got: {all}");
    assert!(all.contains("~self/templates/letter.md"), "got: {all}");
    // Primary results sort ahead of mounted ones.
    assert!(all.find("shared.md") < all.find("~self/"), "got: {all}");

    // A mount-scoped pattern selects only that root.
    let scoped = tools.glob("~self/**/*.md", 100).unwrap().output;
    assert_eq!(scoped, "~self/templates/letter.md");

    let matches = tools.grep("note|template", "**/*.md", 100).unwrap().output;
    assert!(
        matches.contains("shared.md:1:shared note"),
        "got: {matches}"
    );
    assert!(
        matches.contains("~self/templates/letter.md:1:private template"),
        "got: {matches}"
    );
}

#[test]
fn workspace_tools_mount_enforces_containment_on_the_mounted_root() {
    let (_primary, _own, tools) = mounted_pair();

    for unsafe_path in [
        "~self/../escape.txt",
        "~self/a/../../b",
        "~self/C:/tmp/x",
        "~self//etc/passwd",
    ] {
        assert!(
            tools.read(unsafe_path, 1, MAX_READ_LINES).is_err(),
            "expected `{unsafe_path}` to be rejected"
        );
    }
    // The bare mount name is a directory, not a file.
    assert!(tools.read("~self", 1, MAX_READ_LINES).is_err());
}

#[test]
fn workspace_tools_mount_name_must_match_a_whole_segment() {
    let primary = tempdir().unwrap();
    let own = tempdir().unwrap();
    std::fs::create_dir(primary.path().join("~selfish")).unwrap();
    std::fs::write(primary.path().join("~selfish/x.md"), "primary file\n").unwrap();
    let tools = WorkspaceTools::with_mounts(
        primary.path(),
        vec![WorkspaceMount::new(SELF_MOUNT_NAME, own.path()).unwrap()],
    )
    .unwrap();

    // `~selfish` shares a prefix with `~self` but is an ordinary primary path.
    let read = tools.read("~selfish/x.md", 1, MAX_READ_LINES).unwrap();
    assert!(read.output.contains("primary file"));
}

#[test]
fn workspace_tools_mount_inside_the_primary_root_is_dropped() {
    let primary = tempdir().unwrap();
    let nested = primary.path().join("nested");
    std::fs::create_dir(&nested).unwrap();
    std::fs::write(nested.join("x.md"), "nested\n").unwrap();

    let tools = WorkspaceTools::with_mounts(
        primary.path(),
        vec![WorkspaceMount::new(SELF_MOUNT_NAME, &nested).unwrap()],
    )
    .unwrap();

    assert!(
        tools.mounts().is_empty(),
        "a mount already addressable from the primary root must be dropped"
    );
    // The file is still reachable by its ordinary relative path, exactly once.
    assert_eq!(tools.glob("**/*.md", 100).unwrap().output, "nested/x.md");
}

#[tokio::test]
async fn workspace_tools_executor_exposes_mounts_to_file_tools_but_not_bash() {
    let primary = tempdir().unwrap();
    let own = tempdir().unwrap();
    std::fs::write(own.path().join("secret-plan.md"), "mounted\n").unwrap();

    let executor = ToolExecutor::new_with_mounts(
        Some(primary.path().to_path_buf()),
        vec![WorkspaceMount::new(SELF_MOUNT_NAME, own.path()).unwrap()],
        Vec::new(),
    )
    .unwrap();

    let read = executor
        .execute("Read", json!({ "path": "~self/secret-plan.md" }))
        .await;
    assert_eq!(read.status, ToolStatus::Completed);
    assert!(read.output.contains("mounted"));

    // Bash stays bound to the primary root; the mount is not its cwd.
    assert_eq!(
        executor.workspace_root().map(Path::to_path_buf),
        Some(std::fs::canonicalize(primary.path()).unwrap())
    );
    assert_eq!(executor.workspace_mounts().len(), 1);
}

/// `AskUser` must carry a structured `input_request`.
///
/// The runtime lifts that key out of the tool output onto the
/// `waiting_for_user` event; without it the UI falls back to a generic "The
/// agent requested input." and the user never sees the question.
#[tokio::test]
async fn workspace_tools_ask_user_emits_a_structured_input_request() {
    let executor = ToolExecutor::without_workspace();

    let required = executor
        .execute(
            "AskUser",
            json!({ "question": "Which workspace?", "required": true }),
        )
        .await;
    let payload: Value = serde_json::from_str(&required.output).unwrap();
    assert_eq!(payload["input_request"]["question"], "Which workspace?");
    assert_eq!(payload["input_request"]["required"], json!(true));

    let with_choices = executor
        .execute(
            "AskUser",
            json!({
                "question": "Pick one",
                "required": false,
                "choices": ["Create a new one", " Use an existing one ", ""]
            }),
        )
        .await;
    let payload: Value = serde_json::from_str(&with_choices.output).unwrap();
    assert_eq!(payload["input_request"]["question"], "Pick one");
    assert_eq!(payload["input_request"]["required"], json!(false));
    assert_eq!(
        payload["input_request"]["choices"],
        json!(["Create a new one", "Use an existing one"])
    );
    // The rendered choices become buttons, so a choice-shaped request says so.
    assert_eq!(payload["input_request"]["input_type"], "choice");
}
