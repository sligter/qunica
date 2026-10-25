//! Explicit, bounded export; computer execution never mounts a host workspace.
use serde_json::{json, Value};
use uuid::Uuid;

use super::{artifacts::copy_artifact, ComputerExecution};
use crate::{
    api::{
        error::ApiError,
        workspace_files::{load_owned_local_workspace, ConversationRoot, ConversationScope},
    },
    runtime::workspace_scope::WorkspaceMode,
};

impl ComputerExecution {
    /// Publish only a caller-selected file, never a directory or the whole home.
    pub async fn publish_file(
        &self,
        source: &str,
        source_root: &str,
        target: &str,
        filename: &str,
    ) -> Result<Value, ApiError> {
        // Like remote execution, finish an accepted filesystem effect even if
        // the requesting stream disappears, and settle its durable invalidation.
        let execution = self.clone();
        let (source, source_root, target, filename) = (
            source.to_string(),
            source_root.to_string(),
            target.to_string(),
            filename.to_string(),
        );
        tokio::spawn(async move {
            execution
                .publish_file_inner(&source, &source_root, &target, &filename)
                .await
        })
        .await
        .map_err(|_| {
            ApiError::internal("file publication outcome unconfirmed; refresh before retrying")
        })?
    }

    async fn publish_file_inner(
        &self,
        source: &str,
        source_root: &str,
        target: &str,
        filename: &str,
    ) -> Result<Value, ApiError> {
        crate::tools::path_safety::reject_unsafe_relative(source).map_err(|_| {
            ApiError::invalid_input("source must be relative to the computer workspace")
        })?;
        if source.contains(['\\', '\0']) || source.starts_with('~') {
            return Err(ApiError::invalid_input(
                "source must be relative to the computer workspace",
            ));
        }
        // A single leaf avoids granting arbitrary host paths or creating parent
        // directories. persist_noclobber below also refuses existing symlinks.
        if filename.is_empty()
            || filename.len() > 200
            || filename.starts_with('.')
            || filename.contains(['/', '\\', ':'])
            || filename.chars().any(char::is_control)
        {
            return Err(ApiError::invalid_input(
                "filename must be a plain, non-hidden filename",
            ));
        }
        let (kind, mode) = self.publication_scope().await?;
        let agent_id = match target {
            "conversation" if kind == "direct" || mode.uses_group_workspace() => None,
            "self" if matches!(mode, WorkspaceMode::SelfOnly | WorkspaceMode::GroupAndSelf) => {
                Some(self.agent.as_str())
            }
            "conversation" | "self" => {
                return Err(ApiError::permission_denied(
                    "destination workspace is not shared with this Agent",
                ));
            }
            _ => {
                return Err(ApiError::invalid_input(
                    "target must be conversation or self",
                ))
            }
        };
        let scope = if kind == "direct" {
            ConversationScope::DirectChats
        } else {
            ConversationScope::Groups
        };
        let destination = ConversationRoot::from_query(scope, &self.group, &self.owner, agent_id);
        let workspace = load_owned_local_workspace(&self.pool, destination).await?;
        // Note: browser outputs belong to the computer root even on a branch
        // task. Never fall back between roots: same-named files may differ.
        let source = match source_root {
            "task" if !self.cwd.is_empty() => format!("{}/{source}", self.cwd),
            "task" | "computer" => source.to_string(),
            _ => {
                return Err(ApiError::invalid_input(
                    "source_root must be task or computer",
                ))
            }
        };
        let temporary = tempfile::NamedTempFile::new_in(&workspace.root)
            .map_err(|_| ApiError::internal("failed to stage computer file"))?;
        let mut file = tokio::fs::File::from_std(
            temporary
                .reopen()
                .map_err(|_| ApiError::internal("failed to stage computer file"))?,
        );
        let metadata = copy_artifact(&mut file, None, |offset| {
            self.call(json!({"op":"artifact","path":source,"offset":offset}))
        })
        .await?;
        file.sync_all()
            .await
            .map_err(|_| ApiError::internal("failed to flush computer file"))?;
        drop(file);
        if self.publication_scope().await? != (kind, mode) {
            return Err(ApiError::conflict(
                "workspace sharing changed during publication",
            ));
        }
        // Recheck the destination after the remote read, never silently publish
        // into a workspace that was rebound while bytes were being transferred.
        let current = load_owned_local_workspace(&self.pool, destination).await?;
        if current.workspace_id != workspace.workspace_id || current.root != workspace.root {
            return Err(ApiError::conflict(
                "destination workspace changed during publication",
            ));
        }
        let size = metadata.size;
        let sha256 = metadata.sha256;
        let intent = Uuid::new_v4().to_string();
        sqlx::query("INSERT INTO workspace_event_intents(id,owner_id,kind,resource_id,group_id) VALUES(?,?,'file.changed',?,?)")
            .bind(&intent).bind(&self.owner).bind(&self.group).bind(&self.group).execute(&self.pool).await
            .map_err(|_| ApiError::internal("unable to record file publication"))?;
        let root = current.root;
        let name = filename.to_string();
        let write = tokio::task::spawn_blocking(move || {
            temporary
                .persist_noclobber(root.join(name))
                .map_err(|error| error.error)?;
            Ok::<_, std::io::Error>(())
        })
        .await;
        sqlx::query("DELETE FROM workspace_event_intents WHERE id=?")
            .bind(intent)
            .execute(&self.pool)
            .await
            .map_err(|_| {
                ApiError::internal(
                    "file publication synchronization unconfirmed; refresh before retrying",
                )
            })?;
        match write {
            Ok(Ok(())) => {}
            Ok(Err(error)) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                return Err(ApiError::conflict(
                    "destination already exists; choose another filename",
                ));
            }
            _ => return Err(ApiError::internal("failed to publish computer file")),
        }
        Ok(
            json!({"published":true,"path":filename,"size":size,"sha256":sha256,
            "target":target,"agent_id":agent_id,"workspace_id":workspace.workspace_id,
            "conversation_id":self.group,"scope":scope.route_segment(),
            "message":"Published a copy into the UI workspace. The source remains on the computer; other workspaces are not synchronized."}),
        )
    }

    async fn publication_scope(&self) -> Result<(String, WorkspaceMode), ApiError> {
        let row: Option<(String, Option<String>)> = sqlx::query_as(
            "SELECT g.conversation_kind,ga.context_scope_json FROM groups g \
             JOIN group_agents ga ON ga.group_id=g.id JOIN agents a ON a.id=ga.agent_id \
             JOIN group_turns t ON t.group_id=g.id \
             WHERE g.id=? AND g.owner_id=? AND g.status='active' AND ga.agent_id=? \
             AND ga.status='active' AND a.owner_id=? AND a.status='active' AND t.id=? \
             AND t.status='running' AND COALESCE(a.execution_environment_id,g.execution_environment_id)=?",
        ).bind(&self.group).bind(&self.owner).bind(&self.agent).bind(&self.owner).bind(&self.turn).bind(&self.id)
            .fetch_optional(&self.pool).await.map_err(|_| ApiError::internal("publication authorization unavailable"))?;
        let (kind, raw) = row.ok_or_else(|| {
            ApiError::permission_denied("computer publication authorization is no longer active")
        })?;
        Ok((kind, WorkspaceMode::from_context_scope(raw.as_deref())))
    }
}
