#!/usr/bin/env bash

if ! declare -F render_fixture_from_template >/dev/null; then
  source "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd -P)/lib.sh"
fi

write_fake_cargo_installer() {
  local bin_dir="$1"

  mkdir -p "$bin_dir"
  printf '%s\n' \
    '#!/usr/bin/env bash' \
    'set -euo pipefail' \
    'printf "%s\n" "$@" >"$JIG_FIXTURE_CARGO_LOG"' \
    'install_root=""' \
    'while [[ "$#" -gt 0 ]]; do' \
    '  if [[ "$1" == "--root" && "$#" -ge 2 ]]; then install_root="$2"; shift 2; else shift; fi' \
    'done' \
    '[[ -n "$install_root" ]]' \
    'if [[ -n "${JIG_FIXTURE_MUTATE_SOURCE_PATH:-}" ]]; then' \
    '  printf "%s\n" "// changed while cargo install was running" >>"$JIG_FIXTURE_MUTATE_SOURCE_PATH"' \
    'fi' \
    'mkdir -p "$install_root/bin"' \
    'printf "%s\n" "#!/bin/sh" "if [ \"\${1:-}\" = \"__runtime-compatible\" ]; then exit 0; fi" "if [ \"\${1:-}\" = \"--version\" ]; then printf \"%s\\n\" \"jig 99.0.0\"; exit 0; fi" "while [ \"\$#\" -ge 2 ]; do case \"\$1\" in --__launcher-contract-version|--__launcher-profile|--__launcher-repo-root) shift 2 ;; *) break ;; esac; done" "if [ \"\${1:-}\" = \"--help\" ] || [ \"\${1:-}\" = \"mcp\" ] || [ \"\${1:-}\" = \"doctor\" ]; then exit 0; fi" "exit 99" >"$install_root/bin/jig"' \
    'chmod +x "$install_root/bin/jig"' \
    >"$bin_dir/cargo"
  chmod +x "$bin_dir/cargo"
}

write_fake_pre_probe_cargo_installer() {
  local bin_dir="$1"

  mkdir -p "$bin_dir"
  printf '%s\n' \
    '#!/usr/bin/env bash' \
    'set -euo pipefail' \
    'printf "%s\n" "$@" >"$JIG_FIXTURE_CARGO_LOG"' \
    'install_root=""' \
    'while [[ "$#" -gt 0 ]]; do' \
    '  if [[ "$1" == "--root" && "$#" -ge 2 ]]; then install_root="$2"; shift 2; else shift; fi' \
    'done' \
    '[[ -n "$install_root" ]]' \
    'mkdir -p "$install_root/bin"' \
    'printf "%s\n" "#!/bin/sh" "if [ \"\${1:-}\" = \"--version\" ]; then printf \"%s\\n\" \"jig 0.2.0\"; exit 0; fi" "exit 2" >"$install_root/bin/jig"' \
    'chmod +x "$install_root/bin/jig"' \
    >"$bin_dir/cargo"
  chmod +x "$bin_dir/cargo"
}

validate_gnu_stat_fallback_rejects_successful_malformed_bsd_output() {
  local fake_bin_dir="$TMP_DIR/gnu-stat-bin"
  local function_source
  local identity

  mkdir -p "$fake_bin_dir"
  printf '%s\n' \
    '#!/bin/sh' \
    'if [ "$1" = "-f" ]; then' \
    '  printf "%s\n" "?:?m:?c:filesystem-id"' \
    '  exit 0' \
    'fi' \
    'if [ "$1" = "-c" ]; then' \
    '  printf "%s\n" "10:20:30:40:2026-08-06 12:00:00.123 +0000:2026-08-06 12:00:00.456 +0000"' \
    '  exit 0' \
    'fi' \
    'exit 2' \
    >"$fake_bin_dir/stat"
  chmod +x "$fake_bin_dir/stat"

  function_source="$(awk '
    /^binary_file_identity\(\) \{/ { capture = 1 }
    capture { print }
    capture && /^}$/ { exit }
  ' "$ROOT_DIR/scripts/install-jig.sh")"
  identity="$({
    BINARY_IDENTITY_FUNCTION="$function_source" PATH="$fake_bin_dir:$PATH" \
      /bin/bash -c 'eval "$BINARY_IDENTITY_FUNCTION"; binary_file_identity /fixture/jig'
  })"
  [[ "$identity" == "10:20:30:40:2026-08-06T12:00:00.123T+0000:2026-08-06T12:00:00.456T+0000" ]]
}

validate_mutable_source_reminder_requires_matching_cache_lock() {
  local source_cache="$TMP_DIR/mutable-source-cache"
  local active_cache="$TMP_DIR/mutable-runtime-cache"
  local stderr_file="$TMP_DIR/mutable-source-reminder-stderr"
  local function_source

  mkdir -p "$source_cache" "$active_cache"
  function_source="$(awk '
    /^(mark_mutable_source_refresh_reminder|record_mutable_source_refresh_reminder|warn_for_mutable_source_cache_if_due)\(\) \{/ { capture = 1 }
    capture { print }
    capture && /^}$/ { capture = 0 }
  ' "$ROOT_DIR/scripts/install-jig.sh")"

  INSTALL_LOCK_HELD=0 INSTALL_LOCK_PATH="$source_cache.lock" \
    REMINDER_FUNCTIONS="$function_source" SOURCE_CACHE="$source_cache" \
    /bin/bash -c 'eval "$REMINDER_FUNCTIONS"; record_mutable_source_refresh_reminder "$SOURCE_CACHE"'
  [[ ! -e "$source_cache/.jig-mutable-source-reminder" ]]

  INSTALL_LOCK_HELD=1 INSTALL_LOCK_PATH="$active_cache.lock" \
    REMINDER_FUNCTIONS="$function_source" SOURCE_CACHE="$source_cache" \
    /bin/bash -c 'eval "$REMINDER_FUNCTIONS"; record_mutable_source_refresh_reminder "$SOURCE_CACHE"'
  [[ ! -e "$source_cache/.jig-mutable-source-reminder" ]]

  INSTALL_LOCK_HELD=1 INSTALL_LOCK_PATH="$source_cache.lock" \
    REMINDER_FUNCTIONS="$function_source" SOURCE_CACHE="$source_cache" \
    /bin/bash -c 'eval "$REMINDER_FUNCTIONS"; record_mutable_source_refresh_reminder "$SOURCE_CACHE"'
  [[ -f "$source_cache/.jig-mutable-source-reminder" ]]

  rm "$source_cache/.jig-mutable-source-reminder"
  mkdir "$source_cache/.jig-mutable-source-reminder"
  INSTALL_LOCK_HELD=1 INSTALL_LOCK_PATH="$source_cache.lock" \
    REMINDER_FUNCTIONS="$function_source" SOURCE_CACHE="$source_cache" \
    /bin/bash -c 'eval "$REMINDER_FUNCTIONS"; record_mutable_source_refresh_reminder "$SOURCE_CACHE"' \
    2>"$stderr_file"
  grep -Fxq "Could not record the mutable-source reminder under $source_cache; this warning may repeat until that cache directory is writable." "$stderr_file"
  [[ "$(wc -l <"$stderr_file" | tr -d ' ')" == "1" ]]

  rmdir "$source_cache/.jig-mutable-source-reminder"
  rm -f "$active_cache/.jig-mutable-source-reminder"
  INSTALL_LOCK_HELD=1 INSTALL_LOCK_PATH="$active_cache.lock" RESOLVE_ONLY=0 \
    REMINDER_FUNCTIONS="$function_source" WARNED_CACHE="$source_cache" \
    REMINDER_CACHE="$active_cache" /bin/bash -c '
      eval "$REMINDER_FUNCTIONS"
      configured_source_is_mutable() { return 0; }
      warn_for_mutable_source_cache_if_due "$WARNED_CACHE" "$REMINDER_CACHE"
      warn_for_mutable_source_cache_if_due "$WARNED_CACHE" "$REMINDER_CACHE"
    ' 2>"$stderr_file"
  [[ ! -e "$source_cache/.jig-mutable-source-reminder" ]]
  [[ -f "$active_cache/.jig-mutable-source-reminder" ]]
  grep -Fq "Using a cached Jig runtime from a mutable source" "$stderr_file"
  [[ "$(wc -l <"$stderr_file" | tr -d ' ')" == "1" ]]

  touch -t 200001010000 "$active_cache/.jig-mutable-source-reminder"
  INSTALL_LOCK_HELD=1 INSTALL_LOCK_PATH="$active_cache.lock" RESOLVE_ONLY=0 \
    REMINDER_FUNCTIONS="$function_source" WARNED_CACHE="$source_cache" \
    REMINDER_CACHE="$active_cache" /bin/bash -c '
      eval "$REMINDER_FUNCTIONS"
      configured_source_is_mutable() { return 0; }
      warn_for_mutable_source_cache_if_due "$WARNED_CACHE" "$REMINDER_CACHE"
    ' 2>"$stderr_file"
  grep -Fq "Using a cached Jig runtime from a mutable source" "$stderr_file"
  [[ "$(wc -l <"$stderr_file" | tr -d ' ')" == "1" ]]

  INSTALL_LOCK_HELD=1 INSTALL_LOCK_PATH="$active_cache.lock" RESOLVE_ONLY=0 \
    REMINDER_FUNCTIONS="$function_source" WARNED_CACHE="$source_cache" \
    REMINDER_CACHE="$active_cache" /bin/bash -c '
      eval "$REMINDER_FUNCTIONS"
      configured_source_is_mutable() { return 0; }
      warn_for_mutable_source_cache_if_due "$WARNED_CACHE" "$REMINDER_CACHE"
    ' 2>"$stderr_file"
  [[ ! -s "$stderr_file" ]]
}

