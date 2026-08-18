//! Compaction wired into the [hook seam](super::hooks).
//!
//! Two attachment points, matching the two ways a thread outgrows its window:
//!
//! * `pre_step` — the estimate says the next request is approaching the limit,
//!   so reduce before sending it.
//! * `request_error` — the provider says it already did. Recovering here is the
//!   half that was missing: `is_transient_provider_error` never recognised a
//!   context overflow, so an overlong thread failed the turn and failed again
//!   identically on every retry.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use async_trait::async_trait;
use tokio::sync::mpsc::Receiver;

use crate::llm::{
    ChatDelta, ChatMessage, ChatRequest, LlmProvider, ProviderHttpError, ReasoningEffort,
};

use super::{
    compaction::{
        compact, estimate_tokens, CompactionLimits, CompactionTrigger, Summarizer,
        SUMMARY_INSTRUCTION,
    },
    hooks::{RequestRecovery, RuntimeHook, StepContext},
};

/// Reduces the message list before a request, and after one is rejected as too
/// long.
pub struct CompactionHook;

#[async_trait]
impl RuntimeHook for CompactionHook {
    fn name(&self) -> &'static str {
        "compaction"
    }

    async fn pre_step(
        &self,
        step: &StepContext,
        messages: &mut Vec<ChatMessage>,
    ) -> Option<String> {
        // Without a declared window there is no threshold to measure pressure
        // against, and guessing one would discard history on a number nobody
        // supplied. The overflow path still covers this case.
        let limits = CompactionLimits::from_provider(
            step.context_window_tokens,
            step.context_output_reserve_ratio,
            step.fixed_overhead_tokens,
        )?;
        let outcome = compact(
            messages,
            limits,
            CompactionTrigger::Pressure,
            step.summarizer.as_ref(),
        )
        .await?;
        outcome.made_progress().then(|| {
            format!(
                "Compacted context before this request: {}",
                outcome.describe()
            )
        })
    }

    async fn request_error(
        &self,
        step: &StepContext,
        messages: &mut Vec<ChatMessage>,
        error: &anyhow::Error,
    ) -> RequestRecovery {
        if !is_context_overflow(error) {
            return RequestRecovery::Propagate;
        }
        // The provider has just proved the request was too long. When it
        // declares no window, treat the current estimate as the ceiling and
        // reduce from there rather than inventing a token count.
        let limits = CompactionLimits::from_provider(
            step.context_window_tokens,
            step.context_output_reserve_ratio,
            step.fixed_overhead_tokens,
        )
        .unwrap_or(CompactionLimits {
            usable_tokens: estimate_tokens(messages).max(1),
        });

        let Some(outcome) = compact(
            messages,
            limits,
            CompactionTrigger::Overflow,
            step.summarizer.as_ref(),
        )
        .await
        else {
            return RequestRecovery::Propagate;
        };
        // Retrying a request compaction could not shrink would fail the same
        // way. Requiring progress is what keeps this from spinning.
        if !outcome.made_progress() {
            return RequestRecovery::Propagate;
        }
        RequestRecovery::Retry {
            reason: format!(
                "The request exceeded the model's context window, so history was compacted and \
                 the request retried: {}",
                outcome.describe()
            ),
        }
    }
}

/// Whether `error` is a provider rejecting the request as too long.
pub fn is_context_overflow(error: &anyhow::Error) -> bool {
    error
        .downcast_ref::<ProviderHttpError>()
        .is_some_and(ProviderHttpError::is_context_overflow)
}

/// Summarizes a compacted span through the agent's own provider and model.
///
/// Reusing the agent's model rather than configuring a separate one keeps the
/// summary inside the same trust and billing boundary as the conversation it
/// describes, and avoids a second set of credentials that can silently expire.
pub struct ProviderSummarizer {
    provider: Arc<dyn LlmProvider>,
    model: String,
    /// Total tokens the summarizer has spent across every pass this turn.
    ///
    /// Providers report usage on the stream (as `ChatDelta::Usage`), which the
    /// old collector dropped. Recording it here — instead of in the caller's
    /// per-request usage bookkeeping — is what lets it reach the budget and
    /// token records: a summary is not the answer the model is streaming, it is
    /// a second request paid for out of the same turn.
    usage: Arc<AtomicU64>,
}

