<p align="center">
  <img src="assets/qunica-logo.png" alt="Qunica logo" width="160">
</p>

<h1 align="center">Qunica</h1>

<p align="center">
  <strong>A group-first workbench where humans and AI agents build in the same room.</strong>
</p>

<p align="center">
  <img src="https://img.shields.io/badge/version-0.1.1_alpha-informational?style=flat" alt="version">
  <img src="https://img.shields.io/badge/desktop-Windows-0078D4?style=flat" alt="platform">
  <img src="https://img.shields.io/badge/stack-Tauri%20%2B%20Rust%20%2B%20React-informational?style=flat" alt="stack">
</p>

<p align="center">
  <a href="#what-is-qunica">Overview</a>
  ·
  <a href="#get-started">Get started</a>
  ·
  <a href="#build-the-room">Features</a>
  ·
  <a href="#runtimes">Runtimes</a>
</p>

<p align="center">
  <b>English</b> | <a href="README.zh-CN.md">简体中文</a>
</p>

<p align="center">
  <sub><em>One project group. Many agents. Shared history, files, and workspace.</em></sub>
</p>

---

## Why "Qunica"?

**群** (qún, “the group”) + **quorum** — the minimum number of members gathered before real decisions can happen. A project group in Qunica is exactly that: humans and agents assembled in one room, and only then does work start. Pronounced /ˈkwiːnɪkə/ (“KWEE-ni-kuh”).

The logo tells the same story: five speech bubbles converging on a single spark — the quorum itself.

---

## What is Qunica?

Most AI products hand you one chat window and one agent. Real work looks like a team: product, research, engineering, review, docs — different roles, one project context. The more agents you add, the more of your day goes to passing context between tabs that pretend to know about each other.

Qunica treats that project as a **group**. Agents are members you invite, configure, and observe; humans and agents share the same conversation, files, workspace, and execution trail. Every agent gets its own model, tools, skills, and workspace binding — including external CLI agents driven over the Agent Client Protocol (Codex CLI, Claude Code, Pi, OpenCode, DeepSeek Harness) that work for real inside a workspace you chose.

The bet is simple: multi-agent work gets better when the **room** is the product — one group, one conversation log, one workspace boundary — instead of every agent inventing its own chat silo.

---

## Build the room.

*Groups, agents, and one shared context — set a project up once and keep it running.*

