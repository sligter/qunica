# syntax=docker/dockerfile:1.7
#
# Qunica server image: the axum backend serving the built web UI on one origin.
#
#   docker build -t qunica:local .
#   docker run -p 8765:8765 -v qunica-data:/data -v qunica-workspaces:/workspaces qunica:local
#
# See DOCKER.md for volumes, environment, and first-run setup.

ARG NODE_VERSION=22
ARG RUST_VERSION=1.88
ARG GO_VERSION=1.27
ARG UV_VERSION=0.12.9
ARG DOCKER_VERSION=29

# ---------------------------------------------------------------- web assets
FROM node:${NODE_VERSION}-bookworm-slim AS web
WORKDIR /src
ENV CI=1 \
    COREPACK_ENABLE_DOWNLOAD_PROMPT=0
# Upgrade corepack before enabling it. The pnpm version itself still comes from
# `packageManager` in package.json.
RUN npm install --global corepack@latest && corepack enable

# Manifests first so a source-only edit reuses the install layer.
COPY package.json pnpm-lock.yaml pnpm-workspace.yaml ./
COPY frontend/package.json frontend/package.json
RUN --mount=type=cache,id=qunica-pnpm,target=/pnpm-store \
    pnpm config set store-dir /pnpm-store \
    && pnpm install --frozen-lockfile

COPY frontend/ frontend/
# VITE_API_BASE_URL stays unset on purpose: the bundle then calls the API with
# same-origin relative paths, which is exactly how this image serves it.
RUN pnpm --filter @qunica/frontend build

# -------------------------------------------------------------- server binary
FROM rust:${RUST_VERSION}-bookworm AS server
WORKDIR /src
# libssl-dev for reqwest's native-tls; sqlite is compiled in by libsqlite3-sys.
RUN apt-get update \
    && apt-get install -y --no-install-recommends pkg-config libssl-dev \
    && rm -rf /var/lib/apt/lists/*

COPY backend-rs/ backend-rs/
ENV CARGO_TARGET_DIR=/build-target
# The binary is copied out inside the same RUN because a cache mount does not
# survive into the resulting layer.
RUN --mount=type=cache,id=qunica-cargo-registry,target=/usr/local/cargo/registry \
    --mount=type=cache,id=qunica-cargo-target,target=/build-target \
    cargo build --manifest-path backend-rs/Cargo.toml --package qunica-backend --release \
    && cp /build-target/release/qunica-backend /usr/local/bin/qunica-server

# ---------------------------------------------------------- runtime toolchains
FROM golang:${GO_VERSION}-bookworm AS go-dist
FROM ghcr.io/astral-sh/uv:${UV_VERSION} AS uv-dist
FROM docker:${DOCKER_VERSION}-cli AS docker-dist

FROM ubuntu:24.04 AS runtime

# The compiler and headers make the bundled language toolchains useful for
# native dependencies, not merely present enough to print a version number.
RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        bash \
        build-essential \
        ca-certificates \
        curl \
        git \
        gosu \
        less \
        libssl-dev \
        openssh-client \
        pkg-config \
        procps \
        python-is-python3 \
        python3.12 \
        python3.12-venv \
        tini \
    && rm -rf /var/lib/apt/lists/*

# Node powers the external ACP runtimes (Codex, Claude Code, OpenCode, ...),
# which the Agents page installs and launches through npm/npx.
COPY --from=web /usr/local/bin/node /usr/local/bin/node
COPY --from=web /usr/local/lib/node_modules /usr/local/lib/node_modules
RUN ln -s ../lib/node_modules/npm/bin/npm-cli.js /usr/local/bin/npm \
    && ln -s ../lib/node_modules/npm/bin/npx-cli.js /usr/local/bin/npx \
    && ln -s ../lib/node_modules/corepack/dist/corepack.js /usr/local/bin/corepack

COPY --from=go-dist /usr/local/go /usr/local/go
COPY --from=server /usr/local/cargo/bin /usr/local/cargo/bin
COPY --from=server --chown=10001:10001 /usr/local/rustup /usr/local/rustup
COPY --from=uv-dist /uv /uvx /usr/local/bin/
COPY --from=docker-dist /usr/local/bin/docker /usr/local/bin/docker
COPY --from=docker-dist /usr/local/libexec/docker/cli-plugins /usr/local/libexec/docker/cli-plugins

RUN groupadd --gid 10001 qunica \
    && useradd --uid 10001 --gid 10001 --create-home --shell /bin/bash qunica \
    && mkdir -p /data /workspaces /home/qunica/.npm-global \
    && chown qunica:qunica /data /workspaces \
    && chown -R qunica:qunica /home/qunica

# Bind-mounted repositories are owned by the host user, not by `qunica`, so
# without this every git call fails with "detected dubious ownership". A commit
# identity has to exist too; override it per deployment (see DOCKER.md).
RUN git config --system --add safe.directory '*' \
    && git config --system user.name "Qunica" \
    && git config --system user.email "qunica@localhost" \
    && git config --system init.defaultBranch main

COPY --from=server /usr/local/bin/qunica-server /usr/local/bin/qunica-server
COPY --from=web /src/frontend/dist /app/web
COPY docker/entrypoint.sh /usr/local/bin/qunica-entrypoint
RUN chmod +x /usr/local/bin/qunica-entrypoint

ENV QUNICA_HOST=0.0.0.0 \
    QUNICA_PORT=8765 \
    QUNICA_APP_DATA=/data \
    QUNICA_WEB_DIR=/app/web \
    QUNICA_LOG_LEVEL=info \
    QUNICA_WORKSPACES_DIR=/workspaces \
    NPM_CONFIG_UPDATE_NOTIFIER=false

# /usr/local is root-owned, so `npm install -g` from the Agents page would fail
# with EACCES. Global packages go under the qunica home instead.
ENV NPM_CONFIG_PREFIX=/home/qunica/.npm-global
ENV RUSTUP_HOME=/usr/local/rustup
ENV PATH=/home/qunica/.npm-global/bin:/home/qunica/.local/bin:/home/qunica/.cargo/bin:/home/qunica/go/bin:/usr/local/go/bin:/usr/local/cargo/bin:${PATH}

# Fail the image build if any promised tool is missing or the two requested
# language baselines are not met. Run as the same non-root user agents use.
RUN gosu qunica node -e "if (+process.versions.node.split('.')[0] < 22) process.exit(1)" \
    && gosu qunica npm --version \
    && gosu qunica git --version \
    && gosu qunica python3.12 -c "import sys; assert sys.version_info[:2] == (3, 12)" \
    && gosu qunica uv --version \
    && gosu qunica go version \
    && gosu qunica rustc --version \
    && gosu qunica cargo --version \
    && gosu qunica docker --version \
    && gosu qunica docker compose version \
    && gosu qunica docker buildx version

# /data holds the SQLite database, the generated secret key, skills and logs.
# /workspaces is where onboarding should point the group workspace root.
VOLUME ["/data", "/workspaces"]
WORKDIR /workspaces
EXPOSE 8765

HEALTHCHECK --interval=30s --timeout=5s --start-period=20s --retries=3 \
    CMD curl -fsS "http://127.0.0.1:${QUNICA_PORT}/api/v2/health" || exit 1

ENTRYPOINT ["/usr/bin/tini", "--", "/usr/local/bin/qunica-entrypoint"]
CMD ["qunica-server"]
