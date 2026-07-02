use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    Json,
};
use jsonwebtoken::{decode, encode, Algorithm, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use time::{format_description::well_known::Rfc3339, OffsetDateTime};
use uuid::Uuid;

use crate::api::{error::ApiError, AppState};

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

#[derive(Debug, Serialize, Deserialize)]
struct Claims {
    sub: String,
    exp: usize,
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

pub async fn register(
    State(state): State<AppState>,
    Json(body): Json<RegisterRequest>,
) -> Result<(StatusCode, Json<UserResponse>), ApiError> {
    let email = normalize_email(&body.email);
    let name = body.name.trim().to_string();
    validate_register(&email, &body.password, &name)?;

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

    let access_token = create_access_token(
        &user.id,
        &state.auth.secret_key,
        state.auth.access_token_expire_minutes,
    )?;
    Ok(Json(TokenResponse {
        access_token,
        token_type: "bearer".to_string(),
    }))
}

pub async fn me(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<UserResponse>, ApiError> {
    let user_id = current_user_id(&headers, &state.auth.secret_key)?;
    let user = find_user_by_id(state.db.pool(), &user_id)
        .await?
        .ok_or_else(|| ApiError::unauthorized("invalid token"))?;
    Ok(Json(user.into()))
}

/// Resolve the authenticated user id from an `Authorization: Bearer <jwt>` header.
///
/// Reusable by later API v2 routes that need the current user. Any missing or
/// invalid token surfaces as a `401 unauthorized`.
pub fn current_user_id(headers: &HeaderMap, secret: &str) -> Result<String, ApiError> {
    let token =
        bearer_token(headers).ok_or_else(|| ApiError::unauthorized("missing bearer token"))?;
    let claims =
        decode_token(token, secret).ok_or_else(|| ApiError::unauthorized("invalid token"))?;
    Ok(claims.sub)
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

fn validate_register(email: &str, password: &str, name: &str) -> Result<(), ApiError> {
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

    let name_len = name.chars().count();
    if !(1..=100).contains(&name_len) {
        return Err(ApiError::invalid_input(
            "name must be between 1 and 100 characters",
        ));
    }

    Ok(())
}

fn verify_password(password: &str, hashed: &str) -> bool {
    bcrypt::verify(password, hashed).unwrap_or(false)
}

fn create_access_token(
    user_id: &str,
    secret: &str,
    expire_minutes: i64,
) -> Result<String, ApiError> {
    let exp = OffsetDateTime::now_utc().unix_timestamp() + expire_minutes * 60;
    let claims = Claims {
        sub: user_id.to_string(),
        exp: exp.max(0) as usize,
    };
    encode(
        &Header::new(Algorithm::HS256),
        &claims,
        &EncodingKey::from_secret(secret.as_bytes()),
    )
    .map_err(|_| ApiError::internal("failed to sign token"))
}

fn decode_token(token: &str, secret: &str) -> Option<Claims> {
    let validation = Validation::new(Algorithm::HS256);
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
