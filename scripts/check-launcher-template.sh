#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)"

rust_string_const() {
  python3 - "$1" "$2" <<'PY'
import pathlib
import re
import sys

source = pathlib.Path(sys.argv[1]).read_text()
name = re.escape(sys.argv[2])
match = re.search(
    rf'(?m)^[ \t]*(?:pub\(crate\)[ \t]+)?const[ \t]+{name}[ \t]*:[ \t]*&str[ \t]*=[ \t\r\n]*"([^"\n]*)"[ \t]*;',
    source,
)
if match:
    print(match.group(1))
PY
}

manifest_contract_version="$(python3 - "$ROOT_DIR/.agent/jig-contract.json" <<'PY'
import json
import pathlib
import sys

value = json.loads(pathlib.Path(sys.argv[1]).read_text()).get("contract_version")
if not isinstance(value, int) or isinstance(value, bool) or value < 0:
    raise SystemExit("contract_version must be a non-negative integer")
print(value)
PY
)"
launcher_contract_version="$(sed -n 's/^CONTRACT_VERSION="\([0-9][0-9]*\)"$/\1/p' "$ROOT_DIR/scripts/jig")"
if [[ -z "$launcher_contract_version" || "$launcher_contract_version" != "$manifest_contract_version" ]]; then
  echo "scripts/jig contract version ${launcher_contract_version:-<unreadable>} does not match .agent/jig-contract.json contract $manifest_contract_version." >&2
  exit 1
fi
rust_current_contract="$(sed -n 's/^pub(crate) const CURRENT_CONTRACT_VERSION: u32 = \([0-9][0-9]*\);$/\1/p' "$ROOT_DIR/crates/jig/src/context.rs")"
if [[ -z "$rust_current_contract" || "$rust_current_contract" != "$manifest_contract_version" ]]; then
  echo "Rust current contract ${rust_current_contract:-<unreadable>} does not match .agent/jig-contract.json contract $manifest_contract_version." >&2
  exit 1
fi

launcher_global_flags="$(sed -n 's/^# jig-global-flags://p' "$ROOT_DIR/scripts/jig")"
rust_global_flags="$(sed -n 's/^const LAUNCHER_GLOBAL_FLAGS: &str = "\([^"]*\)";$/\1/p' "$ROOT_DIR/crates/jig/src/cli.rs")"
launcher_global_case_flags="$(awk '
  /^[[:space:]]*# jig-global-flags-begin$/ { capture = 1; next }
  capture {
    line = $0
    sub(/^[[:space:]]*/, "", line)
    if (line ~ /\)$/) {
      sub(/[[:space:]]*\)$/, "", line)
      gsub(/[[:space:]]*\|[[:space:]]*/, ",", line)
      print line
      exit
    }
  }
' "$ROOT_DIR/scripts/jig")"
if [[ -z "$launcher_global_flags" || -z "$rust_global_flags" \
  || "$launcher_global_flags" != "$rust_global_flags" \
  || "$launcher_global_case_flags" != "$launcher_global_flags" ]]; then
  echo "Launcher global options '${launcher_global_case_flags:-<unreadable>}' do not match its marker '${launcher_global_flags:-<unreadable>}' and Clap's tested globals '${rust_global_flags:-<unreadable>}' in crates/jig/src/cli.rs." >&2
  exit 1
fi

launcher_capability_subcommands="$(sed -n 's/^# jig-capability-only-subcommands://p' "$ROOT_DIR/scripts/jig")"
rust_capability_subcommands="$(rust_string_const "$ROOT_DIR/crates/jig/src/cli.rs" LAUNCHER_CAPABILITY_ONLY_SUBCOMMANDS)"
launcher_capability_case_subcommands="$(awk '
  /^[[:space:]]*# jig-capability-only-subcommands-begin$/ { capture = 1; next }
  capture {
    line = $0
    sub(/^[[:space:]]*/, "", line)
    if (line ~ /\)$/) {
      sub(/[[:space:]]*\)$/, "", line)
      gsub(/[[:space:]]*\|[[:space:]]*/, ",", line)
      print line
      exit
    }
  }
