use std::collections::HashMap;

use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BudgetLimits {
    pub max_agent_steps: u32,
    pub max_steps_per_agent: u32,
    pub max_hops: u32,
    pub max_moderator_calls: u32,
    pub max_consecutive_failures: u32,
    pub max_total_failures: u32,
    pub max_total_tokens: u64,
}

impl BudgetLimits {
    pub fn resolve_agent_steps(
        candidate_count: usize,
        max_agent_steps: Option<u32>,
        max_steps_per_agent: u32,
        max_hops: u32,
    ) -> u32 {
        max_agent_steps.unwrap_or_else(|| {
            if max_steps_per_agent == 1 && max_hops == 0 {
                candidate_count.min(u32::MAX as usize) as u32
            } else {
                (candidate_count as u32).saturating_mul(3).clamp(8, 24)
            }
        })
    }

    pub fn with_auto_steps(active_agents: usize, max_agent_steps: Option<u32>) -> Self {
        Self {
            max_agent_steps: Self::resolve_agent_steps(active_agents, max_agent_steps, 3, 5),
            max_steps_per_agent: 3,
            max_hops: 5,
            max_moderator_calls: 4,
            max_consecutive_failures: 3,
            max_total_failures: 6,
            max_total_tokens: 120_000,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TurnBudget {
    limits: BudgetLimits,
    agent_steps: u32,
    steps_per_agent: HashMap<String, u32>,
    moderator_calls: u32,
    consecutive_failures: u32,
    total_failures: u32,
    total_tokens: u64,
}

impl TurnBudget {
    pub fn new(limits: BudgetLimits) -> Self {
        Self {
            limits,
            agent_steps: 0,
            steps_per_agent: HashMap::new(),
            moderator_calls: 0,
            consecutive_failures: 0,
            total_failures: 0,
            total_tokens: 0,
        }
    }

    pub fn limits(&self) -> BudgetLimits {
        self.limits
    }
    pub fn agent_steps(&self) -> u32 {
        self.agent_steps
    }
    pub fn total_tokens(&self) -> u64 {
        self.total_tokens
    }
    pub fn moderator_calls(&self) -> u32 {
        self.moderator_calls
    }
    pub fn consecutive_failures(&self) -> u32 {
        self.consecutive_failures
    }
    pub fn total_failures(&self) -> u32 {
        self.total_failures
    }

    pub fn has_dispatched(&self, agent_id: &str) -> bool {
        self.steps_per_agent.contains_key(agent_id)
    }

    pub fn check_dispatch(&self, target_agent_id: &str, hop: u32) -> Result<(), BudgetRejection> {
        if self.agent_steps >= self.limits.max_agent_steps {
            return Err(BudgetRejection::AgentSteps);
        }
        if self
            .steps_per_agent
            .get(target_agent_id)
            .copied()
            .unwrap_or_default()
            >= self.limits.max_steps_per_agent
        {
            return Err(BudgetRejection::PerAgentSteps);
        }
        if hop > self.limits.max_hops {
            return Err(BudgetRejection::Hops);
        }
        if self.total_tokens >= self.limits.max_total_tokens {
            return Err(BudgetRejection::Tokens);
        }
        if self.consecutive_failures >= self.limits.max_consecutive_failures
            || self.total_failures >= self.limits.max_total_failures
        {
            return Err(BudgetRejection::Failures);
        }
        Ok(())
    }

    pub fn check_moderator(&self) -> Result<(), BudgetRejection> {
        if self.moderator_calls >= self.limits.max_moderator_calls {
            return Err(BudgetRejection::ModeratorCalls);
        }
        if self.total_tokens >= self.limits.max_total_tokens {
            return Err(BudgetRejection::Tokens);
        }
        if self.consecutive_failures >= self.limits.max_consecutive_failures
            || self.total_failures >= self.limits.max_total_failures
        {
            return Err(BudgetRejection::Failures);
        }
        Ok(())
    }

    pub fn record_dispatch(&mut self, target_agent_id: &str) {
        self.agent_steps = self.agent_steps.saturating_add(1);
        let steps = self
            .steps_per_agent
            .entry(target_agent_id.to_owned())
            .or_default();
        *steps = steps.saturating_add(1);
    }

    pub fn record_completion(&mut self, tokens: u64) {
        self.record_tokens(tokens);
        self.consecutive_failures = 0;
    }
    pub fn record_tokens(&mut self, tokens: u64) {
        self.total_tokens = self.total_tokens.saturating_add(tokens);
    }
    pub fn record_failure(&mut self) {
        self.consecutive_failures = self.consecutive_failures.saturating_add(1);
        self.total_failures = self.total_failures.saturating_add(1);
    }
    pub fn record_moderator_usage(&mut self, tokens: u64) -> Result<(), BudgetRejection> {
        if self.moderator_calls >= self.limits.max_moderator_calls {
            return Err(BudgetRejection::ModeratorCalls);
        }
        self.moderator_calls = self.moderator_calls.saturating_add(1);
        self.total_tokens = self.total_tokens.saturating_add(tokens);
        if self.total_tokens > self.limits.max_total_tokens {
            return Err(BudgetRejection::Tokens);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum BudgetRejection {
    #[error("agent step budget exhausted")]
    AgentSteps,
    #[error("per-agent step budget exhausted")]
    PerAgentSteps,
    #[error("hop budget exhausted")]
    Hops,
    #[error("moderator call budget exhausted")]
    ModeratorCalls,
    #[error("failure budget exhausted")]
    Failures,
    #[error("token budget exhausted")]
    Tokens,
}

#[cfg(test)]
mod tests {
    use super::{BudgetLimits, BudgetRejection, TurnBudget};

    #[test]
    fn auto_step_limit_is_clamped() {
        assert_eq!(BudgetLimits::with_auto_steps(1, None).max_agent_steps, 8);
        assert_eq!(BudgetLimits::with_auto_steps(4, None).max_agent_steps, 12);
        assert_eq!(BudgetLimits::with_auto_steps(20, None).max_agent_steps, 24);
    }

    #[test]
    fn one_pass_auto_limit_matches_the_candidate_count() {
        assert_eq!(BudgetLimits::resolve_agent_steps(30, None, 1, 0), 30);
        assert_eq!(BudgetLimits::resolve_agent_steps(30, Some(7), 1, 0), 7);
    }

    #[test]
    fn budget_rejects_each_hard_limit() {
        let mut budget = TurnBudget::new(BudgetLimits {
            max_agent_steps: 1,
            max_steps_per_agent: 1,
            max_hops: 0,
            max_moderator_calls: 0,
            max_consecutive_failures: 1,
            max_total_failures: 2,
            max_total_tokens: 10,
        });
        assert_eq!(budget.check_dispatch("a", 1), Err(BudgetRejection::Hops));
        budget.record_dispatch("a");
        assert_eq!(
            budget.check_dispatch("b", 0),
            Err(BudgetRejection::AgentSteps)
        );
        budget.record_failure();
        assert_eq!(
            budget.check_dispatch("b", 0),
            Err(BudgetRejection::AgentSteps)
        );
        assert_eq!(
            budget.record_moderator_usage(1),
            Err(BudgetRejection::ModeratorCalls)
        );
    }

    #[test]
    fn moderator_check_rejects_call_token_and_failure_limits_without_mutating() {
        let call_limited = TurnBudget::new(BudgetLimits {
            max_agent_steps: 8,
            max_steps_per_agent: 3,
            max_hops: 5,
            max_moderator_calls: 0,
            max_consecutive_failures: 3,
            max_total_failures: 6,
            max_total_tokens: 120_000,
        });
        assert_eq!(
            call_limited.check_moderator(),
            Err(BudgetRejection::ModeratorCalls)
        );

        let token_limited = TurnBudget::new(BudgetLimits {
            max_agent_steps: 8,
            max_steps_per_agent: 3,
            max_hops: 5,
            max_moderator_calls: 4,
            max_consecutive_failures: 3,
            max_total_failures: 6,
            max_total_tokens: 0,
        });
        assert_eq!(
            token_limited.check_moderator(),
            Err(BudgetRejection::Tokens)
        );

        let failure_limited = TurnBudget::new(BudgetLimits {
            max_agent_steps: 8,
            max_steps_per_agent: 3,
            max_hops: 5,
            max_moderator_calls: 4,
            max_consecutive_failures: 0,
            max_total_failures: 6,
            max_total_tokens: 120_000,
        });
        assert_eq!(
            failure_limited.check_moderator(),
            Err(BudgetRejection::Failures)
        );
    }
}
