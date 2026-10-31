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

`float_roundtrip` also changes parsing of some decimal numbers; sorting keys
alone cannot establish compatibility with every historical payload digest.
For example, `8.971135692166839e-150` is parsed differently by the two feature
sets. Before using a Rig-enabled binary against an existing production database,
legacy float-bearing Webhook/command identities need an explicit compatibility
or versioned migration policy. Until that gate is resolved, use isolated PoC
data, keep normal builds feature-disabled, and do not declare P1 fully accepted.

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
