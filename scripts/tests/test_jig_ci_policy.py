import importlib.util
from pathlib import Path
import subprocess
import tempfile
import unittest


SCRIPT = Path(__file__).resolve().parents[1] / "ci" / "policy-scope.py"
SPEC = importlib.util.spec_from_file_location("policy_scope", SCRIPT)
SCOPE = importlib.util.module_from_spec(SPEC)
# Avoid writing __pycache__ into the source tree during repository checks.
exec(compile(SCRIPT.read_text(), str(SCRIPT), "exec"), SCOPE.__dict__)


class PolicyScopeTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory(prefix="example-ci-scope-")
        self.addCleanup(temporary.cleanup)
        self.repo = Path(temporary.name)
        self.git("init", "-q")
        self.git("config", "user.name", "Example")
        self.git("config", "user.email", "example@example.invalid")
        (self.repo / "AGENTS.md").write_text("guidance\n")
        (self.repo / "source.rs").write_text("source\n")
        self.commit()
        self.base = self.git("rev-parse", "HEAD").strip()

    def git(self, *args):
        return subprocess.check_output(["git", *args], cwd=self.repo, text=True)

    def commit(self):
        self.git("add", "-A")
        self.git("commit", "-qm", "Example change")

    def required(self, event="pull_request", base=None):
        base = self.base if base is None else base
        return SCOPE.needs_rust(event, {"pull_request": {"base": {"sha": base}}, "before": base}, self.repo)

    def test_metadata_only_pull_request_and_push_skip_rust(self):
        (self.repo / "AGENTS.md").write_text("new guidance\n")
        (self.repo / ".beads").mkdir()
        (self.repo / ".beads" / "issues.jsonl").write_text("{}\n")
        self.commit()
        self.assertFalse(self.required())
        self.assertFalse(self.required("push"))

    def test_source_change_with_metadata_runs_rust(self):
        (self.repo / "AGENTS.md").write_text("new guidance\n")
        (self.repo / "source.rs").write_text("changed source\n")
        self.commit()
        self.assertTrue(self.required())

    def test_renaming_source_into_metadata_still_runs_rust(self):
        (self.repo / "source.rs").rename(self.repo / "agent-map.md")
        self.commit()
        self.assertTrue(self.required())

    def test_missing_base_empty_diff_and_unknown_events_run_rust(self):
        for event, base in [("pull_request", "0" * 40), ("pull_request", "invalid"),
                            ("pull_request", self.base), ("workflow_dispatch", self.base),
                            ("merge_group", self.base)]:
            with self.subTest(event=event, base=base):
                self.assertTrue(self.required(event, base))

    def test_unknown_path_runs_rust(self):
        (self.repo / "new-input").write_text("unknown\n")
        self.commit()
        self.assertTrue(self.required())


if __name__ == "__main__":
    unittest.main()
