# Realtime synchronization

The backend and clients expose durable account discovery and command acceptance.
Direct Web, desktop and Android clients use a shared ticket WebSocket. Chat SSE
remains supported for encrypted desktop pairing. These adapters share
the same authorization, command receipts, jobs and stored run events.

## Instance and login

`GET /api/v2/instance` returns a stable `server_id`, build version, protocol
versions, supported transports and execution capabilities. It contains no keys.
The supported protocol is `qunica.realtime.v2`. Device sessions remain valid
until explicitly revoked; see [Login and device sessions](settings.md#login-and-device-sessions).

A backup restored as the same instance retains its database identity and signing
key. A copy deployed as a different instance must receive a new `server_id` and
have its device sessions and realtime tickets removed during maintenance. Do
not operate a backup clone as a second live instance with the original identity.
After changing identity, clients obtain a new snapshot and sign in again.

## Snapshot and replay

`GET /api/v2/account/snapshot` returns conversation/task summaries, active turns
and jobs, pending approvals, resource versions, `server_id` and a decimal-string
`cursor`. Resource data and that watermark come from one SQLite read transaction.
Provider credentials and model tokens are excluded.

`GET /api/v2/threads/:id/snapshot?before=<message-sequence>` returns up to 100
messages in server sequence order, run cursors, pending approvals, and accumulated
in-flight text/reasoning plus non-token events. These are read in one transaction;
subscribe after each returned run cursor to avoid losing in-flight output.

Account events are small invalidations for database-backed resources. SQLite triggers persist them
in the transaction that changes the resource, including writes from HTTP,
internal tools and the runtime. Resource versions outlive event cleanup. Run
tokens and reasoning stay in the existing run log, with their original event ids.
Clients fetch the affected resource and upsert formal messages by id; account
events do not replace message bodies. Never revive a deleted or terminal message
from a late run token.

Account history retains seven days and approximately 100,000 events per account,
with a minute cleanup cycle. Indexed discovery selects up to 256 old events
outside the write transaction; deletion and replay-floor advancement commit
atomically in at most four batches per cycle, releasing the writer between
batches. Expiration is gradual for a large backlog. A durable retention floor
makes an expired cursor
return `resync_required`, even after all old rows have been deleted. Replay bounds
and rows share a read transaction so cleanup cannot hide a gap between them.

Ended run events default to seven days, configurable with
`QUNICA_ENDED_RUN_EVENT_DAYS` (minimum one). Active jobs and threads with pending
approvals retain recovery material. Run retention floors distinguish expired
replay from a missing or unauthorized resource. Formal messages and receipts
have separate lifetimes.

Filesystem and Git mutations cannot join a SQLite transaction. HTTP writes first
persist an invalidation intent, then settle it into an account event before the
response. Cancellation settles the intent independently; startup settles intents
left by a crashed process. Finished tool/CLI steps also invalidate files. Progress
frames and model tokens do not cause file scans. External edits outside Qunica
use the slower file/Git fallback refresh, not a native filesystem watcher.

Paired clients subscribe with authenticated Fetch SSE:
`GET /api/v2/account/events?server_id=<id>&cursor=<decimal-string>`. `Last-Event-ID`
overrides the query cursor on reconnect. A different instance returns
`server_mismatch`. The SSE and WS adapters read the same log. Neither adapter
starts an agent when subscribing or replaying.

## WebSocket

1. Authenticated `POST /api/v2/realtime/tickets` returns a one-use ticket valid
   for 30 seconds. Only its hash is stored, bound to the instance and device.
2. Connect to `/api/v2/realtime` with subprotocol `qunica.realtime.v2`; use WSS
   through the HTTPS deployment. Origin must match the request host or an explicit
   allowed origin. Native clients may omit Origin but still need a ticket.
3. The server sends `hello`. Within five seconds send
   `{"type":"authenticate","ticket":"..."}` and await `authenticated`.
   Credentials in the query string are rejected. Never put a login token in a URL.
4. Send `{"type":"subscribe","scope":"account","server_id":"...","cursor":"..."}`.
   Use `run:<stream_id>` for run details; `-1` requests that run's first event.
   `unsubscribe` removes a scope. Every run is checked against account ownership.
5. Events arrive batched, one frame per scope per poll:
   `{"type":"events","scope":"...","cursor":"<last>","events":[...]}`. Each entry
   keeps the single-event shape with its own `cursor`. Apply the whole batch,
   then acknowledge the frame's scope/cursor once. On reconnect supply
   the last applied cursor, not the last frame received. On `resync_required`,
   replace the snapshot. Application restart obtains a new snapshot; cursors alone
   are not persisted as a substitute for local state.

Application `ping` runs every 20 seconds; clients reply `pong`. Sixty seconds
without pong ends the connection. Inputs are limited to 64 KiB; output buffering
is capped at 4 MiB, 32 subscriptions, and 256 fetched events per subscription batch.
Each socket write has a two-second timeout. A slow socket closes and replays from
its cursor; runtime execution does not wait for it. Device revocation is checked
throughout WS and SSE lifetimes.

## Commands and jobs

HTTP `POST /api/v2/commands` and WS `command` frames use this envelope:

```json
{
  "request_id": "3ca61722-2788-49bf-9811-35287db6ae3f",
  "name": "message.send",
  "target": { "conversation_id": "<conversation-id>", "thread_id": "<optional-task-id>" },
  "payload": { "content": "Check the project", "attachments": [] }
}
```

Names are `message.send`, `turn.cancel`, `turn.acknowledge_stopped`,
`thread.resume`, and `approval.resolve`. Cancel and stop confirmation name a
`target.turn_id`; continuation and approval name `target.thread_id`.
The accepted response contains stable request, job, turn and stream ids when
execution is involved. `GET /api/v2/commands/:request_id` also reports current
execution status. Accepted means committed, not completed.

Receipt, user message, initial event, job and turn are committed atomically.
The same account/request id and normalized content returns the receipt; a changed
command, target or payload returns `idempotency_conflict`. Clearing or deleting
resources does not remove receipt tombstones. Legacy `client_request_id` uses
this same service. Chat HTTP/SSE and resume endpoints are compatibility adapters.
An explicit `X-Request-ID` is accepted on legacy cancel endpoints.

The server claims queued jobs with an atomic generation update and drains runtime
output independently of clients. Startup executes queued work and marks already
started jobs interrupted; it does not replay uncertain external side effects.
Generation and stream checks prevent an old worker from finalizing a continuation.
Jobs reserve their task thread across manual and automatic entry points, so
independent threads in the same group can execute concurrently. Waiting and
unconfirmed interrupted jobs reserve only their own thread; cancelled workers
keep that reservation until cleanup finishes. Occurrences of the same automation
remain sequential, and shared computers retain their own exclusive lease.
Manual work sorts ahead of automatic work that has not started.
`turn.acknowledge_stopped` explicitly releases the interrupted thread after its
old actions have stopped; it never retries the interrupted action. See
[Groups](groups.md#managing-tasks) and [Automations](automations.md).

## Approvals and restart

An approval question is stored atomically with the interrupted message and binds
the exact tool id, name and arguments to an action hash and version. Resolve with
`tool_call_id`, `approved`, `expected_version`, `action_hash`, and optional
`remember`/`note`. Pending state/version are compared in the same transaction that
claims the continuation. Conflicting devices cannot execute two decisions.
The runtime checks the recorded action again before invoking the tool. Remembered
grants remain scoped to the original thread/rule.

Waiting turns/questions survive startup recovery. An explicit legacy Continue
without an approval decision skips and cancels the unanswered question; it never
grants that action. A subsequent tool proposal still passes through its own gate.
An approval continuation reuses the existing job/turn. Already started work that
crashed remains interrupted and needs observation before any manual recovery:
`turn.acknowledge_stopped` releases the group only after a person confirms the old
worker and its external effects stopped.

### Native tool execution checkpoints

Native `llm_chat` messages use `content_json` schema 2. Legacy schema 1 remains
readable; signatures absent from old records cannot be reconstructed. Invalid
or newer checkpoint schemas stop continuation instead of discarding execution
state and asking the model again.

Each model tool round retains its original Assistant message, including exact
reasoning, signatures, provider-native content blocks and tool metadata. Results
retain the provider-facing output separately from display summaries. Replay
preserves model-round boundaries and exposes private blocks only to their owning
agent. Compaction treats a tool exchange as a unit and includes opaque blocks in
its token estimate.

The complete batch is committed before dispatch, including calls after an
approval gate. A checkpoint revision is atomically claimed before execution;
stale revisions cannot dispatch a tool. Each completed result is committed before
waiting for slower parallel reads. Internal `running_checkpoint` and
`private_checkpoint` rows do not announce unfinished public messages. Public
completion updates the same message id and sequence. Pausing promotes the public
checkpoint to `interrupted` and persists its approval question atomically.

Startup changes started calls without a durable result to `outcome_unknown`.
It does not infer whether the external action succeeded or replay that call.
Such a continuation stops before a model request; inspect the external effects
and use the existing stop-confirmation workflow before starting new work.
Known results and pending calls retain their original ids and order. Approval
continuation settles that original batch before the next model request. Legacy
Continue records an explicit not-executed result for the skipped approval.
SQLite and an external Shell cannot provide an atomic exactly-once transaction.
Connection aborts/resets before response streaming consume the existing bounded
request retry budget. Once a stream starts, truncation fails the turn instead of
automatically replaying its visible output.

## Client recovery and instance selection

Desktop offers a local instance or a server connection. Only local mode invokes
`backend_start`; remote startup does not open the local database or bind its port.
The remote file picker and terminal address the server filesystem. Android starts
with HTTPS server selection; desktop pairing remains available. Selecting a direct
server preserves the pinned pairing identity in Keystore, without reusing its
account token. Restoring that pairing requires login again.

Remote desktop instances show a QR containing only their HTTPS origin in Phone
connection settings; they never submit that instance's account token to the
embedded pairing backend. Android's connection scanner accepts this origin or a
one-use desktop pairing link. Re-entering or scanning the current server preserves
its login. The mobile workspace labels Files, Git, Computer and Triggers explicitly.
Android computer artifacts stream into private native staging in at most 64 KiB
chunks before the system save picker opens, with progress and length validation.
Account changes, view disposal or explicit cancellation abort incomplete transfers;
picker cancellation discards staging. This supports the existing 1000 MiB artifact
limit without constructing a whole-file JavaScript Blob. See the
[Android connection guide](../../../../../../android/CONNECT.md) for capabilities
and physical-device acceptance boundaries.

Desktop first entry offers Local mode (embedded service, no Docker) or an existing
server and remembers the choice. Continuing or retrying the same instance retains
the login. A different server is checked after reload installs its connection policy.

Changing instance clears credentials, drafts, queries, message/activity state,
terminal metadata and cursors before changing origin. Token-origin binding rejects
late requests sending an old credential to a new destination. Native shells
install a connection policy limited to the selected HTTP/WS origin. Android's
base policy permits WSS and the Vite API proxy supports upgrades. Public servers
require trusted HTTPS certificates and explicit allowed client origins.

Account snapshots discover unopened conversations and pending work. Account events
invalidate lists and settle remote activity/notifications by event id, including
jobs that finish before the next snapshot. Opening an active task hydrates its
consistent thread snapshot and subscribes after the returned cursor. Official rows
upsert by message id and decimal server sequence; deletion/clear tombstones reject
late history and stream output. Cursors remain in memory only.

Reconnects query uncertain command receipts and replay subscriptions; they never
resubmit an execution command. Foreground recovery replaces stalled sockets and
refreshes summaries. Offline sending and system push are not implemented.

## Server terminals

Terminals use their own `/api/v2/terminal/realtime` socket and
`qunica.terminal.v1`, with a fresh ticket from the same ticket endpoint. After
`hello`, authenticate with `ticket`, `session_id` and a decimal output `cursor`
(initially `"0"`). `authenticated` returns the starting cursor, optional input
`generation` and a 15-second lease. Output frames contain `cursor` and the existing
terminal event; they never enter the account event log.

`acquire_input`, `renew_input` and `release_input` control the lease. Input,
resize and close frames carry its generation and a `request_id` for an ack/error.
The UI renews every five seconds, shows observers as read-only, and offers explicit
request/release actions. Disconnect releases that connection's lease. Old
generations cannot write or resize. Pending and queued input is rejected after
reconnect instead of replayed. Slow PTY writes use blocking workers and cannot
grant another driver until the in-flight action finishes.

`GET /terminal/sessions?conversation_id=...` and `GET /terminal/sessions/:id`
list/read the owner's live PTYs. The dock can attach another device to a listed
session and retain its id across a client restart. Missing PTYs return
`terminal.session_missing`; a server restart does not silently create another shell.
`terminal.output_gap` means retained bytes are insufficient. An explicit redraw
opens a new view with `repaint: true`, preserving the shell. This omits earlier
output and is not a full terminal-screen snapshot.

Paired terminals retain Fetch SSE plus HTTP input/resize. They acquire, renew and
release via `POST /terminal/sessions/:id/lease`, using a UUID `client_id`. Writes
include `X-Terminal-Client` and `X-Terminal-Generation`. SSE binds lease lifetime to
that body and reports generation/cursor/stream in exposed response headers.
`?repaint=true` explicitly starts at the retained tail. Closing a read-only view
leaves the other device's shell running. Desktop local PTYs retain the native adapter.

Protocol, SQLite, frontend and native compile fixtures are local verification.
Real VPS TLS, physical three-device interaction, the specified performance baseline
and full Noise-device acceptance remain separate gates; local tests do not prove them.

Ended-run retention discovers up to 256 old events outside the SQLite write
transaction, then rechecks active jobs and pending approvals before atomically
deleting each batch and advancing replay floors. Each maintenance tick runs at
most four batches and releases the writer between them. Indexed completion
markers remain until their payload events are gone so later ticks can continue
cleanup. Expiration is gradual for a large backlog; messages and command receipts
are unaffected. This prevents a full legacy history scan and deletion from
monopolizing the writer used by message acceptance and attachment uploads.

Thread run cursors use a covering `(thread_id, stream_id, seq)` index so snapshot
grouping does not read historical event payloads or build a temporary sort table.
SQLx slow-statement warnings remain enabled with their default one-second
threshold. Inspect `summary`, `db.statement`, `elapsed` and `slow_threshold` in
the expanded log fields: elapsed time can also include SQLite lock or I/O waits,
so a warning alone does not establish that an index is missing. WAL and the
existing synchronous durability setting are unchanged.
