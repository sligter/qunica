//! Safe workspace file tools: `Read`, `Write`, `Edit`, `Glob`, `Grep`.
//!
//! [`WorkspaceTools`] is the file-tool facade the Task 8b runtime tool loop will
//! drive. Every method routes caller-supplied paths through
//! [`resolve_workspace_path`](crate::tools::resolve_workspace_path) so no access
//! escapes the workspace root, and enforces the runtime line/size/result bounds
//! used by Rust provider-native tools.

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

/// File tools bound to a single canonicalized workspace root.
///
/// Construct with [`WorkspaceTools::new`], which validates that the root exists
/// and is a directory. All paths and patterns passed to the methods are
/// relative to this root and are checked for escapes on every call.
#[derive(Debug, Clone)]
pub struct WorkspaceTools {
    /// Canonicalized workspace root; every resolved path stays under it.
    root: PathBuf,
}

impl WorkspaceTools {
    /// Bind the file tools to `root`, canonicalizing it.
    ///
    /// Returns [`ToolError::Invalid`] if `root` is empty, does not exist, or is
    /// not a directory.
    pub fn new(root: impl AsRef<Path>) -> Result<Self, ToolError> {
        let root = root.as_ref();
        if root.as_os_str().is_empty() {
            return Err(ToolError::invalid("workspace root must be non-empty"));
        }
        match fs::metadata(root) {
            Ok(meta) if meta.is_dir() => {}
            Ok(_) => return Err(ToolError::invalid("workspace root is not a directory")),
            Err(_) => return Err(ToolError::invalid("workspace root does not exist")),
        }
        let root = fs::canonicalize(root)?;
        Ok(Self { root })
    }

