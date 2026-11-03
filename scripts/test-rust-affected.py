#!/usr/bin/env python3
"""Run only the Rust tests that a change can affect.

Changed files are those that differ between BASE (by default the merge base
with origin/master) and HEAD, plus staged, unstaged, and untracked files. A
file inside a workspace package selects that package, every package that
depends on it through nextest's `rdeps()`, and any package listed as reading
its files. A file outside every package selects the tests listed as reading
it in OUTSIDE_PACKAGE_READERS. Inputs to how the suite builds and runs, and
any path the tables do not list, run the whole workspace suite. Every change
to a tracked file also selects the supported-host policy tests, which scan
every tracked file.

Usage: scripts/test-rust-affected.py [--base REF] [--print | --filterset] [-- NEXTEST_ARGS...]

--filterset prints only the selection as a nextest filterset: `all()` when
the whole suite must run, `none()` when nothing changed, and a union of
`rdeps(...)` and narrower test filters otherwise. CI combines it with its
phase filters.
"""

from __future__ import annotations

import argparse
import json
import os
import shlex
import subprocess
import sys
from pathlib import Path, PurePosixPath

ROOT = Path(__file__).resolve().parent.parent

# A pattern ending in "/" matches every path below that directory; any other
# pattern matches one path.

# Paths outside every package that can change how any test builds or runs.
FULL_SUITE_PATHS = (
    "Cargo.toml",
    "Cargo.lock",
    "rust-toolchain",
    "rust-toolchain.toml",
    ".cargo/",
    ".config/",
    # Checkout normalization and ignore rules, which the tests that clone
    # this checkout depend on.
    ".gitattributes",
    ".gitignore",
    # CI runs the suite through the launcher as `scripts/jig check
    # test-locked`, with a development binary from scripts/jig-dev. The
    # launcher hands every test this checkout as JIG_REPO_ROOT.
    ".jig.toml",
    ".agent/jig-contract.json",
    "scripts/jig",
    "scripts/jig-dev",
    # The CI test step, its environment, and this selection.
    ".github/workflows/rust-tests.yml",
    "scripts/ci/setup-user-systemd.sh",
    "scripts/ci/test-rust.sh",
    "scripts/test-rust-affected.py",
)

# jig-sh's supported-host policy tests scan the name and contents of every
# tracked file, so every change to a tracked file selects them.
HOST_SURFACE_TESTS = "package(=jig-sh) & binary(=supported_host_surface)"
# jig-sh's architecture tests read README.md, CHANGELOG.md, agent-map.md,
# scripts/release.sh, and guides under docs/, and require docs/status-provider.md
# to stay absent.
GUIDE_TESTS = "package(=jig-sh) & binary(=ui_architecture)"