validate_git_local_source_stamp_ignores_diff_helpers_and_rejects_symbolic_links() {
  local source_repo="$TMP_DIR/local-stamp-source"
  local empty_diff="$TMP_DIR/empty-external-diff"
  local diff_marker="$TMP_DIR/external-diff-ran"
  local function_source stamp_before stamp_after
  local binary_stamp_one binary_stamp_two

  mkdir -p "$source_repo/crates/example/src"
  printf '%s\n' '[workspace]' 'members = ["crates/example"]' >"$source_repo/Cargo.toml"
  printf '%s\n' '# lock' >"$source_repo/Cargo.lock"
  printf '%s\n' '[package]' 'name = "example"' 'version = "0.1.0"' \
    >"$source_repo/crates/example/Cargo.toml"
  printf '%s\n' 'pub fn value() -> u8 { 1 }' >"$source_repo/crates/example/src/lib.rs"
  printf '\001\002' >"$source_repo/crates/example/src/asset.bin"
  git -C "$source_repo" init -q
  git -C "$source_repo" config user.name Fixture
  git -C "$source_repo" config user.email fixture@example.com
  git -C "$source_repo" add .
  git -C "$source_repo" commit -qm initial

  function_source="$(awk '
    /^hash_stdin\(\) \{/ { capture = 1 }
    /^local_source_install_is_current\(\) \{/ { exit }
    capture { print }
  ' "$ROOT_DIR/scripts/install-jig.sh")"
  stamp_before="$({
    STAMP_FUNCTIONS="$function_source" SOURCE_ROOT="$source_repo" \
      /bin/bash -c 'eval "$STAMP_FUNCTIONS"; local_source_stamp "$SOURCE_ROOT"'
  })"

  ln -s missing-target "$source_repo/crates/example/src/.#lib.rs"
  if STAMP_FUNCTIONS="$function_source" SOURCE_ROOT="$source_repo" \
    /bin/bash -c 'set -o pipefail; eval "$STAMP_FUNCTIONS"; local_source_stamp "$SOURCE_ROOT"' \
    >/dev/null 2>&1; then
    echo "Local source stamp accepted an untracked symbolic link." >&2
    exit 1
  fi
  rm "$source_repo/crates/example/src/.#lib.rs"

  printf '%s\n' '#!/bin/sh' ': >"$JIG_DIFF_MARKER"' 'exit 0' >"$empty_diff"
  chmod +x "$empty_diff"
  git -C "$source_repo" config diff.external "$empty_diff"
  printf '%s\n' 'pub fn changed() -> u8 { 2 }' >>"$source_repo/crates/example/src/lib.rs"
  stamp_after="$({
    JIG_DIFF_MARKER="$diff_marker" STAMP_FUNCTIONS="$function_source" \
      SOURCE_ROOT="$source_repo" \
      /bin/bash -c 'eval "$STAMP_FUNCTIONS"; local_source_stamp "$SOURCE_ROOT"'
  })"
  [[ -n "$stamp_after" && "$stamp_after" != "$stamp_before" ]]
  [[ ! -e "$diff_marker" ]]

  printf '\003\004' >"$source_repo/crates/example/src/asset.bin"
  binary_stamp_one="$({
    STAMP_FUNCTIONS="$function_source" SOURCE_ROOT="$source_repo" \
      /bin/bash -c 'eval "$STAMP_FUNCTIONS"; local_source_stamp "$SOURCE_ROOT"'
  })"
  printf '\005\006' >"$source_repo/crates/example/src/asset.bin"
  binary_stamp_two="$({
    STAMP_FUNCTIONS="$function_source" SOURCE_ROOT="$source_repo" \
      /bin/bash -c 'eval "$STAMP_FUNCTIONS"; local_source_stamp "$SOURCE_ROOT"'
  })"
  [[ -n "$binary_stamp_one" && "$binary_stamp_one" != "$stamp_after" ]]
  [[ -n "$binary_stamp_two" && "$binary_stamp_two" != "$binary_stamp_one" ]]
}

validate_git_local_source_stamp_fails_closed_when_untracked_hashing_fails() {
  local source_repo="$TMP_DIR/local-stamp-hash-failure-source"
  local fake_bin_dir="$TMP_DIR/local-stamp-hash-failure-bin"
  local function_source positive_stamp real_git

  mkdir -p "$source_repo/crates/example/src" "$fake_bin_dir"
  printf '%s\n' '[workspace]' 'members = ["crates/example"]' >"$source_repo/Cargo.toml"
  printf '%s\n' '# lock' >"$source_repo/Cargo.lock"
  printf '%s\n' '[package]' 'name = "example"' 'version = "0.1.0"' \
    >"$source_repo/crates/example/Cargo.toml"
  printf '%s\n' 'pub fn value() -> u8 { 1 }' >"$source_repo/crates/example/src/lib.rs"
  git -C "$source_repo" init -q
  git -C "$source_repo" config user.name Fixture
  git -C "$source_repo" config user.email fixture@example.com
  git -C "$source_repo" add .
  git -C "$source_repo" commit -qm initial
  printf '%s\n' '// hash failure' >"$source_repo/crates/example/src/a-hash-fails.rs"
  printf '%s\n' '// later success' >"$source_repo/crates/example/src/z-hash-succeeds.rs"

  real_git="$(command -v git)"
  printf '%s\n' \
    '#!/usr/bin/env bash' \
    'set -euo pipefail' \
    'is_hash=0' \
    'last_arg=""' \
    'for argument in "$@"; do' \
    '  [[ "$argument" == "hash-object" ]] && is_hash=1' \
    '  last_arg="$argument"' \
    'done' \
    'if [[ "${JIG_HASH_FAILURE_DISABLED:-0}" != 1 && "$is_hash" == 1 && "$last_arg" == *a-hash-fails.rs ]]; then exit 19; fi' \
    'exec "$JIG_REAL_GIT" "$@"' \
    >"$fake_bin_dir/git"
  chmod +x "$fake_bin_dir/git"

  function_source="$(awk '
    /^hash_stdin\(\) \{/ { capture = 1 }
    /^local_source_install_is_current\(\) \{/ { exit }
    capture { print }
  ' "$ROOT_DIR/scripts/install-jig.sh")"
  positive_stamp="$(JIG_HASH_FAILURE_DISABLED=1 JIG_REAL_GIT="$real_git" \
    PATH="$fake_bin_dir:$PATH" STAMP_FUNCTIONS="$function_source" SOURCE_ROOT="$source_repo" \
    /bin/bash -c '
      set -euo pipefail
      eval "$STAMP_FUNCTIONS"
      local_source_stamp "$SOURCE_ROOT"
    ')"
  if [[ "$positive_stamp" != sha256:* ]]; then
    echo "Local source stamp failure fixture did not prove its success path." >&2
    exit 1
  fi
  if JIG_REAL_GIT="$real_git" PATH="$fake_bin_dir:$PATH" \
    STAMP_FUNCTIONS="$function_source" SOURCE_ROOT="$source_repo" \
    /bin/bash -c '
      set -euo pipefail
      eval "$STAMP_FUNCTIONS"
      if local_source_stamp "$SOURCE_ROOT"; then exit 0; fi
      exit 1
    ' >/dev/null 2>&1
  then
    echo "Local source stamping ignored a failed untracked-file hash." >&2
    exit 1
  fi
}

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
  local contract_version
  local contract_cache_key
  local fake_path_dir="$TMP_DIR/explicit-root-path-bin"

  create_template_snapshot_repo "$template_snapshot"

  cp "$ROOT_DIR/tests/fixtures/backend-only.toml" "$answers_file"
  answers_set "$answers_file" template_source_url ""

  render_fixture_from_template "$template_snapshot" "$answers_file" "$rendered_dir"
  contract_version="$(json_get contract_version < "$rendered_dir/.agent/jig-contract.json")"
  contract_cache_key="contract-$contract_version"
  mkdir -p "$fake_path_dir"
  printf '%s\n' \
    '#!/bin/sh' \
    'if [ "${1:-}" = "__runtime-compatible" ]; then exit 0; fi' \
    'if [ "${1:-}" = "--version" ]; then printf "%s\n" "jig 99.0.0"; exit 0; fi' \
    'exit 99' \
    >"$fake_path_dir/jig"
  chmod +x "$fake_path_dir/jig"

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
    env -u JIG_DEV_BIN PATH="$fake_path_dir:$PATH" \
      scripts/install-jig.sh ".agent/.cache/jig/$contract_cache_key" >/dev/null
    [[ -x ".agent/.cache/jig/$contract_cache_key/bin/jig" ]]
    ".agent/.cache/jig/$contract_cache_key/bin/jig" __runtime-compatible --profile default .
  )
}

validate_relative_src_path_is_anchored_to_repository_root() {
  local template_snapshot="$TMP_DIR/template-relative-source"
  local answers_file="$TMP_DIR/backend-relative-source.toml"
  local rendered_dir="$TMP_DIR/render-relative-source"
  local fake_bin_dir="$TMP_DIR/relative-source-bin"
  local cargo_log="$TMP_DIR/relative-source-cargo.log"
  local install_root="$TMP_DIR/relative-source-install"

  create_template_snapshot_repo "$template_snapshot"
  cp "$ROOT_DIR/tests/fixtures/backend-only.toml" "$answers_file"
  answers_set "$answers_file" template_source_url ""
  render_fixture_from_template "$template_snapshot" "$answers_file" "$rendered_dir"
  answers_set "$rendered_dir/.jig.toml" _src_path "../template-relative-source"
  answers_set "$rendered_dir/.jig.toml" _commit ""
  write_fake_cargo_installer "$fake_bin_dir"

  (
    cd "$ROOT_DIR"
    env -u JIG_DEV_BIN PATH="$fake_bin_dir:$PATH" JIG_FIXTURE_CARGO_LOG="$cargo_log" \
      "$rendered_dir/scripts/install-jig.sh" --profile runtime "$install_root" >/dev/null
  )
  [[ -x "$install_root/bin/jig" ]]
  [[ -s "$cargo_log" ]]

  rm -f "$cargo_log"
  (
    cd "$TMP_DIR"
    env -u JIG_DEV_BIN PATH="$fake_bin_dir:$PATH" JIG_FIXTURE_CARGO_LOG="$cargo_log" \
      "$rendered_dir/scripts/install-jig.sh" --profile runtime --resolve-only \
      "$install_root" >/dev/null
  )
  [[ ! -e "$cargo_log" ]]
}

validate_template_source_url_installs_from_recorded_commit() {
  local bare_remote="$TMP_DIR/template-git-install.git"
  local template_snapshot="$TMP_DIR/template-git-install-snapshot"
  local answers_file="$TMP_DIR/backend-git-install.toml"
  local rendered_dir="$TMP_DIR/render-git-install"
  local contract_version
  local contract_cache_key

  create_template_snapshot_repo "$template_snapshot"
  git clone --bare --no-local "$template_snapshot" "$bare_remote" >/dev/null 2>&1

  cp "$ROOT_DIR/tests/fixtures/backend-only.toml" "$answers_file"
  answers_set "$answers_file" template_source_url "file://$bare_remote"

  render_fixture_from_template "$template_snapshot" "$answers_file" "$rendered_dir"
  contract_version="$(json_get contract_version < "$rendered_dir/.agent/jig-contract.json")"
  contract_cache_key="contract-$contract_version"

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
    env -u JIG_DEV_BIN CARGO_HOME="$TMP_DIR/cargo-home-git-install" \
      scripts/install-jig.sh ".agent/.cache/jig/$contract_cache_key" >/dev/null
    [[ -x ".agent/.cache/jig/$contract_cache_key/bin/jig" ]]
    ".agent/.cache/jig/$contract_cache_key/bin/jig" __runtime-compatible --profile default .
  )
}

