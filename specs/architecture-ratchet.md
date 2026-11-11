# Incremental architecture checks

## Problem

File length does not show ownership or execution risk. A repository-wide complexity limit would block existing debt without distinguishing a new dependency from an unchanged one. Refactors need a review report for the functions and imports they touch.

## Decision

The existing frontend CI job runs the Node tests and `scripts/check-architecture.mjs`. Checkout includes history. Pull requests compare with the target SHA; pushes compare with the previous SHA. An initial push with an all-zero previous SHA establishes the checked-out commit as the existing-debt baseline and records that choice in the report. Invalid or unavailable nonzero bases fail rather than silently changing the comparison.

For local review after installing the existing workspace dependencies:

```sh
node --test scripts/check-architecture.test.mjs
node scripts/check-architecture.mjs --base <commit-sha> --output architecture-report.json
```

The JSON artifact lists changed Rust/TS/TSX files, touched function spans, branch-token counts, added/removed imports and violations. Untracked source files participate in local checks. Deleted files and moves remain visible. TypeScript function spans use the existing TypeScript compiler; Rust spans and branch counts use lexical scanning that masks comments and strings. Nested function bodies can contribute to enclosing counts. These are review indicators, not cognitive-complexity scores, coverage, performance measurements or a whole-repository threshold. Moves can appear as removals and additions.

The initial dependency ratchet rejects these **new per-file** edges relative to the base:

- Rust `runtime`, `tools`, `db`, `group_notes`, `acp` or `llm` code adding a dependency into the `crate::api` namespace, except the shared error module described below. This includes newly introduced API modules, not just the current Groups/Commands handlers.
- The pure `frontend/src/stores/schedulerProjection.ts` module importing React, Zustand or `messageStore` at runtime.

Explicit exceptions are existing edges, the shared `api::error::ApiError` compatibility type, and type-only imports from `messageStore`. Other dependencies are reported without a gate. Introducing an exception requires changing the rule, documenting its reason here and adding a regression test; it is not a generic bypass flag.

This is a bounded import ratchet, not a Rust module resolver or a security control. It recognizes direct/grouped `use`, `crate::api` aliases and fully qualified paths; macro-generated imports, arbitrary re-export chains, glob resolution, crate-root aliases and conditional reachability still require review. Frontend scanning covers static imports. Rules must remain small enough that reviewers can understand their limits.

## Alternatives considered

Doing nothing or reusing test/lint alone costs least and validates behavior, but does not expose architectural drift. Enforcing all existing complexity immediately gives a uniform target, but confuses accumulated debt with the current change. A full compiler dependency graph would resolve more paths, but needs substantially more tooling and configuration than the two current boundaries require. The incremental report complements the existing behavior and contract tests.

## Consequences

New reverse business imports receive an actionable failing check and a retained report; existing debt remains visible without blocking unrelated changes. There are no new dependencies, runtime state or performance claims. Node tests exercise a real temporary Git repository and the CLI's failing exit status, including untracked violations and the initial-push baseline. Temporary repositories live under the ignored local Agent directory.

Revisit the implementation if a missed dependency causes a real regression, a documented exception becomes unnecessary, lexical metrics become misleading for a target module, or reporting overhead becomes material. A new rule needs examples of accepted existing code and rejected new code. Removing the CI steps and these scripts rolls back the check without changing application behavior.