    /// The canonical workspace root.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Read a UTF-8 text file with 1-based line numbers.
    ///
    /// `start_line` must be `>= 1`; at most `min(limit, MAX_READ_LINES)` lines
    /// from `start_line` onward are returned. Invalid bytes are replaced with
    /// U+FFFD. Files larger than [`MAX_FILE_BYTES`] are rejected.
    pub fn read(
        &self,
        file_path: &str,
        start_line: usize,
        limit: usize,
    ) -> Result<ToolResult, ToolError> {
        if start_line < 1 {
            return Err(ToolError::invalid("start_line must be >= 1"));
        }
        if limit < 1 {
            return Err(ToolError::invalid("limit must be >= 1"));
        }
        let target = self.resolve(file_path)?;
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
        let take = limit.min(MAX_READ_LINES);
        let numbered = text
            .lines()
            .skip(start_line - 1)
            .take(take)
            .enumerate()
            .map(|(offset, line)| format!("{}\t{}", start_line + offset, line))
            .collect::<Vec<_>>()
            .join("\n");
        Ok(ToolResult::completed(numbered))
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
        let target = self.resolve(file_path)?;
        if let Ok(meta) = fs::metadata(&target) {
            if !meta.is_file() {
                return Err(ToolError::invalid("target path is not a file"));
            }
        }
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&target, content.as_bytes())?;
        let rel = self.relative_display(&target);
        Ok(ToolResult::completed(format!(
            "Wrote {encoded_len} bytes to {rel}."
        )))
    }

    /// Replace exact text in an existing UTF-8 file.
    ///
    /// `old_string` must be non-empty and must occur in the file. A non-unique
    /// match is rejected unless `replace_all` is set, in which case every
    /// occurrence is replaced. The resulting content must not exceed
    /// [`MAX_WRITE_BYTES`].
    pub fn edit(
        &self,
        file_path: &str,
        old_string: &str,
        new_string: &str,
        replace_all: bool,
    ) -> Result<ToolResult, ToolError> {
        if old_string.is_empty() {
            return Err(ToolError::invalid("old_string must be non-empty"));
        }
        let target = self.resolve(file_path)?;
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
        let text = String::from_utf8_lossy(&bytes).into_owned();
        let occurrences = text.matches(old_string).count();
        if occurrences == 0 {
            return Err(ToolError::invalid("old_string was not found"));
        }
        if occurrences > 1 && !replace_all {
            return Err(ToolError::invalid(
                "old_string is not unique; set replace_all=true to replace all matches",
            ));
        }

        let updated = if replace_all {
            text.replace(old_string, new_string)
        } else {
            text.replacen(old_string, new_string, 1)
        };
        if updated.len() > MAX_WRITE_BYTES {
            return Err(ToolError::invalid(
                "edited content is too large to write with this tool",
            ));
        }
        fs::write(&target, updated.as_bytes())?;
        let replaced = if replace_all { occurrences } else { 1 };
        let rel = self.relative_display(&target);
        Ok(ToolResult::completed(format!(
            "Edited {rel}; replaced {replaced} occurrence(s)."
        )))
    }

    /// List files under the workspace root matching `pattern`, sorted.
    ///
    /// An empty pattern means `**/*` (all files). Directory separators are
    /// significant (`*` does not cross `/`; `**` does). Results are bounded by
    /// `min(limit, MAX_GLOB_RESULTS)`. Symlinks are never followed, so escaping
    /// entries are never traversed or returned.
    pub fn glob(&self, pattern: &str, limit: usize) -> Result<ToolResult, ToolError> {
        if limit < 1 {
            return Err(ToolError::invalid("limit must be >= 1"));
        }
        let matcher = self.glob_matcher(pattern)?;

        let mut matches: Vec<String> = Vec::new();
        for entry in WalkDir::new(&self.root).follow_links(false) {
            let entry = match entry {
                Ok(entry) => entry,
                Err(_) => continue,
            };
            if !entry.file_type().is_file() {
                continue;
            }
            let Ok(rel) = entry.path().strip_prefix(&self.root) else {
                continue;
            };
            let rel = to_forward_slashes(rel);
            if matcher.is_match(rel.as_str()) {
                matches.push(rel);
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

    /// Search file contents under the workspace root with a regular expression.
    ///
    /// Files are selected by `path` (a glob, defaulting to all files), searched
    /// in sorted path order, and matches are returned as `path:line:text`. Files
    /// larger than [`MAX_FILE_BYTES`] are skipped. Results are bounded by
    /// `min(limit, MAX_GREP_RESULTS)`. Symlinks are never followed.
    pub fn grep(&self, pattern: &str, path: &str, limit: usize) -> Result<ToolResult, ToolError> {
        if limit < 1 {
            return Err(ToolError::invalid("limit must be >= 1"));
        }
        let matcher = self.glob_matcher(path)?;
        let regex = Regex::new(pattern)
            .map_err(|err| ToolError::invalid(format!("invalid regex: {err}")))?;

        // Gather candidate files first so the search runs in stable path order.
        let mut files: Vec<(String, PathBuf)> = Vec::new();
        for entry in WalkDir::new(&self.root).follow_links(false) {
            let entry = match entry {
                Ok(entry) => entry,
                Err(_) => continue,
            };
            if !entry.file_type().is_file() {
                continue;
            }
            let Ok(rel) = entry.path().strip_prefix(&self.root) else {
                continue;
            };
            let rel = to_forward_slashes(rel);
            if matcher.is_match(rel.as_str()) {
                files.push((rel, entry.path().to_path_buf()));
            }
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

    /// Resolve a relative path against the root, rejecting escapes.
    fn resolve(&self, relative: &str) -> Result<PathBuf, ToolError> {
        resolve_workspace_path(&self.root, relative)
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

    /// Render a resolved path relative to the root using forward slashes.
    fn relative_display(&self, target: &Path) -> String {
        target
            .strip_prefix(&self.root)
            .map(to_forward_slashes)
            .unwrap_or_else(|_| target.to_string_lossy().into_owned())
    }
}

/// Convert a path to a forward-slash string for stable, OS-independent output.
fn to_forward_slashes(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}
