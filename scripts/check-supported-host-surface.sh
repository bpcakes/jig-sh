#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)"
cd "$ROOT_DIR"

content_pattern="Windows|win32|cygwin|msys|mingw|PATHEXT|ComSpec|PowerShell|pwsh|cmd\\.exe|windows-sys|windows_sys|x86_64-pc-windows|cfg\\([^)]*windows|target_os[^)]*windows|windows[_/-](dependency|launch|process|path|host|runner|ci|job|system|platform|support)|\\.exe([\"' ,)]|$)"
path_pattern='(^|[/_.-])(windows|win32|powershell|pwsh|cygwin|msys|mingw)([/_.-]|$)|\.ps1$|\.exe$|x86_64-pc-windows'

content_matches=""
if content_matches="$(
  # Release notes and append-only agent records are historical evidence, not
  # statements of the currently supported host surface.
  git grep -n -I -E "$content_pattern" -- \
    ':!Cargo.lock' \
    ':!landing/bun.lock' \
    ':!CHANGELOG.md' \
    ':!.agent/state/**' \
    ':!.agent/plans/**' \
    ':!scripts/check-supported-host-surface.sh'
)"; then
  :
else
  status=$?
  if [[ "$status" -ne 1 ]]; then
    echo "Failed to inspect tracked source for unsupported-host content (git grep exited $status)." >&2
    exit "$status"
  fi
fi
if [[ -n "$content_matches" ]]; then
  echo "Tracked current source or guidance still contains unsupported-host implementation or claims:" >&2
  printf '%s\n' "$content_matches" >&2
  exit 1
fi

tracked_paths=""
if tracked_paths="$(git ls-files)"; then
  :
else
  status=$?
  echo "Failed to inventory tracked paths for unsupported-host artifacts (git ls-files exited $status)." >&2
  exit "$status"
fi

path_matches=""
if path_matches="$(printf '%s\n' "$tracked_paths" | grep -Ei "$path_pattern")"; then
  :
else
  status=$?
  if [[ "$status" -ne 1 ]]; then
    echo "Failed to filter tracked paths for unsupported-host artifacts (grep exited $status)." >&2
    exit "$status"
  fi
fi
if [[ -n "$path_matches" ]]; then
  echo "Tracked paths still contain unsupported-host artifacts:" >&2
  printf '%s\n' "$path_matches" >&2
  exit 1
fi
