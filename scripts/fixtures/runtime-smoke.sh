#!/usr/bin/env bash

if ! declare -F json_get >/dev/null; then
  source "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd -P)/lib.sh"
fi

validate_jig_mcp_smoke() {
  local repo_dir="$1"
  local expect_schema_dump="$2"
  local expect_sqlx="$3"

  REPO_DIR="$repo_dir" EXPECT_SCHEMA_DUMP="$expect_schema_dump" EXPECT_SQLX="$expect_sqlx" python3 <<'PY'
import json
import os
import pathlib
import select
import subprocess
import sys
import tempfile

repo_dir = pathlib.Path(os.environ["REPO_DIR"])
expect_schema_dump = os.environ["EXPECT_SCHEMA_DUMP"] == "1"
expect_sqlx = os.environ["EXPECT_SQLX"] == "1"
stderr_file = tempfile.TemporaryFile()
proc = None

def send(message):
    proc.stdin.write(json.dumps(message).encode() + b"\n")
    proc.stdin.flush()

def recv():
    readable, _, _ = select.select([proc.stdout], [], [], 5)
    if not readable:
        raise RuntimeError("Timed out waiting for MCP server response")
    line = proc.stdout.readline()
    if not line:
        raise RuntimeError("MCP server closed stdout unexpectedly")
    return json.loads(line)

def print_mcp_stderr():
    stderr_file.flush()
    stderr_file.seek(0)
    stderr = stderr_file.read().decode(errors="replace")
    if stderr:
        print("MCP server stderr:", file=sys.stderr)
        print(stderr, file=sys.stderr, end="" if stderr.endswith("\n") else "\n")

try:
    proc = subprocess.Popen(
        [str(repo_dir / "scripts" / "jig"), "mcp"],
        cwd=repo_dir,
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=stderr_file,
        env={key: value for key, value in os.environ.items() if key != "JIG_DEV_BIN"},
    )

    send({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "protocolVersion": "2025-06-18",
            "capabilities": {},
            "clientInfo": {"name": "fixture", "version": "1"},
        },
    })
    response = recv()
    assert response["result"]["serverInfo"]["name"] == "jig", response

    send({"jsonrpc": "2.0", "method": "notifications/initialized"})
    send({"jsonrpc": "2.0", "id": 2, "method": "tools/list", "params": {}})
    response = recv()
    tool_names = {tool["name"] for tool in response["result"]["tools"]}
    assert "jig.fmt_check" in tool_names, tool_names
    assert ("jig.schema_check" in tool_names) == expect_schema_dump, tool_names
    assert ("jig.schema_dump" in tool_names) == expect_schema_dump, tool_names
    assert ("jig.sqlx_check" in tool_names) == expect_sqlx, tool_names
    assert ("jig.migration_add" in tool_names) == expect_sqlx, tool_names
    assert "jig.agent_doctor" in tool_names, tool_names
    assert "jig.work_start" in tool_names, tool_names
    assert "jig.session_start" not in tool_names, tool_names

    send({
        "jsonrpc": "2.0",
        "id": 3,
        "method": "tools/call",
        "params": {
            "name": "jig.work_status",
            "arguments": {},
        },
    })
    response = recv()
    content = response["result"]["structuredContent"]
    assert content["ok"] is True, response
    assert "counts" in content, response
except Exception:
    print_mcp_stderr()
    raise
finally:
    if proc is not None and proc.poll() is None:
        proc.terminate()
        try:
            proc.wait(timeout=5)
        except subprocess.TimeoutExpired:
            proc.kill()
            try:
                proc.wait(timeout=5)
            except subprocess.TimeoutExpired:
                pass
    stderr_file.close()
PY
}

