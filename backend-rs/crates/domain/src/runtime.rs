use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ChatContentPart {
    Text {
        text: String,
    },
    Image {
        mime_type: String,
        data_base64: String,
    },
}

impl ChatContentPart {
    pub fn text(text: impl Into<String>) -> Self {
        Self::Text { text: text.into() }
    }

    pub fn image(mime_type: impl Into<String>, data_base64: impl Into<String>) -> Self {
        Self::Image {
            mime_type: mime_type.into(),
            data_base64: data_base64.into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub parts: Vec<ChatContentPart>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_calls: Vec<ToolCall>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_name: Option<String>,
    /// The thinking that produced this assistant message, when the model
    /// surfaced any.
    ///
    /// Kept beside the content rather than folded into it: a provider in
    /// thinking mode treats reasoning as its own field, and some require the
    /// reasoning behind a tool call to travel back with the call on the next
    /// turn. Whether it is actually sent is the provider's decision — see the
    /// per-model `reasoning_passback` setting.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_content: Option<String>,
}

impl ChatMessage {
    pub fn text(role: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            role: role.into(),
            content: content.into(),
            parts: Vec::new(),
            tool_calls: Vec::new(),
            tool_call_id: None,
            tool_name: None,
            reasoning_content: None,
        }
    }

    /// Attach the reasoning that produced this message. Blank text is dropped:
    /// an empty `reasoning_content` is not something a provider wants back.
    #[must_use]
    pub fn with_reasoning(mut self, reasoning: impl Into<String>) -> Self {
        let reasoning = reasoning.into();
        if !reasoning.trim().is_empty() {
            self.reasoning_content = Some(reasoning);
        }
        self
    }

    pub fn with_parts(role: impl Into<String>, parts: Vec<ChatContentPart>) -> Self {
        let content = parts
            .iter()
            .filter_map(|part| match part {
                ChatContentPart::Text { text } => Some(text.as_str()),
                ChatContentPart::Image { .. } => None,
            })
            .collect::<String>();
        Self {
            role: role.into(),
            content,
            parts,
            tool_calls: Vec::new(),
            tool_call_id: None,
            tool_name: None,
            reasoning_content: None,
        }
    }

    pub fn assistant_tool_calls(content: impl Into<String>, tool_calls: Vec<ToolCall>) -> Self {
        Self {
            role: "assistant".to_string(),
            content: content.into(),
            parts: Vec::new(),
            tool_calls,
            tool_call_id: None,
            tool_name: None,
            reasoning_content: None,
        }
    }

    pub fn tool_result(
        tool_call_id: impl Into<String>,
        tool_name: impl Into<String>,
        content: impl Into<String>,
    ) -> Self {
        Self {
            role: "tool".to_string(),
            content: content.into(),
            parts: Vec::new(),
            tool_calls: Vec::new(),
            tool_call_id: Some(tool_call_id.into()),
            tool_name: Some(tool_name.into()),
            reasoning_content: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatRequest {
    pub model: String,
    pub messages: Vec<ChatMessage>,
    pub temperature: Option<f32>,
    pub reasoning_passback: bool,
    #[serde(default)]
    pub include_empty_tools: bool,
    pub tools: Vec<ToolDefinition>,
    /// How hard the model should think before answering.
    ///
    /// A neutral three-level abstraction, not a passthrough: the providers
    /// spell this very differently — an enum for OpenAI, a token budget for
    /// Anthropic, a nested config for Gemini — so each maps it itself.
    /// `None` means the key is omitted entirely rather than sent as null.
    #[serde(default)]
    pub reasoning_effort: Option<ReasoningEffort>,
}

/// Reasoning depth, in terms every supported provider can express.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ReasoningEffort {
    Low,
    Medium,
    High,
}

impl ReasoningEffort {
    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim() {
            "low" => Some(Self::Low),
            "medium" => Some(Self::Medium),
            "high" => Some(Self::High),
            _ => None,
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
        }
    }

    /// A thinking-token budget for providers that take one.
    ///
    /// Callers must keep the result below the request's `max_tokens`; the
    /// provider rejects a budget that is not.
    pub const fn thinking_budget_tokens(self) -> i64 {
        match self {
            Self::Low => 1024,
            Self::Medium => 2048,
            Self::High => 3072,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ChatDelta {
    Token(String),
    Reasoning(String),
    ToolCall(ToolCall),
    Usage(ContextUsage),
    /// The provider connection ended in the middle of the response, so whatever
    /// arrived is incomplete. This is distinct from [`ChatDelta::Done`]: a
    /// consumer must treat the round as failed rather than as an answer, or a
    /// dropped connection reads as a model that simply stopped talking.
    Truncated(String),
    Done,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub args: serde_json::Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_metadata: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    pub input_schema: serde_json::Value,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ContextUsage {
    pub input_tokens: Option<i64>,
    pub output_tokens: Option<i64>,
    pub total_tokens: Option<i64>,
    #[serde(default)]
    pub context_window_tokens: Option<i64>,
    #[serde(default)]
    pub output_reserve_tokens: Option<i64>,
    #[serde(default)]
    pub ratio: Option<f64>,
    #[serde(default)]
    pub source: Option<String>,
}