- **[Groups](backend-rs/crates/backend/src/docs/guide/groups.md) →** A group is the container. Invite specialized agents the way you'd invite teammates; everyone shares one history and one workspace.
- **[Group templates](backend-rs/crates/backend/src/docs/guide/groups.md) →** Save a roster and its settings as a reusable template, then stamp out the next project. Name, avatar, and workspace are still chosen per group.
- **[Agents](backend-rs/crates/backend/src/docs/guide/agents.md) →** Name, prompt, model, tools, skills, and workspace per member. The same agent can join several groups and hold several chats.
- **[Direct chats](backend-rs/crates/backend/src/docs/guide/direct-chats.md) →** One-on-one conversations on the same scheduler, for work that needs no coordinating.
- **Task threads →** Pin several work streams inside one conversation; archive, restore, delete, or clear each one on its own.
- **[Shared notes](backend-rs/crates/backend/src/docs/guide/groups.md#shared-notes) →** A group scratchpad in Markdown that every member can read and edit, next to the chat rather than inside it.

## Set the conversation.

*Who speaks, in what order, under which budget — you set the rules, the scheduler enforces them.*

- **[Communication topologies](backend-rs/crates/backend/src/docs/guide/groups.md#communication-modes) →** `mesh`, `star`, `hierarchical`, or `ring` define the legal speaking route; `speaking_order` fixes it deterministically.
- **[Scheduler modes](docs/GROUP_SCHEDULER.md) →** `bounded` runs within work budgets; `automatic` lets a moderator keep dispatching or close the turn. Both share one persisted scheduler for groups and direct chats.
- **A moderator →** An agent with its own provider and model that picks the next legal speaker, instead of leaving routing to fixed order.
- **@-mentions →** Pick responders in mention mode, or pick the *first* speaker in group-wide mode. Mentions written by agents are display-only — they never dispatch anyone.
- **[Budgets and failure fuses](docs/GROUP_SCHEDULER.md#budget-profiles) →** Cap agent steps, per-agent steps, handoff hops, moderator calls, and tokens; consecutive failures stop the turn instead of burning it down.
- **[AgentAsTool](docs/GROUP_SCHEDULER.md) →** Structured delegation: `call` a helper privately and get the result back, or `handoff` the public reply — without ever waking the same agent twice.

## Hand them real work.

*A workspace you chose, tools you allowed, and an audit trail for everything that ran.*

- **[Workspaces](backend-rs/crates/backend/src/docs/guide/workspaces.md) →** The boundary: file and shell tools resolve every path against a root, and anything that escapes it is rejected.
- **[Built-in tools](backend-rs/crates/backend/src/docs/guide/agents.md#built-in-tools) →** Read, Write, Edit, Glob, Grep, guarded Bash, WebSearch, Fetch, image and video generation, AskUser, TodoWrite, and plan approval.
- **Approval gates →** Destructive Bash pauses the turn for your sign-off — remember a rule for the thread, or enable unattended mode for an agent you trust in a workspace you can afford to lose. A refused class (formatting volumes, host shutdown) never runs.
- **[External CLI agents](backend-rs/crates/backend/src/docs/guide/external-cli-agents.md) →** Drive Codex CLI, Claude Code, Pi, OpenCode, and DeepSeek Harness over ACP, plus any custom ACP server. Every run keeps its command, cwd, status, exit code, and stdout/stderr tails.
- **[MCP servers](backend-rs/crates/backend/src/docs/guide/mcp-servers.md) →** Register `stdio`, Streamable HTTP, or SSE servers; tools appear namespaced as `mcp__<server>__<tool>` with server allowlists and per-agent picks.
- **[Skills](backend-rs/crates/backend/src/docs/guide/skills.md) →** Reusable instruction blocks an agent loads through `SkillManager` when it needs one, imported raw, as a package, or from GitHub.
- **[Providers](backend-rs/crates/backend/src/docs/guide/providers.md) →** OpenAI-compatible, Anthropic, and Gemini dialects; model discovery runs server-side so your key never leaves the machine.
- **Per-message overrides →** Switch model or thinking level for a single message.
- **Workspace Git →** Status, branches, diffs, history, staging, commits, and sync from inside the app.

## Stay in the loop.

*Every turn is persisted, replayable, and auditable — you watch the work instead of chasing it.*

- **Live streaming →** Tokens, messages, errors, and turn traces appear in the room as they happen.
- **[Turn traces](docs/GROUP_SCHEDULER.md) →** Which agent ran, why it was selected, and what it cost — persisted per dispatch and per turn.
- **[Terminal](backend-rs/crates/backend/src/docs/guide/terminal.md) →** Desktop-only tabbed shell docked to the conversation (`Ctrl`/`Cmd` + `` ` ``). It starts in the workspace but is deliberately *not* sandboxed — read the guide before using it.
- **[Built-in assistant](backend-rs/crates/backend/src/docs/guide/assistant.md) →** Configuration help with staged changes that apply only after you approve. It never touches files and never reads raw secrets.
- **Logs →** Launcher and backend logs in the app, or on disk under `%APPDATA%\qunica.desktop\logs`.

## Own the machine.

*Local-first: your data stays on your disk, and the only service running is the one you started.*

- **Windows desktop →** Tauri 2 shell with the Rust backend in-process: tray-resident, native folder picker, and a portable executable that needs no install.
- **Browser build →** The same React frontend runs against a local backend (`pnpm dev`) when you don't want the desktop shell.
- **SQLite storage →** Groups, agents, turns, and history live in `%APPDATA%\qunica.desktop\qunica.sqlite3`. Login tokens are signed with a locally generated key.
- **Not a hosted SaaS (yet) →** Register and log in against your own backend — there is no account on someone else's server.

---

## Get started

Current packaging target is Windows. Build from source:

```powershell
pnpm install
pnpm desktop:build
```

Artifacts:

```text
frontend/src-tauri/target/release/bundle/nsis/Qunica_<version>_x64-setup.exe
frontend/src-tauri/target/release/bundle/portable/Qunica_<version>_x64-portable.exe
```

Run the portable executable directly — no installation. Development builds:

```powershell
pnpm dev          # web UI in the browser
pnpm desktop:dev  # desktop app in dev mode
```

### Your first agent in five minutes

1. **Add a provider.** The API key Qunica uses to call a model; without one, no agent can reply.
2. **Add a workspace.** A local folder an agent may read and write in.
3. **Create an agent.** Bind a provider, a workspace, a system prompt, and a set of tools.
4. **Talk to it.** Open a direct chat with one agent, or create a group and invite several.

Full walkthrough: [getting started](backend-rs/crates/backend/src/docs/guide/getting-started.md).

---

## Runtimes

Qunica does not ship a model. It drives the agent CLIs you already have installed and signed in, so switching is a dropdown, not a migration.

| Runtime | CLI | Notes |
| --- | --- | --- |
| Claude Code | `claude` | tool-call stream with permission handling |
| OpenAI Codex | `codex` | sandboxed execution profiles |
| Pi Agent | `pi` | ACP adapter |
| OpenCode | `opencode` | ACP server |
| DeepSeek Harness | `dsh` | prompt-only ACP surface; per-mode sandbox confinement that fails closed when unavailable |
| Custom ACP server | any | anything speaking the Agent Client Protocol over stdio |

For example, `codex` runs as `codex exec --sandbox danger-full-access <prompt>` and `claude` as `claude -p --output-format stream-json --permission-mode bypassPermissions --max-turns <n> <prompt>`. Qunica detects and launches these CLIs; it does **not** store their account credentials. Install and sign in outside the app, and treat full-auto CLI agents as powerful: bind only workspaces whose contents you are willing to have modified. Details: [external CLI agents](backend-rs/crates/backend/src/docs/guide/external-cli-agents.md).

---

## Documentation

| I want to… | Start here |
| --- | --- |
| Understand the pieces and set up today | [Getting started](backend-rs/crates/backend/src/docs/guide/getting-started.md) |
| Create groups, routing, and conversation rules | [Groups](backend-rs/crates/backend/src/docs/guide/groups.md) · [Scheduler design](docs/GROUP_SCHEDULER.md) |
| Configure agents and their tools | [Agents](backend-rs/crates/backend/src/docs/guide/agents.md) · [Skills](backend-rs/crates/backend/src/docs/guide/skills.md) |
| Drive external CLI agents over ACP | [External CLI agents](backend-rs/crates/backend/src/docs/guide/external-cli-agents.md) |
| Connect MCP tool servers | [MCP servers](backend-rs/crates/backend/src/docs/guide/mcp-servers.md) |
| Configure providers and global settings | [Providers](backend-rs/crates/backend/src/docs/guide/providers.md) · [Settings](backend-rs/crates/backend/src/docs/guide/settings.md) |
| Work with files and workspaces | [Workspaces](backend-rs/crates/backend/src/docs/guide/workspaces.md) · [Workspace files](backend-rs/crates/backend/src/docs/guide/workspace-files.md) |
| Use the terminal or the built-in assistant | [Terminal](backend-rs/crates/backend/src/docs/guide/terminal.md) · [Assistant](backend-rs/crates/backend/src/docs/guide/assistant.md) |

The full guide lives under [`backend-rs/crates/backend/src/docs/guide/`](backend-rs/crates/backend/src/docs/guide/).

---

## Development

Prerequisites: [Node.js](https://nodejs.org/) ≥ 20, [pnpm](https://pnpm.io/) 9, a stable [Rust](https://rust-lang.org/) toolchain; Windows for desktop packaging.

```powershell
pnpm install
pnpm dev          # web UI with Hot Module Reload
pnpm desktop:dev  # desktop app in dev mode
```

Quality checks:

```powershell
pnpm type-check
pnpm lint
cargo fmt --manifest-path backend-rs/crates/backend/Cargo.toml --all --check
cargo clippy --manifest-path backend-rs/Cargo.toml --workspace --all-targets -- -D warnings
cargo test --manifest-path backend-rs/Cargo.toml --workspace
```

---

<p align="center">
  <sub>Qunica</sub><br>
  <sub>Local-first multi-agent collaboration · v0.1.1-alpha</sub>
</p>