# Speaker selection wire contract

## Problem

`speaker_selected` had an anonymous Rust JSON producer, a TypeScript interface and an inline Zod schema. Independently changing any of them could alter a required nullable field without a cross-language failure. This is a bounded first migration, not a new event protocol.

## Decision

`qunica_domain::events::SpeakerSelectedPayload` defines the producer's seven fields. Borrowed strings avoid allocating replacements for values already owned by the coordinator. The Scheduler's existing `SelectionReason` and `ActionKind` retain their ownership and `as_str()` mappings. The runtime's pure payload builder is exercised against the shared fixture, and the durable emit call keeps its existing kind, await and persistence order.

`domain/src/fixtures/speaker_selected.json` contains one complete existing-format event. Rust tests compare the DTO and actual runtime payload builder with it. Frontend tests read the same physical file through Vite's raw loader and validate it with the public Scheduler parser. Event ID and sequence construction still use the existing `StreamEvent::new`.

`frontend/src/lib/api-v2/speakerSelected.ts` owns the existing Zod expressions and derives its TypeScript payload, selection-reason and action-kind types with `z.infer`. Existing imports through `types.ts` retain their paths. Other Scheduler payloads reuse the same enum schemas. The Rust and TypeScript declarations remain separate; the shared fixture is a compatibility gate, not complete schema generation.

The seven keys remain required. In particular, `source_agent_id: null` is valid; omission and `undefined` are invalid. A string source is valid. Unknown payload fields, unknown enum values and fractional hops retain the original rejection behavior. Future event kinds are ignored and legacy `done` with an empty payload remains outside the Scheduler parser. No optional defaults, persisted fields or event identities are changed.

Focused checks from the repository root:

```sh
cargo test --locked --manifest-path backend-rs/Cargo.toml -p qunica-domain --lib speaker_selected
cargo test --locked --manifest-path backend-rs/Cargo.toml -p qunica-backend --lib speaker_selected_wire_fixture
pnpm --filter @qunica/frontend test -- src/lib/api-v2/speakerSelected.contract.test.ts
```

The first check protects the DTO shape; the second protects the actual producer's field mapping and event identity. The frontend test covers both source values, omitted/undefined source, extra fields, invalid reason/hop, future kind and legacy `done`. The complete Runtime/recovery and frontend suites remain the regression gate.

## Alternatives considered

Doing nothing or reusing only the existing parser tests minimizes changes, but does not connect their input to the real Rust producer. Generating TypeScript directly from Rust could remove more duplication, but does not by itself replace runtime validation and would introduce build tooling before a single migration establishes compatibility. JSON Schema generation could connect validation and types across languages, but requires decisions about strict objects, nullable versus optional values and schema versions. Keeping one Zod definition with a shared golden fixture fits the current dependencies; a generator can be evaluated in a separate migration with these tests as a baseline.

## Consequences

The first migrated event has one TypeScript schema/type definition and an explicit tested producer. There is no new dependency, schema migration, global state, retry, approval path or performance claim. One fixture does not enumerate every possible string or enum combination; the existing Scheduler/event tests remain necessary. The Rust DTO is a serialization shape, while Scheduler types still constrain the values supplied by the runtime.

Revisit when another payload needs the same migration, fixture drift becomes recurrent, or a generator demonstrably preserves the current optional/null and strict-object semantics. Any event behavior change requires a separate compatibility decision. Reverting this contract commit restores the anonymous JSON producer and the previous TypeScript declarations without touching stored data.