impl ProviderSummarizer {
    pub fn new(provider: Arc<dyn LlmProvider>, model: String) -> Self {
        Self {
            provider,
            model,
            usage: Arc::new(AtomicU64::new(0)),
        }
    }
}

#[async_trait]
impl Summarizer for ProviderSummarizer {
    async fn summarize(&self, transcript: &str) -> anyhow::Result<String> {
        let request = ChatRequest {
            model: self.model.clone(),
            messages: vec![
                ChatMessage::text("system", SUMMARY_INSTRUCTION),
                ChatMessage::text("user", transcript),
            ],
            temperature: None,
            reasoning_passback: false,
            include_empty_tools: false,
            tools: Vec::new(),
            // A summary is transcription, not deliberation; thinking budget
            // spent here is taken from the turn that needed the room.
            reasoning_effort: Some(ReasoningEffort::Low),
        };
        let stream = self.provider.stream(request).await?;
        let (text, tokens) = collect_text(stream).await;
        self.usage.fetch_add(tokens, Ordering::Relaxed);
        Ok(text)
    }

    fn claimed_tokens(&self) -> u64 {
        self.usage.load(Ordering::Relaxed)
    }
}

/// Drain a provider stream into its assistant text and the tokens it reported.
async fn collect_text(mut stream: Receiver<ChatDelta>) -> (String, u64) {
    let mut text = String::new();
    let mut tokens = 0u64;
    while let Some(delta) = stream.recv().await {
        match delta {
            ChatDelta::Token(chunk) => text.push_str(&chunk),
            ChatDelta::Usage(usage) => {
                tokens = usage
                    .total_tokens
                    .or_else(|| {
                        usage
                            .input_tokens
                            .zip(usage.output_tokens)
                            .map(|(input, output)| input.saturating_add(output))
                    })
                    .unwrap_or_default()
                    .max(0) as u64;
            }
            ChatDelta::Done => break,
            // Reasoning, tool calls, and a truncated stream all leave whatever
            // text arrived usable; an empty result falls back to the
            // host-authored placeholder in `compact`.
            _ => {}
        }
    }
    (text, tokens)
}

/// A summarizer for turns that have no provider to call.
///
/// Returning an error rather than empty text routes through the same
/// "could not be summarized" placeholder as a provider failure, so the model is
/// told the history is gone instead of being handed a blank summary.
pub struct UnavailableSummarizer;

