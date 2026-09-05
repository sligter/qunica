# Running Qunica in Docker

The image runs the Qunica server: one process that serves both the REST API and
the built web UI on a single port. Point a browser at it and you get the same
app the desktop build shows. Folder buttons browse the server's workspace root,
and the integrated terminal runs a shell inside the container.

> **Agents and signed-in terminal users can run shell commands and edit files
> inside the container.** Treat the container as a machine you have handed to
> them. Do not publish the port on a public interface without a reverse proxy
> that terminates TLS and authenticates callers.

## Quick start

### Mobile PWA over HTTPS

Use a dedicated hostname at `/`, serving the UI and API from the same origin.
The example `docker/Caddyfile.mobile` runs on the Docker host, proxies the
loopback-bound Compose port `18765`, obtains HTTPS certificates through Caddy,
and streams SSE without response buffering. Set `QUNICA_DOMAIN` to your real
hostname, configure DNS and the certificate challenge access required by your
deployment, then run Caddy with that config. Private VPN deployments still need
a certificate trusted by the phone. Keep the initial-account and registration
settings described below; the proxy preserves the app's bearer authentication.

The CSP permits existing dynamic styles, but not inline scripts or arbitrary
script/connect origins. Remote images are supported; add other origins only
for a verified integration. The browser theme bootstrap is a same-origin
external script. The sample disables HTTP caching; the PWA worker separately
caches only exact public build assets, fonts and app icons. It never handles
API responses, authenticated requests, workspace files or downloads. Navigation
requires the network; offline execution and offline sending are not supported.

Build with `pnpm build`. Production browsers register `/sw.js`; desktop shells
and development builds do not. Updates wait until all old Qunica tabs/windows
close; the app displays a notice instead of reloading a draft or approval.
Keep the prior release's hashed assets available until its old tabs are closed.

Before remote use, verify installation and safe areas on iOS/Android, a deep
link reload, timely SSE events through the proxy, and expired-token logout.
Inspect Cache Storage to confirm no private response was stored. Logging out
clears local auth and message state and stops streams; it does not revoke a
previously issued JWT on the server. Per-device revocation is a separate server
capability. This example config must be validated against your actual domain
and proxy installation before deployment.

For a deliberately cross-origin browser client, set `QUNICA_ALLOWED_ORIGINS` to
a comma-separated list such as `https://phone.example,https://phone.example:8443`.
Origins are normalized at startup; wildcard hosts, credentials, paths, queries,
fragments and invalid entries fail startup. The existing desktop/local development
origins remain allowed. This setting is additive and is unnecessary for a same-origin
PWA. Add the environment variable to your service/Compose environment explicitly.

```bash
docker compose up -d --build
```

Open <http://127.0.0.1:18765>, create the local account, then finish onboarding.
The image automatically sets the workspace root to its persistent `/workspaces`
volume, so setup continues with:

1. **Model provider** — endpoint, model, API key.
2. **Default model** for the built-in assistant.

Without compose:

```bash
docker build -t qunica:local .
docker run -d --name qunica \
  -p 127.0.0.1:18765:8765 \
  -p 127.0.0.1:8900-8999:8900-8999 \
  -v qunica-data:/data \
  -v qunica-workspaces:/workspaces \
  -v /var/run/docker.sock:/var/run/docker.sock \
  qunica:local
```

Compose also maps container ports `8900-8999` to the same loopback ports on the
VPS for services started by agents. Those services must listen on `0.0.0.0`;
for example, container port `8999` is available at `http://127.0.0.1:8999` on
the VPS. Keep public access behind an authenticated HTTPS reverse proxy.

The first build compiles the Rust backend from scratch and takes a while.
Rebuilds reuse BuildKit cache mounts for cargo and pnpm, so they are much
faster. BuildKit is required — it is the default in Docker 23 and newer.

## Public VPS first boot

Keep port `18765` bound to loopback and put an HTTPS reverse proxy in front of
it. Before the first start, disable public registration and set the one-time
initial account:

```bash
cp .env.example .env
# Edit .env: use your email and a long, unique password.
docker compose up -d --build
```

The initial account is created only when the `users` table is empty. Once you
can sign in, remove the three `QUNICA_INITIAL_USER_*` lines from `.env`, keep
`QUNICA_REGISTRATION_ENABLED=false`, and run `docker compose up -d` once more.
This recreates the container without leaving the bootstrap password in Docker's
stored container configuration. Qunica also removes that password from its own
process environment before it can reach agent shells.

If registration is disabled on an empty database and no complete initial
account is configured, startup fails instead of leaving an unreachable server.
On an existing database, initial-account settings are ignored and never reset a
password or add another user.

## Volumes

| Path | Holds | Lose it and |
| --- | --- | --- |
| `/data` | SQLite database, generated `SECRET_KEY`, skills, logs | every account, group, agent, and message is gone |
| `/workspaces` | group workspaces, uploads, agent-created files | agent work products are gone |
| `/home/qunica` | user-installed tools, caches, and external-agent sign-in state | those tools, caches, and sessions are lost |

Named volumes come out of the image with the right ownership. Bind mounts do
not, so the entrypoint chowns `/data` and `/workspaces` to the container's
`qunica` user (uid 10001) when it starts as root. If you run the container with
`user:` set to something else, chown the host directories yourself first.

To keep workspaces in a normal host directory, replace the compose volume with
`./workspaces:/workspaces` (or `/srv/qunica/workspaces:/workspaces` on a VPS).
Directory names entered in either Agent creation or Workspace management are
created inside that mount. In the browser, **Choose folder** lists directories
under `/workspaces` on the server; it never opens the visitor's OS picker and
does not upload or mount visitor files. Mount any host directory you want Qunica
to see at `/workspaces` (or below it) before selecting it.

