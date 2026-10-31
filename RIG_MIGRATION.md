# Rig migration: build baseline

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
crate. This is a build-time PoC boundary, **not** an Agent runtime selector:
enabling it does not route model requests through Rig yet.

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
`RigProvider`. Nothing in the runtime selects it; `build_provider` is unchanged.
It serves the OpenAI-compatible kinds so far. Anthropic, Gemini and Responses
fail with "not served yet" and stay on the native providers.

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
