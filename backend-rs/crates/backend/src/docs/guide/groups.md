# Groups

A group is a project space where several agents share one conversation, one workspace, and one history.

## Creating one

A group needs a name. Binding a workspace is what lets its agents touch files; without one, file and shell tools report that no workspace is configured.

You can save an existing group's settings and Agent roster as a reusable template from the group's settings page. Choosing that template while creating another group copies the saved configuration; the new group's name and workspace are still chosen separately.

## Who replies

- **Mentions.** In mentioned-only mode, `@Name` selects the responders. In a group-wide mode, mentions choose who starts without removing the other eligible agents from the turn.
- **free_speech.** When on, every eligible agent enters the turn and replies sequentially.
- **proactive_mode.** Includes every eligible agent in the current turn. An agent can answer with `<SILENT>` to skip without creating a message.

Only a user's `@mention` selects public responders. An `@mention` written by an Agent is display-only. Legacy mention-dispatch fields remain readable so old intent is not erased, but requests that send those removed fields are rejected with `400 Bad Request`.

## Communication modes

`communication_mode` sets the legal speaking route.

- `mesh` — no ordering; the default.
- `star` — a hub agent goes first.
- `hierarchical` — leaders go first.
- `ring` — a fixed rotation, by `speaking_order`.

Agents are dispatched one at a time, not in the background. A speaker must finish its response before the scheduler can advance. Writing a shared note or assigning work in prose shares context but does not itself dispatch another agent.

Use `AgentAsTool` for structured delegation when it is available. `call` runs a helper privately and returns the result to the caller. In bounded mode, `handoff` instead transfers the public response to that helper. A pending public responder may be delegated to; doing so claims its scheduled slot so it does not run again later. Agents that already ran and helpers already claimed are excluded. Automatic mode permits `call` only because the moderator owns public dispatch.

Practical combinations:

- **Star synthesis:** address the hub first. It can `call` spokes and then write one integrated answer; called spokes give up any pending public slot.
- **Hierarchical work:** address a leader, which can call or hand off to reachable workers.
- **Mesh discussion:** use everyone/proactive for a public pass; structured delegation replaces the delegated member's pending public execution.
- **Ring pipeline:** use everyone/proactive and let the scheduler follow `speaking_order`; delegating to a pending ring member consumes that member's scheduled slot.

## Scheduling and budgets

Every group and direct chat uses the same persisted scheduler. It records each turn and dispatch so the trace shows which agent ran, why it was selected, and what it cost.

`scheduler_mode` is independent from the communication mode:

- `bounded` consumes candidates and stops at the configured work limits.
- `automatic` lets the moderator repeatedly choose a legal speaker or finish the turn. It ignores agent-step, per-agent, hop, moderator-call, and token limits, while retaining failure limits, moderator timeout, cancellation, and supersession.

Older scheduler-off conversations are migrated to a one-pass profile: each selected agent can run once, moderator dispatch is disabled, and selection follows deterministic order. Direct chats use the same profile with one candidate.

Budgets that end a turn when exhausted:

- `max_agent_steps`, `max_steps_per_agent`
- `max_scheduler_hops`, `max_moderator_calls`
- `max_consecutive_failures`, `max_total_failures`
- `max_total_tokens`

A **moderator** can be enabled with its own provider and model to pick the next legal speaker. Automatic mode requires it. `turn_timeout_seconds` currently limits moderator calls; it is not an agent execution timeout.

See [the scheduler design](../../../../../../docs/GROUP_SCHEDULER.md) for the runtime and persistence contract.

## Shared notes

Group notes are an app-managed scratchpad backed by Markdown files under the local group's `Notes` directory. `index.md` lists the active notes. Every Agent in the group can use `ReadGroupNotes` and `EditGroupNote`, even when its normal workspace scope is set to its own workspace.

The built-in Assistant can list and read these notes and can propose creating or updating one. Like its other writes, the note changes only after approval.

## Notes

- Members and agents can be muted individually.
- Group notes require a local group workspace and are separate from chat history.
- Clearing messages does not delete workspace files.
