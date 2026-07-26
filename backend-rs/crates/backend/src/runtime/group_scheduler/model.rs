use ag_swarmer_domain::events::StreamEvent;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

use crate::runtime::sequence::NewMessage;

use super::state::{DispatchStatus, TurnStatus};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchedulerDispatch {
    pub target_agent_id: String,
    pub selection_reason: SelectionReason,
    pub action_kind: ActionKind,
    pub hop: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SchedulerDecision {
    Dispatch(SchedulerDispatch),
    RequestModerator,
    Finish {
        status: TurnStatus,
        reason: TurnReason,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SelectionReason {
    UserMention,
    AgentCall,
    AgentHandoff,
    AgentTextMention,
    DeterministicOrder,
    Moderator,
    ModeratorFallback,
}

impl SelectionReason {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::UserMention => "user_mention",
            Self::AgentCall => "agent_call",
            Self::AgentHandoff => "agent_handoff",
            Self::AgentTextMention => "agent_text_mention",
            Self::DeterministicOrder => "deterministic_order",
            Self::Moderator => "moderator",
            Self::ModeratorFallback => "moderator_fallback",
        }
    }
}

impl TryFrom<&str> for SelectionReason {
    type Error = SchedulerModelError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        match value {
            "user_mention" => Ok(Self::UserMention),
            "agent_call" => Ok(Self::AgentCall),
            "agent_handoff" => Ok(Self::AgentHandoff),
            "agent_text_mention" => Ok(Self::AgentTextMention),
            "deterministic_order" => Ok(Self::DeterministicOrder),
            "moderator" => Ok(Self::Moderator),
            "moderator_fallback" => Ok(Self::ModeratorFallback),
            other => Err(SchedulerModelError::UnknownSelectionReason(
                other.to_owned(),
            )),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TurnReason {
    WaitingForUser,
    BudgetExhausted,
    FailureBudgetExhausted,
    UserCancelled,
    Superseded,
    ServerRestart,
    PersistenceFailed,
    Silence,
}

impl TurnReason {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::WaitingForUser => "waiting_for_user",
            Self::BudgetExhausted => "budget_exhausted",
            Self::FailureBudgetExhausted => "failure_budget_exhausted",
            Self::UserCancelled => "user_cancelled",
            Self::Superseded => "superseded",
            Self::ServerRestart => "server_restart",
            Self::PersistenceFailed => "persistence_failed",
            Self::Silence => "silence",
        }
    }
}

impl TryFrom<&str> for TurnReason {
    type Error = SchedulerModelError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        match value {
            "waiting_for_user" => Ok(Self::WaitingForUser),
            "budget_exhausted" => Ok(Self::BudgetExhausted),
            "failure_budget_exhausted" => Ok(Self::FailureBudgetExhausted),
            "user_cancelled" => Ok(Self::UserCancelled),
            "superseded" => Ok(Self::Superseded),
            "server_restart" => Ok(Self::ServerRestart),
            "persistence_failed" => Ok(Self::PersistenceFailed),
            "silence" => Ok(Self::Silence),
            other => Err(SchedulerModelError::UnknownTurnReason(other.to_owned())),
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
    #[error("unknown turn termination reason: {0}")]
    UnknownTurnReason(String),
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
    pub termination_reason: Option<TurnReason>,
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
    use super::{ActionKind, SelectionReason, TurnReason};

    #[test]
    fn scheduler_model_database_enums_round_trip_and_reject_unknown_values() {
        for (reason, persisted) in [
            (SelectionReason::UserMention, "user_mention"),
            (SelectionReason::AgentCall, "agent_call"),
            (SelectionReason::AgentHandoff, "agent_handoff"),
            (SelectionReason::AgentTextMention, "agent_text_mention"),
            (SelectionReason::DeterministicOrder, "deterministic_order"),
            (SelectionReason::Moderator, "moderator"),
            (SelectionReason::ModeratorFallback, "moderator_fallback"),
        ] {
            assert_eq!(reason.as_str(), persisted);
            assert_eq!(SelectionReason::try_from(persisted).unwrap(), reason);
        }
        for (reason, persisted) in [
            (TurnReason::WaitingForUser, "waiting_for_user"),
            (TurnReason::BudgetExhausted, "budget_exhausted"),
            (
                TurnReason::FailureBudgetExhausted,
                "failure_budget_exhausted",
            ),
            (TurnReason::UserCancelled, "user_cancelled"),
            (TurnReason::Superseded, "superseded"),
            (TurnReason::ServerRestart, "server_restart"),
            (TurnReason::PersistenceFailed, "persistence_failed"),
            (TurnReason::Silence, "silence"),
        ] {
            assert_eq!(reason.as_str(), persisted);
            assert_eq!(TurnReason::try_from(persisted).unwrap(), reason);
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
        assert!(SelectionReason::try_from("structured_action").is_err());
        assert!(SelectionReason::try_from("agent_mention").is_err());
        assert!(SelectionReason::try_from("deterministic").is_err());
        assert!(TurnReason::try_from("no_candidates").is_err());
        assert!(TurnReason::try_from("new_user_message").is_err());
        assert!(ActionKind::try_from("message").is_err());
    }
}
