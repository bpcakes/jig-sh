#!/usr/bin/env bash
set -euo pipefail
INSTALLER_ORIGINAL_ARGS=("$@")
# jig-generated-runtime-installer:v1
# jig-runtime-repository-scope:v1
# jig-runtime-cache-layout:git=.git/jig-tools;fallback=.agent/.cache/jig;runtime-suffix=-runtime
# jig-runtime-cache-lock:directory-suffix=.lock;guard-suffix=.lock.guard;mechanism=os-exclusive+legacy-directory;record=owner-v1;attempts=30;retry-seconds=1

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)"
ANSWERS_FILE="$ROOT_DIR/.jig.toml"
CONTRACT_MANIFEST="$ROOT_DIR/.agent/jig-contract.json"
LEGACY_VERSION_LOCK_CUTOFF=4

require_python3() {
  if ! command -v python3 >/dev/null 2>&1; then
    echo "Python 3 is required by scripts/install-jig.sh to read Jig configuration and fingerprint local sources." >&2
    exit 1
  fi
}

read_config_fields() {
  local contract_manifest="${1:-}"
  # Repository-scoped callers pass the manifest so one Python startup reads
  # both fixed metadata files before normal launcher dispatch.
  python3 -c '
import ast
import json
import pathlib
import re
import sys

try:
    import tomllib
except ModuleNotFoundError:
    tomllib = None

contract_manifest = sys.argv[2]
keys = sys.argv[3:]
if contract_manifest:
    try:
        contract_version = json.loads(pathlib.Path(contract_manifest).read_text()).get("contract_version")
    except (OSError, UnicodeError, ValueError) as error:
        print(f"Failed to read contract version from {contract_manifest}: {error}", file=sys.stderr)
        raise SystemExit(1)
    if not isinstance(contract_version, int) or isinstance(contract_version, bool) or contract_version < 0:
        print("contract_version must be a non-negative integer.", file=sys.stderr)
        raise SystemExit(1)
    sys.stdout.buffer.write(str(contract_version).encode("utf-8") + b"\0")

try:
    text = pathlib.Path(sys.argv[1]).read_text()
except (OSError, UnicodeError) as error:
    print(f"Failed to read configuration from {sys.argv[1]}: {error}", file=sys.stderr)
    raise SystemExit(1)

if tomllib is not None:
    try:
        document = tomllib.loads(text)
    except (TypeError, ValueError) as error:
        print(f"Failed to parse {sys.argv[1]}: {error}", file=sys.stderr)
        raise SystemExit(1)
    values = [document.get(key, "") for key in keys]
else:
    # The fallback intentionally reads only top-level scalar string answers used
    # by this launcher. tomllib remains authoritative when available.
    def strip_inline_comment(value):
        quote = None
        escaped = False
        for index, char in enumerate(value):
            if quote == chr(34) and escaped:
                escaped = False
                continue
            if quote == chr(34) and char == "\\":
                escaped = True
                continue
            if quote is not None:
                if char == quote:
                    quote = None
                continue
            if char in {chr(39), chr(34)}:
                quote = char
                continue
            if char == "#":
                return value[:index].rstrip()
        return value.strip()

    values_by_key = {}
    patterns = {
        key: re.compile(rf"^\s*{re.escape(key)}\s*=\s*(.*?)\s*$") for key in keys
    }
    for line in text.splitlines():
        stripped = line.strip()
        if not stripped or stripped.startswith("#"):
            continue
        if stripped.startswith("["):
            break
        for key, pattern in patterns.items():
            match = pattern.match(line)
            if not match:
                continue
            if key in values_by_key:
                print(f"Duplicate top-level key {key} in {sys.argv[1]}.", file=sys.stderr)
                raise SystemExit(1)
            raw_value = strip_inline_comment(match.group(1))
            try:
                if raw_value.startswith(chr(39)):
                    if len(raw_value) < 2 or not raw_value.endswith(chr(39)) \
                        or chr(39) in raw_value[1:-1]:
                        raise ValueError("invalid TOML literal string")
                    values_by_key[key] = raw_value[1:-1]
                else:
                    values_by_key[key] = ast.literal_eval(raw_value)
            except (SyntaxError, ValueError) as error:
                print(f"Failed to parse {key} from {sys.argv[1]}: {error}", file=sys.stderr)
                raise SystemExit(1)
            break
    values = [values_by_key.get(key, "") for key in keys]

for key, value in zip(keys, values):
    if value is None:
        value = ""
    if not isinstance(value, str):
        print(f"Unsupported non-string value for {key}.", file=sys.stderr)
        raise SystemExit(1)
    sys.stdout.buffer.write(value.encode("utf-8") + b"\0")
' "$ANSWERS_FILE" "$contract_manifest" jig_version _src_path _commit template_source_url
}

read_config_fields_for_mode() {
  if [[ "$RESOLVE_ONLY" == "1" ]]; then
    read_config_fields "$@" 2>/dev/null
  else
    read_config_fields "$@"
  fi
}

read_contract_version() {
  python3 -c '
import json
import pathlib
import sys

value = json.loads(pathlib.Path(sys.argv[1]).read_text()).get("contract_version")
if not isinstance(value, int) or isinstance(value, bool) or value < 0:
    print("contract_version must be a non-negative integer.", file=sys.stderr)
    raise SystemExit(1)
print(value)
' "$CONTRACT_MANIFEST"
}

INSTALL_PROFILE="${JIG_INSTALL_PROFILE:-default}"
INSTALL_ROOT_ARG=""
RESOLVE_ONLY=0
SEED_DEV_BIN=0
REPOSITORY_SCOPE=0
REFRESH_CACHE="${JIG_INSTALL_REFRESH:-0}"
CONTRACT_VERSION=""
CONTRACT_VERSION_ARG_SET=0
case "$REFRESH_CACHE" in
  0 | 1) ;;
  *)
    echo "JIG_INSTALL_REFRESH must be 0 or 1." >&2
    exit 2
    ;;
esac
while [[ "$#" -gt 0 ]]; do
  case "$1" in
    --profile)
      if [[ "$#" -lt 2 ]]; then
        echo "--profile requires a value." >&2
        exit 2
      fi
      INSTALL_PROFILE="$2"
      shift 2
      ;;
    --profile=*)
      INSTALL_PROFILE="${1#--profile=}"
      shift
      ;;
    --contract-version)
      if [[ "$#" -lt 2 ]]; then
        echo "--contract-version requires a value." >&2
        exit 2
      fi
      CONTRACT_VERSION="$2"
      CONTRACT_VERSION_ARG_SET=1
      shift 2
      ;;
    --contract-version=*)
      CONTRACT_VERSION="${1#--contract-version=}"
      CONTRACT_VERSION_ARG_SET=1
      shift
      ;;
    --resolve-only)
      RESOLVE_ONLY=1
      shift
      ;;
    --refresh)
      REFRESH_CACHE=1
      shift
      ;;
    --seed-dev-bin)
      SEED_DEV_BIN=1
      shift
      ;;
    --repository-scope)
      REPOSITORY_SCOPE=1
      shift
      ;;
    -*)
      echo "Unknown install-jig option: $1" >&2
      exit 2
      ;;
    *)
      if [[ -n "$INSTALL_ROOT_ARG" ]]; then
        echo "Unexpected extra install root argument: $1" >&2
        exit 2
      fi
      INSTALL_ROOT_ARG="$1"
      shift
      ;;
  esac
done

if [[ "$CONTRACT_VERSION_ARG_SET" == "1" && ! "$CONTRACT_VERSION" =~ ^[0-9]+$ ]]; then
  echo "--contract-version must be a non-negative integer." >&2
  exit 2
fi
CONFIG_FIELDS_VALID=1
LEGACY_JIG_VERSION=""
SRC_PATH=""
TEMPLATE_COMMIT=""
TEMPLATE_SOURCE_URL=""
LOAD_CONFIG_FIELDS=0
if [[ -z "${JIG_DEV_BIN:-}" || "$SEED_DEV_BIN" == "1" ]]; then
  LOAD_CONFIG_FIELDS=1
fi

if [[ "$REPOSITORY_SCOPE" == "1" && "$LOAD_CONFIG_FIELDS" == "1" ]]; then
  require_python3
  if ! {
    IFS= read -r -d '' REPOSITORY_CONTRACT_VERSION \
      && IFS= read -r -d '' LEGACY_JIG_VERSION \
      && IFS= read -r -d '' SRC_PATH \
      && IFS= read -r -d '' TEMPLATE_COMMIT \
      && IFS= read -r -d '' TEMPLATE_SOURCE_URL
  } < <(read_config_fields_for_mode "$CONTRACT_MANIFEST"); then
    CONFIG_FIELDS_VALID=0
  fi
elif [[ "$LOAD_CONFIG_FIELDS" == "1" ]]; then
  require_python3
  if ! {
    IFS= read -r -d '' LEGACY_JIG_VERSION \
      && IFS= read -r -d '' SRC_PATH \
      && IFS= read -r -d '' TEMPLATE_COMMIT \
      && IFS= read -r -d '' TEMPLATE_SOURCE_URL
  } < <(read_config_fields_for_mode); then
    CONFIG_FIELDS_VALID=0
  fi
