//! Group message streaming runtime.
//!
//! [`group`] holds the turn orchestration (routing, fan-out, terminal events);
//! [`agent_as_tool`] resolves visible group handoffs; [`sequence`] holds the
//! per-thread monotonic sequence allocator and durable persistence used by the
//! runtime; [`workspace_scope`] decides which workspace roots a group agent may
//! address.
//!
//! [`hooks`] is the seam behaviour attaches to around a step, and
//! [`compaction`]/[`compaction_hook`] are its first consumer: keeping a thread
//! inside the provider's context window, and recovering when it already is not.

pub mod agent_as_tool;
pub mod approval;
pub mod chat_title;
pub mod compaction;
pub mod compaction_hook;
pub mod conversation_context;
pub mod group;
pub mod group_scheduler;
pub mod hooks;
pub mod sequence;
pub mod tool_output;
pub mod workspace_scope;

pub use group::{
    run_group_turn, run_group_turn_with_stream_id, RuntimeServices, TurnOutcome, TurnRequest,
};
pub use hooks::{HookChain, RequestRecovery, RuntimeHook, StepContext};

// Re-export the stream event contract so integration tests (which link only
// against this crate) can name the shared types without depending on the domain
// crate directly.
pub use ag_swarmer_domain::events::{StreamEvent, StreamEventKind};
