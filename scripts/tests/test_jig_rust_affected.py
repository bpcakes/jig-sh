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


HOST = f"({affected.HOST_SURFACE_TESTS})"
GUIDES = f"({affected.GUIDE_TESTS})"


class AffectedSelectionTests(unittest.TestCase):
    def setUp(self):
        self.roots = affected.package_roots(METADATA)

    def select(self, *paths, untracked=frozenset()):
        return affected.scope_filterset(affected.affected_selection(list(paths), self.roots, untracked))

    def test_package_roots_cover_only_workspace_members(self):
        self.assertEqual(self.roots, {"crates/cli": "example-cli", "crates/core": "example-core"})

    def test_files_select_their_owning_packages(self):
        selection = affected.affected_selection(
            ["crates/core/src/lib.rs", "crates/core/AGENTS.md", "crates/cli/tests/end_to_end.rs"],
            self.roots,
        )
        self.assertEqual(selection.packages, {"example-cli", "example-core"})

    def test_package_directory_prefixes_do_not_match_sibling_directories(self):
        with self.assertRaises(affected.FullSuite) as raised:
            affected.affected_selection(["crates/core-extra/src/lib.rs"], self.roots)
        self.assertEqual(raised.exception.path, "crates/core-extra/src/lib.rs")

    def test_build_and_harness_inputs_require_the_full_suite(self):
        for path in [
            "Cargo.lock",
            ".config/nextest.toml",
            ".gitattributes",
            ".jig.toml",
            ".agent/jig-contract.json",
            "scripts/jig",
            "scripts/ci/test-rust.sh",
            ".github/workflows/rust-tests.yml",
        ]:
            with self.subTest(path=path), self.assertRaises(affected.FullSuite) as raised:
                affected.affected_selection(["crates/core/src/lib.rs", path], self.roots)
            self.assertEqual(raised.exception.reason, "is a build or test-harness input")

    def test_unlisted_paths_outside_packages_require_the_full_suite(self):
        for path in ["Makefile", "scripts/new-check.sh", "scripts/ci/new-step.sh", "tests/new.rs"]:
            with self.subTest(path=path), self.assertRaises(affected.FullSuite) as raised:
                affected.affected_selection([path], self.roots)
            self.assertIn("not listed in OUTSIDE_PACKAGE_READERS", raised.exception.reason)

    def test_directory_patterns_do_not_match_sibling_paths(self):
        for path in ["docs", "templates-old/example.jinja", "scripts/jig-new"]:
            with self.subTest(path=path), self.assertRaises(affected.FullSuite):
                affected.affected_selection([path], self.roots)

    def test_templates_select_the_packages_that_embed_or_include_them(self):
        self.assertEqual(
            self.select("templates/project/.jig.toml.jinja"),
            f"rdeps(=jig-bootstrap) | rdeps(=jig-context) | rdeps(=jig-repository) | {HOST}",
        )

    def test_known_runtime_readers_select_their_tests(self):
        """Tests read these files at run time, which the include guard cannot see."""
        for path in [
            "README.md",
            "CHANGELOG.md",
            "agent-map.md",
            "scripts/release.sh",
            "docs/developer-ux.md",
            "docs/repo-intent.md",
            "docs/public-contract.md",
            "docs/status-provider.md",
        ]:
            with self.subTest(path=path):
                self.assertEqual(self.select(path), f"{HOST} | {GUIDES}")
        self.assertEqual(self.select("scripts/install-jig.sh"), f"rdeps(=jig-sh) | rdeps(=jig-ui) | {HOST}")
        for path in ["scripts/check-supported-host-surface.sh", "docs/plans/example.md"]:
            with self.subTest(path=path):
                self.assertIn(HOST, self.select(path))

    def test_paths_no_test_reads_select_only_the_host_surface_scan(self):
        for path in [
            "AGENTS.md",
            "CONTRIBUTING.md",
            ".agent/state/receipts.jsonl",
            ".agent/plans/example.md",
            ".beadroll.json",
            ".beads/issues.jsonl",
            ".github/workflows/release.yml",
            "scripts/tests/test_jig_example.py",
            "scripts/ci/nightly-scope.py",
            "examples/backend-only.toml",
        ]:
            with self.subTest(path=path):
                self.assertEqual(self.select(path), HOST)

    def test_package_changes_combine_with_outside_readers(self):
        self.assertEqual(
            self.select("crates/core/src/lib.rs", "README.md"),
            f"rdeps(=example-core) | {HOST} | {GUIDES}",
        )

    def test_untracked_files_do_not_select_the_tracked_file_scan(self):
        self.assertEqual(self.select(".agent/notes/example.md", untracked={".agent/notes/example.md"}), "none()")
        self.assertEqual(self.select("docs/example.md", untracked={"docs/example.md"}), GUIDES)
        self.assertEqual(
            self.select("crates/core/src/new.rs", untracked={"crates/core/src/new.rs"}), "rdeps(=example-core)"
        )

    def test_bytecode_caches_are_ignored(self):
        self.assertEqual(self.select("scripts/__pycache__/example.cpython-314.pyc"), "none()")
        self.assertEqual(self.select(), "none()")