validate_launcher_contract_drift_fails_before_runtime_install() {
  local template_snapshot="$TMP_DIR/template-contract-drift-snapshot"
  local answers_file="$TMP_DIR/backend-contract-drift.toml"
  local rendered_dir="$TMP_DIR/render-contract-drift"
  local fake_bin_dir="$TMP_DIR/contract-drift-bin"
  local cargo_log="$TMP_DIR/contract-drift-cargo.log"
  local stderr_file="$TMP_DIR/contract-drift.stderr"
  local bare_check_stderr="$TMP_DIR/contract-drift-bare-check.stderr"
  local current_jig

  create_template_snapshot_repo "$template_snapshot"
  cp "$ROOT_DIR/tests/fixtures/backend-only.toml" "$answers_file"
  render_fixture_from_template "$template_snapshot" "$answers_file" "$rendered_dir"
  local contract_version
  local drift_version
  contract_version="$(json_get contract_version < "$rendered_dir/.agent/jig-contract.json")"
  if [[ "$contract_version" -le 1 ]]; then
    echo "Contract drift fixture needs a prior supported epoch." >&2
    exit 1
  fi
  drift_version=$((contract_version - 1))
  LAUNCHER_PATH="$rendered_dir/scripts/jig" \
    CONTRACT_VERSION="$contract_version" DRIFT_VERSION="$drift_version" python3 <<'PY'
import os
import pathlib

path = pathlib.Path(os.environ["LAUNCHER_PATH"])
text = path.read_text()
current = f'CONTRACT_VERSION="{os.environ["CONTRACT_VERSION"]}"'
drifted = f'CONTRACT_VERSION="{os.environ["DRIFT_VERSION"]}"'
if current not in text:
    raise SystemExit(f"rendered launcher does not contain {current}")
path.write_text(text.replace(current, drifted, 1))
PY
  write_fake_cargo_installer "$fake_bin_dir"

  if (
    cd "$rendered_dir"
    env -u JIG_DEV_BIN PATH="$fake_bin_dir:$PATH" JIG_FIXTURE_CARGO_LOG="$cargo_log" \
      scripts/jig info >/dev/null 2>"$stderr_file"
  ); then
    echo "A launcher/manifest contract mismatch unexpectedly dispatched an ordinary command." >&2
    exit 1
  fi
  grep -Fq "Launcher contract version $drift_version does not match repository contract version $contract_version" "$stderr_file"
  if [[ -e "$cargo_log" ]]; then
    echo "Launcher/manifest contract drift invoked Cargo before it was rejected." >&2
    exit 1
  fi

  current_jig="$(cd "$ROOT_DIR" && realpath "${JIG_DEV_BIN:-target/debug/jig}")"
  if (
    cd "$rendered_dir"
    JIG_DEV_BIN="$current_jig" scripts/jig check >/dev/null 2>"$bare_check_stderr"
  ); then
    echo "Bare check unexpectedly succeeded without a check subcommand." >&2
    exit 1
  fi
  grep -Fq 'Usage: jig check [OPTIONS] <COMMAND>' "$bare_check_stderr"
  if grep -Fq 'repository contract did not validate' "$bare_check_stderr"; then
    echo "Repository validation hid bare check's subcommand diagnostic." >&2
    exit 1
  fi
}

validate_legacy_contract_uses_version_tag_only_as_source_locator() {
  local template_snapshot="$TMP_DIR/template-legacy-tag-snapshot"
  local answers_file="$TMP_DIR/backend-legacy-tag.toml"
  local rendered_dir="$TMP_DIR/render-legacy-tag"
  local fake_bin_dir="$TMP_DIR/legacy-tag-bin"
  local cargo_log="$TMP_DIR/legacy-tag-cargo.log"
  local incompatible_stderr="$TMP_DIR/legacy-tag-incompatible.stderr"
  local install_root="$rendered_dir/.agent/.cache/jig/legacy-tag-runtime"

  create_template_snapshot_repo "$template_snapshot"
  cp "$ROOT_DIR/tests/fixtures/backend-only.toml" "$answers_file"
  answers_set "$answers_file" template_source_url ""
  render_fixture_from_template "$template_snapshot" "$answers_file" "$rendered_dir"
  answers_set "$rendered_dir/.jig.toml" _src_path "https://example.invalid/jig.git"
  answers_set "$rendered_dir/.jig.toml" _commit ""
  answers_set "$rendered_dir/.jig.toml" template_source_url "https://example.invalid/jig.git"
  REPO_DIR="$rendered_dir" python3 <<'PY'
import json
import os
import pathlib

repo = pathlib.Path(os.environ["REPO_DIR"])
answers = repo.joinpath(".jig.toml")
lines = answers.read_text().splitlines()
index = next(i for i, line in enumerate(lines) if line.startswith("default_branch = "))
lines.insert(index + 1, 'jig_version = "0.2.0-beta.1"')
answers.write_text("\n".join(lines) + "\n")

contract_path = repo.joinpath(".agent/jig-contract.json")
contract = json.loads(contract_path.read_text())
contract["contract_version"] = 3
contract["jig_version"] = "0.2.0-beta.1"
contract_path.write_text(json.dumps(contract, indent=2) + "\n")
PY
  write_fake_cargo_installer "$fake_bin_dir"

  (
    cd "$rendered_dir"
    env -u JIG_DEV_BIN PATH="$fake_bin_dir:$PATH" JIG_FIXTURE_CARGO_LOG="$cargo_log" \
      scripts/install-jig.sh --profile runtime "$install_root" >/dev/null
  )

  [[ -x "$install_root/bin/jig" ]]
  grep -Fxq -- '--tag' "$cargo_log"
  grep -Fxq -- 'v0.2.0-beta.1' "$cargo_log"
  if grep -Fxq -- '--rev' "$cargo_log"; then
    echo "Legacy source fallback unexpectedly invented an immutable revision." >&2
    exit 1
  fi

  rm -f "$cargo_log"
  (
    cd "$rendered_dir"
    env -u JIG_DEV_BIN PATH="$fake_bin_dir:$PATH" JIG_FIXTURE_CARGO_LOG="$cargo_log" \
      scripts/install-jig.sh --profile runtime --resolve-only "$install_root" >/dev/null
  )
  [[ ! -e "$cargo_log" ]]

  answers_set "$rendered_dir/.jig.toml" jig_version "0.2.0-beta.2"
  REPO_DIR="$rendered_dir" python3 <<'PY'
import json
import os
import pathlib

contract_path = pathlib.Path(os.environ["REPO_DIR"], ".agent/jig-contract.json")
contract = json.loads(contract_path.read_text())
contract["jig_version"] = "0.2.0-beta.2"
contract_path.write_text(json.dumps(contract, indent=2) + "\n")
PY
  (
    cd "$rendered_dir"
    env -u JIG_DEV_BIN PATH="$fake_bin_dir:$PATH" JIG_FIXTURE_CARGO_LOG="$cargo_log" \
      scripts/install-jig.sh --profile runtime "$install_root" >/dev/null
  )
  grep -Fxq -- '--tag' "$cargo_log"
  grep -Fxq -- 'v0.2.0-beta.2' "$cargo_log"

  write_fake_pre_probe_cargo_installer "$fake_bin_dir"
  rm -rf "$install_root"
  if (
    cd "$rendered_dir"
    env -u JIG_DEV_BIN PATH="$fake_bin_dir:$PATH" JIG_FIXTURE_CARGO_LOG="$cargo_log" \
      scripts/install-jig.sh --profile runtime "$install_root" \
      >/dev/null 2>"$incompatible_stderr"
  ); then
    echo "A pre-probe legacy source unexpectedly satisfied runtime compatibility." >&2
    exit 1
  fi
  grep -Fq 'recorded source may predate runtime compatibility probes' "$incompatible_stderr"
  grep -Fq -- '--launcher-only --force' "$incompatible_stderr"
  grep -Fq -- 'update' "$incompatible_stderr"
}

validate_embedded_contract_fallback_requires_opt_in_and_uses_default_branch() {
  local template_snapshot="$TMP_DIR/template-embedded-fallback-snapshot"
  local answers_file="$TMP_DIR/backend-embedded-fallback.toml"
  local rendered_dir="$TMP_DIR/render-embedded-fallback"
  local fake_bin_dir="$TMP_DIR/embedded-fallback-bin"
  local cargo_log="$TMP_DIR/embedded-fallback-cargo.log"
  local stderr_file="$TMP_DIR/embedded-fallback.stderr"
  local install_root="$rendered_dir/.agent/.cache/jig/embedded-runtime"
  local pinned_install_root="$rendered_dir/.agent/.cache/jig/embedded-pinned-runtime"
  local pinned_commit

  create_template_snapshot_repo "$template_snapshot"
  cp "$ROOT_DIR/tests/fixtures/backend-only.toml" "$answers_file"
  answers_set "$answers_file" template_source_url ""
  render_fixture_from_template "$template_snapshot" "$answers_file" "$rendered_dir"
  answers_set "$rendered_dir/.jig.toml" _src_path "embedded:jig-sh"
  answers_set "$rendered_dir/.jig.toml" _commit ""
  answers_set "$rendered_dir/.jig.toml" template_source_url "https://example.invalid/jig.git"
  write_fake_cargo_installer "$fake_bin_dir"

  if (
    cd "$rendered_dir"
    env -u JIG_DEV_BIN PATH="$fake_bin_dir:$PATH" JIG_FIXTURE_CARGO_LOG="$cargo_log" \
      /bin/bash scripts/install-jig.sh --profile runtime "$install_root" \
      >/dev/null 2>"$stderr_file"
  ); then
    echo "Embedded source fallback unexpectedly installed without explicit opt-in." >&2
    exit 1
  fi
  [[ ! -e "$cargo_log" ]]
  grep -q 'JIG_INSTALL_ALLOW_EMBEDDED_SOURCE_FALLBACK=1' "$stderr_file"

  (
    cd "$rendered_dir"
    env -u JIG_DEV_BIN PATH="$fake_bin_dir:$PATH" JIG_FIXTURE_CARGO_LOG="$cargo_log" \
      JIG_INSTALL_ALLOW_EMBEDDED_SOURCE_FALLBACK=1 \
      /bin/bash scripts/install-jig.sh --profile runtime "$install_root" \
      >/dev/null 2>"$stderr_file"
  )

  [[ -x "$install_root/bin/jig" ]]
  grep -Fxq -- '--git' "$cargo_log"
  grep -Fxq -- 'https://example.invalid/jig.git' "$cargo_log"
  if grep -Eq '^--(tag|rev)$' "$cargo_log"; then
    echo "Embedded v4 fallback unexpectedly invented a tag or revision." >&2
    exit 1
  fi
  grep -q 'current default branch' "$stderr_file"
  grep -q -- '--refresh' "$stderr_file"

  rm -f "$install_root/.jig-mutable-source-reminder"
  (
    cd "$rendered_dir"
    env -u JIG_DEV_BIN PATH="$fake_bin_dir:$PATH" JIG_FIXTURE_CARGO_LOG="$cargo_log" \
      /bin/bash scripts/install-jig.sh --profile runtime --resolve-only "$install_root" \
      >/dev/null 2>"$stderr_file"
  )
  if grep -q 'cached Jig runtime from a mutable source' "$stderr_file" \
    || [[ -e "$install_root/.jig-mutable-source-reminder" ]]; then
    echo "--resolve-only mutated mutable-source reminder state." >&2
    exit 1
  fi
  (
    cd "$rendered_dir"
    env -u JIG_DEV_BIN PATH="$fake_bin_dir:$PATH" JIG_FIXTURE_CARGO_LOG="$cargo_log" \
      /bin/bash scripts/install-jig.sh --profile runtime "$install_root" \
      >/dev/null 2>"$stderr_file"
  )
  grep -q 'cached Jig runtime from a mutable source' "$stderr_file"
  grep -q -- '--refresh' "$stderr_file"
  [[ -f "$install_root/.jig-mutable-source-reminder" ]]

  rm -f "$install_root/.jig-mutable-source-reminder"
  mkdir "$install_root/.jig-mutable-source-reminder"
  (
    cd "$rendered_dir"
    env -u JIG_DEV_BIN PATH="$fake_bin_dir:$PATH" JIG_FIXTURE_CARGO_LOG="$cargo_log" \
      JIG_INSTALL_ALLOW_EMBEDDED_SOURCE_FALLBACK=1 \
      /bin/bash scripts/install-jig.sh --profile runtime --refresh "$install_root" \
      >/dev/null 2>"$stderr_file"
  )
  grep -Fq "Could not record the mutable-source reminder under $install_root" "$stderr_file"
  [[ -x "$install_root/bin/jig" ]]
  rmdir "$install_root/.jig-mutable-source-reminder"

  pinned_commit="$(git -C "$template_snapshot" rev-parse HEAD)"
  answers_set "$rendered_dir/.jig.toml" _commit "$pinned_commit"
  rm -f "$cargo_log"
  (
    cd "$rendered_dir"
    env -u JIG_DEV_BIN PATH="$fake_bin_dir:$PATH" JIG_FIXTURE_CARGO_LOG="$cargo_log" \
      JIG_INSTALL_ALLOW_EMBEDDED_SOURCE_FALLBACK=1 \
      /bin/bash scripts/install-jig.sh --profile runtime "$pinned_install_root" \
      >/dev/null 2>"$stderr_file"
  )
  grep -Fxq -- '--rev' "$cargo_log"
  grep -Fxq -- "$pinned_commit" "$cargo_log"
  grep -Fq "installing pinned source revision $pinned_commit" "$stderr_file"
  if grep -q 'current default branch' "$stderr_file" || grep -q -- '--refresh' "$stderr_file"; then
    echo "Pinned embedded fallback was incorrectly reported as a mutable default-branch install." >&2
    exit 1
  fi
}

