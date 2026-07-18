#!/usr/bin/env bash

if ! declare -F render_fixture_from_template >/dev/null; then
  source "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd -P)/lib.sh"
fi

validate_unpushed_commit_stays_local() {
  local bare_remote="$TMP_DIR/template-remote.git"
  local template_snapshot="$TMP_DIR/template-unpushed-snapshot"
  local template_clone="$TMP_DIR/template-clone"
  local answers_file="$TMP_DIR/template-backend.toml"
  local rendered_dir="$TMP_DIR/rendered-from-clone"

  create_template_snapshot_repo "$template_snapshot"
  git clone --bare --no-local "$template_snapshot" "$bare_remote" >/dev/null 2>&1
  git clone "$bare_remote" "$template_clone" >/dev/null 2>&1
  git -C "$template_clone" config user.name "Fixture"
  git -C "$template_clone" config user.email "fixture@example.com"

  cp "$ROOT_DIR/tests/fixtures/backend-only.toml" "$answers_file"
  answers_set "$answers_file" template_source_url ""

  cat > "$template_clone/UNPUSHED_MARKER.md" <<'EOF'
marker
EOF
  git -C "$template_clone" add UNPUSHED_MARKER.md
  git -C "$template_clone" commit -m "unpushed template change" >/dev/null

  render_fixture_from_template "$template_clone" "$answers_file" "$rendered_dir"

  actual_src_path="$(answers_get "$rendered_dir/.jig.toml" _src_path)"
  expected_src_path="$(cd "$template_clone" && pwd -P)"
  if [[ "$actual_src_path" != "$expected_src_path" ]]; then
    echo "Expected _src_path to stay local for an unpushed commit." >&2
    echo "Expected: $expected_src_path" >&2
    echo "Actual:   $actual_src_path" >&2
    exit 1
  fi
}

validate_explicit_template_source_url_rewrites_src_path() {
  local bare_remote="$TMP_DIR/template-explicit-ok.git"
  local template_snapshot="$TMP_DIR/template-explicit-ok-snapshot"
  local answers_file="$TMP_DIR/backend-explicit-ok.toml"
  local rendered_dir="$TMP_DIR/render-explicit-ok"

  create_template_snapshot_repo "$template_snapshot"
  git clone --bare --no-local "$template_snapshot" "$bare_remote" >/dev/null 2>&1

  cp "$ROOT_DIR/tests/fixtures/backend-only.toml" "$answers_file"
  answers_set "$answers_file" template_source_url "$bare_remote"

  render_fixture_from_template "$template_snapshot" "$answers_file" "$rendered_dir"

  actual_src_path="$(answers_get "$rendered_dir/.jig.toml" _src_path)"
  if [[ "$actual_src_path" != "$bare_remote" ]]; then
    echo "Expected explicit template_source_url to replace _src_path after validation." >&2
    echo "Expected: $bare_remote" >&2
    echo "Actual:   $actual_src_path" >&2
    exit 1
  fi
}

validate_quoted_local_src_path_installs_jig() {
  local template_snapshot="$TMP_DIR/template-quoted-local'source"
  local answers_file="$TMP_DIR/backend-quoted-local.toml"
  local rendered_dir="$TMP_DIR/render-quoted-local"
  local jig_version

  create_template_snapshot_repo "$template_snapshot"
  jig_version="$(answers_get "$template_snapshot/.jig.toml" jig_version)"

  cp "$ROOT_DIR/tests/fixtures/backend-only.toml" "$answers_file"
  answers_set "$answers_file" template_source_url ""

  render_fixture_from_template "$template_snapshot" "$answers_file" "$rendered_dir"

  actual_src_path="$(answers_get "$rendered_dir/.jig.toml" _src_path)"
  expected_src_path="$(cd "$template_snapshot" && pwd -P)"
  if [[ "$actual_src_path" != "$expected_src_path" ]]; then
    echo "Expected quoted local _src_path to round-trip through rendering." >&2
    echo "Expected: $expected_src_path" >&2
    echo "Actual:   $actual_src_path" >&2
    exit 1
  fi

  (
    cd "$rendered_dir"
    rm -rf .git .agent/.cache
    env -u JIG_DEV_BIN scripts/install-jig.sh >/dev/null
    [[ -x ".agent/.cache/jig/$jig_version/bin/jig" ]]
  )
}

