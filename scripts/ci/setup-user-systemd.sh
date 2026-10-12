#!/usr/bin/env bash
set -euo pipefail

# Ephemeral Linux CI VMs may start the runner without a login session. Proxy
# lifecycle tests need the real user manager to distinguish an absent service
# from an unreachable manager before stopping their background proxy.
: "${GITHUB_ENV:?This setup script is for GitHub Actions runners}"
runner_uid="$(id -u)"
sudo loginctl enable-linger "$(id -un)"
sudo systemctl start "user@$runner_uid.service"
export XDG_RUNTIME_DIR="/run/user/$runner_uid"
export DBUS_SESSION_BUS_ADDRESS="unix:path=$XDG_RUNTIME_DIR/bus"
systemctl --user show-environment >/dev/null
printf '%s\n' "XDG_RUNTIME_DIR=$XDG_RUNTIME_DIR" \
  "DBUS_SESSION_BUS_ADDRESS=$DBUS_SESSION_BUS_ADDRESS" >> "$GITHUB_ENV"
