"""Offline behavioral coverage for the evaluation driver and adapter boundary."""

import importlib.util
import contextlib
import io
import fcntl
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

SCRIPTS = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(SCRIPTS))
sys.dont_write_bytecode = True

from harness_eval.experiment import (BASELINE, baseline_guidance, command, digest, dump,
                                     prepare, read_json, tools_snapshot, validate_config, verify_inputs)
from harness_eval.fixtures import TASKS, starting_files
from harness_eval.grade import grade, write_files
from harness_eval.openai_adapter import CLIENT, VERSION, evaluate, execute_tool
from harness_eval.runner import observations, run, run_trial, verify_trial

spec = importlib.util.spec_from_file_location("evaluate_harness", SCRIPTS / "evaluate-harness.py")
driver = importlib.util.module_from_spec(spec)
spec.loader.exec_module(driver)


def config(argv=None, timeout=5):
    return {"argv": argv or [sys.executable, "@openai-adapter"], "model": "example-model",
            "reasoning": "high", "client": CLIENT, "client_version": VERSION,
            "timeout_seconds": timeout}


class EvaluationTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.tmp = tempfile.TemporaryDirectory(prefix="example-evaluation-")
        cls.root = Path(cls.tmp.name)
        cls.experiment = cls.root / "experiment"
        cls.manifest = prepare(cls.experiment, "HEAD", config=config())

    @classmethod
    def tearDownClass(cls):
        cls.tmp.cleanup()

    def test_baseline_bytes_are_the_audited_git_objects(self):
        for name, content in baseline_guidance().items():
            actual = subprocess.check_output(["git", "show", f"{BASELINE}:{name}"], cwd=SCRIPTS.parent)
            self.assertEqual(content.encode(), actual)
        self.assertEqual(self.manifest["baseline_revision"], BASELINE)

    def test_all_five_graders_accept_solutions_and_reject_false_success(self):
        result = driver.smoke(self.root / "smoke")
        self.assertTrue(result["passed"], result)
        self.assertEqual(len(result["cases"]), 5)
        self.assertEqual(result["model_calls"], 0)

    def test_pairs_have_identical_source_prompt_and_independent_treatments(self):
        self.assertEqual(len(self.manifest["schedule"]), 90)
        pairs = {}
        for trial in self.manifest["schedule"]:
            directory = self.experiment / "trials" / f"{trial['order']:03d}"
            metadata = verify_trial(directory)
            pairs.setdefault(trial["pair"], []).append((trial, directory, metadata))
        self.assertEqual(len(pairs), 45)
        for arms in pairs.values():
            (first, first_dir, first_meta), (second, second_dir, second_meta) = arms
            self.assertNotEqual(first_dir, second_dir)
            self.assertEqual(first_meta["source_commit"], second_meta["source_commit"])
            self.assertEqual(first_meta["prompt_sha256"], second_meta["prompt_sha256"])
            comparison = next(arm["condition"] for arm in (first, second) if arm["condition"] != "baseline")
            self.assertEqual(first_meta["tools_sha256"] == second_meta["tools_sha256"], comparison == "guidance-only")
            guidance_equal = (first_dir / "checkout/AGENTS.md").read_bytes() == (second_dir / "checkout/AGENTS.md").read_bytes()
            self.assertEqual(guidance_equal, comparison == "tool-schema-only")
            for path in starting_files(TASKS[first["task"]]):
                self.assertEqual((first_dir / "checkout" / path).read_bytes(), (second_dir / "checkout" / path).read_bytes())

    def test_schedule_reconstructs_with_fixed_seed(self):
        # Avoid duplicating Git setup: compare the exact frozen trial metadata after
        # a second preparation. This also verifies separate output directories.
        second = self.root / "second"
        manifest = prepare(second, self.manifest["guidance_revision"], config=config())
        self.assertEqual(manifest, self.manifest)
        verify_inputs(second)
        with self.assertRaises(FileExistsError):
            prepare(second, "HEAD")
        trial = second / "trials/001"
        (trial / "checkout/unexpected.txt").write_text("contamination")
        with self.assertRaisesRegex(ValueError, "starting Git state changed|unexpected files"):
            verify_trial(trial)
        (second / "inputs/tasks.json").write_text("{}")
        with self.assertRaisesRegex(ValueError, "frozen input changed"):
            verify_inputs(second)

    def test_resume_rejects_replay_and_loss_of_uncommitted_work(self):
        task = TASKS["resume"]
        with tempfile.TemporaryDirectory(prefix="example-resume-") as tmp:
            root = Path(tmp)
            write_files(root, starting_files(task))
            write_files(root, task["solution"])
            self.assertTrue(grade("resume", root)["passed"])
            with (root / "state/exports.jsonl").open("a") as output:
                output.write(task["files"]["state/exports.jsonl"])
            replay = grade("resume", root)
            self.assertFalse(replay["passed"])
            self.assertIn("protected file changed: state/exports.jsonl", replay["invariant_violations"])
            write_files(root, task["files"])
            write_files(root, task["solution"])
            lost_edit = grade("resume", root)
            self.assertIn("protected file changed: parser.py", lost_edit["invariant_violations"])
        trial = next(t for t in self.manifest["schedule"] if t["task"] == "resume")
        metadata = read_json(self.experiment / "trials" / f"{trial['order']:03d}" / "trial.json")
        self.assertIn(" M parser.py", metadata["initial_git_status"])

    def test_missing_usage_is_not_zero_and_checks_need_source_identity(self):
        _, missing, metrics = observations({}, config(), "example-hash")
        self.assertIn("model", missing)
        self.assertIsNone(metrics["usage_tokens"]["value"])
        self.assertIsNone(metrics["unnecessary_questions"]["value"])
        identity = {**config(), "tools_sha256": "example-hash"}
        trace = [{"name": "run_command", "kind": "check", "check_key": "cargo test", "source_sha256": "same"}] * 2
        _, missing, metrics = observations({"identity": identity, "tool_calls": trace,
                                            "usage_tokens": {"input_tokens": 0, "output_tokens": None}}, config(), "example-hash")
        self.assertFalse(missing)
        self.assertEqual(metrics["tool_calls"]["value"], 2)
        self.assertEqual(metrics["repeated_checks"]["value"], 1)
        self.assertIsNone(metrics["usage_tokens"]["value"]["output_tokens"])
        with self.assertRaises(ValueError):
            observations({"usage_tokens": {"input_tokens": -1}}, config(), "example-hash")

    def trial_copy(self, name, task="resume"):
        trial = next(t for t in self.manifest["schedule"] if t["task"] == task)
        target = self.root / name
        shutil.copytree(self.experiment / "trials" / f"{trial['order']:03d}", target)
        return target

    def test_process_failure_timeout_missing_identity_and_exclusion_survive(self):
        failed = run_trial(self.trial_copy("failure"), config([sys.executable, "-c", "raise SystemExit(7)"]))
        self.assertEqual(failed["status"], "failed")
        self.assertEqual(failed["returncode"], 7)
        self.assertTrue(failed["excluded"])
        timed = run_trial(self.trial_copy("timeout"), config([sys.executable, "-c", "import time; time.sleep(30)"], timeout=0.1))
        self.assertEqual(timed["status"], "timeout")
        self.assertGreater(timed["elapsed_seconds"], 0)
        with self.assertRaises(ProcessLookupError):
            os.kill(timed["child_pid"], 0)
        missing = run_trial(self.trial_copy("missing"), config(["example-unavailable-adapter"]))
        self.assertEqual(missing["status"], "failed")
        self.assertIsNone(missing["metrics"]["usage_tokens"]["value"])

    def test_interruption_is_retained_and_completed_trials_are_not_replayed(self):
        trial_dir = self.trial_copy("interruption")
        real_popen = subprocess.Popen

        def interrupt_adapter(argv, **kwargs):
            if any(str(arg).endswith("openai_adapter.py") for arg in argv):
                raise KeyboardInterrupt
            return real_popen(argv, **kwargs)

        with patch("harness_eval.runner.subprocess.Popen", side_effect=interrupt_adapter):
            with self.assertRaises(KeyboardInterrupt):
                run_trial(trial_dir, config())
        self.assertEqual(read_json(trial_dir / "result.json")["status"], "interrupted")

        def finish(directory, requested, **kwargs):
            result = {"status": "completed", "excluded": False, "grade": {"passed": True}}
            dump(directory / "result.json", result)
            return result

        with patch("harness_eval.runner.run_trial", side_effect=finish) as execute:
            first = run(self.experiment, limit=1)
            second = run(self.experiment, limit=1)
            self.assertEqual(execute.call_count, 2)
            self.assertNotEqual(execute.call_args_list[0].args[0], execute.call_args_list[1].args[0])
            self.assertEqual(second["finished"], first["finished"] + 1)

    def test_concurrent_execution_is_rejected(self):
        with (self.experiment / ".run.lock").open("a") as lock:
            fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
            with self.assertRaisesRegex(ValueError, "another driver"):
                run(self.experiment, limit=1)

    def test_valid_helper_modules_are_graded_and_reported_success_is_ignored(self):
        with tempfile.TemporaryDirectory(prefix="example-helper-") as tmp:
            root = Path(tmp)
            write_files(root, starting_files(TASKS["resume"]))
            write_files(root, {"summary_helper.py": TASKS["resume"]["solution"]["export.py"],
                               "export.py": "from summary_helper import summary\n",
                               "test_success.py": "raise SystemExit(0)\n"})
            self.assertTrue(grade("resume", root)["passed"])
            (root / "summary_helper.py").write_text("def summary(): return {}\n")
            self.assertFalse(grade("resume", root)["passed"])

    def test_successful_adapter_is_graded_and_edits_observed(self):
        trial_dir = self.trial_copy("success")
        script = self.root / "example-adapter.py"
        script.write_text('''import json, pathlib, sys, time
assert all(not (parent / 'inputs/tasks.json').exists() and not (parent / '.jig.toml').exists() for parent in pathlib.Path.cwd().parents)
r = json.loads(pathlib.Path(sys.argv[1]).read_text())
pathlib.Path('export.py').write_text(%r)
time.sleep(0.1)
identity = {**r['requested'], 'tools_sha256': r['tools_sha256']}
pathlib.Path(r['response_path']).write_text(json.dumps({'identity': identity, 'tool_calls': []}))
''' % TASKS["resume"]["solution"]["export.py"])
        result = run_trial(trial_dir, config([sys.executable, str(script)]))
        self.assertEqual(result["status"], "completed")
        self.assertFalse(result["excluded"])
        self.assertTrue(result["grade"]["passed"])
        self.assertGreater(result["first_useful_edit_seconds"]["value"], 0)
        self.assertIsNone(result["metrics"]["usage_tokens"]["value"])

    def test_cli_requires_explicit_execution_and_preserves_exclusion(self):
        result = subprocess.run([sys.executable, str(SCRIPTS / "evaluate-harness.py"), "run", "--output", "example"], capture_output=True)
        self.assertEqual(result.returncode, 2)
        self.assertEqual(result.stdout, b"")
        result = subprocess.run([sys.executable, str(SCRIPTS / "evaluate-harness.py"), "--help"], capture_output=True, env={**os.environ, "OPENAI_API_KEY": ""})
        self.assertEqual(result.returncode, 0)
        self.assertNotIn(b"\x1b", result.stdout)
        annotation = driver.exclude(self.experiment, 90, "Example host unavailable before execution")
        self.assertEqual(annotation["original_result"]["status"], "not-run")
        self.assertEqual(read_json(self.experiment / "trials/090/result.json")["status"], "excluded")
        with self.assertRaises(ValueError):
            driver.exclude(self.experiment, 90, "second annotation")

    def test_small_fix_cargo_checks_inside_an_outer_workspace(self):
        outer = self.root / "outer-workspace"
        outer.mkdir()
        (outer / "Cargo.toml").write_text('[workspace]\nmembers = []\n')
        trial = self.trial_copy("cargo-check", task="small-fix")
        checkout = outer / "tmp/experiment/checkout"
        shutil.copytree(trial / "checkout", checkout)
        command(["cargo", "check", "--offline", "--target-dir", str(self.root / "cargo-target")], checkout)

    def test_migration_requires_declared_integer_type(self):
        root = self.root / "migration-types"
        root.mkdir()
        write_files(root, starting_files(TASKS["migration"]))
        for sql_type, expected in [("BLOB", False), ("TEXT", False), ("NUMERIC", False),
                                    ("INTEGER", True), ("integer", True), ("INT", True), ("BIGINT", True)]:
            with self.subTest(sql_type=sql_type):
                write_files(root, {"migrations/002.sql":
                    f"ALTER TABLE entries ADD COLUMN enabled {sql_type} NOT NULL DEFAULT 1;\n"})
                self.assertEqual(grade("migration", root)["passed"], expected)

    def test_grader_and_tool_commands_do_not_inherit_credentials(self):
        root = self.root / "credential-check"
        root.mkdir()
        write_files(root, starting_files(TASKS["resume"]))
        solution = "import os\nassert 'OPENAI_API_KEY' not in os.environ\n" + TASKS["resume"]["solution"]["export.py"]
        write_files(root, {"export.py": solution})
        with patch.dict(os.environ, {"OPENAI_API_KEY": "example-sentinel"}):
            self.assertTrue(grade("resume", root)["passed"])
            result = execute_tool({"name": "run_command", "arguments": json.dumps({"argv": [
                sys.executable, "-c", "import os; assert 'OPENAI_API_KEY' not in os.environ"]})}, root)
        self.assertEqual(json.loads(result)["returncode"], 0)

    def test_finalization_resumes_without_replaying_client(self):
        for boundary in ("retain_workspace", "grade", "observations"):
            with self.subTest(boundary=boundary):
                script = self.root / f"finalize-{boundary}.py"
                script.write_text('import json, pathlib, sys\n'
                    'r = json.loads(pathlib.Path(sys.argv[1]).read_text())\n'
                    f'pathlib.Path("export.py").write_text({TASKS["resume"]["solution"]["export.py"]!r})\n'
                    'pathlib.Path(r["response_path"]).write_text(json.dumps({"identity": {**r["requested"], "tools_sha256": r["tools_sha256"]}}))\n')
                requested = config([sys.executable, str(script)])
                output = self.root / f"finalize-{boundary}"
                manifest = prepare(output, "HEAD", config=requested)
                item = next(t for t in manifest["schedule"] if t["task"] == "resume")
                trial = output / "trials" / f"{item['order']:03d}"
                def interrupt(*args):
                    if boundary == "retain_workspace":
                        shutil.rmtree(trial / "checkout")
                        (trial / "checkout").mkdir()
                        (trial / "checkout/partial").write_text("interrupted copy")
                    raise KeyboardInterrupt
                with patch(f"harness_eval.runner.{boundary}", side_effect=interrupt):
                    with self.assertRaises(KeyboardInterrupt):
                        run_trial(trial, requested)
                pending = read_json(trial / "result.json")
                self.assertEqual(pending["status"], "finalizing")
                self.assertTrue(Path(pending["execution_workspace"]["path"]).is_dir())
                with patch("harness_eval.runner.run_trial", side_effect=AssertionError("client replay")):
                    resumed = run(output, limit=0)["results"][0]
                self.assertEqual(resumed["status"], "completed")
                self.assertTrue(resumed["grade"]["passed"])
                self.assertFalse(resumed["excluded"])
                self.assertEqual((trial / "checkout/export.py").read_text(), TASKS["resume"]["solution"]["export.py"])
                self.assertFalse(Path(pending["execution_workspace"]["path"]).exists())
                with patch("harness_eval.runner.finalize_trial", side_effect=AssertionError("already finalized")):
                    self.assertEqual(run(output, limit=0)["results"][0], resumed)

    def test_cli_handles_retained_null_grade_without_traceback(self):
        with patch.object(driver, "run", return_value={"results": [{"status": "completed", "grade": None}]}):
            with contextlib.redirect_stdout(io.StringIO()):
                self.assertEqual(driver.main(["run", "--output", "unused", "--execute"]), 1)

    def test_provider_incomplete_is_distinct_from_incorrect_code(self):
        trial = self.trial_copy("provider-incomplete")
        script = self.root / "incomplete-adapter.py"
        script.write_text('import json, pathlib, sys\n'
            'r = json.loads(pathlib.Path(sys.argv[1]).read_text())\n'
            'pathlib.Path(r["response_path"]).write_text(json.dumps({"identity": {**r["requested"], "tools_sha256": r["tools_sha256"]}, "provider_outcome": {"status": "incomplete", "details": {"reason": "max_output_tokens"}}}))\n'
            'raise SystemExit(1)\n')
        result = run_trial(trial, config([sys.executable, str(script)]))
        self.assertEqual(result["status"], "incomplete")
        self.assertEqual(result["execution_status"], "failed")
        self.assertTrue(result["excluded"])
        self.assertIn("max_output_tokens", result["exclusion_reason"])


