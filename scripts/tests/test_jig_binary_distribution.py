"""Cold release installation uses verified native assets, without running Cargo."""

import hashlib
import importlib.util
import io
import json
from pathlib import Path
import tarfile
import unittest

if __package__:
    from . import test_jig_release_runtime as runtime
else:
    import test_jig_release_runtime as runtime

REPO = Path(__file__).resolve().parents[2]
SPEC = importlib.util.spec_from_file_location("package_binary", REPO / "scripts/package-release-binary.py")
PACKAGE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(PACKAGE)


class BinaryDistributionTests(unittest.TestCase):
    setUpClass = classmethod(runtime.ReleaseRuntimeTests.setUpClass.__func__)
    installer = runtime.ReleaseRuntimeTests.installer
    launcher = runtime.ReleaseRuntimeTests.launcher
    calls = runtime.ReleaseRuntimeTests.calls
    assert_ok = runtime.ReleaseRuntimeTests.assert_ok

    def setUp(self):
        runtime.ReleaseRuntimeTests.setUp(self)
        self.env.pop("JIG_INSTALL_SOURCE")
        self.downloads = self.root / "downloads"
        self.downloads.mkdir()
        self.target = "x86_64-unknown-linux-gnu"
        self.asset = PACKAGE.package(self.binaries / "0.5.0", "0.5.0", self.target, self.downloads)
        self.download_log = self.root / "downloads.jsonl"
        self.env.update(EXAMPLE_DOWNLOADS=str(self.downloads), EXAMPLE_DOWNLOAD_LOG=str(self.download_log))
        curl = self.tools / "curl"
        curl.write_text('''#!/usr/bin/env python3
import json, os, pathlib, shutil, sys
args = sys.argv[1:]
with open(os.environ["EXAMPLE_DOWNLOAD_LOG"], "a") as log:
    log.write(json.dumps(args) + "\\n")
if os.environ.get("EXAMPLE_CURL_EXIT"):
    sys.exit(int(os.environ["EXAMPLE_CURL_EXIT"]))
url = args[-1]
assert url.startswith("https://github.com/bpcakes/jig-sh/releases/download/v0.5.0/")
source = pathlib.Path(os.environ["EXAMPLE_DOWNLOADS"]) / url.rsplit("/", 1)[1]
status = os.environ.get("EXAMPLE_HTTP_STATUS", "200" if source.exists() else "404")
if status == "200":
    shutil.copyfile(source, args[args.index("--output") + 1])
print(status, end="")
''')
        curl.chmod(0o755)
        for name, body in [("uname", 'if [[ "$1" == -s ]]; then echo "${EXAMPLE_OS:-Linux}"; else echo "${EXAMPLE_ARCH:-x86_64}"; fi'),
                           ("getconf", 'echo "${EXAMPLE_LIBC:-glibc 2.35}"')]:
            tool = self.tools / name
            tool.write_text("#!/bin/bash\n" + body + "\n")
            tool.chmod(0o755)

    def downloaded(self):
        return [json.loads(line)[-1] for line in self.download_log.read_text().splitlines()] if self.download_log.exists() else []

    def checksum(self):
        self.asset.with_suffix(".gz.sha256").write_text(
            f"{hashlib.sha256(self.asset.read_bytes()).hexdigest()}  {self.asset.name}\n")

    def test_cold_launcher_download_and_warm_cache_never_invoke_cargo(self):
        (self.tools / "cargo").unlink()
        result = self.launcher("--version")
        self.assert_ok(result)
        self.assertEqual(result.stdout, "jig 0.5.0\n")
        self.assertEqual(len(self.downloaded()), 2)
        self.assert_ok(self.launcher("--version"))
        self.assertEqual(len(self.downloaded()), 2)
        self.assertFalse(self.calls())

    def test_full_binary_can_serve_runtime_and_explicit_install_root(self):
        full = self.installer("--profile", "default")
        self.assert_ok(full)
        self.assertEqual(self.installer("--profile", "runtime").stdout, full.stdout)
        self.assert_ok(self.installer(str(self.root / "explicit cache")))
        self.assertTrue((self.root / "explicit cache/bin/jig").exists())
        self.assertFalse(self.calls())

    def test_missing_archive_falls_back_to_exact_source_release(self):
        self.asset.unlink()
        self.assert_ok(self.installer())
        self.assertEqual(self.calls()[0][4:6], ["--version", "=0.5.0"])

    def test_source_opt_in_and_unsupported_hosts_do_not_download(self):
        for extra in [dict(JIG_INSTALL_SOURCE="1"), dict(EXAMPLE_ARCH="riscv64"),
                      dict(EXAMPLE_LIBC="glibc 2.31")]:
            with self.subTest(extra=extra):
                self.assert_ok(self.installer("--refresh", env=dict(self.env, **extra)))
        self.assertEqual(len(self.calls()), 3)
        self.assertFalse(self.downloaded())

    def test_resolve_only_never_downloads(self):
        self.assertNotEqual(self.installer("--resolve-only").returncode, 0)
        self.assertFalse(self.downloaded())
        self.assertFalse(self.calls())

    def test_transport_and_http_errors_do_not_compile(self):
        for extra in [dict(EXAMPLE_CURL_EXIT="28"), dict(EXAMPLE_HTTP_STATUS="403"),
                      dict(EXAMPLE_HTTP_STATUS="500")]:
            with self.subTest(extra=extra):
                self.assertNotEqual(self.installer(env=dict(self.env, **extra)).returncode, 0)
        self.assertFalse(self.calls())

    def test_missing_or_bad_checksum_fails_without_publishing_or_compiling(self):
        checksum = self.asset.with_suffix(".gz.sha256")
        for content in [None, "0" * 64 + "  " + self.asset.name + "\n", "not a checksum"]:
            if content is None:
                checksum.unlink()
            else:
                checksum.write_text(content)
            self.assertNotEqual(self.installer().returncode, 0)
            self.assertFalse(list(self.root.glob(".git/jig-tools/**/bin/jig")))
        self.assertFalse(self.calls())

    def test_wrong_version_and_contract_are_rejected(self):
        for binary in ["0.5.1", "incompatible"]:
            with tarfile.open(self.asset, "w:gz") as archive:
                archive.add(self.binaries / binary, arcname="jig")
            self.checksum()
            self.assertNotEqual(self.installer().returncode, 0)
        self.assertFalse(self.calls())

    def test_unsafe_archive_entries_are_rejected(self):
        for name, kind in [("../escaped", tarfile.REGTYPE), ("jig", tarfile.SYMTYPE)]:
            with tarfile.open(self.asset, "w:gz") as archive:
                entry = tarfile.TarInfo(name)
                entry.type = kind
                entry.linkname = "../escaped" if kind == tarfile.SYMTYPE else ""
                archive.addfile(entry, io.BytesIO())
            self.checksum()
            self.assertNotEqual(self.installer().returncode, 0)
        self.assertFalse((self.root / "escaped").exists())
        self.assertFalse(self.calls())

    def test_failed_refresh_preserves_previous_binary(self):
        result = self.installer()
        self.assert_ok(result)
        binary = Path(result.stdout.strip())
        original = binary.read_bytes()
        self.asset.write_bytes(b"corrupt")
        self.assertNotEqual(self.installer("--refresh").returncode, 0)
        self.assertEqual(binary.read_bytes(), original)
        self.assert_ok(self.installer("--resolve-only"))
        self.assertFalse(list(binary.parent.parent.glob(".jig-download.*")))

    def test_target_selection_matches_release_asset_names(self):
        for system, arch, target in [("Linux", "aarch64", "aarch64-unknown-linux-gnu"),
                                     ("Darwin", "arm64", "aarch64-apple-darwin"),
                                     ("Darwin", "x86_64", "x86_64-apple-darwin")]:
            PACKAGE.package(self.binaries / "0.5.0", "0.5.0", target, self.downloads)
            self.assert_ok(self.installer("--refresh", env=dict(self.env, EXAMPLE_OS=system, EXAMPLE_ARCH=arch)))
            self.assertIn(target, self.downloaded()[-1])
        self.assertFalse(self.calls())


