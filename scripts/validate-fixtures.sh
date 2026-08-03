#!/usr/bin/env bash
set -Eeuo pipefail

report_fixture_failure() {
  local status=$?
  echo "Fixture validation failed at ${BASH_SOURCE[1]}:${BASH_LINENO[0]} while running: $BASH_COMMAND" >&2
  exit "$status"
}
trap report_fixture_failure ERR

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)"
TMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TMP_DIR"' EXIT

cargo build -p jig-sh --bin jig >/dev/null
export JIG_DEV_BIN="$ROOT_DIR/target/debug/jig"
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$TMP_DIR/cargo-target}"

source "$ROOT_DIR/scripts/fixtures/lib.sh"
source "$ROOT_DIR/scripts/fixtures/runtime-smoke.sh"
source "$ROOT_DIR/scripts/fixtures/stub-repos.sh"
source "$ROOT_DIR/scripts/fixtures/rendered-repos.sh"
source "$ROOT_DIR/scripts/fixtures/source-normalization.sh"

assert_posix_launcher() {
  local launcher="$1"
  sh -n "$launcher"
  if grep -Eq 'BASH_SOURCE|pipefail|(^|[[:space:]])\[\[|^[[:space:]]*local[[:space:]]|\+=|<<EOF' "$launcher"; then
    echo "$launcher contains Bash-only launcher syntax." >&2
    exit 1
  fi
}

BACKEND_DIR="$TMP_DIR/backend-only"
FULL_STACK_DIR="$TMP_DIR/full-stack"
TOOLING_ONLY_DIR="$TMP_DIR/tooling-only"
TEMPLATE_SNAPSHOT="$TMP_DIR/template-snapshot"
EXAMPLE_SMOKE_DIR="$TMP_DIR/example-smoke"

for answer_name in backend-only.toml full-stack.toml tooling-only.toml; do
  if ! cmp -s "$ROOT_DIR/examples/$answer_name" "$ROOT_DIR/tests/fixtures/$answer_name"; then
    echo "examples/$answer_name must match tests/fixtures/$answer_name." >&2
    exit 1
  fi
done

assert_posix_launcher "$ROOT_DIR/scripts/jig"

create_template_snapshot_repo "$TEMPLATE_SNAPSHOT"
mkdir -p "$EXAMPLE_SMOKE_DIR"
for answers_file in "$ROOT_DIR"/examples/*.toml; do
  answer_name="$(basename "$answers_file" .toml)"
  render_fixture_from_template "$TEMPLATE_SNAPSHOT" "$answers_file" "$EXAMPLE_SMOKE_DIR/$answer_name"
  test -f "$EXAMPLE_SMOKE_DIR/$answer_name/.jig.toml"
  test -f "$EXAMPLE_SMOKE_DIR/$answer_name/scripts/jig"
  assert_posix_launcher "$EXAMPLE_SMOKE_DIR/$answer_name/scripts/jig"
done

render_fixture_from_template "$TEMPLATE_SNAPSHOT" "$ROOT_DIR/tests/fixtures/backend-only.toml" "$BACKEND_DIR"
render_fixture_from_template "$TEMPLATE_SNAPSHOT" "$ROOT_DIR/tests/fixtures/full-stack.toml" "$FULL_STACK_DIR"
render_fixture_from_template "$TEMPLATE_SNAPSHOT" "$ROOT_DIR/tests/fixtures/tooling-only.toml" "$TOOLING_ONLY_DIR"

assert_posix_launcher "$BACKEND_DIR/scripts/jig"
assert_posix_launcher "$FULL_STACK_DIR/scripts/jig"
assert_posix_launcher "$TOOLING_ONLY_DIR/scripts/jig"

validate_backend_fixture "$BACKEND_DIR"
validate_full_stack_fixture "$FULL_STACK_DIR"
validate_tooling_only_fixture "$TOOLING_ONLY_DIR"
validate_unpushed_commit_stays_local
validate_explicit_template_source_url_rewrites_src_path
validate_quoted_local_src_path_installs_jig
validate_template_source_url_installs_from_git_tag
validate_quoted_template_source_url_rewrites_src_path

echo "Fixture validation passed."
