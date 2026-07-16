# Runtime Model Capability Discovery Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use subagent-driven-development when workers are available, otherwise execute the tasks inline. Do not use Trellis. Implement each task before adding its regression tests; this plan intentionally does not use TDD.

**Goal:** Populate agent model and thinking settings from live ACP/provider capabilities while preserving custom values and surfacing real runtime configuration failures.

**Architecture:** The Rust backend owns all external discovery. ACP discovery launches a short-lived protocol session and normalizes advertised config options; provider discovery calls the saved provider's model endpoint. React Query caches both catalogs, and editable suggestion controls preserve manually entered values.

**Tech Stack:** Rust, Axum, Tokio, reqwest, ACP JSON-RPC, React 19, TypeScript, TanStack Query, Vitest.

## Global Constraints

- Dynamic choices are suggestions; custom values remain editable and savable.
- ACP probing sends no prompt and always terminates its child process.
- Provider credentials and ACP environment values never leave the backend.
- ACP and provider discovery use bounded timeouts and safe error messages.
- Automatic ACP probing occurs on form open and preset/model change; command/args/env edits require manual refresh.
- Capability results are not persisted and remain fresh in the frontend for five minutes.
- Existing untracked `.superpowers/` content must remain untouched.
- Tests are written after implementation, per the user's explicit no-TDD requirement.

---

### Task 1: ACP Capability Probe

**Files:**
- Create: `backend-rs/crates/backend/src/acp/capabilities.rs`
- Modify: `backend-rs/crates/backend/src/acp/mod.rs`
- Modify: `backend-rs/crates/backend/src/acp/protocol.rs`
- Test: `backend-rs/crates/backend/tests/acp_lifecycle.rs`

**Interfaces:**
- Produces `probe_acp_runtime_capabilities(config: AcpRuntimeConfig, cwd: PathBuf, selected_model: Option<String>) -> Result<AcpRuntimeCapabilities, AcpCapabilityError>`.
- Produces serializable `AcpRuntimeCapabilities` and `AcpCapabilityChoice` values for the API task.
- Extends `AcpConnection::spawn` with an optional raw `session/update` sink used only by probes.

- [ ] **Step 1: Add raw ACP update observation without changing live-run event mapping**

Extend `AcpConnection::spawn` and its reader loop with:

```rust
raw_updates_tx: Option<mpsc::UnboundedSender<Value>>
```

When a `session/update` notification arrives, forward `params.update` to this channel before running the existing `event_from_update` mapping. Existing live callers pass `None`; the probe passes `Some(tx)`.

- [ ] **Step 2: Implement normalized capability parsing**

Create focused types and pure helpers in `capabilities.rs`:

```rust
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct AcpCapabilityChoice {
    pub value: String,
    pub label: String,
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct AcpRuntimeCapabilities {
    pub models: Vec<AcpCapabilityChoice>,
    pub modes: Vec<AcpCapabilityChoice>,
    pub thinking_efforts: Vec<AcpCapabilityChoice>,
    pub current_model: Option<String>,
    pub current_mode: Option<String>,
    pub current_thinking_effort: Option<String>,
    pub source: &'static str,
    pub warning: Option<String>,
}
```

Parse ACP select options by `category` first, then aliases: `model`; `mode`/`approval_preset`; `reasoning_effort`/`effort`/`effortLevel`. Merge `session/new`, setter responses, and raw config-option updates, keeping the newest value for each category.

- [ ] **Step 3: Implement the bounded probe lifecycle**

Spawn with `spawn_acp_child`, send `initialize` and `session/new`, optionally apply the selected model through `session/set_model` with the existing `model` config-option fallback, wait briefly for queued updates, then close and terminate the child using the existing grace-period helpers. Wrap the whole operation in a 15-second timeout and return sanitized `AcpCapabilityError` variants.

- [ ] **Step 4: Add post-implementation regression tests**

Extend the existing fake ACP test process to cover:

```rust
assert_eq!(capabilities.models[0].value, "gpt-5.5");
assert_eq!(capabilities.thinking_efforts[0].value, "xhigh");
assert!(!fake_log.contains("session/prompt"));
assert!(fake_process_exited());
```

Also cover model-dependent config updates, `session/set_model` fallback, timeout cleanup, and redaction of an environment value such as `TOP_SECRET_VALUE`.

- [ ] **Step 5: Verify and commit**

Run:

```powershell
cargo test -p ag-swarmer-backend --test acp_lifecycle -- --nocapture
cargo check -p ag-swarmer-backend
```

Commit:

```powershell
git add backend-rs/crates/backend/src/acp backend-rs/crates/backend/tests/acp_lifecycle.rs
git commit -m "feat(acp): discover runtime capabilities"
```

---

### Task 2: ACP Capability API

