# The built-in assistant

The assistant is the floating panel that helps you configure Qunica, explains how its features work, and carries out setup tasks for you.

## What it can do on its own

- Read your configuration: agents, providers, MCP servers, skills, workspaces, groups, reusable group templates, shared group notes, and chats.
- Answer questions about the app from the bundled guide.
- Propose configuration changes for you to approve.
- Create a group or private chat and send its first message, or message an existing conversation, through the app-action approval flow.
- Inspect group members and propose adding or removing an Agent or user. User accounts are selected by exact email address.
- Save an existing group as a reusable template, or create and update shared group notes, through the same approval flow.
- Read and write scratch files, and run shell commands with the scratch workspace as their working directory.
- Search the web and fetch a page. `WebSearch` needs Tavily or DeepSeek configured under **Settings → Web search** first. `Fetch` uses DeepSeek Markdown when selected and configured, and otherwise works through direct HTTP without a key.

## Its scratch workspace

The assistant is bound to a workspace of its own, created for it the first time you open the dock. It lives in a `qunica-assistant` folder under the system temp directory — `/tmp/qunica-assistant` on Linux and macOS, the equivalent under `%TEMP%` on Windows — and it appears in your workspace list as **Assistant Workspace**.

The file tools `Read` and `Write` enforce workspace path limits. Use the scratch folder for drafts, notes, scratch scripts and one-off commands; treat anything left there as disposable, because the operating system may clear the temp directory between sessions. The folder is recreated whenever the dock loads, so an assistant whose workspace was swept still works.

Shell is different: its working directory is the scratch folder, but that is **not an operating-system sandbox**. Command-policy checks can allow a command, require approval, or block it; a configured decision model can also require approval. Allowed commands run without a confirmation card. When approval is required, the separate shell card offers a one-time decision or permission for the same policy rule in this thread. These checks do not confine programs launched by the shell to the scratch folder.

## What it cannot do

Its file tools cannot browse or edit files outside the scratch workspace. It can read app-managed shared group notes through app-control tools and propose note changes. For work on your own project files, create a regular agent with that workspace bound to it. Do not treat the scratch directory or shell command-policy checks as a filesystem isolation guarantee for shell processes.

Changes made through `AppPropose` are staged, not applied directly. Supported proposals create or update agents, skills, workspaces, groups, shared group notes and remote HTTP/SSE MCP servers; they can also save a group as a template, create a private chat, send messages or change group membership. In manual mode, a card lets you approve or reject the proposal. With **Apply assistant changes automatically** enabled, a mounted pending card calls the same approval endpoint automatically. This is not a background approver: without a mounted card, the setting alone does not process the queue. Only an applied result means the operation succeeded; approved can still mean it is running, and failed records the reason.

The history of these staged app actions, and what became of each one, is under **Settings → Assistant actions**, also linked from the panel and its settings. Read-only tools and scratch-file writes do not use this queue; shell approval is separate and is not disabled by the assistant's automatic app-action approval setting.

Some changes cannot be staged at all:

- Provider configuration, including API keys
- MCP servers that launch a local process (`stdio` transport), and secret-bearing environment or header fields
- CLI runtime installs (there is no dedicated app-control install action)
- Deleting resources (removing an Agent or user from a group is supported)

`AppPrefill` can open **provider creation** and **HTTP/SSE MCP creation** forms with supported non-sensitive fields filled in. HTTP also accepts the `streamable-http` name. Keys, headers, environment variables and stdio commands are never carried into those forms; URLs containing credentials, query parameters or fragments are omitted. You enter any credentials, review the form and submit it yourself. Opening a form does not save it. The desktop assistant uses the existing resource-library window; the browser uses normal navigation.

Other resource-form links are manual hand-offs without automatic field filling, including edit forms. There is no automatic stdio prefill, deletion form or CLI-install hand-off; those steps need instructions and the appropriate existing app entry point.

## Setting it up

The assistant is an LLM agent, so it needs its own provider binding before it can talk. This is separate from the providers your other agents use: adding a provider does not bind it.

Until one is bound, the panel shows the providers you have and lets you pick which the assistant should use. If you have none yet, it opens the provider form directly in the panel.

## Changing its settings

The gear icon in the panel header opens the assistant's settings at any time:

- **Provider** — which one it calls. Clearing it stops the assistant chatting until another is chosen.
- **Model** — which model to use, or the provider's default. Only models the selected provider offers are listed, and switching providers clears a model the new one does not have.
- **Apply assistant changes automatically** — whether displayed pending app-action cards call the approval endpoint without another click. The current mode and an Assistant actions history link are shown in settings; the browser panel also shows a compact status above the conversation.

Its prompt, tools and scratch workspace binding are fixed and are not editable through Assistant settings.

The composer can adjust reasoning effort per message when the selected model supports it. It does not offer per-message model switching; choose the model in Assistant settings.

## The panel

In the browser, the collapsed panel sits as an icon in a corner. Expanded, it can be dragged by its title bar and resized from any edge or corner, and snaps to a corner when dragged near one. Its position and size are remembered in this browser. `Esc` collapses it after any open dialog handles the key.

On desktop, the launcher opens a separate, resizable, always-on-top window. Hiding and reopening it reuses the window; a new window starts at its default size near the screen's bottom-right corner. Use its collapse button to hide it. On compact browser layouts, the header entry opens a bottom sheet.
