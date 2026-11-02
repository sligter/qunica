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

/// The JSON result of the router's tool call `call_id`.
fn tool_result(requests: &[Value], call_id: &str) -> Option<Value> {
    let messages: Vec<Value> = requests
        .iter()
        .flat_map(|request| request["messages"].as_array().cloned().unwrap_or_default())
        .collect();
    result_in(&messages, call_id)
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
    let (app, state) = router_with_state_for_tests().await;
    let email = format!("router-{name}@example.test");
    let token = register_and_login(&app, &email).await;
    let owner = owner_id(&state, &email).await;
    let (root, workspace) = create_local_workspace(&app, &token).await;
    let group = create_group(&app, &token, &workspace, json!({})).await;
    let (url, requests) = router_fake_provider(first_router_reply).await;
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
    assert!(router_requests[0].to_string().contains("Router role:"));
    let worker_request = worker_requests[0].to_string();
    assert!(worker_request.contains("Dispatched task:"));
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
