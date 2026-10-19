//! Built-in stdio MCP server backed by a session-owned loopback broker.
//!
//! Note: the child never receives an account token or database path. Its random
//! capability is useful only while its owning ACP dispatch is active. Keeping
//! the broker with the live ACP session preserves incremental session reuse.

use std::{
    io::{BufRead, Read, Write},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};

use axum::{
    extract::{DefaultBodyLimit, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::post,
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};
use sqlx::SqlitePool;
use tokio::task::JoinHandle;
use uuid::Uuid;

use crate::{
    api::groups,
    decision::{self, DecisionGate, DecisionScenario},
    tools::{FileEdit, WorkspaceTools, MAX_WRITE_BYTES},
};

const RELAY_FLAG: &str = "--qunica-group-notes-mcp";
const ENDPOINT_ENV: &str = "QUNICA_NOTES_MCP_ENDPOINT";
const TOKEN_ENV: &str = "QUNICA_NOTES_MCP_TOKEN";
const MAX_RPC_BYTES: usize = 2 * MAX_WRITE_BYTES + 64 * 1024;

/// Host-provided authority, never accepted from tool arguments.
#[derive(Clone)]
pub struct NotesContext {
    pub pool: SqlitePool,
    pub write_lock: Arc<tokio::sync::Mutex<()>>,
    pub owner_id: String,
    pub group_id: String,
    pub agent_id: String,
    pub thread_id: String,
    pub decision: Option<DecisionGate>,
}

#[derive(Clone)]
struct ActiveContext {
    context: NotesContext,
    active: Arc<AtomicBool>,
}

#[derive(Clone)]
struct BrokerState {
    token: String,
    current: Arc<Mutex<Option<ActiveContext>>>,
}

pub(super) struct NotesBridge {
    endpoint: String,
    state: BrokerState,
    server: JoinHandle<()>,
}

pub(super) struct NotesLease(Arc<AtomicBool>);

impl Drop for NotesLease {
    fn drop(&mut self) {
        self.0.store(false, Ordering::SeqCst);
    }
}

impl Drop for NotesBridge {
    fn drop(&mut self) {
        if let Ok(mut current) = self.state.current.lock() {
            if let Some(active) = current.take() {
                active.active.store(false, Ordering::SeqCst);
            }
        }
        self.server.abort();
    }
}

impl NotesBridge {
    pub(super) async fn start() -> Result<Self, String> {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .map_err(|_| "Could not start the group notes MCP bridge".to_string())?;
        let endpoint = format!(
            "http://{}/rpc",
            listener
                .local_addr()
                .map_err(|_| "Group notes MCP address unavailable")?
        );
        let state = BrokerState {
            token: Uuid::new_v4().to_string(),
            current: Arc::new(Mutex::new(None)),
        };
        let app = Router::new()
            .route("/rpc", post(handle))
            .layer(DefaultBodyLimit::max(MAX_RPC_BYTES))
            .with_state(state.clone());
        let server = tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        Ok(Self {
            endpoint,
            state,
            server,
        })
    }

    pub(super) fn activate(&self, context: NotesContext) -> NotesLease {
        let active = Arc::new(AtomicBool::new(true));
        let mut current = self.state.current.lock().expect("notes authority lock");
        if let Some(old) = current.replace(ActiveContext {
            context,
            active: active.clone(),
        }) {
            old.active.store(false, Ordering::SeqCst);
        }
        NotesLease(active)
    }

    pub(super) fn server_config(&self) -> Result<Value, String> {
        let executable =
            std::env::current_exe().map_err(|_| "Group notes MCP executable unavailable")?;
        Ok(json!({"name":"qunica-group-notes", "command":executable,
        "args":[RELAY_FLAG], "env":[
            {"name":ENDPOINT_ENV,"value":self.endpoint},
            {"name":TOKEN_ENV,"value":self.state.token}
        ]}))
    }
}

async fn handle(
    State(state): State<BrokerState>,
    headers: HeaderMap,
    Json(request): Json<Value>,
) -> Response {
    if headers.contains_key("origin")
        || headers.get("authorization").and_then(|h| h.to_str().ok())
            != Some(format!("Bearer {}", state.token).as_str())
    {
        return StatusCode::FORBIDDEN.into_response();
    }
    let active = state.current.lock().ok().and_then(|v| v.clone());
    let Some(active) = active.filter(|a| a.active.load(Ordering::SeqCst)) else {
        return StatusCode::FORBIDDEN.into_response();
    };
    handle_active(active, request).await
}

pub(crate) async fn remote_call(context: NotesContext, request: Value) -> Response {
    handle_active(
        ActiveContext {
            context,
            active: Arc::new(AtomicBool::new(true)),
        },
        request,
    )
    .await
}

async fn handle_active(active: ActiveContext, request: Value) -> Response {
    let id = request.get("id").cloned();
    let Some(method) = request
        .get("method")
        .and_then(Value::as_str)
        .filter(|_| request["jsonrpc"] == "2.0")
    else {
        return Json(rpc_error(
            id.unwrap_or(Value::Null),
            -32600,
            "Invalid request",
        ))
        .into_response();
    };
    let Some(id) = id else {
        return StatusCode::ACCEPTED.into_response();
    };
    let result = match method {
        "initialize" => {
            let requested = request["params"]["protocolVersion"].as_str().unwrap_or("");
            let version = match requested {
                "2024-11-05" | "2025-03-26" | "2025-06-18" | "2025-11-25" => requested,
                _ => "2025-06-18",
            };
            json!({"protocolVersion":version, "capabilities":{"tools":{}},
                "serverInfo":{"name":"qunica-group-notes","version":env!("CARGO_PKG_VERSION")}})
        }
        "ping" => json!({}),
        "tools/list" => json!({"tools":tool_definitions()}),
        "tools/call" => {
            let name = request["params"]["name"].as_str().unwrap_or("");
            if !["ReadGroupNotes", "CreateGroupNote", "EditGroupNote"].contains(&name) {
                return Json(rpc_error(id, -32602, "Unknown group notes tool")).into_response();
            }
            let args = request["params"]
                .get("arguments")
                .cloned()
                .unwrap_or(json!({}));
            match active.call(name, args).await {
                Ok(text) => json!({"content":[{"type":"text","text":text}],"isError":false}),
                Err(error) => json!({"content":[{"type":"text","text":error}],"isError":true}),
            }
        }
        _ => return Json(rpc_error(id, -32601, "Method not found")).into_response(),
    };
    Json(json!({"jsonrpc":"2.0","id":id,"result":result})).into_response()
}

fn rpc_error(id: Value, code: i64, message: &str) -> Value {
    json!({"jsonrpc":"2.0","id":id,"error":{"code":code,"message":message}})
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReadArgs {
    path: Option<String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CreateArgs {
    title: String,
    content: Option<String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EditArgs {
    path: String,
    edits: Vec<EditBlock>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EditBlock {
    old_text: String,
    new_text: String,
}

fn parse<T: serde::de::DeserializeOwned>(args: Value) -> Result<T, String> {
    serde_json::from_value(args).map_err(|_| "Invalid group notes tool arguments".to_string())
}

fn note_id(path: &str) -> Result<&str, String> {
    let id = path
        .strip_suffix(".md")
        .ok_or("Use the note path returned by ReadGroupNotes")?;
    let parsed = Uuid::parse_str(id).map_err(|_| "Invalid group note path")?;
    if parsed.to_string() != id {
        return Err("Invalid group note path".into());
    }
    Ok(id)
}

impl ActiveContext {
    async fn check(&self) -> Result<(), String> {
        if !self.active.load(Ordering::SeqCst) {
            return Err("Group notes access has expired".into());
        }
        let c = &self.context;
        let allowed: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM groups g JOIN group_agents ga ON ga.group_id = g.id \
             JOIN agents a ON a.id = ga.agent_id JOIN threads t ON t.group_id = g.id \
             WHERE g.id = ? AND g.owner_id = ? AND g.status = 'active' AND g.conversation_kind = 'group' \
             AND ga.agent_id = ? AND ga.status = 'active' AND a.owner_id = g.owner_id AND a.status = 'active' \
             AND t.id = ? AND t.status = 'active')")
            .bind(&c.group_id).bind(&c.owner_id).bind(&c.agent_id).bind(&c.thread_id)
            .fetch_one(&c.pool).await.map_err(|_| "Could not verify group notes access")?;
        if !allowed {
            return Err("Group notes access is no longer available".into());
        }
        Ok(())
    }

    async fn validate_content(&self, content: &str) -> Result<(), String> {
        if content.len() > MAX_WRITE_BYTES {
            return Err("Group note is too large".into());
        }
        if let Some(gate) = self
            .context
            .decision
            .as_ref()
            .filter(|g| g.enabled(DecisionScenario::NoteValidation))
        {
            if let Some(reason) = decision::validate_note_edit(gate, content).await {
                return Err(reason);
            }
        }
        self.check().await
    }

    async fn call(&self, name: &str, args: Value) -> Result<String, String> {
        let c = &self.context;
        self.check().await?;
        let root = groups::group_notes_root_for_owner(&c.pool, &c.owner_id, &c.group_id)
            .await
            .map_err(|e| e.message_text())?;
        match name {
            "ReadGroupNotes" => {
                let args: ReadArgs = parse(args)?;
                match args.path.as_deref().filter(|p| *p != "index.md") {
                    None => {
                        let rows: Vec<(String, String)> = sqlx::query_as(
                            "SELECT id, title FROM group_notes WHERE group_id = ? AND status = 'active' ORDER BY created_at, id")
                            .bind(&c.group_id).fetch_all(&c.pool).await.map_err(|_| "Could not list group notes")?;
                        Ok(json!({"notes": rows.into_iter().map(|(id,title)| json!({"path":format!("{id}.md"),"title":title})).collect::<Vec<_>>()}).to_string())
                    }
                    Some(path) => {
                        let note = groups::get_group_note_inner(
                            &c.pool,
                            &c.owner_id,
                            &c.group_id,
                            note_id(path)?,
                        )
                        .await
                        .map_err(|e| e.message_text())?;
                        Ok(note_result(note))
                    }
                }
            }
            "CreateGroupNote" => {
                let args: CreateArgs = parse(args)?;
                if let Some(content) = &args.content {
                    self.validate_content(content).await?;
                }
                let body =
                    serde_json::from_value(json!({"title":args.title,"content":args.content}))
                        .map_err(|_| "Invalid note")?;
                self.check().await?;
                let _write = c.write_lock.lock().await;
                self.check().await?;
                let note = groups::create_group_note_with_pool(
                    &c.pool,
                    &c.owner_id,
                    &c.group_id,
                    body,
                    Some(groups::GroupNoteWriteGuard {
                        active: &self.active,
                        expected_content: None,
                    }),
                )
                .await
                .map_err(|e| e.message_text())?;
                Ok(note_result(note))
            }
            "EditGroupNote" => {
                let args: EditArgs = parse(args)?;
                let id = note_id(&args.path)?;
                // Membership in the database is mandatory: a UUID-shaped file
                // from a different group sharing this workspace is not ours.
                let existing = groups::get_group_note_inner(&c.pool, &c.owner_id, &c.group_id, id)
                    .await
                    .map_err(|e| e.message_text())?;
                let existing = serde_json::to_value(existing).map_err(|_| "Invalid note")?;
                let original = existing["content"].as_str().ok_or("Invalid note content")?;
                let notes = WorkspaceTools::new(root.join("Notes"))
                    .map_err(|_| "Group notes directory unavailable")?;
                let edits = args
                    .edits
                    .into_iter()
                    .map(|e| FileEdit::new(e.old_text, e.new_text))
                    .collect::<Vec<_>>();
                let content = notes
                    .preview_edit(&args.path, &edits)
                    .map_err(|e| e.to_string())?;
                self.validate_content(&content).await?;
                let body = serde_json::from_value(json!({"content":content}))
                    .map_err(|_| "Invalid note")?;
                let _write = c.write_lock.lock().await;
                self.check().await?;
                let note = groups::update_group_note_with_pool(
                    &c.pool,
                    &c.owner_id,
                    &c.group_id,
                    id,
                    body,
                    Some(groups::GroupNoteWriteGuard {
                        active: &self.active,
                        expected_content: Some(original),
                    }),
                )
                .await
                .map_err(|e| e.message_text())?;
                Ok(note_result(note))
            }
            _ => Err("Unknown group notes tool".into()),
        }
    }
}

fn note_result(note: groups::GroupNoteResponse) -> String {
    let mut result = serde_json::to_value(note).expect("serializable note");
    result["path"] = json!(format!("{}.md", result["id"].as_str().unwrap_or_default()));
    result.to_string()
}

fn tool_definitions() -> Value {
    json!([
        {"name":"ReadGroupNotes","description":"List this group's notes, or read a note. Omit path for the index. Use returned note paths unchanged.",
         "inputSchema":{"type":"object","properties":{"path":{"type":"string"}},"additionalProperties":false}},
        {"name":"CreateGroupNote","description":"Create a shared group note. The host assigns its path and updates the index. Omit content for the proposed-note template. Follow the shared note authoring method.",
         "inputSchema":{"type":"object","properties":{"title":{"type":"string"},"content":{"type":"string"}},"required":["title"],"additionalProperties":false}},
        {"name":"EditGroupNote","description":"Edit an existing note using exact unique replacements. Preserve Since and follow the shared note method. Never edit the index or invent a note path.",
         "inputSchema":{"type":"object","properties":{"path":{"type":"string"},"edits":{"type":"array","minItems":1,"items":{"type":"object","properties":{"old_text":{"type":"string"},"new_text":{"type":"string"}},"required":["old_text","new_text"],"additionalProperties":false}}},"required":["path","edits"],"additionalProperties":false}}
    ])
}

/// Both the desktop executable and standalone backend enter here before their
/// normal startup. No UI, database, tracing, or stdout banners in relay mode.
pub fn run_stdio_if_requested() {
    if std::env::args_os().nth(1).as_deref() != Some(std::ffi::OsStr::new(RELAY_FLAG)) {
        return;
    }
    let result = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|_| ())
        .and_then(|rt| rt.block_on(relay()).map_err(|_| ()));
    if result.is_err() {
        eprintln!("Group notes MCP connection closed or unavailable");
    }
    std::process::exit(if result.is_ok() { 0 } else { 1 });
}

async fn relay() -> anyhow::Result<()> {
    let endpoint = std::env::var(ENDPOINT_ENV)?;
    let token = std::env::var(TOKEN_ENV)?;
    let url = reqwest::Url::parse(&endpoint)?;
    anyhow::ensure!(
        url.scheme() == "http" && url.host_str() == Some("127.0.0.1") && url.path() == "/rpc",
        "Invalid broker address"
    );
    let client = reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(60))
        .build()?;
    let mut input = std::io::stdin().lock();
    let mut output = std::io::stdout().lock();
    loop {
        let mut line = Vec::new();
        let n = input
            .by_ref()
            .take((MAX_RPC_BYTES + 1) as u64)
            .read_until(b'\n', &mut line)?;
        if n == 0 {
            return Ok(());
        }
        anyhow::ensure!(n <= MAX_RPC_BYTES, "Request too large");
        let request: Value = match serde_json::from_slice(&line) {
            Ok(request) => request,
            Err(_) => {
                writeln!(output, "{}", rpc_error(Value::Null, -32700, "Parse error"))?;
                output.flush()?;
                continue;
            }
        };
        let response = client
            .post(url.clone())
            .bearer_auth(&token)
            .json(&request)
            .send()
            .await?;
        if !response.status().is_success() {
            if let Some(id) = request.get("id") {
                writeln!(
                    output,
                    "{}",
                    rpc_error(
                        id.clone(),
                        -32000,
                        "Group notes access is unavailable outside the active dispatch"
                    )
                )?;
                output.flush()?;
            }
            continue;
        }
        if response.status() != StatusCode::ACCEPTED {
            let body: Value = response.json().await?;
            writeln!(output, "{body}")?;
            output.flush()?;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn fixture() -> (NotesContext, tempfile::TempDir) {
        let db = crate::db::Db::connect("sqlite::memory:").await.unwrap();
        db.migrate().await.unwrap();
        let pool = db.pool().clone();
        let root = tempfile::tempdir().unwrap();
        let owner = Uuid::new_v4().to_string();
        let group = Uuid::new_v4().to_string();
        let agent = Uuid::new_v4().to_string();
        let workspace = Uuid::new_v4().to_string();
        let thread = Uuid::new_v4().to_string();
        sqlx::query("INSERT INTO users (id,email,password_hash,name,created_at,updated_at) VALUES (?,'notes@example.com','x','User','now','now')")
            .bind(&owner).execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO workspaces (id,owner_id,name,backend_type,local_path,created_at,updated_at) VALUES (?,?,'Local','local',?,'now','now')")
            .bind(&workspace).bind(&owner).bind(root.path().to_string_lossy().as_ref()).execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO groups (id,owner_id,workspace_id,name,created_at,updated_at) VALUES (?,?,?,'Group','now','now')")
            .bind(&group).bind(&owner).bind(&workspace).execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO agents (id,owner_id,name,system_prompt,created_at,updated_at) VALUES (?,?,'Agent','Help','now','now')")
            .bind(&agent).bind(&owner).execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO group_agents (group_id,agent_id,joined_at,updated_at) VALUES (?,?,'now','now')")
            .bind(&group).bind(&agent).execute(&pool).await.unwrap();
        sqlx::query(
            "INSERT INTO threads (id,group_id,created_at,updated_at) VALUES (?,?,'now','now')",
        )
        .bind(&thread)
        .bind(&group)
        .execute(&pool)
        .await
        .unwrap();
        (
            NotesContext {
                pool,
                owner_id: owner,
                group_id: group,
                agent_id: agent,
                thread_id: thread,
                write_lock: Arc::new(tokio::sync::Mutex::new(())),
                decision: None,
            },
            root,
        )
    }

    async fn rpc(bridge: &NotesBridge, method: &str, params: Value) -> Value {
        reqwest::Client::builder()
            .no_proxy()
            .build()
            .unwrap()
            .post(&bridge.endpoint)
            .bearer_auth(&bridge.state.token)
            .json(&json!({"jsonrpc":"2.0","id":1,"method":method,"params":params}))
            .send()
            .await
            .unwrap()
            .error_for_status()
            .unwrap()
            .json()
            .await
            .unwrap()
    }

    async fn call(bridge: &NotesBridge, name: &str, args: Value) -> Value {
        rpc(bridge, "tools/call", json!({"name":name,"arguments":args})).await["result"].clone()
    }

    fn output(result: &Value) -> Value {
        assert_eq!(result["isError"], false, "{result}");
        serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap()
    }

    #[tokio::test]
    async fn notes_mcp_crud_updates_database_files_and_index() {
        let (context, root) = fixture().await;
        let bridge = NotesBridge::start().await.unwrap();
        let _lease = bridge.activate(context.clone());
        let init = rpc(
            &bridge,
            "initialize",
            json!({"protocolVersion":"2024-11-05"}),
        )
        .await;
        assert_eq!(init["result"]["protocolVersion"], "2024-11-05");
        let tools = rpc(&bridge, "tools/list", json!({})).await;
        assert_eq!(tools["result"]["tools"].as_array().unwrap().len(), 3);
        assert!(
            output(&call(&bridge, "ReadGroupNotes", json!({})).await)["notes"]
                .as_array()
                .unwrap()
                .is_empty()
        );
        let note = output(&call(&bridge, "CreateGroupNote", json!({"title":"Design"})).await);
        let path = note["path"].as_str().unwrap();
        assert!(note["content"]
            .as_str()
            .unwrap()
            .contains("Status: proposed"));
        assert!(root.path().join("Notes").join(path).is_file());
        let index = std::fs::read_to_string(root.path().join("Notes/index.md")).unwrap();
        assert!(index.contains(path));
        let edited = output(&call(&bridge,"EditGroupNote",json!({"path":path,"edits":[{"old_text":"Describe the problem and constraints.","new_text":"Keep state scoped to the group."}]})).await);
        let content = edited["content"].as_str().unwrap();
        assert!(content.contains("Keep state scoped to the group."));
        let saved: String = sqlx::query_scalar("SELECT content FROM group_notes WHERE id = ?")
            .bind(note["id"].as_str().unwrap())
            .fetch_one(&context.pool)
            .await
            .unwrap();
        assert_eq!(saved, content);
        assert_eq!(
            std::fs::read_to_string(root.path().join("Notes").join(path)).unwrap(),
            content
        );
        assert_eq!(
            output(&call(&bridge, "ReadGroupNotes", json!({"path":path})).await)["content"],
            content
        );
        let bad = call(
            &bridge,
            "EditGroupNote",
            json!({"path":path,"edits":[{"old_text":"missing text","new_text":"wrong"}]}),
        )
        .await;
        assert_eq!(bad["isError"], true);
        assert_eq!(
            std::fs::read_to_string(root.path().join("Notes/index.md")).unwrap(),
            index
        );
    }

    #[tokio::test]
    async fn notes_mcp_rejects_unregistered_paths_cross_group_and_revoked_members() {
        let (context, root) = fixture().await;
        let bridge = NotesBridge::start().await.unwrap();
        let _lease = bridge.activate(context.clone());
        let foreign_group = Uuid::new_v4().to_string();
        sqlx::query("INSERT INTO groups (id,owner_id,workspace_id,name,created_at,updated_at) SELECT ?,owner_id,workspace_id,'Other','now','now' FROM groups WHERE id = ?")
            .bind(&foreign_group).bind(&context.group_id).execute(&context.pool).await.unwrap();
        let foreign = groups::create_group_note_with_pool(
            &context.pool,
            &context.owner_id,
            &foreign_group,
            serde_json::from_value(json!({"title":"Other","content":"private"})).unwrap(),
            None,
        )
        .await
        .unwrap();
        let foreign: Value = serde_json::from_str(&note_result(foreign)).unwrap();
        let unknown = format!("{}.md", Uuid::new_v4());
        std::fs::write(root.path().join("Notes").join(&unknown), "not registered").unwrap();
        for path in [
            "../AGENTS.md",
            "Notes/index.md",
            "C:/secret.md",
            foreign["path"].as_str().unwrap(),
            &unknown,
        ] {
            assert_eq!(
                call(&bridge, "ReadGroupNotes", json!({"path":path})).await["isError"],
                true,
                "{path}"
            );
            assert_eq!(
                call(
                    &bridge,
                    "EditGroupNote",
                    json!({"path":path,"edits":[{"old_text":"private","new_text":"oops"}]})
                )
                .await["isError"],
                true,
                "{path}"
            );
        }
        assert_eq!(
            call(
                &bridge,
                "CreateGroupNote",
                json!({"title":"Injected","group_id":foreign_group})
            )
            .await["isError"],
            true
        );
        sqlx::query("UPDATE group_agents SET status='removed' WHERE group_id = ?")
            .bind(&context.group_id)
            .execute(&context.pool)
            .await
            .unwrap();
        assert_eq!(
            call(&bridge, "CreateGroupNote", json!({"title":"Revoked"})).await["isError"],
            true
        );
    }

    #[tokio::test]
    async fn notes_mcp_requires_bearer_and_active_lease_and_rejects_browser_origins() {
        let (context, _root) = fixture().await;
        let bridge = NotesBridge::start().await.unwrap();
        let client = reqwest::Client::builder().no_proxy().build().unwrap();
        let body = json!({"jsonrpc":"2.0","id":1,"method":"tools/list"});
        for (token, origin) in [
            ("wrong", None),
            (bridge.state.token.as_str(), None),
            (bridge.state.token.as_str(), Some("https://evil.example")),
        ] {
            let mut request = client.post(&bridge.endpoint).bearer_auth(token).json(&body);
            if let Some(origin) = origin {
                request = request.header("origin", origin);
            }
            assert_eq!(
                request.send().await.unwrap().status(),
                StatusCode::FORBIDDEN
            );
        }
        let lease = bridge.activate(context.clone());
        assert!(rpc(&bridge, "tools/list", json!({}))
            .await
            .get("result")
            .is_some());
        assert_eq!(
            client
                .post(&bridge.endpoint)
                .bearer_auth(&bridge.state.token)
                .header("origin", "https://evil.example")
                .json(&body)
                .send()
                .await
                .unwrap()
                .status(),
            StatusCode::FORBIDDEN
        );
        drop(lease);
        assert_eq!(
            client
                .post(&bridge.endpoint)
                .bearer_auth(&bridge.state.token)
                .json(&body)
                .send()
                .await
                .unwrap()
                .status(),
            StatusCode::FORBIDDEN
        );
        let _next = bridge.activate(context);
        assert!(rpc(&bridge, "tools/list", json!({}))
            .await
            .get("result")
            .is_some());
    }

    #[tokio::test]
    async fn notes_mcp_creation_and_editing_keep_decision_validation() {
        let (mut context, _root) = fixture().await;
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            axum::serve(
                listener,
                Router::new().fallback(|| async {
                    Json(json!({
                        "answers":{"cites_evidence":{"type":"noul","noul":0.01}},
                        "usage":{"input_tokens":10,"output_tokens":1}
                    }))
                }),
            )
            .await
            .unwrap();
        });
        let gate = DecisionGate::for_tests(
            &endpoint,
            crate::decision::DecisionScenarios {
                note_validation: true,
                ..Default::default()
            },
            0.7,
        );
        context.decision = Some(gate.clone());
        let bridge = NotesBridge::start().await.unwrap();
        let _lease = bridge.activate(context);
        let content = "# Design\nStatus: implemented\nSince: 2026-09-20\nCategory: 决策\n\n## Problem\nx\n## Decision\nNo evidence\n## Alternatives considered\nx\n## Consequences\nx";
        assert_eq!(
            call(
                &bridge,
                "CreateGroupNote",
                json!({"title":"Rejected","content":content})
            )
            .await["isError"],
            true
        );
        let note = output(&call(&bridge, "CreateGroupNote", json!({"title":"Draft"})).await);
        assert_eq!(call(&bridge,"EditGroupNote",json!({"path":note["path"],"edits":[{"old_text":"Status: proposed","new_text":"Status: implemented"}]})).await["isError"],true);
        let usage = gate.take_usage();
        assert_eq!(usage.len(), 2);
        assert_eq!(usage[0].input_tokens, 10);
        server.abort();
    }
    #[tokio::test]
    async fn notes_mcp_delayed_validation_cannot_write_after_revocation() {
        let (mut context, _root) = fixture().await;
        let started = Arc::new(tokio::sync::Notify::new());
        let release = Arc::new(tokio::sync::Notify::new());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let app = Router::new().fallback({
            let started = started.clone();
            let release = release.clone();
            move || {
                let started = started.clone();
                let release = release.clone();
                async move {
                    started.notify_one();
                    release.notified().await;
                    Json(json!({"answers":{"cites_evidence":{"type":"noul","noul":0.99}}}))
                }
            }
        });
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        context.decision = Some(DecisionGate::for_tests(
            &endpoint,
            crate::decision::DecisionScenarios {
                note_validation: true,
                ..Default::default()
            },
            0.7,
        ));
        let bridge = NotesBridge::start().await.unwrap();
        let lease = bridge.activate(context.clone());
        let request=reqwest::Client::builder().no_proxy().build().unwrap().post(&bridge.endpoint).bearer_auth(&bridge.state.token)
            .json(&json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"CreateGroupNote","arguments":{
                "title":"Late note","content":"# Late\nStatus: implemented\nSince: 2026-09-20\nCategory: 决策\n\n## Problem\nx\n## Decision\nTests passed\n## Alternatives considered\nx\n## Consequences\nx"
            }}}));
        let pending =
            tokio::spawn(
                async move { request.send().await.unwrap().json::<Value>().await.unwrap() },
            );
        tokio::time::timeout(Duration::from_secs(5), started.notified())
            .await
            .unwrap();
        drop(lease);
        release.notify_one();
        let result = pending.await.unwrap();
        assert_eq!(result["result"]["isError"], true, "{result}");
        let count: i64 = sqlx::query_scalar("SELECT count(*) FROM group_notes")
            .fetch_one(&context.pool)
            .await
            .unwrap();
        assert_eq!(count, 0);
        server.abort();
    }

    #[tokio::test]
    async fn notes_mcp_stale_edits_preserve_newer_content() {
        let (context, root) = fixture().await;
        let bridge = NotesBridge::start().await.unwrap();
        let _lease = bridge.activate(context.clone());
        let note = output(
            &call(
                &bridge,
                "CreateGroupNote",
                json!({"title":"Revision","content":"Original"}),
            )
            .await,
        );
        let path = root
            .path()
            .join("Notes")
            .join(note["path"].as_str().unwrap());
        std::fs::write(&path, "Newer edit").unwrap();
        let active = AtomicBool::new(true);
        let result = groups::update_group_note_with_pool(
            &context.pool,
            &context.owner_id,
            &context.group_id,
            note["id"].as_str().unwrap(),
            serde_json::from_value(json!({"content":"Stale replacement"})).unwrap(),
            Some(groups::GroupNoteWriteGuard {
                active: &active,
                expected_content: Some("Original"),
            }),
        )
        .await;
        assert_eq!(result.unwrap_err().status_code(), StatusCode::CONFLICT);
        assert_eq!(std::fs::read_to_string(path).unwrap(), "Newer edit");
        active.store(false, Ordering::SeqCst);
        let result = groups::create_group_note_with_pool(
            &context.pool,
            &context.owner_id,
            &context.group_id,
            serde_json::from_value(json!({"title":"Expired"})).unwrap(),
            Some(groups::GroupNoteWriteGuard {
                active: &active,
                expected_content: None,
            }),
        )
        .await;
        assert_eq!(result.unwrap_err().status_code(), StatusCode::FORBIDDEN);
    }
}
