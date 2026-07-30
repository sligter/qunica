//! Safe workspace file tools: `Read`, `Write`, `Edit`, `Glob`, `Grep`.
//!
//! [`WorkspaceTools`] is the file-tool facade the Task 8b runtime tool loop will
//! drive. Every method routes caller-supplied paths through
//! [`resolve_workspace_path`](crate::tools::resolve_workspace_path) so no access
//! escapes the workspace root, and enforces the runtime line/size/result bounds
//! used by Rust provider-native tools.
//!
//! A tool binding has one *primary* root — the address space of every ordinary
//! relative path — plus zero or more *named mounts*. A caller path whose first
//! segment matches a mount name resolves under that mount instead; both sides
//! run the identical containment checks, so a mount widens what an agent can
//! address without weakening what it can escape to.

use std::{
    fs,
    path::{Path, PathBuf},
};

use globset::GlobBuilder;
use regex::Regex;
use walkdir::WalkDir;

use super::{path_safety, resolve_workspace_path, ToolError, ToolResult};

/// Maximum number of lines `Read` returns (and the window cap it applies).
pub const MAX_READ_LINES: usize = 2000;
/// Maximum number of paths `Glob` returns.
pub const MAX_GLOB_RESULTS: usize = 200;
/// Maximum number of matches `Grep` returns.
pub const MAX_GREP_RESULTS: usize = 200;
/// Largest file (in bytes) `Read`/`Edit`/`Grep` will open.
pub const MAX_FILE_BYTES: u64 = 1_000_000;
/// Largest content (in bytes) `Write`/`Edit` will persist.
pub const MAX_WRITE_BYTES: usize = 1_000_000;

/// The mount name under which an agent's own workspace is exposed while the
/// conversation workspace stays primary.
pub const SELF_MOUNT_NAME: &str = "~self";

/// One exact replacement applied by [`WorkspaceTools::edit`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileEdit {
    pub old_text: String,
    pub new_text: String,
}

impl FileEdit {
    pub fn new(old_text: impl Into<String>, new_text: impl Into<String>) -> Self {
        Self {
            old_text: old_text.into(),
            new_text: new_text.into(),
        }
    }
}

/// A named secondary root, addressable from tool paths as `<name>/...`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceMount {
    /// Address prefix, e.g. `~self`. Never contains a path separator.
    pub name: String,
    /// Canonicalized root the prefix resolves to.
    pub root: PathBuf,
}

impl WorkspaceMount {
    /// Build a mount named `name` rooted at `root`, canonicalizing the root.
    pub fn new(name: impl Into<String>, root: impl AsRef<Path>) -> Result<Self, ToolError> {
        let name = name.into();
        if name.is_empty() || name.contains(['/', '\\']) {
            return Err(ToolError::invalid(
                "mount name must be non-empty and must not contain a path separator",
            ));
        }
        Ok(Self {
            name,
            root: canonical_directory(root.as_ref(), "mount root")?,
        })
    }
}

/// File tools bound to a primary workspace root plus optional named mounts.
///
/// Construct with [`WorkspaceTools::new`] (primary root only) or
/// [`WorkspaceTools::with_mounts`], both of which validate that every root
/// exists and is a directory. Paths and patterns passed to the methods are
/// relative to the primary root unless they open with a mount name, and are
/// checked for escapes against the owning root on every call.
#[derive(Debug, Clone)]
pub struct WorkspaceTools {
    /// Canonicalized primary root; plain relative paths stay under it.
    root: PathBuf,
    /// Named secondary roots, in address-resolution order.
    mounts: Vec<WorkspaceMount>,
}

impl WorkspaceTools {
    /// Bind the file tools to `root` with no mounts, canonicalizing it.
    ///
    /// Returns [`ToolError::Invalid`] if `root` is empty, does not exist, or is
    /// not a directory.
    pub fn new(root: impl AsRef<Path>) -> Result<Self, ToolError> {
        Self::with_mounts(root, Vec::new())
    }