' "$ROOT_DIR/scripts/jig")"
if [[ -z "$launcher_capability_subcommands" || -z "$rust_capability_subcommands" \
  || "$launcher_capability_subcommands" != "$rust_capability_subcommands" \
  || "$launcher_capability_case_subcommands" != "$launcher_capability_subcommands" ]]; then
  echo "Launcher capability-only subcommands '${launcher_capability_case_subcommands:-<unreadable>}' do not match its marker '${launcher_capability_subcommands:-<unreadable>}' and Clap's tested policy '${rust_capability_subcommands:-<unreadable>}' in crates/jig/src/cli.rs." >&2
  exit 1
fi

launcher_repository_subcommands="$(sed -n 's/^# jig-repository-scope-subcommands://p' "$ROOT_DIR/scripts/jig")"
rust_repository_subcommands="$(rust_string_const "$ROOT_DIR/crates/jig/src/cli.rs" LAUNCHER_REPOSITORY_SCOPE_SUBCOMMANDS)"
launcher_repository_case_subcommands="$(awk '
  /^[[:space:]]*# jig-repository-scope-subcommands-begin$/ { capture = 1; next }
  capture {
    line = $0
    sub(/^[[:space:]]*/, "", line)
    if (line ~ /\)$/) {
      sub(/[[:space:]]*\)$/, "", line)
      gsub(/[[:space:]]*\|[[:space:]]*/, ",", line)
      print line
      exit
    }
  }
' "$ROOT_DIR/scripts/jig")"
normalized_repository_marker="$(printf '%s\n' "$launcher_repository_subcommands" | tr ',' '\n' | sort | paste -sd, -)"
normalized_repository_case="$(
  printf '%s\n' "check,$launcher_repository_case_subcommands" | tr ',' '\n' | sort | paste -sd, -
)"
if [[ -z "$launcher_repository_subcommands" || -z "$rust_repository_subcommands" \
  || "$launcher_repository_subcommands" != "$rust_repository_subcommands" \
  || "$normalized_repository_case" != "$normalized_repository_marker" ]]; then
  echo "Launcher repository-scoped subcommands '${launcher_repository_case_subcommands:-<unreadable>}' plus check do not match its marker '${launcher_repository_subcommands:-<unreadable>}' and Clap's tested policy '${rust_repository_subcommands:-<unreadable>}' in crates/jig/src/cli.rs." >&2
  exit 1
fi

launcher_check_subcommands="$(sed -n 's/^# jig-check-subcommands://p' "$ROOT_DIR/scripts/jig")"
rust_check_subcommands="$(rust_string_const "$ROOT_DIR/crates/jig/src/cli.rs" LAUNCHER_CHECK_SUBCOMMANDS)"
if [[ -z "$launcher_check_subcommands" || -z "$rust_check_subcommands" \
  || "$launcher_check_subcommands" != "$rust_check_subcommands" ]]; then
  echo "Launcher check subcommands '${launcher_check_subcommands:-<unreadable>}' do not match Clap's tested list '${rust_check_subcommands:-<unreadable>}' in crates/jig/src/cli.rs." >&2
  exit 1
fi

launcher_strict_check_subcommands="$(awk '
  /^[[:space:]]*# jig-strict-check-subcommands-begin$/ { capture = 1; next }
  capture {
    line = $0
    sub(/^[[:space:]]*/, "", line)
    if (line ~ /\)$/) {
      sub(/[[:space:]]*\)$/, "", line)
      gsub(/[[:space:]]*\|[[:space:]]*/, ",", line)
      print line
      exit
    }
  }
' "$ROOT_DIR/scripts/jig")"
wrapped_check_subcommands=",${launcher_check_subcommands},"
without_contract="${wrapped_check_subcommands/,contract,/,}"
if [[ "$without_contract" == "$wrapped_check_subcommands" ]]; then
  echo "Launcher check subcommands do not declare the capability-only contract gate." >&2
  exit 1