assert_jig_mcp_requires_prebuilt_binary() {
  local repo_dir="$1"

  REPO_DIR="$repo_dir" python3 <<'PY'
import os
import pathlib
import subprocess

repo_dir = pathlib.Path(os.environ["REPO_DIR"])
proc = subprocess.run(
    [str(repo_dir / "scripts" / "jig"), "mcp"],
    cwd=repo_dir,
    input=b"",
    stdout=subprocess.PIPE,
    stderr=subprocess.PIPE,
    env={key: value for key, value in os.environ.items() if key != "JIG_DEV_BIN"},
    timeout=5,
)

if proc.returncode == 0:
    raise SystemExit("scripts/jig mcp unexpectedly succeeded without a prebuilt binary")

stderr = proc.stderr.decode(errors="replace")
if "No prebuilt Jig" not in stderr:
    raise SystemExit(f"Missing prebuilt-binary error, got stderr:\n{stderr}")
if "cargo install" not in stderr:
    raise SystemExit(f"Missing no-cargo-install explanation, got stderr:\n{stderr}")
PY
}

assert_compatible_path_binary_is_reused_without_executing_wrappers() {
  local repo_dir="$1"
  local fake_dir="$repo_dir/.agent/.cache/path-version-fixture"
  local stderr_file="$repo_dir/.agent/.cache/path-version-fixture.stderr"
  local wrapper_log="$repo_dir/.agent/.cache/path-wrapper.log"
  local expected
  local selected

  mkdir -p "$fake_dir"
  printf '%s\n' \
    'printf "%s\n" "$*" >>"$JIG_FIXTURE_WRAPPER_LOG"' \
    'exit 0' \
    >"$fake_dir/jig"
  chmod +x "$fake_dir/jig"

  if (
    cd "$repo_dir"
    env -u JIG_DEV_BIN \
      JIG_INSTALL_ALLOW_PATH_BINARY=1 \
      JIG_FIXTURE_WRAPPER_LOG="$wrapper_log" \
      PATH="$fake_dir:$PATH" \
      scripts/install-jig.sh --profile runtime --resolve-only >/dev/null 2>"$stderr_file"
  ); then
    echo "A PATH shell wrapper was unexpectedly accepted as a Jig runtime binary." >&2
    exit 1
  fi
  if [[ -e "$wrapper_log" ]]; then
    echo "PATH runtime discovery executed a rejected shell wrapper." >&2
    exit 1
  fi

  cp "$JIG_DEV_BIN" "$fake_dir/jig"
  chmod +x "$fake_dir/jig"
  expected="$(cd "$fake_dir" && pwd -P)/jig"

  selected="$({
    cd "$repo_dir"
    env -u JIG_DEV_BIN JIG_INSTALL_ALLOW_PATH_BINARY=1 PATH="$fake_dir:$PATH" \
      scripts/install-jig.sh --profile runtime 2>"$stderr_file"
  })"
  if [[ "$selected" != "$expected" ]]; then
    echo "Expected a contract-compatible PATH runtime binary to be reused." >&2
    echo "Expected: $expected" >&2
    echo "Actual:   $selected" >&2
    exit 1
  fi
  if ! grep -Fq "Using explicitly allowed PATH Jig binary: $expected" "$stderr_file"; then
    echo "Expected PATH binary reuse to be reported on stderr." >&2
    exit 1
  fi
}

