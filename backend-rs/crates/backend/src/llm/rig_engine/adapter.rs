//! Two-way conversion between Qunica's [`ChatMessage`] and Rig's [`Message`].
//!
//! Qunica history stays the source of truth. A Rig assistant turn is stored
//! whole under `native_content["rig"]` next to its display fields, so it can
//! travel back to the provider with its origin, native items and signatures
//! intact.

use anyhow::{anyhow, bail, Context};
use base64::Engine as _;
use qunica_domain::runtime::ChatContentPart;
use rig_core::completion::{AssistantContent, AssistantMessage, Message};
use rig_core::message::{
    Api, CallId, ImageMediaType, MimeType, Opaque, Origin, Reasoning, Text,
    ToolCall as RigToolCall, ToolFunction, ToolName, ToolResultContent, UserContent,
};
use serde_json::{json, Value};

use super::super::{ChatMessage, ChatRequest, ToolCall};
use super::Protocol;

/// Key under which a Rig assistant turn is kept in `native_content`.
pub(crate) const NATIVE_KEY: &str = "rig";

/// Converts Qunica history to Rig messages and Rig turns back.
///
/// Neutral text, images and tools round-trip through the shared message types.
/// Provider history additionally imports native checkpoint signatures in the
/// selected provider's context. Rig turns retain their origin and native items.
/// Unrepresentable system images or image MIME types are errors.
pub struct RigMessageAdapter;

/// Legacy native checkpoints have provider blocks but no Rig origin. Import
/// them only on that provider's path; never relabel an existing Rig origin.
pub(super) fn provider_history(
    request: &ChatRequest,
    protocol: Protocol,
) -> anyhow::Result<Vec<Message>> {
    request
        .messages
        .iter()
        .map(|message| {
            let mut mapped = RigMessageAdapter::message_to_rig(message)?;
            if let Message::Assistant(turn) = &mut mapped {
                let has_rig = message
                    .native_content
                    .as_ref()
                    .and_then(|value| value.get(NATIVE_KEY))
                    .is_some();
                if !has_rig {
                    match protocol {
                        Protocol::Anthropic => import_anthropic(turn, message, &request.model)?,
                        Protocol::Gemini => import_gemini(turn, message, &request.model),
                        Protocol::Responses => import_responses(turn, message, &request.model)?,
                        Protocol::OpenAi => {}
                    }
                }
                if matches!(protocol, Protocol::Gemini) {
                    for block in &turn.content {
                        if let Some(signature) = block
                            .native_item()
                            .and_then(|item| item.get("thoughtSignature"))
                            .and_then(Value::as_str)
                        {
                            // Note: Rig silently strips malformed signatures. Refuse
                            // before I/O instead of sending an unsigned tool call.
                            base64::engine::general_purpose::STANDARD
                                .decode(signature)
                                .context("Gemini thoughtSignature is not standard Base64")?;
                        }
                    }
                }
            }
            Ok(mapped)
        })
        .collect()
}

fn import_anthropic(
    turn: &mut AssistantMessage,
    message: &ChatMessage,
    model: &str,
) -> anyhow::Result<()> {
    if let Some(blocks) = message
        .native_content
        .as_ref()
        .and_then(|value| value.get("anthropic"))
        .and_then(Value::as_array)
    {
        turn.content = blocks
            .iter()
            .map(anthropic_block)
            .collect::<anyhow::Result<_>>()?;
        if !same_display(&project(turn), message) {
            bail!("native Anthropic blocks disagree with the stored assistant message");
        }
    } else {
        for block in &mut turn.content {
            if let AssistantContent::Reasoning(reasoning) = block {
                if let Some(signature) = &message.reasoning_signature {
                    let text = reasoning.text.clone();
                    *block = block.clone().with_native(json!({
                        "type": "thinking", "thinking": text, "signature": signature
                    }));
                }
            }
        }
    }
    turn.origin = Some(Origin::new(
        Api::from_static("anthropic.messages"),
        "anthropic",
        model,
    ));
    Ok(())
}

