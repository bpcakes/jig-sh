#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)"
ANSWERS_FILE="$ROOT_DIR/.jig.toml"

read_field() {
  python3 -c '
import ast
import pathlib
import re
import sys

try:
    import tomllib
except ModuleNotFoundError:
    tomllib = None

text = pathlib.Path(sys.argv[1]).read_text()
key = sys.argv[2]

if tomllib is not None:
    value = tomllib.loads(text).get(key, "")
    if value is None:
        value = ""
    if not isinstance(value, str):
        print(f"Unsupported non-string value for {key}.", file=sys.stderr)
        raise SystemExit(1)
    print(value)
    raise SystemExit(0)

# The fallback intentionally reads only top-level scalar string answers used by
# this launcher. tomllib remains authoritative when available.
def strip_inline_comment(value):
    quote = None
    escaped = False
    for index, char in enumerate(value):
        if escaped:
            escaped = False
            continue
        if char == "\\":
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

pattern = re.compile(rf"^\s*{re.escape(key)}\s*=\s*(.*?)\s*$")
for line in text.splitlines():
    stripped = line.strip()
    if not stripped or stripped.startswith("#"):
        continue
    if stripped.startswith("["):
        break
    match = pattern.match(line)
    if match:
        print(ast.literal_eval(strip_inline_comment(match.group(1))))
        break
else:
    print("")
' "$ANSWERS_FILE" "$1"
}

JIG_VERSION="$(read_field jig_version)"
SRC_PATH="$(read_field _src_path)"
TEMPLATE_COMMIT="$(read_field _commit)"
TEMPLATE_SOURCE_URL="$(read_field template_source_url)"

if [[ -z "$JIG_VERSION" ]]; then
  echo "Failed to read jig_version from $ANSWERS_FILE." >&2
  exit 1
fi

if [[ -z "$SRC_PATH" ]]; then
  echo "Failed to read _src_path from $ANSWERS_FILE." >&2
  exit 1
fi

is_remote_source() {
  local source="$1"
  [[ "$source" == *"://"* || "$source" == git@*:* ]]
}

# JIG_INSTALL_PROFILE is for direct installer calls. The scripts/jig launcher
# passes --profile explicitly so command-aware routing wins over ambient env.
INSTALL_PROFILE="${JIG_INSTALL_PROFILE:-default}"
INSTALL_ROOT_ARG=""
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

case "$INSTALL_PROFILE" in
  default | runtime | mcp)
    ;;
  *)
    echo "Unsupported jig install profile: $INSTALL_PROFILE" >&2
    exit 2
    ;;
esac

if [[ -d "$ROOT_DIR/.git" ]]; then
  DEFAULT_INSTALL_BASE="$ROOT_DIR/.git/jig-tools"
else
  DEFAULT_INSTALL_BASE="$ROOT_DIR/.agent/.cache/jig"
fi

case "$INSTALL_PROFILE" in
  default)
    DEFAULT_INSTALL_ROOT="$DEFAULT_INSTALL_BASE/$JIG_VERSION"
    CARGO_INSTALL_FEATURE_ARGS=()
    ;;
  runtime | mcp)
    DEFAULT_INSTALL_ROOT="$DEFAULT_INSTALL_BASE/$JIG_VERSION-runtime"
    CARGO_INSTALL_FEATURE_ARGS=(--no-default-features)
    ;;
esac

INSTALL_ROOT="${INSTALL_ROOT_ARG:-$DEFAULT_INSTALL_ROOT}"
BIN_PATH="$INSTALL_ROOT/bin/jig"
INSTALL_LOCK_DIR="$INSTALL_ROOT.lock"
INSTALL_LOCK_ATTEMPTS=30
STALE_INSTALL_LOCK_SECONDS=300

binary_version() {
  local bin_path="$1"
  "$bin_path" --version 2>/dev/null | awk '{print $2}'
}

assert_exact_version() {
  local bin_path="$1"
  local actual_version
  actual_version="$(binary_version "$bin_path" || true)"
  if [[ "$actual_version" != "$JIG_VERSION" ]]; then
    echo "Expected jig version $JIG_VERSION, found ${actual_version:-<missing>} at $bin_path." >&2
    return 1
  fi
}