assert_capability_discovery_does_not_reinstall_after_strict_failure() {
  local repo_dir="$1"
  local fake_dir="$repo_dir/.agent/.cache/capability-discovery-fixture"
  local probe_log="$repo_dir/.agent/.cache/capability-discovery.log"
  local cargo_log="$repo_dir/.agent/.cache/capability-discovery-cargo.log"
  local stderr_file="$repo_dir/.agent/.cache/capability-discovery.stderr"
  local invalid_check_stderr="$repo_dir/.agent/.cache/capability-invalid-check.stderr"
  local unknown_check_stderr="$repo_dir/.agent/.cache/capability-unknown-check.stderr"
  local unknown_command_stderr="$repo_dir/.agent/.cache/capability-unknown-command.stderr"
  local misplaced_version_stderr="$repo_dir/.agent/.cache/capability-misplaced-version.stderr"

  mkdir -p "$fake_dir"
  printf '%s\n' \
    '#!/bin/sh' \
    'printf "%s\n" "$*" >>"$JIG_FIXTURE_PROBE_LOG"' \
    'if [ "${1:-}" = "--__launcher-contract-version" ]; then' \
    '  printf "%s\n" "The repository contract did not validate under Jig profile runtime." >&2' \
    '  printf "%s\n" "fixture strict repository validation failed" >&2' \
    '  exit 42' \
    'fi' \
    'if [ "${1:-}" != "__runtime-compatible" ]; then' \
    '  if [ "${1:-}" = typo ]; then' \
    '    printf "%s\n" "error: unrecognized subcommand '\''typo'\''" >&2' \
    '    exit 2' \
    '  fi' \
    '  if [ "${1:-}" = check ] && [ "${2:-}" = typo ]; then' \
    '    printf "%s\n" "error: unrecognized subcommand '\''typo'\''" >&2' \
    '    exit 2' \
    '  fi' \
    '  exit 0' \
    'fi' \
    'for arg in "$@"; do [ "$arg" = "--capability-only" ] && exit 0; done' \
    'printf "%s\n" "fixture strict repository validation failed" >&2' \
    'exit 42' \
    >"$fake_dir/jig"
  printf '%s\n' \
    '#!/bin/sh' \
    'printf "%s\n" "cargo was invoked" >"$JIG_FIXTURE_CARGO_LOG"' \
    'exit 88' \
    >"$fake_dir/cargo"
  chmod +x "$fake_dir/jig" "$fake_dir/cargo"

  if (
    cd "$repo_dir"
    JIG_DEV_BIN="$fake_dir/jig" \
      JIG_FIXTURE_PROBE_LOG="$probe_log" \
      JIG_FIXTURE_CARGO_LOG="$cargo_log" \
      PATH="$fake_dir:$PATH" \
      scripts/jig info >/dev/null 2>"$stderr_file"
  ); then
    echo "Strict repository validation unexpectedly succeeded." >&2
    exit 1
  fi

  grep -q -- '--capability-only' "$probe_log"
  grep -q 'fixture strict repository validation failed' "$stderr_file"
  grep -q 'repository contract did not validate' "$stderr_file"
  if grep -q 'Resolved Jig binary is incompatible' "$stderr_file"; then
    echo "Strict repository validation was misreported as binary incompatibility." >&2
    exit 1
  fi
  if [[ -e "$cargo_log" ]]; then
    echo "Capability-compatible runtime selection invoked cargo after strict validation failed." >&2
    exit 1
  fi

  if (
    cd "$repo_dir"
    JIG_DEV_BIN="$fake_dir/jig" \
      JIG_FIXTURE_PROBE_LOG="$probe_log" \
      JIG_FIXTURE_CARGO_LOG="$cargo_log" \
      PATH="$fake_dir:$PATH" \
      scripts/jig check --bogus contract >/dev/null 2>"$invalid_check_stderr"
  ); then
    echo "Malformed check invocation unexpectedly used capability-only validation." >&2
    exit 1
  fi
  grep -q 'fixture strict repository validation failed' "$invalid_check_stderr"

  if (
    cd "$repo_dir"
    JIG_DEV_BIN="$fake_dir/jig" \
      JIG_FIXTURE_PROBE_LOG="$probe_log" \
      JIG_FIXTURE_CARGO_LOG="$cargo_log" \
      PATH="$fake_dir:$PATH" \
      scripts/jig check typo >/dev/null 2>"$unknown_check_stderr"
  ); then
    echo "Unknown check subcommand unexpectedly succeeded." >&2
    exit 1
  fi
  grep -q "unrecognized subcommand 'typo'" "$unknown_check_stderr"
  if grep -q 'fixture strict repository validation failed' "$unknown_check_stderr"; then
    echo "Strict repository validation hid the unknown check subcommand diagnostic." >&2
    exit 1
  fi

  if (
    cd "$repo_dir"
    JIG_DEV_BIN="$fake_dir/jig" \
      JIG_FIXTURE_PROBE_LOG="$probe_log" \
      JIG_FIXTURE_CARGO_LOG="$cargo_log" \
      PATH="$fake_dir:$PATH" \
      scripts/jig typo >/dev/null 2>"$unknown_command_stderr"
  ); then
    echo "Unknown top-level command unexpectedly succeeded." >&2
    exit 1
  fi
  grep -q "unrecognized subcommand 'typo'" "$unknown_command_stderr"
  if grep -q 'fixture strict repository validation failed' "$unknown_command_stderr"; then
    echo "Strict repository validation hid the unknown top-level command diagnostic." >&2
    exit 1
  fi

  if (
    cd "$repo_dir"
    JIG_DEV_BIN="$fake_dir/jig" \
      JIG_FIXTURE_PROBE_LOG="$probe_log" \
      JIG_FIXTURE_CARGO_LOG="$cargo_log" \
      PATH="$fake_dir:$PATH" \
      scripts/jig check test -V >/dev/null 2>"$misplaced_version_stderr"
  ); then
    echo "A misplaced version flag unexpectedly bypassed strict repository validation." >&2
    exit 1
  fi
  grep -q 'fixture strict repository validation failed' "$misplaced_version_stderr"

  (
    cd "$repo_dir"
    JIG_DEV_BIN="$fake_dir/jig" \
      JIG_FIXTURE_PROBE_LOG="$probe_log" \
      JIG_FIXTURE_CARGO_LOG="$cargo_log" \
      PATH="$fake_dir:$PATH" \
      scripts/jig doctor >/dev/null
  )
  (
    cd "$repo_dir"
    JIG_DEV_BIN="$fake_dir/jig" \
      JIG_FIXTURE_PROBE_LOG="$probe_log" \
      JIG_FIXTURE_CARGO_LOG="$cargo_log" \
      PATH="$fake_dir:$PATH" \
      scripts/jig -- doctor >/dev/null
  )
  (
    cd "$repo_dir"
    JIG_DEV_BIN="$fake_dir/jig" \
      JIG_FIXTURE_PROBE_LOG="$probe_log" \
      JIG_FIXTURE_CARGO_LOG="$cargo_log" \
      PATH="$fake_dir:$PATH" \
      scripts/jig --json check contract --no-receipt >/dev/null
  )
  (
    cd "$repo_dir"
    JIG_DEV_BIN="$fake_dir/jig" \
      JIG_FIXTURE_PROBE_LOG="$probe_log" \
      JIG_FIXTURE_CARGO_LOG="$cargo_log" \
      PATH="$fake_dir:$PATH" \
      scripts/jig check --json contract --no-receipt >/dev/null
  )
  (
    cd "$repo_dir"
    JIG_DEV_BIN="$fake_dir/jig" \
      JIG_FIXTURE_PROBE_LOG="$probe_log" \
      JIG_FIXTURE_CARGO_LOG="$cargo_log" \
      PATH="$fake_dir:$PATH" \
      scripts/jig check contract --json --no-receipt >/dev/null
  )
  (
    cd "$repo_dir"
    JIG_DEV_BIN="$fake_dir/jig" \
      JIG_FIXTURE_PROBE_LOG="$probe_log" \
      JIG_FIXTURE_CARGO_LOG="$cargo_log" \
      PATH="$fake_dir:$PATH" \
      scripts/jig update --launcher-only >/dev/null
  )
  (
    cd "$repo_dir"
    JIG_DEV_BIN="$fake_dir/jig" \
      JIG_FIXTURE_PROBE_LOG="$probe_log" \
      JIG_FIXTURE_CARGO_LOG="$cargo_log" \
      PATH="$fake_dir:$PATH" \
      scripts/jig adopt . --write >/dev/null
  )
  (
    cd "$repo_dir"
    JIG_DEV_BIN="$fake_dir/jig" \
      JIG_FIXTURE_PROBE_LOG="$probe_log" \
      JIG_FIXTURE_CARGO_LOG="$cargo_log" \
      PATH="$fake_dir:$PATH" \
      scripts/jig --version >/dev/null
  )
  (
    cd "$repo_dir"
    JIG_DEV_BIN="$fake_dir/jig" \
      JIG_FIXTURE_PROBE_LOG="$probe_log" \
      JIG_FIXTURE_CARGO_LOG="$cargo_log" \
      PATH="$fake_dir:$PATH" \
      scripts/jig -V >/dev/null
  )
  grep -Fxq 'doctor' "$probe_log"
  grep -Fxq -- '-- doctor' "$probe_log"
  grep -Fxq -- '--json check contract --no-receipt' "$probe_log"
  grep -Fxq -- 'check --json contract --no-receipt' "$probe_log"
  grep -Fxq -- 'check contract --json --no-receipt' "$probe_log"
  grep -Fxq -- 'update --launcher-only' "$probe_log"
  grep -Fxq -- 'adopt . --write' "$probe_log"
  grep -Fxq -- '--version' "$probe_log"
  grep -Fxq -- '-V' "$probe_log"
}

