//! Two-way conversion between Qunica's [`ChatMessage`] and Rig's [`Message`].
//!
//! Qunica history stays the source of truth. A Rig assistant turn is stored
//! whole under `native_content["rig"]` next to its display fields, so it can
//! travel back to the provider with its origin, native items and signatures
//! intact.

use anyhow::{anyhow, bail, Context};
use qunica_domain::runtime::ChatContentPart;
use rig_core::completion::{AssistantContent, AssistantMessage, Message};
use rig_core::message::{
    CallId, ImageMediaType, MimeType, Reasoning, Text, ToolCall as RigToolCall, ToolFunction,
    ToolName, ToolResultContent, UserContent,
};
use serde_json::{json, Value};

use super::super::{ChatMessage, ToolCall};

/// Key under which a Rig assistant turn is kept in `native_content`.
pub(crate) const NATIVE_KEY: &str = "rig";

/// Converts Qunica history to Rig messages and Rig turns back.
///
/// Lossless in both directions for what each side can express:
/// `from_rig(to_rig(m)) == m` for every message Qunica writes, and a Rig turn
/// converted to Qunica and back is the same turn, origin and native items
/// included. What Rig cannot express (an image in a system message, an image
/// type Rig does not name) is an error, never a silent drop.
pub struct RigMessageAdapter;

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
    if let Some(stored) = message
        .native_content
        .as_ref()
        .and_then(|native| native.get(NATIVE_KEY))
        .and_then(|turn| serde_json::from_value::<AssistantMessage>(turn.clone()).ok())
    {
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
                provider_metadata: None,
            }),
            _ => {}
        }
    }
    ChatMessage::assistant_tool_calls(text, calls).with_reasoning(reasoning)
}

fn same_display(projected: &ChatMessage, message: &ChatMessage) -> bool {
    projected.content == message.content
        && projected.reasoning_content == message.reasoning_content
        && projected.tool_calls.len() == message.tool_calls.len()
        && projected
            .tool_calls
            .iter()
            .zip(&message.tool_calls)
            .all(|(a, b)| a.id == b.id && a.name == b.name && a.args == b.args)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rig_core::message::{Api, Origin};

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