fi
expected_strict_check_subcommands="${without_contract#,}"
expected_strict_check_subcommands="${expected_strict_check_subcommands%,}"
if [[ -z "$launcher_strict_check_subcommands" \
  || "$launcher_strict_check_subcommands" != "$expected_strict_check_subcommands" ]]; then
  echo "Launcher's strict check case '${launcher_strict_check_subcommands:-<unreadable>}' does not match all declared check subcommands except the capability-only contract gate '${expected_strict_check_subcommands}'." >&2
  exit 1
fi

launcher_generated_marker="$(sed -n '/^# jig-generated-runtime-launcher:v[0-9][0-9]*$/p' "$ROOT_DIR/scripts/jig")"
installer_generated_marker="$(sed -n '/^# jig-generated-runtime-installer:v[0-9][0-9]*$/p' "$ROOT_DIR/scripts/install-jig.sh")"
launcher_scope_marker="$(sed -n '/^# jig-runtime-repository-scope:v[0-9][0-9]*$/p' "$ROOT_DIR/scripts/jig")"
installer_scope_marker="$(sed -n '/^# jig-runtime-repository-scope:v[0-9][0-9]*$/p' "$ROOT_DIR/scripts/install-jig.sh")"
rust_launcher_generated_marker="$(rust_string_const "$ROOT_DIR/crates/jig/src/runtime_artifacts.rs" GENERATED_RUNTIME_LAUNCHER_MARKER)"
rust_installer_generated_marker="$(rust_string_const "$ROOT_DIR/crates/jig/src/runtime_artifacts.rs" GENERATED_RUNTIME_INSTALLER_MARKER)"
rust_scope_marker="$(rust_string_const "$ROOT_DIR/crates/jig/src/runtime_artifacts.rs" RUNTIME_REPOSITORY_SCOPE_MARKER)"
if [[ -z "$launcher_generated_marker" \
  || "$launcher_generated_marker" != "$rust_launcher_generated_marker" ]]; then
  echo "Launcher protocol marker '${launcher_generated_marker:-<unreadable>}' does not match Rust recognition marker '${rust_launcher_generated_marker:-<unreadable>}'." >&2
  exit 1
fi
if [[ -z "$installer_generated_marker" \
  || "$installer_generated_marker" != "$rust_installer_generated_marker" ]]; then
  echo "Installer protocol marker '${installer_generated_marker:-<unreadable>}' does not match Rust recognition marker '${rust_installer_generated_marker:-<unreadable>}'." >&2
  exit 1
fi
if [[ -z "$launcher_scope_marker" || -z "$installer_scope_marker" \
  || "$launcher_scope_marker" != "$installer_scope_marker" \
  || "$launcher_scope_marker" != "$rust_scope_marker" ]]; then
  echo "Launcher/installer repository-scope markers do not match Rust recognition marker '${rust_scope_marker:-<unreadable>}'." >&2
  exit 1
fi

installer_cache_layout="$(sed -n 's/^# jig-runtime-cache-layout://p' "$ROOT_DIR/scripts/install-jig.sh")"
rust_cache_layout_value="$(rust_string_const "$ROOT_DIR/crates/jig/src/context.rs" INSTALLER_CACHE_LAYOUT_MARKER)"
if [[ -z "$rust_cache_layout_value" ]]; then
  echo "Could not read INSTALLER_CACHE_LAYOUT_MARKER from crates/jig/src/context.rs." >&2
  exit 1
fi
if [[ -z "$installer_cache_layout" \
  || "$installer_cache_layout" != "$rust_cache_layout_value" ]]; then
  echo "Installer cache layout '${installer_cache_layout:-<unreadable>}' does not match Rust's shared layout '${rust_cache_layout_value:-<unreadable>}' in crates/jig/src/context.rs." >&2
  exit 1
