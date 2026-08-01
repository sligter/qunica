//! Read-only inspection of the owner's configuration.
//!
//! Each kind has one hand-written projection listing exactly the columns that
//! may reach the model. Secrets are reduced at the SQL layer where possible
//! (`api_key IS NOT NULL AND api_key != ''` rather than `api_key`) so there is
//! no window in which the plaintext sits in a value that could be serialized by
//! mistake.

use serde_json::{json, Map, Value};
use sqlx::{Column, Row, SqlitePool, TypeInfo};

use super::{AppControlContext, TargetKind};
use crate::tools::{ToolError, ToolResult, ToolStatus};

/// Largest number of rows one `AppList` call returns.
///
/// The Assistant reads these in a small panel and pays for them in context; a
/// library with hundreds of skills should be summarized, not dumped.
const MAX_LIST_ITEMS: i64 = 100;

/// List the owner's rows of one kind.
pub(crate) async fn list(ctx: &AppControlContext, args: &Value) -> Result<ToolResult, ToolError> {
    let kind = parse_kind(args)?;
    let sql = format!(
        "SELECT {} FROM {} WHERE {} ORDER BY {} LIMIT {}",
        list_columns(kind),
        table(kind),
        owner_filter(kind),
        order_by(kind),
        MAX_LIST_ITEMS + 1,
    );
    let mut rows = fetch_rows(ctx.pool(), &sql, &[ctx.owner_id()]).await?;

    // Ask for one more than the cap so a truncated list can say so instead of
    // silently looking complete.
    let truncated = rows.len() as i64 > MAX_LIST_ITEMS;
    rows.truncate(MAX_LIST_ITEMS as usize);
    let rows = rows
        .into_iter()
        .map(|row| postprocess(kind, row))
        .collect::<Vec<_>>();

    let mut payload = Map::new();
    payload.insert("kind".to_string(), json!(kind.as_str()));
    payload.insert("count".to_string(), json!(rows.len()));
    payload.insert("items".to_string(), Value::Array(rows));
    if truncated {
        payload.insert(
            "truncated".to_string(),
            json!(format!(
                "only the first {MAX_LIST_ITEMS} are shown; narrow the request or ask the user"
            )),
        );
    }
    Ok(completed(Value::Object(payload)))
}

/// Fetch one row of one kind by id.
pub(crate) async fn get(ctx: &AppControlContext, args: &Value) -> Result<ToolResult, ToolError> {
    let kind = parse_kind(args)?;
    let id = args
        .get("id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .ok_or_else(|| ToolError::invalid("id is required"))?;

    let sql = format!(
        "SELECT {} FROM {} WHERE {} AND {}.id = ? LIMIT 1",
        detail_columns(kind),
        table(kind),
        owner_filter(kind),
        table_alias(kind),
    );
    let rows = fetch_rows(ctx.pool(), &sql, &[ctx.owner_id(), id]).await?;

    // A row the caller does not own is reported exactly like one that does not
    // exist. Distinguishing them would confirm the id belongs to someone.
    let Some(row) = rows.into_iter().next() else {
        return Err(ToolError::invalid(format!(
            "no {} with that id",
            kind.as_str()
        )));
    };

    Ok(completed(json!({
        "kind": kind.as_str(),
        "item": postprocess(kind, row),
    })))
}

/// Summarize what is configured, for onboarding and for "what am I missing?".
pub(crate) async fn state(ctx: &AppControlContext) -> Result<ToolResult, ToolError> {
    let mut counts = Map::new();
    for kind in TargetKind::ALL {
        let sql = format!(
            "SELECT COUNT(*) AS n FROM {} WHERE {}",
            table(kind),
            owner_filter(kind)
        );
        let count: i64 = sqlx::query_scalar(&sql)
            .bind(ctx.owner_id())
            .fetch_one(ctx.pool())
            .await
            .map_err(|_| ToolError::invalid("could not read the app state"))?;
        counts.insert(kind.as_str().to_string(), json!(count));
    }

    // `auto_create` needs a configured root directory. Reporting it here lets
    // the Assistant offer to create a workspace outright when it will work, and
    // explain what is missing when it will not, instead of proposing something
    // that fails on approval.
    let workspace_root: Option<String> = sqlx::query_scalar(
        "SELECT group_workspace_root FROM system_settings WHERE owner_id = ? LIMIT 1",
    )
    .bind(ctx.owner_id())
    .fetch_optional(ctx.pool())
    .await
    .map_err(|_| ToolError::invalid("could not read the app state"))?
    .flatten()
    .filter(|root: &String| !root.trim().is_empty());

    let has = |kind: TargetKind| counts[kind.as_str()].as_i64().unwrap_or(0) > 0;
    Ok(completed(json!({
        "can_auto_create_workspace": workspace_root.is_some(),
        "counts": Value::Object(counts.clone()),
        // The onboarding order: a provider is the hard prerequisite, then a
        // workspace to work in, then an agent to do the work.
        "has_provider": has(TargetKind::Provider),
        "has_workspace": has(TargetKind::Workspace),
        "has_agent": has(TargetKind::Agent),
        "has_group": has(TargetKind::Group),
    })))
}

fn parse_kind(args: &Value) -> Result<TargetKind, ToolError> {
    let raw = args
        .get("kind")
        .and_then(Value::as_str)
        .ok_or_else(|| ToolError::invalid("kind is required"))?;
    TargetKind::parse(raw).ok_or_else(|| {
        let known = TargetKind::ALL
            .iter()
            .map(|kind| kind.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        ToolError::invalid(format!("unknown kind '{raw}'; expected one of: {known}"))
    })
}

fn table(kind: TargetKind) -> &'static str {
    match kind {
        TargetKind::Agent => "agents a",
        TargetKind::Provider => "llm_providers p",
        TargetKind::Mcp => "mcp_servers m",
        TargetKind::Skill => "skills s",
        TargetKind::Workspace => "workspaces w",
        TargetKind::Group | TargetKind::Chat => "groups g",
    }
}

fn table_alias(kind: TargetKind) -> &'static str {
    match kind {
        TargetKind::Agent => "a",
        TargetKind::Provider => "p",
        TargetKind::Mcp => "m",
        TargetKind::Skill => "s",
        TargetKind::Workspace => "w",
        TargetKind::Group | TargetKind::Chat => "g",
    }
}

