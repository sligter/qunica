//! Workspace path safety.
//!
//! [`resolve_workspace_path`] is the single gate every file tool passes a
//! caller-supplied path through. It is strict about Windows drive and UNC
//! prefixes on every host so the same rules hold regardless of where the
//! backend runs.

use std::{
    fs, io,
    path::{Path, PathBuf},
};

use super::ToolError;

/// Resolve a caller-supplied `relative` path against a workspace `root`,
/// rejecting anything that could read or write outside the root.
///
/// The returned path is rooted at the canonicalized workspace root, so callers
/// can use it for filesystem operations directly. A path is rejected when:
///
/// - `root` is empty or is not an existing directory;
/// - `relative` is empty or only whitespace;
/// - `relative` is absolute (`/x`), or starts with a separator (`\x`, UNC
///   `\\host\share`, `//host`);
/// - `relative` carries a Windows drive prefix (`C:\x`, `c:foo`) — rejected on
///   every host, not just Windows;
/// - any component is `..` (parent escape) or exactly `~` (home expansion);
/// - after canonicalizing the deepest existing ancestor (which follows any
///   symlinks), the result would fall outside the canonical root. This catches
///   symlinks that point outside the workspace and existing parents that
///   resolve out of root.
///
/// The target itself need not exist (so `Write` can create new files); only the
/// existing prefix is canonicalized, with the not-yet-created tail appended.
pub fn resolve_workspace_path(root: &Path, relative: &str) -> Result<PathBuf, ToolError> {
    if root.as_os_str().is_empty() {
        return Err(ToolError::invalid(
            "workspace root must be a non-empty existing directory",
        ));
    }
    let canonical_root = match fs::canonicalize(root) {
        Ok(path) if path.is_dir() => path,
        _ => {
            return Err(ToolError::invalid(
                "workspace root must be an existing directory",
            ))
        }
    };

    reject_unsafe_relative(relative)?;

    let joined = canonical_root.join(Path::new(relative));
    let resolved = canonicalize_existing_prefix(&joined)?;
    if !resolved.starts_with(&canonical_root) {
        return Err(ToolError::invalid(
            "path must stay inside the workspace root",
        ));
    }
    Ok(resolved)
}

/// Reject a relative path string that is unsafe by inspection alone, before any
/// filesystem access. Shared with glob/grep pattern validation.
pub(crate) fn reject_unsafe_relative(value: &str) -> Result<(), ToolError> {
    if value.trim().is_empty() {
        return Err(ToolError::invalid("path must be a non-empty relative path"));
    }

    // Windows drive prefix such as "C:\\tmp" or "c:foo" — checked explicitly so
    // it is rejected even on non-Windows hosts, where the OS would otherwise
    // treat "C:foo" as an ordinary relative filename.
    let mut chars = value.chars();
    if matches!(
        (chars.next(), chars.next()),
        (Some(drive), Some(':')) if drive.is_ascii_alphabetic()
    ) {
        return Err(ToolError::invalid(
            "path must be relative to the workspace root (drive paths are not allowed)",
        ));
    }

    // A leading separator means a POSIX-absolute path ("/x"), a Windows root
    // ("\\x"), or a UNC path ("\\\\host\\share", "//host/share").
    if value.starts_with('/') || value.starts_with('\\') {
        return Err(ToolError::invalid(
            "path must be relative to the workspace root (absolute and UNC paths are not allowed)",
        ));
    }

    // Inspect components under both separators so a Windows-style segment is
    // caught even on a POSIX host (and vice versa).
    for segment in value.split(['/', '\\']) {
        if segment == ".." {
            return Err(ToolError::invalid(
                "path must stay inside the workspace root (`..` segments are not allowed)",
            ));
        }
        if segment == "~" {
            return Err(ToolError::invalid(
                "path must not use home-directory expansion",
            ));
        }
    }

    Ok(())
}

/// Canonicalize the deepest existing ancestor of `path` and re-append the
/// not-yet-existing tail, returning a fully canonical path for the existing
/// part. Symlinks in the existing portion are followed, so an escape via a
/// symlinked parent surfaces as a resolved path outside the root.
fn canonicalize_existing_prefix(path: &Path) -> Result<PathBuf, ToolError> {
    match fs::canonicalize(path) {
        Ok(canonical) => Ok(canonical),
        Err(err) if err.kind() == io::ErrorKind::NotFound => {
            match (path.parent(), path.file_name()) {
                (Some(parent), Some(name)) => {
                    let canonical_parent = canonicalize_existing_prefix(parent)?;
                    Ok(canonical_parent.join(name))
                }
                // Nothing left to peel (e.g. a bare root component); hand the
                // path back unchanged and let the containment check decide.
                _ => Ok(path.to_path_buf()),
            }
        }
        Err(err) => Err(ToolError::Io(err)),
    }
}