## Environment

Everything has a working default; nothing is required.

| Variable | Default | Notes |
| --- | --- | --- |
| `QUNICA_HOST` | `0.0.0.0` | bind address inside the container |
| `QUNICA_PORT` | `8765` | listen port |
| `QUNICA_APP_DATA` | `/data` | database, secret, skills, and logs live here |
| `QUNICA_WEB_DIR` | `/app/web` | built UI; unset it to run API-only |
| `QUNICA_DATABASE_URL` | `sqlite:///data/qunica.sqlite3?mode=rwc` | derived from `QUNICA_APP_DATA` |
| `QUNICA_LOG_LEVEL` | `info` | `tracing` filter string |
| `SECRET_KEY` | generated into `/data/desktop-secret.key` | set it explicitly to keep sessions valid across a volume reset |
| `ACCESS_TOKEN_EXPIRE_MINUTES` | `10080` | access token lifetime |
| `QUNICA_REGISTRATION_ENABLED` | `true` | set to `false` to reject registration in both the API and web UI |
| `QUNICA_INITIAL_USER_EMAIL` | unset | one-time account email; requires the password below |
| `QUNICA_INITIAL_USER_PASSWORD` | unset | one-time account password, 8–128 characters |
| `QUNICA_INITIAL_USER_NAME` | `Admin` | display name used when the initial account is created |
| `QUNICA_WORKSPACES_DIR` | `/workspaces` | created by the entrypoint and applied as the workspace root while onboarding is incomplete |

## Git

The image sets a system-wide commit identity (`Qunica <qunica@localhost>`) so
the workspace Git panel can commit at all, and marks every directory as a safe
directory so bind-mounted repositories owned by a host user still work. Override
the identity per deployment with `GIT_AUTHOR_NAME`, `GIT_AUTHOR_EMAIL`,
`GIT_COMMITTER_NAME`, and `GIT_COMMITTER_EMAIL` — the compose file shows where.

Pushing to a remote needs credentials you supply: mount an SSH key at
`/home/qunica/.ssh` or a credential file the remote accepts.

## External CLI agents

The runtime is based on Ubuntu 24.04 and includes Node 22 with npm/npx,
Python 3.12 with uv, Go 1.27, Rust 1.88 with Cargo, Git, and native build tools.
Docker 29 CLI, Compose, and Buildx are included too; no Docker daemon runs
inside the container.
The Agents page can therefore install and run ACP runtimes such as Codex,
Claude Code, and OpenCode. Their **accounts are not** part of the image. Sign
in inside the container, or mount the runtime's config directory from a host
that is already signed in:

```yaml
volumes:
  - ~/.claude:/home/qunica/.claude
  - ~/.codex:/home/qunica/.codex
```

Global npm packages install under `/home/qunica/.npm-global`; `uv tool`,
`cargo install`, and `go install` use paths under `/home/qunica` too. All are on
`PATH`. Keep the compose volume at `/home/qunica` to persist those installs,
runtime configuration, and caches across rebuilds, or bake them into your own
image built `FROM qunica:local`.

Compose mounts the host Docker socket by default, and the entrypoint maps its
group to the non-root `qunica` user. This uses the host daemon rather than
running a second daemon inside Qunica. **Anyone who can run an agent can take
full control of the Docker host**, including mounting and editing host files;
Docker documents this as part of the
[daemon attack surface](https://docs.docker.com/engine/security/#docker-daemon-attack-surface).
Remove the `/var/run/docker.sock` volume to disable it or use a remote Docker
context instead.

## Behind a reverse proxy

The app streams responses over Server-Sent Events, so response buffering has to
be off, and uploads go up to 26 MB.

```nginx
location / {
    proxy_pass http://127.0.0.1:18765;
    proxy_http_version 1.1;
    proxy_set_header Host $host;
    proxy_set_header X-Forwarded-Proto $scheme;
    proxy_buffering off;
    proxy_read_timeout 3600s;
    client_max_body_size 32m;
}
```

Serve the UI and the API from the same origin, as this image does. The backend's
CORS allowlist only covers localhost origins, so a UI hosted on a different host
than the API is not a supported setup. On an internet-facing host, also
rate-limit `/api/v2/auth/login` at the reverse proxy.

## Upgrading

There is no published image; the image is always built from this repository.

```bash
git pull
docker compose build --pull
docker compose up -d
```

Database migrations run at startup, and the volumes survive the container being
replaced. Back up `/data` before a major version jump — it holds the database.

## Troubleshooting

**The desktop app is already on port 8765.** The Tauri build runs its own
backend on `127.0.0.1:8765`, so the container's published port collides with it.
Docker Desktop on Windows does not always report this: it starts the container,
binds nothing, and your browser quietly talks to the desktop app instead — the
API answers but `/` returns 404. Confirm with `docker port qunica` (empty output
means nothing was published) and publish a different host port, for example
`"127.0.0.1:18765:8765"`.

**404 on `/`, API works** — the UI assets are missing, or you are talking to a
different backend. Check `docker exec qunica curl -sI localhost:8765/` first: a
200 there means the container is fine and the host port is the problem, as
above. A 404 there means `/app/web` is empty and the image needs rebuilding.

**`Permission denied` writing to `/data`** — a bind mount whose host directory is
owned by another user, with the container started as a non-root `user:`. Either
drop the `user:` override so the entrypoint can fix ownership, or
`chown -R 10001:10001` the host directory.

**Health check failing** — `docker logs qunica`. The container reports healthy
once `GET /api/v2/health` returns 200.

## Building for another architecture

```bash
docker buildx build --platform linux/arm64 -t qunica:arm64 .
```

All build stages use multi-arch base images, so cross-building works
under emulation. Native runners are considerably faster for the Rust stage.
