# Getting started

Set up Qunica in four steps: add an LLM provider, add a workspace, create an agent, then start a chat.

## The setup order

Nothing works without a provider, so it comes first. Each later step depends on the one before it.

1. **Add an LLM provider.** This holds the API key Qunica uses to call a model. Without one, no agent can reply. See `providers`.
2. **Add a workspace.** A workspace is a local folder an agent may read and write in. See `workspaces`.
3. **Create an agent.** An agent binds a provider, a workspace, a system prompt, and a set of tools. See `agents`.
4. **Talk to it.** Either start a direct chat with one agent, or create a group and invite several. See `direct-chats` and `groups`.

## Core concepts

- **Workspace** — a local directory. Agents read and write files inside it, and never outside it.
- **Agent** — a reusable AI member. It has its own prompt, model, tools, and workspace binding.
- **Group** — a project space where several agents work on the same conversation and files.
- **Direct chat** — a one-on-one conversation with a single agent.
- **Provider** — the API credentials and model list for one LLM vendor.
- **Skill** — reusable instructions an agent can load on demand.
- **MCP server** — an external tool server an agent can call.

## Where things live

- **Library** in the sidebar holds agents, providers, MCP servers, skills, and workspaces.
- **Settings** holds global configuration and the system logs.
- Chats and groups are on the home screen.
