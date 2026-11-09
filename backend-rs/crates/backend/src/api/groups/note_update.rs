//! The accepted SQLite mutation and the file publication are separate outcomes.
//! See specs/notes-failure-contract.md for failure and external-writer boundaries.
use super::*;
use sha2::{Digest, Sha256};
use std::io::{Read, Seek, SeekFrom};

pub(super) struct PreparedUpdate<'a> {
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

fn changed() -> ApiError {
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

pub(super) fn prepare<'a>(
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
    pub(super) fn check_expected(&self, expected: &str) -> Result<(), ApiError> {
        if let PreviousFile::Existing { digest, .. } = &self.previous {
            let expected: [u8; 32] = Sha256::digest(expected.as_bytes()).into();
            if *digest != expected {
                return Err(changed());
            }
        }
        Ok(())
    }

    fn publish(self) -> Result<(), ApiError> {
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

#[derive(sqlx::FromRow)]
struct PublicationRow {
    content: String,
    group_owner_id: String,
    workspace_id: Option<String>,
    #[sqlx(flatten)]
    workspace: GroupNoteWorkspaceRow,
}

pub(super) async fn publish(
    pool: &SqlitePool,
    actor_id: &str,
    group_id: &str,
    expected_owner_id: &str,
    expected_workspace_id: Option<&str>,
    prepared: PreparedUpdate<'_>,
    guard: Option<GroupNoteWriteGuard<'_>>,
) -> Result<(), ApiError> {
    // Note: a second short writer barrier serializes publication with API
    // update/delete, without another mutation/event. Every awaited read uses
    // this connection, including single-connection in-memory pools.
    let mut tx = crate::db::begin_write(pool)
        .await
        .map_err(|_| ApiError::internal("failed to start group note publication transaction"))?;
    let row = sqlx::query_as::<_, PublicationRow>(
        "SELECT n.content, g.owner_id AS group_owner_id, g.workspace_id, \
         w.owner_id, w.backend_type, w.local_path, w.status \
         FROM group_notes n JOIN groups g ON g.id = n.group_id \
         JOIN workspaces w ON w.id = g.workspace_id \
         WHERE n.id = ? AND n.group_id = ? AND n.status = 'active' AND g.status = 'active'",
    )
    .bind(prepared.note_id)
    .bind(group_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|_| ApiError::internal("failed to check group note publication"))?
    .ok_or_else(changed)?;
    if row.group_owner_id != expected_owner_id
        || row.workspace_id.as_deref() != expected_workspace_id
        || row.content != prepared.content
    {
        return Err(changed());
    }
    if row.group_owner_id != actor_id {
        check_group_note_membership(&mut *tx, group_id, actor_id).await?;
    }
    let root = validate_group_note_workspace_root(row.workspace, &row.group_owner_id)?;
    if root != prepared.root {
        return Err(changed());
    }
    // This check must follow all awaits; an expired MCP lease cannot start I/O.
    check_note_write_guard(guard)?;
    prepared.publish()?;
    tx.rollback()
        .await
        .map_err(|_| ApiError::internal("failed to release group note publication transaction"))
}
