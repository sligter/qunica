use super::{AttemptPhase, DriverBudgetExhausted, DriverCheckpoint};
use crate::llm::rig_engine::{CompletionReport, RigMessageAdapter, RigProvider};
use crate::llm::{ChatDelta, ChatMessage, ChatRequest, ContextUsage};
use anyhow::{bail, ensure, Context};
use rig_agent::run::{AgentRun, AgentRunStep, ExecCall, PendingToolCall, StreamedTurn, TurnPolicy};
use rig_core::completion::{FinishReason, Message, ResponseIdentity, Usage};
use rig_core::tool::{ToolOutput, ToolResult};
use serde_json::Value;
use std::sync::Arc;
use tokio::sync::{mpsc::Receiver, oneshot};

pub(crate) struct RigDriver {
    provider: Arc<RigProvider>,
    run: Option<AgentRun>,
    offered: Vec<Message>,
    pending: Vec<ExecCall>,
    policy: TurnPolicy,
    report: Option<oneshot::Receiver<CompletionReport>>,
    pub checkpoint: DriverCheckpoint,
    max_calls: Option<u64>,
    max_failures: u64,
    max_tokens: Option<u64>,
}

impl RigDriver {
    pub fn new(provider: Arc<RigProvider>, config: &Value) -> anyhow::Result<Self> {
        let limit = |key: &str, default| -> anyhow::Result<u64> {
            match config.get("rig_limits").and_then(|limits| limits.get(key)) {
                None => Ok(default),
                Some(value) => value
                    .as_u64()
                    .filter(|value| *value > 0)
                    .with_context(|| format!("rig_limits.{key} must be a positive integer")),
            }
        };
        let optional = |key: &str| -> anyhow::Result<Option<u64>> {
            config
                .get("rig_limits")
                .and_then(|limits| limits.get(key))
                .map(|_| limit(key, 1))
                .transpose()
        };
        Ok(Self {
            provider,
            run: None,
            offered: Vec::new(),
            pending: Vec::new(),
            policy: TurnPolicy::default(),
            report: None,
            checkpoint: DriverCheckpoint::default(),
            // Note: long tool loops are normal work; a model-call cap applies
            // only when the agent configures one.
            max_calls: optional("max_model_calls")?,
            max_failures: limit("max_failed_attempts", 16)?,
            max_tokens: optional("max_total_tokens")?,
        })
    }

    pub fn restore(&mut self, checkpoint: Option<&DriverCheckpoint>) {
        let Some(checkpoint) = checkpoint else {
            return;
        };
        self.checkpoint = checkpoint.clone();
        // The host settles original SQLite tool batches before building a run.
        // A request interrupted without accepted tools has no execution to replay.
        if matches!(
            self.checkpoint.phase,
            AttemptPhase::Prepared
                | AttemptPhase::Streaming
                | AttemptPhase::TerminalReceived
                | AttemptPhase::CompletionRecorded
                | AttemptPhase::StreamedTurnAccepted
        ) {
            self.checkpoint.failed_attempts = self.checkpoint.failed_attempts.saturating_add(1);
            self.checkpoint.phase = AttemptPhase::Aborted;
        }
    }

    /// Note: only an explicit human Resume of an interrupted reply reaches
    /// here. Without it, a run that spent its call or failure budget could never
    /// continue. Retries, approval resumes and restarts keep the current window.
    pub fn open_budget_window(&mut self) {
        let checkpoint = &mut self.checkpoint;
        checkpoint.window_attempts = checkpoint.attempts;
        checkpoint.window_failed_attempts = checkpoint.failed_attempts;
        checkpoint.window_total_tokens = checkpoint.total_tokens;
    }

    fn window_tokens(&self) -> u64 {
        self.checkpoint
            .total_tokens
            .saturating_sub(self.checkpoint.window_total_tokens)
    }

