# Agents

An agent is a reusable AI member: a prompt, a model, a set of tools, and a workspace. The same agent can join several groups and hold several direct chats.

## Fields

- **name** — 1 to 100 characters. Used for `@mentions`.
- **description** — optional, shown in pickers.
- **system_prompt** — the agent's standing instructions. Must not be empty; a default is supplied if omitted.
- **runtime_kind** — `llm_chat` or `acp`. Defaults to `llm_chat`.
- **workspace_id** — the workspace to bind. Required at creation.
- **llm_provider_id** — the provider to call. Only for `llm_chat`.
- **llm_config** — model settings: `model`, `vision`, `temperature`, `top_p`, and `reasoning_effort`. Sampling and thinking fields are omitted automatically for models that reject them.
- **tool_config** — which tools are enabled.
- **skill_ids** — skills mounted for this agent.

## Runtime kinds

**`llm_chat`** calls an LLM provider directly and runs the built-in tools. This is the normal kind.

**`acp`** drives an external CLI agent over the Agent Client Protocol — Codex CLI, Claude Code, and other presets. It stores an `acp_runtime` blob instead of a provider. Install the CLI and its adapter; Codex offers a device-code sign-in entry in the runtime form, while other runtimes require external sign-in. See `external-cli-agents`.

For `llm_chat`, Qunica loads repository conventions from `AGENTS.md` in the primary workspace root, or `CLAUDE.md` when `AGENTS.md` is absent, with a 6,000-character limit. The section is refreshed every turn and is lower priority than host operating, approval, and workspace rules. Qunica removes it from ACP briefs because supported CLI agents load their own project instructions from the working directory.

## Prompt layers

Qunica assembles instructions for every invocation from separate sources:

| Layer | Responsibility |
| --- | --- |
| Agent persona | Role, domain conventions, deliverables and quality criteria; custom prompts remain unchanged |
| Host operating rules | Complete authorized work, inspect before editing, preserve unrelated changes, respect approvals, inspect tool outcomes and verify proportionately |
| Tool guidance (`llm_chat`) | Short usage advice for tools in this invocation's actual catalog, such as bounded reads, focused edits, job status and source inspection |
| Runtime and conversation context | Actual shell, workspace roots, participants, scheduler, relevant skills and shared notes |
| Project conventions | Workspace-provided instructions, subordinate to host constraints |

Host rules reach existing agents as well as newly created ones. The default persona
and the prompt generator focus on role and craft instead of duplicating host
policies. Existing saved personas are not migrated or overwritten.

ACP receives host task context in its existing task envelope. Qunica omits its
provider-native tool guidance and project-convention excerpt from that brief;
the external runtime supplies its own tool catalog and project-instruction loader.
Group dispatch remains sequential, and permission enforcement stays in code.
An instruction, proposal, tool launch or job ID is not proof of completion.