/// The `WHERE` clause every query starts from. The single `?` binds `owner_id`.
fn owner_filter(kind: TargetKind) -> &'static str {
    match kind {
        // The Assistant must not see itself here. Listing itself as a
        // configurable agent invites it to propose changes to its own tools,
        // which is the one edit the whole design exists to prevent.
        TargetKind::Agent => "a.owner_id = ? AND a.status = 'active' AND a.is_system = 0",
        TargetKind::Provider => "p.owner_id = ? AND p.status = 'active'",
        TargetKind::Mcp => "m.owner_id = ? AND m.status = 'active'",
        TargetKind::Skill => "s.owner_id = ? AND s.status = 'active'",
        TargetKind::Workspace => "w.owner_id = ? AND w.status = 'active'",
        TargetKind::Group => {
            "g.owner_id = ? AND g.status = 'active' AND g.conversation_kind = 'group'"
        }
        TargetKind::Chat => {
            "g.owner_id = ? AND g.status = 'active' AND g.conversation_kind = 'direct' \
             AND COALESCE((SELECT a.is_system FROM agents a WHERE a.id = g.direct_agent_id), 0) = 0"
        }
    }
}

fn order_by(kind: TargetKind) -> &'static str {
    match kind {
        TargetKind::Group | TargetKind::Chat => "g.updated_at DESC, g.id DESC",
        _ => "created_at DESC, id DESC",
    }
}

/// Columns returned by `AppList`: enough to identify and choose a row.
fn list_columns(kind: TargetKind) -> &'static str {
    match kind {
        TargetKind::Agent => "a.id, a.name, a.description, a.runtime_kind, a.workspace_id",
        TargetKind::Provider => {
            "p.id, p.name, p.kind, p.default_model, \
             (p.api_key IS NOT NULL AND p.api_key != '') AS api_key_configured"
        }
        TargetKind::Mcp => "m.id, m.name, m.description, m.transport, m.status",
        TargetKind::Skill => "s.id, s.name, s.description, s.source",
        TargetKind::Workspace => "w.id, w.name, w.backend_type, w.local_path",
        TargetKind::Group => "g.id, g.name, g.description, g.scheduler_enabled, g.workspace_id",
        TargetKind::Chat => "g.id, g.name, g.direct_agent_id, g.updated_at",
    }
}

