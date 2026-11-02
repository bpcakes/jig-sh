#!/usr/bin/env python3
"""Skip a nightly run when its workflow already tested the same commit.

Nightly runs replace per-merge runs on the default branch. When nothing has
landed since the last completed nightly run of the same workflow, there is
nothing new to test. Other events, and any lookup failure, run the suite.
"""

import json
import os
from pathlib import Path
import subprocess
from urllib.parse import quote

# Cancelled runs did not finish testing their commit, so they never count.
TESTED_CONCLUSIONS = ("success", "failure")


def last_tested_sha(repository, workflow, branch):
    """The commit of the latest completed nightly run of WORKFLOW that tested one."""
    endpoint = (
        f"repos/{repository}/actions/workflows/{quote(workflow, safe='')}/runs"
        f"?event=schedule&branch={quote(branch, safe='')}&status=completed&per_page=20"
    )
    output = subprocess.check_output(["gh", "api", endpoint], stderr=subprocess.PIPE)
    for run in json.loads(output)["workflow_runs"]:
        if run.get("conclusion") in TESTED_CONCLUSIONS:
            return run.get("head_sha")
    return None


def needs_run(environ):
    if environ.get("GITHUB_EVENT_NAME") != "schedule":
        return True
    try:
        # GITHUB_WORKFLOW_REF is OWNER/REPO/.github/workflows/FILE@REF.
        workflow = environ["GITHUB_WORKFLOW_REF"].split("@", 1)[0].rsplit("/", 1)[-1]
        tested = last_tested_sha(environ["GITHUB_REPOSITORY"], workflow, environ["GITHUB_REF_NAME"])
        return tested != environ["GITHUB_SHA"]
    except (KeyError, OSError, ValueError, TypeError, AttributeError, subprocess.CalledProcessError):
        return True


def main():
    required = needs_run(os.environ)
    result = f"run={str(required).lower()}"
    print(result)
    if output := os.environ.get("GITHUB_OUTPUT"):
        with Path(output).open("a") as stream:
            stream.write(result + "\n")


if __name__ == "__main__":
    main()
