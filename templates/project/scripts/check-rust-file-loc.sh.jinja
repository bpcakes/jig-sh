#!/usr/bin/env bash
set -euo pipefail

default_branch="${1:-${JIG_DEFAULT_BRANCH:-}}"
if [[ -z "$default_branch" ]]; then
  echo "Usage: scripts/check-rust-file-loc.sh <default-branch>" >&2
  exit 2
fi
if ! git check-ref-format --branch "$default_branch" >/dev/null 2>&1; then
  echo "Invalid default branch name: $default_branch" >&2
  exit 2
fi

remote_ref="origin/$default_branch"
if git rev-parse --verify "$remote_ref" >/dev/null 2>&1; then
  base_ref="$(git merge-base HEAD "$remote_ref")"
elif git rev-parse --verify HEAD^ >/dev/null 2>&1; then
  base_ref="HEAD^"
else
  base_ref="4b825dc642cb6eb9a060e54bf8d69288fbee4904"
fi

echo "Using Rust LOC base ref: $base_ref"
exec scripts/jig check rust-file-loc --changed-against "$base_ref"