/// Columns returned by `AppGet`: the full configurable surface, minus secrets.
fn detail_columns(kind: TargetKind) -> &'static str {
    match kind {
        TargetKind::Agent => {
            "a.id, a.name, a.description, a.system_prompt, a.runtime_kind, a.workspace_id, \
             a.provider_id, a.model_config_json, a.tool_config_json, a.skill_ids_json, \
             a.created_at"
        }
        // `api_key` is reduced to a boolean in SQL. `base_url` is configuration
        // the user typed and may need to correct, so it stays.
        TargetKind::Provider => {
            "p.id, p.name, p.kind, p.base_url, p.default_model, p.context_window_tokens, \
             p.reasoning_passback, p.description, \
             (p.api_key IS NOT NULL AND p.api_key != '') AS api_key_configured, p.created_at"
        }
        // `env_json` is excluded outright: stdio servers routinely carry
        // credentials there, and unlike headers there is no useful
        // name-only view worth the risk of getting the redaction wrong.
        TargetKind::Mcp => {
            "m.id, m.name, m.description, m.transport, m.command, m.args_json, m.cwd, m.url, \
             m.headers_json, m.timeout_seconds, m.tool_filter_json, m.status, m.created_at"
        }
        TargetKind::Skill => {
            "s.id, s.name, s.description, s.body_markdown, s.metadata_json, s.source, s.created_at"
        }
        TargetKind::Workspace => {
            "w.id, w.name, w.backend_type, w.local_path, w.config_json, w.created_at"
        }
        TargetKind::Group => {
            "g.id, g.name, g.description, g.announcement, g.workspace_id, g.free_speech, \
             g.proactive_mode, g.communication_mode, g.scheduler_enabled, g.agent_mention_policy, \
             g.max_total_tokens, g.turn_timeout_seconds, g.created_at"
        }
        TargetKind::Chat => "g.id, g.name, g.direct_agent_id, g.created_at, g.updated_at",
    }
}

/// Per-kind cleanup applied after the row is decoded.
///
/// Only structural work: turning header maps into name lists and integers into
/// booleans. Nothing here can un-redact a secret, because no secret column is
/// ever selected in the first place.
fn postprocess(kind: TargetKind, mut row: Map<String, Value>) -> Value {
    if kind == TargetKind::Mcp {
        // Header values carry bearer tokens. The names are the useful part —
        // enough to say "you have an Authorization header set" — so keep those
        // and drop the map entirely.
        let names = row
            .remove("headers_json")
            .and_then(|raw| raw.as_str().and_then(|raw| serde_json::from_str(raw).ok()))
            .map(|value: Value| {
                value
                    .as_object()
                    .map(|map| map.keys().cloned().map(Value::String).collect::<Vec<_>>())
                    .unwrap_or_default()
            })
            .unwrap_or_default();
        row.insert("header_names".to_string(), Value::Array(names));
    }

    // SQLite has no boolean type, so these arrive as 0/1 integers. Handing the
    // model an integer for a flag invites it to report "free_speech: 1".
    for key in [
        "api_key_configured",
        "scheduler_enabled",
        "free_speech",
        "proactive_mode",
        "reasoning_passback",
    ] {
        if let Some(Value::Number(number)) = row.get(key) {
            let flag = number.as_i64().unwrap_or(0) != 0;
            row.insert(key.to_string(), Value::Bool(flag));
        }
    }

    // Embedded JSON columns are stored as text. Parsing them lets the model see
    // the structure instead of an escaped string it has to re-parse.
    for key in [
        "model_config_json",
        "tool_config_json",
        "skill_ids_json",
        "metadata_json",
        "config_json",
        "args_json",
        "tool_filter_json",
    ] {
        if let Some(Value::String(raw)) = row.get(key) {
            if let Ok(parsed) = serde_json::from_str::<Value>(raw) {
                row.insert(key.to_string(), parsed);
            }
        }
    }

    Value::Object(row)
}

/// Run a query and decode each row into a JSON object generically.
///
/// The projections above are the only place column names are chosen, so
/// decoding by name here cannot widen what is returned.
async fn fetch_rows(
    pool: &SqlitePool,
    sql: &str,
    binds: &[&str],
) -> Result<Vec<Map<String, Value>>, ToolError> {
    let mut query = sqlx::query(sql);
    for bind in binds {
        query = query.bind(*bind);
    }
    let rows = query
        .fetch_all(pool)
        .await
        .map_err(|_| ToolError::invalid("could not read that configuration"))?;

    Ok(rows
        .into_iter()
        .map(|row| {
            let mut object = Map::new();
            for (index, column) in row.columns().iter().enumerate() {
                let value = decode_column(&row, index, column.type_info().name());
                object.insert(column.name().to_string(), value);
            }
            object
        })
        .collect())
}

