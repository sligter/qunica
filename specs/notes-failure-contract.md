# Shared notes: observed persistence failures

## Problem

Notes have three persistent representations: SQLite registration/content,
`Notes/<host-id>.md`, and the generated `Notes/index.md`. The registered Markdown
file is preferred by HTTP/ACP detail reads; missing files fall back to the SQLite body. The
HTTP list intentionally returns empty bodies. Native `EditGroupNote` edits only
the file; ACP edits update both the file and SQLite. A mismatch alone is not proof
of corruption. See the [shared-notes contract](../backend-rs/crates/backend/src/docs/guide/groups.md#shared-notes).

Create/update execute SQL, write the body, commit, then refresh the index. Delete
updates the row, removes the body, commits, then refreshes the index. SQLite
rollback cannot undo a completed filesystem operation. Error responses therefore
need examination before extracting this business flow from `api::groups`.

## Decision

The first R08 change adds characterization tests only. The matrix below describes
the **existing implementation**, including confirmed defects; passing these tests
does not mean those defects are fixed or an acceptable permanent contract.

| Injected failure | HTTP result | Database / account events | Body and index / reread |
| --- | --- | --- | --- |
| INSERT/UPDATE statement rejected during create, update or delete | 500, existing operation-specific message | Rows, event identities and account sequence unchanged | Original body and index unchanged. |
| `Notes` is a file, or registered note path is a directory | 400 `invalid_input` | Transaction and its account event roll back | External obstruction is preserved; index unchanged. This is path rejection, not a simulated disk-full write. |
| Create COMMIT rejected | 500 `failed to commit group note create` | No registration or `note.changed` event | New orphan Markdown exists; GET rejects its unregistered ID, list is empty, reads do not repair it. |
| Content update COMMIT rejected | 500 `failed to commit group note update` | Original metadata/body and event sequence restored | New body remains in the file; old index remains. GET reads the new body with old metadata, including after closing/reopening SQLite and rebuilding the HTTP router. |
| Title-only update COMMIT rejected | Same update error | Original row/events restored | Independently edited file remains unchanged. |
| Delete COMMIT rejected | 500 `failed to commit group note delete` | Registration remains active; no deletion event | File is already removed. GET falls back to older DB content. A completed native `EditGroupNote` result is lost, including after database reopen. |
| Index path is a directory after create commits | 400 `group note index path is not a regular file` | New row and account event committed | New body exists and can be read by registered ID. An explicit repeat POST creates a second active note; the test performs the repeat deliberately, not as a recommended recovery policy. |
| Same index failure after update | Same 400 | Updated row and account event committed | Updated body is readable. Removing the obstruction and reading does not regenerate the index. |
| Same index failure after delete | Same 400 | Deleted row and account event committed | Body is absent, GET/repeated DELETE are 404, list excludes the note; repeat deletion does not emit a second event. |

SQLite faults use test-owned triggers and a `DEFERRABLE INITIALLY DEFERRED`
foreign key. A control test proves INSERT succeeds and the actual COMMIT fails
with SQLite code 787; no persistence function is mocked and foreign keys stay
enabled. File obstructions are confined to each fixture's temporary workspace.
No production fault hook, migration, sleep or retry is added.

ACP bridge tests exercise real loopback JSON-RPC. A post-commit create error has
`isError: true` even though listing/reading finds the committed note. An edit
COMMIT error leaves the new file readable with the old DB body and no new account
event; repeating the same exact replacement fails because the old text is absent.
Revoking the lease still rejects create/edit with 403 and leaves data unchanged.
Existing tests retain membership, cross-group, stale-edit and delayed-revocation
coverage.

## Alternatives considered

Existing CRUD and permission tests are inexpensive and already protect ordinary
behavior, but do not observe these failure windows. Mocked commit failures allow
fine timing control, but cannot establish real SQLite rollback behavior.
Extracting a service first would reduce API dependencies sooner, but risks
silently changing these outcomes. Treating SQLite as the sole body authority
would simplify writes, but discards legitimate native/file edits.

## Consequences

Confirmed P1 defects are (1) failed deletion can destroy newer file-only content,
and (2) an operation can commit while reporting an index error, with duplicate
creation on explicit retry. Failed updates/orphan creates also leave persistence
representations inconsistent. These are fault-injection results, not claims about
normal production incidence. There is no application behavior fix in this change.

R08 remains in progress. Preserve these observations in separate bug-fix reviews
before moving shared business ownership:

| Next slice | Required outcome and verification |
| --- | --- |
| R08b: failed body mutation | Start with a failing invariant for COMMIT rejection. Preserve or recover the exact previous file bytes, including native edits, and do not overwrite a concurrent newer file. Test create/update/delete and database reopen; account events must agree with committed state. Choose write ordering or a staged recovery scheme only after examining its own failure windows. |
| R08c: committed operation with failed index | Define how callers learn committed identity/outcome and how the index converges. An ambiguous error must not invite an automatic replay of create. Include HTTP, ACP, native Notes readers and approved Assistant action consumers; assess compatibility explicitly. |
| R08d: shared Notes service | Reuse the existing authoring guide, workspace checks and write guard. Extract only after the failure policy is explicit; preserve file-first reads, ownership, membership, symlink rejection, expected-content checks and lease revocation. |

No frontend behavior, transport, schema, security policy, retry or file-authority
change is made. Revisit the observed assertions when an independently verified
fix deliberately changes them; replace them with stronger invariants, not skips
or relaxed checks. These tests do not simulate process termination, power loss,
partial/short writes, disk exhaustion, permission races or real remote platforms.
