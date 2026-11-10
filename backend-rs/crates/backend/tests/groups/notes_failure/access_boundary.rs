use super::*;

#[tokio::test]
async fn note_access_decodes_the_full_group_before_workspace_checks_or_writes() {
    for blocked_workspace in [false, true] {
        let fixture = Fixture::new(true).await;
        let note = fixture.create().await;
        let id = note["id"].as_str().unwrap();
        let original_budget: Option<i64> =
            sqlx::query_scalar("SELECT max_agent_steps FROM groups WHERE id=?")
                .bind(&fixture.group)
                .fetch_one(fixture.state.db.pool())
                .await
                .unwrap();
        // SQLite affinity/checks allow this TEXT value. The existing full
        // GroupRow decoder rejects it even though Notes do not use the budget.
        sqlx::query("UPDATE groups SET max_agent_steps='unreadable budget' WHERE id=?")
            .bind(&fixture.group)
            .execute(fixture.state.db.pool())
            .await
            .unwrap();
        let stored_type: String =
            sqlx::query_scalar("SELECT typeof(max_agent_steps) FROM groups WHERE id=?")
                .bind(&fixture.group)
                .fetch_one(fixture.state.db.pool())
                .await
                .unwrap();
        assert_eq!(stored_type, "text");
        if blocked_workspace {
            sqlx::query("UPDATE workspaces SET status='deleted' WHERE id=(SELECT workspace_id FROM groups WHERE id=?)")
                .bind(&fixture.group).execute(fixture.state.db.pool()).await.unwrap();
        }
        let before = fixture.snapshot().await;
        let index = fixture.index();
        let assert_unreadable = |(status, body): (StatusCode, Value)| {
            assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "{body}");
            assert_eq!(body["error"]["code"], "internal_error");
            assert_eq!(body["error"]["message"], "database error");
        };
        assert_unreadable(
            send(
                &fixture.app,
                authed(
                    "GET",
                    &format!("/api/v2/groups/{}/notes", fixture.group),
                    &fixture.token,
                ),
            )
            .await,
        );
        assert_unreadable(fixture.get(id).await);
        for (method, target) in [("POST", None), ("PATCH", Some(id)), ("DELETE", Some(id))] {
            assert_unreadable(
                fixture
                    .mutate(
                        method,
                        target,
                        json!({"title":"Rejected", "content":"after"}),
                    )
                    .await,
            );
        }
        assert_eq!(fixture.snapshot().await, before);
        assert_eq!(fixture.index(), index);
        assert_eq!(
            fs::read_to_string(fixture.note_path(id)).unwrap(),
            "db before"
        );
        assert_eq!(
            fs::read_dir(fixture.root.path().join("Notes"))
                .unwrap()
                .count(),
            2
        );

        sqlx::query("UPDATE groups SET max_agent_steps=? WHERE id=?")
            .bind(original_budget)
            .bind(&fixture.group)
            .execute(fixture.state.db.pool())
            .await
            .unwrap();
        let (status, body) = fixture.get(id).await;
        if blocked_workspace {
            assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
            assert_eq!(body["error"]["message"], "group workspace is not active");
        } else {
            assert_eq!(status, StatusCode::OK, "{body}");
            assert_eq!(body["content"], "db before");
        }
        fixture.state.db.pool().close().await;
    }
}
