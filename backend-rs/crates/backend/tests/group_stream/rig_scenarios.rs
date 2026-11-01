use super::*;

struct Scenario {
    app: Router,
    state: AppState,
    token: String,
    root: tempfile::TempDir,
    group: String,
    agent: String,
    requests: Arc<Mutex<Vec<Value>>>,
}

impl Scenario {
    async fn new(name: &str, responses: Vec<(StatusCode, String)>) -> Self {
        let (app, state) = router_with_state_for_tests().await;
        let email = format!("rig-{name}@example.test");
        let token = register_and_login(&app, &email).await;
        let owner = owner_id(&state, &email).await;
        let (root, workspace) = create_local_workspace(&app, &token).await;
        let group = create_group(&app, &token, &workspace, json!({"free_speech":true})).await;
        let (url, requests) = recording_fake_provider_status_sequence(responses).await;
        let provider = seed_provider(&state, &owner, &url).await;
        let agent = seed_agent_with_tool_config(&state, &owner, &group, &provider, "Worker",
            "2024-01-01T00:00:00Z", json!({"tools":{
                "read":{"enabled":true},"write":{"enabled":true},"edit":{"enabled":true},"bash":{"enabled":true}
            }})).await;
        set_agent_model_config(&state, &agent, json!({"engine":"rig"})).await;
        Self {
            app,
            state,
            token,
            root,
            group,
            agent,
            requests,
        }
    }

    async fn run(&self) -> Vec<Value> {
        stream_events(
            &self.app,
            &format!("/api/v2/groups/{}/messages/stream", self.group),
            &self.token,
            json!({"content":"perform the task"}),
        )
        .await
    }

    async fn checkpoint(&self) -> Value {
        let raw: String = sqlx::query_scalar("SELECT content_json FROM messages WHERE group_id=? AND sender_id=? ORDER BY seq DESC LIMIT 1")
            .bind(&self.group).bind(&self.agent).fetch_one(self.state.db.pool()).await.unwrap();
        serde_json::from_str(&raw).unwrap()
    }
}

fn ok(body: String) -> (StatusCode, String) {
    (StatusCode::OK, body)
}
fn billed_text(text: &str, tokens: u64) -> String {
    text_body(text).replace("data: [DONE]", &format!("data: {}\n\ndata: [DONE]",
        json!({"choices":[],"usage":{"prompt_tokens":tokens,"completion_tokens":0,"total_tokens":tokens}})))
}

#[tokio::test]
async fn rig_read_edit_bash_finishes_with_one_completion_per_attempt() {
    let scenario = Scenario::new(
        "edit",
        vec![
            ok(tool_body(vec![(
                "read",
                "Read",
                json!({"file_path":"source.txt"}),
            )])),
            ok(tool_body(vec![(
                "edit",
                "Edit",
                json!({"file_path":"source.txt","old_string":"before","new_string":"after"}),
            )])),
            ok(tool_body(vec![(
                "shell",
                "Bash",
                json!({"command":"echo checked"}),
            )])),
            ok(billed_text("finished", 20)),
        ],
    )
    .await;
    std::fs::write(scenario.root.path().join("source.txt"), "before").unwrap();
    let events = scenario.run().await;
    assert!(
        payloads_of_kind(&events, StreamEventKind::Error).is_empty(),
        "{events:?}"
    );
    assert_eq!(
        std::fs::read_to_string(scenario.root.path().join("source.txt")).unwrap(),
        "after"
    );
    assert_eq!(
        payloads_of_kind(&events, StreamEventKind::ToolCallResult).len(),
        3
    );
    let saved = scenario.checkpoint().await;
    assert_eq!(saved["driver_checkpoint"]["attempts"], 4);
    assert_eq!(saved["driver_checkpoint"]["completion_calls"], 4);
    assert_eq!(
        saved["driver_checkpoint"]["total_tokens"], 20,
        "usage is not counted twice"
    );
    assert_eq!(saved["driver_checkpoint"]["phase"], "done");
    assert_eq!(
        saved["assistant_turns"].as_array().unwrap().len(),
        4,
        "final no-tool message is preserved too"
    );
}

