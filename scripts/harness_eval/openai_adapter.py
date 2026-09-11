#!/usr/bin/env python3
"""Explicit Responses API adapter using an existing OPENAI_API_KEY only.

Invocation: python3 openai_adapter.py REQUEST.json. No model defaults or fallback.
Tool execution is local, not a security sandbox. Use a disposable evaluation host.
"""

import hashlib
import json
import os
from pathlib import Path
import signal
import subprocess
import sys
import urllib.error
import urllib.request

if not __package__:
    # The standalone adapter runs from frozen inputs; keep them read-only.
    sys.dont_write_bytecode = True

if __package__:
    from .child_environment import command_environment
    from .process import capture_command
else:
    from child_environment import command_environment
    from process import capture_command

CLIENT = "jig-harness-responses"
VERSION = "1"
MAX_RESPONSES = 30
TOOL_TIMEOUT_SECONDS = 30


def save(path, value):
    temporary = path.with_suffix(".pending")
    temporary.write_text(json.dumps(value, indent=2, allow_nan=False) + "\n")
    temporary.replace(path)


def post(payload, api_key):
    request = urllib.request.Request("https://api.openai.com/v1/responses",
        data=json.dumps(payload).encode(), headers={"Authorization": "Bearer " + api_key,
                                                   "Content-Type": "application/json"})
    try:
        with urllib.request.urlopen(request, timeout=120) as response:
            return json.load(response)
    except urllib.error.HTTPError as error:
        # Do not put request headers, credentials, or unbounded remote bodies in logs.
        raise ValueError(f"Responses API HTTP {error.code}; no retry or model substitution") from None


def source_digest(root):
    parts = []
    for path in sorted(root.rglob("*")):
        if any(part in {".git", "target", "__pycache__", "node_modules"} for part in path.relative_to(root).parts):
            continue
        if path.is_file() and not path.is_symlink():
            parts.append([str(path.relative_to(root)), hashlib.sha256(path.read_bytes()).hexdigest()])
    return hashlib.sha256(json.dumps(parts).encode()).hexdigest()


def execute_tool(call, root):
    args = json.loads(call["arguments"])
    if call["name"] in {"read_file", "write_file"}:
        path = (root / args["path"]).resolve()
        if not path.is_relative_to(root) or ".git" in path.relative_to(root).parts:
            raise ValueError("file path must stay inside the fixture, outside .git")
        if call["name"] == "read_file":
            return path.read_text()
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(args["content"])
        return "written"
    if call["name"] != "run_command":
        raise ValueError("unknown tool")
    argv = args["argv"]
    if not isinstance(argv, list) or not argv or not all(isinstance(arg, str) for arg in argv):
        raise ValueError("argv must be a nonempty array of strings")
    return json.dumps(capture_command(argv, root, command_environment(), TOOL_TIMEOUT_SECONDS))


