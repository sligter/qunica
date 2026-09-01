<p align="center">
  <img src="assets/qunica-logo.png" alt="Qunica logo" width="132">
</p>

<h1 align="center">Qunica</h1>

<p align="center">
  <strong>One shared room for people and AI agents to plan, delegate, and ship.</strong>
</p>

<p align="center">
  <img src="https://img.shields.io/badge/version-0.1.1-c65d3b?style=flat-square" alt="Version 0.1.1">
  <img src="https://img.shields.io/badge/platform-Windows%20%7C%20macOS%20%7C%20Linux-3f6f91?style=flat-square" alt="Windows | macOS | Linux">
  <img src="https://img.shields.io/badge/data-local--first-4f7651?style=flat-square" alt="Local-first">
</p>

<p align="center">
  <a href="#what-qunica-is">Overview</a> ·
  <a href="#first-run">First run</a> ·
  <a href="#external-runtimes">Runtimes</a> ·
  <a href="#run-it">Run it</a> ·
  <a href="README.zh-CN.md">简体中文</a>
</p>

---

## Why “Qunica”?

**群** (*qún*, “the group”) meets **quorum**: enough people in one room to make progress. That is the product model too. A Qunica group brings people and agents into one conversation, around one set of project files.

## What Qunica is

Most agent tools give each agent a separate chat. Qunica gives the project a room.

A group holds the members, conversation history, shared notes, workspace, files, and execution trail. Each agent keeps its own model, prompt, tools, skills, and workspace access. Qunica’s scheduler decides who may speak next, enforces work budgets, and records what happened.

Built-in agents can use OpenAI-compatible, Anthropic, or Gemini providers. External CLI agents such as Codex and Claude Code join through the Agent Client Protocol (ACP).

## What you can do

- **Keep the project in one room.** People and agents share the same thread, notes, files, and working folder.
- **Control the conversation.** Choose `mesh`, `star`, `hierarchical`, or `ring` routing; run a bounded pass or let a moderator continue the discussion.
- **Delegate real work.** Agents can read and edit files, use guarded shell tools, call MCP servers, load skills, search the web, and hand work to another group member.
- **See every turn.** Streaming output, approvals, errors, token use, and dispatch traces stay attached to the conversation.
- **Work with the repository in place.** Browse and edit workspace files, inspect Git state and diffs, stage changes, commit, sync, or open the desktop terminal.
- **Keep control of the machine.** SQLite data, credentials, and workspaces remain with your local backend. Destructive shell actions require approval; blocked high-risk commands never run.

## First run

After creating the local account, Qunica walks through three choices:

1. Pick the root folder beneath which group workspaces may be created.
2. Connect a model provider with its endpoint, model, and API key.
3. Choose the default model for the built-in assistant.

Then create an agent, bind a workspace and tools, and invite it into a group. The full walkthrough is in [Getting started](backend-rs/crates/backend/src/docs/guide/getting-started.md).

## External runtimes

Qunica detects and launches agent CLIs that are already installed and signed in. It does not store their account credentials.

| Runtime | Command | Integration |
| --- | --- | --- |
| OpenAI Codex | `codex` | ACP with sandbox profiles |
| Claude Code | `claude` | streaming tool calls and permission handling |
| Pi Agent | `pi` | ACP adapter |
| OpenCode | `opencode` | ACP server |
| DeepSeek Harness | `dsh` | prompt-only ACP surface with fail-closed sandboxing |
| Custom ACP server | any | any compatible stdio command |

Full-auto CLI agents can modify everything their workspace and runtime permissions allow. Use a workspace whose contents you are willing to change. See [External CLI agents](backend-rs/crates/backend/src/docs/guide/external-cli-agents.md).

## Run it

Prerequisites: Node.js 20+, pnpm 9, and a stable Rust toolchain. Desktop builds are supported on Windows, macOS, and Linux.

```powershell
pnpm install
pnpm desktop:dev
```

Build the installer and portable executable:

```powershell
pnpm desktop:build
```

Artifacts are written under `frontend/src-tauri/target/release/bundle/`. For the browser UI against a local backend, use `pnpm dev`.

## Documentation

| Topic | Guide |
| --- | --- |
| Groups, routing, budgets, and shared notes | [Groups](backend-rs/crates/backend/src/docs/guide/groups.md) |
| Agents, built-in tools, and delegation | [Agents](backend-rs/crates/backend/src/docs/guide/agents.md) |
| Workspaces and files | [Workspaces](backend-rs/crates/backend/src/docs/guide/workspaces.md) · [Workspace files](backend-rs/crates/backend/src/docs/guide/workspace-files.md) |
| Providers, MCP, and skills | [Providers](backend-rs/crates/backend/src/docs/guide/providers.md) · [MCP](backend-rs/crates/backend/src/docs/guide/mcp-servers.md) · [Skills](backend-rs/crates/backend/src/docs/guide/skills.md) |
| Built-in assistant and terminal | [Assistant](backend-rs/crates/backend/src/docs/guide/assistant.md) · [Terminal](backend-rs/crates/backend/src/docs/guide/terminal.md) |

## Development

```powershell
pnpm type-check
pnpm lint
pnpm --filter @qunica/frontend test
cargo test --manifest-path backend-rs/Cargo.toml --workspace
```
- [Linux Do](https://linux.do/) — A community for developers, by developers.

<p align="center"><sub>Qunica · local-first multi-agent collaboration</sub></p
>
