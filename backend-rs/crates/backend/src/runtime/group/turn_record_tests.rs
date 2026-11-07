use super::TurnData;
use serde_json::{json, Value};

const SCHEMA_2: &str = include_str!("fixtures/turn-record-schema-2.json");
// This tests opaque JSON retention, not acceptance by the Rig SDK or a provider.
const SCHEMA_3: &str = include_str!("fixtures/turn-record-schema-3.json");

fn read(value: &Value) -> TurnData {
    TurnData::from_content_json(Some(&value.to_string())).unwrap()
}

fn write(turn: &TurnData) -> Value {
    serde_json::from_str(&turn.to_content_json().unwrap()).unwrap()
}

fn empty_payload() -> Value {
    json!({
        "schema_version": 2, "response_segments": [], "reasoning": [],
        "tool_calls": [], "todos": [], "context_usage": null,
        "execution_id": null, "checkpoint_version": 0, "phase": null,
        "history_cursor": null, "assistant_turns": []
    })
}

#[test]
fn versioned_fixtures_preserve_the_entire_durable_record() {
    for raw in [SCHEMA_2, SCHEMA_3] {
        let expected: Value = serde_json::from_str(raw).unwrap();
        let turn = TurnData::from_content_json(Some(raw)).unwrap();
        assert_eq!(write(&turn), expected);
        assert_eq!(write(&read(&write(&turn))), expected);
    }
}

#[test]
fn legacy_records_without_a_version_and_schema_one_keep_their_defaults() {
    for version in [None, Some(json!(1)), Some(json!(2))] {
        let mut input = json!({"response_segments": ["legacy"]});
        if let Some(version) = version {
            input["schema_version"] = version;
        }
        let mut expected = empty_payload();
        expected["response_segments"] = json!(["legacy"]);
        assert_eq!(write(&read(&input)), expected);
    }
}

#[test]
fn absent_and_empty_records_keep_the_plain_text_null_column() {
    assert!(TurnData::from_content_json(None)
        .unwrap()
        .to_content_json()
        .is_none());
    for input in [
        json!({}),
        json!({"schema_version": 1}),
        json!({"schema_version": 3}),
    ] {
        assert!(read(&input).to_content_json().is_none());
    }
}

#[test]
fn checkpoint_only_records_preserve_counters_and_default_missing_fields() {
    let turn = read(&json!({"schema_version": 3, "driver_checkpoint": {"attempts": 7}}));
    let mut expected = empty_payload();
    expected["schema_version"] = json!(3);
    expected["driver_checkpoint"] = json!({
        "attempt_id": null, "phase": "prepared", "attempts": 7,
        "failed_attempts": 0, "completion_calls": 0,
        "input_tokens": 0, "output_tokens": 0, "total_tokens": 0,
        "attempt_input_tokens": 0, "attempt_output_tokens": 0, "attempt_total_tokens": 0
    });
    assert_eq!(write(&turn), expected);
}

#[test]
fn empty_display_segments_are_filtered_without_trimming_or_mutating_state() {
    let turn = read(&json!({
        "response_segments": ["", " ", "answer  ", ""],
        "reasoning": ["", "\n", " exact thinking  "]
    }));
    let mut expected = empty_payload();
    expected["response_segments"] = json!([" ", "answer  "]);
    expected["reasoning"] = json!(["\n", " exact thinking  "]);
    assert_eq!(write(&turn), expected);
    assert_eq!(turn.response_segments.len(), 4);
    assert_eq!(turn.reasoning.len(), 3);
}

#[test]
fn control_fields_alone_do_not_make_a_legacy_record_nonempty() {
    let turn = read(&json!({
        "execution_id": "legacy", "checkpoint_version": 7,
        "phase": "requesting", "history_cursor": 42,
        "assistant_turns": [{"role": "assistant", "content": "accepted"}]
    }));
    assert!(turn.to_content_json().is_none());
}

#[test]
fn effort_alone_is_persisted_without_adding_a_driver_checkpoint() {
    let mut expected = empty_payload();
    expected["effort_override"] = json!("max");
    assert_eq!(write(&read(&json!({"effort_override": "max"}))), expected);
}

