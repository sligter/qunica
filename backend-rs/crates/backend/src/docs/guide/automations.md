# Automations

An automation starts a new task thread in one group when a clock or a signed
webhook fires. It reuses the group's members, topology, model and tool
configuration; it is not a second scheduler. The server runs the task even when
no browser or phone is connected.

Open **Settings → Automation → Automations** to list, create, edit, pause,
inspect history and manage webhook secrets. The same operations remain available
over HTTP for integrations and tests. The manual `turn.acknowledge_stopped`
confirmation described below lives in the affected turn's details.

## Fields

An automation stores the group, name, prompt, trigger, IANA timezone, missed-run
policy, execution budget, enabled state, configuration version and the latest
occurrence result. Listing or reading it never returns the webhook secret.

- `name` and `prompt` are required.
- `budget.max_agent_steps`, `max_total_tokens` and `turn_timeout_seconds` cap one
  run. The runtime intersects them with the group's current limits, so editing a
  group later can only lower the ceiling.
- `missed_policy` is `coalesce_latest` (the default) or `skip`.
- Changes carry `expected_version`; a stale edit is rejected instead of silently
  overwriting another device.

## Triggers

`{"kind":"once","at":"2026-10-20T09:00:00+08:00"}` runs once. The time must
include an offset and must be in the future when enabling the task.

`{"kind":"cron","expression":"0 9 * * MON-FRI"}` uses five fields and the task's
IANA timezone. A local time skipped by a daylight-saving gap is skipped, not
shifted. A repeated local time runs only at its first instant. Editing the
configuration preview returns the next five executions. The editor obtains the
preview from the authenticated, read-only `POST /api/v2/automations/preview`
route, which never creates an occurrence.

`{"kind":"webhook"}` creates a `hook_id` and no secret. Rotate a secret before
delivery; the secret appears only in that response. Revoke it to stop delivery.

`{"kind":"github","events":["push","pull_request"]}` accepts GitHub's native
delivery format. An empty `events` list receives every event. GitHub sends a
`ping` when the hook is created; it is ignored unless `ping` is listed
explicitly. Event names are routing hints, not an authorization boundary: GitHub
signs the body but not the event or delivery headers.

## Webhook delivery

Send `POST /api/v2/automation-hooks/<hook_id>` with headers
`X-Qunica-Timestamp` (Unix seconds) and `X-Qunica-Signature`. The signature is
`sha256=<hex HMAC-SHA256>` over `timestamp + "." + raw_body`. Requests older or
newer than five minutes, bodies over 64 KiB and signatures using another secret
are rejected.

The body is `{"event_id":"delivery-id","payload":...}`. The event id is read
from the signed body, never from a replaceable header. The same event id and
normalized payload returns the original occurrence and job; the same id with
different content returns `idempotency_conflict`. A successful 202 means the
occurrence, thread, message, turn, job and command receipt are already committed.
Webhook payload is untrusted user data: it is labelled as such and never becomes
a system instruction.

## GitHub delivery

Point a GitHub webhook at the same hook URL and choose the JSON content type.
The server verifies `X-Hub-Signature-256` as `sha256=<hex HMAC-SHA256>` over the
raw body with the task secret. `X-GitHub-Event` selects the subscription and
`X-GitHub-Delivery` becomes the occurrence identity. Missing or malformed
headers, wrong signatures and bodies over 1 MiB are rejected.

The delivery id header is not covered by GitHub's signature. To prevent a
captured delivery from becoming new work after its id is edited, the adapter
also deduplicates normalized payload content. The same body with a changed
delivery id returns the original occurrence; the same delivery id with changed
content returns `idempotency_conflict`.

## Queue and pause

One group runs at most one job at a time, whether it came from a person or an
automation. Manual work sorts ahead of automatic work that has not started.
Waiting-for-user and interrupted jobs keep their group reserved.

Pausing stops new occurrences and cancels automatic jobs that have not been
claimed. It does not stop a run that is already executing; use the normal turn
stop action for that. Editing an automation replaces its unstarted occurrences
but keeps the configuration snapshot of running work.

## Restart and missed runs

The server's background loop checks due tasks every few seconds. Startup uses
the missed-run policy: `coalesce_latest` runs only the most recent occurrence
inside 24 hours and journals the skipped range, while `skip` records the skip
without running. A once task older than 24 hours is never replayed.

A job that was running during shutdown becomes `interrupted`. Its group stays
blocked until someone confirms the old worker and external effects are stopped:
send the `turn.acknowledge_stopped` command with the turn id. The interrupted
action is never retried automatically. Queued work survives the restart and is
claimed afterwards.