hash_stdin() {
  local digest
  if command -v sha256sum >/dev/null 2>&1; then
    digest="$(sha256sum | awk '{print $1}')"
    printf 'sha256:%s\n' "$digest"
    return
  fi
  if command -v shasum >/dev/null 2>&1; then
    digest="$(shasum -a 256 | awk '{print $1}')"
    printf 'sha256:%s\n' "$digest"
    return
  fi
  if command -v openssl >/dev/null 2>&1; then
    digest="$(openssl dgst -sha256 -r | awk '{print $1}')"
    printf 'sha256:%s\n' "$digest"
    return
  fi
  echo "No SHA-256 utility found; local jig source installs will not be cache-stamped." >&2
  return 1
}

local_source_stamp() {
  local source_root="$1"
  # Keep this path list aligned with the crates and manifests that feed the jig
  # binary; omitted build inputs can make the source-cache stamp stale.
  {
    git -C "$source_root" rev-parse HEAD 2>/dev/null || printf 'unknown-head\n'
    git -C "$source_root" diff HEAD -- Cargo.toml Cargo.lock crates/jig crates/jig-dev-proxy crates/jig-status-tui 2>/dev/null || true
  } | hash_stdin
}

local_source_install_is_current() {
  local source_root="$1"
  local stamp_path="$INSTALL_ROOT/.jig-source-stamp"

  [[ -x "$BIN_PATH" ]] || return 1
  assert_exact_version "$BIN_PATH" >/dev/null || return 1
  [[ -f "$stamp_path" ]] || return 1
  local current_stamp
  current_stamp="$(local_source_stamp "$source_root")" || return 1
  [[ "$(cat "$stamp_path")" == "$current_stamp" ]]
}

write_local_source_stamp() {
  local source_root="$1"
  local current_stamp
  local stamp_path="$INSTALL_ROOT/.jig-source-stamp"
  local temp_stamp="$stamp_path.$$"
  current_stamp="$(local_source_stamp "$source_root")" || {
    rm -f "$stamp_path"
    return 0
  }
  printf '%s\n' "$current_stamp" >"$temp_stamp"
  mv "$temp_stamp" "$stamp_path"
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

  if ! assert_exact_version "$dev_bin"; then
    echo "JIG_DEV_BIN must match jig version $JIG_VERSION; refusing to install a fallback binary." >&2
    echo "Rebuild from the jig source checkout with: cargo build -p jig-sh --bin jig" >&2
    echo "Then set JIG_DEV_BIN=target/debug/jig, unset JIG_DEV_BIN, or run scripts/jig so the normal cached installer path can select a compatible runtime." >&2
    exit 1
  fi
  # scripts/jig captures stdout from this installer and execs the printed path.
  printf '%s\n' "$dev_bin"
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
  mkdir -p "$(dirname "$INSTALL_ROOT")"
  local attempt
  attempt=1
  while [[ "$attempt" -le "$INSTALL_LOCK_ATTEMPTS" ]]; do
    if mkdir "$INSTALL_LOCK_DIR" 2>/dev/null; then
      trap release_install_lock EXIT
      return 0
    fi
    if install_lock_is_stale; then
      rmdir "$INSTALL_LOCK_DIR" 2>/dev/null || true
      continue
    fi
    sleep 1
    attempt=$((attempt + 1))
  done
  echo "Timed out waiting for jig installer lock: $INSTALL_LOCK_DIR" >&2
  if [[ -d "$INSTALL_LOCK_DIR" ]]; then
    # Downstream harnesses intentionally omit jig-sh source-checkout recovery advice.
    echo "Another scripts/jig install may still be running." >&2
  else
    echo "Could not create jig installer lock; check permissions for $(dirname "$INSTALL_LOCK_DIR")." >&2
  fi
  exit 1
}

install_lock_is_stale() {
  [[ -d "$INSTALL_LOCK_DIR" ]] || return 1
  local now mtime
  now="$(date +%s)"
  # macOS/BSD stat uses -f, GNU stat uses -c.
  if mtime="$(stat -f %m "$INSTALL_LOCK_DIR" 2>/dev/null)"; then
    :
  elif mtime="$(stat -c %Y "$INSTALL_LOCK_DIR" 2>/dev/null)"; then
    :
  else
    return 1
  fi
  [[ $((now - mtime)) -gt $STALE_INSTALL_LOCK_SECONDS ]]
}

