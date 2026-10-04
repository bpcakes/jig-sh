#!/usr/bin/env bash
# Standalone Jig installation; no repository checkout or Rust toolchain needed.
set -euo pipefail
if ! command -v python3 >/dev/null 2>&1; then
  echo "Jig's installer requires Python 3. Install python3 and retry." >&2
  exit 1
fi
python3 - "$@" <<'PY'
import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import re
import shutil
import subprocess
import sys
import tarfile
import tempfile
import urllib.error
import urllib.request

RELEASES = "https://github.com/bpcakes/jig-sh/releases/download"
LATEST = "https://api.github.com/repos/bpcakes/jig-sh/releases/latest"
VERSION_PATTERN = r"(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)"


def download(url, destination, limit):
    request = urllib.request.Request(url, headers={"User-Agent": "jig-sh-installer"})
    with urllib.request.urlopen(request, timeout=30) as response:
        if not response.geturl().startswith("https://"):
            raise ValueError("refusing a non-HTTPS release download")
        size = 0
        with destination.open("wb") as stream:
            while True:
                chunk = response.read(1024 * 1024)
                if not chunk:
                    break
                size += len(chunk)
                if size > limit:
                    raise ValueError("release download exceeds the size limit")
                stream.write(chunk)


def host_target():
    system, machine = platform.system(), platform.machine()
    arch = {"x86_64": "x86_64", "amd64": "x86_64", "arm64": "aarch64", "aarch64": "aarch64"}.get(machine)
    if not arch:
        raise ValueError(f"no prebuilt Jig for {system}/{machine}; install jig-sh with Cargo")
    if system == "Linux":
        try:
            libc = os.confstr("CS_GNU_LIBC_VERSION") or ""
            version = tuple(map(int, libc.split()[-1].split(".")))
        except (OSError, ValueError, IndexError):
            version = ()
        if version < (2, 35):
            raise ValueError("prebuilt Linux Jig requires glibc 2.35 or newer; install jig-sh with Cargo")
        return arch + "-unknown-linux-gnu"
    if system == "Darwin":
        version = tuple(map(int, platform.mac_ver()[0].split(".")))
        if version < (13,):
            raise ValueError("prebuilt Jig requires macOS 13 or newer; install jig-sh with Cargo")
        return arch + "-apple-darwin"
    raise ValueError(f"no prebuilt Jig for {system}/{machine}; install jig-sh with Cargo")


def install(version, bin_dir):
    target = host_target()
    if version is not None and not re.fullmatch(VERSION_PATTERN, version):
        raise ValueError("--version must be an exact stable version such as 0.7.0")
    bin_dir = bin_dir.expanduser().resolve()
    bin_dir.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix=".jig-install-", dir=bin_dir) as temporary:
        stage = Path(temporary)
        if version is None:
            metadata = stage / "release.json"
            download(LATEST, metadata, 1024 * 1024)
            tag = json.loads(metadata.read_text())["tag_name"]
            if not isinstance(tag, str) or not re.fullmatch("v" + VERSION_PATTERN, tag):
                raise ValueError("latest release does not identify an exact stable version")
            version = tag[1:]
        asset = f"jig-{version}-{target}.tar.gz"
        archive, checksum = stage / asset, stage / (asset + ".sha256")
        base = f"{RELEASES}/v{version}/{asset}"
        print(f"Downloading Jig {version} ({target})...", file=sys.stderr)
        download(base, archive, 128 * 1024 * 1024)
        download(base + ".sha256", checksum, 1024)
        expected = checksum.read_text(encoding="ascii")
        digest = hashlib.sha256(archive.read_bytes()).hexdigest()
        if expected != f"{digest}  {asset}\n":
            raise ValueError("release checksum is invalid or does not match the archive")
        binary = stage / "jig"
        with tarfile.open(archive, "r:gz") as stream:
            members = stream.getmembers()
            if len(members) != 1 or members[0].name != "jig" or not members[0].isreg():
                raise ValueError("release archive must contain one regular jig executable")
            if not 0 < members[0].size <= 512 * 1024 * 1024:
                raise ValueError("invalid release executable size")
            with stream.extractfile(members[0]) as source, binary.open("wb") as destination:
                shutil.copyfileobj(source, destination)
        with binary.open("rb") as stream:
            if stream.read(4) not in {b"\x7fELF", b"\xfe\xed\xfa\xce", b"\xce\xfa\xed\xfe",
                                       b"\xfe\xed\xfa\xcf", b"\xcf\xfa\xed\xfe"}:
                raise ValueError("release does not contain a native executable")
        binary.chmod(0o755)
        # Direct exec never interprets an invalid binary as a shell script.
        result = subprocess.check_output([str(binary), "--version"], text=True, timeout=15).strip()
        if result != f"jig {version}":
            raise ValueError(f"expected jig {version}, got {result!r}")
        installed = bin_dir / "jig"
        os.replace(binary, installed)
    print(f"Installed Jig {version} to {installed}")
    selected = shutil.which("jig")
    if selected is None or Path(selected).resolve() != installed:
        import shlex
        print("To use this installation, put its directory first on your shell's PATH:")
        print(f"  export PATH={shlex.quote(str(bin_dir))}:\"$PATH\"")
    print("Run jig --version to verify the installation.")
    return installed


def main():
    parser = argparse.ArgumentParser(description="Install a verified Jig release binary without Rust.")
    parser.add_argument("--version", help="exact stable release; defaults to the latest GitHub Release")
    parser.add_argument("--bin-dir", type=Path, default=Path.home() / ".local/bin", help="installation directory (default: ~/.local/bin)")
    args = parser.parse_args()
    try:
        install(args.version, args.bin_dir)
    except urllib.error.HTTPError as error:
        print(f"Jig installation failed: HTTP {error.code} downloading {error.url}. "
              "The requested release may not have binaries yet. No source build was attempted.", file=sys.stderr)
        return 1
    except (OSError, ValueError, KeyError, tarfile.TarError, subprocess.SubprocessError) as error:
        print(f"Jig installation failed: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
PY
