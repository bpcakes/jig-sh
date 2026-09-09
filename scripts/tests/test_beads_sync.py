import json
from pathlib import Path
import runpy
import shutil
import subprocess
import sys
import tempfile
import unittest


SCRIPT = Path(__file__).resolve().parents[1] / "beads-sync.py"


class BeadsExportTests(unittest.TestCase):
    def check(self, records):
        with tempfile.TemporaryDirectory(prefix="example-beads-") as temp:
            root = Path(temp)
            beads = root / ".beads"
            beads.mkdir()
            (beads / "metadata.json").write_text(json.dumps({"jsonl_export": "custom.jsonl"}))
            export = beads / "custom.jsonl"
            export.write_text("\n".join(json.dumps(record) for record in records))
            before = export.read_bytes()
            result = subprocess.run(
                [sys.executable, str(SCRIPT), "--check", "--root", str(root)],
                capture_output=True, text=True,
            )
            self.assertEqual(export.read_bytes(), before)
            return result

    def test_clean_export_accepts_absent_null_and_empty_metadata(self):
        result = self.check([
            {"id": "example-1"},
            {"id": "example-2", "source_repo_path": None},
            {"id": "example-3", "source_repo_path": ""},
        ])
        self.assertEqual(result.returncode, 0, result.stderr)

    def test_path_is_rejected_without_repeating_its_value(self):
        path = "/example/workstation/ExampleProject"
        result = self.check([{"id": "example-1", "source_repo_path": path}])
        self.assertEqual(result.returncode, 1)
        self.assertIn("example-1", result.stderr)
        self.assertNotIn(path, result.stdout + result.stderr)

    def test_tombstone_redaction_preserves_every_other_field(self):
        module = runpy.run_path(str(SCRIPT))
        with tempfile.TemporaryDirectory(prefix="example-beads-") as temp:
            root = Path(temp)
            beads = root / ".beads"
            beads.mkdir()
            (beads / "metadata.json").write_text('{"jsonl_export":"issues.jsonl"}')
            tombstone = {
                "id": "example-deleted", "status": "tombstone", "title": "Example é",
                "updated_at": "2026-01-01T00:00:00Z",
                "source_repo_path": "/example/workstation/ExampleProject",
                "comments": [{"text": "Keep this comment"}],
            }
            other_line = '{"id": "example-open", "status": "open"}\n'
            export = beads / "issues.jsonl"
            export.write_text(json.dumps(tombstone) + "\n" + other_line)
            self.assertEqual(module["clear_tombstone_export_paths"](root), 1)
            del tombstone["source_repo_path"]
            lines = export.read_text().splitlines(keepends=True)
            self.assertEqual(json.loads(lines[0]), tombstone)
            self.assertEqual(lines[1], other_line)
            self.assertEqual(module["clear_tombstone_export_paths"](root), 0)

    @unittest.skipUnless(shutil.which("br"), "br is required for the real sync integration")
    def test_real_sync_cleans_open_closed_and_deleted_issues(self):
        with tempfile.TemporaryDirectory(prefix="example-beads-") as temp:
            root = Path(temp)

            def br(*args):
                return subprocess.run(["br", *args], cwd=root, check=True,
                                      capture_output=True, text=True).stdout

            br("init", "--prefix", "example", "--json")
            config = root / ".beads/config.yaml"
            config.write_text("issue_prefix: example\nsync:\n  auto_flush: false\n")
            export = Path(json.loads(br("info", "--json"))["jsonl_path"])
            before = export.read_bytes() if export.exists() else b""
            ids = [br("create", "--title", f"Example {status}", "--silent").strip()
                   for status in ["open", "closed", "deleted"]]
            br("close", ids[1], "--reason", "Example completed", "--json")
            br("delete", ids[2], "--force", "--json")
            self.assertEqual(export.read_bytes() if export.exists() else b"", before)
            self.assertTrue(json.loads(br("show", ids[0], "--json"))[0]["source_repo_path"])
            for _ in range(2):
                result = subprocess.run([sys.executable, str(SCRIPT), "--root", str(root)],
                                        capture_output=True, text=True)
                self.assertEqual(result.returncode, 0, result.stderr)
                records = [json.loads(line) for line in export.read_text().splitlines()]
                self.assertEqual({record["id"] for record in records}, set(ids))
                self.assertTrue(all(not record.get("source_repo_path") for record in records))
            self.assertFalse(json.loads(br("show", ids[0], "--json"))[0].get("source_repo_path"))
            self.assertFalse(json.loads(br("show", ids[1], "--json"))[0].get("source_repo_path"))


if __name__ == "__main__":
    unittest.main()
