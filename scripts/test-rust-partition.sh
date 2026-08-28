#!/usr/bin/env bash
set -euo pipefail

usage() {
  echo "Usage: scripts/test-rust-partition.sh core|frontend|vault|process" >&2
}

if [ "$#" -ne 1 ]; then
  usage
  exit 2
fi

frontend_filter='package(jig-sh) & (test(bootstrap::tests::frontend_adoption) | test(bootstrap::tests::basic::scaffold_generation) | test(bootstrap::tests::basic::scaffold_runtime))'
process_filter='(package(jig-sh) & (binary(codex_launcher) | binary(dev_lifecycle) | binary(dev_sigint))) | package(jig-owned-process) | (package(jig-dev-proxy) & test(processes))'
vault_filter='package(jig-vault) | package(jig-vault-tui) | (package(jig-sh) & (test(vault) | binary(/vault_.*/)))'
status_args=(--status-level fail --final-status-level fail)

case "$1" in
  core)
    exec cargo nextest run --workspace -P local \
      -E "not (($frontend_filter) | ($process_filter) | ($vault_filter))" \
      "${status_args[@]}"
    ;;
  frontend)
    exec cargo nextest run --workspace -P local -E "$frontend_filter" \
      "${status_args[@]}"
    ;;
  vault)
    cargo nextest run --workspace -P local \
      -E "($vault_filter) & not (package(jig-sh) & binary(vault_tui))" \
      "${status_args[@]}"
    exec cargo nextest run -P local -p jig-sh --test vault_tui -j 1 \
      "${status_args[@]}"
    ;;
  process)
    exec cargo nextest run --workspace -P local -E "$process_filter" \
      "${status_args[@]}"
    ;;
  *)
    usage
    exit 2
    ;;
esac
