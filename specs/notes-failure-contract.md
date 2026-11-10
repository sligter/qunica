# Shared notes: observed persistence failures

## Problem

Notes have three persistent representations: SQLite registration/content,
`Notes/<host-id>.md`, and the generated `Notes/index.md`. The registered Markdown
file is preferred by HTTP/ACP detail reads; missing files fall back to the SQLite body. The
HTTP list intentionally returns empty bodies. Native `EditGroupNote` edits only
the file; ACP edits update both the file and SQLite. A mismatch alone is not proof
of corruption. See the [shared-notes contract](../backend-rs/crates/backend/src/docs/guide/groups.md#shared-notes).

Create executes SQL, stages the body in the same Notes directory, commits, then
publishes without replacing an existing target and refreshes the index. Update
prepares a file version, commits SQL, revalidates publication, writes in place,
then refreshes the index. Delete
validates the file path, commits the row deletion, removes the body, then refreshes
the index. SQLite rollback cannot undo a completed filesystem operation. Error
responses therefore need examination before extracting this business flow from
`api::groups`.

## Decision

R08a (`2f6976b`) established the failure matrix. The deletion fix (`47c1658`)
moves unlink after COMMIT. Committed writes distinguish a failed derived index
from a failed note operation. Creation now stages before COMMIT and publishes
after it. Update also defers body writes until a successful COMMIT. The shared
`group_notes::files` module owns file preparation and publication; the private
`api::groups::note_update` module retains the SQLite authorization barrier.

| Injected failure | HTTP result | Database / account events | Body and index / reread |
| --- | --- | --- | --- |
| INSERT/UPDATE statement rejected during create, update or delete | 500, existing operation-specific message | Rows, event identities and account sequence unchanged | Original body and index unchanged. |
| `Notes` is a file, or registered note path is a directory | 400 `invalid_input` | Transaction and its account event roll back | External obstruction is preserved; index unchanged. This is path rejection, not a simulated disk-full write. |
| Create COMMIT rejected | 500 `failed to commit group note create` | No registration or `note.changed` event | No final Markdown is published. The owned temporary file is removed during normal error unwinding; the fixture Notes directory is empty before and after closing/reopening SQLite. |
| Observe creation during real COMMIT | Ordinary 201, no warning | One active registration and event | No final `.md` is visible inside the COMMIT callback. After success the complete body and index exist, with no temporary file left. |
| Body staging file removed by an external actor during COMMIT | 201 with saved identity and `warning.code=note_content_pending` | Registration, DB body and account event committed exactly once | Publication fails; subsequent index writes stop. GET/ACP detail use the existing missing-file DB fallback, including after database reopen. A pre-existing index obstruction remains untouched. |
| Same publication failure, with pool shutdown during the accepted COMMIT | 201 with the accepted INSERT identity/body and the same content warning | Reopening the physical DB proves one committed registration/event | No post-commit read is required on this error path. The owned stage is absent and index work stops; GET after reopening retains the existing DB fallback. |
| Same publication failure, with the registration changed/deleted by a fixture trigger | HTTP/MCP accepted receipt with content warning; approval records applied | Current registration is deleted; no automatic repair or recreation | The receipt contains the actual INSERT bindings, not a later row. GET remains 404 and ACP list excludes it. Reapproval returns the recorded result without another note/event. |
| ACP lease revoked during COMMIT | MCP saved result with `note_content_pending`, reason `group notes access has expired` | The already accepted registration/body/event remain committed once | The existing write guard is checked again before publication: no final Markdown or index is written and the owned stage is removed. Later MCP requests receive 403. |
| A regular file or directory appears at the target after staging (publisher unit test) | Publication helper rejects it | This filesystem test makes no claim about a transaction | Existing bytes/directory remain; the publisher does not replace the new target and cleans its own staging file. |
| Content update COMMIT rejected | 500 `failed to commit group note update` | Original metadata/body and event sequence restored | The original file and index remain, including after database reopen. No target body is written before COMMIT. |
| External file edit during UPDATE, followed by deferred-FK COMMIT rejection | Same update 500 | Original row and note event sequence restored | The intervening external edit remains, also after reopen. No backup compensation is attempted. |
| COMMIT callback writes a newer body and rejects the commit | Same update 500 | SQLite rolls back the row/events | The newer external body remains, also after reopen. A future rollback handler must not restore an older backup over it. |
| Approved update hits rejected COMMIT | 500 `app_action_failed`; approval becomes `failed` | Note row/events roll back; approval history records the error | The original file remains. Reapproving the same failed action returns 409 and leaves a later external edit untouched. This is a confirmed rejection test, not classification of unknown COMMIT I/O outcomes. |
| File edited, replaced with identical bytes, or removed during successful update COMMIT | 200 with accepted identity/body and `note_content_pending` | Accepted update remains committed | Changed/replacement bytes remain; removed files are not resurrected. Index writes stop. Normal GET stays file-first, or DB fallback for a missing file. |
| Missing body at update preparation | 200 on success, existing 500 on rejected COMMIT, or 200/pending if a target appears during COMMIT | Accepted SQL/event only on successful COMMIT | Reuses staging/noclobber; rejected COMMIT leaves no final body or owned temporary file. A new target is preserved. |
| Registration/content/group/workspace invalidated before publication | 200 with accepted receipt and content warning | No new mutation/event from publication barrier | Current state is checked on the writer connection, including single-connection pools; body/index stay unchanged. Triggers perform real state changes, not mocked queries. |
| Write lease/membership revoked at update COMMIT, or file made read-only on Windows | 200/MCP saved receipt with content warning | Already accepted update remains committed | No body/index publication. Subsequent revoked access is denied. Approved pending updates remain applied; reapproval returns the same receipt without replay. |
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

Creation uses SQLx 0.8.6's [COMMIT callback](https://docs.rs/sqlx/0.8.6/sqlx/sqlite/struct.LockedSqliteHandle.html#method.set_commit_hook)
only on fixture connections to observe publication ordering or remove the body
file during the real commit. These callbacks never query or change their SQLite
connection. The existing deferred-FK
test still proves a rejected COMMIT, independently of this observation hook.

The receipt fixture also calls [SQLx Pool::close](https://docs.rs/sqlx/0.8.6/sqlx/struct.Pool.html#method.close)
inside the COMMIT callback: it fences new acquisitions without cancelling the
checked-out connection. It does not await closure or run SQLite there. The test
awaits closure and reopens the physical database before asserting the response,
independently proving the accepted registration and event survived shutdown.

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

Creation publication failures use the same optional two-field warning shape,
with code `note_content_pending` and reason `failed to publish group note content`
(or the existing path validation reason). On this failure path, the response is
assembled from the accepted INSERT bindings without another DB/file read. This
keeps the accepted identity available if the pool closes or a later deletion
makes an active-row read unavailable. It is a command receipt, not a guarantee
that the registration is still active. Later GETs keep file-first reads.
The index is not refreshed when body publication fails; no unattempted index
outcome is reported. Ordinary creation and index-only failures retain their
previous response shapes. The UI distinguishes saved content awaiting a workspace
file from a saved note awaiting index refresh, in both English and Chinese.

COMMIT is an await boundary, so a scoped MCP write guard is checked once more
before publication. Revocation there cannot undo the accepted DB commit; it stops
the newly deferred file phase and reports its saved identity as pending. A real
COMMIT callback revokes the lease in the regression, while following calls still
fail authorization. This does not add a guard bypass or a new dispatch capability.

The existing `tempfile` dependency owns the staging file. Standard `OpenOptions`
creates it with the same new-file permission policy as `fs::write`, including the
Unix umask. [NamedTempFile::persist_noclobber](https://docs.rs/tempfile/3.27.0/tempfile/struct.NamedTempFile.html#method.persist_noclobber)
does not overwrite an existing target. Its documentation does not promise
all-platform atomicity or cleanup after process termination; some failures may
leave an extra temporary link. The Unix permission comparison test is added for
CI; it is not executed by Windows validation. No process-wide umask is changed.

The create/update response uses an **optional** two-field warning for a failed
post-commit content publication or index refresh. An index failure example:

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
mutation or account event. A confirmed rejected edit COMMIT keeps the original
file and DB body with no new account event. An explicit repeat under the same
fault also fails without a file effect; it is not an automatic retry.
Revoking the lease still rejects create/edit with 403 and leaves data unchanged.
Existing tests retain membership, cross-group, stale-edit and delayed-revocation
coverage.

ACP creation tests also cover a rejected COMMIT and a committed-but-unpublished
body. The latter returns `isError: false`, the saved identity/path and content
warning; reading returns the DB fallback. Revocation still rejects a new create.
Approved Assistant creation with a publication fault remains `applied`; repeating
that approval returns the same result without another registration or event.

### Existing update writers and metadata compatibility

Five update-boundary tests use the existing HTTP fixture. SQLite UPDATE and COMMIT
callbacks change only private fixture files, with no database calls inside the
callbacks. The COMMIT rejection callback returns false, exercising SQLite's real
rollback. The approval case uses AppPropose and the actual approval endpoint.

The Windows fixture marks the note's DACL as protected while retaining its rules.
It then exercises Notes PATCH and the existing Workspace text-save endpoint, not
a replacement mock. A separate hard-link fixture exercises the same two APIs:

| Existing operation | Protected DACL on Windows | Hard-link alias |
| --- | --- | --- |
| Notes content PATCH (checked handle, in-place write) | Protection remains set | Alias sees the changed body. |
| Workspace text save (temporary file + persist) | Protection is lost | Alias keeps the preceding body; target has a different file identity. |

These are observed compatibility differences. The DACL test measures the
protection flag, not an effective-access escalation or every ACL entry. The
Workspace outcomes are not desired Notes update guarantees. Rust's
[Permissions documentation](https://doc.rust-lang.org/std/fs/struct.Permissions.html#method.readonly)
also distinguishes the readonly attribute from ACLs; copying `Permissions` alone
does not establish DACL equivalence. The fixture prints no SID or SDDL, changes
only its temporary note, and clears only the child's inherited `PSModulePath` so
Windows PowerShell can load its own built-in Security module.

Direct reuse also changes validation: Workspace text saves reject NUL bytes and
oversized content, whereas Notes PATCH currently imposes neither of those text
editor restrictions. `SAVE_LOCK` serializes Workspace saves, but native
`WorkspaceTools::edit` performs `fs::write` without acquiring it. Consequently a
check-and-rename solution cannot claim coordination with every Notes writer.

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

Content updates now read/hash the existing file during preparation and again
before writing, and acquire a second short SQLite writer barrier. This adds I/O
and lock work; no latency or throughput improvement is claimed. Revisit this
boundary if measured note-update or SQLite writer wait becomes material.

Index refresh failures report the committed identity without marking a saved
create/edit as failed. A confirmed rejected create COMMIT no longer publishes an
orphan note, and a rejected update COMMIT does not overwrite the target. These are
fault-injection results, not claims about normal production
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

Creation has an explicit commit-to-publication window: termination there can leave
a registered DB body, no final Markdown, and a temporary file. API/ACP missing-file
fallback remains available; native file readers may not see the note until an
explicit repair. There is no new automatic adoption, replay or startup cleanup.
Known constraint rejection is tested; ambiguous I/O errors during COMMIT and
power loss are not classified or solved here. Creation and update publication
failures now return accepted receipts without rereading. Successful-publication
and index-only paths still retain their existing post-commit DB/file reread
behavior. A durable recovery design remains separate work.

R08 remains in progress. Preserve these observations in separate bug-fix reviews
before moving shared business ownership:

| Next slice | Required outcome and verification |
| --- | --- |
| R08b2: failed body mutation | Rejected create/update COMMIT now leaves no final new note or overwritten body. Existing updates preserve metadata/identity and reject observed conflicting publication. Creation/update repair and crash recovery remain explicit boundaries. |
| R08c: committed operation with failed index | Saved create/edit results and committed DELETE conflicts cover HTTP, ACP, native-reader convergence on the next successful write, and approved actions. A failed create publication keeps its accepted identity without a reread, including when the pool closes. Standalone repair and successful-publication/index-only reread failures still need an independently tested policy. |
| R08d: shared Notes service | Reuse the existing authoring guide, workspace checks and write guard. Extract only after the failure policy is explicit; preserve file-first reads, ownership, membership, symlink rejection, expected-content checks and lease revocation. |

### File lifecycle ownership

`group_notes::files` owns body path checks, file-first reads with missing-file DB
fallback, create staging/noclobber, deletion, and the update's file identity/digest
snapshot. It has no connection pool, SQL, HTTP handler, lease, or retry state.
The caller supplies the authorized root and retains responsibility for COMMIT
ordering. The preparation snapshot has private fields and read-only accessors;
the existing database barrier uses those values before consuming publication.

The existing file functions and tests are moved without changing their bodies.
Characterization tests first verify file authority, corrupt/non-file read rejection,
and abandonment of a prepared update on the original implementation. The API's
SQL, membership/lease checks, accepted receipts, index refresh and post-commit
reads retain their order. Reusing `ApiError` preserves existing error envelopes;
this step does not claim a transport-independent Notes service or remove ACP's
remaining dependency on the Group API. R08 service extraction remains incomplete.

### Access and Group projection ownership

`group_notes::access` owns the existing workspace-root, active-group, membership,
and dispatch-lease checks, including the workspace row decoded by the publication
barrier. API orchestration, ACP and the barrier call these functions at their
previous points; authorization is still performed by the host. The same borrowed
lease flag and expected-content reference are preserved. No additional lock,
transaction, query, retry or recovery state is introduced.

The existing `GroupRow`, `GROUP_COLUMNS` and `fetch_row` now live together in
`db::groups`, shared by the API and Notes access. Their fields, SQL and decoding
are retained, rather than adding a second Notes projection. The row is an owned
internal read record, with crate-visible fields; API response mapping remains in
the API. The read helper and access module retain `ApiError` to preserve the
current compatibility envelope; that error-type dependency on the API remains
an explicit cost. This is not a generic Repository framework, and Notes CRUD
orchestration remains in the API.

A characterization test first runs on the original implementation. It stores a
TEXT step budget that SQLite accepts but the original full-row decoder rejects,
with both active and deleted workspaces. All five Notes endpoints return the
same database error before workspace checks, without changing notes, files,
index or account sequence. Restoring the valid budget makes a normal read succeed
or expose the existing inactive-workspace error. This is fixture corruption,
not evidence that the normal API produces such a value. Narrowing the projection
or changing error precedence requires a separate behavior decision and stronger
replacement assertions.

### Update preparation and publication

Existing targets retain a read handle and raw SHA-256 digest before SQL UPDATE.
The MCP expected-content check binds to that same snapshot. Preparation checks
write access without create/truncate; publication reopens the file to honor
changed permissions. Missing targets reuse creation staging/noclobber. Title-only
updates do not rewrite or materialize a body; empty strings and NUL remain valid.

After COMMIT, `db::begin_write` supplies a short publication barrier. Its reads
reuse that connection to check active registration/group, accepted body value,
group owner, workspace identity/root/status and current membership. The scoped
lease is checked after all awaits. This barrier neither mutates a note nor emits
an event; it releases through rollback. A zero-row primary UPDATE is rejected.
An accepted mutation whose publication is blocked returns its known receipt,
without requiring a fresh DB/file read to recover the accepted identity.

The already locked [same-file 1.0.6 Handle](https://docs.rs/same-file/1.0.6/same_file/struct.Handle.html)
provides portable open-file identity in addition to the digest. Both lock files
add only its direct backend dependency edge. Publication checks the opened file,
path fence and identity before writing the handle in place. It refreshes identity
after writing because Windows handle equality includes cached file size. Failures
stop index work and reuse the optional `note_content_pending` warning. English
and Chinese notices cover both create and update without offering a replay.

This boundary does not promise atomic compare-and-swap against uncooperative external
processes between a final check and an in-place write. Partial writes, power loss,
ambiguous COMMIT I/O and durable repair remain separate recovery boundaries.
Stronger external-writer guarantees require an explicit coordination/authority
contract; neither a new mutex nor an optimistic hash check alone establishes it.
This safety-net slice introduces no journal, general storage framework or
production test hook.

The fixes defer deletion/body publication until after COMMIT, add committed
cleanup/index errors and an optional write warning. Routes, ordinary response shapes, database schema,
security policy, shared retry policy and file authority stay compatible. Revisit the observed assertions when a verified
fix deliberately changes them; replace them with stronger invariants, not skips
or relaxed checks. These tests do not simulate process termination, power loss,
partial/short writes, disk exhaustion, permission races or real remote platforms.
