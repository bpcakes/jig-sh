#!/usr/bin/env python3
"""Reject vault test helpers in the production Jig dependency graph."""

import argparse
from pathlib import Path
import subprocess
import sys


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--manifest-path", type=Path,
        default=Path(__file__).resolve().parent.parent / "Cargo.toml",
    )
    parser.add_argument("--target", help="Match the release build's Cargo target")
    args = parser.parse_args()
    manifest = args.manifest_path.resolve()
    command = [
        "cargo", "tree", "--locked", "--manifest-path", str(manifest),
        "-p", "jig-sh", "--edges", "normal,build", "--prefix", "none",
        "--format", "{p}|{f}", "--color", "never",
    ]
    if args.target:
        command.extend(["--target", args.target])
    result = subprocess.run(
        command, cwd=manifest.parent, stdout=subprocess.PIPE, text=True, check=False,
    )
    if result.returncode:
        return result.returncode if result.returncode > 0 else 128 - result.returncode
    for line in result.stdout.splitlines():
        package, _, features = line.rpartition("|")
        if package.startswith("jig-vault "):
            features = features.removesuffix(" (*)").strip().split(",")
            if "test-utils" in features:
                print("Production jig-sh enables jig-vault/test-utils; keep vault "
                      "test helpers in dev-dependencies only.", file=sys.stderr)
                return 1
    # Historical release sources may predate jig-vault entirely.
    print("Production Jig dependencies exclude jig-vault/test-utils.")
    return 0


if __name__ == "__main__":
    try:
        sys.exit(main())
    except OSError as error:
        print(f"Cannot check production vault features: {error}", file=sys.stderr)
        sys.exit(1)