fn decode_column(row: &sqlx::sqlite::SqliteRow, index: usize, type_name: &str) -> Value {
    // SQLite is dynamically typed, so try the declared affinity first and fall
    // back through the others rather than assuming.
    match type_name {
        "INTEGER" | "BIGINT" | "BOOLEAN" => row
            .try_get::<Option<i64>, _>(index)
            .ok()
            .flatten()
            .map_or(Value::Null, |value| json!(value)),
        "REAL" | "FLOAT" | "DOUBLE" => row
            .try_get::<Option<f64>, _>(index)
            .ok()
            .flatten()
            .and_then(serde_json::Number::from_f64)
            .map_or(Value::Null, Value::Number),
        _ => row
            .try_get::<Option<String>, _>(index)
            .ok()
            .flatten()
            .map_or_else(
                || {
                    row.try_get::<Option<i64>, _>(index)
                        .ok()
                        .flatten()
                        .map_or(Value::Null, |value| json!(value))
                },
                Value::String,
            ),
    }
}

fn completed(payload: Value) -> ToolResult {
    ToolResult {
        status: ToolStatus::Completed,
        output: serde_json::to_string(&payload).unwrap_or_else(|_| "{}".to_string()),
    }
}

/// Answer a "how does this work?" question from the bundled guide.
///
/// Takes either a free-text `query` or an exact `slug`. Unlike the other
/// app-control tools this touches no database, but it lives here because it is
/// part of the same capability and shares the same availability rule.
pub(crate) fn docs(args: &Value) -> Result<ToolResult, ToolError> {
    let query = args
        .get("query")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let slug = args
        .get("slug")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty());

    if let Some(slug) = slug {
        let doc = crate::docs::by_slug(slug).ok_or_else(|| {
            ToolError::invalid(format!(
                "no page with slug '{slug}'; available pages: {}",
                available_slugs()
            ))
        })?;
        return Ok(bounded_docs(json!({
            "documents": [{
                "slug": doc.slug,
                "title": doc.title,
                "content": doc.body,
            }],
        })));
    }

    let Some(query) = query else {
        return Err(ToolError::invalid("query or slug is required"));
    };

    let matches = crate::docs::search(query);
    if matches.is_empty() {
        // Returning the least-bad page here would be worse than nothing: the
        // model would summarize it as the answer. Say so, and offer the index
        // so it can pick a page by name instead.
        return Ok(bounded_docs(json!({
            "documents": [],
            "message": format!(
                "no matching page for '{query}'. This guide covers only AG Swarmer itself; \
                 if the question is about something else, say so rather than guessing."
            ),
            "available": doc_index(),
        })));
    }

    Ok(bounded_docs(json!({
        "documents": matches
            .into_iter()
            .map(|hit| json!({
                "slug": hit.slug,
                "title": hit.title,
                "content": hit.excerpt,
            }))
            .collect::<Vec<_>>(),
    })))
}

fn doc_index() -> Value {
    Value::Array(
        crate::docs::index()
            .into_iter()
            .map(|(slug, title)| json!({ "slug": slug, "title": title }))
            .collect(),
    )
}

fn available_slugs() -> String {
    crate::docs::index()
        .into_iter()
        .map(|(slug, _)| slug)
        .collect::<Vec<_>>()
        .join(", ")
}

/// Serialize a docs payload, degrading to the index if it somehow exceeds the
/// budget. The per-document excerpt cap makes that unreachable today; this
/// keeps it true if the guide grows or the caps are raised.
fn bounded_docs(payload: Value) -> ToolResult {
    let output = serde_json::to_string(&payload).unwrap_or_else(|_| "{}".to_string());
    if output.len() <= crate::docs::MAX_DOCS_OUTPUT_BYTES {
        return ToolResult {
            status: ToolStatus::Completed,
            output,
        };
    }
    let fallback = json!({
        "documents": [],
        "message": "the matching pages were too large to return together; \
                    request one by slug",
        "available": doc_index(),
    });
    ToolResult {
        status: ToolStatus::Completed,
        output: serde_json::to_string(&fallback).unwrap_or_else(|_| "{}".to_string()),
    }
}
