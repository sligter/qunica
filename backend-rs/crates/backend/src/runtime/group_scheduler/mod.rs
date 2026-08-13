pub mod budget;
pub mod cancellation;
pub mod mentions;
pub mod model;
pub mod moderator;
pub mod state;
pub mod store;
pub mod topology;

pub use model::{
    ActionKind, DispatchOutput, DispatchSnapshot, FinishDispatch, NewDispatch, NewTurn,
    SchedulerDecision, SchedulerDispatch, SchedulerModelError, SelectionReason, TurnReason,
    TurnSnapshot, TurnTrace,
};
pub use moderator::{
    select_with_moderator, ModeratorAttempt, ModeratorCandidate, ModeratorConfig, ModeratorFailure,
    ModeratorMessage, ModeratorRequest, ModeratorSelection,
};
pub use state::{
    validate_dispatch_transition, validate_turn_transition, DispatchStatus, SchedulerStateError,
    TurnStatus,
};
pub use store::{SchedulerStore, SchedulerStoreError};
pub use topology::{allows_agent_edge, validate_topology, TopologyError, TopologySnapshot};

pub use budget::{BudgetLimits, BudgetRejection, TurnBudget};
pub use cancellation::{ActiveTurn, ActiveTurnRegistry, TurnCancellation};

