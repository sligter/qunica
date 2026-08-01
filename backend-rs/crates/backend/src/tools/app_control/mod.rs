//! App-control tools: how the built-in Assistant inspects and changes the app.
//!
//! These tools own their SQL rather than calling the API handlers, which take
//! axum extractors and cannot be invoked from a tool. That means the owner
//! scoping is this module's responsibility: every query filters on `owner_id`,
//! and every projection lists its columns explicitly. Never `SELECT *` here — a
//! column added to a table later would then start flowing to the model by
//! default, which is exactly how an API key escapes.
//!
//! [`read`] holds the inspection tools. Staged writes arrive in a later change;
//! nothing in this module mutates a row.

pub(crate) mod read;

use sqlx::SqlitePool;

/// The owner and conversation an app-control tool call runs on behalf of.
///
/// Held by the [`ToolExecutor`](crate::tools::ToolExecutor) only for the
/// built-in Assistant. Its absence is what makes these tools unavailable to
/// regular agents, and the executor reports `SETUP_REQUIRED` rather than
/// falling back to some ambient identity.
#[derive(Debug, Clone)]
pub struct AppControlContext {
    pool: SqlitePool,
    owner_id: String,
    /// The conversation the call came from. Staged actions record it so the
    /// approval card can be traced back to the exchange that produced it.
    #[allow(dead_code)]
    conversation_id: String,
}

impl AppControlContext {
    pub fn new(pool: SqlitePool, owner_id: String, conversation_id: String) -> Self {
        Self {
            pool,
            owner_id,
            conversation_id,
        }
    }

    pub(crate) fn pool(&self) -> &SqlitePool {
        &self.pool
    }

    pub(crate) fn owner_id(&self) -> &str {
        &self.owner_id
    }
}

/// The resource families the Assistant may address.
///
/// An explicit enum rather than a free-form string: the parse is the allowlist,
/// so a table this module was never meant to expose cannot be reached by
/// guessing a name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TargetKind {
    Agent,
    Provider,
    Mcp,
    Skill,
    Workspace,
    Group,
    Chat,
}

impl TargetKind {
    pub(crate) fn parse(raw: &str) -> Option<Self> {
        match raw.trim() {
            "agent" => Some(Self::Agent),
            "provider" => Some(Self::Provider),
            "mcp" => Some(Self::Mcp),
            "skill" => Some(Self::Skill),
            "workspace" => Some(Self::Workspace),
            "group" => Some(Self::Group),
            "chat" => Some(Self::Chat),
            _ => None,
        }
    }

    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Agent => "agent",
            Self::Provider => "provider",
            Self::Mcp => "mcp",
            Self::Skill => "skill",
            Self::Workspace => "workspace",
            Self::Group => "group",
            Self::Chat => "chat",
        }
    }

    /// Every kind, for `AppState`'s counts and for error messages that list
    /// what the caller could have asked for.
    pub(crate) const ALL: [Self; 7] = [
        Self::Agent,
        Self::Provider,
        Self::Mcp,
        Self::Skill,
        Self::Workspace,
        Self::Group,
        Self::Chat,
    ];
}