fi

if [[ "$REPOSITORY_SCOPE" == "1" ]]; then
  if [[ -z "${REPOSITORY_CONTRACT_VERSION:-}" ]]; then
    require_python3
    if ! REPOSITORY_CONTRACT_VERSION="$(read_contract_version 2>/dev/null)"; then
      if [[ "$RESOLVE_ONLY" == "0" ]]; then
        echo "Failed to read a numeric contract_version from $CONTRACT_MANIFEST for repository-scoped runtime selection." >&2
        echo "Run scripts/jig doctor for repair guidance." >&2
      fi
      exit 1
    fi
  fi
  if [[ "$CONTRACT_VERSION_ARG_SET" == "0" ]]; then
    CONTRACT_VERSION="$REPOSITORY_CONTRACT_VERSION"
  elif [[ "$REPOSITORY_CONTRACT_VERSION" != "$CONTRACT_VERSION" ]]; then
    if [[ "$RESOLVE_ONLY" == "0" ]]; then
      echo "Launcher contract version $CONTRACT_VERSION does not match repository contract version $REPOSITORY_CONTRACT_VERSION; refusing runtime selection." >&2
    fi
    exit 1
  fi
elif [[ "$CONTRACT_VERSION_ARG_SET" == "0" && -z "$CONTRACT_VERSION" ]]; then
  require_python3
  if ! CONTRACT_VERSION="$(read_contract_version 2>/dev/null)"; then
    if [[ "$RESOLVE_ONLY" == "0" ]]; then
      echo "Failed to read a numeric contract_version from $CONTRACT_MANIFEST." >&2
    fi
    exit 1
  fi
fi
if [[ "$LOAD_CONFIG_FIELDS" == "1" ]]; then
  if [[ "$CONFIG_FIELDS_VALID" == "1" && -z "$SRC_PATH" ]]; then
    if [[ "$RESOLVE_ONLY" == "0" ]]; then
      echo "Failed to read _src_path from $ANSWERS_FILE." >&2
    fi
    CONFIG_FIELDS_VALID=0
  fi
