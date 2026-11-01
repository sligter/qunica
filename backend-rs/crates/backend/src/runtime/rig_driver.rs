//! Host-owned attempt/checkpoint boundary around Rig's sans-I/O AgentRun.
//! Tools, approvals, compaction and cancellation remain in the group host.

use crate::llm::{
    build_provider, ChatMessage, ChatRequest, ContextUsage, LlmProvider, ProviderConfig,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

mod agent_run;

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum AttemptPhase {
    #[default]
    Prepared,
    Streaming,
    TerminalReceived,
    CompletionRecorded,
    StreamedTurnAccepted,
    ToolsPending,
    Done,
    Aborted,
}

/// Stable Qunica data; no AgentRun snapshot or provider-private run format.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default)]
pub(crate) struct DriverCheckpoint {
    pub attempt_id: Option<String>,
    pub phase: AttemptPhase,
    pub attempts: u64,
    pub failed_attempts: u64,
    pub completion_calls: u64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub total_tokens: u64,
    pub attempt_input_tokens: u64,
    pub attempt_output_tokens: u64,
    pub attempt_total_tokens: u64,
}

#[derive(Debug, thiserror::Error)]
#[error("Rig {0} budget exhausted")]
pub(crate) struct DriverBudgetExhausted(pub &'static str);

pub(crate) fn validate_config(config: Option<&serde_json::Value>) -> anyhow::Result<()> {
    if let Some(engine) = config.and_then(|value| value.get("engine")) {
        anyhow::ensure!(
            matches!(engine.as_str(), Some("native" | "rig")),
            "llm_config.engine must be native or rig"
        );
    }
    // Note: native is a saved-config alias, never a new execution path.
    if let Some(limits) = config.and_then(|value| value.get("rig_limits")) {
        let limits = limits
            .as_object()
            .ok_or_else(|| anyhow::anyhow!("rig_limits must be an object"))?;
        for (key, value) in limits {
            anyhow::ensure!(
                matches!(
                    key.as_str(),
                    "max_model_calls" | "max_failed_attempts" | "max_total_tokens"
                ) && value.as_u64().is_some_and(|limit| limit > 0),
                "invalid rig_limits.{key}"
            );
        }
    }
    Ok(())
}

pub(crate) fn validate_resume_engine(
    runtime_kind: &str,
    config: Option<&str>,
    content: Option<&str>,
) -> anyhow::Result<()> {
    let content: serde_json::Value = content
        .map(serde_json::from_str)
        .transpose()?
        .unwrap_or_default();
    let recorded_rig = content
        .get("driver_checkpoint")
        .is_some_and(|value| !value.is_null());
    let recorded_host = content
        .get("assistant_turns")
        .and_then(serde_json::Value::as_array)
        .is_some_and(|turns| !turns.is_empty());
    if recorded_rig || recorded_host {
        let config: serde_json::Value = config
            .map(serde_json::from_str)
            .transpose()?
            .unwrap_or_default();
        anyhow::ensure!(
            runtime_kind == "llm_chat",
            "This recorded model execution requires llm_chat before resuming."
        );
        validate_config(Some(&config))?;
    }
    Ok(())
}

pub(crate) enum TurnEngine {
    LegacyRecovery,
    Rig(Box<agent_run::RigDriver>),
}

pub(crate) fn configured_engine(config: Option<&str>) -> anyhow::Result<&'static str> {
    let config: serde_json::Value = config
        .map(serde_json::from_str)
        .transpose()?
        .unwrap_or_default();
    validate_config(Some(&config))?;
    Ok("rig")
}

impl TurnEngine {
    pub fn name(&self) -> &'static str {
        match self {
            Self::LegacyRecovery => "native",
            Self::Rig(_) => "rig",
        }
    }
    pub fn build(
        config: Option<&str>,
        provider: &ProviderConfig,
        legacy_resume: bool,
    ) -> anyhow::Result<(Self, Arc<dyn LlmProvider>)> {
        let config: serde_json::Value = config
            .map(serde_json::from_str)
            .transpose()?
            .unwrap_or_default();
        validate_config(Some(&config))?;
        // Note: only a recorded legacy continuation may bypass AgentRun. Its
        // original budgets and native attribution cannot be invented from history.
        // Both paths use Rig's sole transport. See RIG_MIGRATION.md#p5-cleanup-and-native-retirement.
        if legacy_resume {
            return Ok((Self::LegacyRecovery, Arc::from(build_provider(provider)?)));
        }
        let provider = Arc::new(crate::llm::rig_engine::RigProvider::from_config(provider)?);
        let driver = agent_run::RigDriver::new(provider.clone(), &config)?;
        Ok((Self::Rig(Box::new(driver)), provider))
    }

    pub fn restore(&mut self, checkpoint: Option<&DriverCheckpoint>) -> anyhow::Result<()> {
        // Note: a config change must not reinterpret an accepted Rig exchange
        // or reset its budget. See RIG_MIGRATION.md#p3-host-driven-agentrun.
        match self {
            Self::LegacyRecovery => anyhow::ensure!(
                checkpoint.is_none(),
                "A Rig checkpoint cannot enter legacy recovery."
            ),
            Self::Rig(driver) => driver.restore(checkpoint),
        }
        Ok(())
    }

    pub fn checkpoint(&self) -> Option<DriverCheckpoint> {
        match self {
            Self::LegacyRecovery => None,
            Self::Rig(driver) => Some(driver.checkpoint.clone()),
        }
    }

    pub fn settle(&mut self, messages: &[ChatMessage]) -> anyhow::Result<()> {
        if let Self::Rig(driver) = self {
            driver.settle(messages)?;
        }
        Ok(())
    }

    pub fn prepare(&mut self, request: &ChatRequest) -> anyhow::Result<()> {
        match self {
            Self::LegacyRecovery => {}
            Self::Rig(driver) => driver.prepare(request)?,
        }
        Ok(())
    }

    pub async fn stream(
        &mut self,
        provider: &dyn LlmProvider,
        request: ChatRequest,
    ) -> anyhow::Result<tokio::sync::mpsc::Receiver<crate::llm::ChatDelta>> {
        match self {
            Self::LegacyRecovery => provider.stream(request).await,
            Self::Rig(driver) => Box::pin(driver.stream(request)).await,
        }
    }

    pub fn observe_usage(&mut self, usage: &ContextUsage) {
        if let Self::Rig(driver) = self {
            driver.observe_usage(usage);
        }
    }

    pub async fn accept(&mut self, message: &ChatMessage, terminal: bool) -> anyhow::Result<()> {
        if let Self::Rig(driver) = self {
            Box::pin(driver.accept(message, terminal)).await?;
        }
        Ok(())
    }

    pub fn abort(&mut self) {
        if let Self::Rig(driver) = self {
            driver.abort();
        }
    }
}
