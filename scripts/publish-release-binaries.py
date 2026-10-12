#!/usr/bin/env python3
"""Upload a complete binary matrix; never replace an existing release asset."""

import hashlib
import json
from pathlib import Path
import re
import subprocess
import sys
import tempfile

from importlib.util import module_from_spec, spec_from_file_location

SPEC = spec_from_file_location("package_binary", Path(__file__).with_name("package-release-binary.py"))
PACKAGE = module_from_spec(SPEC)
SPEC.loader.exec_module(PACKAGE)


def publish(version, directory):
    if not re.fullmatch(r"(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)", version):
        raise ValueError("expected an exact stable release version")
    assets = []
    for target in sorted(PACKAGE.TARGETS):
        archive = directory / f"jig-{version}-{target}.tar.gz"
        checksum = archive.with_suffix(archive.suffix + ".sha256")
        expected = f"{hashlib.sha256(archive.read_bytes()).hexdigest()}  {archive.name}\n"
        if checksum.read_text() != expected:
            raise ValueError(f"invalid checksum: {checksum}")
        assets.extend([archive, checksum])
    tag = f"v{version}"
    release = json.loads(subprocess.check_output(
        ["gh", "release", "view", tag, "--json", "assets"], text=True))
    existing = {asset["name"] for asset in release["assets"]}
    # Preserve immutable existing pairs even when a rebuild produces different
    # bytes. Verify their own checksums before skipping them on a resumed run.
    pending = []
    with tempfile.TemporaryDirectory(prefix="ExampleReleaseAssets-") as temporary:
        for archive, checksum in zip(assets[::2], assets[1::2]):
            present = {archive.name, checksum.name} & existing
            if len(present) == 1:
                raise ValueError(f"incomplete published pair: {sorted(present)}; "
                                 "repair or remove that partial pair before retrying")
            if present:
                subprocess.run(["gh", "release", "download", tag,
                                "--pattern", archive.name, "--pattern", checksum.name,
                                "--dir", temporary], check=True)
                downloaded = Path(temporary) / archive.name
                expected = f"{hashlib.sha256(downloaded.read_bytes()).hexdigest()}  {archive.name}\n"
                if (Path(temporary) / checksum.name).read_text() != expected:
                    raise ValueError(f"published checksum mismatch: {archive.name}")
            else:
                pending.extend([archive, checksum])
    if pending:
        subprocess.run(["gh", "release", "upload", tag, *map(str, pending)], check=True)


if __name__ == "__main__":
    publish(sys.argv[1], Path(sys.argv[2]))