fn import_gemini(turn: &mut AssistantMessage, message: &ChatMessage, model: &str) {
    let calls = turn
        .content
        .iter_mut()
        .filter(|block| matches!(block, AssistantContent::ToolCall(_)));
    for (block, call) in calls.zip(&message.tool_calls) {
        if let Some(signature) = call
            .provider_metadata
            .as_ref()
            .and_then(|value| value.get("thoughtSignature"))
            .and_then(Value::as_str)
        {
            *block = block.clone().with_native(json!({
                "functionCall": {"name": call.name, "args": call.args},
                "thoughtSignature": signature
            }));
        }
    }
    turn.origin = Some(Origin::new(
        Api::from_static("gemini.generate_content"),
        rig_core::providers::gemini::PROVIDER_NAME,
        model,
    ));
}

fn import_responses(
    turn: &mut AssistantMessage,
    message: &ChatMessage,
    model: &str,
) -> anyhow::Result<()> {
    // Note: native history keeps the completed encrypted items on the first
    // call. Import them before the call batch, without duplicating summaries.
    let mut content = Vec::new();
    for call in &message.tool_calls {
        if let Some(items) = call
            .provider_metadata
            .as_ref()
            .and_then(|value| value.get("openai_responses_reasoning"))
        {
            for item in items
                .as_array()
                .context("Responses reasoning metadata is not an array")?
            {
                if item["type"] != "reasoning" || !item["encrypted_content"].is_string() {
                    bail!("Responses replay needs a complete encrypted reasoning item");
                }
                let text = item["summary"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|part| part["text"].as_str())
                    .collect::<String>();
                content.push(
                    AssistantContent::Reasoning(Reasoning::new(text)).with_native(item.clone()),
                );
            }
        }
    }
    let encrypted = !content.is_empty();
    for block in turn
        .content
        .drain(..)
        .filter(|block| !matches!(block, AssistantContent::Reasoning(_)))
    {
        let item = if encrypted {
            match &block {
                AssistantContent::ToolCall(call) => Some(json!({
                    "type": "function_call", "call_id": call.id.wire(),
                    "name": call.function.name, "arguments": call.function.raw_arguments()
                })),
                AssistantContent::Text(text) => Some(json!({
                    "type": "message", "role": "assistant", "content": text.text
                })),
                _ => None,
            }
        } else {
            None
        };
        // Rig requires encrypted reasoning's following item to remain native.
        // These are the same neutral call/text items the native provider sends.
        content.push(match item {
            Some(item) => block.with_native(item),
            None => block,
        });
    }
    turn.content = content;
    turn.origin = Some(Origin::new(
        Api::from_static("openai.responses"),
        "openai-responses",
        model,
    ));
    Ok(())
}

fn anthropic_block(item: &Value) -> anyhow::Result<AssistantContent> {
    let block = match item
        .get("type")
        .and_then(Value::as_str)
        .context("native Anthropic block without type")?
    {
        "text" => AssistantContent::Text(Text::new(
            item["text"].as_str().context("text block without text")?,
        )),
        "thinking" => AssistantContent::Reasoning(Reasoning::new(
            item["thinking"]
                .as_str()
                .context("thinking block without text")?,
        )),
        "redacted_thinking" => AssistantContent::Reasoning(Reasoning {
            redacted: true,
            ..Reasoning::default()
        }),
        "tool_use" => AssistantContent::ToolCall(RigToolCall::from_wire(
            item["id"].as_str().context("tool block without id")?,
            tool_function(
                tool_name(item["name"].as_str().context("tool block without name")?)?,
                &item["input"],
            ),
        )),
        _ => {
            return Ok(AssistantContent::Opaque(Opaque {
                item: item.clone(),
                replay: true,
            }))
        }
    };
    Ok(block.with_native(item.clone()))
}

impl RigMessageAdapter {
    pub fn to_rig(messages: &[ChatMessage]) -> anyhow::Result<Vec<Message>> {
        messages.iter().map(Self::message_to_rig).collect()
    }

    pub fn message_to_rig(message: &ChatMessage) -> anyhow::Result<Message> {
        match message.role.as_str() {
            "system" => {
                if has_image(&message.parts) {
                    bail!("Rig system messages carry text only; this one has an image");
                }
                Ok(Message::System {
                    content: message.content.clone(),
                })
            }
            "user" => Ok(Message::User {
                content: user_content(message)?,
            }),
            "tool" => {
                let call = message
                    .tool_call_id
                    .as_deref()
                    .context("tool message without a tool_call_id")?;
                let name = tool_name(message.tool_name.as_deref().unwrap_or_default())?;
                Ok(Message::User {
                    content: vec![UserContent::tool_result(
                        CallId::from_wire(call),
                        name,
                        vec![ToolResultContent::text(message.content.clone())],
                    )],
                })
            }
            "assistant" => Ok(Message::Assistant(assistant_to_rig(message)?)),
            other => bail!("no Rig message for role {other:?}"),
        }
    }

