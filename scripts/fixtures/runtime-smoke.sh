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
if "No prebuilt jig" not in stderr:
    raise SystemExit(f"Missing prebuilt-binary error, got stderr:\n{stderr}")
if "cargo install" not in stderr:
    raise SystemExit(f"Missing no-cargo-install explanation, got stderr:\n{stderr}")
PY
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
    local jig_version
    local install_base

    jig_version="$(answers_get .jig.toml jig_version)"
    if [[ -d .git ]]; then
      install_base=".git/jig-tools"
    else
      install_base=".agent/.cache/jig"
    fi

    assert_default_binary_state() {
      if [[ "$expect_dev_proxy" == "1" ]]; then
        [[ -x "$install_base/$jig_version/bin/jig" ]]
      else
        [[ ! -e "$install_base/$jig_version/bin/jig" ]]
      fi
    }

    rm -rf .git/jig-tools .agent/.cache
    assert_jig_mcp_requires_prebuilt_binary "$repo_dir"
    # MCP startup must use a prebuilt binary; check contract populates the runtime cache.
    env -u JIG_DEV_BIN scripts/jig check contract >/dev/null
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
    [[ -x "$install_base/$jig_version-runtime/bin/jig" ]]
    assert_default_binary_state
    if "$install_base/$jig_version-runtime/bin/jig" proxy list >/dev/null 2>&1; then
      echo "runtime profile unexpectedly supports proxy commands" >&2
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
    [[ -x "$install_base/$jig_version/bin/jig" ]]

    env -u JIG_DEV_BIN scripts/install-jig.sh >/dev/null

    work_json="$(scripts/jig work start --json --title "Fixture runtime plan" --body "## Fixture\nRuntime validation.")"
    plan_id="$(printf '%s' "$work_json" | python3 -c 'import json,sys; print(json.load(sys.stdin)["plan"]["plan_id"])')"

    if [[ "$expect_sqlx" == "1" ]]; then
      scripts/jig migration-add "$migration_name" --plan-id "$plan_id" >/dev/null
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
    "jig.test",
    "jig.decisions_add",
}
if os.environ["EXPECT_SQLX"] == "1":
    required.update({"jig.sqlx_check", "jig.migration_add"})
if os.environ["EXPECT_SCHEMA_DUMP"] == "1":
    required.update({"jig.schema_check", "jig.schema_dump"})

missing = sorted(required - tools)
if missing:
    raise SystemExit(f"Missing expected runtime receipts: {', '.join(missing)}")
PY

    scripts/jig work finish --plan-id "$plan_id" --resolution "fixture complete" --outcome success >/dev/null

    [[ -f ".agent/plans/${plan_id}.md" ]]
    grep -q "Runtime validation" ".agent/plans/${plan_id}.md"
    [[ -f .agent/state/receipts.jsonl ]]
    [[ -f .agent/state/decisions.jsonl ]]
    [[ -f "$install_base/$jig_version-runtime/bin/jig" ]]
    [[ -f "$install_base/$jig_version/bin/jig" ]]
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
