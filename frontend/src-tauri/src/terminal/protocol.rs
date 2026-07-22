use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateTerminalRequest {
    pub conversation_id: String,
    pub cwd: String,
    pub cols: u16,
    pub rows: u16,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TerminalDescriptor {
    pub session_id: String,
    pub shell_name: String,
    pub cwd: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(
    tag = "event",
    content = "data",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum TerminalEvent {
    Output {
        bytes_base64: String,
    },
    Exit {
        code: Option<u32>,
        signal: Option<String>,
    },
    Error {
        code: String,
        message: String,
    },
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TerminalCommandError {
    pub code: String,
    pub message: String,
}

impl TerminalCommandError {
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{CreateTerminalRequest, TerminalCommandError, TerminalDescriptor, TerminalEvent};
    use serde_json::json;

    #[test]
    fn protocol_uses_camel_case_wire_shapes() {
        let request: CreateTerminalRequest = serde_json::from_value(json!({
            "conversationId": "conversation-1",
            "cwd": "/workspace",
            "cols": 120,
            "rows": 40
        }))
        .expect("request should deserialize");
        assert_eq!(request.conversation_id, "conversation-1");

        let descriptor = TerminalDescriptor {
            session_id: "session-1".to_string(),
            shell_name: "fish".to_string(),
            cwd: "/workspace".to_string(),
        };
        assert_eq!(
            serde_json::to_value(descriptor).expect("descriptor should serialize"),
            json!({
                "sessionId": "session-1",
                "shellName": "fish",
                "cwd": "/workspace"
            })
        );

        let output = TerminalEvent::Output {
            bytes_base64: "dGVybWluYWw=".to_string(),
        };
        assert_eq!(
            serde_json::to_value(output).expect("event should serialize"),
            json!({
                "event": "output",
                "data": { "bytesBase64": "dGVybWluYWw=" }
            })
        );

        let error = TerminalCommandError::new("terminal.test", "test error");
        assert_eq!(
            serde_json::to_value(error).expect("command error should serialize"),
            json!({ "code": "terminal.test", "message": "test error" })
        );
    }
}