#[async_trait]
impl Summarizer for UnavailableSummarizer {
    async fn summarize(&self, _transcript: &str) -> anyhow::Result<String> {
        Err(anyhow::anyhow!("no summarization provider is configured"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::hooks::HookChain;

    fn step(window: Option<i64>) -> StepContext {
        StepContext {
            context_window_tokens: window,
            context_output_reserve_ratio: Some(0.2),
            ..crate::runtime::hooks::test_support::step_context()
        }
    }

    /// `ChatMessage` has no `PartialEq`; compare who said what, in order.
    fn shape(messages: &[ChatMessage]) -> Vec<(String, String)> {
        messages
            .iter()
            .map(|message| (message.role.clone(), message.content.clone()))
            .collect()
    }

    fn long_thread() -> Vec<ChatMessage> {
        let mut messages = vec![ChatMessage::text("system", "system")];
        for index in 0..40 {
            messages.push(ChatMessage::text(
                "user",
                format!("question {index} {}", "x".repeat(200)),
            ));
            messages.push(ChatMessage::text(
                "assistant",
                format!("answer {index} {}", "y".repeat(200)),
            ));
        }
        messages
    }

    fn overflow_error() -> anyhow::Error {
        ProviderHttpError {
            status: 400,
            body: r#"{"error":{"message":"This model's maximum context length is 128000 tokens","code":"context_length_exceeded"}}"#.to_string(),
        }
        .into()
    }

    #[test]
    fn only_a_context_overflow_body_counts_as_one() {
        assert!(is_context_overflow(&overflow_error()));
        assert!(!is_context_overflow(&anyhow::anyhow!("connection reset")));
        assert!(!is_context_overflow(
            &ProviderHttpError {
                status: 400,
                body: "invalid tool schema".to_string(),
            }
            .into()
        ));
        assert!(!is_context_overflow(
            &ProviderHttpError {
                status: 500,
                body: "maximum context length".to_string(),
            }
            .into()
        ));
    }

    #[tokio::test]
    async fn overflow_compacts_and_asks_for_a_retry() {
        let chain = HookChain::new().with(Arc::new(CompactionHook));
        let mut messages = long_thread();
        let before = messages.len();

        let recovery = chain
            .request_error(&step(Some(128_000)), &mut messages, &overflow_error())
            .await;

        match recovery {
            RequestRecovery::Retry { reason } => {
                assert!(reason.contains("context window"), "{reason}");
                assert!(messages.len() < before, "history should have shrunk");
                assert_eq!(messages[0].role, "system");
            }
            RequestRecovery::Propagate => panic!("an overflow should be recoverable"),
        }
    }

    #[tokio::test]
    async fn overflow_recovers_even_without_a_declared_window() {
        let chain = HookChain::new().with(Arc::new(CompactionHook));
        let mut messages = long_thread();
        let recovery = chain
            .request_error(&step(None), &mut messages, &overflow_error())
            .await;
        assert!(matches!(recovery, RequestRecovery::Retry { .. }));
    }

    #[tokio::test]
    async fn an_unrelated_failure_is_left_alone() {
        let chain = HookChain::new().with(Arc::new(CompactionHook));
        let mut messages = long_thread();
        let before = shape(&messages);
        let recovery = chain
            .request_error(
                &step(Some(128_000)),
                &mut messages,
                &anyhow::anyhow!("connection reset"),
            )
            .await;
        assert_eq!(recovery, RequestRecovery::Propagate);
        assert_eq!(
            shape(&messages),
            before,
            "an unrelated failure must not edit history"
        );
    }

    #[tokio::test]
    async fn a_thread_that_cannot_shrink_propagates_instead_of_retrying() {
        let chain = HookChain::new().with(Arc::new(CompactionHook));
        // One system message and one short exchange: nothing to prune, and no
        // span long enough to summarize.
        let mut messages = vec![
            ChatMessage::text("system", "system"),
            ChatMessage::text("user", "hi"),
        ];
        let recovery = chain
            .request_error(&step(Some(128_000)), &mut messages, &overflow_error())
            .await;
        assert_eq!(recovery, RequestRecovery::Propagate);
    }

    #[tokio::test]
    async fn pressure_does_nothing_without_a_declared_window() {
        let chain = HookChain::new().with(Arc::new(CompactionHook));
        let mut messages = long_thread();
        let before = shape(&messages);
        let notices = chain.pre_step(&step(None), &mut messages).await;
        assert!(notices.is_empty());
        assert_eq!(shape(&messages), before);
    }

    #[tokio::test]
    async fn pressure_compacts_when_the_window_is_small_enough_to_matter() {
        let chain = HookChain::new().with(Arc::new(CompactionHook));
        let mut messages = long_thread();
        let before = messages.len();
        let notices = chain.pre_step(&step(Some(2_000)), &mut messages).await;
        assert_eq!(notices.len(), 1, "{notices:?}");
        assert!(notices[0].contains("Compacted context"), "{notices:?}");
        assert!(messages.len() < before);
    }

    #[tokio::test]
    async fn collecting_a_summary_captures_what_the_provider_reported() {
        let (tx, rx) = tokio::sync::mpsc::channel(4);
        tx.send(ChatDelta::Token("a summary".to_string()))
            .await
            .unwrap();
        tx.send(ChatDelta::Usage(ag_swarmer_domain::runtime::ContextUsage {
            input_tokens: Some(1_200),
            output_tokens: Some(30),
            total_tokens: Some(1_230),
            context_window_tokens: None,
            output_reserve_tokens: None,
            ratio: None,
            source: None,
        }))
        .await
        .unwrap();
        tx.send(ChatDelta::Done).await.unwrap();

        let (text, tokens) = collect_text(rx).await;
        assert_eq!(text, "a summary");
        assert_eq!(tokens, 1_230);
    }
}
