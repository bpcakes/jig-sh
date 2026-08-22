#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)"
cd "$ROOT_DIR"

content_pattern="Windows|win32|cygwin|msys|mingw|PATHEXT|ComSpec|PowerShell|pwsh|cmd\\.exe|windows-sys|windows_sys|x86_64-pc-windows|cfg\\([^)]*windows|target_os[^)]*windows|windows[_/-](dependency|launch|process|path|host|runner|ci|job|system|platform|support)|\\.exe([\"' ,)]|$)"
path_pattern='(^|[/_.-])(windows?|win32|powershell|pwsh|cygwin|msys|mingw)([/_.-]|$)|\.ps1$|\.exe$|x86_64-pc-windows'

content_matches="$(
  git grep -n -I -E "$content_pattern" -- \
    ':!Cargo.lock' \
    ':!landing/bun.lock' \
    ':!.agent/state/**' \
    ':!.agent/plans/plan_01M0N7D7WA7NZ1SZ6ZM1FCSV3D.md' \
    ':!.agent/plans/plan_01M0NDBD9J096MWGPBPG4N5GFD.md' \
    ':!scripts/check-supported-host-surface.sh' \
    || true
)"
if [[ -n "$content_matches" ]]; then
  echo "Tracked source still contains unsupported-host implementation or support guidance:" >&2
  printf '%s\n' "$content_matches" >&2
  exit 1
fi

path_matches="$(git ls-files | grep -Ei "$path_pattern" || true)"
if [[ -n "$path_matches" ]]; then
  echo "Tracked paths still contain unsupported-host artifacts:" >&2
  printf '%s\n' "$path_matches" >&2
  exit 1
fi
