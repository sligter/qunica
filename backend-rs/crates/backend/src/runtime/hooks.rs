//! The group runtime's hook seam.
//!
//! Before this existed there was nowhere to attach behaviour that has to run
//! *around* a step: context compaction, request-error recovery, tool-call
//! policy, and per-tool telemetry all had to be written inline in
//! `run_agent_turn`, which is why none of them were. The seam gives each of
//! those a named place to live and makes the order they run in explicit.
//!
//! Four phases, matching the points where the loop can actually be influenced:
//!
//! | phase | when | what it can do |
//! |---|---|---|
//! | [`RuntimeHook::pre_step`] | before a request is derived | rewrite the message list |
//! | [`RuntimeHook::request_error`] | after a provider request failed | ask for a retry |
//! | [`RuntimeHook::pre_tool`] | before a tool executes | short-circuit with a result |
//! | [`RuntimeHook::post_tool`] | after a tool produced a result | rewrite the result |
//!
//! Hooks run in registration order, and each sees what the previous one left
//! behind. This is an in-process seam, not a plugin host: everything registered
//! is compiled in. The point is that the runtime loop states *where* work
//! attaches rather than burying it in the middle of a 400-line function.

use std::sync::Arc;

use async_trait::async_trait;

use crate::{
    llm::{ChatMessage, ToolCall},
    tools::{ToolResult, ToolStatus},
};

/// What a hook wants done about a failed provider request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RequestRecovery {
    /// Leave the failure alone; the next hook, then the caller, decides.
    Propagate,
    /// The messages were changed such that re-issuing the request is worth it.
    /// `reason` is surfaced to the user so a silent retry never looks like a
    /// stall.
    Retry { reason: String },
}

/// Facts about the step a hook is running inside.
#[derive(Clone)]
pub struct StepContext {
    pub agent_id: String,
    pub agent_display_name: String,
    pub model: String,
    /// The provider's declared context window, when it declares one.
    pub context_window_tokens: Option<i64>,
    /// The fraction of that window reserved for output.
    pub context_output_reserve_ratio: Option<f64>,
    /// Tokens this request spends on things compaction cannot shrink — the
    /// tool schemas. Subtracted from the window so the pressure threshold is
    /// measured against the room the messages actually have.
    pub fixed_overhead_tokens: i64,
    /// Produces replacement text when a span of history has to be compacted.
    /// Carried here so a hook can summarize through the same provider and model
    /// the step itself is using.
    pub summarizer: Arc<dyn super::compaction::Summarizer>,
}

impl std::fmt::Debug for StepContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StepContext")
            .field("agent_id", &self.agent_id)
            .field("agent_display_name", &self.agent_display_name)
            .field("model", &self.model)
            .field("context_window_tokens", &self.context_window_tokens)
            .field(
                "context_output_reserve_ratio",
                &self.context_output_reserve_ratio,
            )
            .field("fixed_overhead_tokens", &self.fixed_overhead_tokens)
            .finish_non_exhaustive()
    }
}

/// Behaviour attached around a step of the agent loop.
///
/// Every method has a default that does nothing, so an implementation names only
/// the phases it cares about.
#[async_trait]
pub trait RuntimeHook: Send + Sync {
    /// Identifies the hook in logs and in user-facing notices.
    fn name(&self) -> &'static str;

    /// Runs before the provider request is derived from `messages`.
    async fn pre_step(
        &self,
        _step: &StepContext,
        _messages: &mut Vec<ChatMessage>,
    ) -> Option<String> {
        None
    }

    /// Runs after a provider request failed, before the failure is reported.
    async fn request_error(
        &self,
        _step: &StepContext,
        _messages: &mut Vec<ChatMessage>,
        _error: &anyhow::Error,
    ) -> RequestRecovery {
        RequestRecovery::Propagate
    }

    /// Runs before `call` executes. `Some(result)` skips execution entirely.
    async fn pre_tool(&self, _step: &StepContext, _call: &ToolCall) -> Option<ToolResult> {
        None
    }

    /// Runs after `call` produced `result`, which it may rewrite.
    async fn post_tool(&self, _step: &StepContext, _call: &ToolCall, _result: &mut ToolResult) {}
}

/// The hooks a runtime runs, in order.
#[derive(Clone, Default)]
pub struct HookChain {
    hooks: Vec<Arc<dyn RuntimeHook>>,
}