validate_template_source_url_installs_from_git_tag() {
  local bare_remote="$TMP_DIR/template-git-install.git"
  local template_snapshot="$TMP_DIR/template-git-install-snapshot"
  local answers_file="$TMP_DIR/backend-git-install.toml"
  local rendered_dir="$TMP_DIR/render-git-install"
  local jig_version

  create_template_snapshot_repo "$template_snapshot"
  jig_version="$(answers_get "$template_snapshot/.jig.toml" jig_version)"
  git -C "$template_snapshot" tag -a "v$jig_version" -m "fixture release" >/dev/null
  git clone --bare --no-local "$template_snapshot" "$bare_remote" >/dev/null 2>&1

  cp "$ROOT_DIR/tests/fixtures/backend-only.toml" "$answers_file"
  answers_set "$answers_file" template_source_url "file://$bare_remote"

  render_fixture_from_template "$template_snapshot" "$answers_file" "$rendered_dir"

  actual_src_path="$(answers_get "$rendered_dir/.jig.toml" _src_path)"
  if [[ "$actual_src_path" != "file://$bare_remote" ]]; then
    echo "Expected template_source_url to be used as the generated _src_path." >&2
    echo "Expected: file://$bare_remote" >&2
    echo "Actual:   $actual_src_path" >&2
    exit 1
  fi

  (
    cd "$rendered_dir"
    rm -rf .git .agent/.cache
    env -u JIG_DEV_BIN CARGO_HOME="$TMP_DIR/cargo-home-git-install" scripts/install-jig.sh >/dev/null
    [[ -x ".agent/.cache/jig/$jig_version/bin/jig" ]]
    [[ "$(".agent/.cache/jig/$jig_version/bin/jig" --version)" == "jig $jig_version" ]]
  )
}

validate_quoted_template_source_url_rewrites_src_path() {
  local bare_remote="$TMP_DIR/template-quoted-remote'.git"
  local template_snapshot="$TMP_DIR/template-quoted-remote-snapshot"
  local answers_file="$TMP_DIR/backend-quoted-remote.toml"
  local rendered_dir="$TMP_DIR/render-quoted-remote"

  create_template_snapshot_repo "$template_snapshot"
  git clone --bare --no-local "$template_snapshot" "$bare_remote" >/dev/null 2>&1

  cp "$ROOT_DIR/tests/fixtures/backend-only.toml" "$answers_file"
  answers_set "$answers_file" template_source_url "$bare_remote"

  render_fixture_from_template "$template_snapshot" "$answers_file" "$rendered_dir"

  actual_src_path="$(answers_get "$rendered_dir/.jig.toml" _src_path)"
  if [[ "$actual_src_path" != "$bare_remote" ]]; then
    echo "Expected quoted template_source_url to replace _src_path after validation." >&2
    echo "Expected: $bare_remote" >&2
    echo "Actual:   $actual_src_path" >&2
    exit 1
  fi
}

if [[ "${BASH_SOURCE[0]}" == "$0" ]]; then
  set -euo pipefail
  fixture_create_tmp_dir_if_needed
  export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$TMP_DIR/cargo-target}"

  validate_unpushed_commit_stays_local
  validate_explicit_template_source_url_rewrites_src_path
  validate_quoted_local_src_path_installs_jig
  validate_template_source_url_installs_from_git_tag
  validate_quoted_template_source_url_rewrites_src_path

  echo "Template source fixture validation passed."
fi