assert_repository_independent_commands_skip_strict_validation() {
  local repo_dir="$1"
  local fake_dir="$repo_dir/.agent/bare-invocation-fixture"
  local fake_jig="$fake_dir/jig"
  local output

  mkdir -p "$fake_dir"
  printf '%s\n' \
    '#!/bin/sh' \
    'if [ "${1:-}" = "__runtime-compatible" ]; then' \
    '  for arg in "$@"; do [ "$arg" = "--capability-only" ] && exit 0; done' \
    '  exit 42' \
    'fi' \
    'if [ "$#" -eq 0 ] || { [ "$#" -eq 1 ] && [ "$1" = "--json" ]; }; then printf "%s\n" "fixture bare help"; exit 0; fi' \
    'case "${1:-}" in init|presets|codex) printf "%s\n" "fixture $1"; exit 0 ;; esac' \
    'exit 99' \
    >"$fake_jig"
  chmod +x "$fake_jig"

  output="$({
    cd "$repo_dir"
    JIG_DEV_BIN="$fake_jig" scripts/jig
  })"
  [[ "$output" == "fixture bare help" ]]
  output="$({
    cd "$repo_dir"
    JIG_DEV_BIN="$fake_jig" scripts/jig --json
  })"
  [[ "$output" == "fixture bare help" ]]
  output="$({
    cd "$repo_dir"
    JIG_DEV_BIN="$fake_jig" scripts/jig init fixture-destination
  })"
  [[ "$output" == "fixture init" ]]
  output="$({
    cd "$repo_dir"
    JIG_DEV_BIN="$fake_jig" scripts/jig presets
  })"
  [[ "$output" == "fixture presets" ]]
  output="$({
    cd "$repo_dir"
    JIG_DEV_BIN="$fake_jig" scripts/jig codex homes
  })"
  [[ "$output" == "fixture codex" ]]

  rm -rf "$repo_dir/.git/jig-tools" "$repo_dir/.agent/.cache" "$fake_dir"
}