validate_recorded_remote_source_precedes_fallback_url() {
  local template_snapshot="$TMP_DIR/template-source-precedence-snapshot"
  local answers_file="$TMP_DIR/backend-source-precedence.toml"
  local rendered_dir="$TMP_DIR/render-source-precedence"
  local fake_bin_dir="$TMP_DIR/source-precedence-bin"
  local cargo_log="$TMP_DIR/source-precedence-cargo.log"
  local stderr_file="$TMP_DIR/source-precedence.stderr"
  local install_root="$rendered_dir/.agent/.cache/jig/source-precedence-runtime"
  local recorded_source="https://example.invalid/recorded.git"
  local fallback_source="https://example.invalid/fallback.git"

  create_template_snapshot_repo "$template_snapshot"
  cp "$ROOT_DIR/tests/fixtures/backend-only.toml" "$answers_file"
  answers_set "$answers_file" template_source_url ""
  render_fixture_from_template "$template_snapshot" "$answers_file" "$rendered_dir"
  answers_set "$rendered_dir/.jig.toml" _src_path "$recorded_source"
  answers_set "$rendered_dir/.jig.toml" _commit ""
  answers_set "$rendered_dir/.jig.toml" template_source_url "$fallback_source"
  write_fake_cargo_installer "$fake_bin_dir"

  if (
    cd "$rendered_dir"
    env -u JIG_DEV_BIN PATH="$fake_bin_dir:$PATH" JIG_FIXTURE_CARGO_LOG="$cargo_log" \
      /bin/bash scripts/install-jig.sh --profile runtime "$install_root" \
      >/dev/null 2>"$stderr_file"
  ); then
    echo "Unpinned recorded remote unexpectedly installed without explicit opt-in." >&2
    exit 1
  fi
  [[ ! -e "$cargo_log" ]]
  grep -q 'JIG_INSTALL_ALLOW_UNPINNED_REMOTE=1' "$stderr_file"

  (
    cd "$rendered_dir"
    env -u JIG_DEV_BIN \
      JIG_INSTALL_ALLOW_UNPINNED_REMOTE=1 \
      PATH="$fake_bin_dir:$PATH" \
      JIG_FIXTURE_CARGO_LOG="$cargo_log" \
      /bin/bash scripts/install-jig.sh --profile runtime "$install_root" \
      >/dev/null 2>"$stderr_file"
  )

  grep -Fxq -- '--git' "$cargo_log"
  grep -Fxq -- "$recorded_source" "$cargo_log"
  if grep -Fxq -- "$fallback_source" "$cargo_log"; then
    echo "Installer used template_source_url ahead of the recorded remote _src_path." >&2
    exit 1
  fi
  if grep -Eq '^--(tag|rev)$' "$cargo_log"; then
    echo "Unpinned v4 fallback unexpectedly invented a tag or revision." >&2
    exit 1
  fi
  grep -q 'current default branch of an unpinned remote source' "$stderr_file"
  grep -q -- '--refresh' "$stderr_file"
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

validate_contract_cache_tracks_remote_source_revision() {
  local template_snapshot="$TMP_DIR/template-cache-revision-snapshot"
  local answers_file="$TMP_DIR/backend-cache-revision.toml"
  local rendered_dir="$TMP_DIR/render-cache-revision"
  local fake_bin_dir="$TMP_DIR/cache-revision-bin"
  local cargo_log="$TMP_DIR/cache-revision-cargo.log"
  local mcp_stderr="$TMP_DIR/cache-revision-mcp.stderr"
  local launcher_stderr="$TMP_DIR/cache-revision-launcher.stderr"
  local installer_stderr="$TMP_DIR/cache-revision-installer.stderr"
  local source_url="https://example.invalid/cache-revision.git"
  local first_commit="aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
  local second_commit="bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
  local contract_version
  local install_base
  local install_root

  create_template_snapshot_repo "$template_snapshot"
  cp "$ROOT_DIR/tests/fixtures/backend-only.toml" "$answers_file"
  answers_set "$answers_file" template_source_url ""
  render_fixture_from_template "$template_snapshot" "$answers_file" "$rendered_dir"
  answers_set "$rendered_dir/.jig.toml" _src_path "$source_url"
  answers_set "$rendered_dir/.jig.toml" _commit "$first_commit"
  answers_set "$rendered_dir/.jig.toml" template_source_url "$source_url"
  write_fake_cargo_installer "$fake_bin_dir"

  contract_version="$(json_get contract_version < "$rendered_dir/.agent/jig-contract.json")"
  if [[ -d "$rendered_dir/.git" ]]; then
    install_base="$rendered_dir/.git/jig-tools"
  else
    install_base="$rendered_dir/.agent/.cache/jig"
  fi
  install_root="$install_base/contract-$contract_version-runtime"

  (
    cd "$rendered_dir"
    env -u JIG_DEV_BIN PATH="$fake_bin_dir:$PATH" JIG_FIXTURE_CARGO_LOG="$cargo_log" \
      scripts/install-jig.sh --profile runtime >/dev/null
  )
  [[ -s "$install_root/.jig-source-stamp" ]]
  grep -Fxq -- "$first_commit" "$cargo_log"

  rm -f "$cargo_log"
  (
    cd "$rendered_dir"
    env -u JIG_DEV_BIN PATH="$fake_bin_dir:$PATH" JIG_FIXTURE_CARGO_LOG="$cargo_log" \
      scripts/install-jig.sh --profile runtime --resolve-only >/dev/null
  )
  [[ ! -e "$cargo_log" ]]

  (
    cd "$rendered_dir"
    printf '%s\n' \
      '#!/bin/sh' \
      'if [ "${1:-}" = "__runtime-compatible" ]; then exit 0; fi' \
      'exit 99' \
      >"$fake_bin_dir/jig"
    chmod +x "$fake_bin_dir/jig"
    env -u JIG_DEV_BIN PATH="$fake_bin_dir:$PATH" JIG_FIXTURE_CARGO_LOG="$cargo_log" \
      JIG_INSTALL_ALLOW_PATH_BINARY=1 \
      scripts/install-jig.sh --profile runtime --refresh >/dev/null
  )
  grep -Fxq -- "$first_commit" "$cargo_log"

  rm -f "$cargo_log"
  if (
    cd "$rendered_dir"
    env -u JIG_DEV_BIN PATH="$fake_bin_dir:$PATH" JIG_FIXTURE_CARGO_LOG="$cargo_log" \
      JIG_INSTALL_REFRESH=1 scripts/install-jig.sh --profile runtime --resolve-only >/dev/null
  ); then
    echo "A refresh-only cache resolution unexpectedly reused the remote runtime." >&2
    exit 1
  fi
  [[ ! -e "$cargo_log" ]]

  answers_set "$rendered_dir/.jig.toml" _commit "$second_commit"
  if (
    cd "$rendered_dir"
    env -u JIG_DEV_BIN PATH="$fake_bin_dir:$PATH" JIG_FIXTURE_CARGO_LOG="$cargo_log" \
      scripts/jig mcp >/dev/null 2>"$mcp_stderr"
  ); then
    echo "MCP unexpectedly reused a cache from the prior template revision." >&2
    exit 1
  fi
  [[ ! -e "$cargo_log" ]]
  grep -q 'No prebuilt Jig' "$mcp_stderr"

  (
    cd "$rendered_dir"
    env -u JIG_DEV_BIN PATH="$fake_bin_dir:$PATH" JIG_FIXTURE_CARGO_LOG="$cargo_log" \
      scripts/jig --help >/dev/null
  )
  grep -Fxq -- "$second_commit" "$cargo_log"

  rm -f "$cargo_log"
  (
    cd "$rendered_dir"
    env -u JIG_DEV_BIN PATH="$fake_bin_dir:$PATH" JIG_FIXTURE_CARGO_LOG="$cargo_log" \
      scripts/jig mcp >/dev/null
  )
  [[ ! -e "$cargo_log" ]]

  REPO_DIR="$rendered_dir" python3 <<'PY'
import json
import os
import pathlib

path = pathlib.Path(os.environ["REPO_DIR"]) / ".agent/jig-contract.json"
manifest = json.loads(path.read_text())
manifest["tools"][0]["contract_version"] = 999
path.write_text(json.dumps(manifest, separators=(",", ":")) + "\n")
PY
  (
    cd "$rendered_dir"
    env -u JIG_DEV_BIN PATH="$fake_bin_dir:$PATH" JIG_FIXTURE_CARGO_LOG="$cargo_log" \
      scripts/jig mcp >/dev/null
  )
  [[ ! -e "$cargo_log" ]]

  printf '%s\n' '{"contract_version":4,' >"$rendered_dir/.agent/jig-contract.json"
  if (
    cd "$rendered_dir"
    env -u JIG_DEV_BIN PATH="$fake_bin_dir:$PATH" scripts/jig work status \
      >/dev/null 2>"$launcher_stderr"
  ); then
    echo "Ordinary launcher command accepted malformed contract JSON." >&2
    exit 1
  fi
  grep -q 'scripts/jig doctor' "$launcher_stderr"
  (
    cd "$rendered_dir"
    env -u JIG_DEV_BIN PATH="$fake_bin_dir:$PATH" scripts/jig doctor \
      >/dev/null 2>"$launcher_stderr"
  )

  if (
    cd "$rendered_dir"
    env -u JIG_DEV_BIN PATH="$fake_bin_dir:$PATH" scripts/install-jig.sh --profile runtime \
      >/dev/null 2>"$installer_stderr"
  ); then
    echo "Installer unexpectedly accepted malformed contract JSON." >&2
    exit 1
  fi
  grep -q 'Failed to read a numeric contract_version' "$installer_stderr"
  if grep -q 'Traceback' "$installer_stderr"; then
    echo "Installer leaked a Python traceback for malformed contract JSON." >&2
    exit 1
  fi
}

validate_non_git_local_source_cache_tracks_content_and_identity() {
  local template_snapshot="$TMP_DIR/template-non-git-source-snapshot"
  local source_copy="$TMP_DIR/template-non-git-source"
  local source_archive="$TMP_DIR/template-non-git-source.tar"
  local second_source="$TMP_DIR/template-non-git-source-second"
  local answers_file="$TMP_DIR/backend-non-git-source.toml"
  local rendered_dir="$TMP_DIR/render-non-git-source"
  local fake_bin_dir="$TMP_DIR/non-git-source-bin"
  local cargo_log="$TMP_DIR/non-git-source-cargo.log"
  local stamp_log="$TMP_DIR/non-git-source-stamps.log"
  local install_root="$TMP_DIR/non-git-source-install"
  local real_python

  create_template_snapshot_repo "$template_snapshot"
  mkdir -p "$source_copy"
  git -C "$template_snapshot" archive --format=tar --output="$source_archive" HEAD
  tar -C "$source_copy" -xf "$source_archive"
  rm -f "$source_archive"
  cp "$ROOT_DIR/tests/fixtures/backend-only.toml" "$answers_file"
  answers_set "$answers_file" template_source_url ""
  render_fixture_from_template "$template_snapshot" "$answers_file" "$rendered_dir"
  answers_set "$rendered_dir/.jig.toml" _src_path "$source_copy"
  answers_set "$rendered_dir/.jig.toml" _commit ""
  write_fake_cargo_installer "$fake_bin_dir"

  (
    cd "$rendered_dir"
    env -u JIG_DEV_BIN PATH="$fake_bin_dir:$PATH" JIG_FIXTURE_CARGO_LOG="$cargo_log" \
      scripts/install-jig.sh --profile runtime "$install_root" >/dev/null
  )
  [[ -s "$install_root/.jig-source-stamp" ]]
  [[ -s "$install_root/.jig-source-metadata-stamp" ]]

  real_python="$(command -v python3)"
  printf '%s\n' \
    '#!/bin/sh' \
    'last=' \
    'for argument in "$@"; do last="$argument"; done' \
    'case "$last" in content|metadata) printf "%s\n" "$last" >>"$JIG_FIXTURE_STAMP_LOG" ;; esac' \
    'exec "$JIG_FIXTURE_REAL_PYTHON" "$@"' \
    >"$fake_bin_dir/python3"
  chmod +x "$fake_bin_dir/python3"

  rm -f "$cargo_log"
  (
    cd "$rendered_dir"
    env -u JIG_DEV_BIN PATH="$fake_bin_dir:$PATH" JIG_FIXTURE_CARGO_LOG="$cargo_log" \
      JIG_FIXTURE_STAMP_LOG="$stamp_log" JIG_FIXTURE_REAL_PYTHON="$real_python" \
      scripts/install-jig.sh --profile runtime --resolve-only "$install_root" >/dev/null
  )
  [[ ! -e "$cargo_log" ]]
  grep -Fxq metadata "$stamp_log"
  if grep -Fxq content "$stamp_log"; then
    echo "Unchanged non-Git source cache rehashed file contents." >&2
    exit 1
  fi

  printf '%s\n' '// cache-invalidating source change' >>"$source_copy/crates/jig/src/main.rs"
  rm -f "$stamp_log"
  if (
    cd "$rendered_dir"
    env -u JIG_DEV_BIN PATH="$fake_bin_dir:$PATH" JIG_FIXTURE_CARGO_LOG="$cargo_log" \
      JIG_FIXTURE_STAMP_LOG="$stamp_log" JIG_FIXTURE_REAL_PYTHON="$real_python" \
      scripts/install-jig.sh --profile runtime --resolve-only "$install_root" >/dev/null
  ); then
    echo "Non-Git local source cache ignored a content change." >&2
    exit 1
  fi
  grep -Fxq content "$stamp_log"

  (
    cd "$rendered_dir"
    env -u JIG_DEV_BIN PATH="$fake_bin_dir:$PATH" JIG_FIXTURE_CARGO_LOG="$cargo_log" \
      JIG_FIXTURE_STAMP_LOG="$stamp_log" JIG_FIXTURE_REAL_PYTHON="$real_python" \
      scripts/install-jig.sh --profile runtime "$install_root" >/dev/null
  )
  [[ -e "$cargo_log" ]]

  cp -R "$source_copy" "$second_source"
  answers_set "$rendered_dir/.jig.toml" _src_path "$second_source"
  rm -f "$cargo_log"
  if (
    cd "$rendered_dir"
    env -u JIG_DEV_BIN PATH="$fake_bin_dir:$PATH" JIG_FIXTURE_CARGO_LOG="$cargo_log" \
      JIG_FIXTURE_STAMP_LOG="$stamp_log" JIG_FIXTURE_REAL_PYTHON="$real_python" \
      scripts/install-jig.sh --profile runtime --resolve-only "$install_root" >/dev/null
  ); then
    echo "Non-Git local source cache ignored a source identity change." >&2
    exit 1
  fi
}

validate_unborn_git_local_source_cache_tracks_content() {
  local template_snapshot="$TMP_DIR/template-unborn-git-source-snapshot"
  local source_copy="$TMP_DIR/template-unborn-git-source"
  local source_archive="$TMP_DIR/template-unborn-git-source.tar"
  local answers_file="$TMP_DIR/backend-unborn-git-source.toml"
  local rendered_dir="$TMP_DIR/render-unborn-git-source"
  local fake_bin_dir="$TMP_DIR/unborn-git-source-bin"
  local cargo_log="$TMP_DIR/unborn-git-source-cargo.log"
  local install_root="$TMP_DIR/unborn-git-source-install"

  create_template_snapshot_repo "$template_snapshot"
  mkdir -p "$source_copy"
  git -C "$template_snapshot" archive --format=tar --output="$source_archive" HEAD
  tar -C "$source_copy" -xf "$source_archive"
  rm -f "$source_archive"
  git -C "$source_copy" init >/dev/null
  git -C "$source_copy" add Cargo.toml Cargo.lock crates
  cp "$ROOT_DIR/tests/fixtures/backend-only.toml" "$answers_file"
  answers_set "$answers_file" template_source_url ""
  render_fixture_from_template "$template_snapshot" "$answers_file" "$rendered_dir"
  answers_set "$rendered_dir/.jig.toml" _src_path "$source_copy"
  answers_set "$rendered_dir/.jig.toml" _commit ""
  write_fake_cargo_installer "$fake_bin_dir"

  (
    cd "$rendered_dir"
    env -u JIG_DEV_BIN PATH="$fake_bin_dir:$PATH" JIG_FIXTURE_CARGO_LOG="$cargo_log" \
      scripts/install-jig.sh --profile runtime "$install_root" >/dev/null
  )
  [[ -s "$install_root/.jig-source-stamp" ]]

  rm -f "$cargo_log"
  (
    cd "$rendered_dir"
    env -u JIG_DEV_BIN PATH="$fake_bin_dir:$PATH" JIG_FIXTURE_CARGO_LOG="$cargo_log" \
      scripts/install-jig.sh --profile runtime --resolve-only "$install_root" >/dev/null
  )
  [[ ! -e "$cargo_log" ]]

  printf '%s\n' '// unborn cache-invalidating source change' \
    >>"$source_copy/crates/jig/src/main.rs"
  if (
    cd "$rendered_dir"
    env -u JIG_DEV_BIN PATH="$fake_bin_dir:$PATH" JIG_FIXTURE_CARGO_LOG="$cargo_log" \
      scripts/install-jig.sh --profile runtime --resolve-only "$install_root" >/dev/null
  ); then
    echo "Unborn-Git local source cache ignored a content change." >&2
    exit 1
  fi
}

validate_source_checkout_requires_explicit_dev_binary_and_honors_refresh() {
  local template_snapshot="$TMP_DIR/template-source-checkout-snapshot"
  local answers_file="$TMP_DIR/backend-source-checkout.toml"
  local rendered_dir="$TMP_DIR/render-source-checkout"
  local fake_bin_dir="$TMP_DIR/source-checkout-bin"
  local cargo_log="$TMP_DIR/source-checkout-cargo.log"
  local mutable_warning_stderr="$TMP_DIR/source-checkout-mutable-warning.stderr"
  local contract_version
  local install_root
  local full_install_root
  local selected

  create_template_snapshot_repo "$template_snapshot"
  cp "$ROOT_DIR/tests/fixtures/backend-only.toml" "$answers_file"
  render_fixture_from_template "$template_snapshot" "$answers_file" "$rendered_dir"
  mkdir -p \
    "$rendered_dir/crates/jig/src" \
    "$rendered_dir/target/debug" \
    "$rendered_dir/templates/project/scripts"
  printf '%s\n' '[workspace]' 'members = ["crates/jig"]' >"$rendered_dir/Cargo.toml"
  printf '%s\n' '# fixture lock' >"$rendered_dir/Cargo.lock"
  printf '%s\n' '[package]' 'name = "jig-sh"' 'version = "99.0.0"' \
    >"$rendered_dir/crates/jig/Cargo.toml"
  printf '%s\n' 'fn main() {}' >"$rendered_dir/crates/jig/src/main.rs"
  printf '%s\n' '# fixture source-checkout marker' \
    >"$rendered_dir/templates/project/scripts/install-jig.sh.jinja"
  printf '%s\n' \
    '#!/bin/sh' \
    'if [ "${1:-}" = "__runtime-compatible" ]; then exit 0; fi' \
    'exit 99' \
    >"$rendered_dir/target/debug/jig"
  chmod +x "$rendered_dir/target/debug/jig"
  write_fake_cargo_installer "$fake_bin_dir"

  contract_version="$(json_get contract_version < "$rendered_dir/.agent/jig-contract.json")"
  install_root="$rendered_dir/.git/jig-tools/contract-$contract_version-runtime"
  full_install_root="$rendered_dir/.git/jig-tools/contract-$contract_version"

  if (
    cd "$rendered_dir"
    env -u JIG_DEV_BIN PATH="$fake_bin_dir:$PATH" JIG_FIXTURE_CARGO_LOG="$cargo_log" \
      scripts/install-jig.sh --profile runtime --resolve-only >/dev/null
  ); then
    echo "Source-checkout resolution implicitly trusted target/debug/jig." >&2
    exit 1
  fi
  [[ ! -e "$cargo_log" ]]

  (
    cd "$rendered_dir"
    env -u JIG_DEV_BIN PATH="$fake_bin_dir:$PATH" JIG_FIXTURE_CARGO_LOG="$cargo_log" \
      scripts/install-jig.sh --profile runtime >/dev/null
  )
  [[ -x "$install_root/bin/jig" ]]
  [[ -s "$install_root/.jig-source-stamp" ]]
  [[ -s "$cargo_log" ]]

  rm -f "$cargo_log"
  (
    cd "$rendered_dir"
    env -u JIG_DEV_BIN PATH="$fake_bin_dir:$PATH" JIG_FIXTURE_CARGO_LOG="$cargo_log" \
      scripts/install-jig.sh --profile runtime --resolve-only >/dev/null
  )
  [[ ! -e "$cargo_log" ]]

  answers_set "$rendered_dir/.jig.toml" _src_path \
    "https://example.invalid/configured-source.git"
  answers_set "$rendered_dir/.jig.toml" _commit ""
  (
    cd "$rendered_dir"
    env -u JIG_DEV_BIN PATH="$fake_bin_dir:$PATH" JIG_FIXTURE_CARGO_LOG="$cargo_log" \
      scripts/install-jig.sh --profile runtime >/dev/null 2>"$mutable_warning_stderr"
  )
  [[ ! -e "$cargo_log" ]]
  if grep -q 'cached Jig runtime from a mutable source' "$mutable_warning_stderr"; then
    echo "Source-checkout cache was misreported as an unpinned remote cache." >&2
    exit 1
  fi

  if (
    cd "$rendered_dir"
    env -u JIG_DEV_BIN PATH="$fake_bin_dir:$PATH" JIG_FIXTURE_CARGO_LOG="$cargo_log" \
      scripts/install-jig.sh --profile runtime --refresh --resolve-only >/dev/null
  ); then
    echo "Source-checkout refresh-only resolution reused the stamped cache." >&2
    exit 1
  fi
  [[ ! -e "$cargo_log" ]]

  (
    cd "$rendered_dir"
    env -u JIG_DEV_BIN PATH="$fake_bin_dir:$PATH" JIG_FIXTURE_CARGO_LOG="$cargo_log" \
      scripts/install-jig.sh --profile runtime --refresh >/dev/null
  )
  [[ -s "$cargo_log" ]]

  rm -f "$cargo_log"
  (
    cd "$rendered_dir"
    env -u JIG_DEV_BIN PATH="$fake_bin_dir:$PATH" JIG_FIXTURE_CARGO_LOG="$cargo_log" \
      scripts/install-jig.sh --profile default >/dev/null
  )
  [[ -x "$full_install_root/bin/jig" ]]
  rm -f "$cargo_log"
  selected="$({
    cd "$rendered_dir"
    env -u JIG_DEV_BIN PATH="$fake_bin_dir:$PATH" JIG_FIXTURE_CARGO_LOG="$cargo_log" \
      scripts/install-jig.sh --profile runtime --resolve-only
  })"
  [[ "$(realpath "$selected")" == "$(realpath "$full_install_root/bin/jig")" ]]
  selected="$({
    cd "$rendered_dir"
    env -u JIG_DEV_BIN PATH="$fake_bin_dir:$PATH" JIG_FIXTURE_CARGO_LOG="$cargo_log" \
      scripts/install-jig.sh --profile mcp --resolve-only
  })"
  [[ "$(realpath "$selected")" == "$(realpath "$full_install_root/bin/jig")" ]]
  [[ ! -e "$cargo_log" ]]
}