release_install_lock() {
  if [[ -d "$INSTALL_LOCK_DIR" ]]; then
    rmdir "$INSTALL_LOCK_DIR" 2>/dev/null || true
  fi
}

install_from_local_source() {
  local source_root="$1"
  local crate_path="$source_root/crates/jig"
  if [[ ! -d "$crate_path" ]]; then
    echo "Expected local jig source at $crate_path." >&2
    return 1
  fi

  cargo install \
    --path "$crate_path" \
    --root "$INSTALL_ROOT" \
    --locked \
    --force \
    "${CARGO_INSTALL_FEATURE_ARGS[@]}"

  assert_exact_version "$BIN_PATH"
  write_local_source_stamp "$source_root"
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
  local git_ref_args=(--tag "v$JIG_VERSION")
  if [[ "$TEMPLATE_COMMIT" =~ ^[0-9a-fA-F]{7,40}$ ]]; then
    # Adopted repos pin the exact template revision in .jig.toml. Treat that
    # commit as trusted repo configuration: a hex value intentionally overrides
    # the release tag so updates install the same source revision that rendered
    # the repo-local harness.
    git_ref_args=(--rev "$TEMPLATE_COMMIT")
  fi

  cargo install \
    --git "$SRC_PATH" \
    "${git_ref_args[@]}" \
    --root "$INSTALL_ROOT" \
    --locked \
    --force \
    "${CARGO_INSTALL_FEATURE_ARGS[@]}" \
    jig-sh

  assert_exact_version "$BIN_PATH"
}

if [[ -n "${JIG_DEV_BIN:-}" ]]; then
  install_from_dev_bin
  exit 0
fi

# The jig-sh source repo dogfoods generated harness files. Prefer a cache that
# was built from the current checkout over an older same-version release cache.
# Explicit install roots keep the lower-level installer behavior so callers can
# populate exactly the root they requested.
if [[ -z "$INSTALL_ROOT_ARG" ]] && is_jig_source_checkout "$ROOT_DIR"; then
  if local_source_install_is_current "$ROOT_DIR"; then
    printf '%s\n' "$BIN_PATH"
    exit 0
  fi

  acquire_install_lock

  if local_source_install_is_current "$ROOT_DIR"; then
    printf '%s\n' "$BIN_PATH"
    exit 0
  fi

  install_from_local_source "$ROOT_DIR"
  printf '%s\n' "$BIN_PATH"
  exit 0
fi

if [[ "$INSTALL_PROFILE" != "default" && -z "$INSTALL_ROOT_ARG" ]]; then
  # Runtime and MCP profiles are subsets of the default binary. Reuse a matching
  # full build instead of compiling a stripped binary when it already exists.
  FULL_BIN_PATH="$DEFAULT_INSTALL_BASE/$JIG_VERSION/bin/jig"
  if [[ -x "$FULL_BIN_PATH" ]] && assert_exact_version "$FULL_BIN_PATH"; then
    printf '%s\n' "$FULL_BIN_PATH"
    exit 0
  fi
fi

if [[ -x "$BIN_PATH" ]] && assert_exact_version "$BIN_PATH"; then
  printf '%s\n' "$BIN_PATH"
  exit 0
fi

acquire_install_lock

if [[ -x "$BIN_PATH" ]] && assert_exact_version "$BIN_PATH"; then
  printf '%s\n' "$BIN_PATH"
  exit 0
fi

if [[ -d "$SRC_PATH/crates/jig" ]] || [[ "$SRC_PATH" == /* && -d "$SRC_PATH" ]]; then
  install_from_local_source "$SRC_PATH"
elif [[ -n "$TEMPLATE_SOURCE_URL" ]]; then
  SRC_PATH="$TEMPLATE_SOURCE_URL"
  install_from_git_source
elif is_remote_source "$SRC_PATH"; then
  install_from_git_source
else
  echo "Cannot resolve jig source from _src_path='$SRC_PATH'." >&2
  echo "Re-render from an absolute committed template path or set template_source_url." >&2
  exit 1
fi

printf '%s\n' "$BIN_PATH"