**Files:**
- Modify: `backend-rs/crates/backend/src/api/agents.rs`
- Modify: `backend-rs/crates/backend/src/api/mod.rs`
- Modify: `backend-rs/crates/backend/tests/agents.rs`
- Modify: `frontend/src/types/api.ts`
- Create: `frontend/src/hooks/useAcpRuntimeCapabilities.ts`

**Interfaces:**
- Consumes `probe_acp_runtime_capabilities` from Task 1.
- Produces `POST /api/v2/agents/acp-runtime-capabilities`.
- Produces `useAcpRuntimeCapabilities(input, enabled)` and `AcpRuntimeCapabilitiesRead`.

- [ ] **Step 1: Add the authenticated API handler**

Deserialize a runtime-shaped JSON object plus optional selected model, authenticate the caller, normalize it with `normalize_acp_runtime`, force the probe permission policy to `deny`, and use a valid backend-owned probe directory derived from `skill_storage_root.parent()`.

Return `400` for invalid runtime configuration, `422` for adapter protocol rejection, and `504` for timeout. Map every response through the normalized capability type; never return stderr or environment values.

- [ ] **Step 2: Add frontend types and query hook**

Add:

```ts
export interface AcpRuntimeCapabilitiesRead {
  models: AcpRuntimeChoice[]
  modes: AcpRuntimeChoice[]
  thinking_efforts: AcpRuntimeChoice[]
  current_model: string | null
  current_mode: string | null
  current_thinking_effort: string | null
  source: 'acp'
  warning: string | null
}
```

The hook sends normalized `profile`, `command`, `args`, `env`, `permission_policy`, and selected `model`; use `staleTime: 5 * 60 * 1000` and expose React Query's `refetch` for the refresh button.

- [ ] **Step 3: Add post-implementation API tests**

Use the existing authenticated test router and fake ACP executable. Assert successful shape, invalid command handling, authentication, timeout status, and absence of a secret environment value from the response body.

- [ ] **Step 4: Verify and commit**

Run:

```powershell
cargo test -p ag-swarmer-backend --test agents -- --nocapture
pnpm type-check
```

Commit:

```powershell
git add backend-rs/crates/backend/src/api frontend/src/types/api.ts frontend/src/hooks/useAcpRuntimeCapabilities.ts backend-rs/crates/backend/tests/agents.rs
git commit -m "feat(settings): expose ACP capabilities"
```

---

### Task 3: Provider Model Catalogs

**Files:**
- Create: `backend-rs/crates/backend/src/llm/model_catalog.rs`
- Modify: `backend-rs/crates/backend/src/llm/mod.rs`
- Modify: `backend-rs/crates/backend/src/api/llm_providers.rs`
- Modify: `backend-rs/crates/backend/tests/providers_settings.rs`

**Interfaces:**
- Produces `discover_models(client: &reqwest::Client, config: &ProviderConfig) -> Result<Vec<ModelInfo>, ModelCatalogError>`.
- Keeps the existing `GET /api/v2/llm-providers/:provider_id/models` response contract `{ id, name }[]`.

- [ ] **Step 1: Implement provider-specific URL and authentication builders**

Build requests as follows:

```text
openai-compatible: GET {base}/models, Authorization: Bearer ...
anthropic*:        GET {base}/v1/models, x-api-key + anthropic-version
gemini:            GET {base}/models?key=..., normalize an existing /v1beta suffix
```

Use a 15-second client timeout and reject bodies over 2 MiB.

- [ ] **Step 2: Implement provider response parsing**

Support OpenAI/Anthropic `data` arrays and Gemini `models` arrays. For Gemini, strip the `models/` prefix and retain models whose supported generation methods contain `generateContent` when that metadata exists. Deduplicate by ID, sort case-insensitively, and insert the saved default model when absent.

- [ ] **Step 3: Replace the current default-model-only endpoint implementation**

Load the owned provider, build a `ProviderConfig`, call `discover_models`, and map upstream errors to safe API messages including only the provider kind and HTTP status.

- [ ] **Step 4: Add post-implementation provider tests**

Use a local mock HTTP server to assert all four provider kinds, exact auth headers, URL normalization, malformed response handling, upstream 401 mapping, deterministic sorting, deduplication, Gemini filtering, and default-model preservation.

- [ ] **Step 5: Verify and commit**

Run:

```powershell
cargo test -p ag-swarmer-backend --test providers_settings -- --nocapture
cargo check -p ag-swarmer-backend
```

Commit:

```powershell
git add backend-rs/crates/backend/src/llm backend-rs/crates/backend/src/api/llm_providers.rs backend-rs/crates/backend/tests/providers_settings.rs
git commit -m "feat(providers): discover remote models"
```

---

### Task 4: Editable Dynamic Model Settings

**Files:**
- Create: `frontend/src/components/agents/RuntimeCapabilityField.tsx`
- Create: `frontend/src/components/agents/RuntimeCapabilityField.test.tsx`
- Modify: `frontend/src/components/agents/ExternalRuntimeFields.tsx`
- Modify: `frontend/src/components/agents/CreateAgentForm.tsx`
- Modify: `frontend/src/components/agents/EditAgentForm.tsx`
- Modify: `frontend/src/hooks/useProviders.ts`
- Create: `frontend/src/components/agents/AgentRuntimeCapabilities.test.tsx`

