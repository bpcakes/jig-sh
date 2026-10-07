"""Exercise the guard against real Cargo feature resolution, without a registry."""

from pathlib import Path
import subprocess
import sys
import tempfile
import unittest


GUARD = Path(__file__).resolve().parents[1] / "check-vault-production-features.py"


class ProductionVaultFeatures(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="vault-consumer-fixture-")
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        (self.root / "Cargo.toml").write_text(
            '[workspace]\nresolver = "2"\nmembers = ["jig-sh", "jig-vault", "helper"]\n'
        )
        self.package("jig-vault", '[features]\ntest-utils = []\n')
        self.package("helper", "")

    def package(self, name, extra):
        directory = self.root / name
        (directory / "src").mkdir(parents=True, exist_ok=True)
        (directory / "src/lib.rs").write_text("")
        (directory / "Cargo.toml").write_text(
            f'[package]\nname = "{name}"\nversion = "0.0.0"\nedition = "2021"\n{extra}'
        )

    def check(self, dependencies, *args):
        self.package("jig-sh", dependencies)
        lock = subprocess.run(
            ["cargo", "generate-lockfile", "--offline"], cwd=self.root,
            capture_output=True, text=True,
        )
        self.assertEqual(lock.returncode, 0, lock.stderr)
        return subprocess.run(
            [sys.executable, str(GUARD), "--manifest-path",
             str(self.root / "Cargo.toml"), *args],
            cwd=self.root, capture_output=True, text=True,
        )

    def test_dev_only_feature_does_not_contaminate_production(self):
        result = self.check(
            '[dependencies]\njig-vault = { path = "../jig-vault" }\n'
            '[dev-dependencies]\n'
            'jig-vault = { path = "../jig-vault", features = ["test-utils"] }\n'
        )
        self.assertEqual(result.returncode, 0, result.stderr)

    def test_normal_dependency_feature_is_rejected(self):
        result = self.check(
            '[dependencies]\n'
            'jig-vault = { path = "../jig-vault", features = ["test-utils"] }\n'
        )
        self.assertEqual(result.returncode, 1)
        self.assertIn("Production jig-sh enables jig-vault/test-utils", result.stderr)

    def test_transitive_feature_is_rejected(self):
        self.package("helper", '[dependencies]\n'
                     'jig-vault = { path = "../jig-vault", features = ["test-utils"] }\n')
        result = self.check('[dependencies]\nhelper = { path = "../helper" }\n')
        self.assertEqual(result.returncode, 1, result.stdout + result.stderr)

    def test_target_specific_feature_matches_requested_release_target(self):
        dependencies = (
            '[dependencies]\njig-vault = { path = "../jig-vault" }\n'
            '[target.\'cfg(target_os = "linux")\'.dependencies]\n'
            'jig-vault = { path = "../jig-vault", features = ["test-utils"] }\n'
        )
        linux = self.check(dependencies, "--target", "x86_64-unknown-linux-gnu")
        self.assertEqual(linux.returncode, 1, linux.stdout + linux.stderr)
        mac = self.check(dependencies, "--target", "aarch64-apple-darwin")
        self.assertEqual(mac.returncode, 0, mac.stderr)

    def test_historical_release_without_vault_is_supported(self):
        result = self.check("")
        self.assertEqual(result.returncode, 0, result.stderr)

    def test_cargo_failure_is_not_reported_as_success(self):
        result = self.check("", "--target", "nonexistent-example-target")
        self.assertNotEqual(result.returncode, 0)
        self.assertNotIn("dependencies exclude", result.stdout)


if __name__ == "__main__":
    unittest.main()