#[tokio::test]
async fn rig_overflow_rebuilds_after_settlement_without_resetting_budget() {
    struct Compact;
    #[async_trait::async_trait]
    impl qunica_backend::runtime::RuntimeHook for Compact {
        fn name(&self) -> &'static str {
            "safe-boundary-compaction"
        }
        async fn request_error(
            &self,
            _: &qunica_backend::runtime::StepContext,
            messages: &mut Vec<qunica_backend::llm::ChatMessage>,
            error: &anyhow::Error,
        ) -> qunica_backend::runtime::RequestRecovery {
            if !error
                .downcast_ref::<qunica_backend::llm::ProviderHttpError>()
                .is_some_and(|error| error.is_context_overflow())
            {
                return qunica_backend::runtime::RequestRecovery::Propagate;
            }
            assert!(
                messages.iter().any(|message| message.role == "tool"),
                "compaction sees settled tools"
            );
            let prompt = messages
                .iter_mut()
                .find(|message| message.role == "user")
                .unwrap();
            prompt.content = "compacted summary".into();
            qunica_backend::runtime::RequestRecovery::Retry {
                reason: "compacted at safe boundary".into(),
            }
        }
    }
    let scenario = Scenario::new(
        "overflow",
        vec![
            ok(tool_body_with_usage(
                vec![(
                    "append",
                    "Bash",
                    json!({"command":"echo once >> marker.txt"}),
                )],
                40,
            )),
            (
                StatusCode::BAD_REQUEST,
                json!({"error":{"message":"context window exceeded"}}).to_string(),
            ),
            ok(billed_text("recovered", 20)),
        ],
    )
    .await;
    let owner = owner_id(&scenario.state, "rig-overflow@example.test").await;
    let services = RuntimeServices::new(
        scenario.state.db.pool().clone(),
        scenario.state.write_lock.clone(),
    )
    .with_hooks(qunica_backend::runtime::HookChain::new().with(Arc::new(Compact)));
    let (tx, mut rx) = mpsc::channel(256);
    let task = tokio::spawn(run_group_turn(
        services,
        TurnRequest {
            group_id: scenario.group.clone(),
            owner_id: owner,
            thread_id: None,
            content: "append once".into(),
            attachments: vec![],
            model_override: None,
            effort_override: None,
        },
        tx,
    ));
    let mut events = Vec::new();
    while let Some(event) = rx.recv().await {
        events.push(serde_json::to_value(event).unwrap());
    }
    assert_eq!(task.await.unwrap(), TurnOutcome::Completed, "{events:?}");
    assert_eq!(
        std::fs::read_to_string(scenario.root.path().join("marker.txt"))
            .unwrap()
            .lines()
            .count(),
        1
    );
    let saved = scenario.checkpoint().await;
    assert_eq!(saved["driver_checkpoint"]["attempts"], 3);
    assert_eq!(saved["driver_checkpoint"]["failed_attempts"], 1);
    assert_eq!(saved["driver_checkpoint"]["completion_calls"], 2);
    assert_eq!(saved["driver_checkpoint"]["total_tokens"], 60);
    let requests = scenario.requests.lock().await;
    assert_eq!(requests.len(), 3);
    assert!(requests[2].to_string().contains("compacted summary"));
    assert_eq!(
        requests[2]["messages"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|message| message["role"] == "tool")
            .count(),
        1
    );
}

