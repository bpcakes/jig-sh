"""Exercise generated runtime pinning without downloading or compiling Jig."""

from concurrent.futures import ThreadPoolExecutor
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest

from test_jig_source_runtime import NATIVE_FIXTURE, REPO


class ReleaseRuntimeTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        temporary = tempfile.TemporaryDirectory(prefix="ExampleReleaseBinaries-")
        cls.addClassCleanup(temporary.cleanup)
        cls.binaries = Path(temporary.name)
        source = cls.binaries / "example.c"
        source.write_text(NATIVE_FIXTURE)
        for name, version, contract in [("0.5.0", "0.5.0", "8"),
                                        ("0.5.1", "0.5.1", "8"),
                                        ("incompatible", "0.5.0", "7")]:
            subprocess.run(["cc", str(source), "-o", str(cls.binaries / name),
                            f'-DVERSION="{version}"', f'-DCONTRACT="{contract}"'],
                           check=True, capture_output=True)

    def setUp(self):
        temporary = tempfile.TemporaryDirectory(prefix="ExampleReleaseRuntime-")
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        for name in ["scripts", ".git", ".agent", ".jig", "tools"]:
            (self.root / name).mkdir()
        for name in ["jig", "install-jig.sh"]:
            shutil.copy2(REPO / "scripts" / name, self.root / "scripts" / name)
        self.config = self.root / ".jig.toml"
        self.config.write_text('_src_path = "https://example.invalid/jig.git"\n'
                               '_commit = "0123456789abcdef"\n')
        (self.root / ".agent/jig-contract.json").write_text('{"contract_version":8}\n')
        self.pin = self.root / ".jig/runtime-version"
        self.pin.write_text("0.5.0\n")
        self.tools = self.root / "tools"
        for name in ["dirname", "bash", "awk", "mktemp", "mkdir", "cat", "cp", "chmod", "mv", "rm"]:
            (self.tools / name).symlink_to(shutil.which(name))
        (self.tools / "python3").symlink_to(sys.executable)
        self.log = self.root / "cargo.jsonl"
        cargo = self.tools / "cargo"
        cargo.write_text('''#!/usr/bin/env python3
import json, os, pathlib, shutil, sys
args = sys.argv[1:]
with open(os.environ["EXAMPLE_CARGO_LOG"], "a") as log:
    log.write(json.dumps(args) + "\\n")
if os.environ.get("EXAMPLE_CARGO_EXIT"):
    sys.exit(int(os.environ["EXAMPLE_CARGO_EXIT"]))
version = args[args.index("--version") + 1].lstrip("=") if "--version" in args else "0.5.0"
version = os.environ.get("EXAMPLE_INSTALL_VERSION", version)
root = pathlib.Path(args[args.index("--root") + 1])
(root / "bin").mkdir(parents=True, exist_ok=True)
shutil.copy2(pathlib.Path(os.environ["EXAMPLE_BINARIES"]) / version, root / "bin/jig")
''')
        cargo.chmod(0o755)
        self.env = {key: value for key, value in os.environ.items()
                    if not key.startswith("JIG_")}
        self.env.update(PATH=str(self.tools), EXAMPLE_CARGO_LOG=str(self.log),
                        EXAMPLE_BINARIES=str(self.binaries))

    def installer(self, *args, env=None):
        return subprocess.run([str(self.root / "scripts/install-jig.sh"), *args],
                              cwd=self.root, env=env or self.env,
                              capture_output=True, text=True, timeout=20)

    def launcher(self, *args, env=None):
        return subprocess.run([str(self.root / "scripts/jig"), *args],
                              cwd=self.root, env=env or self.env,
                              capture_output=True, text=True, timeout=20)

    def calls(self):
        return [json.loads(line) for line in self.log.read_text().splitlines()] if self.log.exists() else []

    def assert_ok(self, result):
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)

    def test_cold_install_uses_exact_crate_and_ignores_template_revision_changes(self):
        result = self.launcher("--version")
        self.assert_ok(result)
        self.assertEqual(result.stdout, "jig 0.5.0\n")
        args, = self.calls()
        self.assertEqual(args[:7], ["install", "jig-sh", "--registry", "crates-io",
                                   "--version", "=0.5.0", "--locked"])
        self.assertNotIn("--git", args)
        self.assertIn("--no-default-features", args)
        self.config.write_text('_src_path = "embedded:jig-sh"\n_commit = "fedcba9876543210"\n')
        self.assert_ok(self.launcher("--version"))
        self.assert_ok(self.launcher("mcp"))
        self.assertEqual(len(self.calls()), 1)

    def test_pin_change_selects_new_version_and_can_reuse_previous_release(self):
        self.assert_ok(self.launcher("--version"))
        self.pin.write_text("0.5.1\n")
        result = self.launcher("--version")
        self.assert_ok(result)
        self.assertEqual(result.stdout, "jig 0.5.1\n")
        self.pin.write_text("0.5.0\n")
        self.assertEqual(self.launcher("--version").stdout, "jig 0.5.0\n")
        self.assertEqual(len(self.calls()), 2)

    def test_full_profile_serves_runtime_and_mcp_without_another_install(self):
        full = self.installer("--profile", "default")
        self.assert_ok(full)
        self.assertNotIn("--no-default-features", self.calls()[0])
        for profile in ["runtime", "mcp"]:
            result = self.installer("--profile", profile)
            self.assert_ok(result)
            self.assertEqual(result.stdout, full.stdout)
        self.assertEqual(len(self.calls()), 1)

    def test_runtime_cache_does_not_replace_full_profile(self):
        runtime = self.installer("--profile", "runtime")
        full = self.installer("--profile", "default")
        self.assert_ok(runtime)
        self.assert_ok(full)
        self.assertNotEqual(runtime.stdout, full.stdout)
        self.assertEqual(len(self.calls()), 2)

    def test_installed_native_release_is_imported_and_then_serves_mcp(self):
        shutil.copy2(self.binaries / "0.5.0", self.tools / "jig")
        self.assert_ok(self.launcher("--version"))
        (self.tools / "jig").unlink()
        self.assert_ok(self.launcher("mcp"))
        self.assertFalse(self.calls())

    def test_wrong_or_incompatible_path_binary_does_not_override_pin(self):
        for name in ["0.5.1", "incompatible"]:
            with self.subTest(binary=name):
                shutil.copy2(self.binaries / name, self.tools / "jig")
                result = self.launcher("--version")
                self.assert_ok(result)
                self.assertEqual(result.stdout, "jig 0.5.0\n")
                shutil.rmtree(self.root / ".git/jig-tools")
        self.assertEqual(len(self.calls()), 2)

    def test_path_script_is_not_executed(self):
        marker = self.root / "path-wrapper-called"
        wrapper = self.tools / "jig"
        wrapper.write_text('#!/bin/sh\n: > "$EXAMPLE_PATH_LOG"\nexit 1\n')
        wrapper.chmod(0o755)
        self.assert_ok(self.launcher("--version", env=dict(self.env, EXAMPLE_PATH_LOG=str(marker))))
        self.assertFalse(marker.exists())
        self.assertEqual(len(self.calls()), 1)

    def test_read_only_resolution_and_cold_mcp_never_install(self):
        shutil.copy2(self.binaries / "0.5.0", self.tools / "jig")
        for args in [("--resolve-only",), ("--profile", "mcp")]:
            self.assertNotEqual(self.installer(*args).returncode, 0)
        self.assertFalse(self.calls())
        self.assertFalse(list(self.root.glob(".git/jig-tools/**/bin/jig")))

    def test_invalid_pin_cannot_fall_back_to_cached_or_git_runtime(self):
        self.assert_ok(self.launcher("--version"))
        for invalid in ["", "latest", "^0.5", "0.5.0\n0.5.1", "v0.5.0", "00.5.0", "0.5.0-dev"]:
            with self.subTest(pin=invalid):
                self.pin.write_text(invalid)
                result = self.launcher("--version")
                self.assertNotEqual(result.returncode, 0)
                self.assertIn("Invalid .jig/runtime-version", result.stderr)
        self.assertEqual(len(self.calls()), 1)

    def test_pin_rejects_symlinks_special_files_and_oversized_input(self):
        self.pin.unlink()
        self.pin.symlink_to(self.config)
        self.assertNotEqual(self.installer().returncode, 0)
        self.pin.unlink()
        os.mkfifo(self.pin)
        self.assertNotEqual(self.installer().returncode, 0)
        self.pin.unlink()
        self.pin.write_text("0" * 129)
        self.assertNotEqual(self.installer().returncode, 0)
        self.assertFalse(self.calls())

    def test_failed_or_mismatched_install_does_not_fall_back_to_git(self):
        failed = self.installer(env=dict(self.env, EXAMPLE_CARGO_EXIT="42"))
        self.assertEqual(failed.returncode, 42)
        for version in ["0.5.1", "incompatible"]:
            result = self.installer(env=dict(self.env, EXAMPLE_INSTALL_VERSION=version))
            self.assertNotEqual(result.returncode, 0)
            self.assertIn("does not match release", result.stderr)
        self.assertEqual(len(self.calls()), 3)
        self.assertTrue(all("--git" not in args for args in self.calls()))
        self.assert_ok(self.installer())
        self.assertEqual(len(self.calls()), 4)

    def test_refresh_reinstalls_same_release(self):
        self.assert_ok(self.installer())
        self.assert_ok(self.installer("--refresh"))
        self.assertEqual(len(self.calls()), 2)
        self.assertTrue(all("=0.5.0" in args for args in self.calls()))

    def test_worktree_uses_fallback_cache(self):
        (self.root / ".git").rmdir()
        (self.root / ".git").write_text("gitdir: /example/worktree\n")
        result = self.installer()
        self.assert_ok(result)
        self.assertIn("/.agent/.cache/jig/release-0.5.0-contract-8/", result.stdout)

    def test_explicit_install_root_is_populated_even_with_a_path_binary(self):
        shutil.copy2(self.binaries / "0.5.0", self.tools / "jig")
        destination = self.root / "explicit"
        result = self.installer(str(destination))
        self.assert_ok(result)
        self.assertEqual(result.stdout.strip(), str(destination / "bin/jig"))
        self.assertEqual(len(self.calls()), 1)

    def test_development_override_remains_authoritative(self):
        self.pin.write_text("invalid")
        result = self.launcher("--version", env=dict(self.env, JIG_DEV_BIN=str(self.binaries / "0.5.1")))
        self.assert_ok(result)
        self.assertEqual(result.stdout, "jig 0.5.1\n")
        self.assertFalse(self.calls())

    def test_absent_pin_keeps_source_revision_installation(self):
        self.pin.unlink()
        result = self.installer()
        self.assert_ok(result)
        args, = self.calls()
        self.assertIn("--git", args)
        self.assertIn("0123456789abcdef", args)
        self.assertNotIn("--version", args)

    def test_concurrent_requests_install_once(self):
        with ThreadPoolExecutor(max_workers=3) as executor:
            results = list(executor.map(lambda _: self.installer(), range(3)))
        for result in results:
            self.assert_ok(result)
        self.assertEqual(len(self.calls()), 1)


if __name__ == "__main__":
    unittest.main()
