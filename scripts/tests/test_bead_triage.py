import json
from pathlib import Path
import runpy
import subprocess
import tempfile
import unittest
from unittest.mock import patch


SCRIPT = Path(__file__).resolve().parents[1] / "bead-triage.py"
MODULE = runpy.run_path(str(SCRIPT))


class BeadTriageTests(unittest.TestCase):
    def test_sync_export_and_disposable_snapshot_keep_rankings_without_commands(self):
        seen = []
        snapshot = None
        export = '{"id":"example-1","title":"Example é"}\n'

        def run(command, cwd):
            nonlocal snapshot
            seen.append(command)
            if command[:2] == ["bead", "sync"]:
                return '{}'
            if command == ["bead", "export"]:
                return export
            self.assertEqual(command[0], "bv")
            self.assertIn("--robot-triage", command)
            self.assertIn("--no-cache", command)
            snapshot = Path(command[command.index("--db") + 1])
            self.assertEqual(snapshot.read_text(encoding="utf-8"), export)
            self.assertEqual(snapshot.parent, Path(cwd))
            return json.dumps({"triage": {
                "commands": {"claim_top": "br update example-1 --claim"},
                "recommendations": [{"id": "example-1", "score": 0.8,
                                     "claim_command": "br update example-1 --claim"}],
            }})

        with tempfile.TemporaryDirectory(prefix="example-triage-") as temporary:
            root = Path(temporary)
            with patch.dict(MODULE["triage"].__globals__, run=run):
                result = MODULE["triage"](root)
            self.assertEqual(list(root.iterdir()), [])
        self.assertFalse(snapshot.exists())
        self.assertEqual(seen[:2], [["bead", "sync", "--json"], ["bead", "export"]])
        self.assertEqual(result["triage"], {"recommendations": [{"id": "example-1", "score": 0.8}]})
        self.assertTrue(all("bead " in hint for hint in result["usage_hints"]))

    def test_unpublished_sync_does_not_export_or_rank_stale_data(self):
        with patch.dict(MODULE["triage"].__globals__, run=lambda command, root: '{"unpublished": 1}'):
            with self.assertRaisesRegex(ValueError, "unpublished"):
                MODULE["triage"](Path("/example/ExampleProject"))

    def test_failed_sync_does_not_fall_back_to_an_old_export(self):
        calls = []

        def fail(command, root):
            calls.append(command)
            raise subprocess.CalledProcessError(1, command)

        with patch.dict(MODULE["triage"].__globals__, run=fail):
            with self.assertRaises(subprocess.CalledProcessError):
                MODULE["triage"](Path("/example/ExampleProject"))
        self.assertEqual(calls, [["bead", "sync", "--json"]])

    def test_partial_journal_is_not_ranked(self):
        for field in ("integrity", "waiting_events"):
            with self.subTest(field=field):
                with patch.dict(MODULE["triage"].__globals__,
                                run=lambda command, root: json.dumps({field: 1})):
                    with self.assertRaisesRegex(ValueError, "incomplete or invalid"):
                        MODULE["triage"](Path("/example/ExampleProject"))

    def test_source_path_is_rejected_without_disclosing_its_value(self):
        source = "/example/workstation/ExampleProject"
        contents = json.dumps({"id": "example-1", "source_repo_path": source})
        with self.assertRaises(ValueError) as error:
            MODULE["check_export"](contents)
        self.assertNotIn(source, str(error.exception))

    def test_null_empty_absent_metadata_and_unicode_line_separator_are_allowed(self):
        records = [{"id": "example-1", "title": "Example\u2028title"},
                   {"id": "example-2", "source_repo_path": None},
                   {"id": "example-3", "source_repo_path": ""}]
        MODULE["check_export"]("\n".join(json.dumps(record, ensure_ascii=False) for record in records))


if __name__ == "__main__":
    unittest.main()
