//! Notes body file lifecycle, shared independently of HTTP and SQLite.
//!
//! The caller owns authorization and COMMIT ordering. This module only prepares
//! or publishes files; it never acquires a pool connection or retries a write.
//! See specs/notes-failure-contract.md for the retained failure boundaries.

use crate::{
    api::error::ApiError,
    tools::{resolve_workspace_path, ToolError},
};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{self, Read, Seek, SeekFrom, Write},
    path::{Path as FsPath, PathBuf},
};
use tempfile::NamedTempFile;

pub(crate) const NOTES_DIR: &str = "Notes";
pub(crate) const NOTE_FILE_SUFFIX: &str = ".md";

fn note_relative_path(note_id: &str) -> String {
    format!("{NOTES_DIR}/{note_id}{NOTE_FILE_SUFFIX}")
}

pub(crate) fn group_note_path(root: &FsPath, note_id: &str) -> Result<PathBuf, ApiError> {
    resolve_workspace_path(root, &note_relative_path(note_id)).map_err(path_safety_error)
}

fn group_note_literal_path(root: &FsPath, note_id: &str) -> PathBuf {
    root.join(note_relative_path(note_id))
}

pub(crate) enum GroupNoteFileState {
    Missing,
    File,
}

pub(crate) fn inspect_group_note_file(
    root: &FsPath,
    note_id: &str,
) -> Result<GroupNoteFileState, ApiError> {
    match fs::symlink_metadata(group_note_literal_path(root, note_id)) {
        Ok(metadata) => {
            if metadata.file_type().is_symlink() {
                return Err(ApiError::invalid_input(
                    "group note path must not be a symlink",
                ));
            }
            if !metadata.is_file() {
                return Err(ApiError::invalid_input("group note path is not a file"));
            }
            Ok(GroupNoteFileState::File)
        }
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(GroupNoteFileState::Missing),
        Err(_) => Err(ApiError::invalid_input("group note path is invalid")),
    }
}

pub(crate) fn read_group_note_content(
    root: &FsPath,
    note_id: &str,
    fallback: &str,
) -> Result<String, ApiError> {
    let path = group_note_path(root, note_id)?;
    match inspect_group_note_file(root, note_id)? {
        GroupNoteFileState::Missing => Ok(fallback.to_string()),
        GroupNoteFileState::File => fs::read_to_string(path)
            .map_err(|_| ApiError::invalid_input("group note is not valid UTF-8")),
    }
}

pub(crate) fn stage_group_note_create(
    root: &FsPath,
    note_id: &str,
    content: &str,
) -> Result<NamedTempFile, ApiError> {
    let path = group_note_path(root, note_id)?;
    let parent = path
        .parent()
        .ok_or_else(|| ApiError::invalid_input("group notes path is invalid"))?;
    fs::create_dir_all(parent)
        .map_err(|_| ApiError::invalid_input("group notes path is not a directory"))?;
    let path = group_note_path(root, note_id)?;
    let parent = path
        .parent()
        .ok_or_else(|| ApiError::invalid_input("group notes path is invalid"))?;
    if matches!(
        inspect_group_note_file(root, note_id)?,
        GroupNoteFileState::File
    ) {
        return Err(ApiError::conflict("new group note path already exists"));
    }
    // Use fs::write's standard creation permissions, including the Unix umask.
    let mut staged = tempfile::Builder::new()
        .make_in(parent, |path| {
            fs::OpenOptions::new()
                .read(true)
                .write(true)
                .create_new(true)
                .open(path)
        })
        .map_err(|_| ApiError::internal("failed to stage group note content"))?;
    staged
        .write_all(content.as_bytes())
        .map_err(|_| ApiError::internal("failed to write group note content"))?;
    Ok(staged)
}

pub(crate) fn publish_group_note_create(
    root: &FsPath,
    note_id: &str,
    staged: NamedTempFile,
) -> Result<(), ApiError> {
    let path = group_note_path(root, note_id)?;
    inspect_group_note_file(root, note_id)?;
    staged
        .persist_noclobber(path)
        .map(|_| ())
        .map_err(|_| ApiError::internal("failed to publish group note content"))
}

pub(crate) fn delete_group_note_content(root: &FsPath, note_id: &str) -> Result<(), ApiError> {
    let path = group_note_path(root, note_id)?;
    match inspect_group_note_file(root, note_id)? {
        GroupNoteFileState::Missing => Ok(()),
        GroupNoteFileState::File => fs::remove_file(path)
            .map_err(|_| ApiError::internal("failed to delete group note content")),
    }
}

