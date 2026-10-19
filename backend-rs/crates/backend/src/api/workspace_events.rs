use super::{auth, error::ApiError, AppState};
use axum::{
    extract::{Request, State},
    http::{Method, StatusCode},
    middleware::Next,
    response::Response,
};
use sqlx::SqlitePool;
use uuid::Uuid;

struct Intent {
    pool: SqlitePool,
    id: Option<String>,
}
impl Drop for Intent {
    fn drop(&mut self) {
        if let Some(id) = self.id.take() {
            let pool = self.pool.clone();
            tokio::spawn(async move {
                if let Err(error) = sqlx::query("DELETE FROM workspace_event_intents WHERE id=?")
                    .bind(id)
                    .execute(&pool)
                    .await
                {
                    tracing::error!(%error,"workspace invalidation remains pending until startup recovery");
                }
            });
        }
    }
}

/// Note: the durable intent covers a process crash between filesystem mutation
/// and event publication; cancellation drops it into an independent database task.
pub async fn track(State(state): State<AppState>, request: Request, next: Next) -> Response {
    if matches!(
        *request.method(),
        Method::GET | Method::HEAD | Method::OPTIONS
    ) {
        return next.run(request).await;
    }
    let parts: Vec<_> = request
        .uri()
        .path()
        .split('/')
        .filter(|s| !s.is_empty())
        .collect();
    let target = if parts.len() >= 5
        && parts[0] == "api"
        && parts[1] == "v2"
        && matches!(parts[2], "groups" | "direct-chats")
        && (parts[4].starts_with("workspace-files") || parts[4].starts_with("workspace-git"))
    {
        Some(("file.changed", parts[3], Some(parts[3])))
    } else if parts.len() >= 5
        && parts[0] == "api"
        && parts[1] == "v2"
        && parts[2] == "skills"
        && parts[4] == "resources"
    {
        Some(("skill.changed", parts[3], None))
    } else {
        None
    };
    let Some((kind, resource, group)) = target else {
        return next.run(request).await;
    };
    let owner = match auth::current_user_id(request.headers(), &state).await {
        Ok(owner) => owner,
        Err(error) => return error.into_response(),
    };
    let owned = if let Some(group) = group {
        sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS(SELECT 1 FROM groups WHERE id=? AND owner_id=? AND status='active')",
        )
        .bind(group)
        .bind(&owner)
        .fetch_one(state.db.pool())
        .await
    } else {
        sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS(SELECT 1 FROM skills WHERE id=? AND owner_id=?)",
        )
        .bind(resource)
        .bind(&owner)
        .fetch_one(state.db.pool())
        .await
    };
    if !owned.unwrap_or(false) {
        return next.run(request).await;
    }
    let id = Uuid::new_v4().to_string();
    if sqlx::query("INSERT INTO workspace_event_intents(id,owner_id,kind,resource_id,group_id) VALUES(?,?,?,?,?)")
        .bind(&id).bind(&owner).bind(kind).bind(resource).bind(group).execute(state.db.pool()).await.is_err() {
        return ApiError::internal("unable to record workspace invalidation intent").into_response();
    }
    let mut intent = Intent {
        pool: state.db.pool().clone(),
        id: Some(id.clone()),
    };
    let response = next.run(request).await;
    match sqlx::query("DELETE FROM workspace_event_intents WHERE id=?")
        .bind(id)
        .execute(state.db.pool())
        .await
    {
        Ok(_) => {
            intent.id = None;
            response
        }
        Err(error) => {
            tracing::error!(%error,"workspace change committed but invalidation could not be confirmed");
            ApiError::new(
                StatusCode::SERVICE_UNAVAILABLE,
                "workspace_sync_unconfirmed",
                "Workspace change may have completed. Refresh before retrying.",
            )
            .into_response()
        }
    }
}

pub async fn recover(pool: &SqlitePool) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM workspace_event_intents")
        .execute(pool)
        .await?;
    Ok(())
}

use axum::response::IntoResponse;