    pub fn from_rig(message: &Message) -> anyhow::Result<ChatMessage> {
        match message {
            Message::System { content } => Ok(ChatMessage::text("system", content.clone())),
            Message::User { content } => user_from_rig(content),
            Message::Assistant(turn) => Ok(Self::assistant_from_rig(turn)),
        }
    }

    /// The Qunica message for a Rig assistant turn. The turn itself rides
    /// along in `native_content` when it carries anything the display fields
    /// lose: an origin, or a provider item.
    pub fn assistant_from_rig(turn: &AssistantMessage) -> ChatMessage {
        let mut message = project(turn);
        if turn.origin.is_some()
            || turn
                .content
                .iter()
                .any(|block| block.native_item().is_some())
        {
            message.native_content = serde_json::to_value(turn)
                .ok()
                .map(|turn| json!({ NATIVE_KEY: turn }));
        }
        message
    }
}

fn has_image(parts: &[ChatContentPart]) -> bool {
    parts
        .iter()
        .any(|part| matches!(part, ChatContentPart::Image { .. }))
}

fn tool_name(name: &str) -> anyhow::Result<ToolName> {
    ToolName::new(name).map_err(|_| anyhow!("tool call without a name"))
}

fn user_content(message: &ChatMessage) -> anyhow::Result<Vec<UserContent>> {
    if message.parts.is_empty() {
        return Ok(vec![UserContent::text(message.content.clone())]);
    }
    message
        .parts
        .iter()
        .map(|part| match part {
            ChatContentPart::Text { text } => Ok(UserContent::text(text.clone())),
            ChatContentPart::Image {
                mime_type,
                data_base64,
            } => {
                let media_type = ImageMediaType::from_mime_type(mime_type)
                    .filter(|known| known.to_mime_type() == mime_type)
                    .with_context(|| format!("Rig has no image type {mime_type:?}"))?;
                Ok(UserContent::image_base64(
                    data_base64.clone(),
                    Some(media_type),
                    None,
                ))
            }
        })
        .collect()
}

fn user_from_rig(content: &[UserContent]) -> anyhow::Result<ChatMessage> {
    if let [UserContent::ToolResult(result)] = content {
        let text = result
            .content
            .iter()
            .map(|part| match part {
                ToolResultContent::Text(text) => Ok(text.text.clone()),
                ToolResultContent::Json { value } => Ok(value.to_string()),
                ToolResultContent::Image(_) => bail!("Qunica tool results carry text only"),
            })
            .collect::<anyhow::Result<Vec<_>>>()?
            .join("\n");
        return Ok(ChatMessage::tool_result(
            result.call.wire(),
            result.name.as_str(),
            text,
        ));
    }
    let parts = content
        .iter()
        .map(|part| match part {
            UserContent::Text(text) => Ok(ChatContentPart::text(text.text.clone())),
            UserContent::Image(image) => {
                let rig_core::message::DocumentSourceKind::Base64(data) = &image.data else {
                    bail!("Qunica images are inline base64");
                };
                let mime = image
                    .media_type
                    .as_ref()
                    .context("image without a media type")?
                    .to_mime_type();
                Ok(ChatContentPart::image(mime, data.clone()))
            }
            _ => bail!("Qunica user messages carry text and images only"),
        })
        .collect::<anyhow::Result<Vec<_>>>()?;
    // A lone text part is plain text: that is how Qunica writes it.
    if let [ChatContentPart::Text { text }] = parts.as_slice() {
        return Ok(ChatMessage::text("user", text.clone()));
    }
    Ok(ChatMessage::with_parts("user", parts))
}