/// Pick who speaks next, or report why the turn is over.
///
/// Priority is user `@mentions`, then the agent `@mentions` queued by the last
/// speaker, then the group's deterministic speaking order. Every path draws from
/// the same budget-filtered candidate set, so enabling the moderator changes
/// *who* is chosen, never *whether* anyone is.
pub fn next_decision(
    budget: &TurnBudget,
    previous_speaker: Option<&str>,
    user_mentions: &[String],
    agent_mentions: &[String],
    deterministic_candidates: &[String],
    hop: u32,
    moderator_enabled: bool,
) -> SchedulerDecision {
    if user_mentions.is_empty() && agent_mentions.is_empty() && deterministic_candidates.is_empty()
    {
        return match budget.check_dispatch("terminal", 0) {
            Err(BudgetRejection::Failures) => SchedulerDecision::Finish {
                status: TurnStatus::FailureBudgetExhausted,
                reason: TurnReason::FailureBudgetExhausted,
            },
            Err(BudgetRejection::Tokens) => SchedulerDecision::Finish {
                status: TurnStatus::BudgetExhausted,
                reason: TurnReason::BudgetExhausted,
            },
            _ => SchedulerDecision::Finish {
                status: TurnStatus::Silence,
                reason: TurnReason::Silence,
            },
        };
    }
    if let Some(target) = first_eligible(budget, previous_speaker, user_mentions, hop) {
        return SchedulerDecision::Dispatch(SchedulerDispatch {
            target_agent_id: target,
            selection_reason: SelectionReason::UserMention,
            action_kind: ActionKind::Speak,
            hop,
        });
    }
    if let Some(target) = first_eligible(budget, previous_speaker, agent_mentions, hop) {
        return SchedulerDecision::Dispatch(SchedulerDispatch {
            target_agent_id: target,
            selection_reason: SelectionReason::AgentTextMention,
            action_kind: ActionKind::Speak,
            hop,
        });
    }

    let legal_candidates: Vec<&String> = deterministic_candidates
        .iter()
        .filter(|agent_id| Some(agent_id.as_str()) != previous_speaker)
        .filter(|agent_id| budget.check_dispatch(agent_id, hop).is_ok())
        .collect();
    if moderator_enabled && legal_candidates.len() >= 2 {
        return match budget.check_moderator() {
            Ok(()) => SchedulerDecision::RequestModerator,
            Err(BudgetRejection::ModeratorCalls) => {
                SchedulerDecision::Dispatch(SchedulerDispatch {
                    target_agent_id: legal_candidates[0].clone(),
                    selection_reason: SelectionReason::ModeratorFallback,
                    action_kind: ActionKind::Speak,
                    hop,
                })
            }
            Err(BudgetRejection::Tokens) => SchedulerDecision::Finish {
                status: TurnStatus::BudgetExhausted,
                reason: TurnReason::BudgetExhausted,
            },
            Err(BudgetRejection::Failures) => SchedulerDecision::Finish {
                status: TurnStatus::FailureBudgetExhausted,
                reason: TurnReason::FailureBudgetExhausted,
            },
            Err(_) => {
                unreachable!("moderator checks only reject moderator, token, or failure budgets")
            }
        };
    }
    if let Some(target) = legal_candidates.first() {
        return SchedulerDecision::Dispatch(SchedulerDispatch {
            target_agent_id: (*target).clone(),
            selection_reason: SelectionReason::DeterministicOrder,
            action_kind: ActionKind::Speak,
            hop,
        });
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
        next_decision, SchedulerDecision, SelectionReason,
    };

    fn ids(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_owned()).collect()
    }

    #[test]
    fn scheduler_decision_priority_is_user_then_agent_mention_then_deterministic() {
        let budget = TurnBudget::new(BudgetLimits::with_auto_steps(2, Some(8)));
        let candidates = ids(&["a", "b"]);

        assert!(matches!(
            next_decision(&budget, None, &ids(&["b"]), &ids(&["a"]), &candidates, 0, false),
            SchedulerDecision::Dispatch(ref value)
                if value.target_agent_id == "b"
                    && value.selection_reason == SelectionReason::UserMention
        ));
        assert!(matches!(
            next_decision(&budget, None, &[], &ids(&["b"]), &candidates, 0, false),
            SchedulerDecision::Dispatch(ref value)
                if value.target_agent_id == "b"
                    && value.selection_reason == SelectionReason::AgentTextMention
        ));
        assert!(matches!(
            next_decision(&budget, Some("a"), &[], &[], &candidates, 0, false),
            SchedulerDecision::Dispatch(ref value)
                if value.target_agent_id == "b"
                    && value.selection_reason == SelectionReason::DeterministicOrder
        ));
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

        assert!(matches!(
            next_decision(&budget, None, &[], &[], &ids(&["a"]), 0, false),
            SchedulerDecision::Finish {
                status: super::TurnStatus::FailureBudgetExhausted,
                reason: super::TurnReason::FailureBudgetExhausted,
            }
        ));
    }

    #[test]
    fn deterministic_order_skips_a_candidate_that_is_out_of_per_agent_budget() {
        let mut budget = TurnBudget::new(BudgetLimits::with_auto_steps(2, Some(8)));
        for _ in 0..budget.limits().max_steps_per_agent {
            budget.record_dispatch("a");
        }

        assert!(matches!(
            next_decision(&budget, None, &[], &[], &ids(&["a", "b"]), 0, false),
            SchedulerDecision::Dispatch(ref dispatch)
                if dispatch.target_agent_id == "b"
                    && dispatch.selection_reason == SelectionReason::DeterministicOrder
        ));
    }

    #[test]
    fn moderator_toggle_does_not_change_whether_a_candidate_is_reachable() {
        let mut budget = TurnBudget::new(BudgetLimits::with_auto_steps(3, Some(8)));
        for _ in 0..budget.limits().max_steps_per_agent {
            budget.record_dispatch("a");
        }
        let candidates = ids(&["a", "b"]);

        // Only "b" is affordable. With two legal candidates the moderator would
        // arbitrate, but here both paths must land on the same agent.
        for moderator_enabled in [false, true] {
            assert!(matches!(
                next_decision(&budget, None, &[], &[], &candidates, 0, moderator_enabled),
                SchedulerDecision::Dispatch(ref dispatch) if dispatch.target_agent_id == "b"
            ));
        }
    }

    #[test]
    fn everyone_out_of_budget_finishes_the_turn() {
        let mut budget = TurnBudget::new(BudgetLimits::with_auto_steps(2, Some(8)));
        for agent_id in ["a", "b"] {
            for _ in 0..budget.limits().max_steps_per_agent {
                budget.record_dispatch(agent_id);
            }
        }

        assert!(matches!(
            next_decision(&budget, None, &[], &[], &ids(&["a", "b"]), 0, false),
            SchedulerDecision::Finish { .. }
        ));
    }

    #[test]
    fn exhausting_the_candidate_list_is_a_natural_finish() {
        let mut budget = TurnBudget::new(BudgetLimits {
            max_agent_steps: 1,
            max_steps_per_agent: 1,
            max_hops: 0,
            max_moderator_calls: 0,
            max_consecutive_failures: 1,
            max_total_failures: 1,
            max_total_tokens: 120_000,
        });
        budget.record_dispatch("a");

        assert!(matches!(
            next_decision(&budget, Some("a"), &[], &[], &[], 0, false),
            SchedulerDecision::Finish {
                status: super::TurnStatus::Silence,
                reason: super::TurnReason::Silence,
            }
        ));
    }

    #[test]
    fn empty_candidates_do_not_hide_a_failure_terminal() {
        let mut budget = TurnBudget::new(BudgetLimits {
            max_agent_steps: 1,
            max_steps_per_agent: 1,
            max_hops: 0,
            max_moderator_calls: 0,
            max_consecutive_failures: 1,
            max_total_failures: 1,
            max_total_tokens: 120_000,
        });
        budget.record_failure();

        assert!(matches!(
            next_decision(&budget, None, &[], &[], &[], 0, false),
            SchedulerDecision::Finish {
                status: super::TurnStatus::FailureBudgetExhausted,
                reason: super::TurnReason::FailureBudgetExhausted,
            }
        ));
    }

    #[test]
    fn moderator_call_budget_uses_the_stable_first_candidate_as_fallback() {
        let budget = TurnBudget::new(BudgetLimits {
            max_agent_steps: 8,
            max_steps_per_agent: 3,
            max_hops: 5,
            max_moderator_calls: 0,
            max_consecutive_failures: 3,
            max_total_failures: 6,
            max_total_tokens: 120_000,
        });

        assert!(matches!(
            next_decision(&budget, None, &[], &[], &ids(&["a", "b"]), 0, true),
            SchedulerDecision::Dispatch(ref dispatch)
                if dispatch.target_agent_id == "a"
                    && dispatch.selection_reason == SelectionReason::ModeratorFallback
        ));
    }
}
