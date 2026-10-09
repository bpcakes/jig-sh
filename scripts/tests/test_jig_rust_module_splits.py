"""Exercise the module-split guard against small source trees."""

from pathlib import Path
import subprocess
import sys
import tempfile
import unittest


GUARD = Path(__file__).resolve().parents[1] / "check-rust-module-splits.py"


class RustModuleSplits(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="example-project-")
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.write("crates/example/src/lib.rs", "mod parser;\n")
        self.write("crates/example/src/parser.rs", "pub fn parse() {}\n")

    def write(self, relative, text):
        path = self.root / relative
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(text)

    def guard(self):
        return subprocess.run(
            [sys.executable, str(GUARD), "--root", str(self.root)],
            capture_output=True, text=True, check=False,
        )

    def test_modules_pass(self):
        result = self.guard()
        self.assertEqual(result.returncode, 0, result.stderr)

    def test_spliced_source_is_rejected(self):
        self.write("crates/example/src/lib.rs", 'mod parser;\ninclude!("lib/rest.rs");\n')
        result = self.guard()
        self.assertEqual(result.returncode, 1)
        self.assertIn('crates/example/src/lib.rs:2: include!("lib/rest.rs")', result.stderr)

    def test_generated_sources_may_be_spliced(self):
        self.write(
            "crates/example/src/templates.rs",
            'include!(concat!(env!("OUT_DIR"), "/templates.rs"));\n'
            'include!("embedded_templates_snapshot.rs");\n',
        )
        result = self.guard()
        self.assertEqual(result.returncode, 0, result.stderr)

    def test_text_that_only_mentions_include_passes(self):
        self.write(
            "crates/example/src/parser.rs",
            '// include!("old.rs") used to live here\n'
            'const EXAMPLE: &str = "include!(\\"old.rs\\")";\n'
            'const TEMPLATE: &str = include_str!("template.rs");\n',
        )
        result = self.guard()
        self.assertEqual(result.returncode, 0, result.stderr)

    def test_positional_names_are_rejected(self):
        self.write("crates/example/src/parser/part_01.rs", "")
        self.write("crates/example/src/parser/part_02_assertions.rs", "")
        self.write("crates/example/src/tail.rs", "")
        result = self.guard()
        self.assertEqual(result.returncode, 1)
        for name in ("parser/part_01.rs", "parser/part_02_assertions.rs", "src/tail.rs"):
            self.assertIn(f"{name}: named by position", result.stderr)

    def test_parts_directory_is_reported_once(self):
        self.write("crates/example/src/lib_parts/first.rs", "")
        self.write("crates/example/src/lib_parts/second.rs", "")
        result = self.guard()
        self.assertEqual(result.returncode, 1)
        self.assertEqual(result.stderr.count("crates/example/src/lib_parts/: a `_parts`"), 1)

    def test_build_output_is_skipped(self):
        self.write("crates/example/target/debug/build/part_01.rs", "")
        result = self.guard()
        self.assertEqual(result.returncode, 0, result.stderr)


if __name__ == "__main__":
    unittest.main()
