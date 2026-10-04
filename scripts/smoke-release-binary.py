#!/usr/bin/env python3
"""Exercise release packaging and the real installer with a local asset transport."""

import importlib.util
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile

ROOT = Path(__file__).resolve().parents[1]
SPEC = importlib.util.spec_from_file_location("package_binary", ROOT / "scripts/package-release-binary.py")
PACKAGE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(PACKAGE)


def smoke(binary, target):
    version = subprocess.check_output([str(binary), "--version"], text=True).strip().removeprefix("jig ")
    contract = json.loads((ROOT / ".agent/jig-contract.json").read_text())["contract_version"]
    for profile in ["default", "runtime"]:
        subprocess.run([str(binary), "__runtime-compatible", "--capability-only",
                        "--contract-version", str(contract), "--profile", profile, str(ROOT)], check=True)
    if version.endswith("-dev"):
        print("Development binary passed both profile probes; stable-release installation is tested on tagged builds.")
        return
    with tempfile.TemporaryDirectory(prefix="ExampleBinarySmoke-") as temporary:
        root = Path(temporary)
        for directory in ["scripts", ".agent", ".jig", "tools", "assets"]:
            (root / directory).mkdir()
        shutil.copy2(ROOT / "scripts/install-jig.sh", root / "scripts/install-jig.sh")
        (root / ".jig/runtime-version").write_text(version + "\n")
        (root / ".jig.toml").write_text('_src_path = "embedded:jig-sh"\n')
        (root / ".agent/jig-contract.json").write_text(json.dumps({"contract_version": contract}))
        PACKAGE.package(binary, version, target, root / "assets")
        # Restrict PATH to the installer's utilities: there is no Cargo or
        # ambient Jig available. Only transport is replaced; archive validation,
        # native execution, compatibility checks, and cache publication are real.
        for command in ["bash", "dirname", "awk", "mkdir", "mktemp", "rm", "mv", "chmod", "uname", "getconf"]:
            executable = shutil.which(command)
            if executable:
                (root / "tools" / command).symlink_to(executable)
        (root / "tools/python3").symlink_to(sys.executable)
        curl = root / "tools/curl"
        curl.write_text('''#!/usr/bin/env python3
import os, pathlib, shutil, sys
args = sys.argv[1:]
asset = pathlib.Path(os.environ["EXAMPLE_ASSETS"]) / args[-1].rsplit("/", 1)[-1]
shutil.copyfile(asset, args[args.index("--output") + 1])
print("200", end="")
''')
        curl.chmod(0o755)
        env = {key: value for key, value in os.environ.items() if not key.startswith("JIG_")}
        env.update(PATH=str(root / "tools"), EXAMPLE_ASSETS=str(root / "assets"))
        installer = [str(root / "scripts/install-jig.sh")]
        installed = subprocess.check_output(installer, env=env, text=True).strip()
        assert Path(installed).read_bytes() == binary.read_bytes()
        for profile in ["default", "runtime"]:
            cached = subprocess.check_output(installer + ["--resolve-only", "--profile", profile], env=env, text=True).strip()
            assert cached == installed
        print(f"Verified {version}/{target}: cold install and both cached profiles without Cargo")


if __name__ == "__main__":
    smoke(Path(sys.argv[1]).resolve(), sys.argv[2])
