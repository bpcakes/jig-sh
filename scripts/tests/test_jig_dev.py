import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import textwrap
import unittest


SCRIPT = Path(__file__).resolve().parents[1] / "jig-dev"


class JigDevelopmentEntrypointTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory(prefix="example-jig-dev-")
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name).resolve()
        self.repo = self.root / "ExampleProject"
        self.scripts = self.repo / "scripts"
        self.scripts.mkdir(parents=True)
        shutil.copy2(SCRIPT, self.scripts / "jig-dev")
        self.caller = self.root / "caller directory"
        self.caller.mkdir()
        self.tools = self.root / "tools"
        self.tools.mkdir()
        self.messages = self.root / "cargo-messages.json"
        self.cargo_call = self.root / "cargo-call.json"
        self.launcher_call = self.root / "launcher-called"
        self.artifact = self.root / "custom target" / "debug" / "jig"
        self.write_executable(
            self.artifact,
            """
            import json, os, sys
            print(json.dumps({
                "cwd": os.getcwd(),
                "args": sys.argv[1:],
                "binary": os.environ["JIG_DEV_BIN"],
            }))
            sys.exit(int(os.environ.get("EXAMPLE_RUNTIME_EXIT", "0")))
            """,
        )
        self.stale = self.repo / "target" / "debug" / "jig"
        self.write_executable(self.stale, 'raise SystemExit("stale binary ran")')
        self.write_executable(
            self.tools / "cargo",
            """
            import json, os, pathlib, sys
            pathlib.Path(os.environ["EXAMPLE_CARGO_CALL"]).write_text(json.dumps({
                "cwd": os.getcwd(),
                "args": sys.argv[1:],
                "target_dir": os.environ.get("CARGO_TARGET_DIR"),
            }))
            print("Example Cargo diagnostic", file=sys.stderr)
            for message in json.loads(pathlib.Path(os.environ["EXAMPLE_MESSAGES"]).read_text()):
                print(json.dumps(message))
            sys.exit(int(os.environ.get("EXAMPLE_CARGO_EXIT", "0")))
            """,
        )
        self.write_executable(
            self.scripts / "jig",
            """
            import os, pathlib, sys
            pathlib.Path(os.environ["EXAMPLE_LAUNCHER_CALL"]).touch()
            binary = os.environ["JIG_DEV_BIN"]
            os.execv(binary, [binary, *sys.argv[1:]])
            """,
        )
        self.env = dict(
            os.environ,
            PATH=str(self.tools) + os.pathsep + os.environ.get("PATH", os.defpath),
            JIG_DEV_BIN=str(self.stale),
            CARGO_TARGET_DIR=str(self.artifact.parent.parent),
            EXAMPLE_MESSAGES=str(self.messages),
            EXAMPLE_CARGO_CALL=str(self.cargo_call),
            EXAMPLE_LAUNCHER_CALL=str(self.launcher_call),
        )
        self.set_messages([self.artifact_message()])

    def write_executable(self, path, body):
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text("#!" + sys.executable + "\n" + textwrap.dedent(body).lstrip())
        path.chmod(0o755)

    def artifact_message(self):
        return {
            "reason": "compiler-artifact",
            "target": {"name": "jig", "kind": ["bin"]},
            "executable": str(self.artifact),
        }

    def set_messages(self, messages):
        self.messages.write_text(json.dumps(messages))

    def invoke(self, *args):
        return subprocess.run(
            [str(self.scripts / "jig-dev"), *args],
            cwd=self.caller,
            env=self.env,
            capture_output=True,
            text=True,
            timeout=10,
        )

    def assert_not_launched(self, result):
        self.assertNotEqual(result.returncode, 0)
        self.assertFalse(self.launcher_call.exists())
        self.assertEqual(result.stdout, "")

    def test_reported_artifact_replaces_override_and_preserves_caller_context(self):
        arguments = ["init", "relative destination", "--json"]
        result = self.invoke(*arguments)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertTrue(self.launcher_call.is_file())
        self.assertEqual(json.loads(result.stdout), {
            "cwd": str(self.caller),
            "args": arguments,
            "binary": str(self.artifact),
        })
        self.assertIn("Example Cargo diagnostic", result.stderr)
        build = json.loads(self.cargo_call.read_text())
        self.assertEqual(build["cwd"], str(self.repo))
        self.assertEqual(build["target_dir"], str(self.artifact.parent.parent))
        self.assertEqual(build["args"][0], "build")
        self.assertIn("--locked", build["args"])
        self.assertEqual(build["args"][build["args"].index("-p") + 1], "jig-sh")
        self.assertEqual(build["args"][build["args"].index("--bin") + 1], "jig")
        self.assertNotIn("--release", build["args"])
        self.assertNotIn("--no-default-features", build["args"])

    def test_failed_build_does_not_run_existing_reported_or_stale_binary(self):
        self.env["EXAMPLE_CARGO_EXIT"] = "23"
        result = self.invoke("--version")
        self.assertEqual(result.returncode, 23, result.stderr)
        self.assert_not_launched(result)

    def test_missing_executable_does_not_fall_back_to_stale_binary(self):
        self.artifact.unlink()
        result = self.invoke("--version")
        self.assert_not_launched(result)
        self.assertIn("did not produce an executable", result.stderr)

    def test_nonexecutable_artifact_does_not_launch(self):
        self.artifact.chmod(0o644)
        result = self.invoke("--version")
        self.assert_not_launched(result)

    def test_unrelated_cargo_artifacts_do_not_select_a_runtime(self):
        messages = [
            {**self.artifact_message(), "target": {"name": "helper", "kind": ["bin"]}},
            {**self.artifact_message(), "target": {"name": "jig", "kind": ["lib"]}},
            {"reason": "build-finished", "success": True},
        ]
        self.set_messages(messages)
        result = self.invoke("--version")
        self.assert_not_launched(result)

    def test_runtime_exit_status_is_preserved(self):
        self.env["EXAMPLE_RUNTIME_EXIT"] = "17"
        result = self.invoke("check", "contract")
        self.assertEqual(result.returncode, 17, result.stderr)
        self.assertTrue(self.launcher_call.is_file())
        self.assertEqual(json.loads(result.stdout)["args"], ["check", "contract"])


if __name__ == "__main__":
    unittest.main()
