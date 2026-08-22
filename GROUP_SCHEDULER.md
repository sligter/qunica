# Unified conversation scheduler

Group and direct conversations use one Rust scheduler. Each user message creates a persisted turn, and every agent execution is a dispatch under that turn. There is no separate legacy runtime path.

`scheduler_mode` controls turn lifetime independently from the group's communication topology:

- `bounded` consumes the eligible candidates and applies the configured work budgets.
- `automatic` lets the moderator repeatedly dispatch legal candidates or finish the turn. Agent-step, per-agent, hop, moderator-call, and token limits do not stop the turn; failure fuses, moderator timeout, user cancellation, and supersession remain active.

`mesh`, `star`, `hierarchical`, and `ring` only define the legal routing frontier. In particular, a moderator cannot bypass the hub, leader, or next ring position.

## Routing

The conversation rules first establish eligible agents: explicit user mentions take priority; otherwise `free_speech` or `proactive_mode` may include every eligible member. The topology then establishes the current legal frontier. The scheduler chooses work in this order:

1. User mentions.
2. Agent mentions when `agent_mention_policy` is `bounded_schedule`.
3. The moderator when enabled: bounded mode asks it to choose when at least two candidates remain; automatic mode asks it to dispatch or finish even with one candidate.
4. The deterministic topology and `speaking_order` order.
5. Natural completion.

The moderator can only choose among agents in the legal frontier. A moderator failure falls back to deterministic order and is counted by the failure fuse in automatic mode.

## Budget profiles

The bounded mode limits total agent steps, steps per agent, handoff hops, moderator calls, consecutive and total failures, and total tokens. Automatic mode ignores the work and cost limits but still enforces consecutive and total failure limits.

The former sequential fan-out behavior is the following degenerate profile:

```text
max_steps_per_agent = 1
max_scheduler_hops = 0
max_moderator_calls = 0
moderator_enabled = false
agent_mention_policy = display_only
```

Migration applies this profile to rows formerly stored with `scheduler_enabled = 0`. The physical column remains for SQLite upgrade compatibility, but runtime and API code no longer reads or exposes the switch. Direct chats use the same profile with one candidate.

When bounded mode's `max_agent_steps` uses the automatic budget (`null`), a one-pass profile resolves it to the selected candidate count rather than the normal 8–24 step heuristic, so every selected candidate remains reachable even in large groups. This budget setting is distinct from `scheduler_mode = automatic`.

Exhausting the candidate list is a natural completion. Budget terminal states are reserved for remaining work blocked by a limit, or for token and failure limits.

## Persistence and cancellation

`group_turns` stores configuration and topology snapshots, accumulated budgets, and terminal state. `agent_dispatches` stores selection reasons, parent relationships, state, token usage, and visible output. A partial unique index permits at most one active turn per thread; creating a replacement and superseding its predecessor happen in one transaction.

Cancellation writes the terminal state before notifying the in-process cancellation token. Visible output is committed through dispatch completion so a cancelled or superseded turn cannot append late messages. During retryable provider failures, an interrupted checkpoint is persisted only when the turn and dispatch are still running under the same write lock.

## Stream contract

Every conversation emits scheduler events such as `turn_started`, `moderator_started`, `speaker_selected`, dispatch events, terminal events, and `done`. The frontend does not select a legacy protocol by conversation type or feature flag. Cancellation always uses the scheduler turn id and reconciles the returned trace.

`turn_timeout_seconds` currently limits moderator calls only. Agent provider streams do not yet have a server-side execution timeout.
