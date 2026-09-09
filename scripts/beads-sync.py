#!/usr/bin/env python3
"""Export Beads without machine-local source paths, or check an existing export."""

import argparse
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile


def export_path(root):
    metadata = json.loads((root / ".beads/metadata.json").read_text(encoding="utf-8"))
    relative = Path(metadata["jsonl_export"])
    if relative.is_absolute() or ".." in relative.parts:
        raise ValueError("Beads export must be inside .beads")
    return root / ".beads" / relative


def check_export(root):
    export = export_path(root)
    rejected = []
    for number, line in enumerate(export.read_text(encoding="utf-8").splitlines(), 1):
        if not line.strip():
            continue
        record = json.loads(line)
        if record.get("source_repo_path") not in (None, ""):
            rejected.append(str(record.get("id", f"record {number}")))
    if rejected:
        raise ValueError("source_repo_path must be empty: " + ", ".join(rejected))


def clear_tombstone_export_paths(root):
    # br refuses updates to tombstones, including metadata-only updates. Keep
    # their database records intact and remove only this field from exports.
    export = export_path(root)
    lines = export.read_text(encoding="utf-8").splitlines(keepends=True)
    cleared = 0
    for index, line in enumerate(lines):
        if not line.strip():
            continue
        record = json.loads(line)
        if record.get("status") == "tombstone" and record.get("source_repo_path"):
            del record["source_repo_path"]
            lines[index] = json.dumps(record, ensure_ascii=False, separators=(",", ":")) + "\n"
            cleared += 1
    if cleared:
        temporary = None
        try:
            with tempfile.NamedTemporaryFile(mode="w", encoding="utf-8", dir=export.parent, delete=False) as output:
                temporary = Path(output.name)
                output.writelines(lines)
            temporary.chmod(export.stat().st_mode & 0o777)
            os.replace(temporary, export)
        finally:
            if temporary is not None:
                temporary.unlink(missing_ok=True)
    return cleared


def br(root, *args):
    return subprocess.run(
        ["br", *args], cwd=root, check=True, capture_output=True, text=True
    ).stdout


def issue_records(root, status):
    response = json.loads(br(
        root, "list", "--status", status, "--deferred", "--limit", "0",
        "--json", "--no-auto-flush",
    ))
    if isinstance(response, dict):
        if response.get("has_more"):
            raise ValueError("Beads returned an incomplete issue list; export was not attempted")
        return response["issues"]
    return response


def sync(root):
    records = issue_records(root, "all")
    ids = sorted({record["id"] for record in records if record.get("source_repo_path")})
    for issue_id in ids:
        br(root, "update", "--source-repo-path", "", "--no-auto-flush",
           "--json", "--", issue_id)
    br(root, "sync", "--flush-only")
    tombstones = clear_tombstone_export_paths(root)
    check_export(root)
    return len(ids) + tombstones


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true", help="Check only; no br dependency or writes")
    parser.add_argument("--root", type=Path, default=Path(__file__).resolve().parent.parent)
    args = parser.parse_args()
    try:
        if args.check:
            check_export(args.root)
        else:
            cleared = sync(args.root)
            print(f"Cleared machine-local source metadata from {cleared} issue(s).")
    except subprocess.CalledProcessError as error:
        print(f"Beads sync failed: br {error.cmd[1]} exited {error.returncode}.", file=sys.stderr)
        return 1
    except (OSError, ValueError, KeyError, TypeError, AttributeError) as error:
        print(f"Beads export check failed: {error}", file=sys.stderr)
        return 1
    print("Beads export privacy check passed.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
