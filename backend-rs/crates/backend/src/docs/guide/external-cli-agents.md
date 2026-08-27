# External CLI agents

An agent with `runtime_kind` set to `acp` drives an external command-line agent instead of calling an LLM provider. It runs in the agent's workspace and streams its output into the chat.

## Supported runtimes

- **Codex CLI** — `codex exec --sandbox danger-full-access <prompt>`
- **Claude Code** — `claude -p --output-format stream-json --permission-mode bypassPermissions --max-turns <n> <prompt>`
- **Pi Agent** — the Pi ACP adapter (`pi`)
- **OpenCode** — the OpenCode ACP server (`opencode`)
- **DeepSeek Harness** — `dsh`, a Cordis plugin tree whose ACP surface is prompt-only (no `session/set_model` or tool-call updates over the wire). Its per-mode sandbox confinement (bwrap/Landlock/Seatbelt/Windows restricted tokens) fails closed with `SANDBOX_UNAVAILABLE` when unusable.
- **Custom** — any program speaking the Agent Client Protocol over stdio.

## Installing

Qunica detects and launches these CLIs; it does not manage their accounts. Install each one and sign in outside the app. The runtime version panel can install a preset globally through npm and reports the installed and latest versions.

## Configuration

- **command** and **args** — what to run.
- **model** — which model the CLI should use, when it accepts one.
- **thinking_effort** — reasoning depth, for runtimes that expose it. Codex and Claude Code spell this differently; Qunica maps it per profile.
- **timeout_seconds** — how long one run may take.
- **permission_policy** — how to answer the CLI's permission prompts.

## Auditing

Every external run records its command, working directory, status, exit code, and the tail of stdout/stderr, so a failure can be diagnosed without rerunning it.

## Safety

These CLIs are configured for full-auto execution: they can read and write anything the account can, and the preset flags disable their own confirmation prompts. Bind them only to a workspace whose contents you are willing to have modified.
