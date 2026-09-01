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
    JIG_DEV_BIN="$jig_bin" "$jig_bin" init "$destination" \
      "$@" \
      --no-input \
      --no-vault \
      --json 2>&1
  )"; then
    echo "Failed to initialize generated Rust fixture at $destination" >&2
    printf '%s\n' "$init_output" >&2
    return 1
  fi
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
  local toolchain="$4"
  local network_policy="$5"
  local threshold_output

  cp "$root_dir/tests/fixtures/cognitive-complexity-over-threshold.rs" \
    "$repository/$member/src/cognitive_complexity_probe.rs"
  printf '\npub mod cognitive_complexity_probe;\n' >>"$repository/$member/src/lib.rs"

  if threshold_output="$(run_clippy "$repository" "$toolchain" "$network_policy" 2>&1)"; then
    echo "$label generated Clippy gate accepted the over-threshold fixture." >&2
    exit 1
  fi
  if ! grep -Eq 'cognitive complexity of \([0-9]+/20\)' <<<"$threshold_output"; then
    echo "$label generated Clippy gate failed without proving the threshold-20 policy." >&2
    printf '%s\n' "$threshold_output" >&2
    exit 1
  fi
}

rust_library_repo="$fixture_root/ExampleLibrary"
init_repo "$rust_library_repo" --preset rust-library
prepare_and_check "$rust_library_repo" "$rust_only_toolchain" offline
assert_threshold_rejected \
  "Rust library" \
  "$rust_library_repo" \
  "crates/examplelibrary" \
  "$rust_only_toolchain" \
  offline

rust_cli_repo="$fixture_root/ExampleCli"
init_repo "$rust_cli_repo" --preset rust-cli
prepare_and_check "$rust_cli_repo" "$rust_only_toolchain" offline

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
  "$rust_react_toolchain" \
  online

for database in sqlite postgres; do
  database_repo="$fixture_root/ExampleProject-${database}"
  init_repo "$database_repo" \
    --preset rust-react \
    --db "$database" \
    --frontends web,admin
  prepare_and_check "$database_repo" "$rust_react_toolchain" online
done

echo "Generated Rust Clippy validation passed."
