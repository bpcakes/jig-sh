import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import textwrap
import unittest


SCRIPT = Path(__file__).resolve().parents[1] / "ci" / "test-rust.sh"


class TestBuildReuseTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory(prefix="example-ci-tests-")
        self.addCleanup(temporary.cleanup)
        self.repo = Path(temporary.name)
        (self.repo / "scripts/ci").mkdir(parents=True)
        shutil.copy2(SCRIPT, self.repo / "scripts/ci/test-rust.sh")
        tools = self.repo / "tools"
        tools.mkdir()
        cargo = tools / "cargo"
        cargo.write_text("#!" + sys.executable + "\n" + textwrap.dedent('''\
            import json, os, pathlib, sys
            root = pathlib.Path(os.environ["EXAMPLE_ROOT"])
            args = sys.argv[1:]
            with (root / "calls.jsonl").open("a") as stream:
                stream.write(json.dumps(args) + "\\n")
            if args[0] == "metadata":
                print(json.dumps({"target_directory": str(root / "custom target")}))
            elif args[:2] == ["nextest", "list"]:
                if (root / "tests-started").exists():
                    sys.exit("build attempted after tests changed Git metadata")
                if os.environ.get("EXAMPLE_BUILD_FAIL"):
                    sys.exit(23)
                print('{"build": "original"}')
            elif args[:2] == ["nextest", "run"]:
                metadata = pathlib.Path(args[args.index("--binaries-metadata") + 1])
                assert json.loads(metadata.read_text())["build"] == "original"
                marker = root / "tests-started"
                first = not marker.exists()
                marker.touch()
                report = root / "custom target/nextest/ci/junit.xml"
                report.parent.mkdir(parents=True, exist_ok=True)
                report.write_text('<testsuites tests="1"/>')
                if first and os.environ.get("EXAMPLE_TEST_FAIL"):
                    sys.exit(42)
            else:
                sys.exit("unexpected Cargo command")
        '''))
        cargo.chmod(0o755)
        self.env = dict(os.environ, EXAMPLE_ROOT=str(self.repo), PATH=str(tools) + os.pathsep + os.environ["PATH"])
        self.env.pop("JIG_TEST_REPORT_DIR", None)

    def invoke(self):
        return subprocess.run(["/bin/bash", "scripts/ci/test-rust.sh", "minimal"],
                              cwd=self.repo, env=self.env, text=True, capture_output=True, timeout=10)

    def calls(self):
        return [json.loads(line) for line in (self.repo / "calls.jsonl").read_text().splitlines()]

    def test_all_phases_reuse_one_build_and_preserve_separate_reports(self):
        result = self.invoke()
        self.assertEqual(result.returncode, 0, result.stderr)
        calls = self.calls()
        self.assertEqual(sum(args[:2] == ["nextest", "list"] for args in calls), 1)
        runs = [args for args in calls if args[:2] == ["nextest", "run"]]
        self.assertEqual(len(runs), 3)
        self.assertEqual(runs[-1][-2:], ["-j", "1"])
        reports = self.repo / ".agent/.cache/test-reports/minimal"
        self.assertEqual({p.name for p in reports.iterdir()}, {"non-vault.xml", "vault.xml", "vault-pty.xml"})

    def test_failed_build_never_runs_stale_tests(self):
        self.env["EXAMPLE_BUILD_FAIL"] = "1"
        self.assertEqual(self.invoke().returncode, 23)
        self.assertFalse((self.repo / "tests-started").exists())

    def test_later_success_cannot_mask_failed_phase(self):
        self.env["EXAMPLE_TEST_FAIL"] = "1"
        self.assertEqual(self.invoke().returncode, 42)
        self.assertEqual(sum(args[:2] == ["nextest", "run"] for args in self.calls()), 3)


if __name__ == "__main__":
    unittest.main()