#[tokio::test]
async fn rig_invalid_or_cut_stream_never_dispatches_or_retries_visible_output() {
    let fixtures = [
        (
            sse(vec![
                json!({"choices":[{"delta":{"content":"partial"},"finish_reason":"length"}]}),
            ]) + "data: [DONE]\n\n",
            1,
        ),
        (
            sse(vec![
                json!({"choices":[{"delta":{"content":"partial"}}]}),
                json!({"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"name":"Bash","arguments":"{\"command\":\"echo forbidden\"}"}}]},"finish_reason":"tool_calls"}]}),
            ]) + "data: [DONE]\n\n",
            1,
        ),
        (
            format!(
                "data: {}\n\n{}",
                json!({"choices":[{"delta":{"content":"partial"}}]}),
                tool_body(vec![("unknown", "Undeclared", json!({}))])
            ),
            1,
        ),
        (
            format!(
                "data: {}\n\n",
                json!({"choices":[{"delta":{"content":"partial"}}]})
            ),
            0,
        ),
        (
            format!(
                "data: {}\n\n{}",
                json!({"choices":[{"delta":{"content":"partial"}}]}),
                tool_body(vec![("bad", "Bash", Value::Null)])
            ),
            1,
        ),
    ];
    for (index, (fixture, completions)) in fixtures.into_iter().enumerate() {
        let scenario = Scenario::new(
            &format!("invalid-{index}"),
            vec![ok(fixture), ok(text_body("must not retry"))],
        )
        .await;
        let events = scenario.run().await;
        assert!(
            payloads_of_kind(&events, StreamEventKind::ToolCallStart).is_empty(),
            "{events:?}"
        );
        assert!(
            !payloads_of_kind(&events, StreamEventKind::DispatchFailed).is_empty(),
            "{events:?}"
        );
        assert_eq!(scenario.requests.lock().await.len(), 1);
        let saved = scenario.checkpoint().await;
        assert_eq!(saved["driver_checkpoint"]["phase"], "aborted");
        assert_eq!(saved["driver_checkpoint"]["failed_attempts"], 1);
        assert_eq!(saved["driver_checkpoint"]["completion_calls"], completions);
        assert!(saved["assistant_turns"].as_array().unwrap().is_empty());
        assert!(saved["response_segments"]
            .as_array()
            .unwrap()
            .iter()
            .any(|text| text == "partial"));
        let thread: String =
            sqlx::query_scalar("SELECT thread_id FROM messages WHERE group_id=? AND sender_id=?")
                .bind(&scenario.group)
                .bind(&scenario.agent)
                .fetch_one(scenario.state.db.pool())
                .await
                .unwrap();
        let resumed = stream_events(
            &scenario.app,
            &format!("/api/v2/threads/{thread}/resume"),
            &scenario.token,
            json!({}),
        )
        .await;
        assert_eq!(
            payloads_of_kind(&resumed, StreamEventKind::AgentMessage).len(),
            1,
            "{resumed:?}"
        );
        let requests = scenario.requests.lock().await;
        assert_eq!(
            requests.len(),
            2,
            "only an explicit resume may request again"
        );
        assert!(
            !requests[1].to_string().contains("partial"),
            "aborted display is not a model message: {}",
            requests[1]
        );
        assert_eq!(
            scenario.checkpoint().await["driver_checkpoint"]["attempts"],
            2
        );
    }
}

#[tokio::test]
async fn rig_call_and_failure_limits_stop_before_another_request() {
    for (name, first, limits, failures) in [
        (
            "call-limit",
            ok(tool_body(vec![(
                "read",
                "Read",
                json!({"file_path":"source.txt"}),
            )])),
            json!({"max_model_calls":1}),
            0,
        ),
        (
            "failure-limit",
            (StatusCode::TOO_MANY_REQUESTS, "rate limit".into()),
            json!({"max_failed_attempts":1}),
            1,
        ),
    ] {
        let scenario = Scenario::new(name, vec![first, ok(text_body("must not request"))]).await;
        std::fs::write(scenario.root.path().join("source.txt"), "source").unwrap();
        set_agent_model_config(
            &scenario.state,
            &scenario.agent,
            json!({"engine":"rig","rig_limits":limits}),
        )
        .await;
        let events = scenario.run().await;
        assert!(
            !payloads_of_kind(&events, StreamEventKind::DispatchFailed).is_empty(),
            "{events:?}"
        );
        assert_eq!(scenario.requests.lock().await.len(), 1);
        let saved = scenario.checkpoint().await;
        assert_eq!(saved["driver_checkpoint"]["attempts"], 1);
        assert_eq!(saved["driver_checkpoint"]["failed_attempts"], failures);
    }
}

#[tokio::test]
async fn rig_token_limit_records_charge_but_blocks_tool_execution() {
    let scenario = Scenario::new(
        "token-limit",
        vec![ok(tool_body_with_usage(
            vec![(
                "write",
                "Bash",
                json!({"command":"echo forbidden >> marker.txt"}),
            )],
            10,
        ))],
    )
    .await;
    set_agent_model_config(
        &scenario.state,
        &scenario.agent,
        json!({"engine":"rig","rig_limits":{"max_total_tokens":5}}),
    )
    .await;
    let events = scenario.run().await;
    assert!(
        payloads_of_kind(&events, StreamEventKind::ToolCallStart).is_empty(),
        "{events:?}"
    );
    assert!(!scenario.root.path().join("marker.txt").exists());
    let saved = scenario.checkpoint().await;
    assert_eq!(saved["driver_checkpoint"]["completion_calls"], 1);
    assert_eq!(saved["driver_checkpoint"]["total_tokens"], 10);
    assert_eq!(saved["driver_checkpoint"]["failed_attempts"], 1);
    let billed: i64 =
        sqlx::query_scalar("SELECT SUM(total_tokens) FROM token_usage_records WHERE group_id=?")
            .bind(&scenario.group)
            .fetch_one(scenario.state.db.pool())
            .await
            .unwrap();
    assert_eq!(billed, 10);
}

