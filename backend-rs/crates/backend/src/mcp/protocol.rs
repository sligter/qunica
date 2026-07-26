//! MCP JSON-RPC 2.0 message shapes and the tool subset this client speaks.
//!
//! Every transport carries the same envelope; only the framing differs. This
//! module owns the envelope, the three method payloads the runtime needs
//! (`initialize`, `tools/list`, `tools/call`), and the reduction of a
//! `tools/call` result's `content[]` blocks into the single text string the
//! group runtime hands back to a model.

use serde_json::{json, Map, Value};

use super::McpError;

/// JSON-RPC version string on every message.
pub const JSONRPC_VERSION: &str = "2.0";

/// The MCP revision this client advertises.
///
/// Servers that speak a newer revision answer `initialize` with their own
/// version; the client accepts whatever comes back rather than insisting on a
/// match, because the tool subset used here has been stable across revisions.
pub const PROTOCOL_VERSION: &str = "2025-06-18";

/// `initialize` request method.
pub const METHOD_INITIALIZE: &str = "initialize";
/// `notifications/initialized` notification method.
pub const METHOD_INITIALIZED: &str = "notifications/initialized";
/// `tools/list` request method.
pub const METHOD_TOOLS_LIST: &str = "tools/list";
/// `tools/call` request method.
pub const METHOD_TOOLS_CALL: &str = "tools/call";

/// JSON-RPC error code signalling the peer does not implement a method.
pub const JSONRPC_METHOD_NOT_FOUND: i64 = -32601;

/// Largest tool-call output forwarded to a model, in characters.
///
/// An MCP server can return an arbitrarily large document; without a cap one
/// call could exhaust the agent's context window.
pub const MAX_TOOL_OUTPUT_CHARS: usize = 30_000;

/// Largest number of tools accepted from one server.
pub const MAX_TOOLS_PER_SERVER: usize = 200;

/// A tool as described by `tools/list`.
#[derive(Debug, Clone, PartialEq)]
pub struct McpToolInfo {
    /// Server-side tool name (not yet namespaced).
    pub name: String,
    /// Human-readable description, empty when the server omits one.
    pub description: String,
    /// JSON Schema for the tool's arguments.
    pub input_schema: Value,
}

/// Build the `initialize` request params.
pub fn initialize_params(client_name: &str, client_version: &str) -> Value {
    json!({
        "protocolVersion": PROTOCOL_VERSION,
        // No resources, prompts, sampling or roots are wired through yet, so the
        // client claims none of them.
        "capabilities": {},
        "clientInfo": {
            "name": client_name,
            "version": client_version,
        },
    })
}

/// Build the `tools/call` request params.
pub fn tools_call_params(tool: &str, arguments: &Value) -> Value {
    // MCP requires `arguments` to be an object. Models occasionally emit `null`
    // for a no-argument tool, and some emit a bare scalar; normalize both to an
    // empty object rather than letting the server reject the call.
    let arguments = match arguments {
        Value::Object(map) => Value::Object(map.clone()),
        _ => Value::Object(Map::new()),
    };
    json!({ "name": tool, "arguments": arguments })
}

/// Build a full JSON-RPC request envelope.
pub fn request_envelope(id: i64, method: &str, params: Value) -> Value {
    json!({
        "jsonrpc": JSONRPC_VERSION,
        "id": id,
        "method": method,
        "params": params,
    })
}

/// Build a full JSON-RPC notification envelope (no id, no response expected).
pub fn notification_envelope(method: &str, params: Value) -> Value {
    json!({
        "jsonrpc": JSONRPC_VERSION,
        "method": method,
        "params": params,
    })
}

/// Build the method-not-found response to a server-initiated request.
///
/// A server may ask the client to sample, list roots, or elicit input. None of
/// those are supported, and leaving the request unanswered would stall a server
/// that waits on it, so every one gets an explicit refusal.
pub fn method_not_found_response(id: &Value, method: &str) -> Value {
    json!({
        "jsonrpc": JSONRPC_VERSION,
        "id": id,
        "error": {
            "code": JSONRPC_METHOD_NOT_FOUND,
            "message": format!("method not found: {method}"),
        },
    })
}

/// Split a JSON-RPC response envelope into its result or its error.
pub fn decode_response(message: &Value) -> Result<Value, McpError> {
    if let Some(error) = message.get("error").filter(|value| !value.is_null()) {
        return Err(McpError::Rpc {
            code: error.get("code").and_then(Value::as_i64).unwrap_or(0),
            message: error
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("unknown error")
                .to_string(),
        });
    }
    Ok(message.get("result").cloned().unwrap_or(Value::Null))
}

