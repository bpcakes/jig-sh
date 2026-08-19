#!/usr/bin/env bash

FIXTURE_SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd -P)"
ROOT_DIR="${ROOT_DIR:-$(cd "$FIXTURE_SCRIPT_DIR/../.." && pwd -P)}"

json_get() {
  local expression="$1"
  python3 -c '
import json
import sys

expr = sys.argv[1]
data = json.load(sys.stdin)

value = data
for segment in expr.split("."):
    if not segment:
        continue
    if segment.isdigit():
        value = value[int(segment)]
    else:
        value = value[segment]

if isinstance(value, (dict, list)):
    print(json.dumps(value))
else:
    print(value)
' "$expression"
}

answers_get() {
  python3 - "$1" "$2" <<'PY'
import ast
import os
import pathlib
import re
import sys

text = pathlib.Path(sys.argv[1]).read_text()
key = sys.argv[2]

if os.environ.get("JIG_FIXTURE_FORCE_NO_TOMLLIB") == "1":
    tomllib = None
else:
    try:
        import tomllib
    except ModuleNotFoundError:
        tomllib = None

if tomllib is not None:
    value = tomllib.loads(text).get(key, "")
    print("" if value is None else value)
    raise SystemExit(0)

# Fixture fallback is limited to top-level answer keys. tomllib remains
# authoritative when available.
def strip_inline_comment(value):
    quote = None
    escaped = False
    for index, char in enumerate(value):
        if quote == '"' and escaped:
            escaped = False
            continue
        if quote == '"' and char == "\\":
            escaped = True
            continue
        if quote is not None:
            if char == quote:
                quote = None
            continue
        if char in {"'", '"'}:
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
        raw_value = strip_inline_comment(match.group(1))
        if raw_value.startswith("'"):
            if len(raw_value) < 2 or not raw_value.endswith("'") or "'" in raw_value[1:-1]:
                raise ValueError("invalid TOML literal string")
            value = raw_value[1:-1]
        else:
            value = ast.literal_eval(raw_value)
        break
else:
    value = ""
print("" if value is None else value)
PY
}

answers_set() {
  python3 - "$1" "$2" "$3" <<'PY'
import pathlib
import re
import sys

path = pathlib.Path(sys.argv[1])
key = sys.argv[2]
raw_value = sys.argv[3]
lines = path.read_text().splitlines()
existing_is_bool = False

try:
    import tomllib
except ModuleNotFoundError:
    tomllib = None

if tomllib is not None:
    existing_is_bool = isinstance(tomllib.loads(path.read_text()).get(key), bool)

if existing_is_bool and raw_value.lower() in {"true", "false"}:
    replacement = f"{key} = {raw_value.lower()}"
else:
    value = raw_value.replace("\\", "\\\\").replace('"', '\\"')
    replacement = f'{key} = "{value}"'
pattern = re.compile(rf"^(\s*){re.escape(key)}\s*=")
for index, line in enumerate(lines):
    if pattern.match(line):
        lines[index] = replacement
        break
else:
    lines.append(replacement)
path.write_text("\n".join(lines) + "\n")
PY
}

render_fixture() {
  local answers_file="$1"
  local dest_dir="$2"

  run_jig init "$dest_dir" \
    --template "$ROOT_DIR" \
    --answers-file "$answers_file" \
    --preset harness-only \
    --no-input \
    --no-vault \
    --force >/dev/null
}

render_fixture_from_template() {
  local template_root="$1"
  local answers_file="$2"
  local dest_dir="$3"

  run_jig init "$dest_dir" \
    --template "$template_root" \
    --answers-file "$answers_file" \
    --preset harness-only \
    --no-input \
    --no-vault \
    --force >/dev/null
}

run_jig() {
  if [[ -n "${JIG_DEV_BIN:-}" ]]; then
    "$JIG_DEV_BIN" "$@"
    return
  fi

  cargo run -q -p jig-sh --bin jig -- "$@"
}

create_template_snapshot_repo() {
  local snapshot_dir="$1"
  local snapshot_archive
  local canonical_root canonical_snapshot

  mkdir -p "$snapshot_dir"
  canonical_root="$(cd "$ROOT_DIR" && pwd -P)"
  canonical_snapshot="$(cd "$snapshot_dir" && pwd -P)"
  case "$canonical_snapshot/" in
    "$canonical_root/"*)
      echo "Template snapshot destination must be outside the source repository: $canonical_snapshot" >&2
      return 1
      ;;
  esac
  snapshot_archive="$canonical_snapshot/.jig-template-snapshot.tar"
  (
    cd "$ROOT_DIR"
    tar cf "$snapshot_archive" \
      --exclude='.git' \
      --exclude='target' \
      --exclude='node_modules' \
      --exclude='*/node_modules' \
      --exclude='.agent/.cache' \
      .
  )
  (
    cd "$snapshot_dir"
    tar xf "$snapshot_archive"
  )
  rm -f "$snapshot_archive"

  (
    cd "$snapshot_dir"
    git init -b main >/dev/null
    git config user.name "Fixture"
    git config user.email "fixture@example.com"
    git add .
    git commit -m "template snapshot" >/dev/null
  )
}

fixture_create_tmp_dir_if_needed() {
  if [[ -z "${TMP_DIR:-}" ]]; then
    TMP_DIR="$(mktemp -d)"
    trap 'rm -rf "$TMP_DIR"' EXIT
  fi
}
