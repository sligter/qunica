//! ACP (Agent Client Protocol) runtime.
//!
//! Task 9a lays the foundation: [`config`] normalizes an agent's raw ACP
//! runtime config into a validated [`AcpRuntimeConfig`], and [`process`] holds
//! the audit-persistence helpers for `external_agent_runs` plus a bounded
//! output [`Tail`]. Task 9b will add the ACP stdio JSON-RPC protocol, child
//! process spawning, timeout, and cancellation on top of these.

pub mod config;
pub mod process;

pub use config::{
    normalize_acp_runtime, AcpConfigError, AcpConfigValue, AcpRuntimeConfig, AcpRuntimeProfile,
    PermissionPolicy, BLOCKED_ENV_KEYS, DEFAULT_TIMEOUT_SECONDS, MAX_TIMEOUT_SECONDS,
};
pub use process::{AcpAuditError, AcpRunAudit, AcpRunContext, Tail, MAX_TAIL_CHARS};