impl std::fmt::Debug for HookChain {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HookChain")
            .field(
                "hooks",
                &self
                    .hooks
                    .iter()
                    .map(|hook| hook.name())
                    .collect::<Vec<_>>(),
            )
            .finish()
    }
}

impl HookChain {
    pub fn new() -> Self {
        Self::default()
    }

    /// The chain every group turn runs with unless a caller overrides it.
    pub fn defaults() -> Self {
        Self::new()
            .with(Arc::new(super::compaction_hook::CompactionHook))
            // Before the trace hook, so what it logs is the size that actually
            // reached the model rather than the size the tool happened to
            // return.
            .with(Arc::new(super::tool_output::ToolResultCapHook))
            .with(Arc::new(ToolTraceHook))
    }

    pub fn with(mut self, hook: Arc<dyn RuntimeHook>) -> Self {
        self.hooks.push(hook);
        self
    }

    pub fn is_empty(&self) -> bool {
        self.hooks.is_empty()
    }

    /// Run every `pre_step`, collecting the notices hooks want surfaced.
    pub async fn pre_step(
        &self,
        step: &StepContext,
        messages: &mut Vec<ChatMessage>,
    ) -> Vec<String> {
        let mut notices = Vec::new();
        for hook in &self.hooks {
            if let Some(notice) = hook.pre_step(step, messages).await {
                notices.push(notice);
            }
        }
        notices
    }

    /// Ask each hook in turn whether the failure is recoverable. The first
    /// `Retry` wins, and later hooks do not run: a second hook rewriting the
    /// messages after one has already decided to retry would retry something
    /// neither of them agreed to send.
    pub async fn request_error(
        &self,
        step: &StepContext,
        messages: &mut Vec<ChatMessage>,
        error: &anyhow::Error,
    ) -> RequestRecovery {
        for hook in &self.hooks {
            match hook.request_error(step, messages, error).await {
                RequestRecovery::Propagate => continue,
                retry => return retry,
            }
        }
        RequestRecovery::Propagate
    }

    /// Ask each hook whether `call` should run. The first result short-circuits.
    pub async fn pre_tool(&self, step: &StepContext, call: &ToolCall) -> Option<ToolResult> {
        for hook in &self.hooks {
            if let Some(result) = hook.pre_tool(step, call).await {
                return Some(result);
            }
        }
        None
    }

    /// Let every hook see and revise the result.
    pub async fn post_tool(&self, step: &StepContext, call: &ToolCall, result: &mut ToolResult) {
        for hook in &self.hooks {
            hook.post_tool(step, call, result).await;
        }
    }
}

/// Records every tool call's outcome on the tracing span for the turn.
///
/// Small on purpose: it is the seam's second consumer, and it demonstrates the
/// `post_tool` phase without changing what the model sees.
pub struct ToolTraceHook;

#[async_trait]
impl RuntimeHook for ToolTraceHook {
    fn name(&self) -> &'static str {
        "tool-trace"
    }

    async fn post_tool(&self, step: &StepContext, call: &ToolCall, result: &mut ToolResult) {
        let failed = matches!(
            result.status,
            ToolStatus::Failed | ToolStatus::SetupRequired | ToolStatus::WorkspaceRequired
        );
        if failed {
            tracing::warn!(
                agent_id = %step.agent_id,
                tool = %call.name,
                status = ?result.status,
                "tool call did not complete"
            );
        } else {
            tracing::debug!(
                agent_id = %step.agent_id,
                tool = %call.name,
                output_chars = result.output.chars().count(),
                "tool call completed"
            );
        }
    }
}

/// A [`StepContext`] for tests in this crate that need one but do not care what
/// is in it.
///
/// Shared rather than re-declared per module so a new field on `StepContext`
/// costs one edit instead of one per test module.
#[cfg(test)]
pub(crate) mod test_support {
    use super::StepContext;
    use std::sync::Arc;

