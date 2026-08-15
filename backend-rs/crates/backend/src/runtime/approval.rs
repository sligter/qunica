//! Pausing a turn for a human decision, and replaying the call once it arrives.
//!
//! The shell policy can ask rather than only refuse ([`crate::tools::shell::policy`]),
//! and this module is what makes the question answerable. The flow is the same
//! shape as `AskUser`'s pause, with one addition that matters: the paused call is
//! persisted **without a result**, so resuming can run the exact call the model
//! made instead of re-prompting the model and hoping it asks for the same thing
//! again.
//!
//! ```text
//!   tool returns ApprovalRequired
//!     -> runtime records the call (args, no result) on the interrupted message
//!     -> emits `approval_required`, pauses the thread
//!   user answers  POST /threads/{id}/resume { approval: {...} }
//!     -> grants are loaded, the pending call is found by id
//!     -> approved: the call is executed with the grant in place
//!        declined: a refusal is synthesised, carrying the user's note
//!     -> assistant(tool_calls) + tool result are appended and the loop resumes
//! ```
//!
//! Executing the *recorded* call rather than asking the model again is the whole
//! point. A replayed turn that re-prompts can produce a different command than
//! the one the user saw and approved, which would make the approval card a
//! decoration rather than a control.

use sqlx::SqlitePool;
use time::{format_description::well_known::Rfc3339, OffsetDateTime};
use uuid::Uuid;

use crate::tools::{ApprovalDecision, ApprovalGrants, ApprovalRequest};

/// Load the rules this thread has remembered approvals for.
///
/// Only `remembered AND approved` rows count. A one-time approval authorised one
/// call and nothing else, and a decline is recorded for the audit trail without
/// suppressing the next question.
pub async fn load_grants(pool: &SqlitePool, thread_id: &str) -> anyhow::Result<ApprovalGrants> {
    let rules: Vec<String> = sqlx::query_scalar(
        "SELECT DISTINCT rule FROM tool_approvals \
         WHERE thread_id = ? AND remembered = 1 AND approved = 1",
    )
    .bind(thread_id)
    .fetch_all(pool)
    .await?;
    Ok(ApprovalGrants::new(rules))
}

/// Record one answer, whatever it was.
pub async fn record_decision(
    pool: &SqlitePool,
    thread_id: &str,
    agent_id: &str,
    request: &ApprovalRequest,
    decision: &ApprovalDecision,
) -> anyhow::Result<()> {
    sqlx::query(
        "INSERT INTO tool_approvals \
            (id, thread_id, agent_id, tool_name, rule, tool_call_id, subject, approved, \
             remembered, note, created_at) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(Uuid::new_v4().to_string())
    .bind(thread_id)
    .bind(agent_id)
    .bind(&request.tool_name)
    .bind(&request.rule)
    .bind(&decision.tool_call_id)
    .bind(&request.subject)
    .bind(i64::from(decision.approved))
    // A decline is never remembered: refusing one command must not silently
    // refuse every later one the same rule covers.
    .bind(i64::from(decision.remember && decision.approved))
    .bind(decision.note.as_deref())
    .bind(
        OffsetDateTime::now_utc()
            .format(&Rfc3339)
            .unwrap_or_default(),
    )
    .execute(pool)
    .await?;
    Ok(())
}

/// The tool result a declined call reports back to the model.
///
/// Phrased as a decision already made, not an error to retry: an agent told
/// "failed" will try the same command again, while an agent told the user
/// declined will look for another way. The user's note is the most useful part
/// when there is one, so it leads.
pub fn declined_result(request: &ApprovalRequest, note: Option<&str>) -> String {
    let mut text = String::from("The user declined to approve this command; it was not run.");
    if let Some(note) = note.map(str::trim).filter(|note| !note.is_empty()) {
        text.push_str("\nThe user said: ");
        text.push_str(note);
    }
    text.push_str(&format!(
        "\nDeclined command: {}\nDo not run it again, and do not try to work around the \
         restriction. Continue with another approach, or explain what you need and why.",
        request.subject
    ));
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request() -> ApprovalRequest {
        ApprovalRequest {
            rule: "delete-files".to_string(),
            capability: "delete files in this workspace".to_string(),
            reason: "it deletes files".to_string(),
            tool_name: "Pwsh".to_string(),
            subject: "rm -rf build".to_string(),
        }
    }

    #[test]
    fn a_decline_tells_the_model_it_was_a_decision_not_a_failure() {
        let text = declined_result(&request(), None);
        assert!(text.contains("declined"));
        assert!(text.contains("rm -rf build"));
        assert!(
            text.contains("Do not run it again"),
            "a retry loop is the failure mode this text exists to prevent: {text}"
        );
    }

    #[test]
    fn a_user_note_leads_the_decline() {
        let text = declined_result(&request(), Some("  keep the build, I need the artifacts  "));
        assert!(text.contains("The user said: keep the build, I need the artifacts"));
        assert!(
            !text.contains("  keep"),
            "the note should be trimmed: {text}"
        );
    }

    #[test]
    fn an_empty_note_is_omitted_rather_than_rendered_blank() {
        let text = declined_result(&request(), Some("   "));
        assert!(!text.contains("The user said"), "{text}");
    }
}