fn assistant_to_rig(message: &ChatMessage) -> anyhow::Result<AssistantMessage> {
    // Note: the stored turn is replayed only while it still says what the
    // display fields say. A message edited since (compaction, a host rewrite)
    // is rebuilt from those fields; its stale signatures would not verify.
    if let Some(raw) = message
        .native_content
        .as_ref()
        .and_then(|native| native.get(NATIVE_KEY))
    {
        let stored = serde_json::from_value::<AssistantMessage>(raw.clone())
            .context("invalid persisted Rig assistant turn")?;
        if same_display(&project(&stored), message) {
            return Ok(stored);
        }
    }
    let mut content = Vec::new();
    if let Some(reasoning) = message.reasoning_content.as_deref() {
        content.push(AssistantContent::Reasoning(Reasoning::new(reasoning)));
    }
    if !message.content.is_empty() {
        content.push(AssistantContent::Text(Text::new(message.content.clone())));
    }
    for call in &message.tool_calls {
        content.push(AssistantContent::ToolCall(RigToolCall::from_wire(
            call.id.clone(),
            tool_function(tool_name(&call.name)?, &call.args),
        )));
    }
    Ok(AssistantMessage::new(content))
}

/// Arguments that are not an object travel as their JSON text, so a `null`
/// or a string reads back as itself rather than as `{}`.
fn tool_function(name: ToolName, args: &Value) -> ToolFunction {
    match args {
        Value::Object(arguments) => ToolFunction {
            name,
            arguments: arguments.clone(),
            invalid_arguments: None,
        },
        other => ToolFunction {
            name,
            arguments: serde_json::Map::new(),
            invalid_arguments: Some(other.to_string()),
        },
    }
}

/// What a Rig turn shows: the display fields of its Qunica message.
fn project(turn: &AssistantMessage) -> ChatMessage {
    let mut text = String::new();
    let mut reasoning = String::new();
    let mut calls = Vec::new();
    for block in &turn.content {
        match block {
            AssistantContent::Text(part) => text.push_str(&part.text),
            AssistantContent::Reasoning(part) => reasoning.push_str(&part.text),
            AssistantContent::ToolCall(call) => calls.push(ToolCall {
                id: call.id.wire().into_owned(),
                name: call.function.name.as_str().to_string(),
                // The model's text when it was not an object, as the native
                // providers read it: unparsable text is `null`.
                args: match &call.function.invalid_arguments {
                    Some(raw) => serde_json::from_str(raw).unwrap_or(Value::Null),
                    None => call.function.arguments_value(),
                },
                provider_metadata: block
                    .native_item()
                    .and_then(|value| value.get("thoughtSignature"))
                    .map(|signature| json!({"thoughtSignature": signature})),
            }),
            _ => {}
        }
    }
    let responses_reasoning: Vec<_> = turn
        .content
        .iter()
        .filter_map(AssistantContent::native_item)
        .filter(|item| item["type"] == "reasoning" && item["encrypted_content"].is_string())
        .cloned()
        .collect();
    if !responses_reasoning.is_empty() {
        if let Some(call) = calls.first_mut() {
            let metadata = call.provider_metadata.get_or_insert_with(|| json!({}));
            metadata["openai_responses_reasoning"] = json!(responses_reasoning);
        }
    }
    let signature = turn
        .content
        .iter()
        .filter(|block| matches!(block, AssistantContent::Reasoning(_)))
        .filter_map(|block| block.native_item()?.get("signature")?.as_str())
        .next_back()
        .map(str::to_string);
    ChatMessage::assistant_tool_calls(text, calls)
        .with_reasoning(reasoning)
        .with_reasoning_signature(signature)
}

fn same_display(projected: &ChatMessage, message: &ChatMessage) -> bool {
    projected.content == message.content
        && projected.reasoning_content == message.reasoning_content
        && projected.reasoning_signature == message.reasoning_signature
        && projected.tool_calls.len() == message.tool_calls.len()
        && projected
            .tool_calls
            .iter()
            .zip(&message.tool_calls)
            .all(|(a, b)| {
                a.id == b.id
                    && a.name == b.name
                    && a.args == b.args
                    && a.provider_metadata == b.provider_metadata
            })
}

#[cfg(test)]
mod tests {
    use super::*;
    use rig_core::message::{Api, Origin};

    #[test]
    fn changing_a_signature_invalidates_the_stored_turn() {
        let turn = AssistantMessage::new(vec![AssistantContent::Reasoning(Reasoning::new("plan"))
            .with_native(
                json!({"type": "thinking", "thinking": "plan", "signature": "original"}),
            )])
        .with_origin(Origin::new(
            Api::from_static("anthropic.messages"),
            "anthropic",
            "model",
        ));
        let mut message = RigMessageAdapter::assistant_from_rig(&turn);
        message.reasoning_signature = Some("edited".into());
        let Message::Assistant(rebuilt) = RigMessageAdapter::message_to_rig(&message).unwrap()
        else {
            panic!("assistant");
        };
        assert!(rebuilt.origin.is_none());
        assert!(rebuilt
            .content
            .iter()
            .all(|block| block.native_item().is_none()));
    }

