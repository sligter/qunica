# Settings

Global configuration, under **Settings** in the sidebar.

## System settings

- **appearance** — `system`, `light`, or `dark`.
- **language** — `en-US` or `zh-CN`. Also sets the language agents are told to reply in.
- **group_workspace_root** — the parent directory `auto_create` puts new workspaces in.

## Web search

The `WebSearch` tool needs a provider configured here; without one it reports that setup is required.

- **web_search_provider** — currently `tavily`.
- **tavily_api_key** — write-only, like a provider key. Reads report only whether one is set.
- **tavily_search_url**, **tavily_max_results**, **tavily_search_depth** (`basic` or `advanced`)
- **tavily_include_answer**, **tavily_include_raw_content**

## Media generation

**Settings → Media** connects `GenerateImage` and `GenerateVideo` to one OpenAI-compatible provider. The API key is write-only; reads report only whether it is configured. Each tool is enabled by saving its default model, and generated files are written under `generations/` in the active agent workspace.

The image and video create endpoints may be relative to the shared base URL or complete HTTP(S) URLs. Video status and content endpoint templates use `{id}` for the generated job ID.

## Logs

**Settings → Logs** shows the launcher and backend logs. On the desktop app the log directory is also reachable from the tray menu, at:

```
%APPDATA%\qunica.desktop\logs
```

## Desktop data

```
%APPDATA%\qunica.desktop\qunica.sqlite3
%APPDATA%\qunica.desktop\desktop-secret.key
```

`desktop-secret.key` signs login tokens. Deleting it invalidates existing sessions; logging in again is enough to recover.

## Desktop behavior

- Closing the window hides the app to the tray instead of quitting. Quit from the tray menu.
- On startup the launcher clears any stale process holding TCP `127.0.0.1:8765`, so an old backend cannot block the new one.