#[test]
fn unsupported_versions_and_malformed_records_are_errors() {
    for version in [
        json!(0),
        json!(-1),
        json!(4),
        json!("2"),
        json!(null),
        json!(2.5),
    ] {
        let raw = json!({"schema_version": version, "response_segments": ["visible"]}).to_string();
        assert!(
            TurnData::from_content_json(Some(&raw)).is_err(),
            "version={version}"
        );
    }
    for raw in [
        "{",
        "null",
        "42",
        r#"{"tool_calls":false}"#,
        r#"{"driver_checkpoint":{"phase":"future_phase"}}"#,
        r#"{"assistant_turns":[{"role":"assistant"}]}"#,
        r#"{"checkpoint_version":-1}"#,
        r#"{"todos":null}"#,
        r#"{"execution_id":3}"#,
    ] {
        assert!(
            TurnData::from_content_json(Some(raw)).is_err(),
            "record={raw}"
        );
    }
}

#[test]
fn old_tool_records_default_missing_optional_fields_to_null() {
    let input = json!({"schema_version": 1, "tool_calls": [{
        "tool_call_id": "old-call", "tool_name": "Read", "status": "completed"
    }]});
    let mut expected = empty_payload();
    expected["tool_calls"] = json!([{
        "tool_call_id": "old-call", "tool_name": "Read", "status": "completed",
        "args_summary": null, "result_summary": null, "args": null, "result": null,
        "provider_metadata": null, "model_output": null, "approval_request": null
    }]);
    assert_eq!(write(&read(&input)), expected);
}

#[test]
fn typed_unknown_fields_are_ignored_while_opaque_payloads_are_preserved() {
    let mut expected: Value = serde_json::from_str(SCHEMA_3).unwrap();
    expected["assistant_turns"][0]["native_content"]["future_protocol"] =
        json!([null, {"signed": "raw  "}]);
    expected["tool_calls"][0]["provider_metadata"]["future"] = json!({"order": [3, 1, 2]});
    let mut input = expected.clone();
    input["future_envelope"] = json!({"unknown": true});
    input["tool_calls"][0]["future_record"] = json!("ignored");
    input["driver_checkpoint"]["future_counter"] = json!(77);
    input["assistant_turns"][0]["future_message"] = json!("ignored");
    assert_eq!(write(&read(&input)), expected);
}

#[test]
fn approval_round_trip_recovers_the_original_call_and_stops_after_a_result() {
    let fixture: Value = serde_json::from_str(SCHEMA_2).unwrap();
    let mut turn = read(&write(&read(&fixture)));
    let (call, request) = turn.pending_approval("approved-call").unwrap();
    assert_eq!(
        serde_json::to_value(call).unwrap(),
        fixture["assistant_turns"][0]["tool_calls"][0]
    );
    assert_eq!(
        serde_json::to_value(request).unwrap(),
        fixture["tool_calls"][0]["approval_request"]
    );
    assert!(turn.pending_approval("next-call").is_none());
    assert!(turn.pending_approval("unknown-call").is_none());
    turn.record_tool_output("approved-call", "settled result".into());
    let settled = read(&write(&turn));
    assert!(settled.pending_approval("approved-call").is_none());
    assert_eq!(
        write(&settled)["assistant_turns"],
        fixture["assistant_turns"]
    );
}

#[test]
fn non_approval_and_outcome_unknown_records_do_not_become_pending_approvals() {
    for status in [
        "pending",
        "started",
        "outcome_unknown",
        "completed",
        "failed",
    ] {
        let mut input: Value = serde_json::from_str(SCHEMA_2).unwrap();
        input["tool_calls"][0]["status"] = json!(status);
        let turn = read(&write(&read(&input)));
        assert!(
            turn.pending_approval("approved-call").is_none(),
            "status={status}"
        );
        assert_eq!(write(&turn)["tool_calls"][0]["status"], status);
    }
}

#[test]
fn tool_arguments_keep_null_scalar_array_and_numeric_identity() {
    for args in [
        json!(null),
        json!("raw arguments"),
        json!(["x", null]),
        json!(-0.0),
        serde_json::from_str::<Value>("8.971135692166839e-150").unwrap(),
    ] {
        let mut input: Value = serde_json::from_str(SCHEMA_2).unwrap();
        input["tool_calls"][0]["args"] = args.clone();
        input["assistant_turns"][0]["tool_calls"][0]["args"] = args.clone();
        let output = write(&read(&input));
        assert_eq!(output, input);
        assert_eq!(
            output["tool_calls"][0]["args"].to_string(),
            args.to_string()
        );
    }
}