validate_local_source_change_during_build_is_not_cached() {
  local template_snapshot="$TMP_DIR/template-source-race-snapshot"
  local answers_file="$TMP_DIR/backend-source-race.toml"
  local rendered_dir="$TMP_DIR/render-source-race"
  local fake_bin_dir="$TMP_DIR/source-race-bin"
  local cargo_log="$TMP_DIR/source-race-cargo.log"
  local install_root="$TMP_DIR/source-race-install"
  local stderr_file="$TMP_DIR/source-race.stderr"
  local mutation_path

  create_template_snapshot_repo "$template_snapshot"
  cp "$ROOT_DIR/tests/fixtures/backend-only.toml" "$answers_file"
  answers_set "$answers_file" template_source_url ""
  render_fixture_from_template "$template_snapshot" "$answers_file" "$rendered_dir"
  answers_set "$rendered_dir/.jig.toml" _src_path "$template_snapshot"
  answers_set "$rendered_dir/.jig.toml" _commit ""
  write_fake_cargo_installer "$fake_bin_dir"
  mutation_path="$template_snapshot/crates/jig/src/main.rs"

  if (
    cd "$rendered_dir"
    env -u JIG_DEV_BIN PATH="$fake_bin_dir:$PATH" \
      JIG_FIXTURE_CARGO_LOG="$cargo_log" \
      JIG_FIXTURE_MUTATE_SOURCE_PATH="$mutation_path" \
      scripts/install-jig.sh --profile runtime "$install_root" \
      >/dev/null 2>"$stderr_file"
  ); then
    echo "Local source install accepted a source change during the build." >&2
    exit 1
  fi
  grep -q 'Local Jig source changed while cargo install was running' "$stderr_file"
  [[ -x "$install_root/bin/jig" ]]
  [[ ! -e "$install_root/.jig-source-stamp" ]]
  [[ ! -e "$install_root/.jig-source-metadata-stamp" ]]

  rm -f "$cargo_log"
  if (
    cd "$rendered_dir"
    env -u JIG_DEV_BIN PATH="$fake_bin_dir:$PATH" \
      JIG_FIXTURE_CARGO_LOG="$cargo_log" \
      scripts/install-jig.sh --profile runtime --resolve-only "$install_root" \
      >/dev/null
  ); then
    echo "Unstamped binary from a racing local build was reused." >&2
    exit 1
  fi
  [[ ! -e "$cargo_log" ]]
}

