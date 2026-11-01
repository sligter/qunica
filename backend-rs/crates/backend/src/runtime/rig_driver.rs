//! Host-owned attempt/checkpoint boundary around Rig's sans-I/O AgentRun.
//! Tools, approvals, compaction and cancellation remain in the group host.

use crate::llm::{
    build_provider, ChatMessage, ChatRequest, ContextUsage, LlmProvider, ProviderConfig,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[cfg(feature = "rig-engine")]
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
    let Some(engine) = config.and_then(|value| value.get("engine")) else {
        return Ok(());
    };
    match engine.as_str() {
        Some("native") => Ok(()),
        Some("rig") => {
            anyhow::ensure!(
                cfg!(feature = "rig-engine"),
                "the rig engine requires a build with the rig-engine feature"
            );
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
        _ => anyhow::bail!("llm_config.engine must be native or rig"),
    }
}

pub(crate) enum TurnEngine {
    Native,
    #[cfg(feature = "rig-engine")]
    Rig(Box<agent_run::RigDriver>),
}

impl TurnEngine {
    pub fn build(
        config: Option<&str>,
        provider: &ProviderConfig,
    ) -> anyhow::Result<(Self, Arc<dyn LlmProvider>)> {
        let config: serde_json::Value = config
            .map(serde_json::from_str)
            .transpose()?
            .unwrap_or_default();
        validate_config(Some(&config))?;
        let engine = match config.get("engine") {
            None => "native",
            Some(value) => value
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("llm_config.engine must be native or rig"))?,
        };
        match engine {
            "native" => Ok((Self::Native, Arc::from(build_provider(provider)?))),
            "rig" => {
                #[cfg(feature = "rig-engine")]
                {
                    let provider =
                        Arc::new(crate::llm::rig_engine::RigProvider::from_config(provider)?);
                    let driver = agent_run::RigDriver::new(provider.clone(), &config)?;
                    Ok((Self::Rig(Box::new(driver)), provider))
                }
                #[cfg(not(feature = "rig-engine"))]
                anyhow::bail!("the rig engine requires a build with the rig-engine feature")
            }
            _ => anyhow::bail!("llm_config.engine must be native or rig"),
        }
    }

    pub fn restore(&mut self, checkpoint: Option<&DriverCheckpoint>) {
        #[cfg(feature = "rig-engine")]
        if let Self::Rig(driver) = self {
            driver.restore(checkpoint);
        }
        #[cfg(not(feature = "rig-engine"))]
        let _ = checkpoint;
    }

    pub fn checkpoint(&self) -> Option<DriverCheckpoint> {
        match self {
            Self::Native => None,
            #[cfg(feature = "rig-engine")]
            Self::Rig(driver) => Some(driver.checkpoint.clone()),
        }
    }

    pub fn settle(&mut self, messages: &[ChatMessage]) -> anyhow::Result<()> {
        #[cfg(feature = "rig-engine")]
        if let Self::Rig(driver) = self {
            driver.settle(messages)?;
        }
        let _ = messages;
        Ok(())
    }

    pub fn prepare(&mut self, request: &ChatRequest) -> anyhow::Result<()> {
        #[cfg(feature = "rig-engine")]
        if let Self::Rig(driver) = self {
            driver.prepare(request)?;
        }
        let _ = request;
        Ok(())
    }

    pub async fn stream(
        &mut self,
        provider: &dyn LlmProvider,
        request: ChatRequest,
    ) -> anyhow::Result<tokio::sync::mpsc::Receiver<crate::llm::ChatDelta>> {
        match self {
            Self::Native => provider.stream(request).await,
            #[cfg(feature = "rig-engine")]
            Self::Rig(driver) => Box::pin(driver.stream(request)).await,
        }
    }

    pub fn observe_usage(&mut self, usage: &ContextUsage) {
        #[cfg(feature = "rig-engine")]
        if let Self::Rig(driver) = self {
            driver.observe_usage(usage);
        }
        let _ = usage;
    }

    pub async fn accept(&mut self, message: &ChatMessage, terminal: bool) -> anyhow::Result<()> {
        #[cfg(feature = "rig-engine")]
        if let Self::Rig(driver) = self {
            Box::pin(driver.accept(message, terminal)).await?;
        }
        let _ = (message, terminal);
        Ok(())
    }

    pub fn abort(&mut self) {
        #[cfg(feature = "rig-engine")]
        if let Self::Rig(driver) = self {
            driver.abort();
        }
    }
}
