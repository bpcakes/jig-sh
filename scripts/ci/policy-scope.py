#!/usr/bin/env python3
"""Skip Rust policy work only when every changed path is known metadata."""

import json
import os
from pathlib import PurePosixPath, Path
import subprocess


def needs_rust(event_name, event, repo="."):
    if event_name == "pull_request":
        base = event.get("pull_request", {}).get("base", {}).get("sha")
    elif event_name == "push":
        base = event.get("before")
    else:
        # Manual and merge-queue runs always exercise the complete policy suite.
        return True
    if not isinstance(base, str) or len(base) != 40 or any(c not in "0123456789abcdef" for c in base):
        return True
    try:
        changed = subprocess.check_output(
            ["git", "diff", "--name-only", "--no-renames", "-z", base, "HEAD", "--"],
            cwd=repo, stderr=subprocess.PIPE,
        ).decode().split("\0")[:-1]
    except (OSError, subprocess.CalledProcessError, UnicodeError):
        return True
    return not changed or any(
        not (path.startswith(".beads/") or path == "agent-map.md" or PurePosixPath(path).name == "AGENTS.md")
        for path in changed
    )


def main():
    try:
        event = json.loads(Path(os.environ["GITHUB_EVENT_PATH"]).read_text())
        required = needs_rust(os.environ.get("GITHUB_EVENT_NAME"), event)
    except (KeyError, OSError, ValueError, TypeError, AttributeError):
        required = True
    result = f"rust={str(required).lower()}"
    print(result)
    if output := os.environ.get("GITHUB_OUTPUT"):
        with open(output, "a") as stream:
            stream.write(result + "\n")


if __name__ == "__main__":
    main()