This separation adapts the capability-dependent guidance in
[dsh's prompt assembly](https://github.com/deepseek-ai/deepseek-harness/blob/687f282e8625c6b8d11ff58211a90524be2e4317/packages/core/system-prompt/src/index.ts)
and [file-tool instructions](https://github.com/deepseek-ai/deepseek-harness/blob/687f282e8625c6b8d11ff58211a90524be2e4317/packages/fs/tool-fs/src/read.ts).
Qunica keeps its existing history and ACP protocols; it does not implement dsh's
plugin registry or runtime-context snapshot format.

Request-recording and assembly regressions verify these instructions reach API-model requests
and that unavailable tool guidance stays out of ACP briefs. They do not measure
model behavior. For model evaluation, compare the same tasks before and after:
implementing a bounded fix while preserving unrelated changes, answering a
review without edits, recovering from an edit conflict, inspecting a background
job without launching it twice, and reporting an unconfigured tool or a pending
proposal accurately. Assess actual actions and checks, not just final wording.

## Built-in tools

| Tool | Needs a workspace | Does |
| --- | --- | --- |
| `Read` | yes | Read a UTF-8 file, with offset and limit |
| `Write` | yes | Create or overwrite a file |
| `Edit` | yes | Exact-match replacements in one file |
| `DeleteFile` | yes | Delete one regular file; directories and symlinks are rejected |
| `Glob` | yes | Find files by pattern |
| `Grep` | yes | Search file contents |
| `Bash` | yes | Run a guarded shell command in the root |
| `ReadGroupNotes` | group local workspace | Read the shared note index or one note |
| `EditGroupNote` | group local workspace | Exact-match edits to one shared note |
| `WebSearch` | no | Search the web; needs a Tavily or DeepSeek key in Settings |
| `Fetch` | no | Read one HTTP(S) URL, bounded; uses Markdown via DeepSeek when selected and configured |
| `GenerateImage` | yes | Generate an image and save it under `generations/`; needs Settings → Media |
| `GenerateVideo` | yes | Generate a video and save it under `generations/`; needs Settings → Media |
| `AskUser` | no | Pause and ask the user a question |
| `TodoWrite` | no | Keep a checklist of the current work, one status per item |
| `ExitPlanMode` | no | Present a plan for approval |
| `SkillManager` | no | List and load mounted skills |

An `llm_chat` agent with no tools configured gets `Read`, `Glob`, and `Grep`. Shared-note tools are mounted automatically when the local group's notes directory is available. ACP agents instead receive the built-in `qunica-group-notes` MCP server, with `ReadGroupNotes`, `CreateGroupNote` and `EditGroupNote`, for groups with a local workspace; see [Shared notes](groups.md#shared-notes).

`AgentAsTool` is the host-level group delegation mechanism. Its target list is computed for each turn: it contains only bound, active, unselected group helpers allowed by the topology. Every call must explicitly choose a mode.

| Mode | Does | Available |
| --- | --- | --- |
| `call` | Runs one helper privately and returns its result to the caller | always |
| `fan_out` | Runs several helpers privately in one call, each on its own task from `dispatches`, and returns all their results together | when two or more helpers with distinct display names are reachable |
| `handoff` | Transfers the public turn to one helper and ends the caller's turn | interactive turns only; automatic scheduler turns keep public dispatch with the moderator |

`fan_out` is a batching mode, not a concurrency one: its targets run one after another, like any other group dispatch. What it saves is round trips — delegating to three helpers costs one provider request instead of three, each of which would have carried the caller's whole context. Each target spends one scheduler agent step and each helper still runs at most once per turn, so a target that names an already-dispatched helper is reported and the rest of the batch continues. A `fan_out` that names one assistant, or the same one twice, is rejected before anything runs.

Codex CLI and Claude Code may create their own native subagents inside an ACP run. Those subagents stay private to the external runtime: they are not group members, do not enter the group topology, and return their output only to the owning ACP Agent. The host has no anonymous sub-agent tool of its own — delegation always goes to a real group member through `AgentAsTool`.

## Approvals and unattended mode

`Bash` reviews each command before running it. Destructive but legitimate work — deleting files, `git reset --hard`, `git clean`, a force-push, writing outside the workspace — pauses the turn and shows an approval card. Approving with **remember** grants that rule for the rest of the thread. A second class of command (formatting a volume, powering off the host, `dd of=`) is refused outright, with no approval offered.

With the [decision model](settings.md#decision-model) scenario `shell_risk` switched on for the group, a command the fixed review allowed gets a second opinion: the model scores how destructive it is and whether it deletes files, writes outside the workspace, or discards git work. A risky score shows an approval card under the rule `decision-risk`, which **remember** grants like any other. The model can only add a card, never remove one, and the fixed review keeps the last word on what is refused.

**Unattended mode** (`bypass_approvals` in `tool_config`) turns the whole review off: nothing pauses, and the refused class runs too. It is the equivalent of Codex YOLO mode or Claude Code's `--dangerously-skip-permissions`, and it is off for every agent that does not explicitly ask for it. Only enable it for an agent you trust in a workspace you can afford to lose.

## Vision

Image attachments are on by default for `llm_chat` agents. Set `vision` to `false` in `llm_config` for a text-only model.

## Notes

- Deleting an agent is a soft delete. Its history stays readable; the conversation reports the agent as unavailable.
- The built-in Assistant does not appear in the agent library and cannot be edited or deleted.
