#!/usr/bin/env python3
"""Sync beadroll and run bv triage against a disposable, privacy-checked export."""

import json
from pathlib import Path
import subprocess
import sys
import tempfile


ROOT = Path(__file__).resolve().parent.parent


def run(command, root):
    return subprocess.run(
        command, cwd=root, check=True, capture_output=True, text=True,
        encoding="utf-8",
    ).stdout


def check_export(contents):
    for number, line in enumerate(contents.split("\n"), 1):
        if not line.strip():
            continue
        record = json.loads(line)
        if not isinstance(record, dict):
            raise ValueError(f"Export record {number} must be an object")
        if record.get("source_repo_path") not in (None, ""):
            raise ValueError(f"Export record {number} contains machine-local source metadata")


def without_commands(value):
    # bv still builds br/bd commands. Keep its rankings, never its write advice.
    if isinstance(value, dict):
        return {
            key: without_commands(item) for key, item in value.items()
            if key != "commands" and not key.endswith("_command")
        }
    if isinstance(value, list):
        return [without_commands(item) for item in value]
    return value


def triage(root):
    synced = json.loads(run(["bead", "sync", "--json"], root))
    if synced.get("unpublished"):
        raise ValueError("Tracker still has unpublished events; resolve bead sync before triage")
    if synced.get("integrity") or synced.get("waiting_events"):
        raise ValueError("Tracker has incomplete or invalid events; inspect bead info before triage")
    contents = run(["bead", "export"], root)
    check_export(contents)
    with tempfile.TemporaryDirectory(prefix="bead-triage-") as temporary:
        snapshot = Path(temporary) / "issues.jsonl"
        snapshot.write_text(contents, encoding="utf-8")
        result = json.loads(run([
            "bv", "--db", str(snapshot), "--robot-triage", "--format", "json", "--no-cache",
        ], temporary))
    result = without_commands(result)
    result["usage_hints"] = [
        "Use the graph rankings to select candidates, then verify with bead ready --json.",
        "Read the issue with bead show ID --json; claim with bead claim ID --json.",
    ]
    return result


def main():
    try:
        result = triage(ROOT)
    except subprocess.CalledProcessError as error:
        print(f"Triage failed: {error.cmd[0]} exited {error.returncode}; "
              "run that tool directly to diagnose it.", file=sys.stderr)
        return 1
    except (OSError, ValueError) as error:
        print(f"Triage failed: {error}", file=sys.stderr)
        return 1
    print(json.dumps(result, ensure_ascii=False, indent=2))
    return 0


if __name__ == "__main__":
    sys.exit(main())
