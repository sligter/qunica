# Groups

A group is a project space where several agents share one conversation, one workspace, and one history.

## Creating one

A group needs a name. Binding a workspace is what lets its agents touch files; without one, file and shell tools report that no workspace is configured.

You can save an existing group's settings and Agent roster as a reusable template from the group's settings page. Choosing that template while creating another group copies the saved configuration; the new group's name and workspace are still chosen separately.

## Managing tasks

Open the current task selector to switch between active and archived tasks. Each row has a **Rename task** action; renaming changes only the title and preserves messages, execution state, and Git bindings. Titles contain 1–80 characters after trimming whitespace.

Different task threads in the same group run concurrently, including when they use the same Agent. Each thread has its own history, ACP session, execution lease, approvals, cancellation and restart confirmation. New messages in one thread wait for its current worker to finish; a waiting or interrupted task does not reserve the whole group. Occurrences of the same automation still run one at a time.

Tasks using separate Git branches run in their existing worktrees. Tasks sharing a working directory can still edit the same files, so use separate branch tasks when file isolation is needed. A shared controlled computer retains its own exclusive execution lease; parallel task scheduling does not allow two turns to control it together.

Archived rows also have **Delete task**, and the archived section has **Delete all archived**. Both ask for confirmation. Deletion removes the task and its messages from the app, keeps its Git branch, and attempts to remove its worktree. Bulk deletion uses the archived tasks shown when the confirmation opens; active tasks are untouched. If a deletion fails, completed deletions stay applied and the dialog allows retrying the remaining tasks.

## Who replies

- **Mentions.** In mentioned-only mode, `@Name` selects the responders. In a group-wide mode, mentions choose who starts without removing the other eligible agents from the turn.
- **free_speech.** When on, every eligible agent enters the turn and replies sequentially.
- **proactive_mode.** Includes every eligible agent in the current turn. An agent can answer with `<SILENT>` to skip without creating a message.

In a bounded mesh turn without a moderator, an Agent's public `@mention` requests a response from that member after the current speaker finishes. This can revisit a member who previously replied or stayed silent, within the configured total steps, per-member steps, and hop limits. Muted and manual-only members are excluded; a pending user mention takes priority. A zero hop limit disables these follow-ups. In other topologies and moderated turns, Agent mentions remain display-only. Legacy mention-dispatch fields remain readable, but requests that send those removed fields are rejected with `400 Bad Request`.

## Communication modes

`communication_mode` sets the legal speaking route.

- `mesh` — no ordering; the default.
- `star` — a hub agent goes first.
- `hierarchical` — leaders go first.
- `ring` — a fixed rotation, by `speaking_order`.

Within one task turn, Agents are dispatched one at a time. A speaker must finish its response before that turn's scheduler can advance; other task threads can run concurrently. Writing a shared note or assigning work in prose shares context but does not itself dispatch another agent.

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
- `automatic` lets the moderator repeatedly choose a legal speaker or finish the turn. It uses the same step, per-agent, hop, moderator-call, token, failure, timeout, cancellation, and supersession limits. A legacy automatic group with zero moderator calls receives an effective cap of one decision per agent step plus a final decision.

Older scheduler-off conversations are migrated to a one-pass profile: each selected agent can run once, moderator dispatch is disabled, and selection follows deterministic order. Direct chats use the same profile with one candidate.

Budgets that end a turn when exhausted:

- `max_agent_steps`, `max_steps_per_agent`
- `max_scheduler_hops`, `max_moderator_calls`
- `max_consecutive_failures`, `max_total_failures`
- `max_total_tokens`

A **moderator** can be enabled with its own provider and model to pick the next legal speaker. Automatic mode requires it. `turn_timeout_seconds` currently limits moderator calls; it is not an agent execution timeout.

