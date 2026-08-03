#!/usr/bin/env bash

if ! declare -F render_fixture >/dev/null; then
  source "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd -P)/lib.sh"
fi
if ! declare -F validate_jig_runtime >/dev/null; then
  source "$ROOT_DIR/scripts/fixtures/runtime-smoke.sh"
fi
if ! declare -F write_backend_stub_repo >/dev/null; then
  source "$ROOT_DIR/scripts/fixtures/stub-repos.sh"
fi

settle_fixture_cargo_workspace() {
  # Keep the first structured work check from being invalidated by Cargo settling the repo.
  cargo generate-lockfile >/dev/null
}

configure_fixture_sqlx_boundary() {
  local repo_dir="$1"

  export PATH="$repo_dir/.agent/tmp/fixture-bin:$PATH"
  export JIG_FIXTURE_SQLX_MARKER="$repo_dir/.agent/tmp/fixture-sqlx-invocation"
  rm -f "$JIG_FIXTURE_SQLX_MARKER"
}

assert_fixture_sqlx_boundary() {
  local repo_dir="$1"
  local marker="$repo_dir/.agent/tmp/fixture-sqlx-invocation"

  [[ -f "$marker" ]]
  [[ "$(cat "$marker")" == $'CARGO=cargo\nSQLX_OFFLINE=false\nSQLX_OFFLINE_DIR=.sqlx\nargv=prepare --check --workspace -- --workspace --all-targets' ]]
}

assert_fixture_text_absent() {
  local pattern="$1"
  local path="$2"
  local status

  if grep -F -q -- "$pattern" "$path"; then
    echo "Unexpected fixture content matching '$pattern' in $path." >&2
    return 1
  else
    status=$?
  fi
  # grep uses 1 for a successful search with no matches. Any other failure
  # means the assertion could not inspect its input and must remain visible.
  [[ "$status" -eq 1 ]] && return 0
  return "$status"
}

validate_fixture_text_absence_assertion() {
  local repo_dir="$1"
  local control="$repo_dir/.agent/tmp/text-absence-control"
  local status

  mkdir -p "$(dirname "$control")"
  printf '%s\n' 'forbidden-control-marker' > "$control"
  if assert_fixture_text_absent 'forbidden-control-marker' "$control" 2>/dev/null; then
    echo "Fixture absence assertion accepted a forbidden match." >&2
    return 1
  else
    status=$?
  fi
  [[ "$status" -eq 1 ]] || return "$status"

  printf '%s\n' '"jig.sqlx_check"' > "$control"
  if assert_fixture_text_absent '"jig.sqlx_check"' "$control" 2>/dev/null; then
    echo "Fixture fixed-string absence assertion accepted a dotted forbidden match." >&2
    return 1
  else
    status=$?
  fi
  [[ "$status" -eq 1 ]] || return "$status"

  if assert_fixture_text_absent 'forbidden-control-marker' "${control}.missing" 2>/dev/null; then
    echo "Fixture absence assertion accepted an unreadable input." >&2
    return 1
  else
    status=$?
  fi
  [[ "$status" -eq 2 ]] || {
    echo "Fixture absence assertion did not preserve grep's input error status." >&2
    return 1
  }
  rm -f "$control"
}

validate_backend_fixture() {
  local repo_dir="$1"
  local expected_jig_version

  [[ ! -f "$repo_dir/Cargo.toml" ]]
  [[ ! -f "$repo_dir/package.json" ]]
  [[ ! -d "$repo_dir/apps" ]]
  [[ ! -d "$repo_dir/crates" ]]
  write_backend_stub_repo "$repo_dir"
  expected_jig_version="$(answers_get "$repo_dir/.jig.toml" jig_version)"
  (
    cd "$repo_dir"
    configure_fixture_sqlx_boundary "$repo_dir"
    [[ -f .jig.toml ]]
    git init -b main >/dev/null
    git config user.name "Fixture"
    git config user.email "fixture@example.com"
    settle_fixture_cargo_workspace
    scripts/jig agent-map generate >/dev/null
    git add .
    git commit -m "fixture" >/dev/null
    [[ ! -f Makefile ]]
    scripts/jig check agent-map >/dev/null
    scripts/jig check agent-guides >/dev/null
    scripts/jig check rust-file-loc --all >/dev/null
    scripts/jig check migration-immutability --changed-against HEAD >/dev/null
    scripts/jig check sqlx-unchecked-non-test >/dev/null
    [[ ! -f scripts/enforce-coverage.cjs ]]
    perl -0pi -e 's/default_branch = "main"/default_branch = "dev"/' .jig.toml
    git add .jig.toml
    git commit -m "change answers" >/dev/null
    scripts/jig update --recopy --force >/dev/null
    [[ ! -f Makefile ]]
    grep -q '^default_branch = "dev"$' .jig.toml
    grep -Fqx "jig_version = \"$expected_jig_version\"" .jig.toml
    if [[ -f .github/workflows/webapp-checks.yml ]]; then
      grep -q "No web apps configured" .github/workflows/webapp-checks.yml
    fi
    validate_jig_runtime "$repo_dir" 0 1 "fixture_backend_runtime"
    assert_fixture_sqlx_boundary "$repo_dir"
  )
}

