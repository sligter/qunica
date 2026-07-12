use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TurnStatus {
    Pending,
    Running,
    WaitingForUser,
    Completed,
    Silence,
    BudgetExhausted,
    FailureBudgetExhausted,
    Cancelled,
    Superseded,
    Failed,
}

impl TurnStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Running => "running",
            Self::WaitingForUser => "waiting_for_user",
            Self::Completed => "completed",
            Self::Silence => "silence",
            Self::BudgetExhausted => "budget_exhausted",
            Self::FailureBudgetExhausted => "failure_budget_exhausted",
            Self::Cancelled => "cancelled",
            Self::Superseded => "superseded",
            Self::Failed => "failed",
        }
    }
}

impl TryFrom<&str> for TurnStatus {
    type Error = SchedulerStateError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        match value {
            "pending" => Ok(Self::Pending),
            "running" => Ok(Self::Running),
            "waiting_for_user" => Ok(Self::WaitingForUser),
            "completed" => Ok(Self::Completed),
            "silence" => Ok(Self::Silence),
            "budget_exhausted" => Ok(Self::BudgetExhausted),
            "failure_budget_exhausted" => Ok(Self::FailureBudgetExhausted),
            "cancelled" => Ok(Self::Cancelled),
            "superseded" => Ok(Self::Superseded),
            "failed" => Ok(Self::Failed),
            other => Err(SchedulerStateError::UnknownTurnStatus(other.to_owned())),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DispatchStatus {
    Queued,
    Running,
    Completed,
    Silent,
    WaitingForUser,
    Interrupted,
    Cancelled,
    Failed,
}

impl DispatchStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Running => "running",
            Self::Completed => "completed",
            Self::Silent => "silent",
            Self::WaitingForUser => "waiting_for_user",
            Self::Interrupted => "interrupted",
            Self::Cancelled => "cancelled",
            Self::Failed => "failed",
        }
    }
}