class PublicationTests(unittest.TestCase):
    def setUp(self):
        import tempfile
        from unittest.mock import patch
        spec = importlib.util.spec_from_file_location("publish_binary", REPO / "scripts/publish-release-binaries.py")
        self.publisher = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(self.publisher)
        temporary = tempfile.TemporaryDirectory(prefix="ExampleBinaryPublication-")
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        self.assets = {}
        for target in PACKAGE.TARGETS:
            name = f"jig-0.5.0-{target}.tar.gz"
            data = target.encode()
            self.assets[name] = data
            self.assets[name + ".sha256"] = f"{hashlib.sha256(data).hexdigest()}  {name}\n".encode()
        for name, data in self.assets.items():
            (self.root / name).write_bytes(data)
        self.existing = {}
        self.uploads = []

        def view(*args, **kwargs):
            return json.dumps({"assets": [{"name": name} for name in self.existing]})

        def run(args, **kwargs):
            if args[:3] == ["gh", "release", "download"]:
                dest = Path(args[args.index("--dir") + 1])
                for index, arg in enumerate(args):
                    if arg == "--pattern":
                        name = args[index + 1]
                        (dest / name).write_bytes(self.existing[name])
            else:
                self.assertEqual(args[:4], ["gh", "release", "upload", "v0.5.0"])
                self.uploads.extend(Path(arg).name for arg in args[4:])

        for method, replacement in [("check_output", view), ("run", run)]:
            patcher = patch.object(self.publisher.subprocess, method, side_effect=replacement)
            patcher.start()
            self.addCleanup(patcher.stop)

    def test_uploads_complete_matrix_only(self):
        self.publisher.publish("0.5.0", self.root)
        self.assertEqual(set(self.uploads), set(self.assets))
        self.assertEqual(len(self.uploads), 8)

    def test_missing_or_corrupted_build_prevents_any_upload(self):
        archive = next(name for name in self.assets if name.endswith(".tar.gz"))
        (self.root / archive).write_bytes(b"corrupt")
        with self.assertRaises(ValueError):
            self.publisher.publish("0.5.0", self.root)
        (self.root / archive).unlink()
        with self.assertRaises(FileNotFoundError):
            self.publisher.publish("0.5.0", self.root)
        self.assertFalse(self.uploads)

    def test_resume_verifies_and_preserves_existing_pairs(self):
        archive = next(name for name in self.assets if name.endswith(".tar.gz"))
        data = b"earlier build"
        self.existing = {archive: data, archive + ".sha256":
                         f"{hashlib.sha256(data).hexdigest()}  {archive}\n".encode()}
        self.publisher.publish("0.5.0", self.root)
        self.assertEqual(set(self.uploads), set(self.assets) - set(self.existing))

    def test_partial_or_corrupt_published_pair_prevents_upload(self):
        archive = next(name for name in self.assets if name.endswith(".tar.gz"))
        self.existing = {archive: self.assets[archive]}
        with self.assertRaises(ValueError):
            self.publisher.publish("0.5.0", self.root)
        self.existing[archive + ".sha256"] = b"bad checksum"
        with self.assertRaises(ValueError):
            self.publisher.publish("0.5.0", self.root)
        self.assertFalse(self.uploads)


if __name__ == "__main__":
    unittest.main()
