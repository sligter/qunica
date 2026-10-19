use axum::{
    body::Body,
    http::{Request, StatusCode},
    Router,
};
use futures_util::{SinkExt, StreamExt};
use qunica_backend::api::{self, AppState};
use serde_json::{json, Value};
use tokio_tungstenite::{
    connect_async,
    tungstenite::{client::IntoClientRequest, Message},
};
use tower::ServiceExt;
use uuid::Uuid;

async fn request(
    app: &Router,
    method: &str,
    path: &str,
    token: Option<&str>,
    body: Value,
) -> (StatusCode, Value) {
    let mut builder = Request::builder()
        .method(method)
        .uri(path)
        .header("content-type", "application/json");
    if let Some(token) = token {
        builder = builder.header("authorization", format!("Bearer {token}"));
    }
    let response = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        app.clone()
            .oneshot(builder.body(Body::from(body.to_string())).unwrap()),
    )
    .await
    .unwrap()
    .unwrap();
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

async fn fixture() -> (Router, AppState, String, String) {
    let (app, state) = api::router_with_state_for_tests().await;
    let (_, user) = request(
        &app,
        "POST",
        "/api/v2/auth/register",
        None,
        json!({"email":"realtime@example.com","password":"supersecret","name":"Realtime"}),
    )
    .await;
    let (_, login) = request(
        &app,
        "POST",
        "/api/v2/auth/login",
        None,
        json!({"email":"realtime@example.com","password":"supersecret"}),
    )
    .await;
    let token = login["access_token"].as_str().unwrap().to_string();
    let group = Uuid::new_v4().to_string();
    sqlx::query("INSERT INTO groups(id,owner_id,name,status,created_at,updated_at) VALUES(?,?,'Group','active','2026-10-18T00:00:00Z','2026-10-18T00:00:00Z')")
        .bind(&group).bind(user["id"].as_str().unwrap()).execute(state.db.pool()).await.unwrap();
    (app, state, token, group)
}