assert_doctor_prefers_cached_resolution() {
  local repo_dir="$1"
  local installer="$repo_dir/scripts/install-jig.sh"
  local backup="$repo_dir/.agent/.cache/doctor-installer-backup"
  local fake_jig="$repo_dir/.agent/.cache/doctor-resolve-jig"
  local installer_log="$repo_dir/.agent/.cache/doctor-resolve-installer.log"

  (
    mkdir -p "$repo_dir/.agent/.cache"
    cp "$installer" "$backup"
    trap 'cp "$backup" "$installer"; rm -f "$backup" "$fake_jig" "$installer_log"' EXIT
    printf '%s\n' \
      '#!/bin/sh' \
      'printf "%s\n" "$*" >>"$JIG_FIXTURE_INSTALLER_LOG"' \
      'printf "%s\n" "$JIG_FIXTURE_DOCTOR_BIN"' \
      >"$installer"
    printf '%s\n' \
      '#!/bin/sh' \
      '[ "${1:-}" = doctor ]' \
      >"$fake_jig"
    chmod +x "$installer" "$fake_jig"

    cd "$repo_dir"
    env -u JIG_DEV_BIN \
      JIG_FIXTURE_INSTALLER_LOG="$installer_log" \
      JIG_FIXTURE_DOCTOR_BIN="$fake_jig" \
      scripts/jig doctor >/dev/null
    [[ "$(wc -l <"$installer_log" | tr -d ' ')" == "1" ]]
    grep -q -- '--profile runtime --resolve-only' "$installer_log"
  )
}

