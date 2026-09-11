"""Real process regressions for tool timeout recovery and outer cancellation."""

import importlib.util
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import time
import unittest
from unittest.mock import patch

import test_harness_eval as helpers
from harness_eval import openai_adapter as adapter
from harness_eval.child_environment import command_environment
from harness_eval.process import capture_command
from harness_eval.runner import run_trial


class ProcessTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory(prefix="example-command-")
        self.root = Path(self.tmp.name)

    def tearDown(self):
        self.tmp.cleanup()

    def assert_stopped(self, pid):
        result = subprocess.run(["ps", "-p", str(pid), "-o", "stat="],
                                capture_output=True, text=True, timeout=2)
        state = result.stdout.strip()
        self.assertTrue(not state or state.startswith("Z"), state)

    def test_normal_command_captures_output_and_retires_background_descendants(self):
        pidfile = self.root / "background.pid"
        code = ('import pathlib, subprocess, sys\n'
                f'p = subprocess.Popen([sys.executable, "-c", "import time; time.sleep(30)"])\n'
                f'pathlib.Path({str(pidfile)!r}).write_text(str(p.pid))\n'
                'print("ready")\n')
        started = time.monotonic()
        result = capture_command([sys.executable, "-c", code], self.root, command_environment(), 2)
        self.assertEqual(result, {"returncode": 0, "output": "ready\n"})
        self.assertLess(time.monotonic() - started, 5)
        self.assert_stopped(int(pidfile.read_text()))

    def test_tool_timeout_is_returned_to_the_model_and_loop_continues(self):
        (self.root / "AGENTS.md").write_text("Example guidance")
        request = {"workspace": str(self.root), "response_path": str(self.root / "response.json"),
                   "prompt": "Example task", "requested": helpers.config(),
                   "tools": helpers.tools_snapshot(), "tools_sha256": "example-hash"}
        payloads = []
        def send(payload):
            payloads.append(payload)
            if len(payloads) == 1:
                calls = [{"type": "function_call", "name": "run_command", "call_id": "example-call",
                          "arguments": json.dumps({"argv": [sys.executable, "-c", "import time; time.sleep(30)"]})}]
            else:
                tool_result = json.loads(payload["input"][-1]["output"])
                self.assertEqual(tool_result["error"], "timeout")
                self.assertEqual(tool_result["timeout_seconds"], 0.05)
                self.assertNotEqual(tool_result["returncode"], 0)
                calls = []
            return {"model": "example-model", "reasoning": {"effort": "high"},
                    "status": "completed", "output": calls}
        with patch.object(adapter, "TOOL_TIMEOUT_SECONDS", 0.05):
            result = adapter.evaluate(request, send)
        self.assertEqual(len(payloads), 2)
        self.assertTrue(result["tool_trace_complete"])
        self.assertEqual(json.loads(result["tool_calls"][0]["result"])["error"], "timeout")

    def test_unconfirmed_cleanup_aborts_instead_of_returning_a_tool_success(self):
        (self.root / "AGENTS.md").write_text("Example guidance")
        request = {"workspace": str(self.root), "response_path": str(self.root / "response.json"),
                   "prompt": "Example task", "requested": helpers.config(),
                   "tools": helpers.tools_snapshot(), "tools_sha256": "example-hash"}
        def send(payload):
            return {"model": "example-model", "reasoning": {"effort": "high"}, "status": "completed",
                    "output": [{"type": "function_call", "name": "run_command", "call_id": "example-call",
                                "arguments": json.dumps({"argv": ["example-command"]})}]}
        with patch.object(adapter, "capture_command", side_effect=RuntimeError("cleanup failed")):
            with self.assertRaisesRegex(RuntimeError, "cleanup failed"):
                adapter.evaluate(request, send)
        report = helpers.read_json(self.root / "response.json")
        self.assertEqual(report["client_outcome"]["status"], "execution_failed")
        self.assertNotIn("result", report["tool_calls"][0])

    def test_driver_timeout_cancels_the_adapters_active_command_group(self):
        caches = [Path(importlib.util.cache_from_source(str(helpers.SCRIPTS / 'harness_eval' / name)))
                  for name in ("child_environment.py", "process.py")]
        before = {path: path.read_bytes() if path.exists() else None for path in caches}
        pidfile = self.root / "tool.pid"
        wrapper = self.root / "offline-adapter.py"
        tool_code = f'import pathlib, os, time; pathlib.Path({str(pidfile)!r}).write_text(str(os.getpid())); time.sleep(30)'
        response = {"model": "example-model", "reasoning": {"effort": "high"}, "status": "completed",
                    "output": [{"type": "function_call", "name": "run_command", "call_id": "example-call",
                                "arguments": json.dumps({"argv": [sys.executable, "-c", tool_code]})}]}
        wrapper.write_text(f'''import io, json, os, runpy, sys, urllib.request
from unittest.mock import patch
os.environ['OPENAI_API_KEY'] = 'example-sentinel'
sys.path.insert(0, {str(helpers.SCRIPTS / 'harness_eval')!r})
with patch.object(urllib.request, 'urlopen', return_value=io.BytesIO({json.dumps(response).encode()!r})):
    runpy.run_path({str(helpers.SCRIPTS / 'harness_eval/openai_adapter.py')!r}, run_name='__main__')
''')
        config = helpers.config([sys.executable, str(wrapper)], timeout=2)
        output = self.root / "experiment"
        manifest = helpers.prepare(output, "HEAD", config=config)
        item = next(t for t in manifest["schedule"] if t["task"] == "resume")
        trial = output / "trials" / f"{item['order']:03d}"
        result = run_trial(trial, config)
        self.assertEqual(result["status"], "timeout")
        self.assertTrue(pidfile.exists(), (trial / "stderr.log").read_text())
        self.assert_stopped(int(pidfile.read_text()))
        self.assertEqual({path: path.read_bytes() if path.exists() else None for path in caches}, before)


if __name__ == "__main__":
    unittest.main()