validate_invalid_config_info_reports_once() {
  local template_snapshot="$TMP_DIR/template-invalid-config-snapshot"
  local answers_file="$TMP_DIR/backend-invalid-config.toml"
  local rendered_dir="$TMP_DIR/render-invalid-config"
  local stderr_file="$TMP_DIR/invalid-config.stderr"

  create_template_snapshot_repo "$template_snapshot"
  cp "$ROOT_DIR/tests/fixtures/backend-only.toml" "$answers_file"
  render_fixture_from_template "$template_snapshot" "$answers_file" "$rendered_dir"
  printf '%s\n' 'this is not valid toml = [' >"$rendered_dir/.jig.toml"

  if (
    cd "$rendered_dir"
    env -u JIG_DEV_BIN scripts/jig --help >/dev/null 2>"$stderr_file"
  ); then
    echo "Help unexpectedly started with invalid config and no compatible cache." >&2
    exit 1
  fi
  [[ "$(grep -c 'Cannot resolve a Jig runtime because' "$stderr_file")" == "1" ]]
  [[ "$(grep -c 'Preparing a Jig runtime compatible' "$stderr_file")" == "1" ]]
  [[ "$(grep -c 'Failed to parse' "$stderr_file")" == "1" ]]
}

validate_seed_checks_the_copy_before_cache_publication() {
  local template_snapshot="$TMP_DIR/template-seed-publication-snapshot"
  local answers_file="$TMP_DIR/backend-seed-publication.toml"
  local rendered_dir="$TMP_DIR/render-seed-publication"
  local install_root="$TMP_DIR/seed-publication-install"
  local expected_bin="$TMP_DIR/seed-publication-expected"
  local changing_dev_bin="$TMP_DIR/changing-dev-jig"
  local probe_state="$TMP_DIR/changing-dev-jig.probed"
  local stderr_file="$TMP_DIR/seed-publication.stderr"

  create_template_snapshot_repo "$template_snapshot"
  cp "$ROOT_DIR/tests/fixtures/backend-only.toml" "$answers_file"
  render_fixture_from_template "$template_snapshot" "$answers_file" "$rendered_dir"
  mkdir -p "$install_root/bin"
  printf '%s\n' '#!/bin/sh' '# previous compatible cache' 'exit 0' >"$install_root/bin/jig"
  chmod +x "$install_root/bin/jig"
  cp "$install_root/bin/jig" "$expected_bin"

  printf '%s\n' \
    '#!/bin/sh' \
    'if [ "${1:-}" = "__runtime-compatible" ]; then' \
    '  probes=0' \
    '  if [ -r "$JIG_FIXTURE_SEED_PROBE_STATE" ]; then read -r probes <"$JIG_FIXTURE_SEED_PROBE_STATE"; fi' \
    '  probes=$((probes + 1))' \
    '  printf "%s\n" "$probes" >"$JIG_FIXTURE_SEED_PROBE_STATE"' \
    '  if [ "$probes" -le 2 ]; then' \
    '    exit 0' \
    '  fi' \
    'fi' \
    'exit 1' \
    >"$changing_dev_bin"
  chmod +x "$changing_dev_bin"

  if (
    cd "$rendered_dir"
    JIG_DEV_BIN="$changing_dev_bin" \
      JIG_FIXTURE_SEED_PROBE_STATE="$probe_state" scripts/install-jig.sh \
      --profile runtime --seed-dev-bin "$install_root" >/dev/null 2>"$stderr_file"
  ); then
    echo "Seed unexpectedly published a development binary that changed after validation." >&2
    exit 1
  fi
  grep -Fq \
    'Copied JIG_DEV_BIN failed compatibility validation; the existing cached binary was left unchanged.' \
    "$stderr_file"
  cmp -s "$expected_bin" "$install_root/bin/jig"
  if compgen -G "$install_root/bin/.jig-seed.*" >/dev/null; then
    echo "Failed seed left a temporary binary in the managed cache." >&2
    exit 1
  fi
}

validate_path_launcher_is_not_treated_as_runtime() {
  local template_snapshot="$TMP_DIR/template-path-launcher-snapshot"
  local answers_file="$TMP_DIR/backend-path-launcher.toml"
  local rendered_dir="$TMP_DIR/render-path-launcher"
  local path_bin="$TMP_DIR/path-launcher-bin"
  local stderr_file="$TMP_DIR/path-launcher.stderr"

  create_template_snapshot_repo "$template_snapshot"
  cp "$ROOT_DIR/tests/fixtures/backend-only.toml" "$answers_file"
  render_fixture_from_template "$template_snapshot" "$answers_file" "$rendered_dir"
  mkdir -p "$path_bin"
  cp "$rendered_dir/scripts/jig" "$path_bin/jig"

  if python3 - "$rendered_dir" "$path_bin:$PATH" "$stderr_file" <<'PY'
import os
import pathlib
import subprocess
import sys

repo, path, stderr_path = sys.argv[1:]
environment = os.environ.copy()
environment.pop("JIG_DEV_BIN", None)
environment["JIG_INSTALL_ALLOW_PATH_BINARY"] = "1"
environment["PATH"] = path
with open(stderr_path, "wb") as stderr:
    result = subprocess.run(
        ["scripts/install-jig.sh", "--profile", "runtime", "--resolve-only"],
        cwd=repo,
        env=environment,
        stdout=subprocess.DEVNULL,
        stderr=stderr,
        timeout=5,
    )
raise SystemExit(0 if result.returncode == 0 else 1)
PY
  then
    echo "Generated scripts/jig on PATH was accepted as a runtime binary." >&2
    exit 1
  fi

  local legacy_probe_log="$TMP_DIR/path-legacy-launcher.probed"
  printf '%s\n' \
    '#!/bin/sh' \
    'JIG_VERSION="0.2.0-beta.1"' \
    'resolve_cached_binary() { :; }' \
    'if [ "${1:-}" = "__runtime-compatible" ]; then : >"$JIG_LEGACY_PROBE_LOG"; exit 0; fi' \
    'bin_path=/not/reached' \
    'exec "$bin_path" "$@"' \
    >"$path_bin/jig"
  chmod +x "$path_bin/jig"
  if python3 - "$rendered_dir" "$path_bin:$PATH" "$stderr_file" "$legacy_probe_log" <<'PY'
import os
import subprocess
import sys

repo, path, stderr_path, probe_log = sys.argv[1:]
environment = os.environ.copy()
environment.pop("JIG_DEV_BIN", None)
environment["JIG_INSTALL_ALLOW_PATH_BINARY"] = "1"
environment["JIG_LEGACY_PROBE_LOG"] = probe_log
environment["PATH"] = path
with open(stderr_path, "wb") as stderr:
    result = subprocess.run(
        ["scripts/install-jig.sh", "--profile", "runtime", "--resolve-only"],
        cwd=repo,
        env=environment,
        stdout=subprocess.DEVNULL,
        stderr=stderr,
        timeout=5,
    )
raise SystemExit(0 if result.returncode == 0 else 1)
PY
  then
    echo "Legacy generated scripts/jig on PATH was accepted as a runtime binary." >&2
    exit 1
  fi
  if [[ -e "$legacy_probe_log" ]]; then
    echo "Legacy generated scripts/jig was executed during runtime discovery." >&2
    exit 1
  fi

  local forged_probe_log="$TMP_DIR/path-forged-native-prefix.probed"
  FORGED_PATH="$path_bin/jig" python3 <<'PY'
import os
import pathlib

path = pathlib.Path(os.environ["FORGED_PATH"])
path.write_bytes(
    b"\x7fELF\n"
    b'if [ "${1:-}" = "__runtime-compatible" ]; then : >"$JIG_FORGED_PROBE_LOG"; exit 0; fi\n'
    b"exit 1\n"
)
PY
  chmod +x "$path_bin/jig"
  if (
    cd "$rendered_dir"
    env -u JIG_DEV_BIN PATH="$path_bin:$PATH" JIG_INSTALL_ALLOW_PATH_BINARY=1 \
      JIG_FORGED_PROBE_LOG="$forged_probe_log" \
      scripts/install-jig.sh --profile runtime --resolve-only >/dev/null 2>&1
  ); then
    echo "Forged ELF-prefix PATH script was accepted as a native Jig runtime." >&2
    exit 1
  fi
  if [[ -e "$forged_probe_log" ]]; then
    echo "Forged ELF-prefix PATH script was interpreted during runtime discovery." >&2
    exit 1
  fi

  rm -f "$rendered_dir/scripts/jig" "$path_bin/jig"
  ln -s "$(cd "$ROOT_DIR" && realpath "${JIG_DEV_BIN:-target/debug/jig}")" "$path_bin/jig"
  local resolved_path
  resolved_path="$(
    cd "$rendered_dir"
    env -u JIG_DEV_BIN PATH="$path_bin:$PATH" JIG_INSTALL_ALLOW_PATH_BINARY=1 \
      scripts/install-jig.sh --profile runtime --resolve-only
  )"
  [[ "$(realpath "$resolved_path")" == "$(realpath "$path_bin/jig")" ]]
}