    pub(crate) fn step_context() -> StepContext {
        StepContext {
            agent_id: "agent-1".to_string(),
            agent_display_name: "Agent".to_string(),
            model: "test-model".to_string(),
            context_window_tokens: None,
            context_output_reserve_ratio: None,
            fixed_overhead_tokens: 0,
            summarizer: Arc::new(crate::runtime::compaction_hook::UnavailableSummarizer),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn step() -> StepContext {
        test_support::step_context()
    }

    fn call() -> ToolCall {
        ToolCall {
            id: "c1".to_string(),
            name: "Read".to_string(),
            args: json!({}),
            provider_metadata: None,
        }
    }

    struct Appender(&'static str);

    #[async_trait]
    impl RuntimeHook for Appender {
        fn name(&self) -> &'static str {
            "appender"
        }

        async fn pre_step(
            &self,
            _step: &StepContext,
            messages: &mut Vec<ChatMessage>,
        ) -> Option<String> {
            messages.push(ChatMessage::text("user", self.0));
            Some(format!("appended {}", self.0))
        }

        async fn post_tool(&self, _step: &StepContext, _call: &ToolCall, result: &mut ToolResult) {
            result.output.push_str(self.0);
        }
    }

    struct Blocker;

    #[async_trait]
    impl RuntimeHook for Blocker {
        fn name(&self) -> &'static str {
            "blocker"
        }

        async fn pre_tool(&self, _step: &StepContext, _call: &ToolCall) -> Option<ToolResult> {
            Some(ToolResult {
                status: ToolStatus::Failed,
                output: "refused".to_string(),
            })
        }
    }

    struct Retrier;

    #[async_trait]
    impl RuntimeHook for Retrier {
        fn name(&self) -> &'static str {
            "retrier"
        }

        async fn request_error(
            &self,
            _step: &StepContext,
            messages: &mut Vec<ChatMessage>,
            _error: &anyhow::Error,
        ) -> RequestRecovery {
            messages.clear();
            RequestRecovery::Retry {
                reason: "recovered".to_string(),
            }
        }
    }

    #[tokio::test]
    async fn pre_step_hooks_run_in_order_and_each_sees_the_previous_edit() {
        let chain = HookChain::new()
            .with(Arc::new(Appender("first")))
            .with(Arc::new(Appender("second")));
        let mut messages = Vec::new();
        let notices = chain.pre_step(&step(), &mut messages).await;

        assert_eq!(
            messages
                .iter()
                .map(|m| m.content.clone())
                .collect::<Vec<_>>(),
            vec!["first", "second"]
        );
        assert_eq!(notices, vec!["appended first", "appended second"]);
    }

    #[tokio::test]
    async fn pre_tool_short_circuits_at_the_first_result() {
        let chain = HookChain::new().with(Arc::new(Blocker));
        let blocked = chain.pre_tool(&step(), &call()).await.unwrap();
        assert_eq!(blocked.output, "refused");

        assert!(HookChain::new().pre_tool(&step(), &call()).await.is_none());
    }

    #[tokio::test]
    async fn post_tool_hooks_all_see_the_result() {
        let chain = HookChain::new()
            .with(Arc::new(Appender("-a")))
            .with(Arc::new(Appender("-b")));
        let mut result = ToolResult::completed("out");
        chain.post_tool(&step(), &call(), &mut result).await;
        assert_eq!(result.output, "out-a-b");
    }

    #[tokio::test]
    async fn the_first_retry_wins_and_stops_the_chain() {
        let chain = HookChain::new()
            .with(Arc::new(Retrier))
            .with(Arc::new(Appender("must not run")));
        let mut messages = vec![ChatMessage::text("user", "hi")];
        let recovery = chain
            .request_error(&step(), &mut messages, &anyhow::anyhow!("boom"))
            .await;

        assert_eq!(
            recovery,
            RequestRecovery::Retry {
                reason: "recovered".to_string()
            }
        );
        assert!(messages.is_empty(), "the later hook must not have appended");
    }

    #[tokio::test]
    async fn an_unrecovered_failure_propagates() {
        let chain = HookChain::new().with(Arc::new(Appender("x")));
        let recovery = chain
            .request_error(&step(), &mut Vec::new(), &anyhow::anyhow!("boom"))
            .await;
        assert_eq!(recovery, RequestRecovery::Propagate);
    }

    #[test]
    fn the_default_chain_is_populated_and_names_its_hooks() {
        let chain = HookChain::defaults();
        assert!(!chain.is_empty());
        let described = format!("{chain:?}");
        assert!(described.contains("compaction"), "{described}");
        assert!(described.contains("tool-trace"), "{described}");
    }
}
