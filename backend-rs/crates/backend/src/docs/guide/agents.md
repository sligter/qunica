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

**`acp`** drives an external CLI agent over the Agent Client Protocol — Codex CLI, Claude Code, and other presets. It stores an `acp_runtime` blob instead of a provider, and AG Swarmer only detects and launches the CLI: install it and sign in outside the app. See `external-cli-agents`.

## Built-in tools

| Tool | Needs a workspace | Does |
| --- | --- | --- |
| `Read` | yes | Read a UTF-8 file, with offset and limit |
| `Write` | yes | Create or overwrite a file |
| `Edit` | yes | Exact-match replacements in one file |
| `Glob` | yes | Find files by pattern |
| `Grep` | yes | Search file contents |
| `Bash` | yes | Run a guarded shell command in the root |
| `WebSearch` | no | Search the web; needs a Tavily key in Settings |
| `Fetch` | no | Read one HTTP(S) URL, bounded |
| `GenerateImage` | yes | Generate an image and save it under `generations/`; needs Settings → Media |
| `GenerateVideo` | yes | Generate a video and save it under `generations/`; needs Settings → Media |
| `AskUser` | no | Pause and ask the user a question |
| `TodoWrite` | no | Keep a checklist of the current work, one status per item |
| `ExitPlanMode` | no | Present a plan for approval |
| `SkillManager` | no | List and load mounted skills |

An agent with no tools configured gets `Read`, `Glob`, and `Grep`.

`RunSubAgent` is present as a saved-only placeholder and is not exposed to the runtime yet.

## Vision

Image attachments are on by default for `llm_chat` agents. Set `vision` to `false` in `llm_config` for a text-only model.

## Notes

- Deleting an agent is a soft delete. Its history stays readable; the conversation reports the agent as unavailable.
- The built-in Assistant does not appear in the agent library and cannot be edited or deleted.