When the group has the [decision model](settings.md#decision-model) switched on (`decision_enabled` and one or more `decision_scenarios`, both per group; the endpoint itself is set once under Settings), its scenarios sit in front of these steps: `moderator_selection` picks the bounded or automatic speaker and the dispatch records reason `decision_model`; `automatic_finish` ends an automatic turn once the objective reads as complete; `proactive_prefilter` drops clearly irrelevant members from a proactive or everyone turn and emits a `warning` with code `decision_prefilter` naming them; `reply_outcome` ends the turn as waiting-for-user when a reply asked the user and stopped. Each falls back to the behaviour above when the model fails or is unsure. Templates carry the group's switches.

In automatic mode, completion is checked first after an agent step when `automatic_finish` is enabled. If it does not finish the turn, `moderator_selection` can select a legal speaker, even when only one remains, or choose `defer` when there is no supported concrete unfinished work. Defer falls back to the chat moderator; it does not itself finish the turn. The two switches remain independent. Successful selection avoids a chat moderator call and preserves the existing progress summary and recent messages rather than fabricating a generated summary or work assignment. User mentions, candidate revalidation, budgets and cancellation retain their existing priority and checks.

See [the scheduler design](../../../../../../docs/GROUP_SCHEDULER.md) for the runtime and persistence contract.

## Shared notes

Group notes store durable decisions, conventions and lessons as Markdown files under the local group's `Notes` directory. `index.md` lists the active notes. Agents receive the built-in authoring method in their host context. `llm_chat` agents use the built-in `ReadGroupNotes` and `EditGroupNote` tools. ACP agents in a group with a local workspace receive the built-in `qunica-group-notes` MCP server, including when their working directory is their own workspace or a task worktree.

The ACP notes server exposes `ReadGroupNotes` (omit `path` for the index), `CreateGroupNote` (`title` and optional `content`), and `EditGroupNote` (`path` and exact `old_text`/`new_text` replacements in `edits`). Use the returned note path unchanged. Creation registers the note in the database and lets the host generate its filename and update the index; omitted content creates the proposed-note template. Edits update both the file and database and reject stale content. Creation and editing honor the group's decision-model `note_validation` setting. Do not create note files or edit the index manually.

The MCP bridge is scoped to the current account, group, agent and thread. Only registered notes in that group are accessible. Its authority is active during the ACP dispatch and revoked when the dispatch ends, fails, times out or is cancelled. New and restored ACP sessions receive the server configuration; live session reuse keeps the connection while renewing dispatch authority. The relay is built into both the desktop and standalone backend executables and requires no extra installation. ACP adapters must honor the supplied stdio MCP configuration; an adapter that ignores it cannot use these tools and should report their absence. Direct chats and groups without a local workspace do not receive this server.

New notes start with the [write-notes-like-deepseek](https://github.com/czm15053/write-notes-like-deepseek) four-section template: Problem, Decision, Alternatives considered and Consequences. The editor offers status and category controls. The Markdown header stores the title, `Status`, initial `Since` date and `Category` (决策 / 约定 / 踩坑); filenames remain host-generated. Creating a note through the API with omitted or blank content also supplies a template.

Keep one topic per note, about 200 words. Start at `proposed`; `implemented` describes verified present facts, `rejected` includes a reason, and `archived` retires superseded guidance. Agreement does not establish implementation, and one Agent's suggestion does not establish group consensus. State the strongest argument for alternatives, including doing nothing or reusing the current solution. Record benefits, costs and a concrete trigger for reconsideration. Update facts in place and preserve the initial date; link to repository decision records when those already hold the authoritative explanation.

Existing free-form notes remain editable without automatic conversion. The template and runtime guidance assist authors; they do not mechanically verify consensus or implementation. The app maintains the index.

The built-in Assistant can list and read these notes and can propose creating or updating one. Like its other writes, the note changes only after approval.

## Notes

- Members and agents can be muted individually.
- Group notes require a local group workspace and are separate from chat history.
- Clearing messages does not delete workspace files.
