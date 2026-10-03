"""Release validation reuse must never authorize a changed or unchecked tree."""

import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

REPO = Path(__file__).resolve().parents[2]


class ReleaseValidationTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory(prefix="ExampleReleaseValidation-")
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        self.git("init", "-q")
        self.git("config", "user.name", "Example Release")
        self.git("config", "user.email", "release@example.invalid")
        (self.root / ".agent/state").mkdir(parents=True)
        (self.root / ".agent/state/runs.jsonl").write_text("")
        (self.root / "source.txt").write_text("original\n")
        self.git("add", ".")
        self.git("commit", "-qm", "Initial fixture")
        self.receipt = self.root / ".git/validation"
        self.log = self.root / ".git/checks"
        self.env = {key: value for key, value in os.environ.items()
                    if not key.startswith(("ALLOW_", "GITHUB_", "RELEASE_", "EXAMPLE_"))}
        self.env.update(RELEASE_VALIDATION_RECEIPT=str(self.receipt),
                        GITHUB_RUN_ID="100", GITHUB_RUN_ATTEMPT="1", GITHUB_JOB="release")

    def git(self, *args):
        return subprocess.run(["git", *args], cwd=self.root, check=True,
                              capture_output=True, text=True).stdout.strip()

    def run_shell(self, command, *, env=None, checks="true"):
        script = '''set -euo pipefail
source "$1"
run_release_checks() {
  printf 'check\\n' >> .git/checks
''' + checks + '''
}
''' + command
        return subprocess.run([shutil.which("bash"), "-c", script, "bash",
                               str(REPO / "scripts/release-validation.sh")],
                              cwd=self.root, env=env or self.env,
                              capture_output=True, text=True)

    def assert_ok(self, result):
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)

    def checked(self):
        self.assert_ok(self.run_shell("release_check 1.2.3"))

    def test_check_is_reused_for_tag_and_publish_of_same_commit(self):
        self.checked()
        self.assert_ok(self.run_shell("require_release_validation 1.2.3"))
        self.git("tag", "-a", "v1.2.3", "-m", "Example release")
        self.assert_ok(self.run_shell("require_release_validation 1.2.3"))
        self.assertEqual(self.log.read_text(), "check\n")

    def test_standalone_commands_still_run_full_validation(self):
        env = dict(self.env)
        del env["RELEASE_VALIDATION_RECEIPT"]
        self.assert_ok(self.run_shell("release_check 1.2.3\n"
                                      "require_release_validation 1.2.3\n"
                                      "require_release_validation 1.2.3", env=env))
        self.assertEqual(self.log.read_text(), "check\n" * 3)
        self.assertFalse(self.receipt.exists())

    def test_missing_malformed_or_different_version_evidence_is_rejected(self):
        self.assertNotEqual(self.run_shell("require_release_validation 1.2.3").returncode, 0)
        self.assertFalse(self.log.exists())
        self.receipt.write_text("invalid\n")
        self.assertNotEqual(self.run_shell("require_release_validation 1.2.3").returncode, 0)
        self.checked()
        self.assertNotEqual(self.run_shell("require_release_validation 1.2.4").returncode, 0)
        self.assertEqual(self.log.read_text(), "check\n")

    def test_evidence_cannot_cross_run_attempts_or_jobs(self):
        self.checked()
        for key, value in [("GITHUB_RUN_ID", "101"), ("GITHUB_RUN_ATTEMPT", "2"),
                           ("GITHUB_JOB", "another-job")]:
            with self.subTest(key=key):
                result = self.run_shell("require_release_validation 1.2.3",
                                        env=dict(self.env, **{key: value}))
                self.assertNotEqual(result.returncode, 0)

    def test_changed_commit_index_worktree_or_untracked_input_is_rejected(self):
        self.checked()
        source = self.root / "source.txt"
        source.write_text("changed\n")
        self.assertNotEqual(self.run_shell("require_release_validation 1.2.3").returncode, 0)
        self.git("add", "source.txt")
        self.assertNotEqual(self.run_shell("require_release_validation 1.2.3").returncode, 0)
        self.git("commit", "-qm", "Changed fixture")
        self.assertNotEqual(self.run_shell("require_release_validation 1.2.3").returncode, 0)
        self.checked()
        (self.root / "untracked.txt").write_text("new input\n")
        self.assertNotEqual(self.run_shell("require_release_validation 1.2.3").returncode, 0)

    def test_failed_revalidation_invalidates_previous_success(self):
        self.checked()
        result = self.run_shell("release_check 1.2.3", checks="false")
        self.assertNotEqual(result.returncode, 0)
        self.assertFalse(self.receipt.exists())
        self.assertNotEqual(self.run_shell("require_release_validation 1.2.3").returncode, 0)

    def test_checks_cannot_change_the_validated_tree_or_commit(self):
        for mutation in ["printf changed > source.txt", "git commit --allow-empty -qm Changed"]:
            with self.subTest(mutation=mutation):
                result = self.run_shell("release_check 1.2.3", checks=mutation)
                self.assertNotEqual(result.returncode, 0)
                self.assertFalse(self.receipt.exists())
                self.git("restore", "source.txt")

    def test_only_the_existing_explicit_run_journal_exception_is_allowed(self):
        journal = self.root / ".agent/state/runs.jsonl"
        self.checked()
        journal.write_text('{"event":"example"}\n')
        self.assertNotEqual(self.run_shell("require_release_validation 1.2.3").returncode, 0)
        env = dict(self.env, ALLOW_RELEASE_RUN_JOURNAL_DIRTY="1")
        self.assert_ok(self.run_shell("require_release_validation 1.2.3", env=env))
        self.git("add", ".agent/state/runs.jsonl")
        self.assertNotEqual(self.run_shell("require_release_validation 1.2.3", env=env).returncode, 0)

    def test_allow_dirty_never_authorizes_reuse(self):
        self.checked()
        env = dict(self.env, ALLOW_DIRTY="1")
        self.assertNotEqual(self.run_shell("require_release_validation 1.2.3", env=env).returncode, 0)
        self.assertNotEqual(self.run_shell("release_check 1.2.3", env=env).returncode, 0)
        self.assertFalse(self.receipt.exists())


if __name__ == "__main__":
    unittest.main()
