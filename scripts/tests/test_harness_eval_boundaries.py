"""Recovery, annotation ownership, and bounded grading through real entrypoints."""

import contextlib
import io
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

import test_harness_eval as helpers
from harness_eval.child_environment import command_environment
from harness_eval.experiment import environment_versions, experiment_lock
from harness_eval.runner import finalize_trial, run, run_trial
from harness_eval.process import stop_group
from harness_eval.workspace import release_workspace


class BoundaryTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory(prefix="example-boundaries-")
        self.root = Path(self.tmp.name)

    def tearDown(self):
        self.tmp.cleanup()

    def prepared_trial(self, first=False):
        script = self.root / "client.py"
        script.write_text('import json, pathlib, sys\n'
            'r = json.loads(pathlib.Path(sys.argv[1]).read_text())\n'
            f'pathlib.Path("export.py").write_text({helpers.TASKS["resume"]["solution"]["export.py"]!r})\n'
            'pathlib.Path(r["response_path"]).write_text(json.dumps({"identity": {**r["requested"], "tools_sha256": r["tools_sha256"]}}))\n')
        config = helpers.config([sys.executable, str(script)])
        output = self.root / "experiment"
        manifest = helpers.prepare(output, "HEAD", config=config)
        item = manifest["schedule"][0] if first else next(t for t in manifest["schedule"] if t["task"] == "resume")
        return output, output / "trials" / f"{item['order']:03d}", config, manifest

    def interrupt(self, trial, config, boundary):
        with patch(f"harness_eval.runner.{boundary}", side_effect=KeyboardInterrupt):
            with self.assertRaises(KeyboardInterrupt):
                run_trial(trial, config)
        return helpers.read_json(trial / "result.json")

    def test_retained_checkout_survives_temporary_workspace_loss(self):
        output, trial, config, _ = self.prepared_trial()
        pending = self.interrupt(trial, config, "grade")
        self.assertTrue(pending["checkout_retained"])
        release_workspace(pending["execution_workspace"])
        with patch("harness_eval.runner.run_trial", side_effect=AssertionError("client replay")):
            result = run(output, limit=0)["results"][0]
        self.assertEqual(result["status"], "completed")
        self.assertTrue(result["grade"]["passed"])

    def test_cli_cleanup_failure_is_durable_excluded_and_never_finalized(self):
        output, trial, _, _ = self.prepared_trial(first=True)
        children = []

        def fail_cleanup(child, **kwargs):
            children.append(child)
            raise RuntimeError("example cleanup inspection failure")

        stdout, stderr = io.StringIO(), io.StringIO()
        try:
            with patch("harness_eval.runner.stop_group", side_effect=fail_cleanup), \
                 patch("harness_eval.runner.grade", side_effect=AssertionError("unsafe grading")), \
                 patch("harness_eval.runner.retain_workspace", side_effect=AssertionError("unsafe retention")), \
                 contextlib.redirect_stdout(stdout), contextlib.redirect_stderr(stderr):
                code = helpers.driver.main(["run", "--output", str(output), "--execute", "--limit", "1"])
            self.assertEqual(code, 2)
            self.assertEqual(stdout.getvalue(), "")
            self.assertIn("evaluation error:", stderr.getvalue())
            self.assertIn("process-group cleanup unresolved", stderr.getvalue())
            self.assertNotIn("Traceback", stderr.getvalue())
            result = helpers.read_json(trial / "result.json")
            self.assertEqual(result["status"], "cleanup_failed")
            self.assertEqual(result["execution_status"], "completed")
            self.assertEqual(result["cleanup_error"], "example cleanup inspection failure")
            self.assertTrue(result["excluded"])
            self.assertIsNone(result["grade"])
            self.assertGreater(result["elapsed_seconds"], 0)
            self.assertTrue(Path(result["execution_workspace"]["path"]).is_dir())
            self.assertTrue((trial / "edit-observations.json").exists())
            with patch("harness_eval.runner.finalize_trial", side_effect=AssertionError("unsafe finalization")), \
                 patch("harness_eval.runner.run_trial", side_effect=AssertionError("client replay")):
                resumed = run(output, limit=0)
            self.assertEqual(resumed["results"][0], result)
            with self.assertRaisesRegex(ValueError, "unresolved"):
                finalize_trial(trial, {}, result)
        finally:
            for child in children:
                stop_group(child)
            result_path = trial / "result.json"
            if result_path.exists():
                release_workspace(helpers.read_json(result_path)["execution_workspace"])

    def test_lost_unretained_workspace_can_be_excluded_without_blocking_later_trials(self):
        output, trial, config, _ = self.prepared_trial(first=True)
        pending = self.interrupt(trial, config, "retain_workspace")
        self.assertFalse(pending.get("checkout_retained"))
        release_workspace(pending["execution_workspace"])
        with self.assertRaisesRegex(ValueError, "unavailable or changed"):
            run(output, limit=0)
        helpers.driver.exclude(output, int(trial.name), "Example temporary workspace was removed")
        self.assertEqual(helpers.read_json(trial / "result.json")["status"], "excluded")
        with patch("harness_eval.runner.run_trial", return_value={"status": "completed", "grade": {"passed": True}}) as client:
            result = run(output, limit=1)
        self.assertEqual(client.call_count, 1)
        self.assertGreater(int(client.call_args.args[0].name), int(trial.name))
        self.assertTrue(result["results"][0]["excluded"])

    def test_exclusion_is_serialized_and_authoritative_during_finalization(self):
        output, trial, config, _ = self.prepared_trial()
        pending = self.interrupt(trial, config, "grade")
        with experiment_lock(output):
            with self.assertRaisesRegex(ValueError, "another driver"):
                helpers.driver.exclude(output, int(trial.name), "Example exclusion")
            with contextlib.redirect_stderr(io.StringIO()):
                code = helpers.driver.main(["annotate", "--output", str(output), "--order", trial.name,
                    "--unnecessary-questions", "0", "--reviewer", "example-reviewer", "--reason", "Example reason"])
            self.assertEqual(code, 2)
        self.assertFalse((trial / "exclusion.json").exists())
        self.assertFalse((trial / "annotations.json").exists())
        helpers.driver.exclude(output, int(trial.name), "Example exclusion")
        # Simulate a stale pre-exclusion finalizer, including recovery of old artifacts.
        completed = finalize_trial(trial, config, pending)
        self.assertTrue(completed["excluded"])
        self.assertEqual(completed["exclusion_reason"], "Example exclusion")
        helpers.dump(trial / "result.json", {"status": "completed", "grade": None, "excluded": False})
        self.assertTrue(run(output, limit=0)["results"][0]["excluded"])
        # A crash after the annotation rename but before the result rename is safe too.
        (trial / "result.json").unlink()
        with patch("harness_eval.runner.run_trial", side_effect=AssertionError("excluded trial replay")):
            self.assertEqual(run(output, limit=0)["results"][0]["status"], "excluded")

    def test_recursive_sql_is_bounded_and_operator_cancellation_is_preserved(self):
        checkout = self.root / "checkout"
        checkout.mkdir()
        helpers.write_files(checkout, helpers.starting_files(helpers.TASKS["migration"]))
        helpers.write_files(checkout, {"migrations/002.sql":
            "WITH RECURSIVE forever(n) AS (VALUES(1) UNION ALL SELECT n+1 FROM forever) SELECT sum(n) FROM forever;\n"})
        for mode in ("deadline", "cancel"):
            with self.subTest(mode=mode):
                code = f'''import importlib, json, sys
from pathlib import Path
from unittest.mock import patch
sys.path.insert(0, {str(helpers.SCRIPTS)!r})
g = importlib.import_module('harness_eval.grade')
if {mode!r} == 'deadline':
    g.SQL_TIMEOUT_SECONDS = 0.02
    print(json.dumps(g.grade('migration', Path({str(checkout)!r}))))
else:
    with patch.object(g.time, 'monotonic', side_effect=[0, KeyboardInterrupt]):
        try:
            g.grade('migration', Path({str(checkout)!r}))
        except KeyboardInterrupt:
            print('cancelled')
'''
                result = subprocess.run([sys.executable, "-B", "-c", code],
                    capture_output=True, text=True, timeout=5, env=command_environment())
                self.assertEqual(result.returncode, 0, result.stderr)
                if mode == "deadline":
                    graded = json.loads(result.stdout)
                    self.assertFalse(graded["passed"])
                    self.assertIn("deadline", graded["failure"])
                else:
                    self.assertEqual(result.stdout.strip(), "cancelled")

    def test_resume_rejects_import_time_mutation_of_protected_inputs(self):
        checkout = self.root / "import-mutation"
        checkout.mkdir()
        task = helpers.TASKS["resume"]
        for name in ("state/operations.jsonl", "state/exports.jsonl", "migrations/002.sql", "parser.py"):
            with self.subTest(path=name):
                helpers.write_files(checkout, helpers.starting_files(task))
                prefix = f"from pathlib import Path\np = Path({name!r})\np.write_text(p.read_text() + '\\n')\n"
                helpers.write_files(checkout, {"export.py": prefix + task["solution"]["export.py"]})
                result = helpers.grade("resume", checkout)
                self.assertFalse(result["passed"])
                self.assertIn("protected input changed", result["failure"])

    def test_resume_protection_applies_to_alternate_input_calls(self):
        checkout = self.root / "late-mutation"
        checkout.mkdir()
        task = helpers.TASKS["resume"]
        helpers.write_files(checkout, helpers.starting_files(task))
        implementation = task["solution"]["export.py"].replace("def summary():", "def original_summary():")
        implementation += """
def summary():
    result = original_summary()
    if result['count'] == 2:
        path = Path('state/operations.jsonl')
        path.write_text(path.read_text() + '{}\\n')
    return result
"""
        helpers.write_files(checkout, {"export.py": implementation})
        result = helpers.grade("resume", checkout)
        self.assertFalse(result["passed"])
        self.assertIn("protected input changed", result["failure"])

    def compiler_stubs(self):
        bindir = self.root / "bin"
        bindir.mkdir()
        for name in ("rustc", "cargo"):
            path = bindir / name
            path.write_text(f'#!{sys.executable}\nfrom pathlib import Path\n'
                f'print("repository-toolchain" if Path.cwd() == Path({str(helpers.SCRIPTS.parent)!r}) else "detached-toolchain")\n')
            path.chmod(0o755)
        return bindir

    def test_environment_uses_detached_cwd_and_detects_changes_between_trials(self):
        bindir = self.compiler_stubs()
        with patch.dict(os.environ, {"PATH": str(bindir) + os.pathsep + os.environ["PATH"]}):
            self.assertEqual(helpers.command(["rustc", "--version"], helpers.SCRIPTS.parent).strip(), "repository-toolchain")
            observed = environment_versions()
            self.assertEqual(observed["rustc"], "detached-toolchain")
            self.assertEqual(observed["cargo"], "detached-toolchain")
            output, _, _, _ = self.prepared_trial()
            def client(*args, **kwargs):
                (bindir / "rustc").write_text(f'#!{sys.executable}\nprint("changed-toolchain")\n')
                return {"status": "completed", "grade": {"passed": True}}
            with patch("harness_eval.runner.run_trial", side_effect=client) as execute:
                with self.assertRaisesRegex(ValueError, "environment changed"):
                    run(output, limit=2)
            self.assertEqual(execute.call_count, 1)

    def test_environment_change_during_trial_excludes_the_result(self):
        _, trial, config, manifest = self.prepared_trial()
        changed = {**manifest["environment"], "rustc": "changed-toolchain"}
        with patch("harness_eval.runner.environment_versions", return_value=changed):
            result = run_trial(trial, config, environment=manifest["environment"])
        self.assertTrue(result["grade"]["passed"])
        self.assertTrue(result["excluded"])
        self.assertEqual(result["environment_after"], changed)
        self.assertIn("environment changed", result["exclusion_reason"])


if __name__ == "__main__":
    unittest.main()