# Other paths outside every package, matched in order: the packages whose
# tests read them (selected with their dependents) and narrower Nextest
# filtersets. Entries with neither are read by no test except the host-surface
# scan. A path that no entry matches runs the whole suite.
OUTSIDE_PACKAGE_READERS = (
    # jig-bootstrap's build script embeds every template, and jig-sh links
    # it; jig-context and jig-repository tests include single templates.
    ("templates/", ("jig-bootstrap", "jig-context", "jig-repository"), ()),
    ("docs/", (), (GUIDE_TESTS,)),
    ("README.md", (), (GUIDE_TESTS,)),
    ("CHANGELOG.md", (), (GUIDE_TESTS,)),
    ("agent-map.md", (), (GUIDE_TESTS,)),
    ("scripts/release.sh", (), (GUIDE_TESTS,)),
    # jig-sh tests run the installer and check that it exists.
    ("scripts/install-jig.sh", ("jig-sh",), ()),
    # Other guides, harness state, tracker records, other workflows, and the
    # policy and file-budget settings that other CI jobs check.
    ("AGENTS.md", (), ()),
    ("CONTRIBUTING.md", (), ()),
    ("SECURITY.md", (), ()),
    ("LICENSE", (), ()),
    (".agent/", (), ()),
    (".beadroll.json", (), ()),
    (".beads/", (), ()),
    (".github/", (), ()),
    (".jig/", (), ()),
    ("clippy.toml", (), ()),
    ("landing/", (), ()),
    # Inputs of the shell fixtures that the rendered-fixtures job runs.
    ("examples/", (), ()),
    ("tests/fixtures/", (), ()),
    ("scripts/fixtures/", (), ()),
    ("scripts/validate-fixtures.sh", (), ()),
    ("scripts/check-generated-rust-clippy.sh", (), ()),
    # Python tests, CI gates, repository checks, and release and maintenance
    # tooling that no Rust test runs.
    ("scripts/tests/", (), ()),
    ("scripts/ci/nightly-scope.py", (), ()),
    ("scripts/ci/policy-scope.py", (), ()),
    ("scripts/check-launcher-template.sh", (), ()),
    ("scripts/check-rust-format.sh", (), ()),
    ("scripts/check-rust-module-splits.py", (), ()),
    ("scripts/check-supported-host-surface.sh", (), ()),
    ("scripts/check-vault-production-features.py", (), ()),
    ("scripts/jig-source-runtime.py", (), ()),
    ("scripts/install.sh", (), ()),
    ("scripts/release-notes.sh", (), ()),
    ("scripts/release-validation.sh", (), ()),
    ("scripts/package-release-binary.py", (), ()),
    ("scripts/publish-release-binaries.py", (), ()),
    ("scripts/smoke-release-binary.py", (), ()),
    ("scripts/deploy-landing.sh", (), ()),
    ("scripts/bead-triage.py", (), ()),
    ("scripts/benchmark-ready-dependents.py", (), ()),
    ("scripts/enforce-coverage.js", (), ()),
    ("scripts/new-checkout.sh", (), ()),
    ("scripts/test-rust-partition.sh", (), ()),
)

# Packages whose tests read another package's files without depending on it,
# so Cargo's dependency graph cannot see the edge. The jig-ui dashboard
# contract tests check that jig-sh's terminal tests are registered.
EXTRA_CONSUMERS = {
    "jig-sh": ("jig-ui",),
}

NEXTEST_ARGS = ["-P", "local", "--status-level", "fail", "--final-status-level", "fail"]


class FullSuite(Exception):
    """A changed path can affect tests outside any single package."""

    def __init__(self, path: str, reason: str) -> None:
        super().__init__(path)
        self.path = path
        self.reason = reason


class Selection:
    """Packages selected with their dependents, plus narrower Nextest filters."""

    def __init__(self, packages: set[str] | None = None, filters: set[str] | None = None) -> None:
        self.packages = set(packages or ())
        self.filters = set(filters or ())


def git(root: Path, *args: str) -> str:
    return subprocess.run(
        ["git", *args], cwd=root, check=True, capture_output=True, text=True
    ).stdout


def nul_separated(output: str) -> list[str]:
    return [path for path in output.split("\0") if path]


def default_base(root: Path) -> str:
    for upstream in ("origin/master", "master"):
        try:
            return git(root, "merge-base", "HEAD", upstream).strip()
        except subprocess.CalledProcessError:
            continue
    raise SystemExit("Could not find a merge base with origin/master or master; pass --base.")


def untracked_paths(root: Path) -> set[str]:
    return set(nul_separated(git(root, "ls-files", "-z", "--others", "--exclude-standard")))


def changed_paths(root: Path, base: str) -> list[str]:
    """Paths changed by commits since BASE, plus local and untracked changes.

    Committed changes are kept even when the working tree restores their
    original contents, matching the conservative affected-path contract.
    Renames are reported as both their source and destination paths.
    """
    committed = git(root, "diff", "-z", "--name-only", "--no-renames", base, "HEAD", "--")
    local = git(root, "diff", "-z", "--name-only", "--no-renames", "HEAD", "--")
    return sorted(set(nul_separated(committed) + nul_separated(local)) | untracked_paths(root))


