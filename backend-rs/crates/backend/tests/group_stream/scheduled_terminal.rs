use super::*;

#[tokio::test]
async fn scheduled_terminal_completion_matrix_preserves_budget_state_and_event_order() {
    for (name, reply, code, agents, budget, status, reason, tail) in [
        (
            "visible",
            "visible answer",
            StatusCode::OK,
            1,
            8,
            "completed",
            None,
            vec!["turn_completed", "done"],
        ),
        (
            "silent",
            "<SILENT>",
            StatusCode::OK,
            1,
            8,
            "silence",
            Some("silence"),
            vec!["silence", "turn_completed", "done"],
        ),
        (
            "failed",
            "fixture provider error",
            StatusCode::BAD_REQUEST,
            1,
            8,
            "failed",
            None,
            vec!["turn_completed", "done"],
        ),
        (
            "budget",
            "visible answer",
            StatusCode::OK,
            2,
            1,
            "budget_exhausted",
            Some("budget_exhausted"),
            vec!["silence", "turn_budget_exhausted", "done"],
        ),
        (
            "mixed",
            "visible answer",
            StatusCode::OK,
            2,
            8,
            "completed",
            None,
            vec!["turn_completed", "done"],
        ),
        (
            "waiting",
            "<WAITING_FOR_USER> Proceed?",
            StatusCode::OK,
            1,
            8,
            "waiting_for_user",
            Some("waiting_for_user"),
            vec!["done"],
        ),
    ] {
        let (app, state) = router_with_state_for_tests().await;
        let email = format!("terminal-{name}@example.test");
        let token = register_and_login(&app, &email).await;
        let owner = owner_id(&state, &email).await;
        let workspace = create_workspace(&app, &token).await;
        let group = create_group(
            &app,
            &token,
            &workspace,
            json!({
                "free_speech":true, "proactive_mode":true,
                "scheduler_enabled":true, "max_agent_steps":budget,
            }),
        )
        .await;
        let body = if code == StatusCode::OK {
            text_body(reply)
        } else {
            reply.to_owned()
        };
        let mut responses = vec![(code, body)];
        if name == "mixed" {
            responses.push((StatusCode::BAD_REQUEST, "fixture provider error".into()));
        }
        let provider = seed_provider(
            &state,
            &owner,
            &fake_provider_status_sequence(responses).await,
        )
        .await;
        for index in 0..agents {
            seed_agent(
                &state,
                &owner,
                &group,
                &provider,
                &format!("Agent{index}"),
                if index == 0 {
                    "2024-01-01T00:00:00Z"
                } else {
                    "2024-01-02T00:00:00Z"
                },
            )
            .await;
        }
        let expected_steps: i64 = if name == "mixed" { 2 } else { 1 };
        sqlx::query("CREATE TABLE expected_budget(steps INTEGER NOT NULL)")
            .execute(state.db.pool())
            .await
            .unwrap();
        sqlx::query("INSERT INTO expected_budget(steps) VALUES(?)")
            .bind(expected_steps)
            .execute(state.db.pool())
            .await
            .unwrap();
        // These guards observe real commits, rather than inferring order from
        // final state after the request has already finished.
        for sql in [
            "CREATE TRIGGER terminal_requires_state BEFORE INSERT ON stream_events WHEN NEW.kind IN ('turn_completed','turn_budget_exhausted') AND COALESCE((SELECT status FROM group_turns WHERE id=json_extract(NEW.payload_json,'$.turn_id')),'missing') IN ('pending','running','missing') BEGIN SELECT RAISE(ABORT,'terminal state must commit first'); END",
            "CREATE TRIGGER state_requires_budget BEFORE UPDATE OF status ON group_turns WHEN OLD.status='running' AND NEW.status IN ('completed','silence','budget_exhausted','failed','waiting_for_user') AND NEW.agent_steps!=(SELECT steps FROM expected_budget) BEGIN SELECT RAISE(ABORT,'budget must commit first'); END",
        ] {
            sqlx::query(sql).execute(state.db.pool()).await.unwrap();
        }
        let events = stream_events(
            &app,
            &format!("/api/v2/groups/{group}/messages/stream"),
            &token,
            json!({"content":"hello"}),
        )
        .await;
        let event_kinds = kinds(&events);
        assert_eq!(
            &event_kinds[event_kinds.len() - tail.len()..],
            tail,
            "case={name}"
        );
        let turn: (String, String, Option<String>, i64) = sqlx::query_as(
            "SELECT id,status,termination_reason,agent_steps FROM group_turns WHERE group_id=?",
        )
        .bind(&group)
        .fetch_one(state.db.pool())
        .await
        .unwrap();
        assert_eq!(turn.1, status, "case={name}");
        assert_eq!(turn.2.as_deref(), reason, "case={name}");
        assert_eq!(turn.3, expected_steps, "case={name}");
        let terminal_count = events
            .iter()
            .filter(|event| {
                matches!(
                    event["kind"].as_str(),
                    Some(
                        "turn_completed"
                            | "turn_budget_exhausted"
                            | "turn_cancelled"
                            | "turn_superseded"
                    )
                )
            })
            .count();
        assert_eq!(
            terminal_count,
            usize::from(name != "waiting"),
            "case={name}"
        );
        assert_eq!(
            events
                .iter()
                .filter(|event| event["kind"] == "done")
                .count(),
            1,
            "case={name}"
        );
        let live_tail = &events[events.len() - tail.len()..];
        let stream_id = live_tail[0]["stream_id"].as_str().unwrap();
        let rows: Vec<(String, String, String, i64)> = sqlx::query_as(
            "SELECT kind,payload_json,event_id,seq FROM stream_events WHERE stream_id=? ORDER BY seq DESC LIMIT ?"
        ).bind(stream_id).bind(tail.len() as i64).fetch_all(state.db.pool()).await.unwrap();
        assert_eq!(rows.len(), tail.len());
        for (row, live) in rows.iter().rev().zip(live_tail) {
            assert_eq!(row.0, live["kind"].as_str().unwrap());
            assert_eq!(
                serde_json::from_str::<Value>(&row.1).unwrap(),
                live["payload"]
            );
            assert_eq!(row.2, live["event_id"].as_str().unwrap());
            assert_eq!(row.3, live["seq"].as_i64().unwrap());
        }
        assert_eq!(events.last().unwrap()["payload"]["turn_id"], turn.0);
        if name != "waiting" {
            let terminal = &events[events.len() - 2]["payload"];
            assert_eq!(terminal["status"], status);
            assert_eq!(terminal["reason"], json!(reason));
            assert_eq!(terminal["budget"]["agent_steps"], expected_steps);
        }
    }
}
