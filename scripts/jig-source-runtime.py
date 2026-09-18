#!/usr/bin/env python3
"""Select the source repository's pinned, already-built Jig runtime."""

import argparse
import fcntl
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tempfile
import time


def native_binary(path):
    # Never let PATH resolve to scripts/jig, another shell wrapper, or an
    # ENOEXEC text file. subprocess uses direct exec, with no shell fallback.
    with path.open("rb") as stream:
        header = stream.read(8)
    magic = header[:4]
    if magic in {b"\x7fELF", b"\xfe\xed\xfa\xce", b"\xce\xfa\xed\xfe",
                 b"\xfe\xed\xfa\xcf", b"\xcf\xfa\xed\xfe"}:
        return True
    byte_order = {b"\xca\xfe\xba\xbe": "big", b"\xca\xfe\xba\xbf": "big",
                  b"\xbe\xba\xfe\xca": "little", b"\xbf\xba\xfe\xca": "little"}.get(magic)
    return byte_order is not None and 1 <= int.from_bytes(header[4:8], byte_order) <= 16


def probe(path, root, contract, profile, version=None, require_native=True):
    if not path.is_file() or not os.access(path, os.X_OK):
        raise ValueError(f"Jig binary is missing or not executable: {path}")
    if require_native and not native_binary(path):
        raise ValueError(f"Expected a native Jig executable, found a wrapper: {path}")
    output = subprocess.run(
        [str(path), "--version"], check=True, stdin=subprocess.DEVNULL,
        capture_output=True, text=True, timeout=10,
    ).stdout.strip()
    if not output.startswith("jig "):
        raise ValueError(f"Invalid Jig version response from {path}")
    actual = output[len("jig "):]
    if version is not None and actual != version:
        raise ValueError(f"Jig {actual} at {path} does not match pinned release {version}")
    compatible = subprocess.run(
        [str(path), "__runtime-compatible", "--capability-only",
         "--contract-version", str(contract), "--profile", profile, str(root)],
        stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
        timeout=10,
    )
    if compatible.returncode:
        raise ValueError(f"Jig {actual} cannot run contract {contract} with profile {profile}")
    return actual


def cached_runtime(binary, root, contract, profile, version):
    try:
        probe(binary, root, contract, profile, version)
        return binary
    except (OSError, ValueError, subprocess.SubprocessError):
        return None


def acquire_lock(stream):
    deadline = time.monotonic() + 30
    while True:
        try:
            fcntl.flock(stream, fcntl.LOCK_EX | fcntl.LOCK_NB)
            return
        except BlockingIOError:
            if time.monotonic() >= deadline:
                raise ValueError("Timed out waiting for the source runtime cache lock")
            time.sleep(0.1)


def select_runtime(root, contract, profile, version, resolve_only, refresh):
    base = root / ".git/jig-tools" if (root / ".git").is_dir() else root / ".agent/.cache/jig"
    cache = base / f"source-release-{version}"
    binary = cache / "bin/jig"
    if not refresh and cached_runtime(binary, root, contract, profile, version):
        return binary
    if resolve_only:
        raise ValueError("No cached pinned runtime; run scripts/jig --version once to prepare it")
    cache.mkdir(parents=True, exist_ok=True)
    with (cache / "import.lock").open("a") as lock:
        acquire_lock(lock)
        if not refresh and cached_runtime(binary, root, contract, profile, version):
            return binary
        candidate = shutil.which("jig")
        if candidate is None:
            raise ValueError(f"Pinned Jig {version} is not installed on PATH")
        candidate = Path(candidate).resolve()
        # A full binary can serve every profile. Reject stripped imports so a
        # later dev/proxy command does not unexpectedly require another import.
        probe(candidate, root, contract, "default", version)
        if candidate == binary.resolve():
            raise ValueError("Refresh requires an installed binary outside the source runtime cache")
        binary.parent.mkdir(parents=True, exist_ok=True)
        temporary = None
        try:
            with tempfile.NamedTemporaryFile(dir=binary.parent, prefix=".jig-", delete=False) as stream:
                temporary = Path(stream.name)
            shutil.copyfile(candidate, temporary)
            temporary.chmod(0o755)
            # Validate the copy, not just its source: never publish a partial
            # file or a replacement observed while an installation is changing.
            probe(temporary, root, contract, "default", version)
            os.replace(temporary, binary)
        finally:
            if temporary is not None:
                temporary.unlink(missing_ok=True)
        print(f"Using pinned Jig {version}; cached installed binary at {binary}", file=sys.stderr)
        return binary


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=Path(__file__).resolve().parent.parent)
    parser.add_argument("--contract-version", type=int)
    parser.add_argument("--profile", choices=["runtime", "default", "mcp"], default="runtime")
    parser.add_argument("--resolve-only", choices=["0", "1"], default="0")
    parser.add_argument("--refresh", choices=["0", "1"], default="0")
    parser.add_argument("--info", action="store_true", help="Report the selected runtime as JSON")
    args = parser.parse_args()
    version = None
    try:
        root = args.root.resolve()
        version = (root / ".jig/source-runtime-version").read_text().strip()
        if not re.fullmatch(r"[0-9]+\.[0-9]+\.[0-9]+", version):
            raise ValueError(".jig/source-runtime-version must contain one stable release version")
        contract = args.contract_version
        if contract is None:
            contract = json.loads((root / ".agent/jig-contract.json").read_text())["contract_version"]
        if type(contract) is not int or contract < 0:
            raise ValueError("Contract version must be a non-negative integer")
        override = os.environ.get("JIG_DEV_BIN")
        if override:
            binary = Path(override).resolve()
            actual = probe(binary, root, contract, args.profile, require_native=False)
            mode = "development_override"
        else:
            binary = select_runtime(root, contract, args.profile, version,
                                    args.resolve_only == "1" or args.info or args.profile == "mcp",
                                    args.refresh == "1")
            actual = version
            mode = "released"
        if args.info:
            print(json.dumps({"mode": mode, "release_pin": version, "runtime_version": actual,
                              "binary": str(binary), "profile": args.profile,
                              "contract_version": contract}, indent=2))
        else:
            print(binary)
        return 0
    except (OSError, ValueError, KeyError, subprocess.SubprocessError) as error:
        if args.resolve_only == "1" and not args.info:
            return 1
        print(f"Cannot select the source repository's Jig runtime: {error}", file=sys.stderr)
        if version and re.fullmatch(r"[0-9]+\.[0-9]+\.[0-9]+", version):
            print(f"Install the selected release with: cargo install jig-sh --version ={version} --locked",
                  file=sys.stderr)
        print("For current source, use scripts/jig-dev <command>. "
              "If the repository contract changed, deliberately update the release pin or select JIG_DEV_BIN.",
              file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main())