fn sse(events: Vec<Value>) -> String {
    events
        .iter()
        .map(|event| format!("data: {event}\n\n"))
        .collect()
}

#[tokio::test]
async fn rig_gemini_and_responses_keep_native_metadata_through_the_tool_loop() {
    for kind in ["gemini", "openai-responses"] {
        let (first, next, retained) = if kind == "gemini" {
            (
                sse(vec![
                    json!({"candidates":[{"content":{"role":"model","parts":[
                {"text":"plan","thought":true},
                {"functionCall":{"name":"Read","args":{"file_path":"source.txt"}},"thoughtSignature":"c2lnLW9yaWdpbmFs","opaqueMetadata":{"revision":7}}
            ]},"finishReason":"STOP"}]}),
                ]),
                sse(vec![
                    json!({"candidates":[{"content":{"role":"model","parts":[{"text":"done"}]},"finishReason":"STOP"}]}),
                ]),
                "c2lnLW9yaWdpbmFs",
            )
        } else {
            (
                sse(vec![
                    json!({"type":"response.output_item.done","output_index":0,"item":{"type":"reasoning","id":"rs","encrypted_content":"cipher-final","summary":[],"opaqueMetadata":{"revision":7}}}),
                    json!({"type":"response.output_item.done","output_index":1,"item":{"type":"function_call","id":"fc","call_id":"read","name":"Read","arguments":"{\"file_path\":\"source.txt\"}"}}),
                    json!({"type":"response.completed","response":{"status":"completed"}}),
                ]),
                sse(vec![
                    json!({"type":"response.output_item.added","output_index":0,"item":{"type":"message","id":"msg","role":"assistant","content":[]}}),
                    json!({"type":"response.output_text.delta","output_index":0,"item_id":"msg","content_index":0,"delta":"done"}),
                    json!({"type":"response.output_item.done","output_index":0,"item":{"type":"message","id":"msg","role":"assistant","content":[{"type":"output_text","text":"done"}]}}),
                    json!({"type":"response.completed","response":{"status":"completed"}}),
                ]),
                "cipher-final",
            )
        };
        let scenario = Scenario::new(kind, vec![ok(first), ok(next)]).await;
        sqlx::query(
            "UPDATE llm_providers SET kind=? WHERE id=(SELECT provider_id FROM agents WHERE id=?)",
        )
        .bind(kind)
        .bind(&scenario.agent)
        .execute(scenario.state.db.pool())
        .await
        .unwrap();
        std::fs::write(scenario.root.path().join("source.txt"), "source").unwrap();
        let events = scenario.run().await;
        assert_eq!(
            payloads_of_kind(&events, StreamEventKind::ToolCallResult).len(),
            1,
            "{kind}: {events:?}"
        );
        assert_eq!(
            payloads_of_kind(&events, StreamEventKind::AgentMessage)
                .first()
                .map(|value| &value["content"]),
            Some(&json!("done")),
            "{kind}: {events:?}"
        );
        let saved = scenario.checkpoint().await;
        assert_eq!(saved["driver_checkpoint"]["attempts"], 2);
        assert_eq!(saved["driver_checkpoint"]["completion_calls"], 2);
        let requests = scenario.requests.lock().await;
        assert_eq!(requests.len(), 2);
        assert!(
            requests[1].to_string().contains(retained),
            "{kind}: {}",
            requests[1]
        );
        assert!(
            requests[1].to_string().contains("opaqueMetadata"),
            "{kind}: {}",
            requests[1]
        );
    }
}

#[tokio::test]
async fn rig_silent_checkpoint_never_becomes_public() {
    let scenario = Scenario::new("silent", vec![ok(text_body("<SILENT>"))]).await;
    sqlx::query("UPDATE groups SET proactive_mode=1 WHERE id=?")
        .bind(&scenario.group)
        .execute(scenario.state.db.pool())
        .await
        .unwrap();
    let events = scenario.run().await;
    assert_eq!(
        payloads_of_kind(&events, StreamEventKind::AgentSilent).len(),
        1,
        "{events:?}"
    );
    assert!(payloads_of_kind(&events, StreamEventKind::AgentMessage).is_empty());
    let count:i64=sqlx::query_scalar("SELECT COUNT(*) FROM messages WHERE group_id=? AND sender_type='agent' AND status='visible'")
        .bind(&scenario.group).fetch_one(scenario.state.db.pool()).await.unwrap();
    assert_eq!(count, 0);
    let saved = scenario.checkpoint().await;
    assert_eq!(saved["driver_checkpoint"]["phase"], "done");
    assert_eq!(saved["driver_checkpoint"]["attempts"], 1);
}

