use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StreamEventKind {
    UserMessage,
    AgentStart,
    Token,
    Reasoning,
    ToolCallStart,
    ToolCallResult,
    AgentMessage,
    AgentSilent,
    WaitingForUser,
    ContextUsage,
    AcpAgentRun,
    Silence,
    Warning,
    Error,
    Done,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StreamEvent<TPayload> {
    pub stream_id: Uuid,
    pub seq: i64,
    pub event_id: String,
    pub kind: StreamEventKind,
    pub payload: TPayload,
}

impl<TPayload> StreamEvent<TPayload> {
    pub fn new(stream_id: Uuid, seq: i64, kind: StreamEventKind, payload: TPayload) -> Self {
        let event_id = format!("{stream_id}:{seq}");
        Self {
            stream_id,
            seq,
            event_id,
            kind,
            payload,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn event_id_is_stable_stream_id_and_sequence() {
        let stream_id = Uuid::nil();
        let event = StreamEvent::new(stream_id, 42, StreamEventKind::Done, json!({}));
        assert_eq!(event.event_id, "00000000-0000-0000-0000-000000000000:42");
    }
}
