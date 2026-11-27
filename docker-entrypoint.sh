#!/bin/sh
# Railway mounts volumes owned by root, and this image runs as the unprivileged `gm` user.
# With RAILWAY_RUN_UID=0 the container starts as root: hand the data directory to `gm`, then
# run the server as `gm`. The server itself never runs as root.
set -eu
if [ "$(id -u)" = "0" ]; then
  if [ -n "${GM_DATA_DIR:-}" ]; then
    mkdir -p "$GM_DATA_DIR"
    chown -R gm:gm "$GM_DATA_DIR"
  fi
  exec setpriv --reuid=gm --regid=gm --init-groups gm-api "$@"
fi
exec gm-api "$@"
