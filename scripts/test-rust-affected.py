#!/usr/bin/env python3
"""Run only the Rust tests that a change can affect.

Changed files are those that differ between BASE (by default the merge base
with origin/master) and HEAD, plus staged, unstaged, and untracked files. A
file inside a workspace package selects that package, every package that
depends on it through nextest's `rdeps()`, and any package listed as reading
its files. Paths that no test reads are ignored; any other change outside a
package runs the whole workspace suite.

Usage: scripts/test-rust-affected.py [--base REF] [--print] [-- NEXTEST_ARGS...]
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

# Notes and planning records that no test reads. Python bytecode caches are
# skipped wherever they appear.
IGNORED_PREFIXES = (
    ".beads/",
    ".agent/notes/",
    ".agent/plans/",
    "docs/plans/",
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

    def __init__(self, path: str) -> None:
        super().__init__(path)
        self.path = path


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


def changed_paths(root: Path, base: str) -> list[str]:
    """Paths changed by commits since BASE, plus local and untracked changes.

    Committed changes are kept even when the working tree restores their
    original contents, matching the conservative affected-path contract.
    Renames are reported as both their source and destination paths.
    """
    committed = git(root, "diff", "-z", "--name-only", "--no-renames", base, "HEAD", "--")
    local = git(root, "diff", "-z", "--name-only", "--no-renames", "HEAD", "--")
    untracked = git(root, "ls-files", "-z", "--others", "--exclude-standard")
    return sorted(set(nul_separated(committed) + nul_separated(local) + nul_separated(untracked)))


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


def affected_packages(paths: list[str], roots: dict[str, str]) -> set[str]:
    """Packages whose directories contain a changed path; raises FullSuite."""
    packages = set()
    for path in paths:
        if path.startswith(IGNORED_PREFIXES) or "__pycache__" in PurePosixPath(path).parts:
            continue
        owner = None
        for parent in PurePosixPath(path).parents:
            owner = roots.get(parent.as_posix())
            if owner is not None:
                break
        if owner is None:
            raise FullSuite(path)
        packages.add(owner)
    return packages


def with_extra_consumers(packages: set[str]) -> set[str]:
    selected = set(packages)
    for package in packages:
        selected.update(EXTRA_CONSUMERS.get(package, ()))
    return selected


def filterset(packages: set[str]) -> str:
    return " | ".join(f"rdeps(={package})" for package in sorted(packages))


def nextest_command(packages: set[str] | None, extra_args: list[str]) -> list[str]:
    """The nextest invocation; None selects the whole workspace suite."""
    command = ["cargo", "nextest", "run", "--workspace", *NEXTEST_ARGS]
    if packages is not None:
        command += ["-E", filterset(with_extra_consumers(packages))]
    return command + extra_args


def main(argv: list[str], root: Path = ROOT) -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--base", help="Git revision to compare against.")
    parser.add_argument("--print", action="store_true", help="Show the selection without running it.")
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
        packages = affected_packages(changed_paths(root, base), package_roots(metadata))
    except FullSuite as reason:
        print(f"Running the full suite: {reason.path} is outside any workspace package.")
        command = nextest_command(None, options.nextest_args)
    else:
        if not packages:
            print(f"No Rust package changed since {base[:12]}; nothing to test.")
            return 0
        print(f"Changed packages since {base[:12]}: {', '.join(sorted(packages))}")
        command = nextest_command(packages, options.nextest_args)
    print("+ " + shlex.join(command))
    if options.print:
        return 0
    os.chdir(root)
    return subprocess.run(command).returncode


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