validate_full_stack_fixture() {
  local repo_dir="$1"

  [[ ! -f "$repo_dir/Cargo.toml" ]]
  [[ ! -f "$repo_dir/package.json" ]]
  [[ ! -d "$repo_dir/apps" ]]
  [[ ! -d "$repo_dir/crates" ]]
  [[ ! -d "$repo_dir/frontend" ]]
  [[ ! -d "$repo_dir/admin-panel" ]]
  write_full_stack_stub_repo "$repo_dir"
  (
    cd "$repo_dir"
    configure_fixture_sqlx_boundary "$repo_dir"
    [[ -f .jig.toml ]]
    git init -b main >/dev/null
    git config user.name "Fixture"
    git config user.email "fixture@example.com"
    settle_fixture_cargo_workspace
    scripts/jig agent-map generate >/dev/null
    git add .
    git commit -m "fixture" >/dev/null
    [[ ! -f Makefile ]]
    scripts/jig check agent-map >/dev/null
    scripts/jig check agent-guides >/dev/null
    scripts/jig check rust-file-loc --all >/dev/null
    scripts/jig check migration-immutability --changed-against HEAD >/dev/null
    scripts/jig check sqlx-unchecked-non-test >/dev/null
    scripts/jig check schema >/dev/null
    scripts/jig update --recopy --force >/dev/null
    grep -q "frontend" .github/workflows/webapp-checks.yml
    grep -q "admin-panel" .github/workflows/webapp-checks.yml
    grep -q "40" .github/workflows/webapp-checks.yml
    validate_jig_runtime "$repo_dir" 1 1 "fixture_full_stack_runtime" 1
    assert_fixture_sqlx_boundary "$repo_dir"
  )
}

validate_tooling_only_fixture() {
  local repo_dir="$1"

  [[ ! -f "$repo_dir/Cargo.toml" ]]
  [[ ! -f "$repo_dir/package.json" ]]
  [[ ! -d "$repo_dir/apps" ]]
  [[ ! -d "$repo_dir/crates" ]]
  write_tooling_only_stub_repo "$repo_dir"
  (
    cd "$repo_dir"
    validate_fixture_text_absence_assertion "$repo_dir"
    [[ -f .jig.toml ]]
    git init -b main >/dev/null
    git config user.name "Fixture"
    git config user.email "fixture@example.com"
    settle_fixture_cargo_workspace
    scripts/jig agent-map generate >/dev/null
    git add .
    git commit -m "fixture" >/dev/null
    [[ ! -f Makefile ]]
    scripts/jig check agent-map >/dev/null
    scripts/jig check agent-guides >/dev/null
    scripts/jig check rust-file-loc --all >/dev/null
    [[ ! -f scripts/enforce-coverage.cjs ]]
    [[ ! -f scripts/add-migration.sh ]]
    [[ ! -f scripts/check-migration-immutability.sh ]]
    [[ ! -f scripts/check-schema-dump.sh ]]
    [[ ! -f scripts/check-sqlx-unchecked-non-test.sh ]]
    [[ ! -f scripts/generate-sqlx-unchecked-queries-todo.sh ]]
    [[ ! -f Makefile ]]
    assert_fixture_text_absent '"jig.sqlx_check"' .agent/jig-contract.json
    assert_fixture_text_absent '"jig.schema_check"' .agent/jig-contract.json
    assert_fixture_text_absent '"jig.schema_dump"' .agent/jig-contract.json
    assert_fixture_text_absent '"jig.migration_add"' .agent/jig-contract.json
    assert_fixture_text_absent 'sqlx-unchecked-queries:' .github/workflows/repo-policy.yml
    assert_fixture_text_absent 'migration-immutability:' .github/workflows/repo-policy.yml
    perl -0pi -e 's/default_branch = "main"/default_branch = "dev"/' .jig.toml
    git add .jig.toml
    git commit -m "change answers" >/dev/null
    scripts/jig update --recopy --force >/dev/null
    [[ ! -f Makefile ]]
    grep -q '^default_branch = "dev"$' .jig.toml
    [[ ! -f scripts/add-migration.sh ]]
    [[ ! -f scripts/check-migration-immutability.sh ]]
    [[ ! -f scripts/check-schema-dump.sh ]]
    [[ ! -f scripts/check-sqlx-unchecked-non-test.sh ]]
    [[ ! -f scripts/generate-sqlx-unchecked-queries-todo.sh ]]
    validate_jig_runtime "$repo_dir" 0 0
  )
}

if [[ "${BASH_SOURCE[0]}" == "$0" ]]; then
  set -euo pipefail
  fixture_create_tmp_dir_if_needed

  backend_dir="$TMP_DIR/backend-only"
  full_stack_dir="$TMP_DIR/full-stack"
  tooling_only_dir="$TMP_DIR/tooling-only"
  template_snapshot="$TMP_DIR/template-snapshot"

  create_template_snapshot_repo "$template_snapshot"
  render_fixture_from_template "$template_snapshot" "$ROOT_DIR/tests/fixtures/backend-only.toml" "$backend_dir"
  render_fixture_from_template "$template_snapshot" "$ROOT_DIR/tests/fixtures/full-stack.toml" "$full_stack_dir"
  render_fixture_from_template "$template_snapshot" "$ROOT_DIR/tests/fixtures/tooling-only.toml" "$tooling_only_dir"

  validate_backend_fixture "$backend_dir"
  validate_full_stack_fixture "$full_stack_dir"
  validate_tooling_only_fixture "$tooling_only_dir"

  echo "Rendered fixture validation passed."
fi
