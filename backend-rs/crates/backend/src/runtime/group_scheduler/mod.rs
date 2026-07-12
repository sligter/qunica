pub mod budget;
pub mod model;
pub mod state;
pub mod store;
pub mod topology;

pub use model::{
    ActionKind, DispatchOutput, DispatchSnapshot, FinishDispatch, NewDispatch, NewTurn,
    SchedulerAction, SchedulerCandidate, SchedulerDecision, SchedulerDispatch, SchedulerModelError,
    SelectionReason, TurnReason, TurnSnapshot, TurnTrace,
};
pub use state::{
    validate_dispatch_transition, validate_turn_transition, DispatchStatus, SchedulerStateError,
    TurnStatus,
};
pub use store::{SchedulerStore, SchedulerStoreError};
pub use topology::{allows_agent_edge, validate_topology, TopologyError, TopologySnapshot};

use budget::{BudgetRejection, TurnBudget};

pub fn next_decision(
    budget: &TurnBudget,
    previous_speaker: Option<&str>,
    user_mentions: &[String],
    action: Option<&SchedulerAction>,
    deterministic_candidates: &[SchedulerCandidate],
    hop: u32,
    moderator_enabled: bool,
) -> SchedulerDecision {
    if let Some(target) = first_eligible(budget, previous_speaker, user_mentions, hop) {
        return SchedulerDecision::Dispatch(SchedulerDispatch {
            target_agent_id: target,
            selection_reason: SelectionReason::UserMention,
            action_kind: ActionKind::Speak,
            hop,
        });
    }
    if let Some(action) = action {
        match action {
            SchedulerAction::Call {
                target_agent_id, ..
            } => {
                return action_decision(
                    budget,
                    previous_speaker,
                    target_agent_id,
                    SelectionReason::AgentCall,
                    ActionKind::Call,
                    hop,
                )
            }
            SchedulerAction::Handoff {
                target_agent_id, ..
            } => {
                return action_decision(
                    budget,
                    previous_speaker,
                    target_agent_id,
                    SelectionReason::AgentHandoff,
                    ActionKind::Handoff,
                    hop,
                )
            }
            SchedulerAction::Speak {
                mentioned_agent_ids,
                ..
            } => {
                if let Some(target) =
                    first_eligible(budget, previous_speaker, mentioned_agent_ids, hop)
                {
                    return SchedulerDecision::Dispatch(SchedulerDispatch {
                        target_agent_id: target,
                        selection_reason: SelectionReason::AgentTextMention,
                        action_kind: ActionKind::Speak,
                        hop,
                    });
                }
            }
            SchedulerAction::Wait { .. } => {
                return SchedulerDecision::Finish {
                    status: TurnStatus::WaitingForUser,
                    reason: TurnReason::WaitingForUser,
                }
            }
            SchedulerAction::Silent => {
                return SchedulerDecision::Finish {
                    status: TurnStatus::Silence,
                    reason: TurnReason::Silence,
                }
            }
        }
    }
    if let Some(target) = deterministic_candidates
        .iter()
        .find(|candidate| {
            candidate.eligible && Some(candidate.agent_id.as_str()) != previous_speaker
        })
        .and_then(|candidate| {
            budget
                .check_dispatch(&candidate.agent_id, hop)
                .ok()
                .map(|_| candidate.agent_id.clone())
        })
    {
        return SchedulerDecision::Dispatch(SchedulerDispatch {
            target_agent_id: target,
            selection_reason: SelectionReason::DeterministicOrder,
            action_kind: ActionKind::Speak,
            hop,
        });
    }
    if moderator_enabled
        && deterministic_candidates
            .iter()
            .filter(|candidate| candidate.eligible)
            .count()
            > 1
    {
        return SchedulerDecision::RequestModerator;
    }
    SchedulerDecision::Finish {
        status: terminal_status(budget),
        reason: terminal_reason(budget),
    }
}

