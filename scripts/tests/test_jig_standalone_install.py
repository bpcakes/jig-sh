"""First-time installation needs no checkout, Cargo, or existing Jig runtime."""

import contextlib
import hashlib
import io
from pathlib import Path
import shutil
import subprocess
import tarfile
import tempfile
import types
import unittest
from unittest.mock import patch
import urllib.error

if __package__:
    from . import test_jig_release_runtime as runtime
    from . import test_jig_binary_distribution as distribution
else:
    import test_jig_release_runtime as runtime
    import test_jig_binary_distribution as distribution

REPO = Path(__file__).resolve().parents[2]
SCRIPT = REPO / "scripts/install.sh"
CODE = SCRIPT.read_text().split("<<'PY'\n", 1)[1].rsplit("\nPY\n", 1)[0]


def installer_module():
    module = types.ModuleType("standalone_installer")
    exec(compile(CODE, str(SCRIPT), "exec"), module.__dict__)
    return module


class StandaloneInstallTests(unittest.TestCase):
    setUpClass = classmethod(runtime.ReleaseRuntimeTests.setUpClass.__func__)

    def setUp(self):
        self.installer = installer_module()
        temporary = tempfile.TemporaryDirectory(prefix="ExampleStandaloneInstall-")
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        self.bin_dir = self.root / "example user's bin"
        self.assets = self.root / "assets"
        self.target = "x86_64-unknown-linux-gnu"
        self.archive = distribution.PACKAGE.package(self.binaries / "0.5.0", "0.5.0", self.target, self.assets)
        self.urls = []
        self.output = io.StringIO()
        self.patch("host_target", return_value=self.target)
        self.patch("download", side_effect=self.download)

    def patch(self, name, **kwargs):
        patcher = patch.object(self.installer, name, **kwargs)
        result = patcher.start()
        self.addCleanup(patcher.stop)
        return result

    def download(self, url, destination, limit):
        self.urls.append(url)
        if url == self.installer.LATEST:
            destination.write_text('{"tag_name":"v0.5.0"}')
        else:
            shutil.copyfile(self.assets / url.rsplit("/", 1)[1], destination)

    def install(self, version="0.5.0"):
        with contextlib.redirect_stdout(self.output), contextlib.redirect_stderr(self.output):
            return self.installer.install(version, self.bin_dir)

    def checksum(self):
        self.archive.with_suffix(".gz.sha256").write_text(
            f"{hashlib.sha256(self.archive.read_bytes()).hexdigest()}  {self.archive.name}\n")

    def test_exact_release_installs_verified_native_binary_and_prints_path(self):
        # macOS temporary directories can be reached through /var -> /private/var.
        # Exercise the same aliasing on every host; installation resolves it.
        linked_root = self.root / "linked-root"
        linked_root.symlink_to(self.root, target_is_directory=True)
        self.bin_dir = linked_root / self.bin_dir.name
        installed = self.install()
        self.assertEqual(installed, (self.bin_dir / "jig").resolve())
        self.assertEqual(installed.read_bytes(), (self.binaries / "0.5.0").read_bytes())
        self.assertEqual(subprocess.check_output([str(installed), "--version"], text=True), "jig 0.5.0\n")
        self.assertEqual(len(self.urls), 2)
        self.assertNotIn(self.installer.LATEST, self.urls)
        self.assertIn("export PATH=", self.output.getvalue())
        self.assertFalse(list(self.bin_dir.glob(".jig-install-*")))

    def test_existing_jig_earlier_on_path_still_gets_prepend_guidance(self):
        old_bin = self.root / "old-bin"
        old_bin.mkdir()
        shutil.copy2(self.binaries / "0.5.1", old_bin / "jig")
        with patch.dict(self.installer.os.environ, {"PATH": f"{old_bin}:{self.bin_dir}"}):
            self.install()
        self.assertIn("put its directory first", self.output.getvalue())

    def test_latest_resolves_once_then_uses_exact_versioned_assets(self):
        self.install(None)
        self.assertEqual(self.urls[0], self.installer.LATEST)
        self.assertEqual(len(self.urls), 3)
        self.assertTrue(all("/v0.5.0/" in url for url in self.urls[1:]))

    def test_bad_checksum_leaves_previous_installation_intact(self):
        installed = self.install()
        original = installed.read_bytes()
        self.archive.write_bytes(b"corrupt archive")
        with self.assertRaisesRegex(ValueError, "checksum"):
            self.install()
        self.assertEqual(installed.read_bytes(), original)
        self.assertFalse(list(self.bin_dir.glob(".jig-install-*")))

    def test_wrong_version_leaves_previous_installation_intact(self):
        installed = self.install()
        original = installed.read_bytes()
        with tarfile.open(self.archive, "w:gz") as archive:
            archive.add(self.binaries / "0.5.1", arcname="jig")
        self.checksum()
        with self.assertRaisesRegex(ValueError, "expected jig 0.5.0"):
            self.install()
        self.assertEqual(installed.read_bytes(), original)

    def test_symlink_and_script_archives_are_rejected(self):
        for script in [False, True]:
            with self.subTest(script=script):
                with tarfile.open(self.archive, "w:gz") as archive:
                    entry = tarfile.TarInfo("jig")
                    data = b"#!/bin/sh\necho 'jig 0.5.0'\n"
                    entry.mode = 0o755
                    entry.type = tarfile.REGTYPE if script else tarfile.SYMTYPE
                    entry.linkname = "../escaped" if not script else ""
                    entry.size = len(data) if script else 0
                    archive.addfile(entry, io.BytesIO(data))
                self.checksum()
                with self.assertRaises(ValueError):
                    self.install()
                self.assertFalse((self.bin_dir / "jig").exists())

    def test_invalid_version_does_not_download_or_install(self):
        for version in ["latest", "v0.5.0", "0.5.0-dev", "../0.5.0"]:
            with self.assertRaisesRegex(ValueError, "exact stable"):
                self.install(version)
        self.assertFalse(self.urls)
        self.assertFalse(self.bin_dir.exists())

    def test_missing_release_reports_error_without_source_fallback(self):
        self.patch("download", side_effect=urllib.error.HTTPError("https://example.invalid/asset", 404, "missing", {}, None))
        with patch.object(self.installer.sys, "argv", ["install", "--version", "0.5.0", "--bin-dir", str(self.bin_dir)]), contextlib.redirect_stderr(self.output):
            self.assertEqual(self.installer.main(), 1)
        self.assertIn("No source build was attempted", self.output.getvalue())
        self.assertFalse((self.bin_dir / "jig").exists())

    def test_real_shell_entrypoint_help_needs_no_repository(self):
        result = subprocess.run(["bash", str(SCRIPT), "--help"], cwd=self.root, text=True, capture_output=True)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("--bin-dir", result.stdout)
        self.assertIn("--version", result.stdout)