fn path_safety_error(err: ToolError) -> ApiError {
    match err {
        ToolError::Invalid(message) => ApiError::invalid_input(message),
        ToolError::Io(_) => ApiError::invalid_input("group note path is invalid"),
    }
}

pub(crate) struct PreparedUpdate<'a> {
    root: &'a FsPath,
    note_id: &'a str,
    content: &'a str,
    previous: PreviousFile,
}

enum PreviousFile {
    Missing(NamedTempFile),
    Existing {
        identity: same_file::Handle,
        digest: [u8; 32],
    },
}

pub(crate) fn changed() -> ApiError {
    ApiError::conflict("group note changed; read it again before editing")
}

fn file_error(_: io::Error) -> ApiError {
    ApiError::internal("failed to write group note content")
}

// Raw bytes preserve Notes' existing acceptance of NUL and large bodies. This
// deliberately does not inherit Workspace text-save's size/encoding policy.
fn digest(file: &mut fs::File) -> Result<[u8; 32], ApiError> {
    file.seek(SeekFrom::Start(0)).map_err(file_error)?;
    let mut hash = Sha256::new();
    let mut buffer = [0; 8192];
    loop {
        let count = file.read(&mut buffer).map_err(file_error)?;
        if count == 0 {
            return Ok(hash.finalize().into());
        }
        hash.update(&buffer[..count]);
    }
}

pub(crate) fn prepare<'a>(
    root: &'a FsPath,
    note_id: &'a str,
    content: &'a str,
) -> Result<PreparedUpdate<'a>, ApiError> {
    let path = group_note_path(root, note_id)?;
    let previous = match inspect_group_note_file(root, note_id)? {
        GroupNoteFileState::Missing => {
            PreviousFile::Missing(stage_group_note_create(root, note_id, content)?)
        }
        GroupNoteFileState::File => {
            // Preserve pre-commit rejection for an unwritable target, without
            // create/truncate. Retain only a read handle while COMMIT awaits.
            let writable = fs::OpenOptions::new()
                .read(true)
                .write(true)
                .open(&path)
                .map_err(file_error)?;
            let identity = same_file::Handle::from_path(&path).map_err(file_error)?;
            if identity != same_file::Handle::from_file(writable).map_err(file_error)? {
                return Err(changed());
            }
            let digest = digest(&mut identity.as_file().try_clone().map_err(file_error)?)?;
            PreviousFile::Existing { identity, digest }
        }
    };
    Ok(PreparedUpdate {
        root,
        note_id,
        content,
        previous,
    })
}

