# The built-in assistant

The assistant is the floating panel that helps you configure Qunica, explains how its features work, and carries out setup tasks for you.

## What it can do on its own

- Read your configuration: agents, providers, MCP servers, skills, workspaces, groups, reusable group templates, shared group notes, and chats.
- Answer questions about the app from the bundled guide.
- Propose configuration changes for you to approve.
- Create a group or private chat and send its first message, or message an existing conversation, after you approve the action.
- Inspect group members and propose adding or removing an Agent or user. User accounts are selected by exact email address.
- Save an existing group as a reusable template, or create and update shared group notes, after you approve the action.
- Read and write files, and run shell commands, inside its own scratch workspace.
- Search the web and fetch a page. `WebSearch` needs a provider configured under **Settings → Web search** first; `Fetch` works without one.

## Its scratch workspace

The assistant is bound to a workspace of its own, created for it the first time you open the dock. It lives in a `qunica-assistant` folder under the system temp directory — `/tmp/qunica-assistant` on Linux and macOS, the equivalent under `%TEMP%` on Windows — and it appears in your workspace list as **Assistant Workspace**.

That folder is the only place its `Read`, `Write` and `Bash` tools can reach. Use it for drafts, notes, scratch scripts and one-off commands; treat anything left there as disposable, because the operating system may clear the temp directory between sessions. The folder is recreated whenever the dock loads, so an assistant whose workspace was swept still works.

Shell commands go through the same approval cards as any other agent's: you see the command and approve or reject it before it runs.

## What it cannot do

It cannot browse or edit files outside that scratch workspace. It can read app-managed shared group notes and propose note changes; for work on your own project files, create a regular agent with that workspace bound to it.

Every configuration change it makes is staged, never applied directly. You see a card describing the change and approve or reject it; nothing is written until you approve. The history of everything it proposed, and what became of each one, is under **Settings → Assistant actions**.

Some changes it cannot stage at all, and will instead hand you a prefilled form to complete yourself:

- Provider API keys
- MCP servers that launch a local process (`stdio` transport)
- CLI runtime installs
- Deleting resources (removing an Agent or user from a group is supported)

## Setting it up

The assistant is an LLM agent, so it needs its own provider binding before it can talk. This is separate from the providers your other agents use: adding a provider does not bind it.

Until one is bound, the panel shows the providers you have and lets you pick which the assistant should use. If you have none yet, it opens the provider form directly in the panel.

## Changing its settings

The gear icon in the panel header opens the assistant's settings at any time. Two things are configurable:

- **Provider** — which one it calls. Clearing it stops the assistant chatting until another is chosen.
- **Model** — which model to use, or the provider's default. Only models the selected provider offers are listed, and switching providers clears a model the new one does not have.

Its prompt, its tools, and the scratch workspace it is bound to are fixed. Those are what make it safe to give app-control tools, so they are not editable.

Its model can also be chosen per message from the composer, which overrides the setting above for that one message.

## The panel

Collapsed, it sits as an icon in a corner. Expanded, it can be dragged by its title bar and resized from any edge or corner, and snaps to a corner when dragged near one. Its position and size are remembered. `Esc` collapses it.