#[tokio::test]
async fn rig_rejected_terminal_is_billed_once_without_accepting_a_turn() {
    for kind in ["response.failed", "response.incomplete"] {
        let fixture = sse(vec![
            json!({"type":kind,"response":{"error":{"message":"failed"},"incomplete_details":{"reason":"max_output_tokens"},"usage":{"input_tokens":10,"output_tokens":2,"total_tokens":12}}}),
        ]);
        let scenario =
            Scenario::new(kind, vec![ok(fixture), ok(text_body("must not retry"))]).await;
        sqlx::query("UPDATE llm_providers SET kind='openai-responses' WHERE id=(SELECT provider_id FROM agents WHERE id=?)")
            .bind(&scenario.agent).execute(scenario.state.db.pool()).await.unwrap();
        let events = scenario.run().await;
        assert!(payloads_of_kind(&events, StreamEventKind::ToolCallStart).is_empty());
        assert!(payloads_of_kind(&events, StreamEventKind::AgentMessage).is_empty());
        assert_eq!(scenario.requests.lock().await.len(), 1);
        let saved = scenario.checkpoint().await;
        assert_eq!(saved["driver_checkpoint"]["attempts"], 1, "{kind}: {saved}");
        assert_eq!(
            saved["driver_checkpoint"]["completion_calls"], 1,
            "{kind}: {saved}"
        );
        assert_eq!(saved["driver_checkpoint"]["failed_attempts"], 1);
        assert_eq!(
            saved["driver_checkpoint"]["total_tokens"], 12,
            "{kind}: {saved}"
        );
        assert!(saved["assistant_turns"].as_array().unwrap().is_empty());
        let billed: i64 = sqlx::query_scalar(
            "SELECT SUM(total_tokens) FROM token_usage_records WHERE group_id=?",
        )
        .bind(&scenario.group)
        .fetch_one(scenario.state.db.pool())
        .await
        .unwrap();
        assert_eq!(billed, 12);
    }
}

#[tokio::test]
async fn rig_resume_keeps_accepted_tools_and_excludes_the_aborted_tail() {
    let scenario = Scenario::new(
        "accepted-tail",
        vec![
            ok(text_then_tool_body(
                "accepted",
                vec![("read", "Read", json!({"file_path":"source.txt"}))],
            )),
            ok(sse(vec![
                json!({"choices":[{"delta":{"content":"aborted-tail"}}]}),
            ])),
            ok(text_body("resumed")),
        ],
    )
    .await;
    std::fs::write(scenario.root.path().join("source.txt"), "source").unwrap();
    let events = scenario.run().await;
    assert_eq!(
        payloads_of_kind(&events, StreamEventKind::ToolCallResult).len(),
        1
    );
    assert_eq!(
        payloads_of_kind(&events, StreamEventKind::DispatchFailed).len(),
        1
    );
    let thread: String =
        sqlx::query_scalar("SELECT thread_id FROM messages WHERE group_id=? AND sender_id=?")
            .bind(&scenario.group)
            .bind(&scenario.agent)
            .fetch_one(scenario.state.db.pool())
            .await
            .unwrap();
    let events = stream_events(
        &scenario.app,
        &format!("/api/v2/threads/{thread}/resume"),
        &scenario.token,
        json!({}),
    )
    .await;
    assert_eq!(
        payloads_of_kind(&events, StreamEventKind::AgentMessage).len(),
        1,
        "{events:?}"
    );
    assert!(
        payloads_of_kind(&events, StreamEventKind::ToolCallStart).is_empty(),
        "the settled Read is not repeated"
    );
    let requests = scenario.requests.lock().await;
    assert_eq!(requests.len(), 3);
    assert!(requests[2].to_string().contains("accepted"));
    assert!(!requests[2].to_string().contains("aborted-tail"));
    assert_eq!(
        requests[2]["messages"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|message| message["role"] == "tool")
            .count(),
        1
    );
    let saved = scenario.checkpoint().await;
    assert_eq!(saved["driver_checkpoint"]["attempts"], 3);
    assert_eq!(saved["driver_checkpoint"]["failed_attempts"], 1);
    assert_eq!(saved["driver_checkpoint"]["completion_calls"], 2);
    assert!(
        saved["response_segments"]
            .to_string()
            .contains("aborted-tail"),
        "audit fragments are retained"
    );
}
