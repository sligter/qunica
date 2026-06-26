//! Workspace tool safety and behavior tests.
//!
//! These exercise the path-safety resolver and the `Read`/`Write`/`Edit`/
//! `Glob`/`Grep` file tools directly against a temporary workspace. No network
//! or database is involved. Every test name contains `workspace_tools` so the
//! `--workspace workspace_tools` filter selects them.

use std::path::Path;

use ag_swarmer_backend::tools::{resolve_workspace_path, WorkspaceTools, MAX_READ_LINES};
use tempfile::tempdir;

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