    pub fn settle(&mut self, messages: &[ChatMessage]) -> anyhow::Result<()> {
        if self.pending.is_empty() {
            return Ok(());
        }
        let batch = messages
            .iter()
            .rposition(|message| {
                message.role == "assistant"
                    && self.pending.iter().all(|call| {
                        message.tool_calls.iter().any(|old| {
                            old.id == call.id().wire() && old.name == call.name().as_str()
                        })
                    })
            })
            .context("Rig tool batch is missing its original assistant message")?;
        let mut answers = Vec::new();
        for call in &self.pending {
            let results: Vec<_> = messages[batch + 1..]
                .iter()
                .filter(|message| {
                    message.role == "tool"
                        && message.tool_call_id.as_deref() == Some(call.id().wire().as_ref())
                })
                .collect();
            ensure!(
                results.len() == 1,
                "Rig tool batch is not fully and uniquely settled"
            );
            ensure!(
                results[0].tool_name.as_deref() == Some(call.name().as_str()),
                "Rig tool result changed its tool name"
            );
            answers.push(
                call.clone()
                    .answer(ToolResult::success(ToolOutput::text(&results[0].content))),
            );
        }
        self.run
            .as_mut()
            .context("missing Rig run")?
            .answer_all(answers)?;
        self.pending.clear();
        self.offer_model()?;
        Ok(())
    }

    fn offer_model(&mut self) -> anyhow::Result<()> {
        match self.run.as_mut().context("missing Rig run")?.next_step()? {
            AgentRunStep::CallModel {
                history, prompt, ..
            } => {
                self.offered = history
                    .iter()
                    .chain(std::iter::once(&prompt))
                    .map(|message| (**message).clone())
                    .collect();
                Ok(())
            }
            _ => bail!("Rig did not request the next model step"),
        }
    }

    pub fn prepare(&mut self, request: &ChatRequest) -> anyhow::Result<()> {
        ensure!(
            self.pending.is_empty(),
            "cannot rebuild Rig with unsettled tools"
        );
        let checkpoint = &self.checkpoint;
        if self.max_calls.is_some_and(|limit| {
            checkpoint
                .attempts
                .saturating_sub(checkpoint.window_attempts)
                >= limit
        }) {
            return Err(DriverBudgetExhausted("model-call").into());
        }
        if checkpoint
            .failed_attempts
            .saturating_sub(checkpoint.window_failed_attempts)
            >= self.max_failures
        {
            return Err(DriverBudgetExhausted("failure").into());
        }
        if self
            .max_tokens
            .is_some_and(|limit| self.window_tokens() >= limit)
        {
            return Err(DriverBudgetExhausted("token").into());
        }
        let history = RigMessageAdapter::to_rig(&request.messages)?;
        // Validate the complete exchange before splitting its last prompt:
        // with_validated_history alone rejects a preceding call whose answer
        // is the prompt. No pairing repair is permitted at this boundary.
        rig_core::conversation::pairing::validate(&history)?;
        if self.run.is_none() || coalesce(&self.offered) != coalesce(&history) {
            let mut history = history;
            let prompt = history.pop().context("Rig request has no messages")?;
            self.run = Some(
                AgentRun::new(prompt)
                    .with_history(history)
                    .max_turns(usize::MAX)
                    .max_invalid_tool_call_retries(0),
            );
            self.offer_model()?;
        }
        ensure!(
            coalesce(&self.offered) == coalesce(&RigMessageAdapter::to_rig(&request.messages)?),
            "Rig model step differs from the host request"
        );
        let mut executable: std::collections::BTreeSet<_> =
            request.tools.iter().map(|tool| tool.name.clone()).collect();
        // Note: delegation is a host control request. Qunica revalidates the
        // caller's bindings and active group membership before dispatch, and
        // returns structured unavailable feedback even if no helper is offered.
        // This does not admit unadvertised file, shell or MCP tools.
        executable.insert(crate::runtime::agent_as_tool::AGENT_AS_TOOL_NAME.to_string());
        // Note: ToolExecutor accepts these names for the same authorized shell.
        // Preserve its compatibility aliases without granting a missing tool.
        if ["Bash", "Pwsh", "Cmd", "Shell"]
            .iter()
            .any(|name| executable.contains(*name))
        {
            executable.extend(["Bash", "Pwsh", "Cmd", "Shell"].map(str::to_string));
        }
        self.policy = TurnPolicy::new(executable, None, None)?;
        self.checkpoint.attempts = self.checkpoint.attempts.saturating_add(1);
        self.checkpoint.attempt_id = Some(uuid::Uuid::new_v4().to_string());
        self.checkpoint.phase = AttemptPhase::Prepared;
        self.checkpoint.attempt_input_tokens = 0;
        self.checkpoint.attempt_output_tokens = 0;
        self.checkpoint.attempt_total_tokens = 0;
        self.report = None;
        Ok(())
    }

