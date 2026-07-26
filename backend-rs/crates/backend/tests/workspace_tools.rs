//! Workspace tool safety and behavior tests.
//!
//! These exercise the path-safety resolver and the `Read`/`Write`/`Edit`/
//! `Glob`/`Grep` file tools directly against a temporary workspace. No network
//! or database is involved. Every test name contains `workspace_tools` so the
//! `--workspace workspace_tools` filter selects them.

use std::path::Path;

use ag_swarmer_backend::tools::{
    resolve_workspace_path, ToolExecutor, ToolStatus, WorkspaceMount, WorkspaceTools,
    MAX_READ_LINES, SELF_MOUNT_NAME,
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

    // Read returns 1-based line numbers from the requested window.
    let read = tools.read("src/main.rs", 1, MAX_READ_LINES).unwrap();
    assert_eq!(
        read.output,
        "1\tfn main() {\n2\t    println!(\"hi\");\n3\t}"
    );

    // Read window honors start_line and limit.
    let windowed = tools.read("src/main.rs", 2, 1).unwrap();
    assert_eq!(windowed.output, "2\t    println!(\"hi\");");

    // Edit replaces an exact unique match.
    let edited = tools
        .edit(
            "src/main.rs",
            "println!(\"hi\")",
            "println!(\"bye\")",
            false,
        )
        .unwrap();
    assert!(edited.output.contains("replaced 1 occurrence"));
    let after = std::fs::read_to_string(root.path().join("src/main.rs")).unwrap();
    assert!(after.contains("bye"));

    // Edit rejects a missing match and an empty old_string.
    assert!(tools.edit("src/main.rs", "nope", "x", false).is_err());
    assert!(tools.edit("src/main.rs", "", "x", false).is_err());

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
async fn workspace_tools_bash_rejects_destructive_commands() {
    let root = tempdir().unwrap();
    let executor = ToolExecutor::new(Some(root.path().to_path_buf())).unwrap();

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
            ToolStatus::Failed,
            "command should be blocked: {command}"
        );
        assert!(
            result.output.contains("blocked"),
            "command `{command}` should report the safety policy, got: {}",
            result.output
        );
    }

    // Empty command is rejected.
    let empty = executor.execute("Bash", json!({ "command": "   " })).await;
    assert_eq!(empty.status, ToolStatus::Failed);

    // A redirection target escaping the workspace is rejected before running.
    let escape = executor
        .execute("Bash", json!({ "command": "echo hi > ../escape.txt" }))
        .await;
    assert_eq!(escape.status, ToolStatus::Failed);
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

    // Output is bounded: dumping a large file is truncated with a marker.
    let big = "a".repeat(20_000);
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
        dumped.output.contains("[output truncated]"),
        "large output should be truncated"
    );
    let marker_len = "\n[output truncated]".chars().count();
    assert!(
        dumped.output.chars().count() <= 12_000 + marker_len,
        "truncated output must stay within the char bound"
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

    // TodoWrite completes with a bounded list.
    let todos = executor
        .execute("TodoWrite", json!({ "todos": ["one", "two"] }))
        .await;
    assert_eq!(todos.status, ToolStatus::Completed);
    let payload: Value = serde_json::from_str(&todos.output).unwrap();
    assert_eq!(payload["status"], "COMPLETED");
    assert_eq!(payload["todos"], json!(["one", "two"]));

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
            json!({ "file_path": "a/b.txt", "content": "line1\nline2\n" }),
        )
        .await;
    assert_eq!(written.status, ToolStatus::Completed, "{}", written.output);

    let read = executor
        .execute("Read", json!({ "file_path": "a/b.txt" }))
        .await;
    assert_eq!(read.status, ToolStatus::Completed);
    assert!(read.output.contains("1\tline1"));

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
            json!({ "file_path": "a/b.txt", "old_string": "line1", "new_string": "LINE1" }),
        )
        .await;
    assert_eq!(edited.status, ToolStatus::Completed);

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
        .execute("Read", json!({ "file_path": "../secret.txt" }))
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
        .execute("Read", json!({ "file_path": "a.txt" }))
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
    std::fs::write(
        own.path().join("templates/letter.md"),
        "private template\n",
    )
    .unwrap();
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

    let edited = tools.edit("~self/notes/todo.md", "one", "two", false).unwrap();
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
    assert!(matches.contains("shared.md:1:shared note"), "got: {matches}");
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
        .execute("Read", json!({ "file_path": "~self/secret-plan.md" }))
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