    /// Bind the file tools to a primary `root` plus named `mounts`.
    ///
    /// A mount whose root is the primary root, or lives inside it, is dropped:
    /// it is already addressable as an ordinary relative path, and keeping it
    /// would list the same file twice under two addresses. Duplicate mount
    /// names are rejected so an address always has one meaning.
    pub fn with_mounts(
        root: impl AsRef<Path>,
        mounts: Vec<WorkspaceMount>,
    ) -> Result<Self, ToolError> {
        let root = canonical_directory(root.as_ref(), "workspace root")?;
        let mut retained: Vec<WorkspaceMount> = Vec::new();
        for mount in mounts {
            if mount.root.starts_with(&root) {
                continue;
            }
            if retained.iter().any(|kept| kept.name == mount.name) {
                return Err(ToolError::invalid("mount names must be unique"));
            }
            retained.push(mount);
        }
        Ok(Self {
            root,
            mounts: retained,
        })
    }

    /// The canonical primary workspace root.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The retained named mounts, in address-resolution order.
    pub fn mounts(&self) -> &[WorkspaceMount] {
        &self.mounts
    }

    /// Read a UTF-8 text file, optionally from a 1-based line offset.
    ///
    /// `offset=0` and `offset=1` both start at the first line. At most
    /// `min(limit, MAX_READ_LINES)` lines are returned, with a continuation hint
    /// when more remain. Invalid bytes are replaced with U+FFFD. Files larger
    /// than [`MAX_FILE_BYTES`] are rejected.
    pub fn read(
        &self,
        file_path: &str,
        offset: usize,
        limit: usize,
    ) -> Result<ToolResult, ToolError> {
        if limit < 1 {
            return Err(ToolError::invalid("limit must be >= 1"));
        }
        let (_, target) = self.resolve(file_path)?;
        let meta = fs::metadata(&target).map_err(|_| ToolError::invalid("file does not exist"))?;
        if !meta.is_file() {
            return Err(ToolError::invalid("file does not exist"));
        }
        if meta.len() > MAX_FILE_BYTES {
            return Err(ToolError::invalid(
                "file is too large to read with this tool",
            ));
        }

        let bytes = fs::read(&target)?;
        let text = String::from_utf8_lossy(&bytes);
        let normalized = normalize_to_lf(&text);
        let lines: Vec<&str> = normalized.split('\n').collect();
        let start = offset.saturating_sub(1);
        if start >= lines.len() {
            return Err(ToolError::invalid(format!(
                "offset {offset} is beyond end of file ({} lines total)",
                lines.len()
            )));
        }
        let end = (start + limit.min(MAX_READ_LINES)).min(lines.len());
        let mut output = lines[start..end].join("\n");
        let remaining = lines.len() - end;
        if remaining > 0 {
            output.push_str(&format!(
                "\n\n[{remaining} more lines in file. Use offset={} to continue.]",
                end + 1
            ));
        }
        Ok(ToolResult::completed(output))
    }

