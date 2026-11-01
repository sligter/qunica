use super::*;

async fn get_metrics(app: &Router, token: &str, suffix: &str) -> Value {
    let (status, body) = send(
        app,
        authed_empty(
            "GET",
            &format!("/api/v2/runtime/engine-metrics?days=7{suffix}"),
            token,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    body
}

#[tokio::test]
async fn canary_controls_are_authenticated_owner_scoped_and_never_change_agents() {
    let (app, state) = router_with_state_for_tests().await;
    let one = register_and_login(&app, "canary-one@example.test").await;
    let two = register_and_login(&app, "canary-two@example.test").await;
    let (status, _) = send(
        &app,
        Request::builder()
            .uri("/api/v2/runtime/engines")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let (status, before) = send(&app, authed_empty("GET", "/api/v2/runtime/engines", &one)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(before["default_engine"], "native");
    assert_eq!(before["rig_available"], cfg!(feature = "rig-engine"));
    assert_eq!(before["rig_paused"], false);
    let (status, after) = send(
        &app,
        authed_json(
            "PATCH",
            "/api/v2/runtime/engines/rig",
            &one,
            json!({"paused":true}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(after["rig_paused"], true);
    let (_, other) = send(&app, authed_empty("GET", "/api/v2/runtime/engines", &two)).await;
    assert_eq!(other["rig_paused"], false);
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM engine_rollouts")
        .fetch_one(state.db.pool())
        .await
        .unwrap();
    assert_eq!(count, 1);
    assert_eq!(get_metrics(&app, &two, "").await["rows"], json!([]));
    for days in [0, 91] {
        let (status, _) = send(
            &app,
            authed_empty(
                "GET",
                &format!("/api/v2/runtime/engine-metrics?days={days}"),
                &one,
            ),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }
}

#[tokio::test]
async fn canary_native_metrics_use_execution_snapshots_and_do_not_multiply_usage() {
    let (app, state) = router_with_state_for_tests().await;
    let email = "canary-native@example.test";
    let token = register_and_login(&app, email).await;
    let owner = owner_id(&state, email).await;
    let (_root, workspace) = create_local_workspace(&app, &token).await;
    let group = create_group(&app, &token, &workspace, json!({"free_speech":true})).await;
    let body = tool_body_with_usage(
        vec![("missing", "Read", json!({"file_path":"absent.txt"}))],
        10,
    );
    let final_body=text_body("finished").replace("data: [DONE]",&format!("data: {}\n\ndata: [DONE]",json!({"choices":[],"usage":{"prompt_tokens":20,"completion_tokens":0,"total_tokens":20}})));
    let (url, requests) = recording_fake_provider_status_sequence(vec![
        (StatusCode::OK, body),
        (StatusCode::OK, final_body),
    ])
    .await;
    let provider = seed_provider(&state, &owner, &url).await;
    let agent = seed_agent_with_tool_config(
        &state,
        &owner,
        &group,
        &provider,
        "Worker",
        "2024-01-01T00:00:00Z",
        json!({"tools":{"read":{"enabled":true}}}),
    )
    .await;
    let events = stream_events(
        &app,
        &format!("/api/v2/groups/{group}/messages/stream"),
        &token,
        json!({"content":"read absent file"}),
    )
    .await;
    assert_eq!(
        payloads_of_kind(&events, StreamEventKind::AgentMessage).len(),
        1,
        "{events:?}"
    );
    assert_eq!(requests.lock().await.len(), 2);
    // Current settings must never relabel historical native runs as Rig.
    set_agent_model_config(&state, &agent, json!({"engine":"rig","model":"different"})).await;
    let metrics = get_metrics(
        &app,
        &token,
        &format!("&agent_id={agent}&provider_id={provider}&model=test-model"),
    )
    .await;
    let row = &metrics["rows"][0];
    assert_eq!(row["engine"], "native");
    assert_eq!(row["completed"], 1);
    assert_eq!(row["partial_executions"], 0);
    assert_eq!(row["failed"], 0);
    assert_eq!(row["tool_calls"], 1);
    assert_eq!(row["settled_tool_calls"], 1);
    assert_eq!(row["tool_errors"], 1);
    assert_eq!(row["failure_rate"], 0.0);
    assert_eq!(row["tool_error_rate"], 1.0);
    assert_eq!(row["usage_calls"], 2);
    assert_eq!(row["total_tokens"], 30);
    assert_eq!(row["executions_without_usage"], 0);
    let linked:i64=sqlx::query_scalar("SELECT COUNT(*) FROM token_usage_records t JOIN engine_executions e ON e.id=t.execution_id WHERE e.agent_id=? AND t.engine='native'").bind(&agent).fetch_one(state.db.pool()).await.unwrap();
    assert_eq!(linked, 2);
    assert_eq!(
        get_metrics(&app, &token, "&model=unmatched").await["rows"],
        json!([])
    );
    let other = register_and_login(&app, "canary-outsider@example.test").await;
    assert_eq!(
        get_metrics(&app, &other, &format!("&agent_id={agent}")).await["rows"],
        json!([])
    );
}

#[tokio::test]
async fn canary_partial_and_nonterminal_runs_are_excluded_from_failure_and_cost_samples() {
    let (app, state) = router_with_state_for_tests().await;
    let email = "canary-coverage@example.test";
    let token = register_and_login(&app, email).await;
    let owner = owner_id(&state, email).await;
    let (_root, workspace) = create_local_workspace(&app, &token).await;
    let group = create_group(&app, &token, &workspace, json!({})).await;
    let thread = seed_thread(&state, &group, "active").await;
    let now = time::OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap();
    for (index, status, complete) in [
        (0, "completed", true),
        (1, "failed", true),
        (2, "failed", false),
        (3, "waiting_for_user", true),
        (4, "cancelled", true),
        (5, "blocked", true),
        (6, "outcome_unknown", true),
        (7, "running", true),
    ] {
        sqlx::query("INSERT INTO engine_executions(id,owner_id,group_id,thread_id,engine,app_version,engine_version,status,coverage_complete,started_at,updated_at) VALUES(?,?,?,?,'rig','test','0.45.0',?,?,?,?)")
            .bind(format!("sample-{index}")).bind(&owner).bind(&group).bind(&thread).bind(status).bind(complete).bind(&now).bind(&now).execute(state.db.pool()).await.unwrap();
    }
    // Model gauges from an old invocation cannot be backfilled from today's agent config.
    sqlx::query("INSERT INTO token_usage_records(id,owner_id,group_id,group_name,conversation_kind,thread_id,agent_name,provider_name,model,input_tokens,output_tokens,total_tokens,created_at,updated_at) VALUES('old-call',?,?,'Canary','group',?,'Old agent','Old provider','old-model',5,0,5,?,?)")
        .bind(&owner).bind(&group).bind(&thread).bind(&now).bind(&now).execute(state.db.pool()).await.unwrap();
    sqlx::query("INSERT INTO token_usage_records(id,owner_id,group_name,conversation_kind,agent_name,provider_name,model,total_tokens,execution_id,engine,created_at,updated_at) VALUES('partial-call',?,'Canary','group','Worker','Fake','test-model',99,'sample-2','rig',?,?)")
        .bind(&owner).bind(&now).bind(&now).execute(state.db.pool()).await.unwrap();
    qunica_backend::api::commands::recover(&state)
        .await
        .unwrap();
    let metrics = get_metrics(&app, &token, "").await;
    let row = &metrics["rows"][0];
    assert_eq!(row["executions"], 8);
    assert_eq!(row["completed"], 1);
    assert_eq!(row["failed"], 1);
    assert_eq!(row["failure_rate"], 0.5);
    assert_eq!(row["partial_executions"], 2);
    assert_eq!(row["interrupted"], 1);
    assert_eq!(row["pending"], 1);
    assert_eq!(row["cancelled"], 1);
    assert_eq!(row["outcome_unknown"], 1);
    assert_eq!(row["blocked"], 1);
    assert_eq!(row["executions_without_usage"], 1);
    assert_eq!(row["usage_calls"], 0);
    assert_eq!(
        row["total_tokens"], 0,
        "partial costs are not comparable samples"
    );
    assert_eq!(row["tool_error_rate"], Value::Null);
    assert_eq!(metrics["unattributed_calls"], 1);
}
