#!/usr/bin/env python3
"""Reject Rust sources split by position instead of by responsibility.

Slices spliced back with `include!`, and files named for where a file was cut
(`part_NN.rs`, `tail.rs`, `*_parts/`), share one namespace and escape `cargo fmt`.
Split an oversized file into modules named for what they hold instead.
"""

import argparse
from pathlib import Path
import re
import sys

# Generated sources that a build script rewrites; splicing these in is intended.
GENERATED_INCLUDES = {"embedded_templates_snapshot.rs"}
INCLUDE = re.compile(r'\binclude!\s*\(\s*"([^"]+\.rs)"\s*\)')
POSITIONAL_NAME = re.compile(r"^(?:part_\d+[a-z0-9_]*|tail)\.rs$")
SKIPPED_DIRS = {"target", "node_modules"}


def rust_sources(root):
    for path in sorted((root / "crates").rglob("*.rs")):
        if not SKIPPED_DIRS.intersection(path.relative_to(root).parts):
            yield path


def find_problems(root):
    problems = []
    split_dirs = set()
    for path in rust_sources(root):
        relative = path.relative_to(root)
        if POSITIONAL_NAME.match(path.name):
            problems.append(f"{relative}: named by position; name the module for what it holds")
        split_dirs.update(parent for parent in relative.parents if parent.name.endswith("_parts"))
        text = path.read_text(encoding="utf-8", errors="replace")
        for number, line in enumerate(text.splitlines(), 1):
            for match in INCLUDE.finditer(line.split("//", 1)[0]):
                if Path(match.group(1)).name not in GENERATED_INCLUDES:
                    problems.append(
                        f'{relative}:{number}: include!("{match.group(1)}") splices a source '
                        "file; declare it as a module with `mod` instead"
                    )
    problems.extend(
        f"{directory}/: a `_parts` directory; name it after the module it belongs to"
        for directory in sorted(split_dirs)
    )
    return problems


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument(
        "--root", type=Path, default=Path(__file__).resolve().parent.parent,
        help="Repository root to check (default: this checkout)",
    )
    args = parser.parse_args()
    problems = find_problems(args.root.resolve())
    if problems:
        for problem in problems:
            print(problem, file=sys.stderr)
        print("Split oversized Rust files into modules named for what they hold.",
              file=sys.stderr)
        return 1
    print("Rust sources are modules, not spliced slices.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
