"""Fixture installations must retain the source checkout's Rust toolchain."""

import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import textwrap
import unittest


REPO = Path(__file__).resolve().parents[2]


class FixtureToolchainTests(unittest.TestCase):
    script = "validate-fixtures.sh"
    expected_status = 0

    def setUp(self):
        temporary = tempfile.TemporaryDirectory(prefix="ExampleFixtureToolchain-")
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        self.source = self.root / "source"
        self.outside = self.root / "generated"
        fixtures = self.source / "scripts/fixtures"
        fixtures.mkdir(parents=True)
        self.outside.mkdir()
        shutil.copy2(REPO / "scripts" / self.script, self.source / "scripts")
        (self.source / "rust-toolchain").write_text("1.100.0-example-host")
        tools = self.root / "tools"
        tools.mkdir()
        self.write_tool(tools / "rustup", '''
            import os, pathlib, sys
            assert sys.argv[1:] == ["show", "active-toolchain"]
            if os.environ.get("EXAMPLE_RUSTUP_FAIL"):
                sys.exit(23)
            selected = os.environ.get("RUSTUP_TOOLCHAIN") or pathlib.Path("rust-toolchain").read_text()
            print(selected + " (directory override)")
        ''')
        self.write_tool(tools / "cargo", '''
            import json, os, pathlib, sys
            pathlib.Path(os.environ["EXAMPLE_RESULT"]).write_text(json.dumps({
                "cwd": str(pathlib.Path.cwd()),
                "toolchain": os.environ.get("RUSTUP_TOOLCHAIN"),
            }))
            sys.exit(int(os.environ.get("EXAMPLE_CARGO_EXIT", "0")))
        ''')
        self.write_tool(tools / "jig", "pass")
        # Stop after observing a child Cargo invocation from outside the source
        # checkout, without performing the unrelated full installation suite.
        (fixtures / "lib.sh").write_text('cd "$EXAMPLE_OUTSIDE"\ncargo --version\nexit\n')
        self.result = self.root / "result.json"
        self.env = dict(os.environ, PATH=str(tools) + os.pathsep + os.environ["PATH"],
                        JIG_DEV_BIN=str(tools / "jig"), EXAMPLE_OUTSIDE=str(self.outside),
                        EXAMPLE_RESULT=str(self.result))
        self.env.pop("RUSTUP_TOOLCHAIN", None)

    def write_tool(self, path, body):
        path.write_text("#!" + sys.executable + "\n" + textwrap.dedent(body))
        path.chmod(0o755)

    def invoke(self):
        return subprocess.run(["/bin/bash", str(self.source / "scripts" / self.script)],
                              cwd=self.outside, env=self.env, capture_output=True,
                              text=True, timeout=10)

    def test_source_toolchain_reaches_cargo_outside_checkout(self):
        result = self.invoke()
        self.assertEqual(result.returncode, self.expected_status, result.stderr)
        observed = json.loads(self.result.read_text())
        self.assertTrue(Path(observed["cwd"]).is_relative_to(self.outside.resolve()))
        self.assertEqual(observed["toolchain"], "1.100.0-example-host")

    def test_explicit_toolchain_override_is_preserved(self):
        self.env["RUSTUP_TOOLCHAIN"] = "example-custom-toolchain"
        result = self.invoke()
        self.assertEqual(result.returncode, self.expected_status, result.stderr)
        self.assertEqual(json.loads(self.result.read_text())["toolchain"],
                         "example-custom-toolchain")

    def test_failed_toolchain_resolution_stops_before_installation(self):
        self.env["EXAMPLE_RUSTUP_FAIL"] = "1"
        self.assertEqual(self.invoke().returncode, 23)
        self.assertFalse(self.result.exists())


class GeneratedRustToolchainTests(FixtureToolchainTests):
    script = "check-generated-rust-clippy.sh"
    expected_status = 77

    def setUp(self):
        super().setUp()
        tools = self.root / "tools"
        self.write_tool(tools / "jig", '''
            import pathlib, sys
            assert sys.argv[1] == "init"
            pathlib.Path(sys.argv[2]).mkdir()
        ''')
        self.write_tool(tools / "git", "pass")
        # Observe the real runner's first Cargo call in its generated project,
        # then stop before the unrelated Clippy policy checks need Rust sources.
        self.env.update(TMPDIR=str(self.outside), EXAMPLE_CARGO_EXIT="77")
        self.env.pop("JIG_GENERATED_RUST_ONLY_TOOLCHAIN", None)
        self.env.pop("JIG_GENERATED_RUST_REACT_TOOLCHAIN", None)

    def test_fixture_toolchain_overrides_source_selection(self):
        self.env["RUSTUP_TOOLCHAIN"] = "example-source-toolchain"
        self.env["JIG_GENERATED_RUST_ONLY_TOOLCHAIN"] = "example-fixture-toolchain"
        result = self.invoke()
        self.assertEqual(result.returncode, self.expected_status, result.stderr)
        self.assertEqual(json.loads(self.result.read_text())["toolchain"],
                         "example-fixture-toolchain")


if __name__ == "__main__":
    unittest.main()
