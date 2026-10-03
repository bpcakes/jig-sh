#!/usr/bin/env bash
# Shared by release.sh and its focused validation-reuse regression tests.

require_clean_tree() {
  local status

  if [[ "${ALLOW_DIRTY:-}" == "1" ]]; then
    echo "ALLOW_DIRTY=1 set; skipping clean working tree requirement." >&2
    return 0
  fi

  status="$(git status --short --untracked-files=all)" || return 1
  if [[ "${ALLOW_RELEASE_RUN_JOURNAL_DIRTY:-}" == "1" && "$status" == " M .agent/state/runs.jsonl" ]]; then
    echo "ALLOW_RELEASE_RUN_JOURNAL_DIRTY=1 set; allowing the ephemeral release-check run journal." >&2
    return 0
  fi

  if [[ -n "$status" ]]; then
    echo "Working tree is not clean. Commit or discard changes before releasing." >&2
    printf '%s\n' "$status" >&2
    exit 1
  fi
}

release_validation_identity() {
  local version="$1"
  local commit
  if [[ "${ALLOW_DIRTY:-}" == "1" ]]; then
    echo "Release validation reuse requires a committed, clean tree." >&2
    return 1
  fi
  : "${GITHUB_RUN_ID:?Release validation reuse requires a GitHub Actions run}"
  : "${GITHUB_RUN_ATTEMPT:?Release validation reuse requires a run attempt}"
  : "${GITHUB_JOB:?Release validation reuse requires a job}"
  require_clean_tree || return 1
  commit="$(git rev-parse --verify HEAD)" || return 1
  printf '%s\n' "jig-release-validation-v1" "$version" "$commit" \
    "$GITHUB_RUN_ID" "$GITHUB_RUN_ATTEMPT" "$GITHUB_JOB"
}

release_check() {
  local version="$1"
  local identity current_identity
  if [[ -z "${RELEASE_VALIDATION_RECEIPT:-}" ]]; then
    run_release_checks "$version"
    return
  fi

  # A failed repeat check must never leave an earlier success reusable.
  rm -f -- "$RELEASE_VALIDATION_RECEIPT"
  identity="$(release_validation_identity "$version")" || return 1
  run_release_checks "$version"
  current_identity="$(release_validation_identity "$version")" || return 1
  if [[ "$current_identity" != "$identity" ]]; then
    echo "Release commit changed during validation; refusing to record success." >&2
    return 1
  fi
  printf '%s\n' "$identity" > "$RELEASE_VALIDATION_RECEIPT"
}

require_release_validation() {
  local version="$1"
  local identity
  if [[ -z "${RELEASE_VALIDATION_RECEIPT:-}" ]]; then
    release_check "$version"
    return
  fi

  identity="$(release_validation_identity "$version")" || return 1
  if [[ ! -f "$RELEASE_VALIDATION_RECEIPT" ]] || \
     [[ "$(cat "$RELEASE_VALIDATION_RECEIPT")" != "$identity" ]]; then
    echo "Missing or stale release validation; run scripts/release.sh check for this commit first." >&2
    return 1
  fi
  echo "Reusing successful release validation for v$version at $(git rev-parse --short HEAD)."
}