    /// Create or replace a UTF-8 file under the workspace root.
    ///
    /// Missing parent directories are created. Rejects content larger than
    /// [`MAX_WRITE_BYTES`] and refuses to overwrite an existing non-file target.
    pub fn write(&self, file_path: &str, content: &str) -> Result<ToolResult, ToolError> {
        // `content.len()` is the UTF-8 byte length, matching Python's
        // `content.encode("utf-8")` size check.
        let encoded_len = content.len();
        if encoded_len > MAX_WRITE_BYTES {
            return Err(ToolError::invalid(
                "content is too large to write with this tool",
            ));
        }
        let (index, target) = self.resolve(file_path)?;
        if let Ok(meta) = fs::metadata(&target) {
            if !meta.is_file() {
                return Err(ToolError::invalid("target path is not a file"));
            }
        }
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&target, content.as_bytes())?;
        let rel = self.relative_display(index, &target);
        Ok(ToolResult::completed(format!(
            "Wrote {encoded_len} bytes to {rel}."
        )))
    }

    /// Apply one or more exact replacements to an existing UTF-8 file.
    ///
    /// Every `old_text` must be non-empty, unique in the original file, and not
    /// overlap another edit. All edits are validated before the file is written,
    /// so a bad block leaves the original untouched. Line endings are normalized
    /// for matching and restored on write. The resulting content must not exceed
    /// [`MAX_WRITE_BYTES`].
    pub fn edit(&self, file_path: &str, edits: &[FileEdit]) -> Result<ToolResult, ToolError> {
        if edits.is_empty() {
            return Err(ToolError::invalid(
                "edits must contain at least one replacement",
            ));
        }
        let (index, target) = self.resolve(file_path)?;
        let meta = fs::metadata(&target).map_err(|_| ToolError::invalid("file does not exist"))?;
        if !meta.is_file() {
            return Err(ToolError::invalid("file does not exist"));
        }
        if meta.len() > MAX_FILE_BYTES {
            return Err(ToolError::invalid(
                "file is too large to edit with this tool",
            ));
        }

        let bytes = fs::read(&target)?;
        let text =
            String::from_utf8(bytes).map_err(|_| ToolError::invalid("file is not valid UTF-8"))?;
        let line_ending = detect_line_ending(&text);
        let mut updated = normalize_to_lf(&text);
        let mut replacements = Vec::with_capacity(edits.len());

        for (edit_index, edit) in edits.iter().enumerate() {
            let old_text = normalize_to_lf(&edit.old_text);
            if old_text.is_empty() {
                return Err(ToolError::invalid(format!(
                    "edits[{edit_index}].oldText must be non-empty"
                )));
            }
            let matches: Vec<usize> = updated.match_indices(&old_text).map(|(at, _)| at).collect();
            if matches.is_empty() {
                return Err(ToolError::invalid(format!(
                    "edits[{edit_index}].oldText was not found"
                )));
            }
            if matches.len() > 1 {
                return Err(ToolError::invalid(format!(
                    "edits[{edit_index}].oldText is not unique (found {} occurrences)",
                    matches.len()
                )));
            }
            let start = matches[0];
            replacements.push((
                start,
                start + old_text.len(),
                normalize_to_lf(&edit.new_text),
            ));
        }

        replacements.sort_by_key(|(start, _, _)| *start);
        if replacements.windows(2).any(|pair| pair[0].1 > pair[1].0) {
            return Err(ToolError::invalid("edit blocks must not overlap"));
        }
        for (start, end, new_text) in replacements.into_iter().rev() {
            updated.replace_range(start..end, &new_text);
        }

        let updated = restore_line_endings(updated, line_ending);
        if updated.len() > MAX_WRITE_BYTES {
            return Err(ToolError::invalid(
                "edited content is too large to write with this tool",
            ));
        }
        fs::write(&target, updated.as_bytes())?;
        let rel = self.relative_display(index, &target);
        Ok(ToolResult::completed(format!(
            "Edited {rel}; replaced {} block(s).",
            edits.len()
        )))
    }

    /// List files matching `pattern` across the primary root and every mount,
    /// sorted by their addressable path.
    ///
    /// An empty pattern means `**/*` (all files). Patterns match the address a
    /// caller would pass back to `Read`/`Edit`, so mounted files are matched as
    /// `~self/...`. Directory separators are significant (`*` does not cross
    /// `/`; `**` does). Results are bounded by `min(limit, MAX_GLOB_RESULTS)`
    /// across all roots combined. Symlinks are never followed, so escaping
    /// entries are never traversed or returned.
    pub fn glob(&self, pattern: &str, limit: usize) -> Result<ToolResult, ToolError> {
        if limit < 1 {
            return Err(ToolError::invalid("limit must be >= 1"));
        }
        let matcher = self.glob_matcher(pattern)?;

        let mut matches: Vec<String> = Vec::new();
        for (index, root) in self.roots() {
            for (address, _) in walk_addressable_files(root, |rel| self.address(index, rel)) {
                if matcher.is_match(address.as_str()) {
                    matches.push(address);
                }
            }
        }
        matches.sort();
        matches.truncate(limit.min(MAX_GLOB_RESULTS));

        let output = if matches.is_empty() {
            "No files matched.".to_string()
        } else {
            matches.join("\n")
        };
        Ok(ToolResult::completed(output))
    }

    /// Search file contents across the primary root and every mount with a
    /// regular expression.
    ///
    /// Files are selected by `path` (a glob over addressable paths, defaulting
    /// to all files), searched in sorted address order, and matches are
    /// returned as `path:line:text`. Files larger than [`MAX_FILE_BYTES`] are
    /// skipped. Results are bounded by `min(limit, MAX_GREP_RESULTS)` across all
    /// roots combined. Symlinks are never followed.
    pub fn grep(&self, pattern: &str, path: &str, limit: usize) -> Result<ToolResult, ToolError> {
        if limit < 1 {
            return Err(ToolError::invalid("limit must be >= 1"));
        }
        let matcher = self.glob_matcher(path)?;
        let regex = Regex::new(pattern)
            .map_err(|err| ToolError::invalid(format!("invalid regex: {err}")))?;

        // Gather candidate files first so the search runs in stable path order.
        let mut files: Vec<(String, PathBuf)> = Vec::new();
        for (index, root) in self.roots() {
            files.extend(
                walk_addressable_files(root, |rel| self.address(index, rel))
                    .filter(|(address, _)| matcher.is_match(address.as_str())),
            );
        }
        files.sort_by(|a, b| a.0.cmp(&b.0));

        let cap = limit.min(MAX_GREP_RESULTS);
        let mut results: Vec<String> = Vec::new();
        'files: for (rel, path) in files {
            let Ok(meta) = fs::metadata(&path) else {
                continue;
            };
            if meta.len() > MAX_FILE_BYTES {
                continue;
            }
            let Ok(bytes) = fs::read(&path) else {
                continue;
            };
            let text = String::from_utf8_lossy(&bytes);
            for (index, line) in text.lines().enumerate() {
                if regex.is_match(line) {
                    results.push(format!("{}:{}:{}", rel, index + 1, line));
                    if results.len() >= cap {
                        break 'files;
                    }
                }
            }
        }

        let output = if results.is_empty() {
            "No matches found.".to_string()
        } else {
            results.join("\n")
        };
        Ok(ToolResult::completed(output))
    }

    /// Resolve a caller path against the root that owns its address, rejecting
    /// escapes. Returns the owning root's index alongside the resolved path.
    fn resolve(&self, relative: &str) -> Result<(usize, PathBuf), ToolError> {
        let (index, remainder) = self.route(relative);
        if index > 0 && remainder.trim().is_empty() {
            return Err(ToolError::invalid(
                "path must name a file inside the mounted workspace",
            ));
        }
        let resolved = resolve_workspace_path(self.root_at(index), remainder)?;
        Ok((index, resolved))
    }

    /// Split a caller path into the index of the root that owns it (`0` is the
    /// primary root, `n + 1` is `mounts[n]`) and the remainder relative to it.
    fn route<'a>(&self, path: &'a str) -> (usize, &'a str) {
        for (index, mount) in self.mounts.iter().enumerate() {
            if let Some(remainder) = strip_mount_prefix(path, &mount.name) {
                return (index + 1, remainder);
            }
        }
        (0, path)
    }

    /// The root a route index refers to.
    fn root_at(&self, index: usize) -> &Path {
        match index.checked_sub(1) {
            None => &self.root,
            Some(mount) => &self.mounts[mount].root,
        }
    }

    /// Every root paired with its route index, primary first.
    fn roots(&self) -> impl Iterator<Item = (usize, &Path)> {
        std::iter::once((0, self.root.as_path())).chain(
            self.mounts
                .iter()
                .enumerate()
                .map(|(index, mount)| (index + 1, mount.root.as_path())),
        )
    }

    /// Prefix a root-relative path with its mount name so the result is an
    /// address a caller can pass straight back into `Read`/`Edit`.
    fn address(&self, index: usize, relative: &str) -> String {
        match index.checked_sub(1) {
            None => relative.to_string(),
            Some(mount) => format!("{}/{relative}", self.mounts[mount].name),
        }
    }

    /// Build a separator-aware glob matcher from a safe relative pattern. An
    /// empty pattern defaults to `**/*`.
    fn glob_matcher(&self, pattern: &str) -> Result<globset::GlobMatcher, ToolError> {
        let safe = if pattern.is_empty() {
            "**/*".to_string()
        } else {
            path_safety::reject_unsafe_relative(pattern)?;
            pattern.to_string()
        };
        let glob = GlobBuilder::new(&safe)
            .literal_separator(true)
            .build()
            .map_err(|err| ToolError::invalid(format!("invalid glob pattern: {err}")))?;
        Ok(glob.compile_matcher())
    }

    /// Render a resolved path as the address a caller would use for it.
    fn relative_display(&self, index: usize, target: &Path) -> String {
        target
            .strip_prefix(self.root_at(index))
            .map(|relative| self.address(index, &to_forward_slashes(relative)))
            .unwrap_or_else(|_| target.to_string_lossy().into_owned())
    }
}

