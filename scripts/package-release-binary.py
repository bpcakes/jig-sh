#!/usr/bin/env python3
"""Package a native Jig build into the installer's versioned release format."""

import argparse
import hashlib
from pathlib import Path
import re
import subprocess
import tarfile

TARGETS = {
    "x86_64-unknown-linux-gnu", "aarch64-unknown-linux-gnu",
    "x86_64-apple-darwin", "aarch64-apple-darwin",
}


def package(binary, version, target, output):
    if not re.fullmatch(r"(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)(-dev)?", version):
        raise ValueError("expected an exact Jig version")
    if target not in TARGETS:
        raise ValueError(f"unsupported release target: {target}")
    actual = subprocess.check_output([str(binary.resolve()), "--version"], text=True).strip()
    if actual != f"jig {version}":
        raise ValueError(f"expected jig {version}, got {actual}")
    output.mkdir(parents=True, exist_ok=True)
    archive = output / f"jig-{version}-{target}.tar.gz"
    with tarfile.open(archive, "w:gz") as stream:
        info = stream.gettarinfo(str(binary), arcname="jig")
        info.mode = 0o755
        info.uid = info.gid = info.mtime = 0
        info.uname = info.gname = ""
        with binary.open("rb") as source:
            stream.addfile(info, source)
    digest = hashlib.sha256(archive.read_bytes()).hexdigest()
    archive.with_suffix(archive.suffix + ".sha256").write_text(f"{digest}  {archive.name}\n")
    return archive


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("binary", type=Path)
    parser.add_argument("version")
    parser.add_argument("target", choices=sorted(TARGETS))
    parser.add_argument("output", type=Path)
    args = parser.parse_args()
    print(package(args.binary, args.version, args.target, args.output))


if __name__ == "__main__":
    main()
