# Controlled computers

Qunica runs files, Shell, Chromium and supported ACP agents in an explicitly
bound Linux computer. The computer has its own persistent home, project files
and browser identity. An unavailable binding never falls back to the host.
See the [computer spec](../../../../../../specs/controlled-computers.md) for the
product contract and [local acceptance](../../../../../../specs/computers-local-acceptance.md)
for observed checks and the remaining external acceptance gates.

## Deployment

**Settings → Computers → Setup guide** shows the deployment quick start and
opens this bundled deployment guide, including when no Supervisor is configured.
It works without an external documentation site. The desktop
instance menu beside the Qunica logo lists Local and previously connected
servers, and offers **Add server**. Changing instances clears the previous
instance's active login and cached data; choosing the current instance preserves
its login.

Desktop **Local mode** starts the embedded backend and uses the existing local
database and workspaces without Docker or a Supervisor. Choose it on first entry
or from **Switch instance**. Instance selection does not change execution
bindings: explicitly unbind a group or Agent in computer settings to restore host
execution. An unavailable bound computer never silently falls back to the host.

The application has no Docker socket. The optional Supervisor alone controls
Docker through a separate authenticated, internal interface. It has neither the
application database nor vendor model keys. API callers cannot choose arbitrary
images, mounts, ports, Docker options, host paths or network modes.

```sh
python scripts/init-computer-supervisor.py
docker build -f docker/computer.Dockerfile -t qunica-computer:local .
docker build -f docker/computer-egress.Dockerfile -t qunica-computer-egress:local .
docker compose -f docker-compose.yml -f docker-compose.computers.yml up -d --build
```

Only publish the application's normal HTTPS port. The Supervisor and model/notes
listener have no published host port. Set `QUNICA_COMPUTER_PROXY_LISTEN` only for
the execution deployment; the public API/SPA router never registers proxy routes.
The overlay connects the app to the private control network and to the egress
network. The Supervisor creates a distinct **internal** network per computer,
with only that computer and its egress proxy attached. The egress proxy also joins
`QUNICA_EGRESS_NETWORK` to reach the Internet and the fixed backend listener.

`QUNICA_COMPUTER_SERVICES=true` enables the full execution image. Without it the
Supervisor retains the file/Shell profile and advertises browser/desktop/ACP as
unavailable. `QUNICA_COMPUTER_IMAGE` and `QUNICA_EGRESS_IMAGE` are operator-selected.
The local Supervisor binary uses `QUNICA_SUPERVISOR_TOKEN_FILE`,
`QUNICA_SUPERVISOR_STATE`, and loopback port 9000. Its secret is never supplied
to a computer or a browser.

## Isolation and persistence

Each computer has a stable UUID, owner, Team or Private binding, decimal-string
generation and distinct named home. Containers run as uid/gid 10001 with a
read-only root, no Linux capabilities, no new privileges, no published ports and
bounded CPU/memory/PIDs/tmpfs. The full browser profile uses 2 GiB and 512 PIDs;
the file/Shell profile uses 1 GiB and 128 PIDs. The trusted volume initializer
alone runs as root with CHOWN and DAC_OVERRIDE to initialize the agent-owned home.
The only persistent mount is its own home; the project root is
`/home/agent/workspace`. Host workspaces, skills, MCP processes and CLI auth homes
are not mounted into it. Clone or write project files in the computer workspace.

