//! Owner-scoped resources and explicit Team/Private execution bindings.
use super::{auth::current_user_id, error::ApiError, AppState};
use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    Json,
};
use reqwest::Method;
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Create {
    id: Uuid,
    name: String,
    binding: Binding,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Binding {
    kind: String,
    id: Uuid,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Bind {
    scope: String,
    target_id: Uuid,
    execution_environment_id: Option<Uuid>,
}

fn configured(state: &AppState) -> Result<&crate::computers::SupervisorClient, ApiError> {
    state.computers.as_deref().ok_or_else(|| {
        ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "computer_not_configured",
            "Computer Supervisor is not configured",
        )
    })
}
pub(super) async fn owned(state: &AppState, owner: &str, id: &str) -> Result<(), ApiError> {
    let valid: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM computers WHERE id=? AND owner_id=?)")
            .bind(id)
            .bind(owner)
            .fetch_one(state.db.pool())
            .await
            .map_err(db)?;
    if !valid {
        return Err(ApiError::not_found("computer not found"));
    }
    Ok(())
}
async fn target_owned(state: &AppState, owner: &str, kind: &str, id: &str) -> Result<(), ApiError> {
    let query = match kind {
        "team" => {
            "SELECT EXISTS(SELECT 1 FROM groups WHERE id=? AND owner_id=? AND status='active')"
        }
        "private" => {
            "SELECT EXISTS(SELECT 1 FROM agents WHERE id=? AND owner_id=? AND status='active')"
        }
        _ => {
            return Err(ApiError::invalid_input(
                "binding kind must be team or private",
            ))
        }
    };
    let valid: bool = sqlx::query_scalar(query)
        .bind(id)
        .bind(owner)
        .fetch_one(state.db.pool())
        .await
        .map_err(db)?;
    if !valid {
        return Err(ApiError::not_found("binding target not found"));
    }
    Ok(())
}

