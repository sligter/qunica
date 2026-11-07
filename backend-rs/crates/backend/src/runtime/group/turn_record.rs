//! Durable agent-message JSON compatibility boundary.
//!
//! Note: conversion owns the persisted shape, while the parent owns execution
//! state and side effects. Keep defaults, opaque payloads and legacy write rules
//! compatible with docs/guide/realtime.md#native-tool-execution-checkpoints.

use super::TurnData;
use serde_json::{json, Value};

/// Schema version for the structured `content_json` payload persisted on agent
/// messages. Bump when the shape changes so readers can migrate old rows.
const CONTENT_JSON_SCHEMA_VERSION: i64 = 3;

impl TurnData {
    pub(super) fn from_content_json(content_json: Option<&str>) -> anyhow::Result<Self> {
        let Some(raw) = content_json else {
            return Ok(Self::default());
        };
        let value: Value = serde_json::from_str(raw)?;
        if let Some(version) = value.get("schema_version") {
            anyhow::ensure!(
                version
                    .as_i64()
                    .is_some_and(|version| (1..=CONTENT_JSON_SCHEMA_VERSION).contains(&version)),
                "unsupported agent checkpoint schema"
            );
        }
        Ok(serde_json::from_value(value)?)
    }

    /// Serialize to the versioned `content_json` schema, or `None` when empty so
    /// plain-text messages keep a `NULL` column.
    pub(super) fn to_content_json(&self) -> Option<String> {
        if self.is_empty() && self.effort_override.is_none() && self.driver_checkpoint.is_none() {
            return None;
        }
        let reasoning: Vec<&String> = self
            .reasoning
            .iter()
            .filter(|segment| !segment.is_empty())
            .collect();
        let response_segments: Vec<&String> = self
            .response_segments
            .iter()
            .filter(|segment| !segment.is_empty())
            .collect();
        let tool_calls: Vec<Value> = self
            .tool_calls
            .iter()
            .map(|call| {
                json!({
                    "tool_call_id": call.tool_call_id,
                    "tool_name": call.tool_name,
                    "status": call.status,
                    "args_summary": call.args_summary,
                    "result_summary": call.result_summary,
                    "args": call.args,
                    "result": call.result,
                    "approval_request": call.approval_request,
                    "provider_metadata": call.provider_metadata,
                    "model_output": call.model_output,
                })
            })
            .collect();
        let mut payload = json!({
            "schema_version": if self.driver_checkpoint.is_some() { CONTENT_JSON_SCHEMA_VERSION } else { 2 },
            "response_segments": response_segments,
            "reasoning": reasoning,
            "tool_calls": tool_calls,
            "todos": self.todos,
            "context_usage": self.context_usage,
            "execution_id": self.execution_id,
            "checkpoint_version": self.checkpoint_version,
            "phase": self.phase,
            "history_cursor": self.history_cursor,
            "assistant_turns": self.assistant_turns,
        });
        if let Some(checkpoint) = &self.driver_checkpoint {
            payload["driver_checkpoint"] = json!(checkpoint);
        }
        if let Some(effort) = self.effort_override {
            payload["effort_override"] = json!(effort);
        }
        serde_json::to_string(&payload).ok()
    }
}

#[cfg(test)]
#[path = "turn_record_tests.rs"]
mod tests;