fi
# Configured filesystem sources are repository-relative, independent of the
# directory from which scripts/install-jig.sh was invoked.
if [[ "$CONFIG_FIELDS_VALID" == "1" && -n "$SRC_PATH" ]]; then
  case "$SRC_PATH" in
    /* | *"://"* | git@*:* | embedded:jig-sh) ;;
    *) SRC_PATH="$ROOT_DIR/$SRC_PATH" ;;
  esac
fi
CONFIGURED_SRC_PATH="$SRC_PATH"
OFFICIAL_JIG_SOURCE="https://github.com/bpcakes/jig-sh.git"
UNPINNED_REMOTE_APPROVED=0

CONTRACT_CACHE_KEY="contract-$CONTRACT_VERSION"

is_remote_source() {
  local source="$1"
  [[ "$source" == *"://"* || "$source" == git@*:* ]]
}

is_embedded_source() {
  local source="$1"
  # Keep this sentinel in sync with EMBEDDED_TEMPLATE_SOURCE in the Rust runtime.
  [[ "$source" == "embedded:jig-sh" ]]
}

case "$INSTALL_PROFILE" in
  default | runtime | mcp)
    ;;
  *)
    echo "Unsupported jig install profile: $INSTALL_PROFILE" >&2
    exit 2
    ;;
esac

if [[ "$SEED_DEV_BIN" == "1" && "$RESOLVE_ONLY" == "1" ]]; then
  echo "--seed-dev-bin cannot be combined with --resolve-only." >&2
  exit 2
fi

if [[ -d "$ROOT_DIR/.git" ]]; then
  DEFAULT_INSTALL_BASE="$ROOT_DIR/.git/jig-tools"
else
  DEFAULT_INSTALL_BASE="$ROOT_DIR/.agent/.cache/jig"
fi

case "$INSTALL_PROFILE" in
  default)
    DEFAULT_INSTALL_ROOT="$DEFAULT_INSTALL_BASE/$CONTRACT_CACHE_KEY"
    CARGO_INSTALL_FEATURE_ARG=""
    ;;
  runtime | mcp)
    DEFAULT_INSTALL_ROOT="$DEFAULT_INSTALL_BASE/$CONTRACT_CACHE_KEY-runtime"
    CARGO_INSTALL_FEATURE_ARG="--no-default-features"
    ;;
esac

INSTALL_ROOT="${INSTALL_ROOT_ARG:-$DEFAULT_INSTALL_ROOT}"
BIN_PATH="$INSTALL_ROOT/bin/jig"
INSTALL_LOCK_PATH="$INSTALL_ROOT.lock"
INSTALL_LOCK_GUARD_PATH="$INSTALL_ROOT.lock.guard"
INSTALL_LOCK_ATTEMPTS=30
INSTALL_LOCK_RETRY_SECONDS=1
INSTALL_LOCK_HELD=0
INSTALL_LOCK_TOKEN_HELD=""

binary_version() {
  local bin_path="$1"
  "$bin_path" --version 2>/dev/null | awk '{print $2}'
}

binary_is_compatible() {
  local bin_path="$1"
  [[ -x "$bin_path" ]] || return 1
  # Managed caches and explicit JIG_DEV_BIN values are trusted executable
  # inputs, and development overrides may intentionally be script wrappers.
  # Ambient PATH discovery has a different trust boundary and uses the native
  # header/direct-exec check in resolve_compatible_path_jig instead.
  "$bin_path" __runtime-compatible \
    --capability-only \
    --contract-version "$CONTRACT_VERSION" \
    --profile "$INSTALL_PROFILE" \
    "$ROOT_DIR" >/dev/null 2>&1
}

assert_compatible() {
  local bin_path="$1"
  local actual_version
  if ! binary_is_compatible "$bin_path"; then
    actual_version="$(binary_version "$bin_path" || true)"
    echo "Jig ${actual_version:-<unknown>} at $bin_path is incompatible with contract $CONTRACT_VERSION and profile $INSTALL_PROFILE." >&2
    if [[ "$CONTRACT_VERSION" -lt "$LEGACY_VERSION_LOCK_CUTOFF" ]]; then
      echo "This legacy repository's recorded source may predate runtime compatibility probes. Run a current Jig binary directly with 'jig update \"$ROOT_DIR\" --force' to migrate the full harness; if the repo wrapper cannot start, repair it first with 'jig update \"$ROOT_DIR\" --launcher-only --force'." >&2
    else
      echo "Run a current Jig binary directly with 'jig update \"$ROOT_DIR\" --force', or point JIG_DEV_BIN at a compatible rebuilt binary and retry." >&2
    fi
    return 1
  fi
}

hash_stdin() {
  local digest
  if command -v sha256sum >/dev/null 2>&1; then
    digest="$(sha256sum | awk '{print $1}')" || return 1
    [[ "$digest" =~ ^[0-9a-fA-F]{64}$ ]] || return 1
    printf 'sha256:%s\n' "$digest"
    return
  fi
  if command -v shasum >/dev/null 2>&1; then
    digest="$(shasum -a 256 | awk '{print $1}')" || return 1
    [[ "$digest" =~ ^[0-9a-fA-F]{64}$ ]] || return 1
    printf 'sha256:%s\n' "$digest"
    return
  fi
  if command -v openssl >/dev/null 2>&1; then
    digest="$(openssl dgst -sha256 -r | awk '{print $1}')" || return 1
    [[ "$digest" =~ ^[0-9a-fA-F]{64}$ ]] || return 1
    printf 'sha256:%s\n' "$digest"
    return
  fi
  if command -v python3 >/dev/null 2>&1; then
    digest="$(python3 -c '
import hashlib
import sys

digest = hashlib.sha256()
for chunk in iter(lambda: sys.stdin.buffer.read(1024 * 1024), b""):
    digest.update(chunk)
print(digest.hexdigest())
')" || return 1
    [[ "$digest" =~ ^[0-9a-fA-F]{64}$ ]] || return 1
    printf 'sha256:%s\n' "$digest"
    return
  fi
  echo "No SHA-256 utility found; Jig source installs cannot be cache-stamped." >&2
  return 1
}

binary_file_identity() {
  local path="$1"
  local identity
  # Keep the ordinary seeded-cache path cheap without trusting size and mtime
  # alone: ctime and inode also change when a binary is replaced in place.
  # macOS/BSD stat uses -f, while GNU stat uses -c.
  # Capture the BSD attempt so GNU stat cannot leak its partial multi-file
  # output before returning failure and falling back to -c.
  if identity="$(stat -f '%z:%Fm:%Fc:%i' "$path" 2>/dev/null)" \
    && [[ "$identity" =~ ^[0-9]+:[0-9]+([.][0-9]+)?:[0-9]+([.][0-9]+)?:[0-9]+$ ]]; then
    printf '%s\n' "$identity"
    return
  fi
  identity="$(stat -c '%s:%Y:%Z:%i:%y:%z' "$path" 2>/dev/null)" || return 1
  [[ "$identity" =~ ^[0-9]+:[0-9]+:[0-9]+:[0-9]+: ]] || return 1
  [[ "$identity" != *'?'* && "$identity" != *$'\n'* ]] || return 1
  # GNU's nanosecond timestamp forms contain spaces; keep the stamp field on
  # one whitespace-free line for unambiguous parsing.
  printf '%s\n' "${identity// /T}"
}

non_git_local_source_stamp() {
  local source_root="$1"
  local stamp_mode="$2"
  require_python3
  python3 -c '
import hashlib
import os
import stat
import sys

root = os.path.realpath(sys.argv[1])
mode = sys.argv[2]
if mode not in {"content", "metadata"}:
    raise SystemExit("unsupported local source stamp mode")
digest = hashlib.sha256()
max_entries = 100_000
max_bytes = 512 * 1024 * 1024
max_depth = 128
entry_count = 0
total_bytes = 0

class SourceStampLimit(Exception):
    pass

def field(value):
    if isinstance(value, str):
        value = os.fsencode(value)
    digest.update(str(len(value)).encode("ascii") + b":" + value)

field(b"jig-local-source-v2" if mode == "content" else b"jig-local-source-metadata-v1")
field(root)

def visit(relative, depth=0):
    global entry_count, total_bytes
    if depth > max_depth:
        raise SourceStampLimit(f"directory depth exceeds {max_depth}")
    entry_count += 1
    if entry_count > max_entries:
        raise SourceStampLimit(f"entry count exceeds {max_entries}")
    path = os.path.join(root, relative)
    try:
        metadata = os.lstat(path)
    except FileNotFoundError:
        field(b"missing")
        field(relative)
        return
    field(relative)
    field(str(stat.S_IMODE(metadata.st_mode)))
    if mode == "metadata":
        field(str(metadata.st_dev))
        field(str(metadata.st_ino))
        field(str(metadata.st_size))
        field(str(metadata.st_mtime_ns))
        field(str(metadata.st_ctime_ns))
    if stat.S_ISLNK(metadata.st_mode):
        field(b"symlink")
        field(os.readlink(path))
    elif stat.S_ISDIR(metadata.st_mode):
        field(b"directory")
        with os.scandir(path) as entries:
            names = sorted((entry.name for entry in entries), key=os.fsencode)
        for name in names:
            visit(os.path.join(relative, name), depth + 1)
    elif stat.S_ISREG(metadata.st_mode):
        total_bytes += metadata.st_size
        if total_bytes > max_bytes:
            raise SourceStampLimit(f"file bytes exceed {max_bytes}")
        field(b"file")
        field(str(metadata.st_size))
        if mode == "content":
            with open(path, "rb") as source:
                while True:
                    chunk = source.read(1024 * 1024)
                    if not chunk:
                        break
                    digest.update(chunk)
    else:
        field(b"special")
        field(str(metadata.st_mode))

try:
    for entry in ("Cargo.toml", "Cargo.lock", "crates"):
        visit(entry)
except SourceStampLimit as error:
    print(f"Local Jig source stamp limit exceeded: {error}.", file=sys.stderr)
    raise SystemExit(1)

print("sha256:" + digest.hexdigest())
' "$source_root" "$stamp_mode"
}

local_source_stamp() {
  local source_root="$1"
  local canonical_source_root
  local untracked_path
  # Every workspace crate can feed the jig binary transitively. Git diffs cover
  # tracked edits and deletions; untracked inputs need their paths and contents
  # hashed separately because `git diff` intentionally omits them.
  canonical_source_root="$(CDPATH= cd "$source_root" && pwd -P)" || return 1
  if git -C "$source_root" rev-parse --verify HEAD >/dev/null 2>&1; then
    {
    printf 'source-root:%s\0' "$canonical_source_root"
    git -C "$source_root" rev-parse HEAD || return 1
    git -c core.quotepath=false -C "$source_root" diff \
      --binary --no-ext-diff --no-textconv HEAD -- Cargo.toml Cargo.lock crates 2>/dev/null \
      || return 1
    git -C "$source_root" ls-files --others --exclude-standard -z -- Cargo.toml Cargo.lock crates 2>/dev/null |
      while IFS= read -r -d '' untracked_path; do
        printf 'untracked:%s\0' "$untracked_path"
        # Exit the inner pipeline subshell on failure; the trailing return then
        # exits the outer producer before any later producer command can mask it.
        git -C "$source_root" hash-object --no-filters -- "$untracked_path" 2>/dev/null \
          || exit 1
      done || return 1
    } | hash_stdin
    return
  fi
  non_git_local_source_stamp "$source_root" content
}

compute_local_source_stamps() {
  local source_root="$1"
  local metadata_before metadata_after
  LOCAL_SOURCE_METADATA_STAMP=""
  if git -C "$source_root" rev-parse --verify HEAD >/dev/null 2>&1; then
    LOCAL_SOURCE_CONTENT_STAMP="$(local_source_stamp "$source_root")" || return 1
    return 0
  fi
  metadata_before="$(non_git_local_source_stamp "$source_root" metadata)" || return 1
  LOCAL_SOURCE_CONTENT_STAMP="$(non_git_local_source_stamp "$source_root" content)" || return 1
  metadata_after="$(non_git_local_source_stamp "$source_root" metadata)" || return 1
  if [[ "$metadata_before" != "$metadata_after" ]]; then
    echo "Local Jig source changed while it was being fingerprinted; retry after source writes finish." >&2
    return 1
  fi
  LOCAL_SOURCE_METADATA_STAMP="$metadata_after"
}

recorded_source_stamp() {
  local stored_stamp="$1"
  local recorded
  if [[ "$stored_stamp" == $'jig-seeded-runtime-v1\n'* ]]; then
    recorded="${stored_stamp##*$'\n'}"
    recorded="${recorded#source:}"
  else
    recorded="$stored_stamp"
  fi
  [[ "$recorded" =~ ^sha256:[0-9a-fA-F]{64}$ ]] || return 1
  printf '%s\n' "$recorded"
}

local_source_install_is_current() {
  local source_root="$1"
  local install_root="$2"
  local bin_path="$3"
  local stamp_path="$install_root/.jig-source-stamp"
  local metadata_path="$install_root/.jig-source-metadata-stamp"

  [[ -x "$bin_path" ]] || return 1
  binary_is_compatible "$bin_path" || return 1
  [[ -f "$stamp_path" ]] || return 1
  local current_stamp current_metadata metadata_stamp stored_metadata stored_stamp recorded_stamp
  stored_stamp="$(cat "$stamp_path")" || return 1
  if ! git -C "$source_root" rev-parse --verify HEAD >/dev/null 2>&1; then
    current_metadata="$(non_git_local_source_stamp "$source_root" metadata)" || return 1
    stored_metadata="$(cat "$metadata_path" 2>/dev/null || true)"
    if [[ "$stored_metadata" == "$current_metadata" ]]; then
      recorded_stamp="$(recorded_source_stamp "$stored_stamp")" || return 1
      if source_stamp_matches "$stamp_path" "$recorded_stamp" "$bin_path"; then
        return 0
      fi
    fi
  fi
  compute_local_source_stamps "$source_root" || return 1
  current_stamp="$LOCAL_SOURCE_CONTENT_STAMP"
  source_stamp_matches "$stamp_path" "$current_stamp" "$bin_path" || return 1
  if [[ "$RESOLVE_ONLY" == "0" && -n "$LOCAL_SOURCE_METADATA_STAMP" ]]; then
    metadata_stamp="$(cat "$stamp_path")" || return 1
    refresh_local_source_metadata_stamp \
      "$stamp_path" "$metadata_stamp" "$metadata_path" "$LOCAL_SOURCE_METADATA_STAMP"
  fi
  return 0
}

configured_source_stamp() {
  {
    printf 'jig-runtime-source-v1\0'
    printf '%s\0' "$CONFIGURED_SRC_PATH"
    printf '%s\0' "$TEMPLATE_COMMIT"
    printf '%s\0' "$TEMPLATE_SOURCE_URL"
    if [[ "$CONTRACT_VERSION" -lt "$LEGACY_VERSION_LOCK_CUTOFF" ]]; then
      printf '%s\0' "$LEGACY_JIG_VERSION"
    else
      printf '\0'
    fi
    printf '%s\0' "$CONTRACT_VERSION"
  } | hash_stdin
}

configured_source_install_is_current() {
  local install_root="$1"
  local bin_path="$2"
  local stamp_path="$install_root/.jig-source-stamp"

  [[ -x "$bin_path" ]] || return 1
  binary_is_compatible "$bin_path" || return 1
  [[ -f "$stamp_path" ]] || return 1
  local current_stamp
  current_stamp="$(configured_source_stamp)" || return 1
  source_stamp_matches "$stamp_path" "$current_stamp" "$bin_path"
}

source_stamp_matches() {
  local stamp_path="$1"
  local current_stamp="$2"
  local bin_path="$3"
  local stored_stamp seeded_body seeded_binary_line seeded_identity_line
  local current_binary_identity="" current_binary_stamp
  stored_stamp="$(cat "$stamp_path")" || return 1
  if [[ "$stored_stamp" == "$current_stamp" ]]; then
    return 0
  fi
  [[ "$stored_stamp" == $'jig-seeded-runtime-v1\nbinary:sha256:'* ]] || return 1
  [[ "${stored_stamp##*$'\n'}" == "source:$current_stamp" ]] || return 1
  seeded_body="${stored_stamp#*$'\n'}"
  seeded_binary_line="${seeded_body%%$'\n'*}"
  seeded_body="${seeded_body#*$'\n'}"
  seeded_identity_line="${seeded_body%%$'\n'*}"
  current_binary_identity="$(binary_file_identity "$bin_path" 2>/dev/null || true)"
  if [[ "$seeded_identity_line" == binary-identity:* \
    && -n "$current_binary_identity" ]]; then
    if [[ "$seeded_identity_line" == "binary-identity:$current_binary_identity" ]]; then
      return 0
    fi
  fi
  current_binary_stamp="$(hash_stdin <"$bin_path")" || return 1
  [[ "$seeded_binary_line" == "binary:$current_binary_stamp" ]] || return 1
  if [[ "$RESOLVE_ONLY" == "0" \
    && -n "$current_binary_identity" \
    && "$seeded_identity_line" != "binary-identity:$current_binary_identity" ]]; then
    refresh_seeded_source_identity \
      "$stamp_path" "$stored_stamp" "$seeded_binary_line" \
      "$current_binary_identity" "$current_stamp"
  fi
  return 0
}

refresh_seeded_source_identity() (
  local stamp_path="$1"
  local expected_stamp="$2"
  local seeded_binary_line="$3"
  local current_binary_identity="$4"
  local current_stamp="$5"
  local install_root="${stamp_path%/*}"
  local refresh_lock="${install_root}.lock"
  local temp_stamp="$stamp_path.$$"
  trap 'rm -f "$temp_stamp" 2>/dev/null || true' EXIT
  # The identity line is only a fast-path optimization. Refresh it only while
  # this process is already inside the cache publication protocol; callers that
  # are merely probing a different cache may safely keep hashing the binary.
  [[ "$INSTALL_LOCK_HELD" == "1" && "$refresh_lock" == "$INSTALL_LOCK_PATH" ]] || return 0
  if [[ "$(cat "$stamp_path" 2>/dev/null || true)" != "$expected_stamp" ]]; then
    return 0
  fi
  if ! (
    {
      printf '%s\n' 'jig-seeded-runtime-v1'
      printf '%s\n' "$seeded_binary_line"
      printf 'binary-identity:%s\n' "$current_binary_identity"
      printf 'source:%s\n' "$current_stamp"
    } >"$temp_stamp"
  ) 2>/dev/null; then
    rm -f "$temp_stamp" || true
    return 0
  fi
  if ! mv "$temp_stamp" "$stamp_path" 2>/dev/null; then
    rm -f "$temp_stamp" || true
  fi
)

refresh_local_source_metadata_stamp() (
  local stamp_path="$1"
  local expected_stamp="$2"
  local metadata_path="$3"
  local current_metadata="$4"
  local install_root="${stamp_path%/*}"
  local refresh_lock="${install_root}.lock"
  local temp_metadata="$metadata_path.$$"
  trap 'rm -f "$temp_metadata" 2>/dev/null || true' EXIT
  [[ "$INSTALL_LOCK_HELD" == "1" && "$refresh_lock" == "$INSTALL_LOCK_PATH" ]] || return 0
  if [[ "$(cat "$stamp_path" 2>/dev/null || true)" != "$expected_stamp" ]]; then
    return 0
  fi
  if ! (printf '%s\n' "$current_metadata" >"$temp_metadata") 2>/dev/null; then
    rm -f "$temp_metadata" 2>/dev/null || true
    return 0
  fi
  if ! mv "$temp_metadata" "$metadata_path" 2>/dev/null; then
    rm -f "$temp_metadata" 2>/dev/null || true
  fi
)

cached_install_is_current() {
  local install_root="$1"
  local bin_path="$2"

  [[ "$REFRESH_CACHE" == "0" ]] || return 1
  if [[ -d "$SRC_PATH/crates/jig" ]] || [[ "$SRC_PATH" == /* && -d "$SRC_PATH" ]]; then
    local_source_install_is_current "$SRC_PATH" "$install_root" "$bin_path"
  else
    configured_source_install_is_current "$install_root" "$bin_path"
  fi
}

write_local_source_stamp() {
  local source_root="$1"
  local expected_stamp="${2:-}"
  local current_stamp
  local stamp_path="$INSTALL_ROOT/.jig-source-stamp"
  local metadata_path="$INSTALL_ROOT/.jig-source-metadata-stamp"
  local temp_stamp="$stamp_path.$$"
  local temp_metadata="$metadata_path.$$"
  if [[ -d "$stamp_path" ]]; then
    echo "Failed to record local Jig source provenance at $stamp_path: expected a file path, found a directory." >&2
    return 1
  fi
  rm -f "$metadata_path" 2>/dev/null || true
  compute_local_source_stamps "$source_root" || {
    rm -f "$stamp_path" "$metadata_path" 2>/dev/null || true
    echo "Failed to fingerprint local Jig source at $source_root; the installed runtime will not be treated as cacheable." >&2
    return 1
  }
  current_stamp="$LOCAL_SOURCE_CONTENT_STAMP"
  if [[ -n "$expected_stamp" && "$current_stamp" != "$expected_stamp" ]]; then
    rm -f "$temp_stamp" "$stamp_path" "$metadata_path" 2>/dev/null || true
    echo "Local Jig source changed while cargo install was running; refusing to cache the resulting binary. Retry the install from a stable checkout." >&2
    return 1
  fi
  if ! (printf '%s\n' "$current_stamp" >"$temp_stamp") 2>/dev/null; then
    rm -f "$temp_stamp" "$stamp_path" "$metadata_path" 2>/dev/null || true
    echo "Failed to record local Jig source provenance at $stamp_path." >&2
    return 1
  fi
  if ! mv "$temp_stamp" "$stamp_path" 2>/dev/null; then
    rm -f "$temp_stamp" "$stamp_path" "$metadata_path" 2>/dev/null || true
    echo "Failed to record local Jig source provenance at $stamp_path." >&2
    return 1
  fi
  if [[ -n "$LOCAL_SOURCE_METADATA_STAMP" ]]; then
    if [[ -d "$metadata_path" ]]; then
      echo "Failed to record local Jig source metadata at $metadata_path: expected a file path, found a directory." >&2
      return 1
    fi
    if ! (printf '%s\n' "$LOCAL_SOURCE_METADATA_STAMP" >"$temp_metadata") 2>/dev/null \
      || ! mv "$temp_metadata" "$metadata_path" 2>/dev/null; then
      rm -f "$temp_metadata" "$metadata_path" 2>/dev/null || true
      echo "Failed to record local Jig source metadata at $metadata_path." >&2
      return 1
    fi
  fi
}

write_configured_source_stamp() {
  local current_stamp
  local stamp_path="$INSTALL_ROOT/.jig-source-stamp"
  local metadata_path="$INSTALL_ROOT/.jig-source-metadata-stamp"
  local temp_stamp="$stamp_path.$$"
  if [[ -d "$stamp_path" ]]; then
    echo "Failed to record configured Jig source provenance at $stamp_path: expected a file path, found a directory." >&2
    return 1
  fi
  rm -f "$metadata_path" 2>/dev/null || true
  current_stamp="$(configured_source_stamp)" || {
    rm -f "$stamp_path" 2>/dev/null || true
    echo "Failed to fingerprint configured Jig source; the installed runtime will not be treated as cacheable." >&2
    return 1
  }
  if ! (printf '%s\n' "$current_stamp" >"$temp_stamp") 2>/dev/null; then
    rm -f "$temp_stamp" "$stamp_path" 2>/dev/null || true
    echo "Failed to record configured Jig source provenance at $stamp_path." >&2
    return 1
  fi
  if ! mv "$temp_stamp" "$stamp_path" 2>/dev/null; then
    rm -f "$temp_stamp" "$stamp_path" 2>/dev/null || true
    echo "Failed to record configured Jig source provenance at $stamp_path." >&2
    return 1
  fi
}

write_seeded_source_stamp() {
  local binary_path="$1"
  local source_stamp
  local binary_stamp
  local binary_identity
  local local_metadata_stamp=""
  local stamp_path="$INSTALL_ROOT/.jig-source-stamp"
  local metadata_path="$INSTALL_ROOT/.jig-source-metadata-stamp"
  local temp_stamp="$stamp_path.$$"
  local temp_metadata="$metadata_path.$$"
  if [[ -d "$stamp_path" ]]; then
    echo "Failed to record launcher-repair cache provenance at $stamp_path: expected a file path, found a directory." >&2
    return 1
  fi
  rm -f "$metadata_path" 2>/dev/null || true
  if [[ -d "$SRC_PATH/crates/jig" ]] || [[ "$SRC_PATH" == /* && -d "$SRC_PATH" ]]; then
    compute_local_source_stamps "$SRC_PATH" || {
      rm -f "$stamp_path" 2>/dev/null || true
      echo "Failed to fingerprint local Jig source while recording launcher-repair cache provenance." >&2
      return 1
    }
    source_stamp="$LOCAL_SOURCE_CONTENT_STAMP"
    local_metadata_stamp="$LOCAL_SOURCE_METADATA_STAMP"
  else
    source_stamp="$(configured_source_stamp)" || {
      rm -f "$stamp_path" 2>/dev/null || true
      echo "Failed to fingerprint configured Jig source while recording launcher-repair cache provenance." >&2
      return 1
    }
  fi
  binary_stamp="$(hash_stdin <"$binary_path")" || {
    rm -f "$stamp_path" 2>/dev/null || true
    echo "Failed to fingerprint launcher-repair binary at $binary_path." >&2
    return 1
  }
  binary_identity="$(binary_file_identity "$binary_path" 2>/dev/null || true)"
  if ! (
    {
      printf '%s\n' 'jig-seeded-runtime-v1'
      printf 'binary:%s\n' "$binary_stamp"
      if [[ -n "$binary_identity" ]]; then
        printf 'binary-identity:%s\n' "$binary_identity"
      fi
      printf 'source:%s\n' "$source_stamp"
    } >"$temp_stamp"
  ) 2>/dev/null; then
    rm -f "$temp_stamp" "$stamp_path" 2>/dev/null || true
    echo "Failed to record launcher-repair cache provenance at $stamp_path." >&2
    return 1
  fi
  if ! mv "$temp_stamp" "$stamp_path" 2>/dev/null; then
    rm -f "$temp_stamp" "$stamp_path" 2>/dev/null || true
    echo "Failed to record launcher-repair cache provenance at $stamp_path." >&2
    return 1
  fi
  if [[ -n "$local_metadata_stamp" ]]; then
    if [[ -d "$metadata_path" ]]; then
      echo "Failed to record launcher-repair source metadata at $metadata_path: expected a file path, found a directory." >&2
      return 1
    fi
    if ! (printf '%s\n' "$local_metadata_stamp" >"$temp_metadata") 2>/dev/null \
      || ! mv "$temp_metadata" "$metadata_path" 2>/dev/null; then
      rm -f "$temp_metadata" "$metadata_path" 2>/dev/null || true
      echo "Failed to record launcher-repair source metadata at $metadata_path." >&2
      return 1
    fi
  fi
}

restore_seeded_source_provenance() {
  local stamp_path="$1"
  local stamp_backup="$2"
  local had_stamp="$3"
  local metadata_path="$4"
  local metadata_backup="$5"
  local had_metadata="$6"
  local failed=0
  rm -f "$stamp_path" "$metadata_path" 2>/dev/null || true
  if [[ "$had_stamp" == "1" ]] && ! mv "$stamp_backup" "$stamp_path" 2>/dev/null; then
    echo "Failed to restore prior cache provenance from $stamp_backup." >&2
    failed=1
  fi
  if [[ "$had_metadata" == "1" ]] \
    && ! mv "$metadata_backup" "$metadata_path" 2>/dev/null; then
    echo "Failed to restore prior cache metadata from $metadata_backup." >&2
    failed=1
  fi
  return "$failed"
}

restore_seeded_binary_publication() {
  local bin_path="$1"
  local bin_backup="$2"
  local had_bin="$3"
  rm -f "$bin_path" 2>/dev/null || true
  if [[ "$had_bin" == "1" ]] && ! mv "$bin_backup" "$bin_path" 2>/dev/null; then
    echo "Failed to restore prior cached binary from $bin_backup." >&2
    return 1
  fi
}

configured_source_is_mutable() {
  if [[ -d "$CONFIGURED_SRC_PATH/crates/jig" ]] \
    || [[ "$CONFIGURED_SRC_PATH" == /* && -d "$CONFIGURED_SRC_PATH" ]]; then
    return 1
  fi
  if [[ "$TEMPLATE_COMMIT" =~ ^[0-9a-fA-F]{7,40}$ ]]; then
    return 1
  fi
  if [[ "$CONTRACT_VERSION" -lt "$LEGACY_VERSION_LOCK_CUTOFF" && -n "$LEGACY_JIG_VERSION" ]]; then
    return 1
  fi
  is_embedded_source "$CONFIGURED_SRC_PATH" \
    || is_remote_source "$CONFIGURED_SRC_PATH" \
    || is_remote_source "$TEMPLATE_SOURCE_URL"
}

mark_mutable_source_refresh_reminder() {
  local install_root="$1"
  : >"$install_root/.jig-mutable-source-reminder" 2>/dev/null
}

record_mutable_source_refresh_reminder() {
  local install_root="$1"
  if ! mark_mutable_source_refresh_reminder "$install_root"; then
    echo "Could not record the mutable-source reminder under $install_root; this warning may repeat until that cache directory is writable." >&2
  fi
  return 0
}

warn_for_mutable_source_cache_if_due() {
  local install_root="$1"
  local marker="$install_root/.jig-mutable-source-reminder"
  local now mtime
  [[ "$RESOLVE_ONLY" == "0" ]] || return 0
  configured_source_is_mutable || return 0
  now="$(date +%s)"
  if [[ -f "$marker" ]]; then
    if mtime="$(stat -f %m "$marker" 2>/dev/null)" \
      && [[ "$mtime" =~ ^[0-9]+$ ]]; then
      :
    elif mtime="$(stat -c %Y "$marker" 2>/dev/null)" \
      && [[ "$mtime" =~ ^[0-9]+$ ]]; then
      :
    else
      mtime=0
    fi
    if [[ $((now - mtime)) -lt 604800 ]]; then
      return 0
    fi
  fi
  echo "Using a cached Jig runtime from a mutable source; pass --refresh or set JIG_INSTALL_REFRESH=1 to check that source again (and reapprove an unpinned remote if prompted)." >&2
  record_mutable_source_refresh_reminder "$install_root"
}

install_from_dev_bin() {
  local dev_bin
  dev_bin="$(resolve_executable_path "$JIG_DEV_BIN")" || {
    echo "Failed to resolve JIG_DEV_BIN: $JIG_DEV_BIN" >&2
    exit 1
  }
  if [[ ! -x "$dev_bin" ]]; then
    echo "JIG_DEV_BIN is set but is not executable: $dev_bin" >&2
    exit 1
  fi

  if ! assert_compatible "$dev_bin"; then
    echo "JIG_DEV_BIN is authoritative; refusing to install a fallback binary." >&2
    echo "Rebuild from the jig source checkout with: cargo build -p jig-sh --bin jig" >&2
    echo "Then point JIG_DEV_BIN at the rebuilt binary (for example, target/debug/jig), or unset JIG_DEV_BIN and rerun scripts/jig so the normal cached installer path can select a compatible runtime." >&2
    exit 1
  fi
  # scripts/jig captures stdout from this installer and execs the printed path.
  printf '%s\n' "$dev_bin"
}

seed_from_dev_bin() {
  if [[ -z "${JIG_DEV_BIN:-}" ]]; then
    echo "--seed-dev-bin requires JIG_DEV_BIN." >&2
    exit 2
  fi
  local dev_bin temp_bin
  local stamp_path="$INSTALL_ROOT/.jig-source-stamp"
  local metadata_path="$INSTALL_ROOT/.jig-source-metadata-stamp"
  local stamp_backup="$INSTALL_ROOT/.jig-source-stamp.seed-backup.$$"
  local metadata_backup="$INSTALL_ROOT/.jig-source-metadata-stamp.seed-backup.$$"
  local bin_backup="$INSTALL_ROOT/bin/.jig-seed-bin-backup.$$"
  local had_stamp=0
  local had_metadata=0
  local had_bin=0
  dev_bin="$(resolve_executable_path "$JIG_DEV_BIN")" || {
    echo "Failed to resolve JIG_DEV_BIN: $JIG_DEV_BIN" >&2
    exit 1
  }
  assert_compatible "$dev_bin"
  acquire_install_lock
  mkdir -p "$INSTALL_ROOT/bin"
  temp_bin="$INSTALL_ROOT/bin/.jig-seed.$$"
  cp "$dev_bin" "$temp_bin"
  chmod 755 "$temp_bin"
  if ! assert_compatible "$temp_bin"; then
    echo "Copied JIG_DEV_BIN failed compatibility validation; the existing cached binary was left unchanged." >&2
    rm -f "$temp_bin"
    return 1
  fi
  # A repair seed is contract-compatible but was not built from the configured
  # source. Record both its binary identity and the source state it shadows so
  # cache provenance remains truthful and source changes still invalidate it.
  if [[ -f "$stamp_path" ]]; then
    if ! mv "$stamp_path" "$stamp_backup"; then
      rm -f "$temp_bin"
      return 1
    fi
    had_stamp=1
  fi
  if [[ -f "$metadata_path" ]]; then
    mv "$metadata_path" "$metadata_backup" || {
      rm -f "$temp_bin"
      restore_seeded_source_provenance \
        "$stamp_path" "$stamp_backup" "$had_stamp" \
        "$metadata_path" "$metadata_backup" "$had_metadata" || true
      return 1
    }
    had_metadata=1
  fi
  if [[ -e "$BIN_PATH" || -L "$BIN_PATH" ]]; then
    if ! mv "$BIN_PATH" "$bin_backup"; then
      rm -f "$temp_bin"
      restore_seeded_source_provenance \
        "$stamp_path" "$stamp_backup" "$had_stamp" \
        "$metadata_path" "$metadata_backup" "$had_metadata" || true
      return 1
    fi
    had_bin=1
  fi
  if ! mv -f "$temp_bin" "$BIN_PATH"; then
    echo "Failed to publish the validated launcher-repair binary at $BIN_PATH; restoring prior cache contents and provenance." >&2
    rm -f "$temp_bin"
    restore_seeded_binary_publication "$BIN_PATH" "$bin_backup" "$had_bin" || true
    restore_seeded_source_provenance \
      "$stamp_path" "$stamp_backup" "$had_stamp" \
      "$metadata_path" "$metadata_backup" "$had_metadata" || true
    return 1
  fi
  # rename updates ctime, which is part of binary_file_identity. Record
  # provenance only after publication so the fast-path identity describes the
  # file callers will actually execute.
  if ! write_seeded_source_stamp "$BIN_PATH"; then
    restore_seeded_binary_publication "$BIN_PATH" "$bin_backup" "$had_bin" || true
    restore_seeded_source_provenance \
      "$stamp_path" "$stamp_backup" "$had_stamp" \
      "$metadata_path" "$metadata_backup" "$had_metadata" || true
    return 1
  fi
  rm -f "$stamp_backup" "$metadata_backup" "$bin_backup" 2>/dev/null || true
  printf '%s\n' "$BIN_PATH"
}

resolve_executable_path() {
  local input="$1"
  if command -v realpath >/dev/null 2>&1; then
    realpath "$input"
    return
  fi
  if command -v python3 >/dev/null 2>&1; then
    python3 -c '
import os
import sys

print(os.path.realpath(sys.argv[1]))
' "$input"
    return
  fi

  local input_dir
  input_dir="$(cd "$(dirname "$input")" && pwd -P)" || return 1
  local resolved="$input_dir/$(basename "$input")"
  case "$resolved" in
    /*)
      printf '%s\n' "$resolved"
      ;;
    *)
      echo "Resolved executable path is not absolute: $resolved" >&2
      return 1
      ;;
  esac
}

acquire_install_lock() {
  if [[ -n "${JIG_INSTALL_LOCK_TOKEN:-}" ]]; then
    local record owner token ignored
    if IFS=' ' read -r record owner token ignored <"$INSTALL_LOCK_PATH/owner-v1" \
      && [[ "$record" == "owner-v1" && "$owner" =~ ^[0-9]+$ \
        && "$token" == "$JIG_INSTALL_LOCK_TOKEN" && -z "$ignored" ]]; then
      INSTALL_LOCK_HELD=1
      INSTALL_LOCK_TOKEN_HELD="$token"
      unset JIG_INSTALL_LOCK_TOKEN
      trap release_install_lock EXIT
      return 0
    fi
    echo "The internal Jig installer lock ownership record is invalid; refusing cache mutation." >&2
    exit 1
  fi

  mkdir -p "$(dirname "$INSTALL_ROOT")"
  require_python3
  local status
  if python3 -c '
import errno
import os
import pathlib
import secrets
import subprocess
import sys
import time

guard_path, lock_directory, attempts_text, retry_text, bash_executable, installer, *installer_args = sys.argv[1:]
attempts = max(1, int(attempts_text))
retry_seconds = float(retry_text)

def try_lock(file_descriptor):
    if os.name == "nt":
        import msvcrt

        os.lseek(file_descriptor, 0, os.SEEK_SET)
        try:
            msvcrt.locking(file_descriptor, msvcrt.LK_NBLCK, 1)
            return True
        except OSError as error:
            if error.errno in {errno.EACCES, errno.EAGAIN, errno.EDEADLK}:
                return False
            raise
    import fcntl

    try:
        fcntl.flock(file_descriptor, fcntl.LOCK_EX | fcntl.LOCK_NB)
        return True
    except BlockingIOError:
        return False

def unlock(file_descriptor):
    if os.name == "nt":
        import msvcrt

        os.lseek(file_descriptor, 0, os.SEEK_SET)
        msvcrt.locking(file_descriptor, msvcrt.LK_UNLCK, 1)
        return
    import fcntl

    fcntl.flock(file_descriptor, fcntl.LOCK_UN)

file_descriptor = None
owner_path = os.path.join(lock_directory, "owner-v1")
for attempt in range(attempts):
    try:
        flags = os.O_RDWR | os.O_CREAT
        if hasattr(os, "O_BINARY"):
            flags |= os.O_BINARY
        file_descriptor = os.open(guard_path, flags, 0o600)
        if os.name == "nt" and os.fstat(file_descriptor).st_size == 0:
            os.write(file_descriptor, b"\0")
        if try_lock(file_descriptor):
            if os.path.isdir(lock_directory):
                try:
                    stale_owner_record = pathlib.Path(owner_path).read_bytes()
                    fields = stale_owner_record.split()
                except (OSError, UnicodeError):
                    fields = []
                valid_owner = False
                if len(fields) == 3 and fields[0] == b"owner-v1":
                    try:
                        valid_owner = 0 <= int(fields[1]) <= 4294967295
                    except ValueError:
                        pass
                if valid_owner:
                    try:
                        os.unlink(owner_path)
                        os.rmdir(lock_directory)
                    except FileNotFoundError:
                        unlock(file_descriptor)
                        os.close(file_descriptor)
                        file_descriptor = None
                        if attempt + 1 < attempts:
                            time.sleep(retry_seconds)
                        continue
                    except OSError as error:
                        try:
                            pathlib.Path(owner_path).write_bytes(stale_owner_record)
                        except OSError as restore_error:
                            print(f"Failed to restore stale Jig installer owner {owner_path}: {restore_error}", file=sys.stderr)
                        print(f"Failed to recover stale Jig installer lock {lock_directory}: {error}", file=sys.stderr)
                        raise SystemExit(1)
                else:
                    # A live legacy mkdir-only installer does not hold the OS
                    # guard, so an unmarked directory is never safe to steal.
                    unlock(file_descriptor)
                    os.close(file_descriptor)
                    file_descriptor = None
                    if attempt + 1 < attempts:
                        time.sleep(retry_seconds)
                    continue
            elif os.path.exists(lock_directory):
                unlock(file_descriptor)
                os.close(file_descriptor)
                file_descriptor = None
                if attempt + 1 < attempts:
                    time.sleep(retry_seconds)
                continue
            try:
                os.mkdir(lock_directory)
            except FileExistsError:
                unlock(file_descriptor)
                os.close(file_descriptor)
                file_descriptor = None
                if attempt + 1 < attempts:
                    time.sleep(retry_seconds)
                continue
            except OSError as error:
                print(f"Failed to claim Jig installer lock {lock_directory}: {error}", file=sys.stderr)
                raise SystemExit(1)
            break
        os.close(file_descriptor)
        file_descriptor = None
    except IsADirectoryError as error:
        print(f"Jig installer guard path is a directory instead of a file: {guard_path}: {error}", file=sys.stderr)
        raise SystemExit(1)
    except OSError as error:
        print(f"Failed to open Jig installer guard {guard_path}: {error}", file=sys.stderr)
        raise SystemExit(1)
    if attempt + 1 < attempts:
        time.sleep(retry_seconds)
else:
    print(f"Timed out waiting for jig installer lock: {lock_directory}", file=sys.stderr)
    print("Another scripts/jig install may still be running; an unmarked legacy directory must be removed manually only after confirming no installer is active.", file=sys.stderr)
    raise SystemExit(1)

token = secrets.token_hex(16)
record = f"owner-v1 {os.getpid()} {token}\n".encode("ascii")

def remove_owned_lock_directory():
    cleanup_error = None
    try:
        os.unlink(owner_path)
    except FileNotFoundError:
        pass
    except OSError as error:
        cleanup_error = error
    if cleanup_error is None:
        try:
            os.rmdir(lock_directory)
        except FileNotFoundError:
            return None
        except OSError as error:
            cleanup_error = error
    if cleanup_error is not None:
        try:
            pathlib.Path(owner_path).write_bytes(record)
        except OSError as restore_error:
            print(f"Warning: also failed to restore Jig installer owner {owner_path}: {restore_error}", file=sys.stderr)
    return cleanup_error

try:
    with open(owner_path, "xb") as owner_file:
        owner_file.write(record)
        owner_file.flush()
        os.fsync(owner_file.fileno())
except BaseException:
    cleanup_error = remove_owned_lock_directory()
    if cleanup_error is not None:
        print(f"Warning: failed to clean up Jig installer lock {lock_directory}: {cleanup_error}", file=sys.stderr)
    try:
        unlock(file_descriptor)
    except OSError as error:
        print(f"Warning: failed to unlock Jig installer guard {guard_path}: {error}", file=sys.stderr)
    try:
        os.close(file_descriptor)
    except OSError as error:
        print(f"Warning: failed to close Jig installer guard {guard_path}: {error}", file=sys.stderr)
    raise
environment = os.environ.copy()
environment["JIG_INSTALL_LOCK_TOKEN"] = token
operation = f"record Jig installer guard {guard_path}"
try:
    os.set_inheritable(file_descriptor, True)
    os.ftruncate(file_descriptor, 0)
    os.lseek(file_descriptor, 0, os.SEEK_SET)
    os.write(file_descriptor, record)
    os.fsync(file_descriptor)
    operation = f"re-enter Jig installer {installer}"
    command = [bash_executable, installer, *installer_args]
    if os.name == "nt":
        status = subprocess.call(command, env=environment, close_fds=False)
    else:
        status = subprocess.call(command, env=environment, pass_fds=(file_descriptor,))
    raise SystemExit(status)
except OSError as error:
    print(f"Failed to {operation}: {error}", file=sys.stderr)
    raise SystemExit(1)
finally:
    try:
        try:
            fields = pathlib.Path(owner_path).read_text().split()
        except (OSError, UnicodeError):
            fields = []
        if len(fields) == 3 and fields[0] == "owner-v1" and fields[2] == token:
            cleanup_error = remove_owned_lock_directory()
            if cleanup_error is not None:
                print(f"Warning: failed to clean up Jig installer lock {lock_directory}: {cleanup_error}", file=sys.stderr)
        try:
            unlock(file_descriptor)
        except OSError as error:
            print(f"Warning: failed to unlock Jig installer guard {guard_path}: {error}", file=sys.stderr)
    finally:
        os.close(file_descriptor)
' "$INSTALL_LOCK_GUARD_PATH" "$INSTALL_LOCK_PATH" "$INSTALL_LOCK_ATTEMPTS" "$INSTALL_LOCK_RETRY_SECONDS" \
    "$BASH" "$ROOT_DIR/scripts/install-jig.sh" \
    "${INSTALLER_ORIGINAL_ARGS[@]+"${INSTALLER_ORIGINAL_ARGS[@]}"}"; then
    status=0
  else
    status=$?
  fi
  exit "$status"
}

release_install_lock() {
  # The Python process owns the OS guard and removes the legacy directory in
  # its finally block. Leaving the owner record intact here lets that single
  # cleanup path restore a reclaimable marker if directory removal fails.
  INSTALL_LOCK_HELD=0
  INSTALL_LOCK_TOKEN_HELD=""
  trap - EXIT
}

install_from_local_source() {
  local source_root="$1"
  local crate_path="$source_root/crates/jig"
  local build_source_stamp
  if [[ ! -d "$crate_path" ]]; then
    echo "Expected local jig source at $crate_path." >&2
    return 1
  fi

  local cargo_args=(install --path "$crate_path" --root "$INSTALL_ROOT" --locked --force)
  if [[ -n "$CARGO_INSTALL_FEATURE_ARG" ]]; then
    cargo_args+=("$CARGO_INSTALL_FEATURE_ARG")
  fi
  compute_local_source_stamps "$source_root" || {
    echo "Failed to fingerprint local Jig source before cargo install." >&2
    return 1
  }
  build_source_stamp="$LOCAL_SOURCE_CONTENT_STAMP"
  # Once cargo may replace the cached binary, no prior provenance may remain
  # authoritative. A failed or racing build must force the next run to rebuild.
  rm -f "$INSTALL_ROOT/.jig-source-stamp" \
    "$INSTALL_ROOT/.jig-source-metadata-stamp" 2>/dev/null || true
  cargo "${cargo_args[@]}"

  assert_compatible "$BIN_PATH"
  write_local_source_stamp "$source_root" "$build_source_stamp" || return 1
}

is_jig_source_checkout() {
  local source_root="$1"
  [[ -n "$source_root" ]] || return 1
  # This helper is rendered into downstream harnesses too so the same template
  # can repair the jig-sh source repo; ordinary projects fail these checks and
  # fall through to the configured template source.
  local manifest="$source_root/crates/jig/Cargo.toml"
  [[ -f "$source_root/templates/project/scripts/install-jig.sh.jinja" ]] || return 1
  [[ -f "$manifest" ]] || return 1
  grep -Eq '^[[:space:]]*name[[:space:]]*=[[:space:]]*"jig-sh"' "$manifest"
}

install_from_git_source() {
  # Keep this array nonempty. Bash 3.2 treats an empty "${array[@]}"
  # expansion as an unbound variable under `set -u`.
  local cargo_args=(--git "$SRC_PATH")
  if [[ "$TEMPLATE_COMMIT" =~ ^[0-9a-fA-F]{7,40}$ ]]; then
    # Adopted repos pin the exact template revision in .jig.toml. Treat that
    # commit as trusted repo configuration: a hex value intentionally overrides
    # the release tag so updates install the same source revision that rendered
    # the repo-local harness.
    cargo_args+=(--rev "$TEMPLATE_COMMIT")
  elif [[ "$CONTRACT_VERSION" -lt "$LEGACY_VERSION_LOCK_CUTOFF" && -n "$LEGACY_JIG_VERSION" ]]; then
    # Legacy contracts may lack immutable source provenance. The old product
    # tag remains a source locator only; the installed binary must still prove
    # repository compatibility before it is accepted.
    cargo_args+=(--tag "v$LEGACY_JIG_VERSION")
  elif [[ "$UNPINNED_REMOTE_APPROVED" != "1" ]]; then
    if [[ "${JIG_INSTALL_ALLOW_UNPINNED_REMOTE:-}" != "1" ]]; then
      echo "Refusing to install Jig from a remote source without an immutable _commit." >&2
      echo "Re-render from the remote source to record its revision, provide JIG_DEV_BIN, or set JIG_INSTALL_ALLOW_UNPINNED_REMOTE=1 to knowingly install the current default branch." >&2
      return 1
    fi
    echo "Warning: installing Jig from the current default branch of an unpinned remote source because JIG_INSTALL_ALLOW_UNPINNED_REMOTE=1." >&2
    echo "Future runs reuse this cached build; pass --refresh or set JIG_INSTALL_REFRESH=1 to check the mutable source again." >&2
  fi

  cargo_args+=(--root "$INSTALL_ROOT" --locked --force)
  if [[ -n "$CARGO_INSTALL_FEATURE_ARG" ]]; then
    cargo_args+=("$CARGO_INSTALL_FEATURE_ARG")
  fi
  cargo_args+=(jig-sh)
  cargo install "${cargo_args[@]}"

  assert_compatible "$BIN_PATH"
  write_configured_source_stamp || return 1
  if configured_source_is_mutable; then
    record_mutable_source_refresh_reminder "$INSTALL_ROOT"
  fi
}

resolve_compatible_path_jig() {
  local candidate candidate_header candidate_magic launcher launcher_path
  candidate="$(command -v jig 2>/dev/null || true)"
  [[ -n "$candidate" ]] || return 1
  candidate="$(resolve_executable_path "$candidate")" || return 1
  launcher_path="$ROOT_DIR/scripts/jig"
  if launcher="$(resolve_executable_path "$launcher_path" 2>/dev/null)"; then
    [[ "$candidate" != "$launcher" ]] || return 1
  elif [[ -e "$launcher_path" ]]; then
    # Fail closed if an existing wrapper cannot be resolved for comparison.
    return 1
  fi
  # PATH reuse is an explicit opt-in, but never execute a script wrapper while
  # trying to prove that the candidate itself is a Jig runtime binary. Bash can
  # interpret executable text even without a shebang after execve returns
  # ENOEXEC, so accept only native ELF and Mach-O headers before probing it.
  candidate_header="$(LC_ALL=C od -An -N8 -tx1 "$candidate" 2>/dev/null | tr -d '[:space:]')"
  candidate_magic="${candidate_header:0:8}"
  case "$candidate_magic" in
    7f454c46|feedface|cefaedfe|feedfacf|cffaedfe) ;;
    cafebabe|cafebabf)
      # Fat Mach-O stores a small big-endian architecture count after the
      # magic. Requiring a realistic count rejects Java class files, which use
      # the same leading cafebabe bytes followed by a much larger version.
      [[ "${candidate_header:8:8}" =~ ^000000(0[1-9a-f]|10)$ ]] || return 1
      ;;
    bebafeca|bfbafeca)
      [[ "${candidate_header:8:8}" =~ ^(0[1-9a-f]|10)000000$ ]] || return 1
      ;;
    *) return 1 ;;
  esac
  native_binary_is_compatible "$candidate" || return 1
  printf '%s\n' "$candidate"
}

ambient_path_binary_reuse_allowed() {
  [[ "$REFRESH_CACHE" == "0" \
    && -z "$INSTALL_ROOT_ARG" \
    && "$INSTALL_PROFILE" != "mcp" \
    && "${JIG_INSTALL_ALLOW_PATH_BINARY:-}" == "1" ]]
}

native_binary_is_compatible() {
  local bin_path="$1"
  # Bash retries ENOEXEC files as shell scripts. Use Python's direct exec path
  # so a crafted text file with an ELF/Mach-O prefix is rejected, never sourced.
  require_python3
  python3 - "$bin_path" "$CONTRACT_VERSION" "$INSTALL_PROFILE" "$ROOT_DIR" <<'PY'
import subprocess
import sys

command = [
    sys.argv[1],
    "__runtime-compatible",
    "--capability-only",
    "--contract-version",
    sys.argv[2],
    "--profile",
    sys.argv[3],
    sys.argv[4],
]
try:
    result = subprocess.run(
        command,
        stdin=subprocess.DEVNULL,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
        check=False,
    )
except OSError:
    raise SystemExit(1)
raise SystemExit(result.returncode)
PY
}

resolve_compatible_cached_without_answers() {
  [[ "$REFRESH_CACHE" == "0" ]] || return 1
  if [[ "$INSTALL_PROFILE" != "default" && -z "$INSTALL_ROOT_ARG" ]]; then
    local full_install_root="$DEFAULT_INSTALL_BASE/$CONTRACT_CACHE_KEY"
    local full_bin_path="$full_install_root/bin/jig"
    if binary_is_compatible "$full_bin_path"; then
      printf '%s\n' "$full_bin_path"
      return 0
    fi
  fi
  if binary_is_compatible "$BIN_PATH"; then
    printf '%s\n' "$BIN_PATH"
    return 0
  fi
  return 1
}

if [[ "$SEED_DEV_BIN" == "1" && "$CONFIG_FIELDS_VALID" != "1" ]]; then
  echo "--seed-dev-bin requires a readable $ANSWERS_FILE with a non-empty _src_path so cache provenance can be recorded." >&2
  exit 1
fi

if [[ -n "${JIG_DEV_BIN:-}" ]]; then
  if [[ "$SEED_DEV_BIN" == "1" ]]; then
    seed_from_dev_bin
  else
    install_from_dev_bin
  fi
  exit 0
fi

if [[ "$SEED_DEV_BIN" == "1" ]]; then
  echo "--seed-dev-bin requires JIG_DEV_BIN." >&2
  exit 2
fi

if [[ "$CONFIG_FIELDS_VALID" != "1" ]]; then
  if CACHED_BIN="$(resolve_compatible_cached_without_answers)"; then
    if [[ "$RESOLVE_ONLY" == "0" ]]; then
      echo "Using an existing contract-compatible Jig cache so the repository configuration can be diagnosed." >&2
    fi
    printf '%s\n' "$CACHED_BIN"
    exit 0
  fi
  if ambient_path_binary_reuse_allowed; then
    if PATH_BIN="$(resolve_compatible_path_jig)"; then
      if [[ "$RESOLVE_ONLY" == "0" ]]; then
        echo "Using explicitly allowed PATH Jig binary so the repository configuration can be diagnosed: $PATH_BIN" >&2
      fi
      printf '%s\n' "$PATH_BIN"
      exit 0
    fi
  fi
  if [[ "$RESOLVE_ONLY" == "1" ]]; then
    exit 1
  fi
  echo "Cannot resolve a Jig runtime because $ANSWERS_FILE is unreadable, invalid, or missing _src_path; use JIG_DEV_BIN or repair the file." >&2
  exit 1
fi

if [[ "$RESOLVE_ONLY" == "0" ]]; then
  # Every cache read that may refresh metadata and every cache publication now
  # runs under one kernel-owned protocol. The lock wrapper re-enters this script
  # after acquiring the file lock and keeps ownership until the child exits.
  acquire_install_lock
fi

# The jig-sh source repo dogfoods generated harness files. An explicitly selected
# JIG_DEV_BIN is handled above; otherwise prefer a source-stamped cache over any
# globally installed runtime.
# Explicit install roots keep the lower-level installer behavior so callers can
# populate exactly the root they requested.
if [[ -z "$INSTALL_ROOT_ARG" ]] && is_jig_source_checkout "$ROOT_DIR"; then
  if [[ "$INSTALL_PROFILE" != "default" ]]; then
    SOURCE_FULL_INSTALL_ROOT="$DEFAULT_INSTALL_BASE/$CONTRACT_CACHE_KEY"
    SOURCE_FULL_BIN_PATH="$SOURCE_FULL_INSTALL_ROOT/bin/jig"
    if [[ "$REFRESH_CACHE" == "0" ]] \
      && local_source_install_is_current \
        "$ROOT_DIR" "$SOURCE_FULL_INSTALL_ROOT" "$SOURCE_FULL_BIN_PATH"; then
      printf '%s\n' "$SOURCE_FULL_BIN_PATH"
      exit 0
    fi
  fi
  if [[ "$REFRESH_CACHE" == "0" ]] \
    && local_source_install_is_current "$ROOT_DIR" "$INSTALL_ROOT" "$BIN_PATH"; then
    printf '%s\n' "$BIN_PATH"
    exit 0
  fi

  if [[ "$RESOLVE_ONLY" == "1" ]]; then
    exit 1
  fi

  if [[ "$INSTALL_PROFILE" == "mcp" ]]; then
    echo "No compatible prebuilt Jig binary is available for MCP profile startup." >&2
    echo "Refusing to run cargo install for MCP; run a normal Jig command first, or build and select a development binary with JIG_DEV_BIN=target/debug/jig." >&2
    exit 1
  fi

  install_from_local_source "$ROOT_DIR"
  printf '%s\n' "$BIN_PATH"
  exit 0
fi

if [[ "$INSTALL_PROFILE" != "default" && -z "$INSTALL_ROOT_ARG" ]]; then
  # Runtime and MCP profiles are subsets of the default binary. Reuse a
  # compatible full build instead of compiling a stripped binary.
  FULL_INSTALL_ROOT="$DEFAULT_INSTALL_BASE/$CONTRACT_CACHE_KEY"
  FULL_BIN_PATH="$FULL_INSTALL_ROOT/bin/jig"
  if cached_install_is_current "$FULL_INSTALL_ROOT" "$FULL_BIN_PATH"; then
    warn_for_mutable_source_cache_if_due "$FULL_INSTALL_ROOT"
    printf '%s\n' "$FULL_BIN_PATH"
    exit 0
  fi
fi

if cached_install_is_current "$INSTALL_ROOT" "$BIN_PATH"; then
  warn_for_mutable_source_cache_if_due "$INSTALL_ROOT"
  printf '%s\n' "$BIN_PATH"
  exit 0
fi

if ambient_path_binary_reuse_allowed; then
  if PATH_BIN="$(resolve_compatible_path_jig)"; then
    if [[ "$RESOLVE_ONLY" == "0" ]]; then
      echo "Using explicitly allowed PATH Jig binary: $PATH_BIN" >&2
    fi
    printf '%s\n' "$PATH_BIN"
    exit 0
  fi
fi

if [[ "$RESOLVE_ONLY" == "1" ]]; then
  exit 1
fi

if [[ "$INSTALL_PROFILE" == "mcp" ]]; then
  echo "No compatible prebuilt Jig binary is available for MCP profile startup." >&2
  echo "Refusing to run cargo install during MCP initialization." >&2
  exit 1
fi

if is_embedded_source "$SRC_PATH"; then
  if [[ "${JIG_INSTALL_ALLOW_EMBEDDED_SOURCE_FALLBACK:-}" != "1" ]]; then
    echo "This repo was rendered from embedded Jig templates, but no contract-compatible Jig binary was found." >&2
    echo "Install a compatible Jig binary or set JIG_DEV_BIN to it. To knowingly install from ${TEMPLATE_SOURCE_URL:-$OFFICIAL_JIG_SOURCE} instead, set JIG_INSTALL_ALLOW_EMBEDDED_SOURCE_FALLBACK=1." >&2
    exit 1
  fi
  SRC_PATH="${TEMPLATE_SOURCE_URL:-$OFFICIAL_JIG_SOURCE}"
  if [[ "$TEMPLATE_COMMIT" =~ ^[0-9a-fA-F]{7,40}$ ]]; then
    echo "Warning: installing pinned source revision $TEMPLATE_COMMIT from $SRC_PATH instead of the embedded template payload that adopted this repo." >&2
  elif [[ "$CONTRACT_VERSION" -lt "$LEGACY_VERSION_LOCK_CUTOFF" && -n "$LEGACY_JIG_VERSION" ]]; then
    echo "Warning: installing from legacy source tag v$LEGACY_JIG_VERSION at $SRC_PATH instead of the embedded template payload that adopted this repo." >&2
  else
    echo "Warning: installing from the current default branch of $SRC_PATH instead of the embedded template payload that adopted this repo." >&2
    echo "Future runs reuse this cached build; pass --refresh or set JIG_INSTALL_REFRESH=1 to check the mutable source again." >&2
    UNPINNED_REMOTE_APPROVED=1
  fi
fi

if cached_install_is_current "$INSTALL_ROOT" "$BIN_PATH"; then
  warn_for_mutable_source_cache_if_due "$INSTALL_ROOT"
  printf '%s\n' "$BIN_PATH"
  exit 0
fi

if [[ -d "$SRC_PATH/crates/jig" ]] || [[ "$SRC_PATH" == /* && -d "$SRC_PATH" ]]; then
  install_from_local_source "$SRC_PATH"
elif is_remote_source "$SRC_PATH"; then
  install_from_git_source
elif [[ -n "$TEMPLATE_SOURCE_URL" ]]; then
  SRC_PATH="$TEMPLATE_SOURCE_URL"
  install_from_git_source
else
  echo "Cannot resolve jig source from _src_path='$SRC_PATH'." >&2
  echo "Re-render from an absolute committed template path or set template_source_url." >&2
  exit 1
fi

printf '%s\n' "$BIN_PATH"
