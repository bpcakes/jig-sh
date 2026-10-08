import importlib.util
import json
import os
from pathlib import Path
import re
import subprocess
import tempfile
import unittest


SCRIPT = Path(__file__).resolve().parents[1] / "test-rust-affected.py"
REPO = SCRIPT.parent.parent
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


class CommandTests(unittest.TestCase):
    def test_full_suite_command_has_no_filter(self):
        command = affected.nextest_command(None, ["--no-fail-fast"])
        self.assertEqual(command[:5], ["cargo", "nextest", "run", "--workspace", "-P"])
        self.assertNotIn("-E", command)
        self.assertEqual(command[-1], "--no-fail-fast")

    def test_scoped_command_selects_reverse_dependencies_exactly(self):
        command = affected.nextest_command({"example-core", "example-cli"}, [])
        self.assertEqual(command[-2:], ["-E", "rdeps(=example-cli) | rdeps(=example-core)"])

    def test_scoped_command_adds_packages_that_read_changed_files(self):
        command = affected.nextest_command({"jig-sh"}, [])
        self.assertEqual(command[-1], "rdeps(=jig-sh) | rdeps(=jig-ui)")


class ChangedPathTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory(prefix="example-affected-")
        self.addCleanup(temporary.cleanup)
        self.repo = Path(temporary.name)
        self.git("init", "-q")
        self.write("crates/core/src/lib.rs", "base\n")
        self.write("crates/core/src/old.rs", "old\n")
        self.git("add", ".")
        self.git("commit", "-q", "-m", "base")
        self.base = self.git("rev-parse", "HEAD").strip()

    def git(self, *args):
        return subprocess.run(
            ["git", "-c", "user.name=Fixture", "-c", "user.email=fixture@example.com", *args],
            cwd=self.repo, check=True, capture_output=True, text=True,
        ).stdout

    def write(self, relative, contents):
        path = self.repo / relative
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(contents)

    def changed(self):
        return affected.changed_paths(self.repo, self.base)

    def test_untracked_files_are_reported(self):
        self.write("crates/core/src/new.rs", "new\n")
        self.assertEqual(self.changed(), ["crates/core/src/new.rs"])

    def test_renames_report_source_and_destination(self):
        self.git("mv", "crates/core/src/old.rs", "crates/core/src/renamed.rs")
        self.git("commit", "-q", "-m", "rename")
        self.assertEqual(self.changed(), ["crates/core/src/old.rs", "crates/core/src/renamed.rs"])

    def test_committed_changes_survive_a_local_revert(self):
        self.write("crates/core/src/lib.rs", "changed\n")
        self.git("commit", "-q", "-am", "change")
        self.write("crates/core/src/lib.rs", "base\n")
        self.assertEqual(self.changed(), ["crates/core/src/lib.rs"])

    def test_non_ascii_filenames_are_not_quoted(self):
        self.write(".agent/notes/café.md", "note\n")
        self.write("crates/core/src/naïve.rs", "code\n")
        paths = self.changed()
        self.assertEqual(paths, [".agent/notes/café.md", "crates/core/src/naïve.rs"])
        self.assertEqual(
            affected.affected_packages(paths, {"crates/core": "example-core"}), {"example-core"}
        )


# Files that name another package's directory without their own tests reading
# it: synthetic fixture repositories, and the dashboard parity table whose
# listed sources jig-ui's contract tests read (covered by EXTRA_CONSUMERS).
NON_READING_REFERENCES = {
    ("crates/jig-bootstrap/src/tests/template_source.rs", "crates/jig"),
    ("crates/jig-bootstrap/src/tests/template_source/source_stamp.rs", "crates/jig"),
    ("crates/jig-dashboard/src/parity.rs", "crates/jig"),
    ("crates/jig-dashboard/src/parity.rs", "crates/jig-ui"),
}


class WorkspaceEdgeTests(unittest.TestCase):
    def test_cross_package_file_reads_are_covered(self):
        """A package whose sources name another package's directory must be one
        of its reverse dependencies or be listed in EXTRA_CONSUMERS."""
        metadata = json.loads(
            subprocess.run(
                ["cargo", "metadata", "--format-version", "1", "--no-deps"],
                cwd=REPO, check=True, capture_output=True, text=True,
            ).stdout
        )
        roots = affected.package_roots(metadata)
        names = set(roots.values())
        dependents = {name: set() for name in names}
        for package in metadata["packages"]:
            for dependency in package["dependencies"]:
                if dependency["name"] in names and package["name"] in names:
                    dependents[dependency["name"]].add(package["name"])

        def reverse_closure(name):
            seen, pending = {name}, [name]
            while pending:
                for dependent in dependents[pending.pop()]:
                    if dependent not in seen:
                        seen.add(dependent)
                        pending.append(dependent)
            return seen

        directories = {directory: name for directory, name in roots.items()}
        uncovered = []
        for directory, reader in roots.items():
            for path in (REPO / directory).rglob("*.rs"):
                text = path.read_text(errors="ignore")
                for match in set(re.findall(r"crates/[a-z0-9-]+", text)):
                    owner = directories.get(match)
                    if owner is None or owner == reader:
                        continue
                    if reader in reverse_closure(owner) or reader in affected.EXTRA_CONSUMERS.get(owner, ()):
                        continue
                    if (path.relative_to(REPO).as_posix(), match) in NON_READING_REFERENCES:
                        continue
                    uncovered.append(f"{reader} reads {match} ({path.relative_to(REPO)})")
        self.assertEqual(uncovered, [], "add these readers to EXTRA_CONSUMERS")


if __name__ == "__main__":
    unittest.main()