/// Extract the tool list from a `tools/list` result.
///
/// Entries without a usable `name` are skipped rather than failing the listing:
/// one malformed tool should not hide the rest of a working server.
pub fn decode_tools_list(result: &Value) -> Result<Vec<McpToolInfo>, McpError> {
    let tools = result
        .get("tools")
        .and_then(Value::as_array)
        .ok_or_else(|| McpError::Malformed("tools/list result has no 'tools' array".to_string()))?;

    let mut out = Vec::new();
    for entry in tools.iter().take(MAX_TOOLS_PER_SERVER) {
        let Some(name) = entry
            .get("name")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|name| !name.is_empty())
        else {
            continue;
        };
        let description = entry
            .get("description")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        out.push(McpToolInfo {
            name: name.to_string(),
            description,
            input_schema: normalize_input_schema(entry.get("inputSchema")),
        });
    }
    Ok(out)
}

/// The cursor for the next `tools/list` page, when the server paginates.
pub fn next_cursor(result: &Value) -> Option<String> {
    result
        .get("nextCursor")
        .and_then(Value::as_str)
        .filter(|cursor| !cursor.is_empty())
        .map(str::to_string)
}

/// Coerce a server-supplied argument schema into something every provider will
/// accept as a tool's `input_schema`.
///
/// Providers require an object schema. A server that omits `inputSchema`, or
/// sends a non-object, gets a permissive empty object schema instead.
fn normalize_input_schema(schema: Option<&Value>) -> Value {
    match schema {
        Some(Value::Object(map)) if !map.is_empty() => {
            let mut map = map.clone();
            map.entry("type")
                .or_insert_with(|| Value::String("object".to_string()));
            if !map.contains_key("properties") {
                map.insert("properties".to_string(), Value::Object(Map::new()));
            }
            Value::Object(map)
        }
        _ => json!({ "type": "object", "properties": {} }),
    }
}

/// Outcome of a `tools/call`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpCallOutcome {
    /// Flattened text the model sees.
    pub text: String,
    /// Whether the server flagged the call as an error.
    pub is_error: bool,
}

/// Flatten a `tools/call` result into the text a model receives.
///
/// MCP returns a `content` array of typed blocks. Text blocks contribute their
/// text; every other kind (image, audio, embedded resource) contributes a short
/// placeholder naming what was returned, because the runtime has no channel for
/// binary tool output. `structuredContent`, when present and when no text block
/// carried anything, is serialized as JSON so a structured-only server is not
/// reduced to an empty string.
pub fn decode_tool_call(result: &Value) -> McpCallOutcome {
    let is_error = result
        .get("isError")
        .and_then(Value::as_bool)
        .unwrap_or(false);

    let mut parts: Vec<String> = Vec::new();
    if let Some(blocks) = result.get("content").and_then(Value::as_array) {
        for block in blocks {
            if let Some(rendered) = render_content_block(block) {
                parts.push(rendered);
            }
        }
    }

    if parts.is_empty() {
        if let Some(structured) = result
            .get("structuredContent")
            .filter(|value| !value.is_null())
        {
            parts.push(structured.to_string());
        }
    }

    let text = if parts.is_empty() {
        if is_error {
            "The MCP tool reported an error but returned no detail.".to_string()
        } else {
            "The MCP tool returned no content.".to_string()
        }
    } else {
        truncate(&parts.join("\n"))
    };

    McpCallOutcome { text, is_error }
}

/// Render one content block, or `None` when it carries nothing worth showing.
fn render_content_block(block: &Value) -> Option<String> {
    match block.get("type").and_then(Value::as_str) {
        Some("text") => block
            .get("text")
            .and_then(Value::as_str)
            .filter(|text| !text.is_empty())
            .map(str::to_string),
        Some("image") => Some(format!(
            "[image content omitted: {}]",
            block
                .get("mimeType")
                .and_then(Value::as_str)
                .unwrap_or("unknown type")
        )),
        Some("audio") => Some(format!(
            "[audio content omitted: {}]",
            block
                .get("mimeType")
                .and_then(Value::as_str)
                .unwrap_or("unknown type")
        )),
        Some("resource_link") => block
            .get("uri")
            .and_then(Value::as_str)
            .map(|uri| format!("[resource: {uri}]")),
        Some("resource") => {
            let resource = block.get("resource")?;
            // An embedded resource carries its payload inline as `text` or as
            // base64 `blob`; only the text form is useful to a model.
            if let Some(text) = resource.get("text").and_then(Value::as_str) {
                return Some(text.to_string());
            }
            let uri = resource.get("uri").and_then(Value::as_str).unwrap_or("");
            Some(format!("[binary resource omitted: {uri}]"))
        }
        // An unknown block type may still be readable if it has a `text` field.
        _ => block
            .get("text")
            .and_then(Value::as_str)
            .filter(|text| !text.is_empty())
            .map(str::to_string),
    }
}

