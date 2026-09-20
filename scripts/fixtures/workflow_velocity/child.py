#!/usr/bin/env python3
"""Instrument only the fixed T-03 fixture children; not a production runner."""
import json
import os
from pathlib import Path
import subprocess
import sys
import time


def event(kind, **values):
    record = dict(kind=kind, label=os.environ.get("PROBE_LABEL"), at=time.monotonic(), **values)
    with open(os.environ["PROBE_EVENTS"], "a", encoding="utf-8") as output:
        output.write(json.dumps(record) + "\n")


def run(argv):
    event("child_start", argv=argv)
    child = subprocess.Popen(argv, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True)
    for line in child.stdout:
        if "Blocking waiting for file lock" in line:
            event("cargo_lock_wait", resource="build_directory" if "build directory" in line else "package_cache", message=line.strip())
        if "already used" in line or "EADDRINUSE" in line or "Address already in use" in line:
            event("server_conflict", message=line.strip())
        print(line, end="", flush=True)
    status = child.wait()
    event("child_end", status=status)
    return status


if __name__ == "__main__":
    mode = sys.argv[1]
    if mode == "cargo-proxy":
        event("cargo_invocation", argv=sys.argv[2:])
        os.execv(os.environ["PROBE_CARGO"], [os.environ["PROBE_CARGO"], *sys.argv[2:]])
    os.environ["PROBE_LABEL"] = sys.argv[2]
    if mode == "cargo":
        command = [os.environ["PROBE_CARGO"], "check", "--offline", "--locked"]
    elif mode == "database":
        command = [os.environ["PROBE_SQLX"], "prepare", "--check", "--workspace", "--", "--workspace", "--all-targets"]
    elif mode == "browser":
        if os.environ.get("PROBE_SEPARATE_BROWSER_PORTS") == "1" and sys.argv[2] == "right":
            os.environ["E2E_API_PORT"] = os.environ["PROBE_ALT_API_PORT"]
            os.environ["E2E_WEB_PORT"] = os.environ["PROBE_ALT_WEB_PORT"]
        command = [os.environ["PROBE_NODE"], os.environ["PROBE_PLAYWRIGHT"] + "/cli.js", "test", "--config", "playwright.config.cjs"]
    else:
        raise SystemExit("Unknown fixed probe")
    raise SystemExit(run(command))
