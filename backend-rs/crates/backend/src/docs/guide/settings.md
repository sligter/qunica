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

## Decision model

**Settings → Decision model** connects a System One decision model (TypeSafe's Jev) through any of three endpoints. Unlike a chat model it generates no text: it evaluates a small `state` against typed questions and returns calibrated answers — the probability of a yes, one option out of a set with a confidence, or a position on an ordered scale. The runtime uses it for narrow judgments it otherwise puts to a chat model or a fixed rule.

| Endpoint | URL | Model name |
| --- | --- | --- |
| TypeSafe | `https://api.typesafe.ai/v1/systemone` | `jev-latest` |
| OpenRouter | `https://openrouter.ai/api/alpha/decisions` | `~typesafe/jev-latest` |
| Vercel AI Gateway | `https://ai-gateway.vercel.sh/v4/ai/evaluation-model` | `typesafe-ai/jev` |

The wire dialect follows the URL. A path ending in `/evaluation-model` is the AI SDK evaluation protocol that Vercel AI Gateway serves: the model travels in the `ai-model-id` header, a yes/no question is sent as `boolean`, and answers carry no `confidence`. Qunica then takes TypeSafe's confidence from the gateway's provider metadata when it is forwarded and otherwise uses the largest probability in the answer's distribution, which is a little more permissive than TypeSafe's own statistic. Every other URL is spoken to as System One. A Vercel key comes from the AI Gateway → API Keys page or `vercel ai-gateway api-keys create`.

The endpoint is account-level; whether a group uses it, and for which scenarios, is set on each group under **Group settings → Decision model**.

- **decision_endpoint** — the full decisions URL from the table above, not a base URL.
- **decision_api_key** — write-only; reads report only whether one is set.
- **decision_model** — the model name for the chosen endpoint (table above). Pin a versioned ID once thresholds are tuned; aliases move.
- **decision_min_confidence** — a `choice` answer below this confidence counts as no opinion (0 to 1, default 0.7).

Per group (`PATCH /api/v2/groups/{id}`, and saved into group templates):

- **decision_enabled** — the group's switch. Off by default.
- **decision_scenarios** — one switch per scenario, all off by default. A partial object updates only the named scenarios; `null` turns all off:

| Scenario | What it does | Fallback |
| --- | --- | --- |
| `moderator_selection` | Bounded and automatic turns with a moderator: picks the next legal speaker. Automatic selection includes a `defer` option when no concrete unfinished work is supported, including with one candidate. The dispatch records reason `decision_model`. | On defer, failure or low confidence, the moderator chat model is asked as before |
| `automatic_finish` | Automatic turns: ends the turn when the objective reads as complete (probability ≥ 0.85), before the moderator is called | The moderator decides |
| `proactive_prefilter` | Proactive and everyone modes: skips members the latest message clearly does not call for (probability < 0.15). Mentioned members are never skipped; at least one member always runs. A `warning` event with code `decision_prefilter` lists who was skipped | Everyone is dispatched |
| `shell_risk` | A second opinion on shell commands the fixed policy allowed. It can only add an approval card (rule `decision-risk`), never remove one | The policy verdict stands |
| `skill_suggestion` | Names the one mounted skill or MCP server the request calls for, as one line in the system prompt | No suggestion |
| `note_validation` | `EditGroupNote` refuses a note that sets `Status: implemented` without verification evidence in its Decision section | Any edit is written |
| `reply_outcome` | After each reply: ends the turn as waiting-for-user when the agent asked and stopped (probability ≥ 0.9), and labels replies that only restate finished work so the moderator does not dispatch for them | Only the explicit waiting marker ends a turn |

Every enabled scenario sends conversation excerpts to the endpoint: the objective, the last few messages, a command line, a note, or a reply. That is a new place data leaves the machine, which is why nothing is on until you turn it on for a group. Failed or slow calls (12 s timeout) mean "no opinion" and the runtime behaves as it did before. Tokens spent are recorded under the agent name "Decision model" in token usage and count toward the turn's token budget.

**Test connection** sends one sample question and reports the answering model and the dialect in use; a chat endpoint entered by mistake fails this test.

Jev reads instructions literally, does no arithmetic, and is trained mainly on English. Chinese conversations work but with lower accuracy: start with one scenario, watch the dispatch trace, and raise `decision_min_confidence` if it acts when it should not.

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

Login has no time limit and persists across app restarts until you sign out.
This also applies to previously issued tokens with an expiry date. Signing out
clears the saved login and private client state; it does not revoke server-side
copies of the token. `ACCESS_TOKEN_EXPIRE_MINUTES` no longer controls login.

`desktop-secret.key` signs login tokens. Deleting it invalidates existing sessions; logging in again is enough to recover.

## Desktop behavior

- Closing the window hides the app to the tray instead of quitting. Quit from the tray menu.
- On startup the launcher clears any stale process holding TCP `127.0.0.1:8765`, so an old backend cannot block the new one.
