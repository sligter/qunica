#!/usr/bin/env bash
# Make the mounted volumes writable, then run the server as the unprivileged
# `qunica` user. Named volumes inherit the image's ownership, but bind mounts
# arrive owned by whoever created them on the host, so the chown is what makes
# `docker run -v ./data:/data` work without manual setup.
set -euo pipefail

APP_DATA="${QUNICA_APP_DATA:-/data}"
# Container-level only: the group workspace root is a per-account setting chosen
# during onboarding. This just guarantees the directory exists and is writable.
WORKSPACES="${QUNICA_WORKSPACES_DIR:-/workspaces}"

if [ "$(id -u)" = "0" ]; then
  mkdir -p "${APP_DATA}" "${WORKSPACES}"
  # /data is small and app-owned, so a deep chown is cheap. /workspaces can hold
  # large checkouts, and recursing over them would both stall startup and rewrite
  # ownership the host still needs, so only the mount point itself is adjusted.
  chown -R qunica:qunica "${APP_DATA}"
  chown qunica:qunica "${WORKSPACES}"
  exec gosu qunica "${BASH_SOURCE[0]}" "$@"
fi

mkdir -p "${APP_DATA}" "${WORKSPACES}" 2>/dev/null || true

exec "$@"
