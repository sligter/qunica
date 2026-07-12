//! Group message streaming runtime.
//!
//! [`group`] holds the turn orchestration (routing, fan-out, terminal events);
//! [`agent_as_tool`] resolves visible group handoffs; [`sequence`] holds the
//! per-thread monotonic sequence allocator and durable persistence used by the
//! runtime.

pub mod agent_as_tool;
pub mod conversation_context;
pub mod group;
pub mod group_scheduler;
pub mod sequence;

pub use group::{run_group_turn, RuntimeServices, TurnOutcome, TurnRequest};

// Re-export the stream event contract so integration tests (which link only
// against this crate) can name the shared types without depending on the domain
// crate directly.
pub use ag_swarmer_domain::events::{StreamEvent, StreamEventKind};
