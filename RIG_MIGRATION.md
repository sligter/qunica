# Rig migration: providers and host-driven execution

The migration keeps Qunica's scheduler, tool executor, approval boundary and
SQLite history authoritative. Native execution remains the default. P0's
execution and recovery contract is documented in
[Realtime synchronization](backend-rs/crates/backend/src/docs/guide/realtime.md#native-tool-execution-checkpoints).

## P1 dependency boundary

`rust-toolchain.toml` pins Rust 1.97.1, matching the
[Rig v0.45.0 minimum](https://github.com/0xPlaygrounds/rig/blob/v0.45.0/Cargo.toml).
CI, release jobs and both Rust Docker build stages use the same version.
The application crates retain edition 2021; Rig uses its own edition 2024.

The non-default Cargo feature `rig-engine` enables exactly pinned `rig-core`,
`rig-agent` and `rig-reqwest` 0.45.0 dependencies. It is forwarded by the desktop
crate. Enabling the feature alone keeps native execution. P3 adds an explicit
per-agent selector described below; builds without the feature reject it.

The existing `reqwest` 0.12 clients stay unchanged. The `reqwest_013` alias is
reserved for the explicitly injected Rig transport. Both use native-tls so P1
does not change the system certificate backend or introduce a second TLS stack
for the new transport. Default Rig derive, bundled transport, coding tools,
harness and ECS features are disabled. `rig-memory` is an unavoidable transitive
dependency of `rig-agent`; its presence does not delegate context management to
Rig. Qunica's history and compaction policy remain unchanged.

Rig enables `serde_json/preserve_order` and `float_roundtrip` through Cargo
feature unification. Qunica's durable command, Webhook and approval digests
explicitly sort a copy of their JSON objects, retaining legacy sorted-key
identity bytes without sorting provider history. Native and Rig-enabled tests
both cover this boundary; opaque provider data must never use this digest
serializer as its history encoding.

### Float parsing in identity digests

Digests hash *parsed* numbers, and `float_roundtrip` changes how a decimal
lexeme is parsed. The legacy parser can produce a neighbouring double for
full-precision decimal lexemes; integers and short decimals such as `1.25`
parse identically. For example, `8.971135692166839e-150` changes between the two
parsers. A feature that only Rig enabled would therefore make the
same request hash differently in Rig and non-Rig builds.

The backend therefore enables `float_roundtrip` itself, in every build. Rig no
longer changes any digest, and the exact parser round-trips Qunica's own JSON.
`canonical_json` tests fail if the pin is dropped.

Rows written by v0.1.7 and earlier still hold legacy-parsed digests. They are
not rehashed: receipts and occurrences do not retain enough original input to
recompute historical hashes using a new parser. No legacy parser is maintained
in the authority path. The upgrade boundary is handled per table:

| Table | Old row vs. new parse | Outcome |
|---|---|---|
| `pending_approvals` | digest or stored-action mismatch | Fail closed, without resolving or executing the call. Hash and canonical persisted action bytes must both match exactly, including floats and signed zero. Old rounding drift cannot be distinguished from a changed argument, so ULP tolerances never grant approval. |
| `command_receipts` | retry of a float-bearing command | `409 idempotency_conflict`, fail-closed. Current typed command fields do not require floats, but arbitrary JSON fields must not be assumed float-free. |
| `automation_occurrences`, same event key | retried float-bearing delivery | Recognised as the original event through the legacy candidates below; otherwise `409`, fail-closed. The original run is not repeated. |
| GitHub payload dedup (new delivery id) | replay of a float-bearing payload | Recognised through the legacy candidates below. With more than three floats the candidates are not enumerated; while the automation has version-1 rows the delivery is refused with `409 legacy_replay_unverifiable` rather than run. |

Migration 0049 marks pre-existing occurrences `digest_version=1`; new rows store
2 and match exactly. For webhook identities only, `canonical_json::legacy_variants`
re-encodes the payload with every double within 3 ULPs of each float (≤3 floats,
≤343 encodings); legacy drift measured at 2 ULPs worst over ~10M probes,
including long mantissas, subnormals and extreme exponents. A version-1 row
matching any candidate counts as the same event. This is deliberately one-sided:
for dedup a false match only suppresses a run, while a miss repeats one. It is
not a parser and is never used for approvals, where a near match must not grant.

This closes the float-digest cutover gate. Rig runtime routing stays disabled
until the Driver phase is accepted.
No approval, receipt or occurrence is silently rehashed or replayed.
There is no in-place reapproval endpoint for a mismatched old action: terminate
the old task and submit a new task for fresh review; do not edit stored hashes
to force a resume.
Revisit if command fields change, if duplicate webhook runs or
`legacy_replay_unverifiable` refusals appear after an upgrade, or once no
version-1 occurrence can still be replayed (then the candidates can be removed). Opaque provider data must never use the digest serializer as history
encoding.

## Driver stack boundary

The larger JSON-backed state also exposes Windows stack limits in nested native
delegation, before any Rig Driver is active. Large sub-futures for ACP, invocation
setup, tool batches, helper dispatch and completion/interruption persistence stay
heap-allocated. Even an unused ACP branch contributes to the native Future's
size. This changes storage, not dispatch, cancellation or approval policy. The
existing nested success/silent/failure/cancellation/budget tests are required
under both feature sets without increasing `RUST_MIN_STACK`.

`tests/rig_dependencies.rs` exercises a sans-I/O `AgentRun` step and constructs a
Provider with Qunica's reqwest 0.13 client. It makes no network requests and is
not evidence of message compatibility or durable execution. Those remain P2/P3
acceptance gates.

## P2 message adapter and provider

`llm::rig_engine` (feature `rig-engine` only) holds `RigMessageAdapter` and
`RigProvider`. The native `build_provider` factory is unchanged; P3's explicit
engine selector constructs the Rig provider separately.
It serves OpenAI-compatible, Anthropic (including its compatible aliases), and
Gemini/Google kinds, plus OpenAI Responses. All kinds default to native.

- **History.** A Rig assistant turn is stored whole as `native_content["rig"]`,
  beside its display fields. It is replayed only while its projection still
  equals those fields; an edited message is rebuilt from the fields, without
  native items. Non-object tool arguments travel as JSON text, so `null` stays
  `null`. Anything Rig cannot express (an image in a system message, an image
  MIME type Rig does not name) is an error, never a silent drop.
- **Request.** A single neutral gateway dialect is used for every compatible
  host, not Rig's vendor dialects, which rewrite bodies per vendor. `top_p` and
  `reasoning_effort` go through `additional_params`: Rig refuses `top_p` on a
  gateway and would round effort levels it does not list. Passback stays per
  model. Blank `reasoning_content`, which Rig writes on every assistant message
  once a dialect names the field, is stripped by transport middleware.
- **Transport and errors.** Qunica builds the reqwest 0.13 client. Configured
  headers are reqwest defaults, so the credential header always wins and there
  is one `Authorization`, as on the native path. Rig sends nothing until
  polled, so `stream` reads the first item. A non-2xx response is therefore an
  `Err(ProviderHttpError)` with its body (capped as before), and context
  overflow, passback and transient classification keep working. A mid-stream
  failure becomes `Truncated`, with no body. Tool calls are released only on a
  `stop`/`tool_calls` finish.

Differences from the native wire, asserted per engine in `tests/llm_contract.rs`:
- the tool message has no `name` field (it is not in OpenAI's schema);
- image parts carry `detail: "auto"`;
- tool history in a request that declares no tools is sent as text;
- a reply that is a bare `[DONE]` with no finish is an error;
- SSE needs blank-line framing (the contract fixtures now use it);
- system images are refused.

Revisit these if a gateway needs the tool-message `name` (some
OpenAI-compatible fronts for other vendors have), sends single-newline SSE or
bare `[DONE]`, or rejects `detail`. Also revisit if a Rig upgrade changes
`AssistantMessage` serde, because stored `native_content["rig"]` depends on it.

### Anthropic and Gemini

Both providers use the same injected transport, HTTP error classification and
completion gate as OpenAI. Streamed tool blocks remain buffered until Rig
accepts a terminal completion. Transport truncation, missing terminal events
and malformed tool arguments cannot release executable calls.
Malformed argument completions still report billed usage, but emit
`Truncated` without committing native history. A request-scoped transport
middleware preserves explicitly requested `tools: []`, which Rig otherwise
consumes and omits; it adds no tool definitions. History with tools but no
declared tools follows Rig's text fallback on all three protocols.

Anthropic keeps the native path's API version, thinking token budgets and
output reserve; thinking suppresses both sampling settings. The adapter
projects the last signature for the existing event contract while retaining
every signed thinking/redacted block in order in the complete Rig turn.
P0's native `native_content["anthropic"]` blocks are also imported in order.
They must agree with the stored display fields, signature and calls; a
disagreement fails before I/O. Native checkpoints contain no original model
identity, so this import assumes the configured Anthropic model, as the native
path does. It must not be used to migrate history to another model.
Existing Rig origins are never relabelled.
An encoded-request guard verifies that required signed/redacted blocks survive
Rig's history adaptation in their original order, with their text and opaque
metadata unchanged. A model switch or context mismatch fails before I/O.
Legacy checkpoints for context-bound Claude models lack the required context
fingerprint and are therefore refused; a fresh Rig turn retains that context
and can replay under an unchanged model/tools/system prompt. The guard does
not invent an original context for old history.

Gemini keeps the native thinking budgets and sampling fields in
`generationConfig`. Tool `thoughtSignature` is projected to
`provider_metadata`; full parts, including opaque metadata, stay in the stored
Rig turn. Legacy call signatures import under Rig's `gcp.gemini` origin.
Rig silently removes non-Base64 signatures when encoding. The adapter rejects
those inputs before I/O, rather than sending an unsigned call.

Additional explicit wire differences covered by local contract tests:

- Anthropic system and text messages use content-block arrays. Tool results
  carry text-block arrays. Adjacent tool results remain one user message.
- Anthropic emits one final usage snapshot through this adapter; native also
  emits the initial input snapshot. Final input includes cache reads/writes.
- Gemini may add `thought: false` to ordinary parts; reasoning marked
  `thought: true` is emitted as reasoning rather than visible answer text.
- Anthropic requires a terminal `message_delta.delta.stop_reason`; Gemini
  requires a candidate `finishReason`. Plain HTTP EOF is insufficient.
- Rig system messages cannot carry images; both adapters refuse system images
  before I/O. User images remain supported.

The signing tests use local fixtures: a completed turn is JSON-serialized,
loaded, and sent in a second request with tool results. They verify retained
bytes, block order and metadata, not cryptographic acceptance by a live vendor.
Real Anthropic/Gemini validation remains a P2 gate.
Revisit if model changes or context-binding rules prevent signed replay, a
gateway requires nonstandard signatures/framing, or any opaque part is lost.

Local P2 verification (Windows, Rust 1.97.1): the Rig build passes 434
library tests and 59 contract tests; the default build passes 428 library
tests and 50 contract tests. Each library suite has two existing ignored
tests. Formatting, workspace/all-target/all-feature Clippy with warnings
denied, and whitespace checks pass. This is targeted validation, not a fresh
full-workspace or cross-platform CI run.
An earlier parallel contract run hit a native loopback connection abort
(Windows 10053); unchanged native code passed in the final parallel run.
No production retry policy or test assertion was relaxed.

### OpenAI Responses

Responses uses a separate wire with the same injected HTTP client and neutral
events. The native endpoint resolver is shared: roots, complete `/responses`
URLs, tenant queries and fragments keep their existing behavior. The complete
resolved URL is Rig's base with an empty route suffix, avoiding double paths
and misplaced queries.

Requests keep `store: false`, request `reasoning.encrypted_content`, send system
messages in `input`, and retain non-strict function schemas. All effort levels
and sampling values use the existing Qunica mappings. Explicit empty tools are
preserved. Rig's user text/image parts are separate message items and images
carry `detail: "auto"`; assistant text uses a typed output-message item rather
than the native client's string shorthand. Those shapes are asserted separately.

Complete Rig turns persist encrypted reasoning, opaque fields, item ids and
original call ids, including turns without tools. Only completed ciphertext
is replayed, following the [official OpenAI streaming guidance](https://developers.openai.com/api/reference/resources/responses/streaming-events#response.output_item.added).
Native history's `openai_responses_reasoning` metadata imports the same complete
reasoning items before the associated call batch. Their following neutral
text/call items keep native representations so Rig retains the association.
An existing Rig origin is never relabelled; losing required ciphertext during
adaptation fails before I/O.

Executable calls come from the finalized response in provider output order.
Before releasing any, the adapter compares every function call against Rig's
raw response: ids, names and object arguments must match, ids must be unique,
and no call may be unfinished. A missing id/name or a repaired/omitted call
fails the entire batch. An incomplete response, malformed arguments or a
failed stream emits no tools and commits no native turn. Billed usage is
retained for incomplete completions and failed terminal envelopes.

The local contracts cover out-of-order done items, partial-vs-complete
ciphertext, JSON persistence followed by a second request, no-tool reasoning,
missing identity, malformed calls, HTTP errors, EOF/transport failure,
sampling, query routing and credential precedence. Live provider acceptance
remains unverified. P3 exercises durable Driver behavior with local fixtures.

## P3 host-driven AgentRun

An agent may opt in through its existing `llm_config` API field:

```json
{
  "engine": "rig",
  "rig_limits": {
    "max_model_calls": 128,
    "max_failed_attempts": 16,
    "max_total_tokens": 120000
  }
}
```

Missing `engine` means `native`. Unknown engines and invalid limits fail at API
validation and again when loading a stored configuration. `rig` requires the
`rig-engine` Cargo feature. The default model-call and failed-attempt limits are
128 and 16; the optional token cap has no default. These limits cover one agent
execution, including request retries and approval resumes. They supplement the
existing group scheduler budget, which also accounts for summarization and
delegated agents. They do not reset when an AgentRun is rebuilt.

Before accepting a Rig resume command, the agent must have `engine: "rig"`.
An engine mismatch is rejected inside the approval transaction, without consuming
the pending decision; restoring Rig allows the original approval to resume.
After acceptance, the continuation stays bound to its recorded engine, so later
engine-setting edits apply to new executions. Native also refuses to replay
Rig-native history without a verified lossless conversion. A native rollback
therefore starts a fresh thread. Other agents still read public Rig replies as
ordinary untrusted conversation text.

`runtime/rig_driver.rs` owns a sans-I/O `AgentRun`. Qunica still drives streaming,
hooks, tool execution, approval, SQLite persistence, cancellation and scheduling.
The driver requires `CallModel` before sending a request, accepts a complete
model turn through `streamed_turn`, then requires matching `CallTools` or `Done`.
It never executes tools through Rig. Tool names and arguments must agree with
the accepted batch; shell compatibility aliases follow ToolExecutor and are
allowed only when that execution already has a shell tool.

Each prepared request gets a durable UUID attempt id and consumes the call
budget before HTTP I/O. Its terminal metadata travels on a private channel,
separate from frontend deltas. The lifecycle is Prepared → Streaming →
TerminalReceived → CompletionRecorded → StreamedTurnAccepted → ToolsPending /
Done. Failed or cancelled attempts become Aborted. Completion metadata is
recorded at most once; an incomplete or rejected terminal response can still
incur usage without becoming an accepted turn. Repeated usage snapshots are
accounted by their positive difference. Accepted calls and their original
assistant message are persisted together before execution.
Only successful stop/tool-call finishes can be accepted; token-limit and other
incomplete finishes abort even if the transport has ended cleanly. Original
tool arguments must be objects, and required provider ids/names cannot be repaired
before dispatch.

The driver sets `max_invalid_tool_call_retries(0)`. There is no model retry after
streamed output, including invalid-call and validation failures. Existing bounded
HTTP and reasoning-passback retries happen before any visible output and consume
separate attempt ids. Context-overflow hooks may compact only after the previous
tool batch has settled. The host validates the complete call/result history,
then splits the last prompt to construct a new run. `with_validated_history`
alone cannot validate an exchange whose final result is that prompt. Neither
pairing repair nor `canonical_history()` is used to decide whether tools ran.

Rig executions use Qunica `content_json` schema 3, adding `driver_checkpoint`
with cumulative attempts, failures, completions and token counts. Native still
writes schema 2. No serialized Rig state is stored. On approval/restart the host
first settles the original persisted batch, without asking a model to recreate
calls, then starts AgentRun at the next model boundary. A previously started
tool without a durable result remains outcome-unknown and stops recovery.
Final assistant messages without tools also retain their native blocks.
Model-history loading projects Rig rows from accepted assistant messages only,
including during explicit Resume. Failed display fragments remain in the UI and
audit row but never become a successful assistant message in a later prompt.
Gemini's calls may have Rig-local identifiers. Result conversion restores the
original identity variant as well as its UUID string, so pairing remains valid
after SQLite/JSON persistence. Provider-native blocks remain unchanged.

Migration 0050 makes context-summary invalidation respect each agent's cursor.
Editing or deleting a covered row invalidates that summary; execution-checkpoint
updates beyond the cursor leave it usable and are loaded as incremental history.
This also invalidates a covered row when its context-admission flag changes.

Rig HTTP opening is polled in a separate awaited task: reqwest 0.13's Windows
debug connector otherwise exhausts the small default stack beneath nested
AgentAsTool frames. Dropping the opener aborts that task. This does not detach
tool execution or change the scheduler's sequential dispatch contract.

The P3 acceptance tests exercise actual local file/shell operations against fake
providers. Required scenarios are:

| Scenario | Required invariant |
| --- | --- |
| Read → Edit → Shell | One accepted batch and one completion per attempt |
| Approval + restart | Original calls, reasoning and signatures survive |
| Cancellation | No duplicate dispatch or late tool execution |
| Text/reasoning/usage/tool events | Existing SSE contracts and billing remain |
| AgentAsTool | Private output, order and scheduler budget remain |
| Overflow + compaction | Settled history, prior output and cumulative budget survive |
| Anthropic / DeepSeek replay | Signed blocks and complete reasoning are preserved |
| Parallel reads | Fast results become durable before slow siblings |
| Shell succeeded, result not committed | Unknown outcome blocks automatic replay |
| Invalid calls / truncation / 429 / limits | No tool side effects, polluted history or unbounded retry |

These fixtures do not establish live-vendor signature acceptance, production
gateway compatibility or two-release canary stability. The default stays native;
P4 rollout and P5 deletion require their own evidence.

The recovery guards additionally cover engine changes during approval, restoring
the original engine without losing approval, and native rollback on a new thread.

### Local P3 verification (Windows, Rust 1.97.1, 2026-11-01)

The final Rig run passes 716 tests across the library, agents API, AgentAsTool,
four scheduler suites, group streaming and provider contracts, with three
existing ignored tests and no filters. Group streaming includes 24 Rig scenarios,
covering the ten acceptance areas above, explicit resume after invalid/truncated
attempts, retained accepted tools, all four provider loops and rejected-terminal
billing. Formatting, strict workspace/all-target/all-feature Clippy and whitespace
checks pass.

Native passes 677 tests in the same nine suites, with three existing ignored
tests and no filters. Both final commands scope `NO_PROXY=127.0.0.1,localhost`
to the test process; the 50 native provider contracts also passed three complete
default-parallel runs with this setting. Ambient Windows
system-proxy runs hit connection abort/reset errors (10053/10054); the system
proxy did not bypass loopback. A serial contract run also passed. Application
proxy behavior and production retry policy were not changed.

An intermediate overlapping Windows test/build run failed to relink a running
test executable and had two group-test failures (early end of the 25-call chain,
missing Shell artifact). Both group cases passed in the final unfiltered Rig run;
their underlying intermittent causes are not claimed fixed. Do not concurrently
rebuild an executable while its tests are still running on Windows.

P3 head `de982a8` passed all eight jobs in [CI run 41264985892](https://github.com/sligter/qunica/actions/runs/41264985892),
including unfiltered workspace tests: native 1,065 and Rig 1,106 passed, seven
existing ignored each. This includes the approval-engine binding follow-up.
Live-vendor replay and P4/P5 remain separate gates.

## P4 canary controls and measurements

The rollout tooling keeps native as the default. The Agent create/edit forms
offer an explicit engine choice only for `llm_chat`; Rig requires a server built
with `rig-engine`. Editing visible model settings preserves `rig_limits` and
other model metadata. An unavailable Rig build does not silently convert an
existing Rig agent to native.

The Token usage page includes owner-scoped engine measurements and a **Pause
new Rig executions** control. The authenticated API is:

| Endpoint | Contract |
| --- | --- |
| `GET /api/v2/runtime/engines` | Build availability, native default, versions and this owner's intake setting |
| `PATCH /api/v2/runtime/engines/rig` | `{ "paused": true }` pauses new Rig executions for this owner; `false` reopens intake |
| `GET /api/v2/runtime/engine-metrics?days=7` | Rolling UTC window, 1–90 days; optional `agent_id`, `provider_id`, `model` filters |

Intake is admitted atomically with the execution snapshot. A pause leaves
already admitted execution and pending approval work on its recorded engine;
it does not cancel running work or edit agents. A newly dispatched nested Rig
agent is a new execution and must pass intake too. Paused attempts record
`blocked` before provider or tool I/O. To stop an already running task, use the
existing cancellation control. After a pause, accepted continuations can still
incur model usage while settling their original batch.

### Measurement contract

Migration 0051 adds owner-scoped execution snapshots and nullable engine /
execution links to the existing token ledger. Snapshot identity is the original
checkpoint message id; approval resumes update that execution instead of adding
a second successful sample. The snapshot freezes engine, app and SDK versions,
provider id/kind and model. Changes to today's Agent settings cannot relabel
history. Active time excludes waiting between invocations. Tool totals come from
the cumulative durable snapshot; model usage is pre-aggregated per execution
before joining, so multiple model calls and resumes do not multiply costs.
Each delegated execution has its own measurement. Parent tools and child tools
remain separate executions; moderator / ACP calls are separately tagged and
do not enter llm_chat engine comparisons.

- Failure rate is `failed / (completed + failed)` for fully observed executions.
  Waiting, cancellation, blocked intake, interrupted runs and unknown external
  outcomes are reported separately. A tool can fail while the agent completes;
  tool error rate is failed tools / settled tools, reported independently.
- Startup marks unfinished observations `interrupted` with incomplete coverage.
  Resuming a pre-P4 checkpoint, changing model/provider/version during a resume,
  or recovering a crashed measurement marks it partial. Its counters and tokens
  are excluded from complete comparison samples; the billing ledger is retained.
- Old ledger rows remain unassigned. Their count is account-wide even when the
  metrics query filters one agent/model. There is no inference from current
  settings and no reconstruction of old token ownership.
- A missing usage record is not zero cost: the API reports measured call count
  and completed executions with no usage, and the UI shows `—` / `*`. These do
  not detect every missing usage frame in a multi-call execution. Compare only
  cohorts with verified provider usage coverage, on matched providers, models,
  task/tool workloads and versions. Observed token counts alone do not establish
  cost improvement. Zero denominators are `null`, not a claimed 0% failure rate.
- Failure to finalize measurement does not reinterpret an already committed
  side effect or response; the unfinished row remains outside terminal rates.
  Intake/snapshot failures stop before provider I/O. Measurement data contains
  identifiers and counts, not prompts, tool arguments, signatures or credentials.

### Rollout and rollback procedure

1. Record the authorized instance, owner and internal Agent IDs, provider/model,
   app/SDK versions, cohort start and matching native baseline. Use an isolated
   workspace/branch for coding tasks. Do not duplicate side-effecting work by
   shadow-running native and Rig.
2. Complete live-provider replay for each enrolled provider: signed Anthropic
   thinking through a tool call and process-restarted approval; DeepSeek
   reasoning passback through the real alias/gateway; Gemini thought signatures;
   Responses encrypted reasoning; custom headers and usage/error propagation.
   Local fake-provider fixtures do not establish vendor acceptance.
3. Explicitly set only those Agents to Rig. Start fresh threads. Observe the
   engine panel and export the filtered metrics API result with each release's
   revision/window/cohort. Check provider usage coverage before cost comparison.
4. On duplicate side effects, lost signed metadata, incorrect approvals,
   cross-account leakage, unknown outcomes or failure/tool-error regression,
   pause intake immediately. Preserve checkpoints and audit events; cancel
   running work explicitly if needed. Never replay an unknown Shell outcome.
5. Roll back new work by selecting native and starting a fresh thread. Retain a
   Rig-enabled server for pending original Rig checkpoints. Restore llm_chat /
   Rig configuration before accepting their approvals; accepted jobs retain
   that execution path despite later Agent runtime edits. Downgrading the server
   to a native-only build cannot resume Rig checkpoints. Do not downgrade a
   migrated SQLite database to an older application as a rollback shortcut.
6. Require two distinct release windows with recorded cohorts, matching native
   evidence and no unresolved regressions before separately approving a default
   switch. Retain native for another release. P5 deletion has its own gate.

The P4 tooling and isolated tests are implementation work, not live enrollment.
No real instance/Agent cohort has been selected or enabled here. Live-vendor
replay, production observation and the two-release promotion gate remain open;
no default switch or P5 deletion is included.

## Validation

Run from the repository root:

```sh
cargo fmt --manifest-path backend-rs/Cargo.toml --all -- --check
cargo clippy --locked --manifest-path backend-rs/Cargo.toml --workspace --all-targets --all-features -- -D warnings
cargo test --locked --manifest-path backend-rs/Cargo.toml --workspace
cargo test --locked --manifest-path backend-rs/Cargo.toml --workspace --features qunica-backend/rig-engine
cargo check --locked --manifest-path frontend/src-tauri/Cargo.toml --all-targets --features rig-engine
```

CI checks the Rig-enabled backend and desktop shell on Windows, Linux and
macOS. The Android workflow uses the pinned toolchain but deliberately depends
only on the mobile link library, not the backend or Rig. The Docker compile
gate builds the `server` stage with `BACKEND_FEATURES=rig-engine` and builds the
separate supervisor image. Normal image builds keep `BACKEND_FEATURES` empty.

```sh
docker build --target server --build-arg BACKEND_FEATURES=rig-engine -t qunica-rig-server:poc .
docker build -f docker/supervisor.Dockerfile -t qunica-supervisor:poc .
```

Checked-in build gates do not establish cross-platform success: P1 is not
fully accepted until their actual results and unchanged native tests are
observed. Lockfiles for backend and desktop must be updated together. Android
and mobile-link lockfiles need no Rig packages because they do not depend on
the backend.