**Interfaces:**
- Consumes ACP capability and provider-model hooks from Tasks 2 and 3.
- Produces an editable datalist/combobox field with optional refresh action, loading state, stale state, and warning.
- Persists non-ACP model overrides under `llm_config.model`.

- [ ] **Step 1: Build the reusable editable capability field**

Use a text input backed by a stable native `datalist`, preserving arbitrary text. Add a `RefreshCw` icon button with `aria-label="Refresh available values"`, a loading spinner state, and compact warning text. The component must not replace a saved value merely because discovery returned different choices.

- [ ] **Step 2: Wire ACP automatic and manual discovery**

In both forms, build a committed probe input on initial ACP render and preset selection. Pass dynamic `models`, `modes`, and `thinking_efforts` into `ExternalRuntimeFields`. Command/args/env edits set `capabilitiesStale=true`; pressing refresh commits the current form values and refetches. Model selection commits a new probe input automatically to retrieve model-specific thinking options.

- [ ] **Step 3: Add non-ACP model override controls**

Add `model: z.string().optional()` to each form schema, initialize it from `agent.llm_config?.model`, and write non-empty values to `llm_config.model`. When a provider is selected, call `useProviderModels(providerId)` and display an editable `Model` field with provider results and placeholder `Provider default`.

- [ ] **Step 4: Add post-implementation component tests**

Mock capability hooks and assert automatic probe input, preset-triggered refresh, model-dependent refresh, manual refresh after command edits, loading/warning UI, fallback choices, custom ACP values, provider model suggestions, and persisted custom LLM model submission.

- [ ] **Step 5: Verify and commit**

Run:

```powershell
pnpm --filter @ag-swarmer/frontend test -- src/components/agents/RuntimeCapabilityField.test.tsx src/components/agents/AgentRuntimeCapabilities.test.tsx
pnpm type-check
pnpm lint
```

Commit:

```powershell
git add frontend/src/components/agents frontend/src/hooks/useProviders.ts
git commit -m "feat(settings): load runtime model choices"
```

---

### Task 5: ACP Failure Classification And Final Verification

**Files:**
- Modify: `backend-rs/crates/backend/src/acp/mod.rs`
- Modify: `backend-rs/crates/backend/src/runtime/group.rs`
- Modify: `backend-rs/crates/backend/tests/group_stream.rs`
- Modify: `frontend/src/hooks/useSendMessageStream.test.tsx`

**Interfaces:**
- Produces a failed `AcpRun::join` result when protocol execution emitted a failed terminal run.
- Produces an `agent_error` stream event with a sanitized configuration failure message.

- [ ] **Step 1: Preserve ACP terminal protocol failures**

When the ACP driver emits a failed run event, retain a sanitized failure result instead of completing the task successfully with empty content. Keep cancellation and timeout behavior distinct.

- [ ] **Step 2: Map real ACP failures to agent errors**

In `run_acp_agent_turn`, emit `StreamEventKind::AgentError` with agent identity and the sanitized message before returning a failed result. Ensure the scheduler records the dispatch as failed and legacy fan-out does not append `agent_silent` for the same agent.

- [ ] **Step 3: Add post-implementation regression tests**

Simulate `-32602 Invalid params` before `session/prompt` and assert the stream contains `acp_agent_run: failed` and `agent_error`, excludes `agent_silent` and turn-level `silence`, and leaves no queued/running dispatch.

- [ ] **Step 4: Run full verification**

Run:

```powershell
cargo fmt --all -- --check
cargo check -p ag-swarmer-backend
cargo test -p ag-swarmer-backend --test acp_lifecycle -- --nocapture
cargo test -p ag-swarmer-backend --test agents -- --nocapture
cargo test -p ag-swarmer-backend --test providers_settings -- --nocapture
cargo test -p ag-swarmer-backend --test group_stream -- --nocapture
pnpm --filter @ag-swarmer/frontend test
pnpm type-check
pnpm lint
pnpm build
```

Run clippy and record unrelated baseline warnings separately:

```powershell
cargo clippy -p ag-swarmer-backend --all-targets -- -D warnings
```

- [ ] **Step 5: Commit and build a local desktop artifact**

```powershell
git add backend-rs/crates/backend/src/acp/mod.rs backend-rs/crates/backend/src/runtime/group.rs backend-rs/crates/backend/tests/group_stream.rs frontend/src/hooks/useSendMessageStream.test.tsx
git commit -m "fix(acp): surface runtime configuration errors"
pnpm desktop:build
```

If desktop signing fails only because `TAURI_SIGNING_PRIVATE_KEY` is absent, run `pnpm desktop:portable` and report both generated artifact paths.
