#!/usr/bin/env bash
set -euo pipefail

root_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)"
invocation_dir="$PWD"

resolve_invocation_path() {
  case "$1" in
    /*) printf '%s\n' "$1" ;;
    *) printf '%s/%s\n' "$invocation_dir" "$1" ;;
  esac
}

if [[ -n "${JIG_DEV_BIN:-}" ]]; then
  jig_bin="$(resolve_invocation_path "$JIG_DEV_BIN")"
elif [[ -n "${CARGO_TARGET_DIR:-}" ]]; then
  jig_bin="$(resolve_invocation_path "$CARGO_TARGET_DIR")/debug/jig"
else
  jig_bin="$root_dir/target/debug/jig"
fi

if [[ ! -x "$jig_bin" ]]; then
  echo "Build the development Jig binary before checking generated Rust scaffolds: cargo build -p jig-sh --bin jig" >&2
  echo "Resolved Jig binary: $jig_bin" >&2
  exit 1
fi

fixture_root="$(mktemp -d "${TMPDIR:-/tmp}/jig-generated-rust-clippy.XXXXXX")"
cleanup() {
  rm -rf -- "$fixture_root"
}
trap cleanup EXIT

# Rust/React init validates that its default package manager is available, but
# this check never executes JavaScript tooling. Keep the generated Bun default
# under test without coupling the Rust-only gate to a runner image's tool list.
package_manager_probe_dir="$fixture_root/package-manager-probe"
mkdir -p "$package_manager_probe_dir"
printf '%s\n' '#!/bin/sh' 'exit 0' >"$package_manager_probe_dir/bun"
chmod +x "$package_manager_probe_dir/bun"

# Share dependency artifacts across the generated repositories while keeping
# them outside every repository fingerprint and cleaning them with the fixture.
export CARGO_TARGET_DIR="$fixture_root/cargo-target"

rust_only_toolchain="${JIG_GENERATED_RUST_ONLY_TOOLCHAIN:-}"
rust_react_toolchain="${JIG_GENERATED_RUST_REACT_TOOLCHAIN:-}"

init_repo() {
  local destination="$1"
  local init_output
  shift
  if ! init_output="$(
    PATH="$package_manager_probe_dir:$PATH" JIG_DEV_BIN="$jig_bin" \
      "$jig_bin" init "$destination" \
      "$@" \
      --no-input \
      --no-vault \
      --json 2>&1
  )"; then
    echo "Failed to initialize generated Rust fixture at $destination" >&2
    printf '%s\n' "$init_output" >&2
    return 1
  fi
  # Jig's worktree-effect proof is commit-relative. Give each generated fixture
  # an explicit baseline so later probe edits exercise Clippy instead of the
  # unrelated unborn-branch behavior of a freshly initialized repository.
  git -C "$destination" -c user.name="Jig Fixture" \
    -c user.email="jig-fixture@example.invalid" add --all
  git -C "$destination" -c user.name="Jig Fixture" \
    -c user.email="jig-fixture@example.invalid" commit --quiet \
    --message="Initialize generated scaffold fixture"
}

with_toolchain() {
  local toolchain="$1"
  shift
  if [[ -n "$toolchain" ]]; then
    RUSTUP_TOOLCHAIN="$toolchain" "$@"
  else
    "$@"
  fi
}

prepare_and_check() {
  local repository="$1"
  local toolchain="$2"
  local network_policy="$3"
  (
    cd "$repository"
    case "$network_policy" in
      offline)
        CARGO_NET_OFFLINE=true with_toolchain "$toolchain" \
          cargo generate-lockfile --offline >/dev/null
        ;;
      online)
        # This deliberately models the dependency resolution performed after
        # a fresh Rust/React init, before the project commits its Cargo.lock.
        with_toolchain "$toolchain" cargo generate-lockfile >/dev/null
        ;;
      *)
        echo "Unknown generated Cargo network policy: $network_policy" >&2
        exit 1
        ;;
    esac
  )
  run_clippy "$repository" "$toolchain" "$network_policy" >/dev/null
}

run_clippy() {
  local repository="$1"
  local toolchain="$2"
  local network_policy="$3"
  (
    cd "$repository"
    case "$network_policy" in
      offline)
        CARGO_NET_OFFLINE=true JIG_DEV_BIN="$jig_bin" with_toolchain "$toolchain" \
          scripts/jig check clippy
        ;;
      online)
        JIG_DEV_BIN="$jig_bin" with_toolchain "$toolchain" scripts/jig check clippy
        ;;
      *)
        echo "Unknown generated Cargo network policy: $network_policy" >&2
        exit 1
        ;;
    esac
  )
}

assert_threshold_rejected() {
  local label="$1"
  local repository="$2"
  local member="$3"
  local entry_file="$4"
  local toolchain="$5"
  local network_policy="$6"
  local threshold_output
  local entry_backup

  entry_backup="$(mktemp "$fixture_root/jig-clippy-entry.XXXXXX")"
  cp "$repository/$member/$entry_file" "$entry_backup"
  cp "$root_dir/tests/fixtures/cognitive-complexity-over-threshold.rs" \
    "$repository/$member/src/cognitive_complexity_probe.rs"
  printf '\nmod cognitive_complexity_probe;\n' >>"$repository/$member/$entry_file"

  if threshold_output="$(run_clippy "$repository" "$toolchain" "$network_policy" 2>&1)"; then
    echo "$label generated Clippy gate accepted the over-threshold fixture." >&2
    exit 1
  fi
  if ! grep -Eq 'cognitive complexity of \([0-9]+/20\)' <<<"$threshold_output"; then
    echo "$label generated Clippy gate failed without proving the threshold-20 policy." >&2
    printf '%s\n' "$threshold_output" >&2
    exit 1
  fi
  mv "$entry_backup" "$repository/$member/$entry_file"
  rm -f -- "$repository/$member/src/cognitive_complexity_probe.rs"
}

assert_mod_module_files_rejected() {
  local label="$1"
  local repository="$2"
  local member="$3"
  local entry_file="$4"
  local toolchain="$5"
  local network_policy="$6"
  local policy_source="$7"
  local lint_output
  local answers_backup
  local entry_backup
  local feature_manifest
  local manifest_backup

  answers_backup="$(mktemp "$fixture_root/jig-clippy-answers.XXXXXX")"
  entry_backup="$(mktemp "$fixture_root/jig-clippy-entry.XXXXXX")"
  feature_manifest="$(mktemp "$fixture_root/jig-clippy-feature-manifest.XXXXXX")"
  manifest_backup="$(mktemp "$fixture_root/jig-clippy-manifest.XXXXXX")"
  cp "$repository/.jig.toml" "$answers_backup"
  cp "$repository/$member/$entry_file" "$entry_backup"
  cp "$repository/$member/Cargo.toml" "$manifest_backup"
  case "$policy_source" in
    command)
      # Prove the command-line denial remains effective even when a later member
      # accidentally omits workspace-lint inheritance.
      awk '
        /^\[lints\]$/ { in_lints = 1; next }
        in_lints && /^\[/ { in_lints = 0 }
        !in_lints { print }
      ' "$manifest_backup" >"$repository/$member/Cargo.toml"
      ;;
    workspace)
      # Prove the workspace lint is independently inherited when the generated
      # command does not explicitly deny mod_module_files.
      sed 's/ -D clippy::mod_module_files//g' "$answers_backup" \
        >"$repository/.jig.toml"
      ;;
    *)
      echo "Unknown mod_module_files policy source: $policy_source" >&2
      exit 1
      ;;
  esac
  if grep -q '^\[features\]$' "$repository/$member/Cargo.toml"; then
    awk '
      /^\[features\]$/ && !inserted { print; print "module-layout-probe = []"; inserted = 1; next }
      { print }
    ' "$repository/$member/Cargo.toml" >"$feature_manifest"
    mv "$feature_manifest" "$repository/$member/Cargo.toml"
  else
    printf '\n[features]\nmodule-layout-probe = []\n' \
      >>"$repository/$member/Cargo.toml"
  fi
  mkdir -p "$repository/$member/src/module_layout_probe"
  printf '%s\n' '#[allow(dead_code)]' 'pub fn probe() {}' \
    >"$repository/$member/src/module_layout_probe/mod.rs"
  printf '\n#[cfg(feature = "module-layout-probe")]\nmod module_layout_probe;\n' \
    >>"$repository/$member/$entry_file"

  if lint_output="$(run_clippy "$repository" "$toolchain" "$network_policy" 2>&1)"; then
    echo "$label generated Clippy gate accepted a feature-gated mod.rs module layout via $policy_source policy." >&2
    exit 1
  fi
  if ! grep -Eq 'clippy::mod[-_]module[-_]files' <<<"$lint_output"; then
    echo "$label generated Clippy gate failed without proving the mod_module_files policy." >&2
    printf '%s\n' "$lint_output" >&2
    exit 1
  fi
  mv "$answers_backup" "$repository/.jig.toml"
  mv "$entry_backup" "$repository/$member/$entry_file"
  mv "$manifest_backup" "$repository/$member/Cargo.toml"
  rm -rf -- "$repository/$member/src/module_layout_probe"
}

assert_mutually_exclusive_features_opt_out() {
  local repository="$1"
  local member="$2"
  local entry_file="$3"
  local toolchain="$4"
  local network_policy="$5"
  local conflict_output
  local answers_backup
  local entry_backup
  local feature_manifest
  local manifest_backup
  local updated_answers

  answers_backup="$(mktemp "$fixture_root/jig-clippy-answers.XXXXXX")"
  entry_backup="$(mktemp "$fixture_root/jig-clippy-entry.XXXXXX")"
  feature_manifest="$(mktemp "$fixture_root/jig-clippy-feature-manifest.XXXXXX")"
  manifest_backup="$(mktemp "$fixture_root/jig-clippy-manifest.XXXXXX")"
  updated_answers="$(mktemp "$fixture_root/jig-clippy-answers-updated.XXXXXX")"
  cp "$repository/.jig.toml" "$answers_backup"
  cp "$repository/$member/$entry_file" "$entry_backup"
  cp "$repository/$member/Cargo.toml" "$manifest_backup"
  if grep -q '^\[features\]$' "$repository/$member/Cargo.toml"; then
    awk '
      /^\[features\]$/ && !inserted {
        print
        print "exclusive-a = []"
        print "exclusive-b = []"
        inserted = 1
        next
      }
      { print }
    ' "$repository/$member/Cargo.toml" >"$feature_manifest"
    mv "$feature_manifest" "$repository/$member/Cargo.toml"
  else
    printf '\n[features]\nexclusive-a = []\nexclusive-b = []\n' \
      >>"$repository/$member/Cargo.toml"
  fi
  printf '\n#[cfg(all(feature = "exclusive-a", feature = "exclusive-b"))]\ncompile_error!("mutually exclusive feature probe");\n' \
    >>"$repository/$member/$entry_file"

  if conflict_output="$(run_clippy "$repository" "$toolchain" "$network_policy" 2>&1)"; then
    echo "Generated Clippy gate did not check the mutually exclusive feature combination." >&2
    exit 1
  fi
  if ! grep -q 'mutually exclusive feature probe' <<<"$conflict_output"; then
    echo "Generated Clippy gate failed without proving all-feature coverage." >&2
    printf '%s\n' "$conflict_output" >&2
    exit 1
  fi

  sed 's/--all-features //g' "$answers_backup" >"$updated_answers"
  mv "$updated_answers" "$repository/.jig.toml"
  run_clippy "$repository" "$toolchain" "$network_policy" >/dev/null

  mv "$answers_backup" "$repository/.jig.toml"
  mv "$entry_backup" "$repository/$member/$entry_file"
  mv "$manifest_backup" "$repository/$member/Cargo.toml"
}

rust_library_repo="$fixture_root/ExampleLibrary"
init_repo "$rust_library_repo" --preset rust-library
prepare_and_check "$rust_library_repo" "$rust_only_toolchain" offline
assert_threshold_rejected \
  "Rust library" \
  "$rust_library_repo" \
  "crates/examplelibrary" \
  "src/lib.rs" \
  "$rust_only_toolchain" \
  offline
assert_mod_module_files_rejected \
  "Rust library command-line lint" \
  "$rust_library_repo" \
  "crates/examplelibrary" \
  "src/lib.rs" \
  "$rust_only_toolchain" \
  offline \
  command
assert_mod_module_files_rejected \
  "Rust library inherited workspace lint" \
  "$rust_library_repo" \
  "crates/examplelibrary" \
  "src/lib.rs" \
  "$rust_only_toolchain" \
  offline \
  workspace
assert_mutually_exclusive_features_opt_out \
  "$rust_library_repo" \
  "crates/examplelibrary" \
  "src/lib.rs" \
  "$rust_only_toolchain" \
  offline

rust_cli_repo="$fixture_root/ExampleCli"
init_repo "$rust_cli_repo" --preset rust-cli
prepare_and_check "$rust_cli_repo" "$rust_only_toolchain" offline
assert_threshold_rejected \
  "Rust CLI" \
  "$rust_cli_repo" \
  "crates/examplecli" \
  "src/main.rs" \
  "$rust_only_toolchain" \
  offline
assert_mod_module_files_rejected \
  "Rust CLI command-line lint" \
  "$rust_cli_repo" \
  "crates/examplecli" \
  "src/main.rs" \
  "$rust_only_toolchain" \
  offline \
  command
assert_mod_module_files_rejected \
  "Rust CLI inherited workspace lint" \
  "$rust_cli_repo" \
  "crates/examplecli" \
  "src/main.rs" \
  "$rust_only_toolchain" \
  offline \
  workspace
rust_react_repo="$fixture_root/ExampleProject"
init_repo "$rust_react_repo" \
  --preset rust-react \
  --db none \
  --frontends web
prepare_and_check "$rust_react_repo" "$rust_react_toolchain" online
assert_threshold_rejected \
  "Rust/React" \
  "$rust_react_repo" \
  "crates/exampleproject-core" \
  "src/lib.rs" \
  "$rust_react_toolchain" \
  online
assert_mod_module_files_rejected \
  "Rust/React inherited workspace lint" \
  "$rust_react_repo" \
  "crates/exampleproject-core" \
  "src/lib.rs" \
  "$rust_react_toolchain" \
  online \
  workspace

for database in sqlite postgres; do
  database_repo="$fixture_root/ExampleProject-${database}"
  init_repo "$database_repo" \
    --preset rust-react \
    --repo-name ExampleProject \
    --db "$database" \
    --frontends web,admin
  prepare_and_check "$database_repo" "$rust_react_toolchain" online
  if [[ "$database" == sqlite ]]; then
    policy_source=command
  else
    policy_source=workspace
  fi
  assert_mod_module_files_rejected \
    "Rust/React $database $policy_source lint" \
    "$database_repo" \
    "crates/exampleproject-core" \
    "src/lib.rs" \
    "$rust_react_toolchain" \
    online \
    "$policy_source"
done

echo "Generated Rust Clippy validation passed."