def evaluate(request, send):
    root = Path(request["workspace"]).resolve()
    output = Path(request["response_path"])
    requested = request["requested"]
    if (requested["client"], requested["client_version"]) != (CLIENT, VERSION):
        raise ValueError("config must select jig-harness-responses client version 1")
    tools = [{"type": "function", "name": tool["name"], "description": tool["description"],
              "parameters": tool["inputSchema"], "strict": True} for tool in request["tools"]]
    instructions = (root / "AGENTS.md").read_text()
    history = [{"role": "user", "content": request["prompt"]}]
    report = {"identity": {}, "tool_calls": [], "usage_tokens": None,
              "client_context_tokens": None, "provider_responses": [], "messages": [],
              "wire_descriptor_bytes": len(json.dumps(tools).encode()),
              "tool_trace_complete": False,
              "check_classification": "cargo test/check/clippy argv only; other verification is unclassified"}
    totals = {"input_tokens": 0, "output_tokens": 0, "cached_input_tokens": 0, "reasoning_tokens": 0}
    seen_identity = None
    for _ in range(MAX_RESPONSES):
        payload = {"model": requested["model"], "reasoning": {"effort": requested["reasoning"]},
                   "instructions": instructions, "input": history, "tools": tools,
                   "store": False, "include": ["reasoning.encrypted_content"],
                   "max_output_tokens": requested.get("max_output_tokens", 4096)}
        response = send(payload)
        identity = {"model": response.get("model"), "reasoning": (response.get("reasoning") or {}).get("effort"),
                    "client": CLIENT, "client_version": VERSION, "tools_sha256": request["tools_sha256"]}
        if seen_identity is not None and seen_identity != identity:
            report["identity_changed"] = True
            save(output, report)
            raise ValueError("provider changed model or reasoning during the trial")
        seen_identity = identity
        report["identity"] = identity
        usage = response.get("usage") or {}
        counts = {"input_tokens": usage.get("input_tokens"), "output_tokens": usage.get("output_tokens"),
                  "cached_input_tokens": (usage.get("input_tokens_details") or {}).get("cached_tokens"),
                  "reasoning_tokens": (usage.get("output_tokens_details") or {}).get("reasoning_tokens")}
        for key, value in counts.items():
            totals[key] = totals[key] + value if totals[key] is not None and value is not None else None
        report["usage_tokens"] = dict(totals)
        report["provider_responses"].append({"id": response.get("id"), "identity": identity,
                                              "usage": response.get("usage"), "status": response.get("status"),
                                              "incomplete_details": response.get("incomplete_details")})
        history.extend(response.get("output", []))
        calls = []
        for item in response.get("output", []):
            if item["type"] == "function_call":
                calls.append(item)
            elif item["type"] == "message":
                report["messages"].append(item)
        save(output, report)
        if identity["model"] != requested["model"] or identity["reasoning"] != requested["reasoning"]:
            raise ValueError("provider identity differs from explicit selection; no substitution")
        if response.get("status") != "completed":
            report["provider_outcome"] = {"status": response.get("status"),
                                          "details": response.get("incomplete_details")}
            save(output, report)
            raise ValueError("provider response did not complete")
        if not calls:
            report["tool_trace_complete"] = True
            save(output, report)
            return report
        for call in calls:
            event = {"name": call["name"], "call_id": call["call_id"], "arguments": call["arguments"]}
            report["tool_calls"].append(event)
            save(output, report)
            try:
                args = json.loads(call["arguments"])
                argv = args.get("argv", [])
                if call["name"] == "run_command" and len(argv) > 1 and argv[0] == "cargo" and argv[1] in {"test", "check", "clippy"}:
                    event.update(kind="check", check_key=json.dumps(argv), source_sha256=source_digest(root))
                value = execute_tool(call, root)
            except (OSError, ValueError) as error:
                value = json.dumps({"error": str(error)})
            except RuntimeError as error:
                report["client_outcome"] = {"status": "execution_failed", "reason": str(error)}
                save(output, report)
                raise
            event["result"] = value
            save(output, report)
            history.append({"type": "function_call_output", "call_id": call["call_id"], "output": value})
    raise ValueError("fixed limit of 30 provider responses reached")


def main():
    if len(sys.argv) != 2:
        raise ValueError("usage: openai_adapter.py REQUEST.json")
    api_key = os.environ.get("OPENAI_API_KEY")
    if not api_key:
        raise ValueError("existing OPENAI_API_KEY is required; no account setup or fallback")
    request = json.loads(Path(sys.argv[1]).read_text())
    evaluate(request, lambda payload: post(payload, api_key))


if __name__ == "__main__":
    def cancel(signum, frame):
        raise KeyboardInterrupt

    signal.signal(signal.SIGTERM, cancel)
    try:
        main()
    except KeyboardInterrupt:
        print("adapter interrupted", file=sys.stderr)
        sys.exit(130)
    except (OSError, ValueError, RuntimeError, subprocess.TimeoutExpired) as error:
        print("adapter error: " + ascii(str(error)), file=sys.stderr)
        sys.exit(1)