fn first_eligible(
    budget: &TurnBudget,
    previous_speaker: Option<&str>,
    candidates: &[String],
    hop: u32,
) -> Option<String> {
    candidates
        .iter()
        .find(|candidate| {
            Some(candidate.as_str()) != previous_speaker
                && budget.check_dispatch(candidate, hop).is_ok()
        })
        .cloned()
}

fn action_decision(
    budget: &TurnBudget,
    previous_speaker: Option<&str>,
    target: &str,
    reason: SelectionReason,
    action_kind: ActionKind,
    hop: u32,
) -> SchedulerDecision {
    if Some(target) != previous_speaker && budget.check_dispatch(target, hop).is_ok() {
        SchedulerDecision::Dispatch(SchedulerDispatch {
            target_agent_id: target.to_owned(),
            selection_reason: reason,
            action_kind,
            hop,
        })
    } else {
        SchedulerDecision::Finish {
            status: terminal_status(budget),
            reason: terminal_reason(budget),
        }
    }
}

fn terminal_status(budget: &TurnBudget) -> TurnStatus {
    match budget.check_dispatch("terminal", 0) {
        Err(BudgetRejection::Failures) => TurnStatus::FailureBudgetExhausted,
        Err(_) => TurnStatus::BudgetExhausted,
        Ok(()) => TurnStatus::Silence,
    }
}

fn terminal_reason(budget: &TurnBudget) -> TurnReason {
    match budget.check_dispatch("terminal", 0) {
        Err(BudgetRejection::Failures) => TurnReason::FailureBudgetExhausted,
        Err(_) => TurnReason::BudgetExhausted,
        Ok(()) => TurnReason::Silence,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        budget::{BudgetLimits, TurnBudget},
        next_decision, SchedulerAction, SchedulerCandidate, SchedulerDecision, SelectionReason,
    };
    #[test]
    fn scheduler_decision_priority_is_user_then_action_then_deterministic() {
        let budget = TurnBudget::new(BudgetLimits::with_auto_steps(2, Some(8)));
        let candidates = vec![
            SchedulerCandidate {
                agent_id: "a".into(),
                eligible: true,
            },
            SchedulerCandidate {
                agent_id: "b".into(),
                eligible: true,
            },
        ];
        assert!(
            matches!(next_decision(&budget, None, &["b".into()], Some(&SchedulerAction::Call { target_agent_id: "a".into(), task: String::new() }), &candidates, 0, false), SchedulerDecision::Dispatch(ref value) if value.target_agent_id == "b" && value.selection_reason == SelectionReason::UserMention)
        );
        assert!(
            matches!(next_decision(&budget, None, &[], Some(&SchedulerAction::Call { target_agent_id: "b".into(), task: String::new() }), &candidates, 0, false), SchedulerDecision::Dispatch(ref value) if value.selection_reason == SelectionReason::AgentCall)
        );
        assert!(
            matches!(next_decision(&budget, Some("a"), &[], None, &candidates, 0, false), SchedulerDecision::Dispatch(ref value) if value.target_agent_id == "b")
        );
    }

    #[test]
    fn scheduler_decision_reports_failure_budget_exhaustion() {
        let mut budget = TurnBudget::new(BudgetLimits {
            max_agent_steps: 8,
            max_steps_per_agent: 3,
            max_hops: 5,
            max_moderator_calls: 4,
            max_consecutive_failures: 1,
            max_total_failures: 6,
            max_total_tokens: 120_000,
        });
        budget.record_failure();
        let candidates = vec![SchedulerCandidate {
            agent_id: "a".into(),
            eligible: true,
        }];

        assert!(matches!(
            next_decision(&budget, None, &[], None, &candidates, 0, false),
            SchedulerDecision::Finish {
                status: super::TurnStatus::FailureBudgetExhausted,
                reason: super::TurnReason::FailureBudgetExhausted,
            }
        ));
    }
}