    pub async fn stream(&mut self, request: ChatRequest) -> anyhow::Result<Receiver<ChatDelta>> {
        let stream = Box::pin(self.provider.stream_completion(request)).await?;
        self.report = Some(stream.completion);
        self.checkpoint.phase = AttemptPhase::Streaming;
        Ok(stream.deltas)
    }

    pub fn observe_usage(&mut self, usage: &ContextUsage) {
        fn account(total: &mut u64, previous: &mut u64, next: Option<i64>) {
            if let Some(next) = next.and_then(|value| u64::try_from(value).ok()) {
                let next = next.max(*previous);
                *total = total.saturating_add(next - *previous);
                *previous = next;
            }
        }
        account(
            &mut self.checkpoint.input_tokens,
            &mut self.checkpoint.attempt_input_tokens,
            usage.input_tokens,
        );
        account(
            &mut self.checkpoint.output_tokens,
            &mut self.checkpoint.attempt_output_tokens,
            usage.output_tokens,
        );
        account(
            &mut self.checkpoint.total_tokens,
            &mut self.checkpoint.attempt_total_tokens,
            usage.total_tokens.or_else(|| {
                usage
                    .input_tokens
                    .zip(usage.output_tokens)
                    .map(|(a, b)| a.saturating_add(b))
            }),
        );
    }

    fn record(&mut self, report: &CompletionReport) -> anyhow::Result<()> {
        ensure!(
            !matches!(
                self.checkpoint.phase,
                AttemptPhase::CompletionRecorded
                    | AttemptPhase::StreamedTurnAccepted
                    | AttemptPhase::ToolsPending
                    | AttemptPhase::Done
            ),
            "duplicate Rig completion"
        );
        self.checkpoint.phase = AttemptPhase::TerminalReceived;
        let (usage, identity, finish, raw) = match report {
            CompletionReport::Response(response) => (
                response.usage,
                ResponseIdentity {
                    response_id: response.response_id().map(str::to_string),
                    provider_request_id: response.provider_request_id.clone(),
                },
                response.finish_reason(),
                response.raw.clone(),
            ),
            CompletionReport::Rejected(raw) => {
                let counters = &raw["response"]["usage"];
                let mut usage = Usage::default();
                usage.input_tokens = counters["input_tokens"].as_u64();
                usage.output_tokens = counters["output_tokens"].as_u64();
                usage.total_tokens = counters["total_tokens"].as_u64();
                (usage, ResponseIdentity::default(), None, raw.clone())
            }
        };
        let count = |value: Option<u64>| value.and_then(|value| i64::try_from(value).ok());
        self.observe_usage(&ContextUsage {
            input_tokens: count(usage.input_tokens),
            output_tokens: count(usage.output_tokens),
            total_tokens: count(usage.total_tokens),
            ..ContextUsage::default()
        });
        self.run
            .as_mut()
            .context("missing Rig run")?
            .record_streamed_completion_call(usage, identity, finish, raw)?;
        self.checkpoint.completion_calls = self.checkpoint.completion_calls.saturating_add(1);
        self.checkpoint.phase = AttemptPhase::CompletionRecorded;
        Ok(())
    }