assert_incompatible_jig_dev_bin_is_authoritative() {
  local repo_dir="$1"
  local fake_bin="$repo_dir/.agent/.cache/incompatible-jig"
  local stderr_file="$repo_dir/.agent/.cache/incompatible-jig.stderr"

  printf '%s\n' \
    '#!/bin/sh' \
    'if [ "${1:-}" = "--version" ]; then printf "%s\n" "jig 99.0.0"; exit 0; fi' \
    'exit 1' \
    >"$fake_bin"
  chmod +x "$fake_bin"

  if JIG_DEV_BIN="$fake_bin" "$repo_dir/scripts/install-jig.sh" --profile runtime \
    > /dev/null 2>"$stderr_file"; then
    echo "An incompatible JIG_DEV_BIN unexpectedly fell back to another binary." >&2
    exit 1
  fi
  if ! grep -q 'JIG_DEV_BIN is authoritative' "$stderr_file"; then
    echo "The incompatible JIG_DEV_BIN failure did not explain the authoritative override." >&2
    cat "$stderr_file" >&2
    exit 1
  fi
}

assert_malformed_answers_keep_diagnostics_reachable() {
  local repo_dir="$1"

  (
    cd "$repo_dir"
    local answers_backup doctor_stdout doctor_stderr help_stdout help_stderr
    answers_backup="$(mktemp "$repo_dir/.jig.toml.fixture-backup.XXXXXX")"
    doctor_stdout="$(mktemp "$repo_dir/.doctor-stdout.XXXXXX")"
    doctor_stderr="$(mktemp "$repo_dir/.doctor-stderr.XXXXXX")"
    help_stdout="$(mktemp "$repo_dir/.help-stdout.XXXXXX")"
    help_stderr="$(mktemp "$repo_dir/.help-stderr.XXXXXX")"
    cp .jig.toml "$answers_backup"
    trap 'mv "$answers_backup" .jig.toml; rm -f "$doctor_stdout" "$doctor_stderr" "$help_stdout" "$help_stderr"' EXIT
    printf '%s\n' '[malformed' >.jig.toml

    env -u JIG_DEV_BIN scripts/jig doctor --json >"$doctor_stdout" 2>"$doctor_stderr" || true
    if grep -Fq 'Traceback (most recent call last)' "$doctor_stderr"; then
      echo "Malformed .jig.toml leaked a Python traceback before doctor could run." >&2
      exit 1
    fi
    python3 -c '
import json
import sys

payload = json.load(sys.stdin)
assert payload["command"] == "doctor", payload
checks = {check["id"]: check for check in payload["checks"]}
config = checks["config"]
assert config["status"] == "invalid", config
assert "Failed to parse" in config["detail"], config
' <"$doctor_stdout"

    env -u JIG_DEV_BIN scripts/jig --help >"$help_stdout" 2>"$help_stderr"
    if grep -Fq 'Traceback (most recent call last)' "$help_stderr"; then
      echo "Malformed .jig.toml leaked a Python traceback before help could run." >&2
      exit 1
    fi
    grep -Fq 'Usage:' "$help_stdout"
  )
}