class StandaloneHostTests(unittest.TestCase):
    def test_host_matrix_and_unsupported_systems(self):
        installer = installer_module()
        for system, arch, expected in [
            ("Linux", "x86_64", "x86_64-unknown-linux-gnu"),
            ("Linux", "aarch64", "aarch64-unknown-linux-gnu"),
            ("Darwin", "arm64", "aarch64-apple-darwin"),
            ("Darwin", "x86_64", "x86_64-apple-darwin"),
            ("Linux", "riscv64", None), ("FreeBSD", "x86_64", None),
        ]:
            with self.subTest(system=system, arch=arch), patch.object(installer.platform, "system", return_value=system), patch.object(installer.platform, "machine", return_value=arch), patch.object(installer.platform, "mac_ver", return_value=("13.0", (), "")), patch.object(installer.os, "confstr", return_value="glibc 2.35"):
                if expected:
                    self.assertEqual(installer.host_target(), expected)
                else:
                    with self.assertRaises(ValueError):
                        installer.host_target()

    def test_old_or_non_glibc_linux_is_rejected(self):
        installer = installer_module()
        with patch.object(installer.platform, "system", return_value="Linux"), patch.object(installer.platform, "machine", return_value="x86_64"):
            for version in ["glibc 2.31", None]:
                with patch.object(installer.os, "confstr", return_value=version), self.assertRaisesRegex(ValueError, "glibc 2.35"):
                    installer.host_target()


if __name__ == "__main__":
    unittest.main()
