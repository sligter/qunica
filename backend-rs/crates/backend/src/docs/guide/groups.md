# Groups

A group is a project space where several agents share one conversation, one workspace, and one history.

## Creating one

A group needs a name. Binding a workspace is what lets its agents touch files; without one, file and shell tools report that no workspace is configured.

## Who replies

- **Mentions.** `@Name` addresses one agent. Explicit mentions always win.
- **free_speech.** When on, every agent may reply to an unaddressed message. When off, only mentioned agents reply.
- **proactive_mode.** Lets agents continue for extra rounds without a new user message, bounded by `proactive_max_rounds`.
- **allow_agent_free_mention.** Lets an agent's `@mention` of another agent dispatch a follow-up, capped by `agent_free_mention_max_dispatches`. Set it to `0` to disable.

## Communication modes

`communication_mode` sets who speaks first, and applies whether or not the scheduler is on.

- `mesh` — no ordering; the default.
- `star` — a hub agent goes first.
- `hierarchical` — leaders go first.
- `ring` — a fixed rotation, by `speaking_order`.

## The bounded scheduler

`scheduler_enabled` turns on turn-by-turn scheduling with explicit budgets. It records every dispatch, so the turn trace shows which agent ran, why it was chosen, and what it cost.

Budgets that end a turn when exhausted:

- `max_agent_steps`, `max_steps_per_agent`
- `max_scheduler_hops`, `max_moderator_calls`
- `max_consecutive_failures`, `max_total_failures`
- `max_total_tokens`
- `turn_timeout_seconds`

`agent_mention_policy` decides what an agent-to-agent mention does: `display_only` just renders it, `bounded_schedule` dispatches a real follow-up.

A **moderator** can be enabled with its own provider and model to pick the next speaker.

## Notes

- Members and agents can be muted individually.
- Group notes are a shared scratchpad, separate from chat history.
- Clearing messages does not delete workspace files.
