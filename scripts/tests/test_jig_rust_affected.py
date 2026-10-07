import importlib.util
from pathlib import Path
import unittest


SCRIPT = Path(__file__).resolve().parents[1] / "test-rust-affected.py"
SPEC = importlib.util.spec_from_file_location("test_rust_affected", SCRIPT)
affected = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(affected)

METADATA = {
    "workspace_root": "/workspace",
    "workspace_members": ["example-cli 0.1.0", "example-core 0.1.0"],
    "packages": [
        {"id": "example-cli 0.1.0", "name": "example-cli", "manifest_path": "/workspace/crates/cli/Cargo.toml"},
        {"id": "example-core 0.1.0", "name": "example-core", "manifest_path": "/workspace/crates/core/Cargo.toml"},
        {"id": "outside 1.0.0", "name": "outside", "manifest_path": "/registry/outside/Cargo.toml"},
    ],
}


class AffectedPackagesTests(unittest.TestCase):
    def setUp(self):
        self.roots = affected.package_roots(METADATA)

    def test_package_roots_cover_only_workspace_members(self):
        self.assertEqual(self.roots, {"crates/cli": "example-cli", "crates/core": "example-core"})

    def test_files_select_their_owning_packages(self):
        packages = affected.affected_packages(
            ["crates/core/src/lib.rs", "crates/core/AGENTS.md", "crates/cli/tests/end_to_end.rs"],
            self.roots,
        )
        self.assertEqual(packages, {"example-cli", "example-core"})

    def test_package_directory_prefixes_do_not_match_sibling_directories(self):
        with self.assertRaises(affected.FullSuite) as raised:
            affected.affected_packages(["crates/core-extra/src/lib.rs"], self.roots)
        self.assertEqual(raised.exception.path, "crates/core-extra/src/lib.rs")

    def test_shared_inputs_outside_packages_require_the_full_suite(self):
        for path in ["Cargo.lock", ".config/nextest.toml", "templates/project/.jig.toml.jinja", "docs/configuration.md"]:
            with self.subTest(path=path), self.assertRaises(affected.FullSuite):
                affected.affected_packages(["crates/core/src/lib.rs", path], self.roots)

    def test_notes_that_no_test_reads_are_ignored(self):
        packages = affected.affected_packages(
            [
                ".beads/issues.jsonl",
                ".agent/plans/example.md",
                "docs/plans/example.md",
                "scripts/__pycache__/example.cpython-314.pyc",
            ],
            self.roots,
        )
        self.assertEqual(packages, set())

    def test_filterset_selects_reverse_dependencies_exactly(self):
        self.assertEqual(
            affected.filterset({"example-core", "example-cli"}),
            "rdeps(=example-cli) | rdeps(=example-core)",
        )


if __name__ == "__main__":
    unittest.main()