fi
installer_cache_lock="$(sed -n 's/^# jig-runtime-cache-lock://p' "$ROOT_DIR/scripts/install-jig.sh")"
rust_cache_lock="$(rust_string_const "$ROOT_DIR/crates/jig/src/runtime_cache_lock.rs" INSTALLER_CACHE_LOCK_PROTOCOL_MARKER)"
installer_lock_attempts="$(sed -n 's/^INSTALL_LOCK_ATTEMPTS=\([0-9][0-9]*\)$/\1/p' "$ROOT_DIR/scripts/install-jig.sh")"
installer_lock_retry="$(sed -n 's/^INSTALL_LOCK_RETRY_SECONDS=\([0-9][0-9]*\)$/\1/p' "$ROOT_DIR/scripts/install-jig.sh")"
installer_lock_from_values="directory-suffix=.lock;guard-suffix=.lock.guard;mechanism=os-exclusive+legacy-directory;record=owner-v1;attempts=$installer_lock_attempts;retry-seconds=$installer_lock_retry"
if [[ -z "$installer_cache_lock" || -z "$rust_cache_lock" \
  || "$installer_cache_lock" != "$rust_cache_lock" \
  || "$installer_cache_lock" != "$installer_lock_from_values" ]]; then
  echo "Installer cache-lock protocol '${installer_lock_from_values:-<unreadable>}' does not match its marker '${installer_cache_lock:-<unreadable>}' and Rust's publication-lock protocol '${rust_cache_lock:-<unreadable>}'." >&2
  exit 1
fi
if ! grep -Fxq 'INSTALL_LOCK_PATH="$INSTALL_ROOT.lock"' "$ROOT_DIR/scripts/install-jig.sh"; then
  echo "Installer cache-lock directory no longer follows the declared .lock suffix protocol." >&2
  exit 1
fi
if ! grep -Fxq 'INSTALL_LOCK_GUARD_PATH="$INSTALL_ROOT.lock.guard"' "$ROOT_DIR/scripts/install-jig.sh"; then
  echo "Installer cache-lock guard no longer follows the declared .lock.guard suffix protocol." >&2
  exit 1
fi
for installer_lock_protocol_fragment in \
  'fcntl.flock(file_descriptor, fcntl.LOCK_EX | fcntl.LOCK_NB)' \
  'record = f"owner-v1 {os.getpid()} {token}\n".encode("ascii")' \
  'os.mkdir(lock_directory)' \
  'except FileExistsError:' \
  'pass_fds=(file_descriptor,)'; do
  if ! grep -Fq "$installer_lock_protocol_fragment" "$ROOT_DIR/scripts/install-jig.sh"; then
    echo "Installer cache-lock implementation is missing protocol fragment: $installer_lock_protocol_fragment" >&2
    exit 1
  fi
done
for rust_lock_protocol_fragment in \
  'FileExt::try_lock_exclusive(&guard)' \
  'Err(error) if error.kind() == ErrorKind::AlreadyExists => Ok(false)'; do
  if ! grep -Fq "$rust_lock_protocol_fragment" "$ROOT_DIR/crates/jig/src/runtime_cache_lock.rs"; then
    echo "Rust cache-lock implementation is missing protocol fragment: $rust_lock_protocol_fragment" >&2
    exit 1
  fi
done
installer_original_args_expansion='${INSTALLER_ORIGINAL_ARGS[@]+"${INSTALLER_ORIGINAL_ARGS[@]}"}'
if ! grep -Fq "$installer_original_args_expansion" "$ROOT_DIR/scripts/install-jig.sh"; then
  echo "Installer recursive lock handoff is not safe for an empty argument array under Bash 3.2." >&2
  exit 1
fi
if ! /bin/bash -uc '
forward() { [[ "$#" -eq 0 ]]; }
INSTALLER_ORIGINAL_ARGS=("$@")
forward "${INSTALLER_ORIGINAL_ARGS[@]+"${INSTALLER_ORIGINAL_ARGS[@]}"}"
' jig; then
  echo "Stock /bin/bash could not forward an empty installer argument array under nounset." >&2
  exit 1
fi
installer_source_checkout_cache_checks="$(
  grep -c '&& local_source_install_is_current' "$ROOT_DIR/scripts/install-jig.sh"
)"
if [[ "$installer_source_checkout_cache_checks" != "2" ]]; then
  echo "Source-checkout installer flow must validate only the full-profile candidate and the requested-profile candidate once each; found $installer_source_checkout_cache_checks checks." >&2
  exit 1