validate_jig_runtime() {
  local repo_dir="$1"
  local expect_schema_dump="$2"
  local expect_sqlx="$3"
  local migration_name="${4:-}"
  local expect_dev_proxy="${5:-0}"

  (
    cd "$repo_dir"
    [[ -f .mcp.json ]]
    [[ -f .agent/jig-contract.json ]]
    scripts/jig check contract >/dev/null

    EXPECT_SCHEMA_DUMP="$expect_schema_dump" EXPECT_SQLX="$expect_sqlx" python3 <<'PY'
import json
import os
import pathlib

manifest = json.loads(pathlib.Path(".agent/jig-contract.json").read_text())
expect_schema_dump = os.environ["EXPECT_SCHEMA_DUMP"] == "1"
expect_sqlx = os.environ["EXPECT_SQLX"] == "1"
commands = set(manifest.get("required_commands", []))
tools = {tool["name"] for tool in manifest["tools"]}
tools_by_name = {tool["name"]: tool for tool in manifest["tools"]}

assert ("schema_dump_command" in commands) == expect_schema_dump, manifest
assert ("jig.schema_dump" in tools) == expect_schema_dump, manifest
assert ("jig.schema_check" in tools) == expect_schema_dump, manifest
assert ("sqlx_check_command" in commands) == expect_sqlx, manifest
assert ("jig.sqlx_check" in tools) == expect_sqlx, manifest
assert ("jig.migration_add" in tools) == expect_sqlx, manifest
if "jig.contract_check" in tools_by_name:
    assert tools_by_name["jig.contract_check"]["kind"] == "native", manifest
if "jig.migration_add" in tools_by_name:
    assert tools_by_name["jig.migration_add"]["kind"] == "native", manifest
if "jig.schema_check" in tools_by_name:
    assert tools_by_name["jig.schema_check"]["kind"] == "native", manifest
assert "jig.session_start" not in tools, manifest
PY

    local work_json
    local plan_id
    local receipts_json
    local contract_version
    local contract_cache_key
    local install_base

    contract_version="$(json_get contract_version < .agent/jig-contract.json)"
    contract_cache_key="contract-$contract_version"
    if [[ -d .git ]]; then
      install_base=".git/jig-tools"
    else
      install_base=".agent/.cache/jig"
    fi

    assert_default_binary_state() {
      if [[ "$expect_dev_proxy" == "1" ]]; then
        [[ -x "$install_base/$contract_cache_key/bin/jig" ]]
      else
        [[ ! -e "$install_base/$contract_cache_key/bin/jig" ]]
      fi
    }

    rm -rf .git/jig-tools .agent/.cache
    assert_jig_mcp_requires_prebuilt_binary "$repo_dir"
    assert_repository_independent_commands_skip_strict_validation "$repo_dir"
    assert_doctor_prefers_cached_resolution "$repo_dir"
    assert_capability_discovery_does_not_reinstall_after_strict_failure "$repo_dir"
    assert_compatible_path_binary_is_reused_without_executing_wrappers "$repo_dir"
    # MCP startup must use a prebuilt binary; check contract populates the runtime cache.
    env -u JIG_DEV_BIN scripts/jig check contract >/dev/null
    assert_malformed_answers_keep_diagnostics_reachable "$repo_dir"
    assert_incompatible_jig_dev_bin_is_authoritative "$repo_dir"
    doctor_json="$(env -u JIG_DEV_BIN scripts/jig doctor --json || true)"
    DOCTOR_JSON="$doctor_json" EXPECT_DEV_PROXY="$expect_dev_proxy" python3 <<'PY'
import json
import os

payload = json.loads(os.environ["DOCTOR_JSON"])
checks = {check["id"]: check for check in payload["checks"]}
proxy = checks["proxy"]
expect_dev_proxy = os.environ["EXPECT_DEV_PROXY"] == "1"

if expect_dev_proxy:
    assert proxy["data"]["configured"] is True, proxy
    assert proxy["status"] in {"running", "not running"}, proxy
    assert "built without" not in proxy["detail"], proxy
else:
    assert proxy["ok"] is True, proxy
    assert proxy["status"] == "not configured", proxy
    assert proxy["data"]["configured"] is False, proxy
PY
    validate_jig_mcp_smoke "$repo_dir" "$expect_schema_dump" "$expect_sqlx"
    [[ -x "$install_base/$contract_cache_key-runtime/bin/jig" ]]
    assert_default_binary_state
    "$install_base/$contract_cache_key-runtime/bin/jig" __runtime-compatible --profile runtime .
    "$install_base/$contract_cache_key-runtime/bin/jig" __runtime-compatible --profile mcp .
    if "$install_base/$contract_cache_key-runtime/bin/jig" __runtime-compatible --profile default . >/dev/null 2>&1; then
      echo "runtime profile unexpectedly satisfied the default profile" >&2
      exit 1
    fi
    assert_default_binary_state
    env -u JIG_DEV_BIN JIG_INSTALL_PROFILE=default scripts/jig check contract >/dev/null
    assert_default_binary_state
    env -u JIG_DEV_BIN scripts/jig dev --help >/dev/null
    env -u JIG_DEV_BIN scripts/jig proxy --help >/dev/null
    env -u JIG_DEV_BIN scripts/jig proxy list --help >/dev/null
    assert_default_binary_state
    env -u JIG_DEV_BIN JIG_INSTALL_PROFILE=runtime scripts/jig proxy list >/dev/null
    [[ -x "$install_base/$contract_cache_key/bin/jig" ]]
    "$install_base/$contract_cache_key/bin/jig" __runtime-compatible --profile default .

    # Exercise zero-argument lock re-entry under stock Bash without relying on
    # the generated installer's executable bit.
    (
      trap 'chmod +x scripts/install-jig.sh' EXIT
      chmod -x scripts/install-jig.sh
      env -u JIG_DEV_BIN /bin/bash scripts/install-jig.sh >/dev/null
    )

    work_json="$(scripts/jig work start --json --title "Fixture runtime plan" --body "## Fixture\nRuntime validation.")"
    plan_id="$(printf '%s' "$work_json" | python3 -c 'import json,sys; print(json.load(sys.stdin)["plan"]["plan_id"])')"

    if [[ "$expect_sqlx" == "1" ]]; then
      scripts/jig sqlx migration add "$migration_name" --plan-id "$plan_id" >/dev/null
    fi

    scripts/jig work check --plan-id "$plan_id" >/dev/null
    scripts/jig work gates --plan-id "$plan_id" >/dev/null

    scripts/jig work decide \
      --title "Fixture decision" \
      --selected-option "Use jig" \
      --rationale "Runtime contract is wired and validated." \
      --plan-id "$plan_id" \
      --alternatives "Ad-hoc shell commands" \
      >/dev/null

    receipts_json="$(scripts/jig work receipts --json --plan-id "$plan_id" --limit 20)"
    RECEIPTS_JSON="$receipts_json" EXPECT_SQLX="$expect_sqlx" EXPECT_SCHEMA_DUMP="$expect_schema_dump" python3 <<'PY'
import json
import os

payload = json.loads(os.environ["RECEIPTS_JSON"])
tools = {receipt["tool_name"] for receipt in payload["receipts"]}
required = {
    "jig.plans_open",
    "jig.contract_check",
    "jig.rust_file_loc",
    "jig.test",
    "jig.decisions_add",
}
if os.environ["EXPECT_SQLX"] == "1":
    required.update({"jig.sqlx_check", "jig.migration_add"})
if os.environ["EXPECT_SCHEMA_DUMP"] == "1":
    required.add("jig.schema_check")

missing = sorted(required - tools)
if missing:
    raise SystemExit(f"Missing expected runtime receipts: {', '.join(missing)}")
PY

    scripts/jig work finish --plan-id "$plan_id" --resolution "fixture complete" --outcome success >/dev/null

    [[ -f ".agent/plans/${plan_id}.md" ]]
    grep -q "Runtime validation" ".agent/plans/${plan_id}.md"
    [[ -f .agent/state/receipts.jsonl ]]
    [[ -f .agent/state/decisions.jsonl ]]
    [[ -f "$install_base/$contract_cache_key-runtime/bin/jig" ]]
    [[ -f "$install_base/$contract_cache_key/bin/jig" ]]
    if [[ "$expect_sqlx" == "1" ]]; then
      find crates/acme-db/migrations -name "*_${migration_name}.up.sql" | grep -q .
    fi
  )
}

if [[ "${BASH_SOURCE[0]}" == "$0" ]]; then
  set -euo pipefail
  if [[ "$#" -lt 3 || "$#" -gt 5 ]]; then
    echo "Usage: $0 REPO_DIR EXPECT_SCHEMA_DUMP EXPECT_SQLX [MIGRATION_NAME] [EXPECT_DEV_PROXY]" >&2
    exit 2
  fi

  validate_jig_runtime "$@"
fi
