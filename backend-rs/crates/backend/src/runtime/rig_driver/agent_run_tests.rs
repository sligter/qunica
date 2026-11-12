use super::*;
use crate::runtime::rig_driver::TurnEngine;
use serde_json::json;

fn engine(config: Value) -> TurnEngine {
    let provider = Arc::new(
        RigProvider::openai_compatible("http://127.0.0.1:9/v1", "unused-test-key").unwrap(),
    );
    TurnEngine::Rig(Box::new(RigDriver::new(provider, &config).unwrap()))
}

fn request() -> ChatRequest {
    ChatRequest {
        model: "test".to_owned(),
        messages: vec![ChatMessage::text("user", "continue")],
        temperature: None,
        top_p: None,
        reasoning_passback: false,
        include_empty_tools: false,
        tools: vec![],
        reasoning_effort: None,
    }
}

#[test]
fn legacy_checkpoint_keeps_cumulative_usage_and_starts_at_zero_window() {
    let checkpoint: DriverCheckpoint = serde_json::from_value(json!({
        "phase":"aborted", "attempts":7, "failed_attempts":4, "total_tokens":30
    }))
    .unwrap();
    assert_eq!(
        (
            checkpoint.attempts,
            checkpoint.failed_attempts,
            checkpoint.total_tokens
        ),
        (7, 4, 30)
    );
    assert_eq!(
        (
            checkpoint.window_attempts,
            checkpoint.window_failed_attempts,
            checkpoint.window_total_tokens
        ),
        (0, 0, 0)
    );
    let saved = serde_json::to_value(checkpoint).unwrap();
    for key in [
        "window_attempts",
        "window_failed_attempts",
        "window_total_tokens",
    ] {
        assert!(saved.get(key).is_none(), "legacy record gained {key}");
    }
}

#[tokio::test]
async fn restored_budgets_stay_exhausted_until_an_explicit_resume() {
    for (limits, reason) in [
        (json!({"max_model_calls":2}), "model-call"),
        (json!({"max_failed_attempts":3}), "failure"),
        (json!({"max_total_tokens":20}), "token"),
    ] {
        for phase in [AttemptPhase::Aborted, AttemptPhase::ToolsPending] {
            let checkpoint = DriverCheckpoint {
                phase,
                attempts: 7,
                failed_attempts: 4,
                total_tokens: 30,
                window_attempts: 5,
                window_failed_attempts: 1,
                window_total_tokens: 10,
                ..Default::default()
            };
            // Exercise persisted recovery, not only an in-memory assignment.
            let saved = serde_json::to_value(&checkpoint).unwrap();
            let restored: DriverCheckpoint = serde_json::from_value(saved.clone()).unwrap();
            let mut engine = engine(json!({"rig_limits":limits}));
            engine.restore(Some(&restored), false).unwrap();
            let error = engine.prepare(&request()).unwrap_err();
            assert_eq!(
                error.downcast_ref::<DriverBudgetExhausted>().unwrap().0,
                reason
            );
            assert_eq!(
                serde_json::to_value(engine.checkpoint().unwrap()).unwrap(),
                saved
            );

            engine.restore(Some(&restored), true).unwrap();
            engine.prepare(&request()).unwrap();
            let next = engine.checkpoint().unwrap();
            assert_eq!(
                (next.attempts, next.failed_attempts, next.total_tokens),
                (8, 4, 30)
            );
            assert_eq!(
                (
                    next.window_attempts,
                    next.window_failed_attempts,
                    next.window_total_tokens
                ),
                (7, 4, 30)
            );
        }
    }
}

#[tokio::test]
async fn an_unconfigured_model_call_cap_does_not_stop_a_long_tool_loop() {
    let mut engine = engine(json!({}));
    let checkpoint = DriverCheckpoint {
        phase: AttemptPhase::Aborted,
        attempts: 128,
        ..Default::default()
    };
    engine.restore(Some(&checkpoint), false).unwrap();
    engine.prepare(&request()).unwrap();
    assert_eq!(engine.checkpoint().unwrap().attempts, 129);
}