impl PreparedUpdate<'_> {
    pub(crate) fn root(&self) -> &FsPath {
        self.root
    }

    pub(crate) fn note_id(&self) -> &str {
        self.note_id
    }

    pub(crate) fn content(&self) -> &str {
        self.content
    }

    pub(crate) fn check_expected(&self, expected: &str) -> Result<(), ApiError> {
        if let PreviousFile::Existing { digest, .. } = &self.previous {
            let expected: [u8; 32] = Sha256::digest(expected.as_bytes()).into();
            if *digest != expected {
                return Err(changed());
            }
        }
        Ok(())
    }

    pub(crate) fn publish(self) -> Result<(), ApiError> {
        let path = group_note_path(self.root, self.note_id)?;
        inspect_group_note_file(self.root, self.note_id)?;
        match self.previous {
            PreviousFile::Missing(staged) => {
                publish_group_note_create(self.root, self.note_id, staged)
            }
            PreviousFile::Existing {
                identity,
                digest: expected_digest,
            } => {
                // Reopen to respect permissions changed while COMMIT awaited.
                // Write the checked handle in place to preserve ACL/hard links.
                let mut file = fs::OpenOptions::new()
                    .read(true)
                    .write(true)
                    .open(&path)
                    .map_err(file_error)?;
                let current = same_file::Handle::from_file(file.try_clone().map_err(file_error)?)
                    .map_err(file_error)?;
                if current != identity || digest(&mut file)? != expected_digest {
                    return Err(changed());
                }
                let checked_path = group_note_path(self.root, self.note_id)?;
                inspect_group_note_file(self.root, self.note_id)?;
                if path != checked_path
                    || current != same_file::Handle::from_path(&checked_path).map_err(file_error)?
                {
                    return Err(changed());
                }
                file.seek(SeekFrom::Start(0)).map_err(file_error)?;
                file.write_all(self.content.as_bytes())
                    .map_err(file_error)?;
                file.set_len(self.content.len() as u64)
                    .map_err(file_error)?;
                // Refresh identity after writing: Windows Handle equality also
                // caches size. A final name change is pending, never retried.
                let written = same_file::Handle::from_file(file).map_err(file_error)?;
                let checked_path = group_note_path(self.root, self.note_id)?;
                inspect_group_note_file(self.root, self.note_id)?;
                if written != same_file::Handle::from_path(checked_path).map_err(file_error)? {
                    return Err(changed());
                }
                Ok(())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    #[test]
    fn note_create_publication_never_overwrites_a_newer_target() {
        for kind in ["file", "directory"] {
            let root = tempfile::tempdir().unwrap();
            let id = Uuid::new_v4().to_string();
            let staged = stage_group_note_create(root.path(), &id, "staged body").unwrap();
            let path = group_note_literal_path(root.path(), &id);
            if kind == "file" {
                fs::write(&path, "newer body").unwrap();
            } else {
                fs::create_dir(&path).unwrap();
            }
            assert!(publish_group_note_create(root.path(), &id, staged).is_err());
            if kind == "file" {
                assert_eq!(fs::read_to_string(&path).unwrap(), "newer body");
            } else {
                assert!(path.is_dir());
            }
            assert_eq!(fs::read_dir(root.path().join("Notes")).unwrap().count(), 1);
        }
    }

    #[cfg(unix)]
    #[test]
    fn note_create_publication_keeps_standard_creation_permissions() {
        use std::os::unix::fs::PermissionsExt;
        let root = tempfile::tempdir().unwrap();
        let control = root.path().join("control.md");
        fs::write(&control, "control").unwrap();
        let id = Uuid::new_v4().to_string();
        let staged = stage_group_note_create(root.path(), &id, "body").unwrap();
        publish_group_note_create(root.path(), &id, staged).unwrap();
        assert_eq!(
            fs::metadata(control).unwrap().permissions().mode() & 0o777,
            fs::metadata(group_note_literal_path(root.path(), &id))
                .unwrap()
                .permissions()
                .mode()
                & 0o777
        );
    }

    #[test]
    fn note_reads_prefer_file_bytes_and_fall_back_only_when_missing() {
        let root = tempfile::tempdir().unwrap();
        let id = Uuid::new_v4().to_string();
        fs::create_dir(root.path().join("Notes")).unwrap();
        let path = group_note_literal_path(root.path(), &id);
        assert_eq!(
            read_group_note_content(root.path(), &id, "registered body").unwrap(),
            "registered body"
        );
        fs::write(&path, "external\0body").unwrap();
        assert_eq!(
            read_group_note_content(root.path(), &id, "registered body").unwrap(),
            "external\0body"
        );
        fs::remove_file(path).unwrap();
        assert_eq!(
            read_group_note_content(root.path(), &id, "registered body").unwrap(),
            "registered body"
        );
    }

    #[test]
    fn note_reads_reject_invalid_or_non_file_bodies_without_fallback() {
        let root = tempfile::tempdir().unwrap();
        let id = Uuid::new_v4().to_string();
        fs::create_dir(root.path().join("Notes")).unwrap();
        let path = group_note_literal_path(root.path(), &id);
        fs::write(&path, [0xff]).unwrap();
        let error = read_group_note_content(root.path(), &id, "fallback").unwrap_err();
        assert_eq!(error.code(), "invalid_input");
        assert_eq!(error.message_text(), "group note is not valid UTF-8");
        assert_eq!(fs::read(&path).unwrap(), [0xff]);
        fs::remove_file(&path).unwrap();
        fs::create_dir(&path).unwrap();
        let error = read_group_note_content(root.path(), &id, "fallback").unwrap_err();
        assert_eq!(error.code(), "invalid_input");
        assert!(path.is_dir());
    }

    #[test]
    fn note_update_preparation_and_drop_never_publish_a_body() {
        for missing in [false, true] {
            let root = tempfile::tempdir().unwrap();
            let id = Uuid::new_v4().to_string();
            fs::create_dir(root.path().join("Notes")).unwrap();
            let path = group_note_literal_path(root.path(), &id);
            if !missing {
                fs::write(&path, "original").unwrap();
            }
            let prepared = prepare(root.path(), &id, "proposed\0body").unwrap();
            if missing {
                assert!(!path.exists());
                assert_eq!(fs::read_dir(root.path().join("Notes")).unwrap().count(), 1);
            } else {
                assert_eq!(fs::read_to_string(&path).unwrap(), "original");
                fs::write(&path, "external edit").unwrap();
            }
            drop(prepared);
            if missing {
                assert!(!path.exists());
                assert_eq!(fs::read_dir(root.path().join("Notes")).unwrap().count(), 0);
            } else {
                assert_eq!(fs::read_to_string(&path).unwrap(), "external edit");
            }
        }
    }
}