/// Cap tool output at [`MAX_TOOL_OUTPUT_CHARS`], noting the truncation.
fn truncate(text: &str) -> String {
    if text.chars().count() <= MAX_TOOL_OUTPUT_CHARS {
        return text.to_string();
    }
    let kept: String = text.chars().take(MAX_TOOL_OUTPUT_CHARS).collect();
    format!("{kept}\n\n[output truncated to {MAX_TOOL_OUTPUT_CHARS} characters]")
}

#[cfg(test)]
mod tests {
    use super::{
        decode_response, decode_tool_call, decode_tools_list, next_cursor, tools_call_params,
        MAX_TOOL_OUTPUT_CHARS,
    };
    use crate::mcp::McpError;
    use serde_json::json;

    #[test]
    fn decode_response_surfaces_rpc_errors() {
        let message = json!({"jsonrpc":"2.0","id":1,"error":{"code":-32602,"message":"bad args"}});
        match decode_response(&message) {
            Err(McpError::Rpc { code, message }) => {
                assert_eq!(code, -32602);
                assert_eq!(message, "bad args");
            }
            other => panic!("expected an rpc error, got {other:?}"),
        }
    }

    #[test]
    fn decode_response_treats_a_null_error_as_success() {
        let message = json!({"jsonrpc":"2.0","id":1,"error":null,"result":{"ok":true}});
        assert_eq!(decode_response(&message).unwrap(), json!({"ok": true}));
    }

    #[test]
    fn tools_list_skips_entries_without_a_name() {
        let result = json!({"tools":[
            {"name":"search","description":"Search","inputSchema":{"type":"object","properties":{"q":{"type":"string"}}}},
            {"description":"nameless"},
            {"name":"   "},
            {"name":"ping"}
        ]});
        let tools = decode_tools_list(&result).unwrap();
        assert_eq!(tools.len(), 2);
        assert_eq!(tools[0].name, "search");
        assert_eq!(tools[1].name, "ping");
        // A tool with no schema still gets a provider-acceptable object schema.
        assert_eq!(tools[1].input_schema, json!({"type":"object","properties":{}}));
    }

    #[test]
    fn tools_list_requires_the_tools_array() {
        assert!(decode_tools_list(&json!({})).is_err());
    }

    #[test]
    fn pagination_cursor_is_read_when_present() {
        assert_eq!(next_cursor(&json!({})), None);
        assert_eq!(next_cursor(&json!({"nextCursor":""})), None);
        assert_eq!(
            next_cursor(&json!({"nextCursor":"page-2"})),
            Some("page-2".to_string())
        );
    }

    #[test]
    fn call_params_normalize_non_object_arguments() {
        assert_eq!(
            tools_call_params("ping", &json!(null)),
            json!({"name":"ping","arguments":{}})
        );
        assert_eq!(
            tools_call_params("ping", &json!({"a":1})),
            json!({"name":"ping","arguments":{"a":1}})
        );
    }

    #[test]
    fn tool_call_flattens_text_blocks_and_marks_errors() {
        let outcome = decode_tool_call(&json!({
            "content":[{"type":"text","text":"line one"},{"type":"text","text":"line two"}],
            "isError": true,
        }));
        assert_eq!(outcome.text, "line one\nline two");
        assert!(outcome.is_error);
    }

    #[test]
    fn tool_call_describes_non_text_blocks() {
        let outcome = decode_tool_call(&json!({
            "content":[{"type":"image","mimeType":"image/png"}]
        }));
        assert_eq!(outcome.text, "[image content omitted: image/png]");
        assert!(!outcome.is_error);
    }

    #[test]
    fn tool_call_falls_back_to_structured_content() {
        let outcome = decode_tool_call(&json!({
            "content": [],
            "structuredContent": {"temperature": 21},
        }));
        assert_eq!(outcome.text, r#"{"temperature":21}"#);
    }

    #[test]
    fn tool_call_reports_empty_results_without_panicking() {
        assert_eq!(
            decode_tool_call(&json!({})).text,
            "The MCP tool returned no content."
        );
        assert_eq!(
            decode_tool_call(&json!({"isError":true})).text,
            "The MCP tool reported an error but returned no detail."
        );
    }

    #[test]
    fn tool_call_output_is_capped() {
        let huge = "x".repeat(MAX_TOOL_OUTPUT_CHARS + 500);
        let outcome = decode_tool_call(&json!({"content":[{"type":"text","text":huge}]}));
        assert!(outcome.text.contains("[output truncated"));
        assert!(outcome.text.chars().count() < MAX_TOOL_OUTPUT_CHARS + 200);
    }
}
