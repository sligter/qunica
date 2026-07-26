//! Which workspace roots a group agent may address during a turn.
//!
//! An agent has two candidate roots: the conversation's workspace and its own.
//! [`WorkspaceMode`] chooses which of them is *primary* — the address space of
//! every plain relative path, and the root conversation attachments resolve
//! against — and whether the other is reachable as the `~self/` mount.
//!
//! The mode is stored in the free-form `group_agents.context_scope_json` column
//! so no schema migration is needed. Rows written before the mode existed only
//! carry the older `share_group_workspace` boolean; [`WorkspaceMode::from_context_scope`]
//! maps those forward without changing what they did.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// JSON key holding the mode.
const MODE_KEY: &str = "workspace_mode";
/// Legacy JSON key: `true` meant "use the group workspace instead of my own".
const LEGACY_SHARE_KEY: &str = "share_group_workspace";

/// The workspace roots one group agent may address.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceMode {
    /// Conversation workspace only. The shared room, and nothing else.
    #[default]
    Group,
    /// Conversation workspace as primary, with the agent's own workspace
    /// mounted at `~self/`. The shared room plus the agent's own desk.
    GroupAndSelf,
    /// The agent's own workspace only; the conversation workspace — including
    /// its attachments — is out of reach.
    SelfOnly,
}

impl WorkspaceMode {
    /// Wire value used by the API and the `context_scope_json` column.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Group => "group",
            Self::GroupAndSelf => "group_and_self",
            Self::SelfOnly => "self",
        }
    }

    /// Parse a wire value, returning `None` for anything unrecognised so the
    /// caller can reject it rather than silently widening or narrowing access.
    pub fn parse(raw: &str) -> Option<Self> {
        match raw {
            "group" => Some(Self::Group),
            "group_and_self" => Some(Self::GroupAndSelf),
            "self" => Some(Self::SelfOnly),
            _ => None,
        }
    }

    /// Whether the conversation workspace is this agent's primary root. This is
    /// the legacy `share_group_workspace` boolean, preserved for older clients.
    pub const fn uses_group_workspace(self) -> bool {
        matches!(self, Self::Group | Self::GroupAndSelf)
    }

    /// Whether the agent's own workspace is mounted alongside a primary
    /// conversation workspace.
    pub const fn mounts_own_workspace(self) -> bool {
        matches!(self, Self::GroupAndSelf)
    }

    /// Read the mode out of a `context_scope_json` payload.
    ///
    /// Precedence is explicit mode, then the legacy boolean, then the default.
    /// A row written before this field existed carries `share_group_workspace:
    /// true` (group workspace) or nothing at all (its own workspace), so absent
    /// JSON maps to [`WorkspaceMode::SelfOnly`] — not to the [`Default`], which
    /// applies to *new* memberships instead.
    pub fn from_context_scope(raw: Option<&str>) -> Self {
        let Some(object) = raw.and_then(parse_object) else {
            return Self::SelfOnly;
        };
        if let Some(mode) = object
            .get(MODE_KEY)
            .and_then(Value::as_str)
            .and_then(Self::parse)
        {
            return mode;
        }
        match object.get(LEGACY_SHARE_KEY).and_then(Value::as_bool) {
            Some(true) => Self::Group,
            _ => Self::SelfOnly,
        }
    }

    /// Write the mode into a `context_scope_json` payload, preserving unrelated
    /// keys and keeping the legacy boolean in sync for older readers.
    pub fn to_context_scope(self, raw: Option<&str>) -> Result<Option<String>, serde_json::Error> {
        let mut object = raw.and_then(parse_object).unwrap_or_default();
        object.insert(MODE_KEY.to_string(), Value::from(self.as_str()));
        if self.uses_group_workspace() {
            object.insert(LEGACY_SHARE_KEY.to_string(), Value::Bool(true));
        } else {
            object.remove(LEGACY_SHARE_KEY);
        }
        serde_json::to_string(&Value::Object(object)).map(Some)
    }
}

/// Parse a JSON object payload, ignoring malformed or non-object values.
fn parse_object(raw: &str) -> Option<Map<String, Value>> {
    match serde_json::from_str::<Value>(raw).ok()? {
        Value::Object(object) => Some(object),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn absent_scope_keeps_the_agents_own_workspace() {
        assert_eq!(
            WorkspaceMode::from_context_scope(None),
            WorkspaceMode::SelfOnly
        );
        assert_eq!(
            WorkspaceMode::from_context_scope(Some("{}")),
            WorkspaceMode::SelfOnly
        );
        assert_eq!(
            WorkspaceMode::from_context_scope(Some("not json")),
            WorkspaceMode::SelfOnly
        );
    }

    #[test]
    fn legacy_share_flag_maps_to_group_mode() {
        assert_eq!(
            WorkspaceMode::from_context_scope(Some(r#"{"share_group_workspace":true}"#)),
            WorkspaceMode::Group
        );
        assert_eq!(
            WorkspaceMode::from_context_scope(Some(r#"{"share_group_workspace":false}"#)),
            WorkspaceMode::SelfOnly
        );
    }

    #[test]
    fn explicit_mode_wins_over_the_legacy_flag() {
        let raw = r#"{"share_group_workspace":true,"workspace_mode":"self"}"#;
        assert_eq!(
            WorkspaceMode::from_context_scope(Some(raw)),
            WorkspaceMode::SelfOnly
        );
    }

    #[test]
    fn unknown_mode_falls_back_to_the_legacy_flag() {
        let raw = r#"{"share_group_workspace":true,"workspace_mode":"everything"}"#;
        assert_eq!(
            WorkspaceMode::from_context_scope(Some(raw)),
            WorkspaceMode::Group
        );
    }

    #[test]
    fn writing_preserves_unrelated_keys_and_syncs_the_legacy_flag() {
        let raw = r#"{"other":1,"share_group_workspace":true}"#;
        let written = WorkspaceMode::GroupAndSelf
            .to_context_scope(Some(raw))
            .unwrap()
            .unwrap();
        let object = parse_object(&written).unwrap();
        assert_eq!(object.get("other"), Some(&Value::from(1)));
        assert_eq!(object.get(MODE_KEY), Some(&Value::from("group_and_self")));
        assert_eq!(object.get(LEGACY_SHARE_KEY), Some(&Value::Bool(true)));

        let isolated = WorkspaceMode::SelfOnly
            .to_context_scope(Some(&written))
            .unwrap()
            .unwrap();
        let object = parse_object(&isolated).unwrap();
        assert_eq!(object.get(MODE_KEY), Some(&Value::from("self")));
        assert!(!object.contains_key(LEGACY_SHARE_KEY));
    }

    #[test]
    fn round_trips_through_the_wire_value() {
        for mode in [
            WorkspaceMode::Group,
            WorkspaceMode::GroupAndSelf,
            WorkspaceMode::SelfOnly,
        ] {
            assert_eq!(WorkspaceMode::parse(mode.as_str()), Some(mode));
        }
        assert_eq!(WorkspaceMode::parse("shared"), None);
    }
}
