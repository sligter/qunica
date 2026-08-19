<p align="center">
  <img src="assets/ag-swarmer-logo.png" alt="AG Swarmer Logo" width="160">
</p>

<h1 align="center">AG Swarmer</h1>

<p align="center">
  <strong>A group-first workbench where humans and multiple agents build in the same room.</strong>
</p>

<p align="center">
  <a href="README.zh-CN.md">中文</a>
  ·
  <a href="#what-is-this-really">Overview</a>
  ·
  <a href="#getting-started">Getting started</a>
  ·
  <a href="#architecture">Architecture</a>
  ·
  <a href="#current-status">Status</a>
</p>

<p align="center">
  <sub><em>One project group. Many agents. Shared history, files, and workspace.</em></sub>
</p>

---

## What is this, really?

AG Swarmer is a multi-agent collaboration workbench built around **groups** as the primary container.

Most AI products give you one chat window and one agent. Real work looks more like a team: product, research, engineering, review, docs — different roles, one project context. AG Swarmer treats that project as a **group**. Agents are members you invite, configure, and observe. Humans and agents share the same message history, files, workspace, and execution trail.

Agents are not bolted-on bots. They are first-class members with roles, models, tools, skills, MCP servers, and optional external CLI runtimes (Codex CLI, Claude Code) that execute inside a bound workspace.

---

## Stuff you do in AG Swarmer

- **Stand up a project group** and invite specialized agents the way you'd invite teammates.
- **Keep long-lived project context** — messages, files, workspace state, run logs, and agent replies stay in one place.
- **Give each agent a job** — different models, tools, skills, and workspace bindings per member.
- **Watch work stream in** — token streams, final messages, errors, and turn traces show up live in the room.
- **Let external CLI agents touch real files** in a user-confirmed workspace, with audit trails for command, cwd, exit code, and stdout/stderr tails.
- **Review workspace Git state** — branches, diffs, history, staging, commits, and sync actions stay in the app.
- **Wire MCP tools** (stdio, Streamable HTTP, SSE) so agents call external capabilities like first-class tools.
- **Use the built-in assistant** for configuration help and staged app changes that only apply after you approve them.
- **Run light on your machine** with the Windows desktop build: tray resident, native folder picker, portable executable.

---

## Why this shape

One group. One conversation log. One workspace boundary.

Humans, LLM agents, external CLI agents, MCP tools, and skills all meet in the same project room instead of seven tabs that pretend to know about each other. Scope comes from membership, workspace binding, and tool allowlists — closer to how you'd scope a teammate than a pile of global permission flags.

The bet is simple: multi-agent work gets better when the **room** is the product, not when every agent invents its own chat silo.

---

## Three little stories

**Feature swarm.** You drop a PRD into a group with product, backend, frontend, and test agents. They discuss, draft interfaces, propose tests, and leave a durable trail of *why* the plan looks the way it does.

**Workspace-bound implementation.** You bind a local repo as the group workspace, bring in Codex or Claude Code, and watch patches land where you can see them — same room as the discussion, with runtime audit for every external command.

**Safe config help.** You're stuck on providers or MCP. The built-in assistant proposes changes as staged actions; nothing mutates until you click approve. Secrets stay masked; destructive paths stay out of its reach.

---

## Current status

| ✅ Works today | 🚧 Being improved | 💭 Direction, not a promise |
|---|---|---|
| Auth (register / login / JWT) | Cross-platform desktop packaging beyond Windows | Richer multi-agent orchestration policies |
| Groups, agents, membership, group chat | Deeper workspace git review UX | Agent marketplace / pack distribution |
| Streaming replies, clear history, turn traces | Mobile / lightweight remote view | Stronger enterprise knowledge-base hooks |
| Workspace file browse, reference, UTF-8 edit, safe previews | Data migration from legacy Docker/Postgres stacks | — |
| Workspace Git: status, branches, diffs, history, stage/commit/sync | — | — |
| LLM provider config, per-message model & thinking overrides | — | — |
| Skills management and injection | — | — |
| MCP: stdio · Streamable HTTP · SSE | — | — |
| External CLI runtimes: Codex CLI, Claude Code | — | — |
| Built-in assistant with approve-gated app actions | — | — |
| Windows desktop: Tauri shell, in-process Rust backend, SQLite, tray, portable executable | — | — |

<sub>Plan product bets around the ✅ column. The 💭 column is product direction, not a shipping checklist.</sub>

---

## Getting started

### I want the Windows desktop app

Build from source (current packaging target is Windows):

```powershell
pnpm install
pnpm desktop:build
```

Artifacts:

```text
frontend/src-tauri/target/release/bundle/nsis/AG Swarmer_<version>_x64-setup.exe
frontend/src-tauri/target/release/bundle/portable/AG Swarmer_<version>_x64-portable.exe
```

Portable build: run the standalone `AG Swarmer_<version>_x64-portable.exe` directly.

### I want to develop the web UI

```powershell
pnpm install
pnpm dev
```

### I want desktop dev mode

```powershell
pnpm install
pnpm desktop:dev
```

### Quality checks

```powershell
pnpm type-check
pnpm lint
```

Rust backend:

```powershell
cargo fmt --manifest-path backend-rs/crates/backend/Cargo.toml --all --check
cargo clippy --manifest-path backend-rs/Cargo.toml --workspace --all-targets -- -D warnings
cargo test --manifest-path backend-rs/Cargo.toml --workspace
```

---

## Core concepts

