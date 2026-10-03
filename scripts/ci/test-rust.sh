#!/usr/bin/env bash
# Compile once, then run isolated test phases from that exact set of binaries.
set -euo pipefail

cd "$(dirname "$0")/../.."
mode="${1:-workspace}"
profile=ci
case "$mode" in
  workspace) build_args=(--workspace); feature_args=() ;;
  minimal|minimal-focused) build_args=(-p jig-sh --no-default-features); feature_args=(--no-default-features) ;;
  *) echo "Usage: scripts/ci/test-rust.sh [workspace|minimal|minimal-focused]" >&2; exit 2 ;;
esac
if [ "$mode" = minimal-focused ]; then profile=minimal-ci; fi

metadata_dir="$(mktemp -d "${TMPDIR:-/tmp}/jig-test-build.XXXXXX")"
trap 'rm -rf "$metadata_dir"' EXIT
# Expanding an empty array requires this form on macOS's Bash 3.2.
cargo metadata --format-version 1 --locked ${feature_args[@]+"${feature_args[@]}"} > "$metadata_dir/cargo.json"
cargo nextest list --locked "${build_args[@]}" \
  --list-type binaries-only --message-format json > "$metadata_dir/binaries.json"
target_dir="$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["target_directory"])' "$metadata_dir/cargo.json")"
report_dir="${JIG_TEST_REPORT_DIR:-$PWD/.agent/.cache/test-reports/$mode}"
mkdir -p "$report_dir"
reuse_args=(--cargo-metadata "$metadata_dir/cargo.json" --binaries-metadata "$metadata_dir/binaries.json")
vault_filter='package(jig-vault) | package(jig-vault-tui) | (package(jig-sh) & (test(vault) | binary(/vault_.*/)))'
pty_filter='package(jig-sh) & binary(vault_tui)'

run_phase() {
  local name="$1" result=0
  shift
  rm -f "$target_dir/nextest/$profile/junit.xml" "$report_dir/$name.xml"
  cargo nextest run "${reuse_args[@]}" -P "$profile" \
    --status-level fail --final-status-level fail "$@" || result=$?
  if [ -f "$target_dir/nextest/$profile/junit.xml" ]; then
    cp "$target_dir/nextest/$profile/junit.xml" "$report_dir/$name.xml"
  fi
  return "$result"
}

if [ "$mode" = minimal-focused ]; then
  run_phase compatibility
  exit
fi

status=0
run_phase non-vault -E "not ($vault_filter)" || status=$?
run_phase vault -E "($vault_filter) & not ($pty_filter)" || status=$?
run_phase vault-pty -E "$pty_filter" -j 1 || status=$?
exit "$status"
