use super::*;
use qunica_backend::{api::automations, runtime::recall};

fn tool_names(request: &Value) -> Vec<String> {
    request["tools"]
        .as_array()
        .map(|tools| {
            tools
                .iter()
                .filter_map(|tool| tool["function"]["name"].as_str())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

fn is_router_request(request: &Value) -> bool {
    tool_names(request)
        .iter()
        .any(|name| name == "DispatchTask")
}

/// The task id named by a task report in the router's history.
fn dispatched_task(messages: &[Value]) -> String {
    let history = Value::Array(messages.to_vec()).to_string();
    let start = history
        .find("task_id: ")
        .expect("a task report in the router's history")
        + "task_id: ".len();
    history[start..start + 36].to_string()
}

/// The task id in a user message shaped `SEND <task-id>`.
fn sent_task(messages: &[Value]) -> String {
    messages
        .iter()
        .rev()
        .find(|message| message["role"] == "user")
        .map(|message| wire_text(&message["content"]))
        .and_then(|text| {
            let start = text.find("SEND ")? + "SEND ".len();
            text.get(start..start + 36).map(str::to_string)
        })
        .unwrap_or_default()
}

/// A task thread seeded directly, as though the router had dispatched it to
/// `worker` from `origin` at `ts`. No job, so the runtime reads it as idle.
async fn seed_task_thread(
    state: &AppState,
    group: &str,
    worker: &str,
    origin: &str,
    title: &str,
    ts: &str,
) -> String {
    let id = uuid::Uuid::new_v4().to_string();
    sqlx::query(
        "INSERT INTO threads (id, group_id, agent_id, title, status, next_seq, \
         assigned_agent_id, origin_thread_id, origin_turn_id, created_at, updated_at) \
         VALUES (?, ?, NULL, ?, 'active', 1, ?, ?, NULL, ?, ?)",
    )
    .bind(&id)
    .bind(group)
    .bind(title)
    .bind(worker)
    .bind(origin)
    .bind(ts)
    .bind(ts)
    .execute(state.db.pool())
    .await
    .unwrap();
    id
}

/// A fake provider whose replies a handler scripted by request shape; the
/// router and worker turns run as independent jobs, so replies key off the
/// message state rather than arrival order.
async fn scripted_router_provider(
    handler: impl Fn(&Value, &[Value]) -> String + Clone + Send + Sync + 'static,
) -> (String, Arc<Mutex<Vec<Value>>>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let requests = Arc::new(Mutex::new(Vec::new()));
    let app = Router::new().fallback({
        let requests = Arc::clone(&requests);
        move |request: Request<Body>| {
            let requests = Arc::clone(&requests);
            let handler = handler.clone();
            async move {
                let bytes = axum::body::to_bytes(request.into_body(), usize::MAX)
                    .await
                    .unwrap();
                let request: Value = serde_json::from_slice(&bytes).unwrap();
                let messages = request["messages"].as_array().cloned().unwrap_or_default();
                let body = if is_router_request(&request) {
                    handler(&request, &messages)
                } else {
                    text_body("worker result 42")
                };
                requests.lock().await.push(request);
                ([(header::CONTENT_TYPE, "text/event-stream")], body).into_response()
            }
        }
    });
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    (format!("http://{addr}"), requests)
}

/// The JSON result of the router's tool call `call_id`.
fn tool_result(requests: &[Value], call_id: &str) -> Option<Value> {
    let messages: Vec<Value> = requests
        .iter()
        .flat_map(|request| request["messages"].as_array().cloned().unwrap_or_default())
        .collect();
    result_in(&messages, call_id)
}

/// The raw text of the router's tool call `call_id`, failures included.
fn tool_text(requests: &[Value], call_id: &str) -> Option<String> {
    requests
        .iter()
        .flat_map(|request| request["messages"].as_array().cloned().unwrap_or_default())
        .find(|message| message["role"] == "tool" && message["tool_call_id"] == call_id)
        .map(|message| wire_text(&message["content"]))
}

/// The text after `marker` in the latest user message, split into task ids.
fn ids_after(messages: &[Value], marker: &str) -> Vec<String> {
    let text = messages
        .iter()
        .rev()
        .find(|message| message["role"] == "user")
        .map(|message| wire_text(&message["content"]))
        .unwrap_or_default();
    let Some(start) = text.find(marker) else {
        return Vec::new();
    };
    text[start + marker.len()..]
        .split_whitespace()
        .filter_map(|word| word.get(..36).map(str::to_string))
        .collect()
}

fn last_user_text(messages: &[Value]) -> String {
    messages
        .iter()
        .rev()
        .find(|message| message["role"] == "user")
        .map(|message| wire_text(&message["content"]))
        .unwrap_or_default()
}

fn result_in(messages: &[Value], call_id: &str) -> Option<Value> {
    messages
        .iter()
        .find(|message| message["role"] == "tool" && message["tool_call_id"] == call_id)
        .and_then(|message| {
            // Tool output opens with a `status: ...` line.
            let content = wire_text(&message["content"]);
            serde_json::from_str(content.split_once('\n')?.1).ok()
        })
}

/// A watchdog check: review, then alert twice and stop the first flagged
/// task, then summarise.
fn watchdog_reply(messages: &[Value]) -> String {
    let results = messages
        .iter()
        .filter(|message| message["role"] == "tool")
        .count();
    match results {
        0 => tool_body(vec![("call-review", "ReviewTasks", json!({}))]),
        1 => {
            let review = result_in(messages, "call-review").expect("the review result");
            let task = review["tasks"][0]["task_id"].clone();
            tool_body(vec![
                (
                    "call-notify",
                    "NotifyUser",
                    json!({"task_id": task, "message": "It failed."}),
                ),
                (
                    "call-notify-again",
                    "NotifyUser",
                    json!({"task_id": task, "message": "Still failed."}),
                ),
                (
                    "call-stop",
                    "StopTask",
                    json!({"task_id": task, "reason": "It keeps failing."}),
                ),
            ])
        }
        _ => text_body("Checked 1 task."),
    }
}

/// Answers by request shape rather than arrival order, because router and
/// worker turns run as independent jobs.
async fn router_fake_provider(first_router_reply: String) -> (String, Arc<Mutex<Vec<Value>>>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let requests = Arc::new(Mutex::new(Vec::new()));
    let app = Router::new().fallback({
        let requests = Arc::clone(&requests);
        move |request: Request<Body>| {
            let requests = Arc::clone(&requests);
            let first_router_reply = first_router_reply.clone();
            async move {
                let bytes = axum::body::to_bytes(request.into_body(), usize::MAX)
                    .await
                    .unwrap();
                let request: Value = serde_json::from_slice(&bytes).unwrap();
                let messages = request["messages"].as_array().cloned().unwrap_or_default();
                let last_is_tool = messages
                    .last()
                    .is_some_and(|message| message["role"] == "tool");
                let last_user = messages
                    .iter()
                    .rev()
                    .find(|message| message["role"] == "user")
                    .map(|message| message["content"].to_string())
                    .unwrap_or_default();
                let body = if tool_names(&request)
                    .iter()
                    .any(|name| name == "ReviewTasks")
                {
                    watchdog_reply(&messages)
                } else if !is_router_request(&request) {
                    text_body("worker result 42")
                } else if last_is_tool {
                    text_body("Started the task.")
                } else if last_user.contains("Task report") {
                    text_body("Summary: the worker finished.")
                } else if last_user.contains("FOLLOW") {
                    let task = dispatched_task(&messages);
                    tool_body(vec![(
                        "call-send",
                        "SendToTask",
                        json!({"task_id": task, "message": "Add tests"}),
                    )])
                } else if last_user.contains("STOP") {
                    let task = dispatched_task(&messages);
                    tool_body(vec![(
                        "call-cancel",
                        "CancelTask",
                        json!({"task_id": task}),
                    )])
                } else {
                    first_router_reply
                };
                requests.lock().await.push(request);
                ([(header::CONTENT_TYPE, "text/event-stream")], body).into_response()
            }
        }
    });
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    (format!("http://{addr}"), requests)
}

struct RouterGroup {
    app: Router,
    state: AppState,
    token: String,
    _root: tempfile::TempDir,
    group: String,
    router: String,
    worker: String,
    requests: Arc<Mutex<Vec<Value>>>,
}

async fn router_group(name: &str, first_router_reply: String, extra: Value) -> RouterGroup {
    let (url, requests) = router_fake_provider(first_router_reply).await;
    router_group_with(name, url, requests, extra).await
}

async fn router_group_with(
    name: &str,
    url: String,
    requests: Arc<Mutex<Vec<Value>>>,
    extra: Value,
) -> RouterGroup {
    let (app, state) = router_with_state_for_tests().await;
    let email = format!("router-{name}@example.test");
    let token = register_and_login(&app, &email).await;
    let owner = owner_id(&state, &email).await;
    let (root, workspace) = create_local_workspace(&app, &token).await;
    let group = create_group(&app, &token, &workspace, json!({})).await;
    let provider = seed_provider(&state, &owner, &url).await;
    let router = seed_agent(
        &state,
        &owner,
        &group,
        &provider,
        "Lead",
        "2024-01-01T00:00:00Z",
    )
    .await;
    let worker = seed_agent(
        &state,
        &owner,
        &group,
        &provider,
        "Coder",
        "2024-01-01T00:00:01Z",
    )
    .await;
    let mut body = json!({"scheduler_mode": "router", "router_agent_id": router});
    if let (Some(body), Some(extra)) = (body.as_object_mut(), extra.as_object()) {
        body.extend(extra.clone());
    }
    let (status, updated) = send(
        &app,
        authed_json("PATCH", &format!("/api/v2/groups/{group}"), &token, body),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{updated}");
    RouterGroup {
        app,
        state,
        token,
        _root: root,
        group,
        router,
        worker,
        requests,
    }
}

impl RouterGroup {
    /// Router-written task instructions in `thread`.
    async fn instructions_in(&self, thread: &str) -> i64 {
        sqlx::query_scalar(
            "SELECT COUNT(*) FROM messages WHERE thread_id=? AND \
             COALESCE(json_extract(CASE WHEN json_valid(content_json) THEN content_json ELSE '{}' END,'$.router_dispatch'),0)=1",
        )
        .bind(thread)
        .fetch_one(self.state.db.pool())
        .await
        .unwrap()
    }

    async fn topics_of(&self, thread: &str) -> Vec<String> {
        sqlx::query_scalar("SELECT topic FROM thread_topics WHERE thread_id=? ORDER BY topic")
            .bind(thread)
            .fetch_all(self.state.db.pool())
            .await
            .unwrap()
    }

    /// The single task this conversation dispatched.
    async fn only_task(&self, main: &str) -> String {
        let tasks: Vec<String> =
            sqlx::query_scalar("SELECT id FROM threads WHERE origin_thread_id=?")
                .bind(main)
                .fetch_all(self.state.db.pool())
                .await
                .unwrap();
        assert_eq!(tasks.len(), 1, "one task thread: {tasks:?}");
        tasks[0].clone()
    }

    async fn wait_for_tool_text(&self, call_id: &str) -> String {
        tokio::time::timeout(Duration::from_secs(15), async {
            loop {
                if let Some(output) = tool_text(&self.requests.lock().await, call_id) {
                    break output;
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .unwrap_or_else(|_| panic!("the router sees the result of {call_id}"))
    }

    /// The `task_reports` entries posted to `thread`, oldest first.
    async fn reports(&self, thread: &str) -> Vec<Value> {
        self.messages(thread)
            .await
            .into_iter()
            .filter(|m| m.2 == "task_report")
            .flat_map(|m| {
                let value: Value = serde_json::from_str(m.4.as_deref().unwrap()).unwrap();
                value["task_reports"]
                    .as_array()
                    .cloned()
                    .unwrap_or_default()
            })
            .collect()
    }

    async fn say(&self, thread: &str, content: &str) {
        let (status, receipt) = send(
            &self.app,
            authed_json(
                "POST",
                "/api/v2/commands",
                &self.token,
                json!({"request_id": uuid::Uuid::new_v4().to_string(), "name": "message.send",
                    "target": {"conversation_id": self.group, "thread_id": thread},
                    "payload": {"content": content}}),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::ACCEPTED, "{receipt}");
    }

    /// Wait until the router has replied to a task report in `thread`.
    async fn wait_for_summary(&self, thread: &str) {
        self.wait_for_summaries(thread, 1).await;
    }

    async fn wait_for_summaries(&self, thread: &str, count: i64) {
        tokio::time::timeout(Duration::from_secs(15), async {
            loop {
                let done: i64 = sqlx::query_scalar(
                    "SELECT COUNT(*) FROM messages WHERE thread_id=? AND sender_id=? \
                     AND content LIKE 'Summary:%'",
                )
                .bind(thread)
                .bind(&self.router)
                .fetch_one(self.state.db.pool())
                .await
                .unwrap();
                if done >= count {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .expect("the router summarises the task report");
    }

    async fn messages(
        &self,
        thread: &str,
    ) -> Vec<(String, Option<String>, String, String, Option<String>)> {
        sqlx::query_as(
            "SELECT sender_type,sender_id,message_type,COALESCE(content,''),content_json \
             FROM messages WHERE thread_id=? ORDER BY seq",
        )
        .bind(thread)
        .fetch_all(self.state.db.pool())
        .await
        .unwrap()
    }
}

#[tokio::test]
async fn router_mode_is_stored_as_bounded_and_guards_its_router() {
    let scenario = router_group(
        "config",
        text_body("unused"),
        json!({"max_router_tasks": 3}),
    )
    .await;
    let RouterGroup {
        app,
        state,
        token,
        group,
        router,
        worker,
        ..
    } = &scenario;
    let uri = format!("/api/v2/groups/{group}");

    let (status, body) = send(app, authed_empty("GET", &uri, token)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["scheduler_mode"], "router");
    assert_eq!(body["router_agent_id"], json!(router));
    assert_eq!(body["max_router_tasks"], 3);
    let stored: (String, Option<String>) =
        sqlx::query_as("SELECT scheduler_mode,router_agent_id FROM groups WHERE id=?")
            .bind(group)
            .fetch_one(state.db.pool())
            .await
            .unwrap();
    assert_eq!(stored, ("bounded".to_string(), Some(router.clone())));

    let (status, _) = send(
        app,
        authed_empty("DELETE", &format!("{uri}/agents/{router}"), token),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CONFLICT,
        "the router cannot leave the group"
    );

    for body in [
        json!({"router_agent_id": uuid::Uuid::new_v4().to_string()}),
        json!({"max_router_tasks": 17}),
        json!({"scheduler_mode": "router", "router_agent_id": null}),
    ] {
        let (status, error) = send(app, authed_json("PATCH", &uri, token, body.clone())).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{body} -> {error}");
    }

    let (status, body) = send(
        app,
        authed_json("PATCH", &uri, token, json!({"router_agent_id": worker})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["router_agent_id"], json!(worker));

    let (status, body) = send(
        app,
        authed_json("PATCH", &uri, token, json!({"scheduler_mode": "bounded"})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["scheduler_mode"], "bounded");
    assert_eq!(body["router_agent_id"], Value::Null);
}

#[tokio::test]
async fn router_dispatches_a_task_and_summarises_its_report() {
    let dispatch = tool_body(vec![(
        "call-dispatch",
        "DispatchTask",
        json!({"assistant": "Coder", "title": "Build it", "instructions": "Build the widget"}),
    )]);
    let scenario = router_group("flow", dispatch, json!({})).await;
    let main = seed_thread(&scenario.state, &scenario.group, "active").await;

    // A mention does not bypass the router in its own conversation.
    scenario.say(&main, "@Coder please build the widget").await;
    scenario.wait_for_summary(&main).await;

    type Task = (String, Option<String>, Option<String>, Option<String>);
    let tasks: Vec<Task> = sqlx::query_as(
        "SELECT id,title,assigned_agent_id,origin_thread_id FROM threads \
         WHERE group_id=? AND origin_thread_id IS NOT NULL",
    )
    .bind(&scenario.group)
    .fetch_all(scenario.state.db.pool())
    .await
    .unwrap();
    assert_eq!(tasks.len(), 1);
    let (task, title, assigned, origin) = tasks[0].clone();
    assert_eq!(title.as_deref(), Some("Build it"));
    assert_eq!(assigned.as_deref(), Some(scenario.worker.as_str()));
    assert_eq!(origin.as_deref(), Some(main.as_str()));

    let task_messages = scenario.messages(&task).await;
    assert_eq!(task_messages[0].0, "agent");
    assert_eq!(
        task_messages[0].1.as_deref(),
        Some(scenario.router.as_str())
    );
    assert_eq!(task_messages[0].3, "Build the widget");
    assert!(task_messages
        .iter()
        .any(|m| m.1.as_deref() == Some(scenario.worker.as_str()) && m.3 == "worker result 42"));

    let main_messages = scenario.messages(&main).await;
    assert!(
        main_messages
            .iter()
            .all(|m| m.1.as_deref() != Some(scenario.worker.as_str())),
        "only the router speaks in the main thread: {main_messages:?}"
    );
    let report = main_messages
        .iter()
        .find(|m| m.2 == "task_report")
        .expect("a task report is posted to the main thread");
    assert_eq!(report.0, "system");
    let reports: Value = serde_json::from_str(report.4.as_deref().unwrap()).unwrap();
    assert_eq!(reports["task_reports"][0]["task_id"], json!(task));
    assert_eq!(reports["task_reports"][0]["status"], "completed");
    assert_eq!(reports["task_reports"][0]["excerpt"], "worker result 42");
    assert!(main_messages
        .iter()
        .any(|m| m.1.as_deref() == Some(scenario.router.as_str()) && m.3 == "Started the task."));

    let requests = scenario.requests.lock().await;
    let router_requests: Vec<&Value> = requests.iter().filter(|r| is_router_request(r)).collect();
    let worker_requests: Vec<&Value> = requests.iter().filter(|r| !is_router_request(r)).collect();
    assert_eq!(router_requests.len(), 3, "dispatch, acknowledge, summarise");
    assert_eq!(worker_requests.len(), 1);
    let router_tools = tool_names(router_requests[0]);
    for name in [
        "DispatchTask",
        "ListTasks",
        "SendToTask",
        "CancelTask",
        "ReadThread",
        "ReadTask",
        "ReadEvent",
    ] {
        assert!(
            router_tools.iter().any(|tool| tool == name),
            "{router_tools:?}"
        );
    }
    assert!(!router_tools.iter().any(|tool| tool == "AgentAsTool"));
    let router_request = router_requests[0].to_string();
    assert!(router_request.contains("Router role:"));
    // The router must not be told it cannot delegate next to DispatchTask.
    for stale in [
        "no delegation tool",
        "never in the background",
        "Only the group scheduler or the AgentAsTool tool",
    ] {
        assert!(
            !router_request.contains(stale),
            "router prompt has {stale:?}"
        );
    }
    assert!(router_request.contains("DispatchTask is your delegation tool"));
    let worker_request = worker_requests[0].to_string();
    assert!(worker_request.contains("Dispatched task:"));
    assert!(!worker_request.contains("never in the background"));
    assert!(worker_request.contains("Build the widget"));
    assert!(
        !worker_request.contains("please build the widget"),
        "the worker sees only its instructions"
    );
    let summary_request = router_requests[2].to_string();
    // The transcript escapes the report, delimiter included.
    assert!(
        summary_request.contains("&lt;task_result untrusted=&quot;true&quot;&gt;"),
        "{summary_request}"
    );
}

#[tokio::test]
async fn router_dispatch_respects_the_group_task_limit() {
    let dispatch = tool_body(vec![
        (
            "call-one",
            "DispatchTask",
            json!({"assistant": "Coder", "instructions": "First job"}),
        ),
        (
            "call-two",
            "DispatchTask",
            json!({"assistant": "Coder", "instructions": "Second job"}),
        ),
    ]);
    let scenario = router_group("limit", dispatch, json!({"max_router_tasks": 1})).await;
    let main = seed_thread(&scenario.state, &scenario.group, "active").await;

    scenario.say(&main, "do two things").await;
    scenario.wait_for_summary(&main).await;

    let tasks: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM threads WHERE group_id=? AND origin_thread_id IS NOT NULL",
    )
    .bind(&scenario.group)
    .fetch_one(scenario.state.db.pool())
    .await
    .unwrap();
    assert_eq!(tasks, 1);
    let requests = scenario.requests.lock().await;
    let acknowledge = requests
        .iter()
        .find(|r| {
            is_router_request(r)
                && r["messages"].as_array().unwrap().last().unwrap()["role"] == "tool"
        })
        .expect("the router sees both tool results")
        .to_string();
    assert!(
        acknowledge.contains("the limit for this group"),
        "{acknowledge}"
    );
}

#[tokio::test]
async fn a_router_mention_does_not_bring_a_member_into_the_conversation() {
    // A new group is mesh with hops and no moderator: the setup where a text
    // @mention would otherwise schedule the named member next.
    let scenario = router_group(
        "mention",
        text_body("I will ask @Coder about it."),
        json!({}),
    )
    .await;
    let main = seed_thread(&scenario.state, &scenario.group, "active").await;

    scenario.say(&main, "hello").await;
    tokio::time::timeout(Duration::from_secs(15), async {
        loop {
            let open: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM execution_jobs WHERE thread_id=? \
                 AND status IN ('queued','running')",
            )
            .bind(&main)
            .fetch_one(scenario.state.db.pool())
            .await
            .unwrap();
            let replied = scenario
                .messages(&main)
                .await
                .iter()
                .any(|message| message.1.as_deref() == Some(scenario.router.as_str()));
            if open == 0 && replied {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("the router turn finishes");

    let messages = scenario.messages(&main).await;
    assert!(
        messages
            .iter()
            .all(|message| message.1.as_deref() != Some(scenario.worker.as_str())),
        "{messages:?}"
    );
    let requests = scenario.requests.lock().await;
    assert!(requests.iter().all(is_router_request));
    let system = requests[0]["messages"][0]["content"].to_string();
    assert!(system.contains("display-only"), "{system}");
}

#[tokio::test]
async fn router_follows_up_on_and_cancels_its_task() {
    let dispatch = tool_body(vec![(
        "call-dispatch",
        "DispatchTask",
        json!({"assistant": "Coder", "instructions": "Build the widget"}),
    )]);
    let scenario = router_group("follow-up", dispatch, json!({})).await;
    let main = seed_thread(&scenario.state, &scenario.group, "active").await;
    scenario.say(&main, "build the widget").await;
    scenario.wait_for_summary(&main).await;

    scenario.say(&main, "FOLLOW up with tests").await;
    scenario.wait_for_summaries(&main, 2).await;
    let task: String = sqlx::query_scalar("SELECT id FROM threads WHERE origin_thread_id=?")
        .bind(&main)
        .fetch_one(scenario.state.db.pool())
        .await
        .unwrap();
    let from_router: Vec<String> = scenario
        .messages(&task)
        .await
        .into_iter()
        .filter(|m| m.1.as_deref() == Some(scenario.router.as_str()))
        .map(|m| m.3)
        .collect();
    assert_eq!(from_router, ["Build the widget", "Add tests"]);
    let reports = scenario
        .messages(&main)
        .await
        .into_iter()
        .filter(|m| m.2 == "task_report")
        .count();
    assert_eq!(reports, 2, "each finished run of the task is reported");

    scenario.say(&main, "STOP the task").await;
    let cancelled = tokio::time::timeout(Duration::from_secs(15), async {
        loop {
            if let Some(result) = tool_result(&scenario.requests.lock().await, "call-cancel") {
                break result;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("the router sees the CancelTask result");
    assert_eq!(cancelled["task_id"], json!(task));
    assert_eq!(cancelled["status"], "idle", "nothing was running");
}

#[tokio::test]
async fn send_to_task_takes_a_slot_for_idle_tasks_but_queues_on_running_ones() {
    let dispatch_two = tool_body(vec![
        (
            "call-one",
            "DispatchTask",
            json!({"assistant": "Coder", "instructions": "First job"}),
        ),
        (
            "call-two",
            "DispatchTask",
            json!({"assistant": "Coder", "instructions": "Second job"}),
        ),
    ]);
    let handler = {
        let dispatch_two = dispatch_two;
        move |_request: &Value, messages: &[Value]| {
            let last_user = messages
                .iter()
                .rev()
                .find(|message| message["role"] == "user")
                .map(|message| wire_text(&message["content"]))
                .unwrap_or_default();
            let last_is_tool = messages
                .last()
                .is_some_and(|message| message["role"] == "tool");
            if last_user.contains("Task report") {
                text_body("Summary: the worker finished.")
            } else if last_is_tool {
                text_body("Started the task.")
            } else if last_user.contains("SEND") {
                let task = sent_task(messages);
                let call_id = format!("call-send-{}", &task[..8]);
                tool_body(vec![(
                    &call_id,
                    "SendToTask",
                    json!({ "task_id": task, "message": "Add tests" }),
                )])
            } else {
                dispatch_two.clone()
            }
        }
    };
    let (url, requests) = scripted_router_provider(handler).await;
    let scenario =
        router_group_with("send-slot", url, requests, json!({ "max_router_tasks": 2 })).await;
    let main = seed_thread(&scenario.state, &scenario.group, "active").await;
    scenario.say(&main, "do two things").await;
    scenario.wait_for_summary(&main).await;
    // Both workers finished, so both tasks are idle before we freeze one.
    tokio::time::timeout(Duration::from_secs(15), async {
        loop {
            let open: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM execution_jobs WHERE thread_id IN \
                 (SELECT id FROM threads WHERE group_id=? AND origin_thread_id=?) \
                 AND status IN ('queued','running','waiting_for_user')",
            )
            .bind(&scenario.group)
            .bind(&main)
            .fetch_one(scenario.state.db.pool())
            .await
            .unwrap();
            if open == 0 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("both dispatched tasks finish");
    let tasks: Vec<String> = sqlx::query_scalar(
        "SELECT id FROM threads WHERE origin_thread_id=? ORDER BY created_at,id",
    )
    .bind(&main)
    .fetch_all(scenario.state.db.pool())
    .await
    .unwrap();
    assert_eq!(tasks.len(), 2);
    let (busy, idle) = (tasks[0].clone(), tasks[1].clone());
    // Keep the first task counted as running while the group's single slot is
    // tightened, the same way the watchdog test fakes a failed run.
    sqlx::query("UPDATE execution_jobs SET status='running' WHERE thread_id=?")
        .bind(&busy)
        .execute(scenario.state.db.pool())
        .await
        .unwrap();
    let (status, _) = send(
        &scenario.app,
        authed_json(
            "PATCH",
            &format!("/api/v2/groups/{}", scenario.group),
            &scenario.token,
            json!({ "max_router_tasks": 1 }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // A follow-up to the running task queues behind its current run.
    scenario.say(&main, &format!("SEND {busy}")).await;
    let queued = tokio::time::timeout(Duration::from_secs(15), async {
        loop {
            if let Some(result) = tool_result(
                &scenario.requests.lock().await,
                &format!("call-send-{}", &busy[..8]),
            ) {
                break result;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("the router sees the SendToTask result");
    assert_eq!(queued["status"], "accepted");
    assert!(
        scenario
            .messages(&busy)
            .await
            .iter()
            .any(|m| m.3 == "Add tests"),
        "the follow-up lands in the running task"
    );

    // A follow-up that would restart the idle task needs a slot, and the one
    // slot this group has is taken by the running task.
    scenario.say(&main, &format!("SEND {idle}")).await;
    tokio::time::timeout(Duration::from_secs(15), async {
        loop {
            let requests = scenario.requests.lock().await;
            if requests.iter().any(|r| {
                is_router_request(r)
                    && r.to_string().contains(&format!("SEND {idle}"))
                    && r.to_string()
                        .contains("the limit for this group; wait for one to report")
            }) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("the router sees the SendToTask failure");
    assert!(
        !scenario
            .messages(&idle)
            .await
            .iter()
            .any(|m| m.3 == "Add tests"),
        "the denied follow-up writes nothing into the idle task"
    );
}

#[tokio::test]
async fn the_per_request_limit_counts_follow_ups_not_new_threads() {
    follow_ups_stop_at_the_per_request_cap("per-request", "SendToTask").await;
}

#[tokio::test]
async fn the_per_request_limit_counts_continued_dispatches() {
    follow_ups_stop_at_the_per_request_cap("per-request-continue", "DispatchTask").await;
}

/// The router keeps adding work to one task with `tool` within a single user
/// request; the 17th instruction is refused and no second thread opens.
async fn follow_ups_stop_at_the_per_request_cap(name: &str, tool: &'static str) {
    let handler = move |_request: &Value, messages: &[Value]| {
        let last_user = messages
            .iter()
            .rev()
            .find(|message| message["role"] == "user")
            .map(|message| wire_text(&message["content"]))
            .unwrap_or_default();
        let tool_results = messages
            .iter()
            .filter(|message| message["role"] == "tool")
            .count();
        if last_user.contains("Task report") {
            text_body("Summary: the worker finished.")
        } else if tool_results > 16 {
            // The 17th follow-up was the one refused.
            text_body("Stopped at the limit.")
        } else {
            let task = sent_task(messages);
            let call_id = format!("call-send-{tool_results:02}");
            let round = format!("Round {}", tool_results + 1);
            let args = if tool == "SendToTask" {
                json!({ "task_id": task, "message": round })
            } else {
                json!({ "task_id": task, "instructions": round })
            };
            tool_body(vec![(&call_id, tool, args)])
        }
    };
    let (url, requests) = scripted_router_provider(handler).await;
    let scenario = router_group_with(name, url, requests, json!({})).await;
    let main = seed_thread(&scenario.state, &scenario.group, "active").await;
    let task = seed_task_thread(
        &scenario.state,
        &scenario.group,
        &scenario.worker,
        &main,
        "Follow up",
        "2024-01-01T00:00:00Z",
    )
    .await;

    scenario.say(&main, &format!("SEND {task}")).await;
    let refused = tokio::time::timeout(Duration::from_secs(30), async {
        loop {
            if let Some(output) = tool_text(&scenario.requests.lock().await, "call-send-16") {
                break output;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("the router runs into the per-request cap");
    assert!(
        refused.contains(
            "16 task instructions were already sent for the user's latest message; report what you have instead"
        ),
        "{refused}"
    );
    let earlier = tool_text(&scenario.requests.lock().await, "call-send-15").unwrap();
    assert!(!earlier.contains("already sent"), "{earlier}");
    assert_eq!(scenario.instructions_in(&task).await, 16);
    // The loop never opened a second thread.
    let threads: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM threads WHERE group_id=? AND origin_thread_id IS NOT NULL",
    )
    .bind(&scenario.group)
    .fetch_one(scenario.state.db.pool())
    .await
    .unwrap();
    assert_eq!(threads, 1);
}

#[tokio::test]
async fn the_router_prompt_lists_this_conversations_tasks_newest_first() {
    let scenario = router_group("tasks-view", text_body("Done."), json!({})).await;
    let main = seed_thread(&scenario.state, &scenario.group, "active").await;
    let other = seed_thread(&scenario.state, &scenario.group, "active").await;

    let mut seeded = Vec::new();
    for n in 1..=10 {
        let ts = format!("2024-01-01T00:{n:02}:00Z");
        let title = if n == 5 {
            "Title <x> & \"quoted\"".to_string()
        } else {
            format!("Seed task {n}")
        };
        let id = seed_task_thread(
            &scenario.state,
            &scenario.group,
            &scenario.worker,
            &main,
            &title,
            &ts,
        )
        .await;
        seeded.push((id, title, ts));
    }
    // A newer task of another conversation must never leak into this list.
    let foreign = seed_task_thread(
        &scenario.state,
        &scenario.group,
        &scenario.worker,
        &other,
        "Foreign task",
        "2024-01-02T00:00:00Z",
    )
    .await;

    scenario.say(&main, "hello").await;
    // Wait on the provider request itself: the router's reply row can exist
    // before the request is recorded.
    let prompt = tokio::time::timeout(Duration::from_secs(15), async {
        loop {
            if let Some(request) = scenario
                .requests
                .lock()
                .await
                .iter()
                .find(|r| is_router_request(r) && r.to_string().contains("hello"))
            {
                break request.to_string();
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("the router's request for the hello turn");
    assert!(
        prompt.contains("Tasks in this conversation (newest first):"),
        "{prompt}"
    );
    // The eight newest tasks of this conversation are listed newest first.
    for n in 3..=10 {
        let (id, title, ts) = &seeded[n - 1];
        let title = title
            .replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
            .replace('"', "&quot;");
        assert!(
            prompt.contains(&format!(
                "- {id} · {title} · Coder · idle · run 0 · updated {ts}"
            )),
            "newest tasks are listed: {title}, {prompt}"
        );
    }
    // The two oldest fall off the eight-row budget and the cut is marked.
    for (id, title, _) in &seeded[..2] {
        assert!(
            !prompt.contains(&format!("- {id} · ")),
            "oldest tasks drop off: {title}, {prompt}"
        );
    }
    assert!(
        prompt.contains("(More tasks exist; use ListTasks to see them all.)"),
        "{prompt}"
    );
    assert!(
        !prompt.contains(&format!("- {foreign} ·")),
        "tasks of another conversation are not listed: {prompt}"
    );
    assert!(!prompt.contains("Foreign task"), "{prompt}");
    // The router's own markup characters are escaped like transcript text.
    assert!(!prompt.contains("Title <x>"), "{prompt}");
    assert!(
        prompt.contains("Title &lt;x&gt; &amp; &quot;quoted&quot;"),
        "{prompt}"
    );
}

impl RouterGroup {
    /// A signed generic webhook automation in this group: (hook id, secret).
    async fn webhook_automation(&self) -> (String, String) {
        let (status, automation) = send(
            &self.app,
            authed_json(
                "POST",
                "/api/v2/automations",
                &self.token,
                json!({"group_id": self.group, "name": "Tickets", "prompt": "Handle the ticket event",
                    "trigger": {"kind": "webhook"}, "timezone": "UTC"}),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED, "{automation}");
        let id = automation["id"].as_str().unwrap();
        let (status, rotated) = send(
            &self.app,
            authed_json(
                "POST",
                &format!("/api/v2/automations/{id}/secret"),
                &self.token,
                json!({"action": "rotate", "expected_version": 1}),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{rotated}");
        (
            automation["hook_id"].as_str().unwrap().to_string(),
            rotated["secret"].as_str().unwrap().to_string(),
        )
    }

    async fn post_event(&self, hook: &(String, String), event: &str, topic: &str) -> Value {
        use hmac::{Hmac, Mac};
        let body =
            json!({"event_id": event, "topic": topic, "payload": {"ticket": 9, "note": event}})
                .to_string();
        let timestamp = chrono::Utc::now().timestamp().to_string();
        let mut mac = Hmac::<sha2::Sha256>::new_from_slice(hook.1.as_bytes()).unwrap();
        mac.update(timestamp.as_bytes());
        mac.update(b".");
        mac.update(body.as_bytes());
        let request = Request::builder()
            .method("POST")
            .uri(format!("/api/v2/automation-hooks/{}", hook.0))
            .header("content-type", "application/json")
            .header("x-qunica-timestamp", timestamp)
            .header(
                "x-qunica-signature",
                format!("sha256={:x}", mac.finalize().into_bytes()),
            )
            .body(Body::from(body))
            .unwrap();
        let (status, response) = send(&self.app, request).await;
        assert_eq!(status, StatusCode::ACCEPTED, "{response}");
        response
    }
}

#[tokio::test]
async fn a_dispatched_task_follows_its_topic_and_receives_later_events() {
    let dispatch = tool_body(vec![(
        "call-dispatch",
        "DispatchTask",
        json!({"assistant": "Coder", "title": "Ticket 9", "instructions": "Resolve ticket 9"}),
    )]);
    let scenario = router_group("topic", dispatch, json!({})).await;
    let hook = scenario.webhook_automation().await;

    let first = scenario.post_event(&hook, "ev-1", "ticket-9").await;
    assert_eq!(first["route"], "home");
    let home = first["thread_id"].as_str().unwrap().to_string();
    scenario.wait_for_summary(&home).await;

    let task: String = sqlx::query_scalar("SELECT id FROM threads WHERE origin_thread_id=?")
        .bind(&home)
        .fetch_one(scenario.state.db.pool())
        .await
        .unwrap();
    let followed: Vec<String> =
        sqlx::query_scalar("SELECT topic FROM thread_topics WHERE thread_id=?")
            .bind(&task)
            .fetch_all(scenario.state.db.pool())
            .await
            .unwrap();
    assert_eq!(
        followed,
        ["ticket-9"],
        "the task inherits its origin's topic"
    );

    let second = scenario.post_event(&hook, "ev-2", "ticket-9").await;
    assert_eq!(second["route"], "subscribers");
    assert_eq!(second["thread_id"], json!(task));
    scenario.wait_for_summaries(&home, 2).await;

    let home_events = scenario
        .messages(&home)
        .await
        .into_iter()
        .filter(|m| m.0 == "user")
        .count();
    assert_eq!(home_events, 1, "the second event bypasses the router");
    let task_messages = scenario.messages(&task).await;
    assert!(
        task_messages.iter().any(|m| m.0 == "user"
            && m.3.starts_with("New event on ticket-9")
            && m.3.contains("ev-2")),
        "{task_messages:?}"
    );
    let worker_runs = task_messages
        .iter()
        .filter(|m| m.1.as_deref() == Some(scenario.worker.as_str()))
        .count();
    assert_eq!(worker_runs, 2);
    let reports = scenario
        .messages(&home)
        .await
        .into_iter()
        .filter(|m| m.2 == "task_report")
        .count();
    assert_eq!(
        reports, 2,
        "each run of the subscribed task reports to the router"
    );
    {
        let requests = scenario.requests.lock().await;
        let first_router = requests
            .iter()
            .find(|r| is_router_request(r))
            .unwrap()
            .to_string();
        assert!(first_router.contains("topic ticket-9"), "{first_router}");
    }

    // Recall walks conversation -> task -> event, each naming the next id.
    let pool = scenario.state.db.pool();
    let group = scenario.group.as_str();
    let home_view = recall::read_thread(pool, group, &json!({"thread_id": home}))
        .await
        .unwrap();
    assert_eq!(home_view["kind"], "conversation");
    assert_eq!(home_view["tasks"][0]["task_id"], json!(task));
    assert!(
        home_view["messages"]
            .as_array()
            .unwrap()
            .iter()
            .any(|m| m["task_ids"] == json!([task])),
        "task reports name their task: {home_view}"
    );
    let first_event = home_view["messages"]
        .as_array()
        .unwrap()
        .iter()
        .find_map(|m| m["event_ids"][0].as_str())
        .expect("the event message names its event")
        .to_string();
    assert_eq!(home_view["events"][0]["event_id"], json!(first_event));
    let task_view = recall::read_task(pool, group, &json!({"task_id": task}))
        .await
        .unwrap();
    assert_eq!(task_view["kind"], "task");
    assert_eq!(task_view["origin"]["thread_id"], json!(home));
    assert_eq!(task_view["assistant"], "Coder");
    assert_eq!(task_view["latest_reply"], "worker result 42");
    assert_eq!(task_view["topics"], json!(["ticket-9"]));
    let later_event = task_view["events"][0]["event_id"].as_str().unwrap();
    let event = recall::read_event(pool, group, &json!({"event_id": later_event}))
        .await
        .unwrap();
    assert_eq!(event["event_key"], "ev-2");
    assert_eq!(event["topic"], "ticket-9");
    assert_eq!(event["route"], "subscribers");
    assert_eq!(event["delivered_to"][0]["thread_id"], json!(task));
    assert_eq!(event["delivered_to"][0]["kind"], "task");
    assert_eq!(event["payload"]["untrusted"], true);
    assert!(
        event["payload"]["data"]
            .as_str()
            .unwrap()
            .contains("\"note\":\"ev-2\""),
        "{event}"
    );
    let first = recall::read_event(pool, group, &json!({"event_id": first_event}))
        .await
        .unwrap();
    assert_eq!(first["delivered_to"][0]["thread_id"], json!(home));
    assert!(first["payload"]["data"].as_str().unwrap().contains("ev-1"));
    // Reads are scoped to the caller's group, and a conversation is not a task.
    let other = uuid::Uuid::new_v4().to_string();
    assert!(
        recall::read_thread(pool, &other, &json!({"thread_id": home}))
            .await
            .is_err()
    );
    assert!(
        recall::read_event(pool, &other, &json!({"event_id": later_event}))
            .await
            .is_err()
    );
    assert!(recall::read_task(pool, group, &json!({"task_id": home}))
        .await
        .is_err());

    // Cancelling the task hands the topic back to the router.
    scenario.say(&home, "STOP the task").await;
    let cancelled = tokio::time::timeout(Duration::from_secs(15), async {
        loop {
            if let Some(result) = tool_result(&scenario.requests.lock().await, "call-cancel") {
                break result;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("the router sees the CancelTask result");
    assert_eq!(cancelled["unsubscribed"], json!(["ticket-9"]));
    let third = scenario.post_event(&hook, "ev-3", "ticket-9").await;
    assert_eq!(third["route"], "home");
    assert_eq!(third["thread_id"], json!(home));
}

#[tokio::test]
async fn a_watchdog_check_reviews_tasks_alerts_the_user_and_stops_one_read_only() {
    let dispatch = tool_body(vec![(
        "call-dispatch",
        "DispatchTask",
        json!({"assistant": "Coder", "title": "Build it", "instructions": "Build the widget"}),
    )]);
    let scenario = router_group("watchdog", dispatch, json!({})).await;
    let main = seed_thread(&scenario.state, &scenario.group, "active").await;
    scenario.say(&main, "build the widget").await;
    scenario.wait_for_summary(&main).await;
    let pool = scenario.state.db.pool();
    let task: String = sqlx::query_scalar("SELECT id FROM threads WHERE origin_thread_id=?")
        .bind(&main)
        .fetch_one(pool)
        .await
        .unwrap();
    // The task's run failed just now.
    sqlx::query("UPDATE execution_jobs SET status='failed',updated_at=? WHERE thread_id=?")
        .bind(chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true))
        .bind(&task)
        .execute(pool)
        .await
        .unwrap();

    let create = |trigger: Value| {
        authed_json(
            "POST",
            "/api/v2/automations",
            &scenario.token,
            json!({"group_id": scenario.group, "name": "Watchdog", "kind": "watchdog",
                "prompt": "Check the tasks.", "trigger": trigger, "timezone": "UTC"}),
        )
    };
    let (status, error) = send(&scenario.app, create(json!({"kind": "webhook"}))).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{error}");
    let (status, automation) = send(
        &scenario.app,
        create(json!({"kind": "cron", "expression": "*/10 * * * *"})),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{automation}");
    assert_eq!(automation["kind"], "watchdog");
    let automation = automation["id"].as_str().unwrap().to_string();

    // Due on the minute boundary of a schedule that runs every minute.
    let due = chrono::Utc::now().timestamp() / 60 * 60;
    let now = chrono::DateTime::from_timestamp(due + 3, 0).unwrap();
    sqlx::query("UPDATE automations SET next_run_at=?,trigger_json=? WHERE id=?")
        .bind(due)
        .bind(json!({"kind": "cron", "expression": "* * * * *"}).to_string())
        .bind(&automation)
        .execute(pool)
        .await
        .unwrap();
    assert_eq!(
        automations::tick(&scenario.state, now, false)
            .await
            .unwrap(),
        1
    );
    let check: String = sqlx::query_scalar("SELECT id FROM threads WHERE watchdog_automation_id=?")
        .bind(&automation)
        .fetch_one(pool)
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(15), async {
        loop {
            let done = scenario
                .messages(&check)
                .await
                .iter()
                .any(|m| m.3 == "Checked 1 task.");
            if done {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("the watchdog check finishes");
    assert!(scenario
        .messages(&check)
        .await
        .iter()
        .any(|m| m.1.as_deref() == Some(scenario.router.as_str())));

    let requests = scenario.requests.lock().await;
    let checks: Vec<&Value> = requests
        .iter()
        .filter(|r| tool_names(r).iter().any(|name| name == "ReviewTasks"))
        .collect();
    assert_eq!(checks.len(), 3, "review, act, summarise");
    let mut tools = tool_names(checks[0]);
    tools.sort();
    assert_eq!(
        tools,
        [
            "NotifyUser",
            "ReadEvent",
            "ReadTask",
            "ReadThread",
            "ReviewTasks",
            "StopTask"
        ],
        "a watchdog gets only its read-only tools"
    );
    assert!(checks[0].to_string().contains("Watchdog role:"));
    let review = tool_result(&requests, "call-review").unwrap();
    assert_eq!(review["tasks"][0]["task_id"], json!(task));
    assert_eq!(review["tasks"][0]["signals"], json!(["failed"]));
    let mut notified = [
        tool_result(&requests, "call-notify").unwrap()["status"].clone(),
        tool_result(&requests, "call-notify-again").unwrap()["status"].clone(),
    ];
    notified.sort_by_key(|status| status.to_string());
    assert_eq!(
        notified,
        [json!("already_notified"), json!("notified")],
        "one alert per task inside the cooldown"
    );
    assert_eq!(
        tool_result(&requests, "call-stop").unwrap()["status"],
        "idle"
    );
    drop(requests);

    let alerts: Vec<(String, String, i64)> = sqlx::query_as(
        "SELECT sender_type,content_json,context_admitted FROM messages \
         WHERE thread_id=? AND message_type='watchdog_alert' ORDER BY seq",
    )
    .bind(&main)
    .fetch_all(pool)
    .await
    .unwrap();
    assert_eq!(alerts.len(), 2, "{alerts:?}");
    let actions: Vec<Value> = alerts
        .iter()
        .map(|(sender, meta, admitted)| {
            assert_eq!(sender, "system");
            assert_eq!(*admitted, 0, "alerts stay out of model history");
            let meta: Value = serde_json::from_str(meta).unwrap();
            assert_eq!(meta["watchdog_alert"]["task_id"], json!(task));
            meta["watchdog_alert"]["action"].clone()
        })
        .collect();
    assert_eq!(actions, [json!("notified"), json!("stopped")]);
    let events: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM account_events WHERE kind='watchdog.alert'")
            .fetch_one(pool)
            .await
            .unwrap();
    assert_eq!(events, 2);
    let quiet: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM account_events WHERE kind='job.changed' AND thread_id=?          AND json_extract(payload_json,'$.watchdog')=1)",
    )
    .bind(&check)
    .fetch_one(pool)
    .await
    .unwrap();
    assert!(quiet, "job events mark the watchdog thread");
    let (status, page) = send(
        &scenario.app,
        authed_empty(
            "GET",
            &format!(
                "/api/v2/groups/{}/messages?thread_id={main}",
                scenario.group
            ),
            &scenario.token,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{page}");
    assert!(page.to_string().contains("\"watchdog_alert\":{"), "{page}");

    // Without router mode the check has no seat.
    let (status, error) = send(
        &scenario.app,
        authed_json(
            "PATCH",
            &format!("/api/v2/groups/{}", scenario.group),
            &scenario.token,
            json!({"scheduler_mode": "bounded"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{error}");
    let (status, error) = send(
        &scenario.app,
        create(json!({"kind": "cron", "expression": "0 * * * *"})),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{error}");

    // With every other thread archived, an untargeted message still opens a
    // new conversation instead of landing in the watchdog thread.
    sqlx::query("UPDATE threads SET status='archived' WHERE id IN (?,?)")
        .bind(&main)
        .bind(&task)
        .execute(pool)
        .await
        .unwrap();
    let (status, receipt) = send(
        &scenario.app,
        authed_json(
            "POST",
            "/api/v2/commands",
            &scenario.token,
            json!({"request_id": uuid::Uuid::new_v4().to_string(), "name": "message.send",
                "target": {"conversation_id": scenario.group},
                "payload": {"content": "untargeted hello"}}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{receipt}");
    let landed: String =
        sqlx::query_scalar("SELECT thread_id FROM messages WHERE content='untargeted hello'")
            .fetch_one(pool)
            .await
            .unwrap();
    assert_ne!(landed, check);
    assert_ne!(landed, main);
}

#[tokio::test]
async fn dispatch_with_task_id_continues_the_task_in_its_thread() {
    let handler = |_request: &Value, messages: &[Value]| {
        let last_user = last_user_text(messages);
        let last_is_tool = messages
            .last()
            .is_some_and(|message| message["role"] == "tool");
        if last_user.contains("Task report") {
            text_body("Summary: the worker finished.")
        } else if last_is_tool {
            text_body("Started the task.")
        } else if last_user.contains("AGAIN") {
            tool_body(vec![(
                "call-continue",
                "DispatchTask",
                json!({"task_id": dispatched_task(messages), "title": "Renamed",
                    "instructions": "Now add tests"}),
            )])
        } else {
            tool_body(vec![(
                "call-dispatch",
                "DispatchTask",
                json!({"assistant": "Coder", "title": "Widget", "instructions": "Build the widget"}),
            )])
        }
    };
    let (url, requests) = scripted_router_provider(handler).await;
    let scenario = router_group_with("continue", url, requests, json!({})).await;
    let main = seed_thread(&scenario.state, &scenario.group, "active").await;
    scenario.say(&main, "build the widget").await;
    scenario.wait_for_summary(&main).await;
    scenario.say(&main, "AGAIN with tests").await;
    scenario.wait_for_summaries(&main, 2).await;

    let task = scenario.only_task(&main).await;
    let continued = tool_result(&scenario.requests.lock().await, "call-continue").unwrap();
    assert_eq!(continued["task_id"], json!(task));
    assert_eq!(continued["status"], "continued");
    assert_eq!(continued["run"], 2);
    assert_eq!(continued["assistant"], "Coder");
    assert_eq!(
        continued["title"], "Widget",
        "a continued task keeps its title"
    );
    let title: String = sqlx::query_scalar("SELECT title FROM threads WHERE id=?")
        .bind(&task)
        .fetch_one(scenario.state.db.pool())
        .await
        .unwrap();
    assert_eq!(title, "Widget");

    // The worker's second run sees the first instruction and its own reply.
    let requests = scenario.requests.lock().await;
    let worker_requests: Vec<String> = requests
        .iter()
        .filter(|r| !is_router_request(r))
        .map(Value::to_string)
        .collect();
    assert_eq!(worker_requests.len(), 2);
    let second = &worker_requests[1];
    for seen in ["Build the widget", "worker result 42", "Now add tests"] {
        assert!(second.contains(seen), "{seen} in {second}");
    }
    drop(requests);

    let runs: Vec<Value> = scenario
        .reports(&main)
        .await
        .iter()
        .map(|report| report["run"].clone())
        .collect();
    assert_eq!(runs, [json!(1), json!(2)]);
    let summary_requests = scenario.requests.lock().await;
    assert!(
        summary_requests
            .iter()
            .any(|r| is_router_request(r) && r.to_string().contains("- run: 2")),
        "the router reads which run reported"
    );
}

#[tokio::test]
async fn dispatch_with_task_id_refuses_tasks_it_cannot_continue() {
    let handler = |_request: &Value, messages: &[Value]| {
        let last_user = last_user_text(messages);
        let last_is_tool = messages
            .last()
            .is_some_and(|message| message["role"] == "tool");
        if last_user.contains("Task report") {
            text_body("Summary: the worker finished.")
        } else if last_is_tool {
            text_body("Checked.")
        } else if last_user.contains("TRY ") {
            let ids = ids_after(messages, "TRY ");
            let continue_call = |id: &str, extra: Value| {
                let mut args = json!({"task_id": id, "instructions": "More"});
                args.as_object_mut()
                    .unwrap()
                    .extend(extra.as_object().cloned().unwrap_or_default());
                args
            };
            tool_body(vec![
                (
                    "call-foreign",
                    "DispatchTask",
                    continue_call(&ids[0], json!({})),
                ),
                (
                    "call-send-foreign",
                    "SendToTask",
                    json!({"task_id": ids[0], "message": "More"}),
                ),
                (
                    "call-waiting",
                    "DispatchTask",
                    continue_call(&ids[1], json!({})),
                ),
                (
                    "call-archived",
                    "DispatchTask",
                    continue_call(&ids[2], json!({})),
                ),
                (
                    "call-other-assistant",
                    "DispatchTask",
                    continue_call(&ids[3], json!({"assistant": "Coder"})),
                ),
            ])
        } else {
            tool_body(vec![(
                "call-dispatch",
                "DispatchTask",
                json!({"assistant": "Coder", "instructions": "Build the widget"}),
            )])
        }
    };
    let (url, requests) = scripted_router_provider(handler).await;
    let scenario = router_group_with("continue-refused", url, requests, json!({})).await;
    let pool = scenario.state.db.pool();
    let main = seed_thread(&scenario.state, &scenario.group, "active").await;
    let other = seed_thread(&scenario.state, &scenario.group, "active").await;

    // A real task whose run is then held waiting for the user.
    scenario.say(&main, "build the widget").await;
    scenario.wait_for_summary(&main).await;
    let waiting = scenario.only_task(&main).await;
    sqlx::query("UPDATE execution_jobs SET status='waiting_for_user' WHERE thread_id=?")
        .bind(&waiting)
        .execute(pool)
        .await
        .unwrap();
    let foreign = seed_task_thread(
        &scenario.state,
        &scenario.group,
        &scenario.worker,
        &other,
        "Foreign",
        "2024-01-01T00:00:00Z",
    )
    .await;
    let archived = seed_task_thread(
        &scenario.state,
        &scenario.group,
        &scenario.worker,
        &main,
        "Archived",
        "2024-01-01T00:00:00Z",
    )
    .await;
    sqlx::query("UPDATE threads SET status='archived' WHERE id=?")
        .bind(&archived)
        .execute(pool)
        .await
        .unwrap();
    let (owner, provider): (String, String) =
        sqlx::query_as("SELECT owner_id,provider_id FROM agents WHERE id=?")
            .bind(&scenario.worker)
            .fetch_one(pool)
            .await
            .unwrap();
    let reviewer = seed_agent(
        &scenario.state,
        &owner,
        &scenario.group,
        &provider,
        "Reviewer",
        "2024-01-01T00:00:02Z",
    )
    .await;
    let reviewed = seed_task_thread(
        &scenario.state,
        &scenario.group,
        &reviewer,
        &main,
        "Review",
        "2024-01-01T00:00:00Z",
    )
    .await;

    scenario
        .say(
            &main,
            &format!("TRY {foreign} {waiting} {archived} {reviewed}"),
        )
        .await;
    for (call, reason) in [
        ("call-foreign", "dispatched from another conversation"),
        ("call-send-foreign", "dispatched from another conversation"),
        ("call-waiting", "waiting for the user's own answer"),
        ("call-archived", "is archived and cannot be continued"),
        (
            "call-other-assistant",
            "is run by Reviewer; to hand the work to another assistant, dispatch a new task",
        ),
    ] {
        let output = scenario.wait_for_tool_text(call).await;
        assert!(output.contains(reason), "{call}: {output}");
    }
    for (task, expected) in [(&foreign, 0), (&waiting, 1), (&archived, 0), (&reviewed, 0)] {
        assert_eq!(
            scenario.instructions_in(task).await,
            expected,
            "a refused continuation writes nothing"
        );
    }
}

#[tokio::test]
async fn a_cancelled_task_continues_without_its_topics_until_given_again() {
    let handler = |_request: &Value, messages: &[Value]| {
        let last_user = last_user_text(messages);
        let last_is_tool = messages
            .last()
            .is_some_and(|message| message["role"] == "tool");
        if last_user.contains("Task report") {
            text_body("Summary: the worker finished.")
        } else if last_is_tool {
            text_body("Started the task.")
        } else if last_user.contains("STOP") {
            tool_body(vec![(
                "call-cancel",
                "CancelTask",
                json!({"task_id": dispatched_task(messages)}),
            )])
        } else if last_user.contains("AGAIN") {
            tool_body(vec![(
                "call-again",
                "DispatchTask",
                json!({"task_id": dispatched_task(messages), "instructions": "Resume"}),
            )])
        } else if last_user.contains("TOPICS") {
            tool_body(vec![(
                "call-topics",
                "DispatchTask",
                json!({"task_id": dispatched_task(messages), "instructions": "Resume with the PR",
                    "topics": ["github:owner/repo#12"]}),
            )])
        } else {
            tool_body(vec![(
                "call-dispatch",
                "DispatchTask",
                json!({"assistant": "Coder", "instructions": "Watch the PR",
                    "topics": ["github:owner/repo#12"]}),
            )])
        }
    };
    let (url, requests) = scripted_router_provider(handler).await;
    let scenario = router_group_with("continue-cancelled", url, requests, json!({})).await;
    let main = seed_thread(&scenario.state, &scenario.group, "active").await;
    scenario.say(&main, "watch the PR").await;
    scenario.wait_for_summary(&main).await;
    let task = scenario.only_task(&main).await;
    let followed = scenario.topics_of(&task).await;
    assert_eq!(followed.len(), 1);

    scenario.say(&main, "STOP it").await;
    scenario.wait_for_tool_text("call-cancel").await;
    assert!(scenario.topics_of(&task).await.is_empty());

    scenario.say(&main, "AGAIN please").await;
    scenario.wait_for_summaries(&main, 2).await;
    let again = tool_result(&scenario.requests.lock().await, "call-again").unwrap();
    assert_eq!(again["status"], "continued");
    assert_eq!(
        again["topics"],
        json!([]),
        "topics are not restored on their own"
    );
    assert!(scenario.topics_of(&task).await.is_empty());

    scenario.say(&main, "TOPICS back").await;
    scenario.wait_for_summaries(&main, 3).await;
    let restored = tool_result(&scenario.requests.lock().await, "call-topics").unwrap();
    assert_eq!(restored["topics"], json!(followed));
    assert_eq!(restored["run"], 3);
    assert_eq!(scenario.topics_of(&task).await, followed);
}