    pub async fn accept(&mut self, message: &ChatMessage, terminal: bool) -> anyhow::Result<()> {
        ensure!(terminal, "Rig stream ended without Done");
        let report = self
            .report
            .take()
            .context("missing Rig completion receiver")?
            .await
            .context("Rig stream has no terminal metadata")?;
        self.record(&report)?;
        let CompletionReport::Response(response) = report else {
            bail!("rejected Rig turn cannot be accepted");
        };
        ensure!(
            matches!(
                response.finish_reason(),
                Some(FinishReason::Stop | FinishReason::ToolCalls)
            ),
            "Rig completion did not terminate successfully"
        );
        let projected = match response.message() {
            Some(Message::Assistant(turn)) => RigMessageAdapter::assistant_from_rig(&turn),
            // Note: an explicitly stopped completion may have no content. Rig
            // still decides the next step; do not invent text or mark it failed.
            None if response.finish_reason() == Some(FinishReason::Stop) => {
                ChatMessage::assistant_tool_calls("", Vec::new())
            }
            _ => bail!("Rig response has no assistant turn"),
        };
        ensure!(
            projected.content == message.content
                && projected.reasoning_content == message.reasoning_content
                && projected.reasoning_signature == message.reasoning_signature
                && projected.tool_calls == message.tool_calls,
            "Rig streamed display differs from its accepted model turn"
        );
        if self
            .max_tokens
            .is_some_and(|limit| self.window_tokens() > limit)
        {
            return Err(DriverBudgetExhausted("token").into());
        }
        self.run
            .as_mut()
            .context("missing Rig run")?
            .streamed_turn(StreamedTurn {
                head: response.head(),
                choice: response.choice.clone(),
                policy: self.policy.clone(),
                finish_reason: response.finish_reason(),
            })?;
        self.checkpoint.phase = AttemptPhase::StreamedTurnAccepted;
        match self.run.as_mut().unwrap().next_step()? {
            AgentRunStep::CallTools { calls } => {
                let mut executable = Vec::new();
                for call in calls {
                    match call {
                        PendingToolCall::Execute(call) => executable.push(call),
                        _ => bail!("Rig declined an executable tool call"),
                    }
                }
                ensure!(
                    executable.len() == message.tool_calls.len()
                        && executable
                            .iter()
                            .zip(&message.tool_calls)
                            .all(|(a, b)| a.id().wire() == b.id
                                && a.name().as_str() == b.name
                                && a.arguments() == b.args),
                    "Rig tool dispatch differs from the accepted batch"
                );
                self.pending = executable;
                self.checkpoint.phase = AttemptPhase::ToolsPending;
            }
            AgentRunStep::Done(_) => {
                ensure!(
                    message.tool_calls.is_empty(),
                    "Rig completed with unexecuted calls"
                );
                self.checkpoint.phase = AttemptPhase::Done;
            }
            AgentRunStep::CallModel { .. } => bail!("Rig requested an unapproved retry"),
        }
        Ok(())
    }

    pub fn abort(&mut self) {
        if matches!(
            self.checkpoint.phase,
            AttemptPhase::Aborted | AttemptPhase::ToolsPending | AttemptPhase::Done
        ) {
            return;
        }
        if let Some(report) = self
            .report
            .as_mut()
            .and_then(|receiver| receiver.try_recv().ok())
        {
            let _ = self.record(&report);
        }
        self.checkpoint.failed_attempts = self.checkpoint.failed_attempts.saturating_add(1);
        self.checkpoint.phase = AttemptPhase::Aborted;
        self.run = None;
        self.offered.clear();
        self.report = None;
    }
}

fn coalesce(messages: &[Message]) -> Vec<Message> {
    let mut normalized = Vec::new();
    for message in messages {
        match (normalized.last_mut(), message) {
            (Some(Message::User { content: previous }), Message::User { content }) => {
                previous.extend(content.clone())
            }
            _ => normalized.push(message.clone()),
        }
    }
    normalized
}

#[cfg(test)]
#[path = "agent_run_tests.rs"]
mod tests;