def package_roots(metadata: dict) -> dict[str, str]:
    """Workspace package directory (repository-relative) -> package name."""
    members = set(metadata["workspace_members"])
    workspace = Path(metadata["workspace_root"])
    roots = {}
    for package in metadata["packages"]:
        if package["id"] in members:
            directory = Path(package["manifest_path"]).parent.relative_to(workspace)
            roots[directory.as_posix()] = package["name"]
    return roots


def matches(path: str, pattern: str) -> bool:
    return path.startswith(pattern) if pattern.endswith("/") else path == pattern


def affected_selection(
    paths: list[str], roots: dict[str, str], untracked: set[str] | frozenset[str] = frozenset()
) -> Selection:
    """The tests that changed paths can affect; raises FullSuite.

    Untracked paths do not select the host-surface tests, which scan only
    tracked files.
    """
    selection = Selection()
    for path in paths:
        if "__pycache__" in PurePosixPath(path).parts:
            continue
        if path not in untracked:
            selection.filters.add(HOST_SURFACE_TESTS)
        owner = None
        for parent in PurePosixPath(path).parents:
            owner = roots.get(parent.as_posix())
            if owner is not None:
                break
        if owner is not None:
            selection.packages.add(owner)
            continue
        if any(matches(path, pattern) for pattern in FULL_SUITE_PATHS):
            raise FullSuite(path, "is a build or test-harness input")
        for pattern, packages, filters in OUTSIDE_PACKAGE_READERS:
            if matches(path, pattern):
                selection.packages.update(packages)
                selection.filters.update(filters)
                break
        else:
            raise FullSuite(path, "is outside every package and not listed in OUTSIDE_PACKAGE_READERS")
    return selection


def with_extra_consumers(packages: set[str]) -> set[str]:
    selected = set(packages)
    for package in packages:
        selected.update(EXTRA_CONSUMERS.get(package, ()))
    return selected


def scope_filterset(selection: Selection | None) -> str:
    """The selection as a nextest filterset; None selects everything."""
    if selection is None:
        return "all()"
    terms = [f"rdeps(={package})" for package in sorted(with_extra_consumers(selection.packages))]
    terms += [f"({test_filter})" for test_filter in sorted(selection.filters)]
    return " | ".join(terms) or "none()"


def nextest_command(selection: Selection | None, extra_args: list[str]) -> list[str]:
    """The nextest invocation; None selects the whole workspace suite."""
    command = ["cargo", "nextest", "run", "--workspace", *NEXTEST_ARGS]
    if selection is not None:
        command += ["-E", scope_filterset(selection)]
    return command + extra_args


def main(argv: list[str], root: Path = ROOT) -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--base", help="Git revision to compare against.")
    parser.add_argument("--print", action="store_true", help="Show the selection without running it.")
    parser.add_argument(
        "--filterset", action="store_true", help="Print only the selection as a nextest filterset."
    )
    parser.add_argument("nextest_args", nargs="*", help="Extra arguments for cargo nextest run (after --).")
    options = parser.parse_args(argv)

    base = options.base or default_base(root)
    metadata = json.loads(
        subprocess.run(
            ["cargo", "metadata", "--format-version", "1", "--no-deps"],
            cwd=root, check=True, capture_output=True, text=True,
        ).stdout
    )
    try:
        selection = affected_selection(
            changed_paths(root, base), package_roots(metadata), untracked_paths(root)
        )
    except FullSuite as reason:
        if options.filterset:
            print(scope_filterset(None))
            return 0
        print(f"Running the full suite: {reason.path} {reason.reason}.")
        command = nextest_command(None, options.nextest_args)
    else:
        if options.filterset:
            print(scope_filterset(selection))
            return 0
        if not selection.packages and not selection.filters:
            print(f"No test reads the files changed since {base[:12]}; nothing to test.")
            return 0
        if selection.packages:
            packages = ", ".join(sorted(selection.packages))
            print(f"Packages selected by changes since {base[:12]}, with their dependents: {packages}")
        command = nextest_command(selection, options.nextest_args)
    print("+ " + shlex.join(command))
    if options.print:
        return 0
    os.chdir(root)
    return subprocess.run(command).returncode


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