| Concept | Meaning |
|---|---|
| **Group** | Project room — the main collaboration container |
| **Agent** | Reusable AI member that can join one or more groups |
| **Workspace** | Local directory bound to a group; external agents and tools read/write here |
| **Runtime** | How an agent runs — LLM chat runtime or external CLI runtime |
| **Streaming** | Live token / message / error events in the UI |
| **Audit** | External runs record command, cwd, status, exit code, stdout/stderr tail |
| **MCP** | Model Context Protocol servers exposed as `mcp__<server>__<tool>` |
| **Assistant** | System agent for help + staged configuration changes (approve to apply) |

---

## Feature highlights

### MCP tools

Register MCP servers in the resource library. Agents call their tools like built-ins.

| Transport | Notes |
|---|---|
| `stdio` | Local process, line-delimited JSON-RPC 2.0 on stdin/stdout |
| `Streamable HTTP` | Single HTTP endpoint; JSON or SSE responses; `Mcp-Session-Id` for sessions |
| `SSE` (legacy) | GET event stream + POST message endpoint |

- Tools are namespaced: `mcp__<server-slug>__<tool-name>`.
- Per-server allowlists and per-agent tool picks both apply.
- “Test connection” lists live tools before save.
- A failed server only drops *its* tools for that turn; the turn continues with a note in the system prompt.
- stdio inherits process env + config overrides; HTTP header values are masked in API responses.

### Workspace files

- Dragging a file into chat stores a **workspace-relative reference**, not a copy; the server checks the path belongs to the bound workspace.
- Dragging a directory inserts the relative path at the cursor (no recursive file dump).
- In-app edit is UTF-8 text only, with content digests so external edits don't get silently overwritten.
- HTML preview runs in a locked-down sandbox iframe; images/PDFs use size-limited safe previews.
- Office / unknown formats: metadata + download only.

### Built-in assistant

Floating panel for setup help and in-app actions. It is a system (`is_system`) agent on a normal direct chat — streaming, resume, interrupt, and turn traces reuse the same machinery. It is hidden from the agent library and generic agent CRUD.

**Boundaries:**

- No workspace, file tools, or shell. File work belongs to workspace-bound agents.
- Config changes are **staged** via `AppPropose` until you approve. Approve runs the same handlers the UI uses.
- Cannot stage: provider API keys, stdio MCP servers, CLI runtime installs, deletes — only `AppPrefill` form links.
- Reads never return raw secrets.

History lives under **Settings → Assistant action log**. Gear on the panel title configures its provider/model; prompt, tools, and “no workspace” stay fixed. Bind a provider to the assistant explicitly — adding a provider does not auto-bind it.

### Per-message model & thinking

The composer can override model and thinking level for a single message.

- Model picker appears only for single-agent sessions when the provider exposes multiple models.
- Thinking controls appear only when the model declares support (OpenAI `reasoning_effort`, Anthropic `thinking.budget_tokens`, Gemini `thinkingConfig`).
- Five levels — `low`, `medium`, `high`, `xhigh`, `max` — each its own depth. OpenAI-compatible endpoints receive the level itself and never a token budget; Anthropic and Gemini receive a budget that grows with it (and Anthropic's `max_tokens` grows with the budget).

### External CLI agents

Separate from LLM chat runtimes. They run in the resolved workspace and stream back into chat.

| Runtime | Invocation shape (illustrative) |
|---|---|
| Codex CLI | `codex exec --sandbox danger-full-access <prompt>` |
| Claude Code | `claude -p --output-format stream-json --permission-mode bypassPermissions --max-turns <n> <prompt>` |

The app detects and launches CLIs; it does **not** store their account credentials. Install and log in outside the app. Treat full-auto CLI agents as powerful: only bind workspaces you trust.

---

## Architecture

```text
frontend/
  React + Vite + TypeScript + TanStack Query + Zustand
  Tauri desktop shell under src-tauri/; links the Rust backend in process

backend-rs/
  Rust backend workspace
  Axum HTTP API and API v2 runtime
  SQLite desktop data storage
  External CLI runtime adapters

shared/
  Cross-package TypeScript events / contracts
```

Desktop process shape:

```text
AG Swarmer.exe
  ├─ Tauri WebView shell
  ├─ starts the Rust / Axum backend in process
  ├─ listens on http://127.0.0.1:8765
  └─ frontend talks to backend via runtime API base URL
```

```text
┌──────────────────────────────────────────────────────────────┐
│ Clients                                                      │
│  Web (Vite)          Desktop (Tauri WebView)                 │
└──────────────┬───────────────────────────┬───────────────────┘
               │ HTTP / SSE-style streams  │
               ▼                           ▼
┌──────────────────────────────────────────────────────────────┐
│ ag-swarmer-backend (Rust / Axum)                             │
│  auth · groups · agents · chat · workspace · MCP · runtimes  │
└──────────────┬───────────────────────────┬───────────────────┘
               │                           │
        ┌──────▼──────┐             ┌──────▼──────┐
        │   SQLite    │             │  Workspace  │
        │  (desktop)  │             │  + CLI / MCP│
        └─────────────┘             └─────────────┘
```

---

## What it is not

- **Not a single-chat chatbot.** The unit of work is a group with members.
- **Not a hosted multi-tenant SaaS product (yet).** Desktop defaults to local SQLite on your machine.
- **Not finished.** Windows is the first packaging target; treat external CLI full-auto modes with care.

**What it is:** a group-shaped multi-agent workbench — shared context, visible execution, and agents that can actually work in a workspace you chose.

---

<p align="center">
  <sub>AG Swarmer</sub><br>
  <sub>Local-first multi-agent collaboration · v0.1.1-alpha</sub>
</p>