The egress proxy permits public HTTP/HTTPS on ports 80/443. It resolves every
address, rejects mixed public/private DNS answers and connects to the checked IP,
so DNS rebinding cannot redirect a checked hostname. Loopback, link-local, cloud
metadata, private and Docker networks are excluded. Internal sites require
`QUNICA_EGRESS_ALLOW_CIDRS`; deny CIDRs and all Docker network subnets take
precedence. The fixed `/qunica/model/v1/chat/completions` and `/qunica/notes`
bridges reach only the private backend handlers and require both dispatch grants
and a per-computer/generation HMAC proof added by the trusted egress sidecar.
Caller-supplied proof headers are overwritten. A grant from another computer
cannot authorize requests through this gateway. The sidecar receives only its
derived proof, never the Supervisor credential.
They do not forward arbitrary paths to the app. Chromium may access services on
its own loopback; the native broker rejects browser Origins and non-JSON requests.
Direct external sockets, UDP, arbitrary CONNECT ports and host network mode are
not enabled. Docker still shares the host kernel and is not a VM or a hostile
tenant boundary; see [Docker security](https://docs.docker.com/engine/security/).

Stop/rebuild retain home. A destroyed computer remains a tombstone, and deleting
home requires the explicit destroy UUID confirmation plus `purge_home`. Browser
shutdown closes its persistent Chromium context before the container stops.
The operator backup tool confirms a stop, verifies the volume labels, and copies
home without an app socket mount:

```sh
python scripts/computer-home.py backup COMPUTER_UUID backups/computer.zip
python scripts/computer-home.py restore COMPUTER_UUID backups/computer.zip --confirm COMPUTER_UUID
```

Both operations leave the computer stopped. Archives are limited to 64 MiB and
preserve regular project files and browser databases; transient locks and links
are omitted. Restore rejects traversal, links/devices and a different owner or
binding before replacing home. Keep the archive with the Supervisor state,
application database and its signing/session secrets. Starting is explicit after
restore, so old generations cannot keep driving the restored environment.

## Resources and Runtime

The owner-scoped application API provides:

| Route | Behavior |
| --- | --- |
| GET/POST `/api/v2/computers` | List or idempotently provision an explicit Team/Private resource. |
| GET `/api/v2/computers/:id` | Persistent state, generation and advertised capabilities. |
| POST `/api/v2/computers/:id/actions` | Start/stop/rebuild/destroy with expected generation. |
| PUT `/api/v2/execution-environments/binding` | Bind a group or Agent explicitly; active work cannot be rebound. |
| POST `/api/v2/computers/:id/control` | Inspect, request, renew, release, or explicitly resume control. |
| GET `/api/v2/computers/desktop` | Separate desktop WebSocket, authenticated by a single-use ticket in its first frame. |
| GET `/api/v2/computers/:id/artifacts` | Task/turn, path and size for recorded browser artifacts. |
| GET `/api/v2/computer-artifacts/:id` | Authenticated, bounded file download. |

Settings → Computers exposes lifecycle, binding, desktop control and artifacts.
The conversation's right-hand **Computer** tab shows its Team computer and
active members' Private computers; a direct chat shows only its Agent's computer.
It supports an inline desktop, expansion, start/stop, explicit execution binding
and artifact downloads. Changing tabs or conversations closes that viewer and
releases its human lease without automatically resuming the Agent.
Team computers belong to one group; Private computers belong to an Agent. A
Private Agent used in different groups still cannot reserve its computer for two
active/unconfirmed turns. Tool calls recheck owner, active turn, binding and
generation. Turn traces record the actual computer and generation.

A branch task resolves to `.worktrees/GROUP_UUID/THREAD_UUID` inside the computer
workspace. Creating it in a computer-bound group records metadata only; the first
reserved dispatch creates or verifies its Git worktree. Clone the repository into
the computer workspace first. Existing task branches map by branch name, never by
mounting or copying their host path. A missing repository, occupied branch or
mismatched checkout fails explicitly. Built-in files/Shell and OpenCode use the
same task cwd, also included in the native-session reuse signature. Task deletion
retains remote files; remove the worktree explicitly on that computer before
reusing its branch. Computer destruction remains the explicit home purge boundary.

Dedicated built-in group-note tools keep their existing backend-scoped Notes
access. This does not mount the host directory into the computer or expose it to
ordinary file/Shell tools. Container ACP uses the separate notes grant below.

Foreground commands and ACP use a Linux subreaper: descendants that start new
sessions are still stopped before a confirmed exit. A foreground command that
leaves background children returns an explicit failure after cleanup; abnormal
guard/signal exits stay uncertain until stopped. Every remote
side effect has a durable `computer_operations` intent; an ACP process keeps one
for its entire lifetime, including the gaps between protocol polls. Cancellation
retains occupancy until termination is confirmed. Uncertain execution requires
`turn.acknowledge_stopped`, never automatic replay.

## Browser, screen and human control

The pinned Playwright MCP server drives Chromium in the persistent home. The
`ComputerBrowser` tool first lists its supported tool schemas, then dispatches
navigation, snapshots, element actions, tabs, upload and download on that same
computer. Mutations consume `expected_page`; a stale snapshot or takeover cannot
silently target fresh elements. Three repeated unchanged DOM actions or identical
failures require user inspection; scrolling keys are not classified solely by DOM
equality. Snapshot files and downloads are recorded as
artifacts and can be read with the computer file tools or downloaded from settings.
Desktop image/input tools are exposed only when the model has image input enabled.
Every ordered model input consumes the screen reference from its last observation.
Images go to that model request and never into account events or turn trace text.

Human control follows `agent_controlled → takeover_requested → user_controlled →
released`. The Supervisor fences new agent actions **before** waiting for the
current action, then terminates an active ACP driver before granting human input.
It never grants another driver when the old outcome is uncertain. Human leases
last 60 seconds, renew every 20 seconds and are bound to computer generation.
Release/expiry does not automatically resume the agent. Return control explicitly,
then continue the task. An agent encountering held control waits for the user.

The screen uses a separate noVNC/RFB WebSocket and server-side view-only endpoint.
A writable connection must present the current control lease. Lease changes,
computer generation changes and device session revocation close stale input.
Read-only RFB remains observable during a long agent action. Frames are bounded;
input is never replayed on reconnect. Desktop control uses server direct mode on
Web/desktop/Android; the retained Noise pairing path reports this limitation
explicitly and keeps its existing chat/account/file functionality. Neither the
internal VNC address nor the
Supervisor credential reaches the frontend. noVNC is pinned to 1.5.0: the 1.6.0
npm CommonJS build contains top-level await and fails the Vite production build.

## Container ACP and model proxy

Only the OpenCode preset is enabled for computer ACP execution. Select an owned
OpenAI-compatible API provider on the Agent form, then bind its computer. The
managed preset applies ACP mode/settings and rejects local environment overrides
and custom launch arguments/config options. Its stdio process, Shell/files and
native session database run in
that computer. Unsupported presets fail explicitly. The local CLI launch path
retains its existing behavior; a local subscription identity is not converted into
an API provider or copied into a computer.

The backend issues separate model and notes grants for each dispatch. Only hashes
are stored. Grants bind owner, Agent, job/turn, computer generation, provider/model
and a one-hour expiry, and are revoked when the driver ends. Every call checks the
live job, turn, current binding/control and generation, including during a stream
and while waiting for upstream response headers. Reopening the backend revokes
old grants and marks unfinished calls unknown, preserving their budget reservations.
A short-lived notes grant reaches the existing group-note implementation and
rechecks group membership and workspace access for every note operation.

The private proxy supports streaming OpenAI chat completions. Its raw protocol
preserves text, tool arguments, images, errors and usage; non-success vendor errors
are returned without their body, and authentication values are redacted even when
split across SSE chunks. Vendor keys/configuration stay in the backend. There is
no arbitrary upstream URL or request-header passthrough from the computer.

Each call has a durable model_call_id, a payload deduplication anchor and one
usage-ledger entry. The ACP occupancy meter is not billed again. Limits are one
in-flight call and 64 calls per turn, at most 8192 output tokens per request, a
300-second upstream timeout and a 131072-token proxy ceiling. The effective turn
budget also intersects the current group limit, its accepted turn snapshot and
an automation's `max_total_tokens`. Inputs and output allowance are reserved
conservatively before submission; missing usage remains unknown/reserved rather
than zero. A stream interruption never resubmits inference. OpenCode native session
IDs are restored only for the same computer/Agent/thread and context/model signature;
CLI cancellation terminates the guarded process and its descendants, keeping the
durable intent until their stop is confirmed.

## Verification

The [local acceptance record](../../../../../../specs/computers-local-acceptance.md)
records the real Docker browser/download, desktop, persistent home backup/restore,
native Runtime and proxied OpenCode fixtures separately from the ordinary test
suite. Docker tests are opt-in and require both execution images. Physical device,
public TLS, real provider and load acceptance remain external gates; passing a
local fixture does not mark those gates complete.
