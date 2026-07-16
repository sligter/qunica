# Runtime Model Capability Discovery

## Goal

Replace stale, hard-coded model and thinking-effort choices with capabilities reported by the configured runtime.

- ACP agents discover models, modes, and thinking effort from the selected ACP process.
- LLM chat agents discover models from the selected provider's model-list endpoint.
- Dynamic choices remain suggestions. Users can always retain or enter a custom value.
- Discovery failures never block saving an agent configuration.

## Scope

This change covers the model-related fields in agent create and edit forms. It also improves runtime error classification so an ACP configuration failure is shown as an agent error instead of a silent turn.

Provider creation still accepts a manually entered default model because an unsaved provider does not yet have a server-side credential record. Dynamic provider discovery begins once a saved provider is selected by an agent or edited through its existing provider record.

## Architecture

Capability discovery is owned by the backend. The frontend must not receive provider API keys or spawn ACP processes directly.

### ACP Capability Probe

Add an authenticated endpoint:

```text
POST /api/v2/agents/acp-runtime-capabilities
```

The request contains the same normalized runtime fields used by an ACP agent plus an optional model selection:

```json
{
  "profile": "codex",
  "command": "npx",
  "args": ["@zed-industries/codex-acp"],
  "env": {},
  "permission_policy": "deny",
  "model": "gpt-5.5"
}
```

The response is normalized independently of ACP adapter-specific option names:

```json
{
  "models": [{ "value": "gpt-5.5", "label": "GPT-5.5" }],
  "modes": [{ "value": "auto", "label": "Default" }],
  "thinking_efforts": [{ "value": "xhigh", "label": "XHigh" }],
  "current_model": "gpt-5.5",
  "current_mode": "auto",
  "current_thinking_effort": "xhigh",
  "source": "acp",
  "warning": null
}
```

The backend starts the configured ACP command and performs only:

1. `initialize`
2. `session/new`
3. Optional model selection when the request includes a model
4. Collection of the latest `configOptions`, `modes`, and related session updates
5. Session/process shutdown

It never sends `session/prompt`. The whole probe uses a short bounded timeout. Every success, protocol error, and timeout path closes stdin, terminates the child process if needed, and drains its tasks.

ACP option IDs are normalized by category first and known aliases second:

- Model: category `model`, then IDs such as `model`
- Mode: `modes` response or category `mode`, then IDs such as `mode` and `approval_preset`
- Thinking: category `thought_level`, then IDs such as `reasoning_effort`, `effort`, and `effortLevel`

Some adapters expose thinking effort only after a model is selected. When a model is supplied, the probe applies it using `session/set_model` with the existing config-option fallback, then consumes the updated capability response before returning.

Raw environment values, API keys, and unrestricted process output are never included in API errors.

### Provider Model Discovery

Upgrade the existing endpoint:

```text
GET /api/v2/llm-providers/:provider_id/models
```

After ownership validation, the backend uses the stored provider credentials to call the provider's model endpoint:

- `openai-compatible`: `GET {base_url}/models` with bearer authentication
- `anthropic`: `GET {base_url}/v1/models` with `x-api-key` and `anthropic-version`
- `anthropic-compatible`: the same Anthropic-compatible route and headers
- `gemini`: `GET {base_url}/models?key=...`, avoiding a duplicate `/v1beta` segment

Responses are mapped to `{ id, name }`, deduplicated by ID, sorted deterministically, and filtered to generation-capable models when the provider supplies capability metadata. The saved default model is inserted when it is absent from the remote response.

The HTTP client uses a bounded timeout and response-size limit. Authentication failures, unsupported routes, and malformed payloads become safe API errors without returning credentials or full upstream bodies.

## Frontend Data Flow

### ACP Agents

The agent create and edit forms use a capability query keyed by:

```text
profile + command + args + env + selected model
```

The query runs when the settings view opens and when a runtime preset changes. Editing command, arguments, or environment marks capabilities as stale but does not launch a process on every keystroke. The user can then press the refresh icon to probe the edited configuration.

Selecting a model triggers a capability refresh so model-dependent thinking options update.

### LLM Chat Agents

Selecting a saved provider runs the existing provider-model query. Provider updates invalidate its model query immediately. Results are cached for five minutes.

### Choice Controls

Model, mode, and thinking fields become editable comboboxes rather than strict selects:

- Suggestions come from dynamic discovery.
- The saved value remains visible even when discovery does not return it.
- Users can enter and save an arbitrary value.
- Loading and refresh use an icon button with an accessible label.
- A compact warning appears when discovery fails; it does not disable saving.

Static ACP preset choices remain only as fallback data when the adapter cannot be started or does not advertise a category.

## Error Classification

Capability discovery failures are local to the settings query.

During a real ACP run, failure before any visible response must emit an `agent_error` carrying a sanitized actionable message. A failed external run must not be reduced to `agent_silent` or the turn-level `silence` warning. The final turn status follows the scheduler's failure handling rather than displaying `No one replied`.

Examples:

- `ACP model "gpt-5.6-sol" is not supported by the installed adapter.`
- `ACP option "reasoning_effort=xhigh" was rejected by the selected model.`
- `Provider model discovery was rejected by the upstream service (401).`

## Caching And Concurrency

- Frontend ACP capability queries are fresh for five minutes.
- Manual refresh invalidates and refetches regardless of freshness.
- Identical in-flight probes are deduplicated by the query key.
- The backend does not keep probe sessions in the reusable live-session pool.
- Provider model queries use the existing five-minute frontend cache.
- No capability result is persisted to the database; saved agent values remain the source of truth.

## Testing

Tests are added after implementation and cover:

- ACP response normalization from initial and model-dependent config updates
- Model selection through `session/set_model` and config-option fallback
- Probe timeout and child-process cleanup
- Safe ACP warnings without environment leakage
- OpenAI-compatible, Anthropic, Anthropic-compatible, and Gemini model-list parsing
- Authentication headers and URL normalization
- Default/custom model preservation and deterministic deduplication
- Editable combobox behavior, stale state, automatic probe, and manual refresh
- Discovery failure fallback without blocking form submission
- Real ACP execution failure producing `agent_error` instead of `silence`

## Non-Goals

- Sending a prompt as part of capability discovery
- Persisting remote model catalogs
- Inferring universal reasoning levels for non-ACP providers whose model endpoints do not report them
- Validating that a manually entered custom model will accept a prompt before saving
- Automatically installing or upgrading ACP adapters
