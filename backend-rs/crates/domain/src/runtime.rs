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
    /// The provider's cryptographic signature over [`Self::reasoning_content`],
    /// for providers that sign the thinking they emit.
    ///
    /// Anthropic signs every thinking block and verifies the signature when the
    /// block travels back, which it requires for the assistant turn that made
    /// the tool calls. Reasoning replayed from stored history has no signature,
    /// which is how a provider tells "thinking from this turn" (send it back)
    /// from "thinking recovered from the database" (do not).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_signature: Option<String>,
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
            reasoning_signature: None,
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

    /// Attach the provider's signature over the reasoning already on this
    /// message.
    ///
    /// A signature without the thinking it signs is useless — the provider
    /// verifies one against the other — so it is only kept when there is
    /// reasoning here to sign.
    #[must_use]
    pub fn with_reasoning_signature(mut self, signature: Option<String>) -> Self {
        let Some(signature) = signature.filter(|value| !value.trim().is_empty()) else {
            return self;
        };
        if self.reasoning_content.is_some() {
            self.reasoning_signature = Some(signature);
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
            reasoning_signature: None,
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
            reasoning_signature: None,
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
            reasoning_signature: None,
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
    /// A neutral five-level abstraction, not a passthrough: the providers
    /// spell this very differently — an enum for OpenAI, a token budget for
    /// Anthropic, a nested config for Gemini — so each maps it itself.
    /// `None` means the key is omitted entirely rather than sent as null.
    #[serde(default)]
    pub reasoning_effort: Option<ReasoningEffort>,
}

/// Reasoning depth, in terms every supported provider can express.
///
/// The two deepest levels are levels of their own rather than aliases of
/// [`Self::High`]: an agent set to `max` asks for markedly more thinking than
/// one set to `high`, and collapsing them made the deepest settings in the
/// agent form change nothing about the request they configure.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ReasoningEffort {
    Low,
    Medium,
    High,
    XHigh,
    Max,
}

impl ReasoningEffort {
    /// Every level, shallowest first — the vocabulary the API and the agent
    /// form share.
    pub const ALL: [Self; 5] = [Self::Low, Self::Medium, Self::High, Self::XHigh, Self::Max];

    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim() {
            "low" => Some(Self::Low),
            "medium" => Some(Self::Medium),
            "high" => Some(Self::High),
            "xhigh" => Some(Self::XHigh),
            "max" => Some(Self::Max),
            _ => None,
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
            Self::XHigh => "xhigh",
            Self::Max => "max",
        }
    }

    /// A thinking-token budget for providers that take one.
    ///
    /// Callers must keep the result below the request's `max_tokens`; the
    /// provider rejects a budget that is not. The ceiling is deliberately
    /// 24576 rather than something larger: it is the deepest budget every
    /// provider that takes one accepts (Gemini 2.5 Flash caps there, and
    /// Anthropic still has room for the answer itself under the smallest
    /// output limit of a thinking-capable Claude model).
    pub const fn thinking_budget_tokens(self) -> i64 {
        match self {
            Self::Low => 2048,
            Self::Medium => 4096,
            Self::High => 8192,
            Self::XHigh => 16384,
            Self::Max => 24576,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ChatDelta {
    Token(String),
    Reasoning(String),
    /// The provider's signature over the thinking it just streamed.
    ///
    /// Arrives after the reasoning it signs, and only from providers that sign
    /// their thinking. A consumer that replays the reasoning has to carry this
    /// with it: Anthropic verifies the signature against the thinking text and
    /// rejects a block whose signature is missing or does not match.
    ReasoningSignature(String),
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
    #[serde(default)]
    pub cached_input_tokens: Option<i64>,
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