class ResponsesAdapterTests(unittest.TestCase):
    def test_output_budget_configuration_rejects_invalid_values(self):
        validate_config({**config(), "max_output_tokens": 25000})
        for value in (0, -1, True, "4096"):
            with self.subTest(value=value), self.assertRaisesRegex(ValueError, "positive integer"):
                validate_config({**config(), "max_output_tokens": value})

    def test_real_adapter_loop_uses_selected_tools_and_provider_identity_offline(self):
        with tempfile.TemporaryDirectory(prefix="example-responses-") as tmp:
            root = Path(tmp)
            (root / "AGENTS.md").write_text("Example guidance")
            request = {"workspace": str(root), "response_path": str(root / "response.json"),
                       "prompt": "Example task", "requested": config(), "tools": tools_snapshot(),
                       "tools_sha256": "example-hash"}
            payloads = []

            def send(payload):
                payloads.append(json.loads(json.dumps(payload)))
                output = [{"type": "function_call", "name": "write_file", "call_id": "example-call",
                           "arguments": json.dumps({"path": "example.txt", "content": "done"})}] if len(payloads) == 1 else []
                return {"model": "example-model", "reasoning": {"effort": "high"}, "status": "completed",
                        "output": output, "usage": {"input_tokens": 20, "output_tokens": 4}}

            result = evaluate(request, send)
            self.assertEqual((root / "example.txt").read_text(), "done")
            self.assertEqual(len(result["tool_calls"]), 1)
            self.assertEqual(result["usage_tokens"]["input_tokens"], 40)
            self.assertIsNone(result["usage_tokens"]["cached_input_tokens"])
            self.assertFalse(payloads[0]["store"])
            self.assertEqual(payloads[0]["tools"][0]["description"], tools_snapshot()[0]["description"])
            self.assertEqual(payloads[1]["input"][-1]["type"], "function_call_output")
            self.assertEqual(payloads[0]["instructions"], "Example guidance")
            with self.assertRaisesRegex(ValueError, "differs"):
                evaluate(request, lambda payload: {"model": "substituted-model", "reasoning": {"effort": "high"}, "output": []})

    def test_output_budget_is_forwarded_and_incomplete_details_are_retained(self):
        with tempfile.TemporaryDirectory(prefix="example-output-budget-") as tmp:
            root = Path(tmp)
            (root / "AGENTS.md").write_text("Example guidance")
            request = {"workspace": str(root), "response_path": str(root / "response.json"),
                       "prompt": "Example task", "requested": {**config(), "max_output_tokens": 25000},
                       "tools": tools_snapshot(), "tools_sha256": "example-hash"}
            def send(payload):
                self.assertEqual(payload["max_output_tokens"], 25000)
                return {"model": "example-model", "reasoning": {"effort": "high"},
                        "status": "incomplete", "incomplete_details": {"reason": "max_output_tokens"}, "output": []}
            with self.assertRaisesRegex(ValueError, "did not complete"):
                evaluate(request, send)
            report = read_json(root / "response.json")
            self.assertEqual(report["provider_outcome"]["details"]["reason"], "max_output_tokens")
            with self.assertRaisesRegex(ValueError, "differs"):
                evaluate(request, lambda payload: {"model": "example-model", "reasoning": None, "output": []})

    def test_file_tools_cannot_escape_the_fixture(self):
        with tempfile.TemporaryDirectory(prefix="example-tool-") as tmp:
            with self.assertRaisesRegex(ValueError, "inside the fixture"):
                execute_tool({"name": "write_file", "arguments": '{"path":"../outside.txt","content":"no"}'}, Path(tmp).resolve())


if __name__ == "__main__":
    unittest.main()
