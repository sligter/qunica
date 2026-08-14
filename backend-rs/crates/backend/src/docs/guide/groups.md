# Groups

A group is a project space where several agents share one conversation, one workspace, and one history.

## Creating one

A group needs a name. Binding a workspace is what lets its agents touch files; without one, file and shell tools report that no workspace is configured.

## Who replies

- **Mentions.** `@Name` addresses one agent. Explicit mentions always win.
- **free_speech.** When on, every agent may reply to an unaddressed message. When off, only mentioned agents reply.
- **proactive_mode.** Includes every eligible agent in the current turn. An agent can answer with `<SILENT>` to skip without creating a message.
- **allow_agent_free_mention.** Lets an agent's `@mention` of another agent dispatch a follow-up, capped by `agent_free_mention_max_dispatches`. Set it to `0` to disable.

## Communication modes

`communication_mode` sets the legal speaking route.

- `mesh` — no ordering; the default.
- `star` — a hub agent goes first.
- `hierarchical` — leaders go first.
- `ring` — a fixed rotation, by `speaking_order`.

## Scheduling and budgets

Every group and direct chat uses the same persisted scheduler. It records each turn and dispatch so the trace shows which agent ran, why it was selected, and what it cost.

`scheduler_mode` is independent from the communication mode:

- `bounded` consumes candidates and stops at the configured work limits.
- `automatic` lets the moderator repeatedly choose a legal speaker or finish the turn. It ignores agent-step, per-agent, hop, moderator-call, and token limits, while retaining failure limits, moderator timeout, cancellation, and supersession.

Older scheduler-off conversations are migrated to a one-pass profile: each selected agent can run once, moderator and agent follow-ups are disabled, and selection follows the deterministic order. Direct chats use the same profile with one candidate.

Budgets that end a turn when exhausted:

- `max_agent_steps`, `max_steps_per_agent`
- `max_scheduler_hops`, `max_moderator_calls`
- `max_consecutive_failures`, `max_total_failures`
- `max_total_tokens`

`agent_mention_policy` decides what an agent-to-agent mention does: `display_only` just renders it, `bounded_schedule` dispatches a real follow-up.

A **moderator** can be enabled with its own provider and model to pick the next legal speaker. Automatic mode requires it. `turn_timeout_seconds` currently limits moderator calls; it is not an agent execution timeout.

See [the scheduler design](../../../../../../GROUP_SCHEDULER.md) for the runtime and persistence contract.

## Notes

- Members and agents can be muted individually.
- Group notes are a shared scratchpad, separate from chat history.
- Clearing messages does not delete workspace files.
