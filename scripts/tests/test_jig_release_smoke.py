"""Exercise backfill smoke checks with native release fixtures."""

import json
from pathlib import Path
import platform
import subprocess
import sys
import tempfile
import unittest

from test_jig_source_runtime import NATIVE_FIXTURE


ROOT = Path(__file__).resolve().parents[2]


class ReleaseSmokeTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory(prefix="ExampleReleaseSmoke-")
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        self.source = self.root / "release-source"
        (self.source / ".agent").mkdir(parents=True)
        self.contract = self.source / ".agent/jig-contract.json"
        self.contract.write_text(json.dumps({"contract_version": 7}))
        machine = {"arm64": "aarch64"}.get(platform.machine(), platform.machine())
        suffix = "apple-darwin" if sys.platform == "darwin" else "unknown-linux-gnu"
        self.target = f"{machine}-{suffix}"

    def run_smoke(self, version="0.2.0", contract="7"):
        source = self.root / "example.c"
        source.write_text(NATIVE_FIXTURE)
        binary = self.root / "jig"
        subprocess.run(["cc", str(source), "-o", str(binary),
                        f'-DVERSION="{version}"', f'-DCONTRACT="{contract}"'],
                       check=True, capture_output=True)
        return subprocess.run(
            [sys.executable, "-B", str(ROOT / "scripts/smoke-release-binary.py"),
             str(binary), self.target, "--release-source", str(self.source)],
            text=True, capture_output=True, timeout=30,
        )

    def test_older_release_uses_its_own_contract_for_all_install_checks(self):
        result = self.run_smoke()
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertIn("standalone install, repo cold install, and both cached profiles", result.stdout)

    def test_mismatched_release_contract_fails(self):
        self.contract.write_text(json.dumps({"contract_version": 8}))
        result = self.run_smoke()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("__runtime-compatible", result.stderr)

    def test_original_release_without_probe_checks_standalone_install(self):
        self.contract.unlink()
        result = self.run_smoke(version="0.1.0", contract="")
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertIn("standalone install (v0.1.0 predates repository compatibility probes)", result.stdout)

    def test_newer_release_without_working_probe_still_fails(self):
        result = self.run_smoke(contract="")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("__runtime-compatible", result.stderr)


if __name__ == "__main__":
    unittest.main()