validate_incompatible_dev_help_is_not_retried() {
  local template_snapshot="$TMP_DIR/template-dev-help-snapshot"
  local answers_file="$TMP_DIR/backend-dev-help.toml"
  local rendered_dir="$TMP_DIR/render-dev-help"
  local incompatible_bin="$TMP_DIR/incompatible-dev-help-jig"
  local probe_log="$TMP_DIR/incompatible-dev-help.probes"
  local stderr_file="$TMP_DIR/incompatible-dev-help.stderr"

  create_template_snapshot_repo "$template_snapshot"
  cp "$ROOT_DIR/tests/fixtures/backend-only.toml" "$answers_file"
  render_fixture_from_template "$template_snapshot" "$answers_file" "$rendered_dir"
  printf '%s\n' \
    '#!/bin/sh' \
    'if [ "${1:-}" = "__runtime-compatible" ]; then printf "%s\n" probe >>"$JIG_PROBE_LOG"; exit 1; fi' \
    'exit 1' \
    >"$incompatible_bin"
  chmod +x "$incompatible_bin"

  if (
    cd "$rendered_dir"
    JIG_DEV_BIN="$incompatible_bin" JIG_PROBE_LOG="$probe_log" \
      scripts/jig --help >/dev/null 2>"$stderr_file"
  ); then
    echo "Incompatible JIG_DEV_BIN unexpectedly served help." >&2
    exit 1
  fi
  [[ "$(wc -l <"$probe_log" | tr -d '[:space:]')" == "1" ]]
  if grep -q 'Preparing a Jig runtime' "$stderr_file"; then
    echo "Help retried an authoritative incompatible JIG_DEV_BIN." >&2
    exit 1
  fi
  grep -q 'jig update' "$stderr_file"
  grep -q 'JIG_DEV_BIN' "$stderr_file"
}

validate_missing_python_is_actionable() {
  local minimal_path="$TMP_DIR/no-python-bin"
  local stderr_file="$TMP_DIR/no-python.stderr"
  local current_jig
  current_jig="$(cd "$ROOT_DIR" && realpath "${JIG_DEV_BIN:-target/debug/jig}")"
  mkdir -p "$minimal_path"
  ln -s "$(command -v dirname)" "$minimal_path/dirname"
  ln -s "$(command -v realpath)" "$minimal_path/realpath"

  PATH="$minimal_path" JIG_DEV_BIN="$current_jig" \
    /bin/bash "$ROOT_DIR/scripts/install-jig.sh" --contract-version 4 --profile runtime \
    >/dev/null

  if env -u JIG_DEV_BIN PATH="$minimal_path" \
    /bin/bash "$ROOT_DIR/scripts/install-jig.sh" --contract-version 4 \
    --profile runtime --resolve-only >/dev/null 2>"$stderr_file"; then
    echo "Installer unexpectedly ran without Python 3." >&2
    exit 1
  fi
  grep -q 'Python 3 is required' "$stderr_file"
  if grep -q 'Failed to read a numeric contract_version' "$stderr_file"; then
    echo "Missing Python was misreported as malformed contract JSON." >&2
    exit 1
  fi
}

validate_refresh_environment_is_not_masked_by_cli_override() {
  local stderr_file="$TMP_DIR/invalid-refresh.stderr"
  local current_jig
  current_jig="$(cd "$ROOT_DIR" && realpath "${JIG_DEV_BIN:-target/debug/jig}")"

  if JIG_INSTALL_REFRESH=bogus JIG_DEV_BIN="$current_jig" \
    /bin/bash "$ROOT_DIR/scripts/install-jig.sh" --contract-version 4 \
    --profile runtime --refresh --resolve-only "$TMP_DIR/invalid-refresh-root" \
    >/dev/null 2>"$stderr_file"; then
    echo "--refresh masked an invalid JIG_INSTALL_REFRESH value." >&2
    exit 1
  fi
  grep -q 'JIG_INSTALL_REFRESH must be 0 or 1' "$stderr_file"
}