impl TryFrom<&str> for DispatchStatus {
    type Error = SchedulerStateError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        match value {
            "queued" => Ok(Self::Queued),
            "running" => Ok(Self::Running),
            "completed" => Ok(Self::Completed),
            "silent" => Ok(Self::Silent),
            "waiting_for_user" => Ok(Self::WaitingForUser),
            "interrupted" => Ok(Self::Interrupted),
            "cancelled" => Ok(Self::Cancelled),
            "failed" => Ok(Self::Failed),
            other => Err(SchedulerStateError::UnknownDispatchStatus(other.to_owned())),
        }
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum SchedulerStateError {
    #[error("unknown turn status: {0}")]
    UnknownTurnStatus(String),
    #[error("unknown dispatch status: {0}")]
    UnknownDispatchStatus(String),
    #[error("invalid turn status transition from {from} to {to}")]
    InvalidTurnTransition {
        from: &'static str,
        to: &'static str,
    },
    #[error("invalid dispatch status transition from {from} to {to}")]
    InvalidDispatchTransition {
        from: &'static str,
        to: &'static str,
    },
}

pub fn validate_turn_transition(
    from: TurnStatus,
    to: TurnStatus,
) -> Result<(), SchedulerStateError> {
    let allowed = matches!(
        (from, to),
        (TurnStatus::Pending, TurnStatus::Running)
            | (TurnStatus::Pending, TurnStatus::Cancelled)
            | (TurnStatus::Pending, TurnStatus::Superseded)
            | (TurnStatus::Pending, TurnStatus::Failed)
            | (TurnStatus::Running, TurnStatus::WaitingForUser)
            | (TurnStatus::Running, TurnStatus::Completed)
            | (TurnStatus::Running, TurnStatus::Silence)
            | (TurnStatus::Running, TurnStatus::BudgetExhausted)
            | (TurnStatus::Running, TurnStatus::FailureBudgetExhausted)
            | (TurnStatus::Running, TurnStatus::Cancelled)
            | (TurnStatus::Running, TurnStatus::Superseded)
            | (TurnStatus::Running, TurnStatus::Failed)
            | (TurnStatus::WaitingForUser, TurnStatus::Running)
            | (TurnStatus::WaitingForUser, TurnStatus::Cancelled)
            | (TurnStatus::WaitingForUser, TurnStatus::Superseded)
            | (TurnStatus::WaitingForUser, TurnStatus::Failed)
    );

    if allowed {
        Ok(())
    } else {
        Err(SchedulerStateError::InvalidTurnTransition {
            from: from.as_str(),
            to: to.as_str(),
        })
    }
}

pub fn validate_dispatch_transition(
    from: DispatchStatus,
    to: DispatchStatus,
) -> Result<(), SchedulerStateError> {
    let allowed = matches!(
        (from, to),
        (DispatchStatus::Queued, DispatchStatus::Running)
            | (DispatchStatus::Queued, DispatchStatus::Cancelled)
            | (DispatchStatus::Queued, DispatchStatus::Failed)
            | (DispatchStatus::Running, DispatchStatus::Completed)
            | (DispatchStatus::Running, DispatchStatus::Silent)
            | (DispatchStatus::Running, DispatchStatus::WaitingForUser)
            | (DispatchStatus::Running, DispatchStatus::Interrupted)
            | (DispatchStatus::Running, DispatchStatus::Cancelled)
            | (DispatchStatus::Running, DispatchStatus::Failed)
    );

    if allowed {
        Ok(())
    } else {
        Err(SchedulerStateError::InvalidDispatchTransition {
            from: from.as_str(),
            to: to.as_str(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{
        validate_dispatch_transition, validate_turn_transition, DispatchStatus, TurnStatus,
    };

    #[test]
    fn turn_transitions_allow_running_lifecycle_outcomes() {
        for next in [
            TurnStatus::WaitingForUser,
            TurnStatus::Completed,
            TurnStatus::Silence,
            TurnStatus::BudgetExhausted,
            TurnStatus::FailureBudgetExhausted,
            TurnStatus::Cancelled,
            TurnStatus::Superseded,
            TurnStatus::Failed,
        ] {
            assert!(validate_turn_transition(TurnStatus::Running, next).is_ok());
        }
    }

    #[test]
    fn turn_transitions_reject_terminal_to_running() {
        for terminal in [
            TurnStatus::Completed,
            TurnStatus::Silence,
            TurnStatus::BudgetExhausted,
            TurnStatus::FailureBudgetExhausted,
            TurnStatus::Cancelled,
            TurnStatus::Superseded,
            TurnStatus::Failed,
        ] {
            assert!(validate_turn_transition(terminal, TurnStatus::Running).is_err());
        }
    }

    #[test]
    fn dispatch_transitions_allow_queue_cancellation_and_running_outcomes() {
        assert!(
            validate_dispatch_transition(DispatchStatus::Queued, DispatchStatus::Cancelled,)
                .is_ok()
        );

        for next in [
            DispatchStatus::Completed,
            DispatchStatus::Silent,
            DispatchStatus::WaitingForUser,
            DispatchStatus::Interrupted,
            DispatchStatus::Cancelled,
            DispatchStatus::Failed,
        ] {
            assert!(validate_dispatch_transition(DispatchStatus::Running, next).is_ok());
        }
    }

    #[test]
    fn dispatch_transitions_reject_terminal_to_running() {
        for terminal in [
            DispatchStatus::Completed,
            DispatchStatus::Silent,
            DispatchStatus::WaitingForUser,
            DispatchStatus::Interrupted,
            DispatchStatus::Cancelled,
            DispatchStatus::Failed,
        ] {
            assert!(validate_dispatch_transition(terminal, DispatchStatus::Running).is_err());
        }
    }

    #[test]
    fn status_database_strings_round_trip_and_reject_unknown_values() {
        for status in [
            TurnStatus::Pending,
            TurnStatus::Running,
            TurnStatus::WaitingForUser,
            TurnStatus::Completed,
            TurnStatus::Silence,
            TurnStatus::BudgetExhausted,
            TurnStatus::FailureBudgetExhausted,
            TurnStatus::Cancelled,
            TurnStatus::Superseded,
            TurnStatus::Failed,
        ] {
            assert_eq!(TurnStatus::try_from(status.as_str()).unwrap(), status);
        }
        for status in [
            DispatchStatus::Queued,
            DispatchStatus::Running,
            DispatchStatus::Completed,
            DispatchStatus::Silent,
            DispatchStatus::WaitingForUser,
            DispatchStatus::Interrupted,
            DispatchStatus::Cancelled,
            DispatchStatus::Failed,
        ] {
            assert_eq!(DispatchStatus::try_from(status.as_str()).unwrap(), status);
        }
        assert!(TurnStatus::try_from("RUNNING").is_err());
        assert!(DispatchStatus::try_from("done").is_err());
    }
}
