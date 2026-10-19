use anyhow::{anyhow, Context};
use axum::{
    body::Body,
    extract::{Path, Request, State},
    http::{HeaderMap, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
    Json,
};
use futures_util::StreamExt;
use jsonwebtoken::{decode, encode, Algorithm, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use time::{format_description::well_known::Rfc3339, OffsetDateTime};
use uuid::Uuid;

use crate::api::{
    agents::{double_option, normalize_avatar_url},
    error::ApiError,
    AppState,
};
use crate::config::InitialUserConfig;

#[derive(Debug, Deserialize)]
pub struct RegisterRequest {
    pub email: String,
    pub password: String,
    pub name: String,
}

#[derive(Debug, Deserialize)]
pub struct LoginRequest {
    pub email: String,
    pub password: String,
    #[serde(default)]
    pub device_label: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct CreateSessionRequest {
    pub device_label: String,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct SessionResponse {
    pub id: String,
    pub device_label: String,
    pub created_at: String,
    pub revoked_at: Option<String>,
    pub current: bool,
}

#[derive(Debug, Clone)]
pub struct AuthenticatedSession {
    pub owner_id: String,
    pub session_id: String,
    pub(crate) credential_id: String,
}

#[derive(Debug, Deserialize)]
pub struct UpdateMeRequest {
    #[serde(default)]
    name: Option<String>,
    #[serde(default, deserialize_with = "double_option")]
    avatar_url: Option<Option<String>>,
}

#[derive(Debug, Serialize)]
pub struct UserResponse {
    pub id: String,
    pub email: String,
    pub name: String,
    pub avatar_url: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Serialize)]
pub struct TokenResponse {
    pub access_token: String,
    pub token_type: String,
}

#[derive(Debug, Serialize)]
pub struct PublicAuthConfig {
    pub registration_enabled: bool,
}

#[derive(Debug, Serialize, Deserialize)]
struct Claims {
    sub: String,
    #[serde(default)]
    session_id: Option<String>,
    #[serde(default)]
    jti: Option<String>,
}

#[derive(Debug, sqlx::FromRow)]
struct UserRow {
    id: String,
    email: String,
    password_hash: String,
    name: String,
    avatar_url: Option<String>,
    created_at: String,
}

impl From<UserRow> for UserResponse {
    fn from(row: UserRow) -> Self {
        Self {
            id: row.id,
            email: row.email,
            name: row.name,
            avatar_url: row.avatar_url,
            created_at: row.created_at,
        }
    }
}

pub async fn initialize_auth(
    pool: &SqlitePool,
    registration_enabled: bool,
    initial_user: Option<&InitialUserConfig>,
) -> anyhow::Result<()> {
    let user_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users")
        .fetch_one(pool)
        .await
        .context("failed to count users")?;
    if user_count > 0 {
        return Ok(());
    }

    let Some(initial_user) = initial_user else {
        if registration_enabled {
            return Ok(());
        }
        anyhow::bail!(
            "registration is disabled and the database has no users; configure QUNICA_INITIAL_USER_EMAIL and QUNICA_INITIAL_USER_PASSWORD"
        );
    };

    let email = normalize_email(&initial_user.email);
    let name = normalize_name(&initial_user.name)
        .map_err(|error| anyhow!("invalid initial user: {}", error.message_text()))?;
    validate_register(&email, &initial_user.password)
        .map_err(|error| anyhow!("invalid initial user: {}", error.message_text()))?;
    let password_hash = bcrypt::hash(&initial_user.password, bcrypt::DEFAULT_COST)
        .context("failed to hash initial user password")?;
    let id = Uuid::new_v4().to_string();
    let now = now_rfc3339();

    let result = sqlx::query(
        "INSERT INTO users (id, email, password_hash, name, avatar_url, created_at, updated_at) \
         SELECT ?, ?, ?, ?, NULL, ?, ? WHERE NOT EXISTS (SELECT 1 FROM users)",
    )
    .bind(&id)
    .bind(&email)
    .bind(&password_hash)
    .bind(&name)
    .bind(&now)
    .bind(&now)
    .execute(pool)
    .await
    .context("failed to create initial user")?;

    if result.rows_affected() == 1 {
        tracing::info!("created initial user account");
    }
    Ok(())
}

pub async fn public_config(State(state): State<AppState>) -> Json<PublicAuthConfig> {
    Json(PublicAuthConfig {
        registration_enabled: state.auth.registration_enabled,
    })
}

pub async fn register(
    State(state): State<AppState>,
    Json(body): Json<RegisterRequest>,
) -> Result<(StatusCode, Json<UserResponse>), ApiError> {
    if !state.auth.registration_enabled {
        return Err(ApiError::new(
            StatusCode::FORBIDDEN,
            "registration_disabled",
            "registration is disabled",
        ));
    }
    let email = normalize_email(&body.email);
    let name = normalize_name(&body.name)?;
    validate_register(&email, &body.password)?;

    if find_user_by_email(state.db.pool(), &email).await?.is_some() {
        return Err(ApiError::conflict("user already exists"));
    }

    let password_hash = bcrypt::hash(&body.password, bcrypt::DEFAULT_COST)
        .map_err(|_| ApiError::internal("failed to hash password"))?;
    let id = Uuid::new_v4().to_string();
    let now = now_rfc3339();

    let result = sqlx::query(
        "INSERT INTO users (id, email, password_hash, name, avatar_url, created_at, updated_at) \
         VALUES (?, ?, ?, ?, NULL, ?, ?)",
    )
    .bind(&id)
    .bind(&email)
    .bind(&password_hash)
    .bind(&name)
    .bind(&now)
    .bind(&now)
    .execute(state.db.pool())
    .await;

    if let Err(err) = result {
        // Guard against a race where two registrations slip past the check above.
        if is_unique_violation(&err) {
            return Err(ApiError::conflict("user already exists"));
        }
        return Err(ApiError::internal("failed to create user"));
    }

    let user = find_user_by_id(state.db.pool(), &id)
        .await?
        .ok_or_else(|| ApiError::internal("user vanished after insert"))?;
    Ok((StatusCode::CREATED, Json(user.into())))
}

pub async fn login(
    State(state): State<AppState>,
    Json(body): Json<LoginRequest>,
) -> Result<Json<TokenResponse>, ApiError> {
    let email = normalize_email(&body.email);
    if email.is_empty() || body.password.is_empty() {
        return Err(ApiError::invalid_input("email and password are required"));
    }

    let user = find_user_by_email(state.db.pool(), &email)
        .await?
        .filter(|u| verify_password(&body.password, &u.password_hash))
        .ok_or_else(|| ApiError::permission_denied("invalid credentials"))?;

    let label = normalize_device_label(body.device_label.as_deref().unwrap_or("Device"))?;
    Ok(Json(issue_session(&state, &user.id, &label, None).await?))
}

pub async fn list_sessions(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Vec<SessionResponse>>, ApiError> {
    let session = current_session(&headers, &state).await?;
    let sessions = sqlx::query_as(
        "SELECT id, device_label, created_at, revoked_at, id = ? AS current \
         FROM auth_sessions WHERE owner_id = ? ORDER BY created_at DESC, id",
    )
    .bind(&session.session_id)
    .bind(&session.owner_id)
    .fetch_all(state.db.pool())
    .await
    .map_err(|_| ApiError::internal("database error"))?;
    Ok(Json(sessions))
}

/// Issue an independent device credential after an authenticated pairing claim.
pub async fn create_session(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<CreateSessionRequest>,
) -> Result<Json<TokenResponse>, ApiError> {
    let parent = current_session(&headers, &state).await?;
    let label = normalize_device_label(&body.device_label)?;
    Ok(Json(
        issue_session(&state, &parent.owner_id, &label, Some(&parent)).await?,
    ))
}

pub async fn logout(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<StatusCode, ApiError> {
    let session = current_session(&headers, &state).await?;
    revoke_owned_session(&state, &session.owner_id, &session.session_id).await
}

pub async fn revoke_session(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(session_id): Path<String>,
) -> Result<StatusCode, ApiError> {
    let session = current_session(&headers, &state).await?;
    revoke_owned_session(&state, &session.owner_id, &session_id).await
}

async fn revoke_owned_session(
    state: &AppState,
    owner_id: &str,
    session_id: &str,
) -> Result<StatusCode, ApiError> {
    let result = sqlx::query(
        "UPDATE auth_sessions SET revoked_at = COALESCE(revoked_at, ?) WHERE id = ? AND owner_id = ?",
    )
    .bind(now_rfc3339())
    .bind(session_id)
    .bind(owner_id)
    .execute(state.db.pool())
    .await
    .map_err(|_| ApiError::internal("database error"))?;
    if result.rows_affected() == 0 {
        return Err(ApiError::not_found("device session not found"));
    }
    Ok(StatusCode::NO_CONTENT)
}

pub async fn me(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<UserResponse>, ApiError> {
    let user_id = current_user_id(&headers, &state).await?;
    let user = find_user_by_id(state.db.pool(), &user_id)
        .await?
        .ok_or_else(|| ApiError::unauthorized("invalid token"))?;
    Ok(Json(user.into()))
}

pub async fn update_me(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<UpdateMeRequest>,
) -> Result<Json<UserResponse>, ApiError> {
    let user_id = current_user_id(&headers, &state).await?;
    let name = body.name.as_deref().map(normalize_name).transpose()?;
    let avatar_url = body
        .avatar_url
        .map(|value| normalize_avatar_url(value.as_deref()))
        .transpose()?;
    let update_avatar = avatar_url.is_some();
    let avatar_url = avatar_url.flatten();
    let now = now_rfc3339();

    let result = sqlx::query(
        "UPDATE users SET name = COALESCE(?, name), \
         avatar_url = CASE WHEN ? THEN ? ELSE avatar_url END, updated_at = ? WHERE id = ?",
    )
    .bind(name)
    .bind(update_avatar)
    .bind(avatar_url)
    .bind(now)
    .bind(&user_id)
    .execute(state.db.pool())
    .await
    .map_err(|_| ApiError::internal("database error"))?;
    if result.rows_affected() == 0 {
        return Err(ApiError::unauthorized("invalid token"));
    }

    let user = find_user_by_id(state.db.pool(), &user_id)
        .await?
        .ok_or_else(|| ApiError::unauthorized("invalid token"))?;
    Ok(Json(user.into()))
}

/// Resolve the authenticated user id from an `Authorization: Bearer <jwt>` header.
///
/// Database validation belongs here so HTTP handlers and future transports share
/// revocation semantics even when invoked without the HTTP router middleware.
pub async fn current_user_id(headers: &HeaderMap, state: &AppState) -> Result<String, ApiError> {
    Ok(current_session(headers, state).await?.owner_id)
}

pub async fn current_session(
    headers: &HeaderMap,
    state: &AppState,
) -> Result<AuthenticatedSession, ApiError> {
    let token =
        bearer_token(headers).ok_or_else(|| ApiError::unauthorized("missing bearer token"))?;
    let claims = decode_token(token, &state.auth.secret_key)
        .ok_or_else(|| ApiError::unauthorized("invalid token"))?;
    let session_id = claims.session_id.ok_or_else(|| {
        ApiError::new(
            StatusCode::UNAUTHORIZED,
            "session_upgrade_required",
            "Sign in again to create a device session; paired devices can scan a new pairing code",
        )
    })?;
    let session = AuthenticatedSession {
        owner_id: claims.sub,
        session_id,
        credential_id: claims
            .jti
            .ok_or_else(|| ApiError::unauthorized("invalid token"))?,
    };
    if !session_is_active(state.db.pool(), &session).await? {
        return Err(ApiError::new(
            StatusCode::UNAUTHORIZED,
            "session_revoked",
            "Device session was revoked. Sign in again",
        ));
    }
    Ok(session)
}

pub(crate) async fn session_is_active(
    pool: &SqlitePool,
    session: &AuthenticatedSession,
) -> Result<bool, ApiError> {
    sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM auth_sessions s JOIN users u ON u.id = s.owner_id \
         WHERE s.id = ? AND s.owner_id = ? AND s.credential_id = ? AND s.revoked_at IS NULL)",
    )
    .bind(&session.session_id)
    .bind(&session.owner_id)
    .bind(&session.credential_id)
    .fetch_one(pool)
    .await
    .map_err(|_| ApiError::internal("database error"))
}

/// Note: checking only request headers leaves an already-open SSE authenticated
/// forever. End its body on revocation without cancelling the server-side job.
pub async fn guard_session_stream(
    State(state): State<AppState>,
    request: Request,
    next: Next,
) -> Response {
    let headers = request.headers().clone();
    let response = next.run(request).await;
    if !response
        .headers()
        .get(axum::http::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.starts_with("text/event-stream"))
    {
        return response;
    }
    let session = match current_session(&headers, &state).await {
        Ok(session) => session,
        Err(error) => return error.into_response(),
    };
    let (parts, body) = response.into_parts();
    let (mut revoked_tx, revoked_rx) = tokio::sync::oneshot::channel::<()>();
    // Note: a future stored in the body is polled only while the client reads.
    // Run database checks independently so backpressure cannot park a query
    // holding a pool connection. Dropping the body closes the receiver and
    // releases the monitor once its current short database read completes.
    tokio::spawn(async move {
        loop {
            // Database reads also cover revocations made by another router or
            // after restart; no process-local notification is a source of truth.
            if revoked_tx.is_closed() {
                return;
            }
            // Complete an acquired SQLite read before stopping the monitor.
            // Cancelling a SQLx future can retire its connection; for a pinned
            // in-memory database that also destroys the entire database.
            let active = session_is_active(state.db.pool(), &session)
                .await
                .unwrap_or(false);
            if !active {
                let _ = revoked_tx.send(());
                return;
            }
            tokio::select! {
                _ = revoked_tx.closed() => return,
                _ = tokio::time::sleep(std::time::Duration::from_millis(250)) => {},
            }
        }
    });
    let revoked = async move {
        let _ = revoked_rx.await;
    };
    Response::from_parts(
        parts,
        Body::from_stream(body.into_data_stream().take_until(revoked)),
    )
}

fn bearer_token(headers: &HeaderMap) -> Option<&str> {
    headers
        .get(axum::http::header::AUTHORIZATION)?
        .to_str()
        .ok()?
        .strip_prefix("Bearer ")
        .map(str::trim)
        .filter(|t| !t.is_empty())
}

fn normalize_email(email: &str) -> String {
    email.trim().to_lowercase()
}

fn normalize_name(raw: &str) -> Result<String, ApiError> {
    let name = raw.trim().to_string();
    let name_len = name.chars().count();
    if !(1..=100).contains(&name_len) {
        return Err(ApiError::invalid_input(
            "name must be between 1 and 100 characters",
        ));
    }
    Ok(name)
}

fn validate_register(email: &str, password: &str) -> Result<(), ApiError> {
    match email.split_once('@') {
        Some((local, domain)) if !local.is_empty() && !domain.is_empty() => {}
        _ => return Err(ApiError::invalid_input("email is invalid")),
    }

    let password_len = password.chars().count();
    if !(8..=128).contains(&password_len) {
        return Err(ApiError::invalid_input(
            "password must be between 8 and 128 characters",
        ));
    }

    Ok(())
}

fn verify_password(password: &str, hashed: &str) -> bool {
    bcrypt::verify(password, hashed).unwrap_or(false)
}

async fn issue_session(
    state: &AppState,
    user_id: &str,
    device_label: &str,
    parent: Option<&AuthenticatedSession>,
) -> Result<TokenResponse, ApiError> {
    let session_id = Uuid::new_v4().to_string();
    let credential_id = Uuid::new_v4().to_string();
    // Note: login persists until the user signs out; a large finite expiry
    // would only postpone automatic logout. See docs/guide/settings.md.
    let claims = Claims {
        sub: user_id.to_string(),
        session_id: Some(session_id.clone()),
        jti: Some(credential_id.clone()),
    };
    let access_token = encode(
        &Header::new(Algorithm::HS256),
        &claims,
        &EncodingKey::from_secret(state.auth.secret_key.as_bytes()),
    )
    .map_err(|_| ApiError::internal("failed to sign token"))?;
    let mut tx = state
        .db
        .begin_write()
        .await
        .map_err(|_| ApiError::internal("database error"))?;
    if let Some(parent) = parent {
        let active: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM auth_sessions WHERE id = ? AND owner_id = ? \
             AND credential_id = ? AND revoked_at IS NULL)",
        )
        .bind(&parent.session_id)
        .bind(&parent.owner_id)
        .bind(&parent.credential_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(|_| ApiError::internal("database error"))?;
        if !active {
            return Err(ApiError::unauthorized("device session was revoked"));
        }
    }
    sqlx::query("INSERT INTO auth_sessions (id, owner_id, credential_id, device_label, created_at) VALUES (?, ?, ?, ?, ?)")
        .bind(session_id).bind(user_id).bind(credential_id).bind(device_label).bind(now_rfc3339())
        .execute(&mut *tx).await.map_err(|_| ApiError::internal("database error"))?;
    tx.commit()
        .await
        .map_err(|_| ApiError::internal("database error"))?;
    Ok(TokenResponse {
        access_token,
        token_type: "bearer".to_string(),
    })
}

fn normalize_device_label(raw: &str) -> Result<String, ApiError> {
    let label = raw.trim();
    if label.is_empty() || label.chars().count() > 100 || label.chars().any(char::is_control) {
        return Err(ApiError::invalid_input(
            "device_label must be 1 to 100 printable characters",
        ));
    }
    Ok(label.to_string())
}

fn decode_token(token: &str, secret: &str) -> Option<Claims> {
    let mut validation = Validation::new(Algorithm::HS256);
    // Note: sessions have no automatic deadline. Legacy JWTs are decoded only
    // to return session_upgrade_required; they never create sessions implicitly.
    validation.set_required_spec_claims(&["sub"]);
    validation.validate_exp = false;
    decode::<Claims>(
        token,
        &DecodingKey::from_secret(secret.as_bytes()),
        &validation,
    )
    .ok()
    .map(|data| data.claims)
}

fn now_rfc3339() -> String {
    OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .unwrap_or_default()
}

fn is_unique_violation(err: &sqlx::Error) -> bool {
    matches!(err, sqlx::Error::Database(db) if db.is_unique_violation())
}

async fn find_user_by_email(pool: &SqlitePool, email: &str) -> Result<Option<UserRow>, ApiError> {
    sqlx::query_as::<_, UserRow>(
        "SELECT id, email, password_hash, name, avatar_url, created_at FROM users WHERE email = ?",
    )
    .bind(email)
    .fetch_optional(pool)
    .await
    .map_err(|_| ApiError::internal("database error"))
}

async fn find_user_by_id(pool: &SqlitePool, id: &str) -> Result<Option<UserRow>, ApiError> {
    sqlx::query_as::<_, UserRow>(
        "SELECT id, email, password_hash, name, avatar_url, created_at FROM users WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(pool)
    .await
    .map_err(|_| ApiError::internal("database error"))
}
