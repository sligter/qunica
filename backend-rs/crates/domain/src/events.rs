use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StreamEventKind {
    UserMessage,
    ConversationUpdated,
    AgentStart,
    Token,
    Reasoning,
    ToolCallStart,
    ToolCallResult,
    AgentMessage,
    AgentSilent,
    WaitingForUser,
    /// A tool call is paused for a human to approve or decline it. The payload
    /// carries the `approval_request` the client renders and the `tool_call_id`
    /// the answer must name, so the paused call can be replayed exactly.
    ApprovalRequired,
    /// The agent rewrote its checklist. The payload carries the complete list
    /// as of now, not a delta: `TodoWrite` replaces rather than appends, so a
    /// client that missed an event still catches up on the next one.
    TodoUpdate,
    ContextUsage,
    AcpAgentRun,
    Silence,
    TurnStarted,
    ModeratorStarted,
    SpeakerSelected,
    DispatchFailed,
    ModeratorFallback,
    TurnCancelled,
    TurnSuperseded,
    TurnBudgetExhausted,
    TurnCompleted,
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
