//! External topics: the issue, pull request or caller-chosen key a trigger
//! event belongs to. Events of one topic share a thread instead of each
//! opening a new one, and dispatched tasks subscribed to a topic receive its
//! events directly; see `specs/thread-subscriptions.md`.

use serde_json::Value;
use sqlx::{Sqlite, Transaction};

const MAX_TOPIC_CHARS: usize = 200;
/// Topics one dispatched task may follow.
pub const MAX_TASK_TOPICS: usize = 8;

/// A usable topic key, or `None` when it is empty, too long or has control
/// characters.
pub fn normalize(raw: &str) -> Option<String> {
    let topic = raw.trim();
    (!topic.is_empty()
        && topic.chars().count() <= MAX_TOPIC_CHARS
        && !topic.chars().any(char::is_control))
    .then(|| topic.to_string())
}

/// The issue or pull request a GitHub delivery is about.
///
/// Note: issues and pull requests share one number space per repository, so
/// a comment on a pull request (an `issue_comment` naming the issue number)
/// and its review or CI events land on the same `github:owner/repo#N` topic.
pub fn github(event: &str, payload: &Value) -> Option<String> {
    let repo = payload
        .pointer("/repository/full_name")?
        .as_str()?
        .to_ascii_lowercase();
    let number = |pointer: &str| payload.pointer(pointer).and_then(Value::as_u64);
    let topic = if event.starts_with("discussion") {
        format!(
            "github:{repo}/discussions/{}",
            number("/discussion/number")?
        )
    } else {
        let item = number("/pull_request/number")
            .or_else(|| number("/issue/number"))
            .or_else(|| number("/check_suite/pull_requests/0/number"))
            .or_else(|| number("/check_run/pull_requests/0/number"))
            .or_else(|| number("/workflow_run/pull_requests/0/number"))?;
        format!("github:{repo}#{item}")
    };
    normalize(&topic)
}

pub(crate) async fn subscribe_in_tx(
    tx: &mut Transaction<'_, Sqlite>,
    group: &str,
    thread: &str,
    topics: &[String],
    now: &str,
) -> Result<(), sqlx::Error> {
    for topic in topics {
        sqlx::query(
            "INSERT OR IGNORE INTO thread_topics(thread_id,group_id,topic,created_at) VALUES(?,?,?,?)",
        )
        .bind(thread)
        .bind(group)
        .bind(topic)
        .bind(now)
        .execute(&mut **tx)
        .await?;
    }
    Ok(())
}

pub(crate) async fn of_thread<'e, E>(executor: E, thread: &str) -> Result<Vec<String>, sqlx::Error>
where
    E: sqlx::Executor<'e, Database = Sqlite>,
{
    sqlx::query_scalar(
        "SELECT topic FROM thread_topics WHERE thread_id=? ORDER BY created_at,topic",
    )
    .bind(thread)
    .fetch_all(executor)
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn github_events_of_one_pull_request_share_a_topic() {
        let repo = json!({"full_name": "Owner/Repo"});
        let cases = [
            (
                "pull_request",
                json!({"repository": repo, "number": 7, "pull_request": {"number": 7}}),
            ),
            (
                "issue_comment",
                json!({"repository": repo, "issue": {"number": 7}}),
            ),
            (
                "pull_request_review",
                json!({"repository": repo, "pull_request": {"number": 7}}),
            ),
            (
                "check_suite",
                json!({"repository": repo, "check_suite": {"pull_requests": [{"number": 7}]}}),
            ),
            (
                "workflow_run",
                json!({"repository": repo, "workflow_run": {"pull_requests": [{"number": 7}]}}),
            ),
        ];
        for (event, payload) in cases {
            assert_eq!(
                github(event, &payload).as_deref(),
                Some("github:owner/repo#7"),
                "{event}"
            );
        }
        assert_eq!(
            github(
                "discussion_comment",
                &json!({"repository": repo, "discussion": {"number": 3}})
            )
            .as_deref(),
            Some("github:owner/repo/discussions/3")
        );
        assert_eq!(
            github(
                "push",
                &json!({"repository": repo, "ref": "refs/heads/main"})
            ),
            None
        );
        assert_eq!(
            github(
                "check_suite",
                &json!({"repository": repo, "check_suite": {"pull_requests": []}})
            ),
            None
        );
        assert_eq!(github("issues", &json!({"issue": {"number": 1}})), None);
    }

    #[test]
    fn topics_are_trimmed_and_bounded() {
        assert_eq!(normalize("  ticket-9 ").as_deref(), Some("ticket-9"));
        assert_eq!(normalize(" "), None);
        assert_eq!(normalize("a\nb"), None);
        assert_eq!(normalize(&"x".repeat(201)), None);
        assert!(normalize(&"话".repeat(200)).is_some());
    }
}