    #[test]
    fn corrupt_persisted_rig_content_is_an_error() {
        let mut message = ChatMessage::text("assistant", "answer");
        message.native_content = Some(json!({"rig": {"content": "corrupt"}}));
        assert!(RigMessageAdapter::message_to_rig(&message).is_err());
    }

    fn call(id: &str, name: &str, args: Value) -> ToolCall {
        ToolCall {
            id: id.to_string(),
            name: name.to_string(),
            args,
            provider_metadata: None,
        }
    }

    fn round_trip(message: &ChatMessage) -> ChatMessage {
        let rig = RigMessageAdapter::message_to_rig(message).unwrap();
        RigMessageAdapter::from_rig(&rig).unwrap()
    }

    #[test]
    fn qunica_messages_survive_a_round_trip_through_rig() {
        let messages = [
            ChatMessage::text("system", "be brief"),
            ChatMessage::text("user", "hi"),
            ChatMessage::with_parts(
                "user",
                vec![
                    ChatContentPart::text("describe"),
                    ChatContentPart::image("image/png", "AQID"),
                ],
            ),
            ChatMessage::text("assistant", "hello"),
            ChatMessage::assistant_tool_calls(
                "",
                vec![
                    call("call_1", "Read", json!({"file_path": "a.txt"})),
                    call("call_2", "Bad", Value::Null),
                    call("call_3", "Odd", json!("text")),
                ],
            )
            .with_reasoning("weighing it"),
            ChatMessage::tool_result("call_1", "Read", "file body"),
        ];
        for message in &messages {
            assert_eq!(&round_trip(message), message);
        }
    }

    #[test]
    fn a_rig_turn_survives_a_round_trip_through_qunica() {
        let signed = AssistantContent::Reasoning(Reasoning::new("plan"))
            .with_native(json!({"type": "thinking", "thinking": "plan", "signature": "sig"}));
        let turn = AssistantMessage::new(vec![
            signed,
            AssistantContent::Text(Text::new("on it")),
            AssistantContent::tool_call("toolu_1", ToolName::new("Read").unwrap(), json!({"p": 1})),
        ])
        .with_origin(Origin::new(
            Api::from_static("anthropic.messages"),
            "anthropic",
            "claude-x",
        ));

        let message = RigMessageAdapter::assistant_from_rig(&turn);
        assert_eq!(message.content, "on it");
        assert_eq!(message.reasoning_content.as_deref(), Some("plan"));
        assert_eq!(message.reasoning_signature.as_deref(), Some("sig"));
        assert_eq!(
            message.tool_calls,
            vec![call("toolu_1", "Read", json!({"p": 1}))]
        );

        let Message::Assistant(back) = RigMessageAdapter::message_to_rig(&message).unwrap() else {
            panic!("assistant turn expected");
        };
        assert_eq!(back, turn);
    }

    #[test]
    fn an_edited_message_is_rebuilt_instead_of_replaying_its_stale_turn() {
        let turn = AssistantMessage::new(vec![AssistantContent::Text(Text::new("long answer"))])
            .with_origin(Origin::new(Api::from_static("openai.chat"), "x", "m"));
        let mut message = RigMessageAdapter::assistant_from_rig(&turn);
        assert!(message.native_content.is_some());
        message.content = "short".to_string();

        let Message::Assistant(back) = RigMessageAdapter::message_to_rig(&message).unwrap() else {
            panic!("assistant turn expected");
        };
        assert!(back.origin.is_none());
        assert_eq!(back.text(), "short");
    }

    #[test]
    fn what_rig_cannot_express_is_an_error_not_a_drop() {
        let system_image =
            ChatMessage::with_parts("system", vec![ChatContentPart::image("image/png", "AQID")]);
        assert!(RigMessageAdapter::message_to_rig(&system_image).is_err());
        let alias = ChatMessage::with_parts("user", vec![ChatContentPart::image("image/jpg", "x")]);
        assert!(RigMessageAdapter::message_to_rig(&alias).is_err());
        let unnamed = ChatMessage::assistant_tool_calls("", vec![call("c", "", json!({}))]);
        assert!(RigMessageAdapter::message_to_rig(&unnamed).is_err());
    }
}