validate_legacy_launcher_repair_seeds_current_runtime() {
  local template_snapshot="$TMP_DIR/template-legacy-repair-snapshot"
  local answers_file="$TMP_DIR/backend-legacy-repair.toml"
  local rendered_dir="$TMP_DIR/render-legacy-repair"
  local current_jig
  local doctor_output="$TMP_DIR/legacy-repair-doctor.out"

  current_jig="$(cd "$ROOT_DIR" && realpath "${JIG_DEV_BIN:-target/debug/jig}")"
  create_template_snapshot_repo "$template_snapshot"
  cp "$ROOT_DIR/tests/fixtures/backend-only.toml" "$answers_file"
  render_fixture_from_template "$template_snapshot" "$answers_file" "$rendered_dir"

  REPO_DIR="$rendered_dir" python3 <<'PY'
import json
import os
import pathlib

repo = pathlib.Path(os.environ["REPO_DIR"])
contract_path = repo / ".agent/jig-contract.json"
contract = json.loads(contract_path.read_text())
contract["contract_version"] = 3
contract["jig_version"] = "0.2.0-beta.1"
contract_path.write_text(json.dumps(contract, indent=2) + "\n")

answers = repo.joinpath(".jig.toml")
lines = answers.read_text().splitlines()
if not any(line.startswith("jig_version =") for line in lines):
    index = next(i for i, line in enumerate(lines) if line.startswith("default_branch = "))
    lines.insert(index + 1, 'jig_version = "0.2.0-beta.1"')
answers.write_text("\n".join(lines) + "\n")
PY
  printf '%s\n' '#!/bin/sh' 'JIG_VERSION="0.2.0-beta.1"' >"$rendered_dir/scripts/jig"
  printf '%s\n' '#!/usr/bin/env bash' 'JIG_VERSION="0.2.0-beta.1"' >"$rendered_dir/scripts/install-jig.sh"
  chmod +x "$rendered_dir/scripts/jig" "$rendered_dir/scripts/install-jig.sh"

  env -u JIG_DEV_BIN "$current_jig" update "$rendered_dir" --launcher-only --force >/dev/null
  grep -Fq 'CONTRACT_VERSION="3"' "$rendered_dir/scripts/jig"
  local runtime_stamp="$rendered_dir/.git/jig-tools/contract-3-runtime/.jig-source-stamp"
  local saved_runtime_stamp="$TMP_DIR/legacy-repair-source-stamp"
  grep -Fxq 'jig-seeded-runtime-v1' "$runtime_stamp"
  grep -Eq '^binary:sha256:[0-9a-f]{64}$' "$runtime_stamp"
  grep -Eq '^binary-identity:.+$' "$runtime_stamp"
  grep -Eq '^source:sha256:[0-9a-f]{64}$' "$runtime_stamp"

  local saved_answers="$TMP_DIR/legacy-repair-valid-answers.toml"
  local invalid_seed_root="$TMP_DIR/legacy-repair-invalid-config-seed"
  cp "$rendered_dir/.jig.toml" "$saved_answers"
  REPO_DIR="$rendered_dir" python3 <<'PY'
import os
import pathlib

answers = pathlib.Path(os.environ["REPO_DIR"]) / ".jig.toml"
answers.write_text(
    "\n".join(
        line for line in answers.read_text().splitlines()
        if not line.startswith("_src_path =")
    ) + "\n"
)
PY
  local quiet_resolve_stderr="$TMP_DIR/legacy-repair-quiet-resolve.stderr"
  local quiet_resolved
  quiet_resolved="$({
    cd "$rendered_dir"
    env -u JIG_DEV_BIN scripts/install-jig.sh --contract-version 3 \
      --repository-scope --profile runtime --resolve-only 2>"$quiet_resolve_stderr"
  })"
  [[ -x "$quiet_resolved" ]]
  cmp -s "$quiet_resolved" "$current_jig"
  [[ ! -s "$quiet_resolve_stderr" ]]

  local quiet_path_bin="$TMP_DIR/legacy-repair-quiet-path-bin"
  local quiet_saved_cache="$TMP_DIR/legacy-repair-quiet-saved-cache"
  mkdir -p "$quiet_path_bin"
  ln -s "$current_jig" "$quiet_path_bin/jig"
  mv "$rendered_dir/.git/jig-tools" "$quiet_saved_cache"
  quiet_resolved="$({
    cd "$rendered_dir"
    env -u JIG_DEV_BIN PATH="$quiet_path_bin:$PATH" JIG_INSTALL_ALLOW_PATH_BINARY=1 \
      scripts/install-jig.sh --contract-version 3 --repository-scope \
      --profile runtime --resolve-only 2>"$quiet_resolve_stderr"
  })"
  mv "$quiet_saved_cache" "$rendered_dir/.git/jig-tools"
  [[ "$(realpath "$quiet_resolved")" == "$(realpath "$current_jig")" ]]
  [[ ! -s "$quiet_resolve_stderr" ]]
  cp "$saved_answers" "$rendered_dir/.jig.toml"

  printf '%s\n' '{' >"$rendered_dir/.jig.toml"
  if (
    cd "$rendered_dir"
    JIG_DEV_BIN="$current_jig" scripts/install-jig.sh --contract-version 3 \
      --profile runtime --seed-dev-bin "$invalid_seed_root" >/dev/null 2>&1
  ); then
    echo "Repair seed accepted invalid configuration without truthful source provenance." >&2
    exit 1
  fi
  [[ ! -e "$invalid_seed_root/bin/jig" ]]
  cp "$saved_answers" "$rendered_dir/.jig.toml"

  local failed_stamp_root="$TMP_DIR/legacy-repair-failed-stamp"
  local failed_stamp_expected_bin="$TMP_DIR/legacy-repair-failed-stamp-expected-bin"
  local failed_stamp_bin="$TMP_DIR/legacy-repair-failed-stamp-bin"
  local real_mv
  mkdir -p "$failed_stamp_root/bin" "$failed_stamp_bin"
  printf '%s\n' '#!/bin/sh' '# prior cached binary' 'exit 0' >"$failed_stamp_root/bin/jig"
  chmod +x "$failed_stamp_root/bin/jig"
  cp "$failed_stamp_root/bin/jig" "$failed_stamp_expected_bin"
  printf '%s\n' 'prior-cache-provenance' >"$failed_stamp_root/.jig-source-stamp"
  real_mv="$(command -v mv)"
  printf '%s\n' \
    '#!/bin/sh' \
    'case "${1:-}:${2:-}" in' \
    '  *.jig-source-stamp.[0-9]*:*/.jig-source-stamp) exit 1 ;;' \
    'esac' \
    'exec "$JIG_FIXTURE_REAL_MV" "$@"' \
    >"$failed_stamp_bin/mv"
  chmod +x "$failed_stamp_bin/mv"
  if (
    cd "$rendered_dir"
    PATH="$failed_stamp_bin:$PATH" JIG_FIXTURE_REAL_MV="$real_mv" \
      JIG_DEV_BIN="$current_jig" scripts/install-jig.sh --contract-version 3 \
      --profile runtime --seed-dev-bin "$failed_stamp_root" >/dev/null 2>&1
  ); then
    echo "Repair seed reported success without recording cache provenance." >&2
    exit 1
  fi
  cmp -s "$failed_stamp_expected_bin" "$failed_stamp_root/bin/jig"
  [[ "$(cat "$failed_stamp_root/.jig-source-stamp")" == "prior-cache-provenance" ]]
  if compgen -G "$failed_stamp_root/.jig-*.seed-backup.*" >/dev/null \
    || compgen -G "$failed_stamp_root/bin/.jig-seed.*" >/dev/null \
    || compgen -G "$failed_stamp_root/bin/.jig-seed-bin-backup.*" >/dev/null; then
    echo "Failed seed left temporary publication artifacts in the managed cache." >&2
    exit 1
  fi

  (
    cd "$rendered_dir"
    env -u JIG_DEV_BIN scripts/jig --version >/dev/null
  )

  local runtime_bin="$rendered_dir/.git/jig-tools/contract-3-runtime/bin/jig"
  local runtime_install_root
  local identity_function published_identity stamped_identity
  runtime_install_root="$(dirname "$(dirname "$runtime_bin")")"
  identity_function="$(awk '
    /^binary_file_identity\(\) \{/ { capture = 1 }
    capture { print }
    capture && /^}$/ { exit }
  ' "$rendered_dir/scripts/install-jig.sh")"
  published_identity="$({
    BINARY_IDENTITY_FUNCTION="$identity_function" RUNTIME_BIN="$runtime_bin" \
      /bin/bash -c 'eval "$BINARY_IDENTITY_FUNCTION"; binary_file_identity "$RUNTIME_BIN"'
  })"
  stamped_identity="$(sed -n 's/^binary-identity://p' "$runtime_stamp")"
  if [[ -z "$published_identity" || "$stamped_identity" != "$published_identity" ]]; then
    echo "Launcher-repair stamp identity does not describe the published binary." >&2
    exit 1
  fi
  local stamp_with_identity="$TMP_DIR/legacy-repair-stamp-with-identity"
  cp "$runtime_stamp" "$stamp_with_identity"
  sed '/^binary-identity:/d' "$stamp_with_identity" >"$runtime_stamp"
  (
    cd "$rendered_dir"
    env -u JIG_DEV_BIN scripts/install-jig.sh --contract-version 3 \
      --profile runtime --resolve-only "$runtime_install_root" >/dev/null
  )
  if grep -q '^binary-identity:' "$runtime_stamp"; then
    echo "--resolve-only mutated launcher-repair source identity metadata." >&2
    exit 1
  fi
  (
    cd "$rendered_dir"
    env -u JIG_DEV_BIN scripts/install-jig.sh --contract-version 3 \
      --profile runtime "$runtime_install_root" >/dev/null
  )
  grep -Eq '^binary-identity:.+$' "$runtime_stamp"
  cp "$stamp_with_identity" "$runtime_stamp"

  local failing_stat_bin="$TMP_DIR/legacy-repair-failing-stat"
  mkdir -p "$failing_stat_bin"
  printf '%s\n' '#!/bin/sh' 'exit 1' >"$failing_stat_bin/stat"
  chmod +x "$failing_stat_bin/stat"
  (
    cd "$rendered_dir"
    env -u JIG_DEV_BIN PATH="$failing_stat_bin:$PATH" \
      scripts/install-jig.sh --contract-version 3 --profile runtime \
      --resolve-only "$runtime_install_root" >/dev/null
  )

  local identity_before identity_after
  identity_before="$(grep '^binary-identity:' "$runtime_stamp")"
  touch -t 200001010000 "$runtime_bin"
  mkdir "$runtime_install_root.lock"
  (
    trap 'rmdir "$runtime_install_root.lock"' EXIT
    cd "$rendered_dir"
    env -u JIG_DEV_BIN scripts/install-jig.sh --contract-version 3 \
      --profile runtime --resolve-only "$runtime_install_root" >/dev/null
    [[ "$(grep '^binary-identity:' "$runtime_stamp")" == "$identity_before" ]]
  )
  (
    cd "$rendered_dir"
    env -u JIG_DEV_BIN scripts/install-jig.sh --contract-version 3 \
      --profile runtime "$runtime_install_root" >/dev/null
  )
  identity_after="$(grep '^binary-identity:' "$runtime_stamp")"
  [[ "$identity_after" != "$identity_before" ]]

  local stamp_before_read_only_refresh runtime_cache_dir
  stamp_before_read_only_refresh="$(cat "$runtime_stamp")"
  runtime_cache_dir="$(dirname "$runtime_stamp")"
  touch -t 200101010000 "$runtime_bin"
  chmod 0555 "$runtime_cache_dir"
  (
    trap 'chmod 0755 "$runtime_cache_dir"' EXIT
    cd "$rendered_dir"
    env -u JIG_DEV_BIN scripts/install-jig.sh --contract-version 3 \
      --profile runtime --resolve-only "$runtime_install_root" >/dev/null
  )
  [[ "$(cat "$runtime_stamp")" == "$stamp_before_read_only_refresh" ]]
  (
    cd "$rendered_dir"
    env -u JIG_DEV_BIN scripts/install-jig.sh --contract-version 3 \
      --profile runtime "$runtime_install_root" >/dev/null
  )
  [[ "$(grep '^binary-identity:' "$runtime_stamp")" != "$identity_after" ]]

  cp "$runtime_stamp" "$saved_runtime_stamp"
  python3 - "$runtime_stamp" <<'PY'
import pathlib
import re
import sys

path = pathlib.Path(sys.argv[1])
stamp = path.read_text()
path.write_text(re.sub(r"^binary:sha256:[0-9a-f]{64}$", "binary:sha256:" + "0" * 64, stamp, flags=re.MULTILINE))
PY
  (
    cd "$rendered_dir"
    env -u JIG_DEV_BIN scripts/install-jig.sh --contract-version 3 \
      --profile runtime --resolve-only "$runtime_install_root" >/dev/null
  )
  cp "$saved_runtime_stamp" "$runtime_stamp"

  printf '%s\n' \
    '#!/bin/sh' \
    'if [ "${1:-}" = "__runtime-compatible" ]; then exit 0; fi' \
    'exit 1' \
    >"$runtime_bin"
  chmod +x "$runtime_bin"
  if (
    cd "$rendered_dir"
    env -u JIG_DEV_BIN scripts/install-jig.sh --contract-version 3 \
      --profile runtime --resolve-only "$runtime_install_root" >/dev/null
  ); then
    echo "Launcher-repair seed accepted a compatible binary with the wrong recorded digest." >&2
    exit 1
  fi
  cp "$current_jig" "$runtime_bin"
  chmod +x "$runtime_bin"

  printf '%s\n' '{' >"$rendered_dir/.agent/jig-contract.json"
  if (
    cd "$rendered_dir"
    env -u JIG_DEV_BIN scripts/jig doctor >"$doctor_output" 2>&1
  ); then
    echo "Doctor unexpectedly accepted a malformed contract manifest." >&2
    exit 1
  fi
  grep -Eq 'contract|Contract|jig-contract' "$doctor_output"
  if grep -q 'Failed to read a numeric contract_version' "$doctor_output"; then
    echo "Launcher blocked doctor before the runtime could diagnose the manifest." >&2
    exit 1
  fi
  local resolve_only_stderr="$TMP_DIR/legacy-repair-resolve-only-stderr"
  if (
    cd "$rendered_dir"
    env -u JIG_DEV_BIN scripts/install-jig.sh --contract-version 3 \
      --repository-scope --profile mcp --resolve-only >/dev/null 2>"$resolve_only_stderr"
  ); then
    echo "Repository-scoped resolve-only unexpectedly accepted a malformed contract manifest." >&2
    exit 1
  fi
  if [[ -s "$resolve_only_stderr" ]]; then
    echo "Repository-scoped resolve-only leaked a diagnostic instead of deferring to its caller." >&2
    cat "$resolve_only_stderr" >&2
    exit 1
  fi

  printf '%s\n' '// invalidate launcher-repair seed source state' \
    >>"$template_snapshot/crates/jig/src/main.rs"
  if (
    cd "$rendered_dir"
    env -u JIG_DEV_BIN scripts/install-jig.sh --contract-version 3 \
      --profile runtime --resolve-only >/dev/null
  ); then
    echo "Launcher-repair seed remained current after its recorded source changed." >&2
    exit 1
  fi
}

validate_source_normalization_fixtures() {
  validate_gnu_stat_fallback_rejects_successful_malformed_bsd_output
  validate_mutable_source_reminder_requires_matching_cache_lock
  validate_git_local_source_stamp_ignores_diff_helpers_and_rejects_symbolic_links
  validate_git_local_source_stamp_fails_closed_when_untracked_hashing_fails
  validate_unpushed_commit_stays_local
  validate_explicit_template_source_url_rewrites_src_path
  validate_quoted_local_src_path_installs_jig
  validate_relative_src_path_is_anchored_to_repository_root
  validate_template_source_url_installs_from_recorded_commit
  validate_launcher_contract_drift_fails_before_runtime_install
  validate_legacy_contract_uses_version_tag_only_as_source_locator
  validate_embedded_contract_fallback_requires_opt_in_and_uses_default_branch
  validate_recorded_remote_source_precedes_fallback_url
  validate_quoted_template_source_url_rewrites_src_path
  validate_contract_cache_tracks_remote_source_revision
  validate_non_git_local_source_cache_tracks_content_and_identity
  validate_unborn_git_local_source_cache_tracks_content
  validate_source_checkout_requires_explicit_dev_binary_and_honors_refresh
  validate_local_source_change_during_build_is_not_cached
  validate_invalid_config_info_reports_once
  validate_seed_checks_the_copy_before_cache_publication
  validate_path_launcher_is_not_treated_as_runtime
  validate_incompatible_dev_help_is_not_retried
  validate_missing_python_is_actionable
  validate_refresh_environment_is_not_masked_by_cli_override
  validate_legacy_launcher_repair_seeds_current_runtime
}

if [[ "${BASH_SOURCE[0]}" == "$0" ]]; then
  set -euo pipefail
  fixture_create_tmp_dir_if_needed
  export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$TMP_DIR/cargo-target}"

  validate_source_normalization_fixtures

  echo "Template source fixture validation passed."
fi
