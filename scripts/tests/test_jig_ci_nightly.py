import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import textwrap
import unittest


SCRIPT = Path(__file__).resolve().parents[1] / "ci" / "nightly-scope.py"
HEAD = "a" * 40
OLDER = "b" * 40


class NightlyScopeTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory(prefix="example-ci-nightly-")
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        tools = self.root / "tools"
        tools.mkdir()
        gh = tools / "gh"
        gh.write_text("#!" + sys.executable + "\n" + textwrap.dedent('''\
            import os, pathlib, sys
            root = pathlib.Path(os.environ["EXAMPLE_ROOT"])
            (root / "gh-args.txt").write_text("\\n".join(sys.argv[1:]))
            if os.environ.get("EXAMPLE_GH_FAIL"):
                sys.exit("HTTP 502")
            sys.stdout.write((root / "runs.json").read_text())
        '''))
        gh.chmod(0o755)
        self.output = self.root / "github-output"
        self.env = {
            "PATH": str(tools) + os.pathsep + os.environ["PATH"],
            "EXAMPLE_ROOT": str(self.root),
            "GITHUB_EVENT_NAME": "schedule",
            "GITHUB_WORKFLOW_REF": "example-owner/example-repo/.github/workflows/example-tests.yml@refs/heads/master",
            "GITHUB_REPOSITORY": "example-owner/example-repo",
            "GITHUB_REF_NAME": "master",
            "GITHUB_SHA": HEAD,
            "GITHUB_OUTPUT": str(self.output),
        }

    def runs(self, *runs):
        payload = {"workflow_runs": [{"head_sha": sha, "conclusion": conclusion} for sha, conclusion in runs]}
        (self.root / "runs.json").write_text(json.dumps(payload))

    def scope(self):
        result = subprocess.run([sys.executable, str(SCRIPT)], env=self.env,
                                text=True, capture_output=True, timeout=10)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(self.output.read_text(), result.stdout)
        return result.stdout.strip()

    def test_commit_already_tested_by_a_nightly_run_is_skipped(self):
        for conclusion in ("success", "failure"):
            with self.subTest(conclusion=conclusion):
                self.output.unlink(missing_ok=True)
                self.runs((HEAD, conclusion), (OLDER, "success"))
                self.assertEqual(self.scope(), "run=false")

    def test_new_commit_runs(self):
        self.runs((OLDER, "success"))
        self.assertEqual(self.scope(), "run=true")

    def test_cancelled_runs_do_not_count_as_tested(self):
        self.runs((HEAD, "cancelled"), (OLDER, "success"))
        self.assertEqual(self.scope(), "run=true")

    def test_first_nightly_run_runs(self):
        self.runs()
        self.assertEqual(self.scope(), "run=true")

    def test_queries_this_workflow_and_branch(self):
        self.runs((HEAD, "success"))
        self.scope()
        args = (self.root / "gh-args.txt").read_text().splitlines()
        self.assertEqual(args[0], "api")
        self.assertEqual(
            args[1],
            "repos/example-owner/example-repo/actions/workflows/example-tests.yml/runs"
            "?event=schedule&branch=master&status=completed&per_page=20",
        )

    def test_lookup_failures_run(self):
        self.env["EXAMPLE_GH_FAIL"] = "1"
        self.runs((HEAD, "success"))
        self.assertEqual(self.scope(), "run=true")
        del self.env["EXAMPLE_GH_FAIL"]
        (self.root / "runs.json").write_text("not json")
        self.output.unlink()
        self.assertEqual(self.scope(), "run=true")
        del self.env["GITHUB_WORKFLOW_REF"]
        self.output.unlink()
        self.assertEqual(self.scope(), "run=true")

    def test_other_events_run_without_a_lookup(self):
        for event in ("pull_request", "workflow_dispatch", "merge_group", "push"):
            with self.subTest(event=event):
                self.env["GITHUB_EVENT_NAME"] = event
                self.output.unlink(missing_ok=True)
                self.assertEqual(self.scope(), "run=true")
        self.assertFalse((self.root / "gh-args.txt").exists())


if __name__ == "__main__":
    unittest.main()