async fn wait_job(state: &AppState, id: &str) -> String {
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            let status: String = sqlx::query_scalar("SELECT status FROM execution_jobs WHERE id=?")
                .bind(id)
                .fetch_one(state.db.pool())
                .await
                .unwrap();
            if !["queued", "running"].contains(&status.as_str()) {
                break status;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap()
}

#[tokio::test]
async fn queued_input_enters_model_history_only_after_claim_and_after_older_replies() {
    use qunica_backend::runtime::conversation_context::load_conversation;
    let (app, state, token, group) = fixture().await;
    let owner: String = sqlx::query_scalar("SELECT owner_id FROM groups WHERE id=?")
        .bind(&group)
        .fetch_one(state.db.pool())
        .await
        .unwrap();
    let thread = Uuid::new_v4().to_string();
    sqlx::query("INSERT INTO threads(id,group_id,status,next_seq,created_at,updated_at) VALUES(?,?,'running',2,'now','now')").bind(&thread).bind(&group).execute(state.db.pool()).await.unwrap();
    sqlx::query("INSERT INTO messages(id,thread_id,group_id,seq,sender_type,message_type,content,status,created_at) VALUES('previous-input',?,?,1,'user','text','previous request','visible','now')").bind(&thread).bind(&group).execute(state.db.pool()).await.unwrap();
    sqlx::query("INSERT INTO group_turns(id,thread_id,group_id,status,scheduler_strategy,config_snapshot_json,topology_snapshot_json,created_at,updated_at) VALUES('previous-turn',?,?,'running','bounded','{}','{}','now','now')").bind(&thread).bind(&group).execute(state.db.pool()).await.unwrap();
    sqlx::query("INSERT INTO execution_jobs(id,owner_id,request_id,command_name,group_id,thread_id,turn_id,stream_id,payload_json,status,generation,occupies_group,worker_active,created_at,updated_at) VALUES('previous-job',?,'previous-request','message.send',?,?,'previous-turn','previous-stream','{}','running',1,1,1,'now','now')").bind(&owner).bind(&group).bind(&thread).execute(state.db.pool()).await.unwrap();
    let send = |content: &str| json!({"request_id":Uuid::new_v4().to_string(),"name":"message.send","target":{"conversation_id":group,"thread_id":thread},"payload":{"content":content}});
    let (status, accepted) = request(
        &app,
        "POST",
        "/api/v2/commands",
        Some(&token),
        send("next request"),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{accepted}");
    let (status, cancelled) = request(
        &app,
        "POST",
        "/api/v2/commands",
        Some(&token),
        send("cancel before running"),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{cancelled}");
    let history = load_conversation(state.db.pool(), &thread).await.unwrap();
    assert_eq!(
        history
            .iter()
            .map(|m| m.content.as_str())
            .collect::<Vec<_>>(),
        vec!["previous request"]
    );
    // A late response and compaction can pass both acceptance sequence numbers.
    sqlx::query("INSERT INTO messages(id,thread_id,group_id,seq,sender_type,message_type,content,status,created_at) SELECT 'previous-reply',?,?,next_seq,'agent','text','previous response','visible','now' FROM threads WHERE id=?").bind(&thread).bind(&group).bind(&thread).execute(state.db.pool()).await.unwrap();
    sqlx::query("UPDATE threads SET next_seq=next_seq+1 WHERE id=?")
        .bind(&thread)
        .execute(state.db.pool())
        .await
        .unwrap();
    let reply_seq: i64 = sqlx::query_scalar("SELECT seq FROM messages WHERE id='previous-reply'")
        .fetch_one(state.db.pool())
        .await
        .unwrap();
    let (status, _) = request(&app,"POST","/api/v2/commands",Some(&token),json!({"request_id":Uuid::new_v4().to_string(),"name":"turn.cancel","target":{"turn_id":cancelled["turn_id"]},"payload":{}})).await;
    assert_eq!(status, StatusCode::ACCEPTED);
    sqlx::query("UPDATE group_turns SET status='completed' WHERE id='previous-turn'")
        .execute(state.db.pool())
        .await
        .unwrap();
    sqlx::query("UPDATE execution_jobs SET status='succeeded',occupies_group=0,worker_active=0 WHERE id='previous-job'").execute(state.db.pool()).await.unwrap();
    api::commands::launch(state.clone());
    wait_job(&state, accepted["job_id"].as_str().unwrap()).await;
    let history = load_conversation(state.db.pool(), &thread).await.unwrap();
    let next = history
        .iter()
        .find(|m| m.content == "next request")
        .unwrap();
    assert!(
        next.seq > reply_seq,
        "input must also survive an earlier through_seq checkpoint"
    );
    assert!(!history.iter().any(|m| m.content == "cancel before running"));
}

#[tokio::test]
async fn cancelling_an_old_or_queued_turn_preserves_the_current_approval() {
    let (app, state, token, group) = fixture().await;
    let thread = Uuid::new_v4().to_string();
    let old = Uuid::new_v4().to_string();
    let current = Uuid::new_v4().to_string();
    let queued = Uuid::new_v4().to_string();
    sqlx::query("INSERT INTO threads(id,group_id,status,next_seq,created_at,updated_at) VALUES(?,?,'paused',2,'now','now')").bind(&thread).bind(&group).execute(state.db.pool()).await.unwrap();
    for (turn, status) in [
        (&old, "completed"),
        (&current, "waiting_for_user"),
        (&queued, "queued"),
    ] {
        sqlx::query("INSERT INTO group_turns(id,thread_id,group_id,status,scheduler_strategy,config_snapshot_json,topology_snapshot_json,created_at,updated_at) VALUES(?,?,?,?,'bounded','{}','{}','now','now')").bind(turn).bind(&thread).bind(&group).bind(status).execute(state.db.pool()).await.unwrap();
    }
    let checkpoint=json!({"tool_calls":[{"tool_call_id":"current-call","tool_name":"Bash","args":{"command":"echo approved"},"status":"approval_required","approval_request":{"rule":"test","capability":"test","reason":"test","tool_name":"Bash","subject":"echo approved"}}]}).to_string();
    sqlx::query("INSERT INTO messages(id,thread_id,group_id,seq,sender_type,message_type,content,content_json,status,turn_id,created_at) VALUES('approval-checkpoint',?,?,1,'agent','text','',?,'interrupted',?,'now')").bind(&thread).bind(&group).bind(&checkpoint).bind(&current).execute(state.db.pool()).await.unwrap();
    qunica_backend::runtime::approval::ensure_pending(state.db.pool(), &thread, Some(&checkpoint))
        .await
        .unwrap();
    for turn in [&old, &queued] {
        let (status, _) = request(&app,"POST","/api/v2/commands",Some(&token),json!({"request_id":Uuid::new_v4().to_string(),"name":"turn.cancel","target":{"turn_id":turn},"payload":{}})).await;
        assert_eq!(status, StatusCode::ACCEPTED);
        let question: (String, i64) =
            sqlx::query_as("SELECT status,version FROM pending_approvals WHERE thread_id=?")
                .bind(&thread)
                .fetch_one(state.db.pool())
                .await
                .unwrap();
        assert_eq!(question, ("pending".into(), 1));
    }
    let (status, _) = request(&app,"POST","/api/v2/commands",Some(&token),json!({"request_id":Uuid::new_v4().to_string(),"name":"turn.cancel","target":{"turn_id":current},"payload":{}})).await;
    assert_eq!(status, StatusCode::ACCEPTED);
    let question: (String, i64) =
        sqlx::query_as("SELECT status,version FROM pending_approvals WHERE thread_id=?")
            .bind(&thread)
            .fetch_one(state.db.pool())
            .await
            .unwrap();
    assert_eq!(question, ("cancelled".into(), 2));
}

#[tokio::test]
async fn thread_snapshot_joins_live_text_and_tool_progress_stays_out_of_the_account_log() {
    let (app, state, token, group) = fixture().await;
    let owner: String = sqlx::query_scalar("SELECT owner_id FROM groups WHERE id=?")
        .bind(&group)
        .fetch_one(state.db.pool())
        .await
        .unwrap();
    let thread = Uuid::new_v4().to_string();
    let stream = Uuid::new_v4().to_string();
    let pool = state.db.pool();
    sqlx::query("INSERT INTO threads(id,group_id,status,next_seq,created_at,updated_at) VALUES(?,?,'active',1,'now','now')")
        .bind(&thread).bind(&group).execute(pool).await.unwrap();
    sqlx::query("INSERT INTO execution_jobs(id,owner_id,request_id,command_name,group_id,thread_id,turn_id,stream_id,payload_json,status,created_at,updated_at) VALUES(?,?,?,'message.send',?,?,?,?,'{}','running','now','now')")
        .bind(Uuid::new_v4().to_string()).bind(&owner).bind(&stream).bind(&group).bind(&thread)
        .bind(Uuid::new_v4().to_string()).bind(&stream).execute(pool).await.unwrap();
    let events = [
        ("token", json!({"agent_id":"a","text":"old"})),
        ("agent_message", json!({"agent_id":"a"})),
        ("token", json!({"agent_id":"a","text":"he"})),
        ("token", json!({"agent_id":"b","delta":"hi"})),
        (
            "tool_call_result",
            json!({"agent_id":"a","tool_call_id":"t","status":"started"}),
        ),
        (
            "tool_call_result",
            json!({"agent_id":"a","tool_call_id":"t","result_summary":"…"}),
        ),
        ("token", json!({"agent_id":"a","text":"llo"})),
    ];
    let before: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM account_events WHERE kind='file.changed'")
            .fetch_one(pool)
            .await
            .unwrap();
    for (seq, (kind, payload)) in events.iter().enumerate() {
        sqlx::query("INSERT INTO stream_events(id,stream_id,thread_id,seq,event_id,kind,payload_json,created_at) VALUES(?,?,?,?,?,?,?,'now')")
            .bind(Uuid::new_v4().to_string()).bind(&stream).bind(&thread).bind(seq as i64)
            .bind(format!("{stream}:{seq}")).bind(kind).bind(payload.to_string()).execute(pool).await.unwrap();
    }
    let file_events = || async {
        sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM account_events WHERE kind='file.changed'",
        )
        .fetch_one(pool)
        .await
        .unwrap()
            - before
    };
    assert_eq!(
        file_events().await,
        0,
        "tool progress must not invalidate files"
    );
    sqlx::query("INSERT INTO stream_events(id,stream_id,thread_id,seq,event_id,kind,payload_json,created_at) VALUES(?,?,?,7,?,'tool_call_result',?,'now')")
        .bind(Uuid::new_v4().to_string()).bind(&stream).bind(&thread).bind(format!("{stream}:7"))
        .bind(json!({"agent_id":"a","tool_call_id":"t","status":"completed"}).to_string()).execute(pool).await.unwrap();
    assert_eq!(file_events().await, 1);

    let (status, snapshot) = request(
        &app,
        "GET",
        &format!("/api/v2/threads/{thread}/snapshot"),
        Some(&token),
        Value::Null,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let mut output: Vec<(String, String, i64)> = snapshot["inflight_output"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| {
            (
                row["agent_id"].as_str().unwrap().to_string(),
                row["text"].as_str().unwrap().to_string(),
                row["seq"].as_i64().unwrap(),
            )
        })
        .collect();
    output.sort();
    assert_eq!(
        output,
        vec![
            ("a".into(), "hello".into(), 6),
            ("b".into(), "hi".into(), 3)
        ]
    );
}

#[tokio::test]
async fn snapshot_boundary_replays_committed_mutations_without_secrets_or_token_events() {
    let (app, state, token, group) = fixture().await;
    let (status, snapshot) = request(
        &app,
        "GET",
        "/api/v2/account/snapshot",
        Some(&token),
        Value::Null,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(snapshot["groups"][0]["id"], group);
    let cursor = snapshot["cursor"].as_str().unwrap().parse().unwrap();
    let owner: String = sqlx::query_scalar("SELECT owner_id FROM groups WHERE id=?")
        .bind(&group)
        .fetch_one(state.db.pool())
        .await
        .unwrap();
    let mut tx = state.db.begin_write().await.unwrap();
    sqlx::query("UPDATE groups SET name='committed' WHERE id=?")
        .bind(&group)
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    let events = api::account::events_after(state.db.pool(), &owner, cursor)
        .await
        .unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].kind, "group.changed");
    assert!(events[0].seq > cursor);
    let mut tx = state.db.begin_write().await.unwrap();
    sqlx::query("UPDATE groups SET name='rolled back' WHERE id=?")
        .bind(&group)
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.rollback().await.unwrap();
    assert_eq!(
        api::account::events_after(state.db.pool(), &owner, cursor)
            .await
            .unwrap()
            .len(),
        1
    );
    assert!(
        api::account::events_after(state.db.pool(), "another-account", 0)
            .await
            .unwrap()
            .is_empty()
    );
    let raw = events[0].wire().to_string();
    assert!(!raw.contains("password"));
    assert!(!raw.contains("api_key"));
}

#[tokio::test]
async fn concurrent_identical_commands_create_one_message_job_turn_and_conflicts_are_rejected() {
    let (app, state, token, group) = fixture().await;
    let command = json!({"request_id":Uuid::new_v4().to_string(),"name":"message.send","target":{"conversation_id":group},"payload":{"content":"hello"}});
    let (left, right) = tokio::join!(
        request(
            &app,
            "POST",
            "/api/v2/commands",
            Some(&token),
            command.clone()
        ),
        request(
            &app,
            "POST",
            "/api/v2/commands",
            Some(&token),
            command.clone()
        )
    );
    assert_eq!(left.0, StatusCode::ACCEPTED, "{left:?}");
    assert_eq!(right.0, StatusCode::ACCEPTED, "{right:?}");
    assert_eq!(left.1, right.1);
    assert_eq!(
        wait_job(&state, left.1["job_id"].as_str().unwrap()).await,
        "succeeded"
    );
    for table in [
        "messages",
        "execution_jobs",
        "group_turns",
        "command_receipts",
    ] {
        let count: i64 = sqlx::query_scalar(&format!("SELECT COUNT(*) FROM {table}"))
            .fetch_one(state.db.pool())
            .await
            .unwrap();
        assert_eq!(count, 1, "{table}");
    }
    let mut conflicting = command.clone();
    conflicting["payload"]["content"] = json!("different");
    let (status, error) =
        request(&app, "POST", "/api/v2/commands", Some(&token), conflicting).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(error["error"]["code"], "idempotency_conflict");
    // Tombstones remain authoritative after clearing all conversation history.
    sqlx::query("UPDATE messages SET status='deleted'")
        .execute(state.db.pool())
        .await
        .unwrap();
    sqlx::query("UPDATE threads SET status='cleared'")
        .execute(state.db.pool())
        .await
        .unwrap();
    sqlx::query("UPDATE groups SET status='deleted'")
        .execute(state.db.pool())
        .await
        .unwrap();
    let replay = request(&app, "POST", "/api/v2/commands", Some(&token), command).await;
    assert_eq!(replay.0, StatusCode::ACCEPTED);
    assert_eq!(replay.1["job_id"], left.1["job_id"]);
}

#[tokio::test]
async fn retention_preserves_a_floor_and_server_identity_survives_router_recreation() {
    let (app, state, token, group) = fixture().await;
    let (_, snapshot) = request(
        &app,
        "GET",
        "/api/v2/account/snapshot",
        Some(&token),
        Value::Null,
    )
    .await;
    let owner: String = sqlx::query_scalar("SELECT owner_id FROM groups WHERE id=?")
        .bind(group)
        .fetch_one(state.db.pool())
        .await
        .unwrap();
    sqlx::query("UPDATE account_events SET occurred_at='2000-01-01T00:00:00Z'")
        .execute(state.db.pool())
        .await
        .unwrap();
    api::account::prune(state.db.pool()).await.unwrap();
    let error = api::account::events_after(state.db.pool(), &owner, 0)
        .await
        .unwrap_err();
    assert_eq!(error.code(), "resync_required");
    let high = snapshot["cursor"].as_str().unwrap().parse().unwrap();
    assert!(api::account::events_after(state.db.pool(), &owner, high)
        .await
        .unwrap()
        .is_empty());
    let restarted = api::router(state);
    assert_eq!(
        request(&restarted, "GET", "/api/v2/instance", None, Value::Null)
            .await
            .1["server_id"],
        snapshot["server_id"]
    );
    let (status, error) = request(
        &restarted,
        "GET",
        "/api/v2/account/events?cursor=0&server_id=wrong",
        Some(&token),
        Value::Null,
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(error["error"]["code"], "server_mismatch");
}

type Socket =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;
async fn frame(socket: &mut Socket) -> Value {
    let frame = tokio::time::timeout(std::time::Duration::from_secs(5), socket.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    serde_json::from_str(frame.to_text().unwrap()).unwrap()
}
/// The next frame's events: a batched `events` frame unpacks, others pass through.
async fn events(socket: &mut Socket) -> Vec<Value> {
    let frame = frame(socket).await;
    if frame["type"] == "events" {
        let events = frame["events"].as_array().unwrap().clone();
        assert_eq!(frame["cursor"], events.last().unwrap()["cursor"]);
        events
    } else {
        vec![frame]
    }
}
async fn open(address: std::net::SocketAddr) -> Socket {
    let mut request = format!("ws://{address}/api/v2/realtime")
        .into_client_request()
        .unwrap();
    request.headers_mut().insert(
        "sec-websocket-protocol",
        api::realtime::PROTOCOL.parse().unwrap(),
    );
    connect_async(request).await.unwrap().0
}

async fn terminal_socket(
    app: &Router,
    address: std::net::SocketAddr,
    token: &str,
    session: &str,
) -> (Socket, Value) {
    let (_, ticket) = request(
        app,
        "POST",
        "/api/v2/realtime/tickets",
        Some(token),
        Value::Null,
    )
    .await;
    let mut req = format!("ws://{address}/api/v2/terminal/realtime")
        .into_client_request()
        .unwrap();
    req.headers_mut().insert(
        "sec-websocket-protocol",
        api::terminal_realtime::PROTOCOL.parse().unwrap(),
    );
    let mut socket = connect_async(req).await.unwrap().0;
    assert_eq!(
        frame(&mut socket).await["protocol"],
        api::terminal_realtime::PROTOCOL
    );
    socket.send(Message::Text(json!({"type":"authenticate","ticket":ticket["ticket"],"session_id":session,"cursor":"0"}).to_string())).await.unwrap();
    let reply = frame(&mut socket).await;
    (socket, reply)
}
async fn terminal_reply(socket: &mut Socket, request_id: &str) -> Value {
    loop {
        let value = frame(socket).await;
        if value["request_id"] == request_id {
            return value;
        }
    }
}

#[tokio::test]
async fn filesystem_writes_publish_durable_invalidations_and_cancelled_requests_settle_intents() {
    let (app, state, token, group) = fixture().await;
    let owner: String = sqlx::query_scalar("SELECT owner_id FROM groups WHERE id=?")
        .bind(&group)
        .fetch_one(state.db.pool())
        .await
        .unwrap();
    let root = tempfile::tempdir().unwrap();
    let (status, workspace) = request(
        &app,
        "POST",
        "/api/v2/workspaces",
        Some(&token),
        json!({"name":"Files","backend_type":"local","local_path":root.path()}),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{workspace}");
    sqlx::query("UPDATE groups SET workspace_id=? WHERE id=?")
        .bind(workspace["id"].as_str().unwrap())
        .bind(&group)
        .execute(state.db.pool())
        .await
        .unwrap();
    std::fs::write(root.path().join("test.txt"), "before").unwrap();
    let (_, file) = request(
        &app,
        "GET",
        &format!("/api/v2/groups/{group}/workspace-files/text?path=test.txt"),
        Some(&token),
        Value::Null,
    )
    .await;
    let (_, snapshot) = request(
        &app,
        "GET",
        "/api/v2/account/snapshot",
        Some(&token),
        Value::Null,
    )
    .await;
    let cursor = snapshot["cursor"].as_str().unwrap().parse().unwrap();
    let (status, saved) = request(
        &app,
        "PATCH",
        &format!("/api/v2/groups/{group}/workspace-files/text/save?path=test.txt"),
        Some(&token),
        json!({"content":"private updated content","version":file["version"]}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{saved}");
    assert_eq!(
        std::fs::read_to_string(root.path().join("test.txt")).unwrap(),
        "private updated content"
    );
    let events = api::account::events_after(state.db.pool(), &owner, cursor)
        .await
        .unwrap();
    assert!(events
        .iter()
        .any(|event| event.kind == "file.changed" && event.group_id.as_deref() == Some(&group)));
    assert!(
        !serde_json::to_string(&events.iter().map(|event| event.wire()).collect::<Vec<_>>())
            .unwrap()
            .contains("private updated content")
    );

    let blocked = Router::new()
        .route(
            "/api/v2/groups/:id/workspace-files/blocked",
            axum::routing::post(|| async { std::future::pending::<StatusCode>().await }),
        )
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            api::workspace_events::track,
        ));
    let req = Request::builder()
        .method("POST")
        .uri(format!("/api/v2/groups/{group}/workspace-files/blocked"))
        .header("authorization", format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap();
    let pending = tokio::spawn(async move { blocked.oneshot(req).await });
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM workspace_event_intents")
                .fetch_one(state.db.pool())
                .await
                .unwrap();
            if count == 1 {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    pending.abort();
    let _ = pending.await;
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM workspace_event_intents")
                .fetch_one(state.db.pool())
                .await
                .unwrap();
            if count == 0 {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    sqlx::query("INSERT INTO workspace_event_intents(id,owner_id,kind,resource_id,group_id) VALUES('interrupted',?,'file.changed',?,?)").bind(&owner).bind(&group).bind(&group).execute(state.db.pool()).await.unwrap();
    api::workspace_events::recover(state.db.pool())
        .await
        .unwrap();
    let events = api::account::events_after(state.db.pool(), &owner, cursor)
        .await
        .unwrap();
    assert_eq!(
        events
            .iter()
            .filter(|event| event.kind == "file.changed")
            .count(),
        3
    );
}

#[tokio::test]
async fn ended_run_retention_returns_explicit_resync_and_keeps_pending_approval_material() {
    let (app, state, token, group) = fixture().await;
    let thread = Uuid::new_v4().to_string();
    sqlx::query("INSERT INTO threads(id,group_id,status,next_seq,created_at,updated_at) VALUES(?,?,'active',1,'now','now')").bind(&thread).bind(&group).execute(state.db.pool()).await.unwrap();
    let stream = Uuid::new_v4().to_string();
    for (seq, kind) in [(0, "agent_start"), (1, "done")] {
        sqlx::query("INSERT INTO stream_events(id,stream_id,thread_id,seq,event_id,kind,payload_json,created_at) VALUES(?,?,?,?,?,?, '{}','2000-01-01T00:00:00Z')")
            .bind(Uuid::new_v4().to_string()).bind(&stream).bind(&thread).bind(seq).bind(format!("{stream}:{seq}")).bind(kind).execute(state.db.pool()).await.unwrap();
    }
    api::account::prune(state.db.pool()).await.unwrap();
    let (status, error) = request(
        &app,
        "GET",
        &format!("/api/v2/groups/{group}/streams/{stream}"),
        Some(&token),
        Value::Null,
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{error}");
    assert_eq!(error["error"]["code"], "resync_required");
    let floor: i64 =
        sqlx::query_scalar("SELECT retained_after FROM run_replay_floors WHERE stream_id=?")
            .bind(&stream)
            .fetch_one(state.db.pool())
            .await
            .unwrap();
    assert_eq!(floor, 1);

    let owner: String = sqlx::query_scalar("SELECT owner_id FROM groups WHERE id=?")
        .bind(&group)
        .fetch_one(state.db.pool())
        .await
        .unwrap();
    sqlx::query("INSERT INTO pending_approvals(id,owner_id,group_id,thread_id,tool_call_id,action_hash,action_json,request_json,created_at,updated_at) VALUES('keep',?,?,?,'call','hash','{}','{}','now','now')").bind(&owner).bind(&group).bind(&thread).execute(state.db.pool()).await.unwrap();
    let active = Uuid::new_v4().to_string();
    sqlx::query("INSERT INTO stream_events(id,stream_id,thread_id,seq,event_id,kind,payload_json,created_at) VALUES(?,?,?,0,?,'done','{}','2000-01-01T00:00:00Z')").bind(Uuid::new_v4().to_string()).bind(&active).bind(&thread).bind(format!("{active}:0")).execute(state.db.pool()).await.unwrap();
    api::account::prune(state.db.pool()).await.unwrap();
    let retained: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM stream_events WHERE stream_id=?")
        .bind(active)
        .fetch_one(state.db.pool())
        .await
        .unwrap();
    assert_eq!(retained, 1);
}

#[tokio::test]
async fn terminal_socket_separates_observation_from_fenced_input_and_reports_missing_sessions() {
    use qunica_backend::{terminal::CreateTerminalOptions, tools::ShellPreference};
    let (app, state, token, group) = fixture().await;
    let owner: String = sqlx::query_scalar("SELECT owner_id FROM groups WHERE id=?")
        .bind(&group)
        .fetch_one(state.db.pool())
        .await
        .unwrap();
    let root = tempfile::tempdir().unwrap();
    let terminal = state
        .terminals
        .create(CreateTerminalOptions {
            owner_id: owner.clone(),
            conversation_id: group,
            cwd: root.path().to_string_lossy().to_string(),
            cols: 80,
            rows: 24,
            shell: ShellPreference::Auto,
        })
        .unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let service = app.clone();
    let server = tokio::spawn(async move {
        axum::serve(listener, service).await.unwrap();
    });
    let (mut first, auth) = terminal_socket(&app, address, &token, &terminal.id).await;
    assert_eq!(auth["generation"], "1");
    let (mut second, auth) = terminal_socket(&app, address, &token, &terminal.id).await;
    assert!(auth["generation"].is_null());
    second
        .send(Message::Text(
            json!({"type":"resize","request_id":"observer","generation":"1","cols":90,"rows":30})
                .to_string(),
        ))
        .await
        .unwrap();
    assert_eq!(
        terminal_reply(&mut second, "observer").await["code"],
        "terminal.lease_required"
    );
    first
        .send(Message::Text(
            json!({"type":"resize","request_id":"holder","generation":"1","cols":90,"rows":30})
                .to_string(),
        ))
        .await
        .unwrap();
    assert_eq!(terminal_reply(&mut first, "holder").await["type"], "ack");
    first.close(None).await.unwrap();
    let next = tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            second
                .send(Message::Text(
                    json!({"type":"acquire_input","request_id":"take"}).to_string(),
                ))
                .await
                .unwrap();
            let reply = terminal_reply(&mut second, "take").await;
            if reply["generation"].is_string() {
                break reply["generation"].clone();
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(next, "2");
    second
        .send(Message::Text(
            json!({"type":"resize","request_id":"old","generation":"1","cols":90,"rows":30})
                .to_string(),
        ))
        .await
        .unwrap();
    assert_eq!(
        terminal_reply(&mut second, "old").await["code"],
        "terminal.lease_required"
    );
    second
        .send(Message::Text(
            json!({"type":"resize","request_id":"new","generation":next,"cols":90,"rows":30})
                .to_string(),
        ))
        .await
        .unwrap();
    assert_eq!(terminal_reply(&mut second, "new").await["type"], "ack");
    state.terminals.close(&terminal.id, &owner);
    let (_, missing) = terminal_socket(&app, address, &token, &terminal.id).await;
    assert_eq!(missing["code"], "terminal.session_missing");
    server.abort();
}

#[tokio::test]
async fn real_websocket_uses_single_use_ticket_discovers_other_device_and_revokes_live_connection()
{
    let (app, _state, token, group) = fixture().await;
    let (_, snapshot) = request(
        &app,
        "GET",
        "/api/v2/account/snapshot",
        Some(&token),
        Value::Null,
    )
    .await;
    let (_, ticket) = request(
        &app,
        "POST",
        "/api/v2/realtime/tickets",
        Some(&token),
        Value::Null,
    )
    .await;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let service = app.clone();
    let server = tokio::spawn(async move {
        axum::serve(listener, service).await.unwrap();
    });
    let mut first = open(address).await;
    assert_eq!(frame(&mut first).await["type"], "hello");
    let authenticate = json!({"type":"authenticate","ticket":ticket["ticket"]});
    first
        .send(Message::Text(authenticate.to_string()))
        .await
        .unwrap();
    assert_eq!(frame(&mut first).await["type"], "authenticated");
    let mut duplicate = open(address).await;
    frame(&mut duplicate).await;
    duplicate
        .send(Message::Text(authenticate.to_string()))
        .await
        .unwrap();
    assert_eq!(frame(&mut duplicate).await["code"], "unauthorized");
    first.send(Message::Text(json!({"type":"subscribe","scope":"account","cursor":snapshot["cursor"],"server_id":snapshot["server_id"]}).to_string())).await.unwrap();
    assert_eq!(frame(&mut first).await["type"], "subscribed");
    let (_, second) = request(
        &app,
        "POST",
        "/api/v2/auth/login",
        None,
        json!({"email":"realtime@example.com","password":"supersecret"}),
    )
    .await;
    let second = second["access_token"].as_str().unwrap();
    let command = json!({"request_id":Uuid::new_v4().to_string(),"name":"message.send","target":{"conversation_id":group},"payload":{"content":"from another device"}});
    assert_eq!(
        request(&app, "POST", "/api/v2/commands", Some(second), command)
            .await
            .0,
        StatusCode::ACCEPTED
    );
    let observed = tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            if let Some(event) = events(&mut first)
                .await
                .into_iter()
                .find(|event| event["kind"] == "message.changed")
            {
                break event;
            }
        }
    })
    .await
    .unwrap();
    assert_eq!(observed["group_id"], group);
    assert!(observed["cursor"].is_string());
    assert_eq!(
        request(
            &app,
            "POST",
            "/api/v2/auth/logout",
            Some(&token),
            Value::Null
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
    tokio::time::timeout(std::time::Duration::from_secs(3), async {
        while let Some(Ok(message)) = first.next().await {
            if message.is_close() {
                break;
            }
        }
    })
    .await
    .unwrap();
    assert_eq!(
        request(&app, "GET", "/api/v2/auth/me", Some(second), Value::Null)
            .await
            .0,
        StatusCode::OK
    );
    server.abort();
}

#[tokio::test]
async fn approval_survives_restart_and_two_opposing_decisions_only_claim_one_resume() {
    let (app, state, token, group) = fixture().await;
    let owner: String = sqlx::query_scalar("SELECT owner_id FROM groups WHERE id=?")
        .bind(&group)
        .fetch_one(state.db.pool())
        .await
        .unwrap();
    let agent = Uuid::new_v4().to_string();
    let thread = Uuid::new_v4().to_string();
    let message = Uuid::new_v4().to_string();
    let turn = Uuid::new_v4().to_string();
    sqlx::query("INSERT INTO agents(id,owner_id,name,system_prompt,created_at,updated_at) VALUES(?,?,'Agent','test','now','now')").bind(&agent).bind(&owner).execute(state.db.pool()).await.unwrap();
    sqlx::query(
        "INSERT INTO group_agents(group_id,agent_id,joined_at,updated_at) VALUES(?,?,'now','now')",
    )
    .bind(&group)
    .bind(&agent)
    .execute(state.db.pool())
    .await
    .unwrap();
    sqlx::query("INSERT INTO threads(id,group_id,status,next_seq,created_at,updated_at) VALUES(?,?,'paused',2,'now','now')").bind(&thread).bind(&group).execute(state.db.pool()).await.unwrap();
    let checkpoint=json!({"tool_calls":[{"tool_call_id":"call-1","tool_name":"Bash","args":{"command":"echo approved"},"status":"approval_required","approval_request":{"rule":"test","capability":"test","reason":"test","tool_name":"Bash","subject":"echo approved"}}]}).to_string();
    sqlx::query("INSERT INTO messages(id,thread_id,group_id,seq,sender_type,sender_id,message_type,content,content_json,status,created_at) VALUES(?,?,?,1,'agent',?,'text','',?,'interrupted','now')").bind(&message).bind(&thread).bind(&group).bind(&agent).bind(&checkpoint).execute(state.db.pool()).await.unwrap();
    sqlx::query("INSERT INTO group_turns(id,thread_id,group_id,status,scheduler_strategy,config_snapshot_json,topology_snapshot_json,created_at,updated_at) VALUES(?,?,?,'waiting_for_user','bounded','{}','{}','now','now')").bind(&turn).bind(&thread).bind(&group).execute(state.db.pool()).await.unwrap();
    qunica_backend::runtime::approval::ensure_pending(state.db.pool(), &thread, Some(&checkpoint))
        .await
        .unwrap();
    qunica_backend::runtime::group_scheduler::SchedulerStore::new(
        state.db.pool().clone(),
        state.write_lock.clone(),
    )
    .recover_incomplete_turns()
    .await
    .unwrap();
    let status: String = sqlx::query_scalar("SELECT status FROM group_turns WHERE id=?")
        .bind(&turn)
        .fetch_one(state.db.pool())
        .await
        .unwrap();
    assert_eq!(status, "waiting_for_user");
    let (_, snapshot) = request(
        &app,
        "GET",
        "/api/v2/account/snapshot",
        Some(&token),
        Value::Null,
    )
    .await;
    assert_eq!(snapshot["pending_approvals"].as_array().unwrap().len(), 1);
    let hash = snapshot["pending_approvals"][0]["action_hash"].clone();
    let base = json!({"request_id":Uuid::new_v4().to_string(),"name":"approval.resolve","target":{"thread_id":thread},"payload":{"tool_call_id":"call-1","approved":false,"expected_version":99,"action_hash":hash}});
    let invalid = request(&app, "POST", "/api/v2/commands", Some(&token), base.clone()).await;
    assert_eq!(invalid.0, StatusCode::CONFLICT);
    let mut one = base.clone();
    one["request_id"] = json!(Uuid::new_v4().to_string());
    one["payload"]["expected_version"] = json!(1);
    let mut two = one.clone();
    two["request_id"] = json!(Uuid::new_v4().to_string());
    two["payload"]["approved"] = json!(true);
    let (one, two) = tokio::join!(
        request(&app, "POST", "/api/v2/commands", Some(&token), one),
        request(&app, "POST", "/api/v2/commands", Some(&token), two)
    );
    assert!(
        (one.0 == StatusCode::ACCEPTED && two.0 == StatusCode::CONFLICT)
            || (two.0 == StatusCode::ACCEPTED && one.0 == StatusCode::CONFLICT),
        "{one:?} {two:?}"
    );
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM execution_jobs")
        .fetch_one(state.db.pool())
        .await
        .unwrap();
    assert_eq!(count, 1);
    let (status, version): (String, i64) =
        sqlx::query_as("SELECT status,version FROM pending_approvals WHERE thread_id=?")
            .bind(&thread)
            .fetch_one(state.db.pool())
            .await
            .unwrap();
    assert_eq!(status, "resolved");
    assert_eq!(version, 2);
}

#[tokio::test]
async fn websocket_commands_share_http_receipts_and_replay_run_cursors() {
    let (app, state, token, group) = fixture().await;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let service = app.clone();
    let server = tokio::spawn(async move {
        axum::serve(listener, service).await.unwrap();
    });
    let (_, ticket) = request(
        &app,
        "POST",
        "/api/v2/realtime/tickets",
        Some(&token),
        Value::Null,
    )
    .await;
    let mut socket = open(address).await;
    let hello = frame(&mut socket).await;
    socket
        .send(Message::Text(
            json!({"type":"authenticate","ticket":ticket["ticket"]}).to_string(),
        ))
        .await
        .unwrap();
    assert_eq!(frame(&mut socket).await["type"], "authenticated");
    let command = json!({"type":"command","request_id":Uuid::new_v4().to_string(),"name":"message.send","target":{"conversation_id":group},"payload":{"content":"socket request"}});
    socket
        .send(Message::Text(command.to_string()))
        .await
        .unwrap();
    let result = frame(&mut socket).await;
    assert_eq!(result["type"], "command_result");
    assert_eq!(result["result"]["status"], "accepted", "{result}");
    let duplicate = request(&app, "POST", "/api/v2/commands", Some(&token), command).await;
    assert_eq!(duplicate.1, result["result"]);
    wait_job(&state, result["result"]["job_id"].as_str().unwrap()).await;
    let scope = format!("run:{}", result["result"]["stream_id"].as_str().unwrap());
    socket
        .send(Message::Text(
            json!({"type":"subscribe","scope":scope,"cursor":"-1","server_id":hello["server_id"]})
                .to_string(),
        ))
        .await
        .unwrap();
    assert_eq!(frame(&mut socket).await["type"], "subscribed");
    let mut user_messages = 0;
    let mut previous = -1;
    'frames: loop {
        for event in events(&mut socket).await {
            let seq = event["seq"].as_i64().unwrap();
            assert!(seq > previous);
            previous = seq;
            if event["kind"] == "user_message" {
                user_messages += 1;
            }
            if event["kind"] == "done" {
                break 'frames;
            }
        }
    }
    assert_eq!(user_messages, 1);
    socket.send(Message::Text(json!({"type":"subscribe","scope":format!("run:{}",Uuid::new_v4()),"cursor":"-1","server_id":hello["server_id"]}).to_string())).await.unwrap();
    assert_eq!(frame(&mut socket).await["code"], "not_found");
    server.abort();
}

#[tokio::test]
async fn paired_account_sse_uses_the_same_cursor_and_closes_on_revocation() {
    let (app, state, token, group) = fixture().await;
    let (_, snapshot) = request(
        &app,
        "GET",
        "/api/v2/account/snapshot",
        Some(&token),
        Value::Null,
    )
    .await;
    let path = format!(
        "/api/v2/account/events?cursor={}&server_id={}",
        snapshot["cursor"].as_str().unwrap(),
        snapshot["server_id"].as_str().unwrap()
    );
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(path)
                .header("authorization", format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let mut stream = response.into_body().into_data_stream();
    sqlx::query("UPDATE groups SET name='from direct client' WHERE id=?")
        .bind(&group)
        .execute(state.db.pool())
        .await
        .unwrap();
    let bytes = tokio::time::timeout(std::time::Duration::from_secs(2), stream.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    let wire = std::str::from_utf8(&bytes).unwrap();
    assert!(wire.contains("group.changed"));
    assert!(wire.contains(&group));
    assert_eq!(
        request(
            &app,
            "POST",
            "/api/v2/auth/logout",
            Some(&token),
            Value::Null
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        while stream.next().await.is_some() {}
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn a_durable_queued_job_survives_database_reopen_and_is_claimed_once() {
    use qunica_backend::{
        db::Db,
        server::{build_state, ServerConfig},
    };
    let dir = tempfile::tempdir().unwrap();
    let url = format!(
        "sqlite:{}?mode=rwc",
        dir.path().join("jobs.sqlite3").display()
    );
    let db = Db::connect(&url).await.unwrap();
    db.migrate().await.unwrap();
    let owner = Uuid::new_v4().to_string();
    let group = Uuid::new_v4().to_string();
    let thread = Uuid::new_v4().to_string();
    let turn = Uuid::new_v4().to_string();
    let job = Uuid::new_v4().to_string();
    let message = Uuid::new_v4().to_string();
    let stream = Uuid::new_v4().to_string();
    let mut tx = db.begin_write().await.unwrap();
    sqlx::query("INSERT INTO users(id,email,password_hash,name,created_at,updated_at) VALUES(?,'restart@example.com','unused','Owner','now','now')").bind(&owner).execute(&mut *tx).await.unwrap();
    sqlx::query("INSERT INTO groups(id,owner_id,name,created_at,updated_at) VALUES(?,?,'Group','now','now')").bind(&group).bind(&owner).execute(&mut *tx).await.unwrap();
    sqlx::query("INSERT INTO threads(id,group_id,status,next_seq,created_at,updated_at) VALUES(?,?,'active',2,'now','now')").bind(&thread).bind(&group).execute(&mut *tx).await.unwrap();
    sqlx::query("INSERT INTO messages(id,thread_id,group_id,seq,sender_type,sender_id,message_type,content,created_at) VALUES(?,?,?,1,'user',?,'text','hello','now')").bind(&message).bind(&thread).bind(&group).bind(&owner).execute(&mut *tx).await.unwrap();
    sqlx::query("INSERT INTO group_turns(id,thread_id,group_id,trigger_message_id,status,scheduler_strategy,config_snapshot_json,topology_snapshot_json,created_at,updated_at) VALUES(?,?,?,?,'pending','bounded','{}','{}','now','now')").bind(&turn).bind(&thread).bind(&group).bind(&message).execute(&mut *tx).await.unwrap();
    sqlx::query("INSERT INTO stream_events(id,stream_id,thread_id,seq,event_id,kind,payload_json,created_at) VALUES(?,?,?,0,?,'user_message',?,'now')").bind(Uuid::new_v4().to_string()).bind(&stream).bind(&thread).bind(format!("{stream}:0")).bind(json!({"message_id":message,"content":"hello","thread_id":thread}).to_string()).execute(&mut *tx).await.unwrap();
    sqlx::query("INSERT INTO execution_jobs(id,owner_id,request_id,command_name,group_id,thread_id,turn_id,stream_id,message_id,payload_json,status,created_at,updated_at) VALUES(?,?,?,'message.send',?,?,?,?,?,?,'queued','now','now')")
        .bind(&job).bind(&owner).bind(&stream).bind(&group).bind(&thread).bind(&turn).bind(&stream).bind(&message).bind(json!({"content":"hello","attachments":[],"model_override":null,"effort_override":null}).to_string()).execute(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    db.pool().close().await;
    let state = build_state(&ServerConfig {
        host: "127.0.0.1".into(),
        port: 0,
        database_url: url,
        secret_key: "secret".into(),
        registration_enabled: true,
        initial_user: None,
        app_data_dir: Some(dir.path().join("app")),
        web_dir: None,
        workspaces_dir: None,
    })
    .await
    .unwrap();
    api::commands::launch(state.clone());
    api::commands::launch(state.clone());
    assert_eq!(wait_job(&state, &job).await, "succeeded");
    let generation: i64 = sqlx::query_scalar("SELECT generation FROM execution_jobs WHERE id=?")
        .bind(&job)
        .fetch_one(state.db.pool())
        .await
        .unwrap();
    assert_eq!(generation, 1);
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM messages")
            .fetch_one(state.db.pool())
            .await
            .unwrap(),
        1
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM group_turns")
            .fetch_one(state.db.pool())
            .await
            .unwrap(),
        1
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM stream_events WHERE kind='done'")
            .fetch_one(state.db.pool())
            .await
            .unwrap(),
        1
    );
    state.db.pool().close().await;
}

#[tokio::test]
async fn recovery_reconciles_committed_results_and_waits_but_never_replays_uncertain_work() {
    let (app, state, token, group) = fixture().await;
    let (_,accepted)=request(&app,"POST","/api/v2/commands",Some(&token),json!({"request_id":Uuid::new_v4().to_string(),"name":"message.send","target":{"conversation_id":group},"payload":{"content":"recovery"}})).await;
    let job = accepted["job_id"].as_str().unwrap();
    wait_job(&state, job).await;
    for (turn_status, expected) in [
        ("completed", "succeeded"),
        ("waiting_for_user", "waiting_for_user"),
        ("failed", "interrupted"),
    ] {
        sqlx::query("UPDATE group_turns SET status=? WHERE id=?")
            .bind(turn_status)
            .bind(accepted["turn_id"].as_str())
            .execute(state.db.pool())
            .await
            .unwrap();
        sqlx::query("UPDATE execution_jobs SET status='running',error=NULL WHERE id=?")
            .bind(job)
            .execute(state.db.pool())
            .await
            .unwrap();
        if expected == "interrupted" {
            sqlx::query("DELETE FROM stream_events WHERE stream_id=? AND kind='done'")
                .bind(accepted["stream_id"].as_str())
                .execute(state.db.pool())
                .await
                .unwrap();
        }
        api::commands::recover(&state).await.unwrap();
        let (status, error, generation): (String, Option<String>, i64) =
            sqlx::query_as("SELECT status,error,generation FROM execution_jobs WHERE id=?")
                .bind(job)
                .fetch_one(state.db.pool())
                .await
                .unwrap();
        assert_eq!(status, expected);
        assert_eq!(generation, 1);
        assert_eq!(
            error.as_deref(),
            if expected == "interrupted" {
                Some("server_restart")
            } else {
                None
            }
        );
    }
    api::commands::recover(&state).await.unwrap();
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM stream_events WHERE kind='done'")
            .fetch_one(state.db.pool())
            .await
            .unwrap(),
        1
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM stream_events WHERE kind='error'")
            .fetch_one(state.db.pool())
            .await
            .unwrap(),
        1
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM messages")
            .fetch_one(state.db.pool())
            .await
            .unwrap(),
        1
    );
}
