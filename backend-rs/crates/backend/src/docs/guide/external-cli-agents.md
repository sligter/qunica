# External CLI agents

An agent with `runtime_kind` set to `acp` drives an external command-line agent instead of calling an LLM provider. It runs in the agent's workspace and streams its output into the chat.

## Supported runtimes

- **Codex CLI** — `codex-acp` (fallback: `npx -y @agentclientprotocol/codex-acp`).
- **Claude Code** — `claude -p --output-format stream-json --permission-mode bypassPermissions --max-turns <n> <prompt>`
- **Pi Agent** — the Pi ACP adapter (`pi`)
- **OpenCode** — the OpenCode ACP server (`opencode`)
- **DeepSeek Harness** — `dsh`, a Cordis plugin tree whose ACP surface is prompt-only (no `session/set_model` or tool-call updates over the wire). Its per-mode sandbox confinement (bwrap/Landlock/Seatbelt/Windows restricted tokens) fails closed with `SANDBOX_UNAVAILABLE` when unusable.
- **Custom** — any program speaking the Agent Client Protocol over stdio.

## Installing

Qunica detects and launches these CLIs. The runtime version panel can install a preset globally through npm and reports the installed and latest versions. Codex supports the sign-in entry below; sign in to other runtimes outside the app.

## Codex sign-in

In an Agent's create or edit form, choose **ACP → Codex**, then **Sign in with ChatGPT**. Open the displayed authorization link in a browser and enter the one-time code. Keep the settings page open until completion; Qunica then refreshes the runtime's model capabilities. **Check sign-in** detects an existing ChatGPT or API-key login without starting authorization.

Install the Codex CLI (`@openai/codex`) on the backend host in addition to the ACP adapter. The sign-in entry uses Codex's app-server account protocol so it receives device codes and completion events without parsing terminal output. The CLI must support device-code authentication. Enable device-code login in your ChatGPT account or workspace settings when required. See [OpenAI authentication](https://learn.chatgpt.com/docs/auth).

The login belongs to Codex on the connected backend machine. Codex stores and refreshes its credentials; Qunica receives no tokens. Agents that reuse that host's Codex login share the account. The Agent's runtime environment also applies to the login process. Canceling, leaving the form, or losing the stream terminates the temporary login process; login attempts are not automatically retried. Existing ACP sessions are not restarted. Existing externally established Codex logins remain usable.

## Configuration

- **command** and **args** — what to run.
- **model** — which model the CLI should use, when it accepts one.
- **thinking_effort** — reasoning depth, for runtimes that expose it. Qunica uses the runtime's declared `thought_level` config option, including model-dependent option IDs. Older adapters use profile-specific fallback names. A runtime that does not implement the setting emits a visible warning; a returned value that differs from the requested effort fails the turn before prompting. DeepSeek Harness applies its supported `off` / `high` / `max` settings through its managed Cordis configuration. The same session-setting path applies to container OpenCode.
- **timeout_seconds** — how long one run may take.
- **permission_policy** — how to answer the CLI's permission prompts.

The direct-chat composer offers a per-message thinking selector for API models
that advertise support. **Follow Agent** omits the override and uses the Agent's
saved configuration. ACP runtime choices remain in the Agent's runtime settings;
they can differ from the API model's five depth levels. A message-level effort
submitted through the API also takes precedence for ACP. Selecting a depth sends
the provider or runtime's native setting; it does not guarantee a particular
amount of reasoning or expose its private reasoning.

The configuration identifiers follow the runtime's
[ACP session config options](https://github.com/agentclientprotocol/agent-client-protocol/blob/main/docs/protocol/v2/schema.mdx).
API model levels follow their provider dialect, including
[OpenAI reasoning effort](https://developers.openai.com/api/reference/resources/chat/subresources/completions/methods/create).

## Shared group notes

In groups with a local workspace, Qunica automatically supplies the `qunica-group-notes` stdio MCP server on ACP `session/new` and `session/load`. It provides `ReadGroupNotes`, `CreateGroupNote` and `EditGroupNote`; the CLI may prefix the tool names. No user MCP configuration or additional interpreter is needed. Notes stay in the group's bound workspace even if the agent runs in an independent workspace or task worktree. See [Shared notes](groups.md#shared-notes) for arguments and lifecycle guarantees.

The executable's internal `--qunica-group-notes-mcp` mode runs only the stdio relay, before UI or backend startup. A session-owned loopback broker holds host authority; the relay receives an opaque capability via its MCP environment, not an account credential or database path. Operations are permitted only during the current dispatch. This preserves reusable ACP sessions without giving idle CLI processes continuing access to notes. Runtime adapters that ignore `mcpServers` do not gain these tools merely from the host prompt.

## Auditing

Every external run records its command, working directory, status, exit code, and the tail of stdout/stderr, so a failure can be diagnosed without rerunning it.

## Safety

These CLIs are configured for full-auto execution: they can read and write anything the account can, and the preset flags disable their own confirmation prompts. Bind them only to a workspace whose contents you are willing to have modified.
