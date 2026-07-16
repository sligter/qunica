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
    SchedulerAction, SchedulerCandidate, SchedulerDecision, SchedulerDispatch, SchedulerModelError,
    SelectionReason, TurnReason, TurnSnapshot, TurnTrace,
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
    if !moderator_enabled {
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
        return SchedulerDecision::Finish {
            status: terminal_status(budget),
            reason: terminal_reason(budget),
        };
    }

    let legal_candidates: Vec<&SchedulerCandidate> = deterministic_candidates
        .iter()
        .filter(|candidate| {
            candidate.eligible && Some(candidate.agent_id.as_str()) != previous_speaker
        })
        .filter(|candidate| budget.check_dispatch(&candidate.agent_id, hop).is_ok())
        .collect();
    if moderator_enabled && legal_candidates.len() >= 2 {
        return match budget.check_moderator() {
            Ok(()) => SchedulerDecision::RequestModerator,
            Err(BudgetRejection::ModeratorCalls) => {
                SchedulerDecision::Dispatch(SchedulerDispatch {
                    target_agent_id: legal_candidates[0].agent_id.clone(),
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
            target_agent_id: target.agent_id.clone(),
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

    #[test]
    fn disabled_scheduler_keeps_the_first_deterministic_candidate_semantics() {
        let mut budget = TurnBudget::new(BudgetLimits::with_auto_steps(2, Some(8)));
        for _ in 0..budget.limits().max_steps_per_agent {
            budget.record_dispatch("a");
        }
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

        assert!(matches!(
            next_decision(&budget, None, &[], None, &candidates, 0, false),
            SchedulerDecision::Finish {
                status: super::TurnStatus::Silence,
                reason: super::TurnReason::Silence,
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

        assert!(matches!(
            next_decision(&budget, None, &[], None, &candidates, 0, true),
            SchedulerDecision::Dispatch(ref dispatch)
                if dispatch.target_agent_id == "a"
                    && dispatch.selection_reason == SelectionReason::ModeratorFallback
        ));
    }
}
