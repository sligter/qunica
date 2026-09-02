# Running Qunica in Docker

The image runs the Qunica server: one process that serves both the REST API and
the built web UI on a single port. Point a browser at it and you get the same
app the desktop build shows, minus the desktop-only terminal and native dialogs.

> **Agents in this app run shell commands and edit files inside the container.**
> Treat the container as a machine you have handed to the agents. Do not publish
> the port on a public interface without a reverse proxy that terminates TLS and
> authenticates callers.

## Quick start

```bash
docker compose up -d --build
```

Open <http://127.0.0.1:18765>, create the local account, then finish onboarding:

1. **Workspace root** — set it to `/workspaces`. This is the container path the
   compose file keeps on a volume; anything else is lost on the next `up`.
2. **Model provider** — endpoint, model, API key.
3. **Default model** for the built-in assistant.

Without compose:

```bash
docker build -t qunica:local .
docker run -d --name qunica \
  -p 127.0.0.1:18765:8765 \
  -v qunica-data:/data \
  -v qunica-workspaces:/workspaces \
  qunica:local
```

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
| `/home/qunica` | optional; npm-installed ACP runtimes and their sign-in state | external CLI agents must be installed and signed in again |

Named volumes come out of the image with the right ownership. Bind mounts do
not, so the entrypoint chowns `/data` and `/workspaces` to the container's
`qunica` user (uid 10001) when it starts as root. If you run the container with
`user:` set to something else, chown the host directories yourself first.

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
| `QUNICA_WORKSPACES_DIR` | `/workspaces` | entrypoint only — it creates and chowns this path. The workspace root the app uses is the one you pick in onboarding. |

## Git

The image sets a system-wide commit identity (`Qunica <qunica@localhost>`) so
the workspace Git panel can commit at all, and marks every directory as a safe
directory so bind-mounted repositories owned by a host user still work. Override
the identity per deployment with `GIT_AUTHOR_NAME`, `GIT_AUTHOR_EMAIL`,
`GIT_COMMITTER_NAME`, and `GIT_COMMITTER_EMAIL` — the compose file shows where.

Pushing to a remote needs credentials you supply: mount an SSH key at
`/home/qunica/.ssh` or a credential file the remote accepts.

## External CLI agents

Node 20, npm, and npx are in the image, so the Agents page can install and run
ACP runtimes such as Codex, Claude Code, and OpenCode. Their **accounts are not**
part of the image. Sign in inside the container, or mount the runtime's config
directory from a host that is already signed in:

```yaml
volumes:
  - ~/.claude:/home/qunica/.claude
  - ~/.codex:/home/qunica/.codex
```

Global npm packages install under `/home/qunica/.npm-global`, which is on
`PATH`. That path is in the container filesystem, so installed runtimes are gone
after a rebuild. To keep them, mount a volume at `/home/qunica` — that also
persists each runtime's own config and its npx cache — or bake the installs into
your own image built `FROM qunica:local`.

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

Both build stages are plain multi-arch base images, so cross-building works
under emulation. Native runners are considerably faster for the Rust stage.
