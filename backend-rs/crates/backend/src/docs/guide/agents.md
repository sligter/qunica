# Agents

An agent is a reusable AI member: a prompt, a model, a set of tools, and a workspace. The same agent can join several groups and hold several direct chats.

## Fields

- **name** — 1 to 100 characters. Used for `@mentions`.
- **description** — optional, shown in pickers.
- **system_prompt** — the agent's standing instructions. Must not be empty; a default is supplied if omitted.
- **runtime_kind** — `llm_chat` or `acp`. Defaults to `llm_chat`.
- **workspace_id** — the workspace to bind. Required at creation.
- **llm_provider_id** — the provider to call. Only for `llm_chat`.
- **llm_config** — model settings, including `model` and `vision`.
- **tool_config** — which tools are enabled.
- **skill_ids** — skills mounted for this agent.

## Runtime kinds

**`llm_chat`** calls an LLM provider directly and runs the built-in tools. This is the normal kind.

**`acp`** drives an external CLI agent over the Agent Client Protocol — Codex CLI, Claude Code, and other presets. It stores an `acp_runtime` blob instead of a provider, and Qunica only detects and launches the CLI: install it and sign in outside the app. See `external-cli-agents`.

For `llm_chat`, Qunica loads repository conventions from `AGENTS.md` in the primary workspace root, or `CLAUDE.md` when `AGENTS.md` is absent, with a 6,000-character limit. The section is refreshed every turn and is lower priority than host operating, approval, and workspace rules. Qunica removes it from ACP briefs because supported CLI agents load their own project instructions from the working directory.

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
| `WebSearch` | no | Search the web; needs a Tavily key in Settings |
| `Fetch` | no | Read one HTTP(S) URL, bounded |
| `GenerateImage` | yes | Generate an image and save it under `generations/`; needs Settings → Media |
| `GenerateVideo` | yes | Generate a video and save it under `generations/`; needs Settings → Media |
| `AskUser` | no | Pause and ask the user a question |
| `TodoWrite` | no | Keep a checklist of the current work, one status per item |
| `ExitPlanMode` | no | Present a plan for approval |
| `SkillManager` | no | List and load mounted skills |

An agent with no tools configured gets `Read`, `Glob`, and `Grep`. Shared-note tools are mounted automatically for members of a group that has a local workspace.

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

**Unattended mode** (`bypass_approvals` in `tool_config`) turns the whole review off: nothing pauses, and the refused class runs too. It is the equivalent of Codex YOLO mode or Claude Code's `--dangerously-skip-permissions`, and it is off for every agent that does not explicitly ask for it. Only enable it for an agent you trust in a workspace you can afford to lose.

## Vision

Image attachments are on by default for `llm_chat` agents. Set `vision` to `false` in `llm_config` for a text-only model.

## Notes

- Deleting an agent is a soft delete. Its history stays readable; the conversation reports the agent as unavailable.
- The built-in Assistant does not appear in the agent library and cannot be edited or deleted.
