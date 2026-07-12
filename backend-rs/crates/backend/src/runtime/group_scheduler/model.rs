use ag_swarmer_domain::events::StreamEvent;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

use crate::runtime::sequence::NewMessage;

use super::state::{DispatchStatus, TurnStatus};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SelectionReason {
    UserMention,
    StructuredAction,
    AgentMention,
    Deterministic,
    Moderator,
}

impl SelectionReason {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::UserMention => "user_mention",
            Self::StructuredAction => "structured_action",
            Self::AgentMention => "agent_mention",
            Self::Deterministic => "deterministic",
            Self::Moderator => "moderator",
        }
    }
}

impl TryFrom<&str> for SelectionReason {
    type Error = SchedulerModelError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        match value {
            "user_mention" => Ok(Self::UserMention),
            "structured_action" => Ok(Self::StructuredAction),
            "agent_mention" => Ok(Self::AgentMention),
            "deterministic" => Ok(Self::Deterministic),
            "moderator" => Ok(Self::Moderator),
            other => Err(SchedulerModelError::UnknownSelectionReason(
                other.to_owned(),
            )),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActionKind {
    Speak,
    Call,
    Handoff,
    Wait,
    Silent,
}

impl ActionKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Speak => "speak",
            Self::Call => "call",
            Self::Handoff => "handoff",
            Self::Wait => "wait",
            Self::Silent => "silent",
        }
    }
}

impl TryFrom<&str> for ActionKind {
    type Error = SchedulerModelError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        match value {
            "speak" => Ok(Self::Speak),
            "call" => Ok(Self::Call),
            "handoff" => Ok(Self::Handoff),
            "wait" => Ok(Self::Wait),
            "silent" => Ok(Self::Silent),
            other => Err(SchedulerModelError::UnknownActionKind(other.to_owned())),
        }
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum SchedulerModelError {
    #[error("unknown dispatch selection reason: {0}")]
    UnknownSelectionReason(String),
    #[error("unknown scheduler action kind: {0}")]
    UnknownActionKind(String),
}

pub struct NewTurn {
    pub id: String,
    pub thread_id: String,
    pub group_id: String,
    pub trigger_message_id: Option<String>,
    pub scheduler_strategy: String,
    pub config_snapshot: Value,
    pub topology_snapshot: Value,
}

pub struct NewDispatch {
    pub id: String,
    pub turn_id: String,
    pub parent_dispatch_id: Option<String>,
    pub source_agent_id: Option<String>,
    pub target_agent_id: String,
    pub selection_reason: SelectionReason,
    pub action_kind: ActionKind,
    pub hop: i64,
    pub input_message_id: Option<String>,
}

pub struct DispatchOutput {
    pub thread_id: String,
    pub group_id: String,
    pub message: NewMessage,
    pub event: StreamEvent<Value>,
}

pub struct FinishDispatch {
    pub dispatch_id: String,
    pub next: DispatchStatus,
    pub artifact: Option<Value>,
    pub total_tokens: i64,
    pub failure_code: Option<String>,
    pub output: Option<DispatchOutput>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TurnSnapshot {
    pub id: String,
    pub thread_id: String,
    pub group_id: String,
    pub trigger_message_id: Option<String>,
    pub status: TurnStatus,
    pub scheduler_strategy: String,
    pub config_snapshot: Value,
    pub topology_snapshot: Value,
    pub agent_steps: i64,
    pub moderator_calls: i64,
    pub consecutive_failures: i64,
    pub total_failures: i64,
    pub total_tokens: i64,
    pub termination_reason: Option<String>,
    pub created_at: String,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
    pub updated_at: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DispatchSnapshot {
    pub id: String,
    pub turn_id: String,
    pub parent_dispatch_id: Option<String>,
    pub source_agent_id: Option<String>,
    pub target_agent_id: String,
    pub selection_reason: SelectionReason,
    pub action_kind: ActionKind,
    pub hop: i64,
    pub status: DispatchStatus,
    pub input_message_id: Option<String>,
    pub output_message_id: Option<String>,
    pub artifact: Option<Value>,
    pub total_tokens: i64,
    pub failure_code: Option<String>,
    pub created_at: String,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
    pub updated_at: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TurnTrace {
    pub turn: TurnSnapshot,
    pub dispatches: Vec<DispatchSnapshot>,
}

#[cfg(test)]
mod tests {
    use super::{ActionKind, SelectionReason};

    #[test]
    fn dispatch_database_enums_round_trip_and_reject_unknown_values() {
        for reason in [
            SelectionReason::UserMention,
            SelectionReason::StructuredAction,
            SelectionReason::AgentMention,
            SelectionReason::Deterministic,
            SelectionReason::Moderator,
        ] {
            assert_eq!(SelectionReason::try_from(reason.as_str()).unwrap(), reason);
        }
        for action in [
            ActionKind::Speak,
            ActionKind::Call,
            ActionKind::Handoff,
            ActionKind::Wait,
            ActionKind::Silent,
        ] {
            assert_eq!(ActionKind::try_from(action.as_str()).unwrap(), action);
        }
        assert!(SelectionReason::try_from("fallback").is_err());
        assert!(ActionKind::try_from("message").is_err());
    }
}