fi
for contract_template in \
  "$ROOT_DIR/templates/project/.agent/jig-contract.json.jinja" \
  "$ROOT_DIR/crates/jig/src/bootstrap/embedded_template_snapshots/.agent/jig-contract.json.jinja"
do
  if ! grep -Fxq '  "contract_version": <<[ _jig.contract_version ]>>,' "$contract_template"; then
    echo "$contract_template must render contract_version from _jig.contract_version." >&2
    exit 1
  fi
done

if ! cmp -s \
  "$ROOT_DIR/templates/project/.agent/jig-contract.json.jinja" \
  "$ROOT_DIR/crates/jig/src/bootstrap/embedded_template_snapshots/.agent/jig-contract.json.jinja"
then
  echo "Generated and embedded contract-manifest templates drifted." >&2
  exit 1
fi

legacy_cutoff="$(sed -n 's/^LEGACY_VERSION_LOCK_CUTOFF=\([0-9][0-9]*\)$/\1/p' "$ROOT_DIR/scripts/install-jig.sh")"
rust_last_locked="$(sed -n 's/^pub(crate) const LAST_VERSION_LOCKED_CONTRACT_VERSION: u32 = \([0-9][0-9]*\);$/\1/p' "$ROOT_DIR/crates/jig/src/context.rs")"
if [[ -z "$legacy_cutoff" || -z "$rust_last_locked" \
  || "$legacy_cutoff" -ne $((rust_last_locked + 1)) ]]; then
  echo "Installer legacy cutoff ${legacy_cutoff:-<unreadable>} does not follow Rust's last version-locked contract ${rust_last_locked:-<unreadable>}." >&2
  exit 1
fi

normalize_launcher() {
  local input_path="$1"
  local output_path="$2"
  sed \
    -e '/^# Keep launcher behavior synchronized /{
N
d
}' \
    -e '/^\[% if repo_name == "jig-sh" %\]$/d' \
    -e '/^\[% endif %\]$/d' \
    -e 's/^CONTRACT_VERSION="[^"]*"$/CONTRACT_VERSION="<<[ _jig.contract_version ]>>"/' \
    "$input_path" >"$output_path"
  if [[ ! -s "$output_path" ]]; then
    echo "Failed to normalize launcher $input_path." >&2
    return 1
  fi
}

if ! grep -Fxq 'CONTRACT_VERSION="<<[ _jig.contract_version ]>>"' \
  "$ROOT_DIR/templates/project/scripts/jig.jinja"
then
  echo "Generated launcher template must render CONTRACT_VERSION from _jig.contract_version." >&2
  exit 1
fi

normalized_dir="$(mktemp -d "${TMPDIR:-/tmp}/jig-launcher-parity.XXXXXX")"
trap 'rm -rf "$normalized_dir"' EXIT
normalize_launcher "$ROOT_DIR/scripts/jig" "$normalized_dir/current"
normalize_launcher \
  "$ROOT_DIR/templates/project/scripts/jig.jinja" \
  "$normalized_dir/template"

if ! diff -u "$normalized_dir/current" "$normalized_dir/template"
then
  echo "scripts/jig and templates/project/scripts/jig.jinja drifted." >&2
  exit 1
fi

if ! cmp -s \
  "$ROOT_DIR/templates/project/scripts/jig.jinja" \
  "$ROOT_DIR/crates/jig/src/bootstrap/embedded_template_snapshots/scripts/jig.jinja"
then
  echo "Generated and embedded launcher templates drifted." >&2
  exit 1
fi

if ! cmp -s \
  "$ROOT_DIR/scripts/install-jig.sh" \
  "$ROOT_DIR/templates/project/scripts/install-jig.sh.jinja"
then
  echo "scripts/install-jig.sh and templates/project/scripts/install-jig.sh.jinja drifted." >&2
  exit 1
fi

if ! cmp -s \
  "$ROOT_DIR/templates/project/scripts/install-jig.sh.jinja" \
  "$ROOT_DIR/crates/jig/src/bootstrap/embedded_template_snapshots/scripts/install-jig.sh.jinja"
then
  echo "Generated and embedded installer templates drifted." >&2
  exit 1
fi