/// Walk the regular files under `root`, pairing each with the address produced
/// by `address`. Symlinks are never followed and unreadable entries are skipped.
fn walk_addressable_files<'a>(
    root: &'a Path,
    address: impl Fn(&str) -> String + 'a,
) -> impl Iterator<Item = (String, PathBuf)> + 'a {
    WalkDir::new(root)
        .follow_links(false)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_file())
        .filter_map(move |entry| {
            let relative = entry.path().strip_prefix(root).ok()?;
            Some((
                address(&to_forward_slashes(relative)),
                entry.path().to_path_buf(),
            ))
        })
}

/// Match `<name>`, `<name>/rest`, or `<name>\rest`, returning the remainder.
/// A path that merely starts with the same characters (`~selfish/x`) is not a
/// mount hit.
fn strip_mount_prefix<'a>(path: &'a str, name: &str) -> Option<&'a str> {
    let remainder = path.strip_prefix(name)?;
    match remainder.chars().next() {
        None => Some(""),
        Some('/' | '\\') => Some(&remainder[1..]),
        Some(_) => None,
    }
}

/// Canonicalize `path`, requiring it to be a non-empty existing directory.
/// `label` names the value in the rejection message.
fn canonical_directory(path: &Path, label: &str) -> Result<PathBuf, ToolError> {
    if path.as_os_str().is_empty() {
        return Err(ToolError::invalid(format!("{label} must be non-empty")));
    }
    match fs::metadata(path) {
        Ok(meta) if meta.is_dir() => {}
        Ok(_) => return Err(ToolError::invalid(format!("{label} is not a directory"))),
        Err(_) => return Err(ToolError::invalid(format!("{label} does not exist"))),
    }
    Ok(fs::canonicalize(path)?)
}

/// Convert a path to a forward-slash string for stable, OS-independent output.
fn to_forward_slashes(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn normalize_to_lf(text: &str) -> String {
    text.replace("\r\n", "\n").replace('\r', "\n")
}

fn detect_line_ending(text: &str) -> &'static str {
    if text.contains("\r\n") {
        "\r\n"
    } else if text.contains('\r') {
        "\r"
    } else {
        "\n"
    }
}

fn restore_line_endings(text: String, line_ending: &str) -> String {
    if line_ending == "\n" {
        text
    } else {
        text.replace('\n', line_ending)
    }
}