class CommandTests(unittest.TestCase):
    def test_scope_filterset_covers_full_empty_and_scoped_selections(self):
        self.assertEqual(affected.scope_filterset(None), "all()")
        self.assertEqual(affected.scope_filterset(affected.Selection()), "none()")
        self.assertEqual(
            affected.scope_filterset(affected.Selection({"jig-sh"}, {affected.HOST_SURFACE_TESTS})),
            f"rdeps(=jig-sh) | rdeps(=jig-ui) | {HOST}",
        )

    def test_full_suite_command_has_no_filter(self):
        command = affected.nextest_command(None, ["--no-fail-fast"])
        self.assertEqual(command[:5], ["cargo", "nextest", "run", "--workspace", "-P"])
        self.assertNotIn("-E", command)
        self.assertEqual(command[-1], "--no-fail-fast")

    def test_scoped_command_selects_reverse_dependencies_exactly(self):
        command = affected.nextest_command(affected.Selection({"example-core", "example-cli"}), [])
        self.assertEqual(command[-2:], ["-E", "rdeps(=example-cli) | rdeps(=example-core)"])

    def test_scoped_command_adds_packages_that_read_changed_files(self):
        command = affected.nextest_command(affected.Selection({"jig-sh"}), [])
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
            affected.affected_selection(paths, {"crates/core": "example-core"}).packages,
            {"example-core"},
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
    @classmethod
    def setUpClass(cls):
        metadata = json.loads(
            subprocess.run(
                ["cargo", "metadata", "--format-version", "1", "--no-deps"],
                cwd=REPO, check=True, capture_output=True, text=True,
            ).stdout
        )
        cls.roots = affected.package_roots(metadata)
        names = set(cls.roots.values())
        cls.dependents = {name: set() for name in names}
        for package in metadata["packages"]:
            for dependency in package["dependencies"]:
                if dependency["name"] in names and package["name"] in names:
                    cls.dependents[dependency["name"]].add(package["name"])

    def reverse_closure(self, name):
        seen, pending = {name}, [name]
        while pending:
            for dependent in self.dependents[pending.pop()]:
                if dependent not in seen:
                    seen.add(dependent)
                    pending.append(dependent)
        return seen

    def test_cross_package_file_reads_are_covered(self):
        """A package whose sources name another package's directory must be one
        of its reverse dependencies or be listed in EXTRA_CONSUMERS."""
        directories = {directory: name for directory, name in self.roots.items()}
        uncovered = []
        for directory, reader in self.roots.items():
            for path in (REPO / directory).rglob("*.rs"):
                text = path.read_text(errors="ignore")
                for match in set(re.findall(r"crates/[a-z0-9-]+", text)):
                    owner = directories.get(match)
                    if owner is None or owner == reader:
                        continue
                    if reader in self.reverse_closure(owner) or reader in affected.EXTRA_CONSUMERS.get(owner, ()):
                        continue
                    if (path.relative_to(REPO).as_posix(), match) in NON_READING_REFERENCES:
                        continue
                    uncovered.append(f"{reader} reads {match} ({path.relative_to(REPO)})")
        self.assertEqual(uncovered, [], "add these readers to EXTRA_CONSUMERS")

    def test_files_included_from_outside_packages_select_their_readers(self):
        """A file that a package's sources include from outside every package
        must select that package or a dependency of it, or run the whole suite."""
        relative_include = re.compile(r'include_(?:str|bytes)!\(\s*"([^"]+)"\s*\)')
        manifest_include = re.compile(
            r'include_(?:str|bytes)!\(\s*concat!\(\s*env!\("CARGO_MANIFEST_DIR"\),\s*"([^"]+)"\s*\)\s*\)'
        )
        uncovered = []
        for directory, reader in self.roots.items():
            for path in (REPO / directory).rglob("*.rs"):
                text = path.read_text(errors="ignore")
                targets = [path.parent / include for include in relative_include.findall(text)]
                targets += [REPO / directory / include.lstrip("/") for include in manifest_include.findall(text)]
                for target in targets:
                    included = Path(os.path.normpath(target)).relative_to(REPO).as_posix()
                    if any(included.startswith(root + "/") for root in self.roots):
                        continue
                    try:
                        selection = affected.affected_selection([included], self.roots)
                    except affected.FullSuite:
                        continue
                    if not any(reader in self.reverse_closure(package) for package in selection.packages):
                        uncovered.append(f"{reader} includes {included} ({path.relative_to(REPO)})")
        self.assertEqual(uncovered, [], "add these readers to OUTSIDE_PACKAGE_READERS")

    def test_reader_tables_name_existing_packages_and_test_binaries(self):
        directories = {name: directory for directory, name in self.roots.items()}
        filters = {affected.HOST_SURFACE_TESTS}
        for _, packages, entry_filters in affected.OUTSIDE_PACKAGE_READERS:
            self.assertLessEqual(set(packages), set(directories), packages)
            filters.update(entry_filters)
        for test_filter in filters:
            with self.subTest(test_filter=test_filter):
                match = re.fullmatch(r"package\(=([a-z0-9-]+)\) & binary\(=([a-z0-9_]+)\)", test_filter)
                self.assertIsNotNone(match, "extend this check for the new filter form")
                package, binary = match.groups()
                self.assertTrue((REPO / directories[package] / "tests" / f"{binary}.rs").is_file())


if __name__ == "__main__":
    unittest.main()
