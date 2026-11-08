# Shared notes: observed persistence failures

## Problem

Notes have three persistent representations: SQLite registration/content,
`Notes/<host-id>.md`, and the generated `Notes/index.md`. The registered Markdown
file is preferred by HTTP/ACP detail reads; missing files fall back to the SQLite body. The
HTTP list intentionally returns empty bodies. Native `EditGroupNote` edits only
the file; ACP edits update both the file and SQLite. A mismatch alone is not proof
of corruption. See the [shared-notes contract](../backend-rs/crates/backend/src/docs/guide/groups.md#shared-notes).

Create/update execute SQL, write the body, commit, then refresh the index. Delete
validates the file path, commits the row deletion, removes the body, then refreshes
the index. SQLite rollback cannot undo a completed filesystem operation. Error
responses therefore need examination before extracting this business flow from
`api::groups`.

## Decision

R08a (`2f6976b`) established the failure matrix. The deletion fix (`47c1658`)
moves unlink after COMMIT. Committed writes now distinguish a failed derived
index from a failed note operation. The matrix includes these corrections;
create/update COMMIT failures remain explicitly unfixed.

| Injected failure | HTTP result | Database / account events | Body and index / reread |
| --- | --- | --- | --- |
| INSERT/UPDATE statement rejected during create, update or delete | 500, existing operation-specific message | Rows, event identities and account sequence unchanged | Original body and index unchanged. |
| `Notes` is a file, or registered note path is a directory | 400 `invalid_input` | Transaction and its account event roll back | External obstruction is preserved; index unchanged. This is path rejection, not a simulated disk-full write. |
| Create COMMIT rejected | 500 `failed to commit group note create` | No registration or `note.changed` event | New orphan Markdown exists; GET rejects its unregistered ID, list is empty, reads do not repair it. |
| Content update COMMIT rejected | 500 `failed to commit group note update` | Original metadata/body and event sequence restored | New body remains in the file; old index remains. GET reads the new body with old metadata, including after closing/reopening SQLite and rebuilding the HTTP router. |
| Title-only update COMMIT rejected | Same update error | Original row/events restored | Independently edited file remains unchanged. |
| Delete COMMIT rejected | 500 `failed to commit group note delete` | Registration remains active; no deletion event | File bytes remain, including completed native edits and an edit performed during the UPDATE. GET retains those bytes after database reopen. R08a demonstrated their loss before this fix. |
| File removal fails after delete commits | 409 `note_cleanup_pending`, committed identity in details | Deleted row and account event remain committed | File and stale index may remain. GET/repeated DELETE are 404. Releasing the external lock does not automatically replay cleanup. |
| Index path is a directory after create commits | 201 with saved note identity and `warning.code=note_index_pending` | New row and account event committed | New body is readable by registered ID. No repeat create is required. Removing the obstruction and making a legitimate title-only update refreshes the index without creating another note. |
| Same index failure after update | 200 with saved note identity and the same warning | Updated row and account event committed | Updated body is readable. Removing the obstruction and reading does not regenerate the index. |
| Same index failure after delete | 409 `note_index_pending`, committed identity in details | Deleted row and account event committed | Body is absent, GET/repeated DELETE are 404, list excludes the note; repeat deletion does not emit a second event. |
| Windows sharing lock permits index reads/deletion but denies writing | Same 201/200 warning or DELETE 409, reason `failed to write group note index` | Each actual note mutation emits exactly one event | Existing index bytes remain intact. After releasing the lock a successful update regenerates the index, including new notes and excluding deleted notes. |

SQLite faults use test-owned triggers and a `DEFERRABLE INITIALLY DEFERRED`
foreign key. A control test proves INSERT succeeds and the actual COMMIT fails
with SQLite code 787; no persistence function is mocked and foreign keys stay
enabled. File obstructions are confined to each fixture's temporary workspace.
No production fault hook, migration, sleep or retry is added.

The in-flight edit test uses SQLx 0.8.6's [connection update callback](https://docs.rs/sqlx/0.8.6/sqlx/sqlite/struct.LockedSqliteHandle.html#method.set_update_hook)
only on fixture connections to write newer file bytes during the real UPDATE.
The callback does not query or modify SQLite. Windows coverage uses an actual
file handle allowing read/write sharing but withholding delete sharing; it permits
path validation while making file removal fail.

The new deletion error preserves the existing error envelope and adds:

```json
{"error":{"code":"note_cleanup_pending","message":"group note was deleted but its file cleanup failed","details":{"committed":true,"group_id":"<group-id>","note_id":"<note-id>","phase":"file_cleanup","reason":"failed to delete group note content"}}}
```

Its status is 409 because the existing frontend retries DELETE 500 responses.
A client regression proves it keeps this identity/details and sends only one
DELETE. Pre-commit SQL/path/permission errors and successful 204 responses keep
their existing meanings. No shared retry policy or frontend production code is
changed. Cleanup/index reconciliation remains explicit pending work.

The create/update response adds an **optional** field only when the post-commit
index refresh fails:

```json
{"id":"<note-id>","group_id":"<group-id>","title":"Saved","content":"body","created_at":"...","updated_at":"...","warning":{"code":"note_index_pending","reason":"group note index path is not a regular file"}}
```

This is a successful note write, not an instruction to retry it. Ordinary writes,
GET and list omit `warning` entirely, preserving their previous JSON shapes.
The existing TypeScript note type includes the optional warning; this endpoint
already uses the generic API client rather than a Notes-specific Zod schema.
The panel closes its save form and shows a translated saved/index-pending notice.
The original index path checks still reject directories and symlinks; warnings
do not authorize overwriting an obstruction.

DELETE still returns 204 when all steps succeed. Its index error uses the existing
error envelope, code `note_index_pending`, and details
`{committed:true,group_id,note_id,phase:"index_sync",reason:"..."}` with status 409.
The client preserves these details and does not automatically retry DELETE.

ACP bridge tests exercise real loopback JSON-RPC. Committed create/edit results
have `isError: false`, the saved identity/path, and the same warning. Approved
Assistant create/update actions finish as `applied` and persist that result in
`result_json`; approving the same action again returns it without another note
mutation or account event. An edit COMMIT error still leaves the new file readable
with the old DB body and no new account event; repeating the same exact replacement
fails because the old text is absent.
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

The confirmed P1 deletion data loss is corrected by committing before unlink.
This neither restores a stale backup over concurrent edits nor claims atomicity
between SQLite and the filesystem. A crash or cleanup error after commit can
leave the deleted note's file; that is reported explicitly when a response is
available, without replaying the operation or falsely marking it uncommitted.

Index refresh failures now report the committed identity without marking a saved
create/edit as failed. Failed updates/orphan creates on COMMIT rejection remain
inconsistent. These are fault-injection results, not claims about normal production
incidence. A deliberately submitted new POST is still a new create; this change
does not introduce a request deduplication key or guarantee exactly-once delivery.

Index convergence retains the existing rule: a subsequent successful note write
rebuilds it from active registrations. Native readers use the physical index;
ACP lists registrations directly and can still list saved notes while the index
is blocked. Reads have no new filesystem side effects. A standalone repair command
or startup repair is separate work; no artificial note edit or replay of the
original create is recommended as recovery. A lock/obstruction must be resolved
before refresh can succeed. The deletion UI still uses its existing conflict
error display/cache handling; no broader query invalidation policy changes here.

R08 remains in progress. Preserve these observations in separate bug-fix reviews
before moving shared business ownership:

| Next slice | Required outcome and verification |
| --- | --- |
| R08b2: failed create/update body mutation | Deletion's rejected COMMIT now preserves bytes. Establish equally strong create/update invariants without replacing file-first reads or overwriting newer external edits. Choose write ordering or a staged recovery scheme only after examining its own failure windows. |
| R08c: committed operation with failed index | Saved create/edit results and committed DELETE conflicts now cover HTTP, ACP, native-reader convergence on the next successful write, and approved Assistant action consumers. Standalone repair and post-commit reread failures still need an independently tested policy. |
| R08d: shared Notes service | Reuse the existing authoring guide, workspace checks and write guard. Extract only after the failure policy is explicit; preserve file-first reads, ownership, membership, symlink rejection, expected-content checks and lease revocation. |

The fixes change deletion's failure ordering, add committed-cleanup/index errors,
and an optional write warning. Routes, ordinary response shapes, database schema,
security policy, shared retry policy and file authority stay compatible. Revisit the observed assertions when a verified
fix deliberately changes them; replace them with stronger invariants, not skips
or relaxed checks. These tests do not simulate process termination, power loss,
partial/short writes, disk exhaustion, permission races or real remote platforms.