pub async fn create(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<Create>,
) -> Result<(StatusCode, Json<Value>), ApiError> {
    let owner = current_user_id(&headers, &state).await?;
    let client = configured(&state)?;
    if body.name.trim().is_empty() || body.name.len() > 120 {
        return Err(ApiError::invalid_input(
            "computer name must contain 1–120 bytes",
        ));
    }
    let id = body.id.to_string();
    let binding_id = body.binding.id.to_string();
    target_owned(&state, &owner, &body.binding.kind, &binding_id).await?;
    // Persist the idempotent intent before provisioning, without holding a
    // SQLite writer during a Docker action. A failed response is read back.
    sqlx::query("INSERT INTO computers(id,owner_id,binding_kind,binding_id,name,created_at) VALUES(?,?,?,?,?,?) ON CONFLICT(id) DO NOTHING").bind(&id).bind(&owner).bind(&body.binding.kind).bind(&binding_id).bind(&body.name).bind(super::commands::now()).execute(state.db.pool()).await.map_err(db)?;
    let same:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM computers WHERE id=? AND owner_id=? AND binding_kind=? AND binding_id=? AND name=?)").bind(&id).bind(&owner).bind(&body.binding.kind).bind(&binding_id).bind(&body.name).fetch_one(state.db.pool()).await.map_err(db)?;
    if !same {
        return Err(ApiError::conflict(
            "computer id already used with a different specification",
        ));
    }
    let value=client.request(Method::POST,"/v1/computers",Some(&json!({"id":id,"owner_id":owner,"binding":{"kind":body.binding.kind,"id":binding_id},"name":body.name}))).await?;
    Ok((StatusCode::CREATED, Json(value)))
}
pub async fn list(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Value>, ApiError> {
    let owner = current_user_id(&headers, &state).await?;
    let rows:Vec<(String,String,String,String,bool)>=sqlx::query_as("SELECT c.id,c.name,c.binding_kind,c.binding_id,EXISTS(SELECT 1 FROM groups g WHERE g.execution_environment_id=c.id) OR EXISTS(SELECT 1 FROM agents a WHERE a.execution_environment_id=c.id) FROM computers c WHERE c.owner_id=? ORDER BY c.created_at,c.id").bind(owner).fetch_all(state.db.pool()).await.map_err(db)?;
    Ok(Json(
        json!({"configured":state.computers.is_some(),"items":rows.into_iter().map(|(id,name,kind,target,selected)|json!({"id":id,"name":name,"binding":{"kind":kind,"id":target},"selected":selected})).collect::<Vec<_>>()}),
    ))
}
pub async fn get(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    let owner = current_user_id(&headers, &state).await?;
    let id = id.to_string();
    owned(&state, &owner, &id).await?;
    let client = configured(&state)?;
    let mut value = client
        .request(Method::GET, &format!("/v1/computers/{id}"), None)
        .await?;
    value["capabilities"] = client.request(Method::GET, "/v1/health", None).await?;
    Ok(Json(value))
}
pub async fn action(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Json(body): Json<Value>,
) -> Result<Json<Value>, ApiError> {
    let owner = current_user_id(&headers, &state).await?;
    let id = id.to_string();
    owned(&state, &owner, &id).await?;
    let result = configured(&state)?
        .request(
            Method::POST,
            &format!("/v1/computers/{id}/actions"),
            Some(&body),
        )
        .await;
    sqlx::query("UPDATE computers SET revision=revision+1 WHERE id=? AND owner_id=?")
        .bind(&id)
        .bind(&owner)
        .execute(state.db.pool())
        .await
        .map_err(db)?;
    Ok(Json(result?))
}
pub async fn bind(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<Bind>,
) -> Result<Json<Value>, ApiError> {
    let owner = current_user_id(&headers, &state).await?;
    let (kind, table) = match body.scope.as_str() {
        "group" => ("team", "groups"),
        "agent" => ("private", "agents"),
        _ => return Err(ApiError::invalid_input("scope must be group or agent")),
    };
    let target = body.target_id.to_string();
    let _guard = state.write_lock.lock().await;
    let mut tx = state.db.begin_write().await.map_err(db)?;
    let valid: bool = sqlx::query_scalar(&format!(
        "SELECT EXISTS(SELECT 1 FROM {table} WHERE id=? AND owner_id=? AND status='active')"
    ))
    .bind(&target)
    .bind(&owner)
    .fetch_one(&mut *tx)
    .await
    .map_err(db)?;
    if !valid {
        return Err(ApiError::not_found("binding target not found"));
    }
    if let Some(id) = body.execution_environment_id {
        configured(&state)?;
        let valid:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM computers WHERE id=? AND owner_id=? AND binding_kind=? AND binding_id=?)").bind(id.to_string()).bind(&owner).bind(kind).bind(&target).fetch_one(&mut *tx).await.map_err(db)?;
        if !valid {
            return Err(ApiError::invalid_input(
                "computer does not belong to this binding target",
            ));
        }
        let acp: bool = if table == "agents" {
            sqlx::query_scalar("SELECT runtime_kind='acp' AND COALESCE(json_extract(external_runtime_json,'$.profile'),'custom')!='opencode' FROM agents WHERE id=?")
                .bind(&target)
                .fetch_one(&mut *tx)
                .await
                .map_err(db)?
        } else {
            sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM group_agents ga JOIN agents a ON a.id=ga.agent_id WHERE ga.group_id=? AND a.runtime_kind='acp' AND COALESCE(json_extract(a.external_runtime_json,'$.profile'),'custom')!='opencode')").bind(&target).fetch_one(&mut *tx).await.map_err(db)?
        };
        if acp {
            return Err(ApiError::invalid_input("ACP computer execution requires a verified model proxy preset; host fallback is disabled"));
        }
    }
    let busy: bool = if table == "groups" {
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM execution_jobs WHERE group_id=? AND (worker_active=1 OR occupies_group=1 OR (status='interrupted' AND COALESCE(error,'')!='effects_verified_stopped')))").bind(&target).fetch_one(&mut *tx).await.map_err(db)?
    } else {
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM group_agents ga JOIN execution_jobs j ON j.group_id=ga.group_id WHERE ga.agent_id=? AND (j.worker_active=1 OR j.occupies_group=1 OR (j.status='interrupted' AND COALESCE(j.error,'')!='effects_verified_stopped')))").bind(&target).fetch_one(&mut *tx).await.map_err(db)?
    };
    if busy {
        return Err(ApiError::conflict(
            "stop active or waiting work before changing its execution environment",
        ));
    }
    sqlx::query(&format!(
        "UPDATE {table} SET execution_environment_id=?,updated_at=? WHERE id=? AND owner_id=?"
    ))
    .bind(body.execution_environment_id.map(|id| id.to_string()))
    .bind(super::commands::now())
    .bind(&target)
    .bind(&owner)
    .execute(&mut *tx)
    .await
    .map_err(db)?;
    tx.commit().await.map_err(db)?;
    Ok(Json(
        json!({"scope":body.scope,"target_id":target,"execution_environment_id":body.execution_environment_id}),
    ))
}
fn db(_: sqlx::Error) -> ApiError {
    ApiError::internal("computer storage unavailable")
}
