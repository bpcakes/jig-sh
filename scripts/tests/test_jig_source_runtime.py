"""Behavioral regression tests for the source checkout's default runner."""

from concurrent.futures import ThreadPoolExecutor
import fcntl
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest


REPO = Path(__file__).resolve().parents[2]
NATIVE_FIXTURE = r'''
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#ifndef VERSION
#define VERSION "0.4.0"
#endif
#ifndef CONTRACT
#define CONTRACT "8"
#endif
#ifndef PIN_AWARE_UPDATE
#define PIN_AWARE_UPDATE 0
#endif
int main(int argc, char **argv) {
    if (argc > 1 && strcmp(argv[1], "__runtime-compatible") == 0) {
        int contract_matches = 0;
        int requires_pin_aware_update = 0;
        for (int i = 2; i + 1 < argc; i++) {
            if (strcmp(argv[i], "--contract-version") == 0)
                contract_matches = strcmp(argv[i + 1], CONTRACT) == 0;
            if (strcmp(argv[i], "--require-runtime-pin-update") == 0)
                requires_pin_aware_update = 1;
        }
        return !contract_matches || (requires_pin_aware_update && !PIN_AWARE_UPDATE);
    }
    int i = 1;
    while (i + 1 < argc && strncmp(argv[i], "--__launcher-", 13) == 0) i += 2;
    if (i < argc && strcmp(argv[i], "--version") == 0) {
        puts("jig " VERSION);
        return 0;
    }
    if (i < argc && strcmp(argv[i], "check") == 0) {
        /* Model a runner invoking the tool under test. Selection must not build. */
        const char *tool = getenv("EXAMPLE_CHECK_TOOL");
        return tool ? system(tool) : 0;
    }
    puts("fixture runtime " VERSION);
    return 0;
}
'''


class SourceRuntimeTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.native_temp = tempfile.TemporaryDirectory(prefix="ExampleRuntimeBinaries-")
        cls.addClassCleanup(cls.native_temp.cleanup)
        directory = Path(cls.native_temp.name)
        source = directory / "example.c"
        source.write_text(NATIVE_FIXTURE)
        cls.binaries = {}
        for name, version, contract in [("release", "0.4.0", "8"),
                                        ("newer", "0.5.0", "8"),
                                        ("incompatible", "0.4.0", "7")]:
            binary = directory / name
            subprocess.run(["cc", str(source), "-o", str(binary),
                            f'-DVERSION="{version}"', f'-DCONTRACT="{contract}"'],
                           check=True, capture_output=True)
            cls.binaries[name] = binary

    def setUp(self):
        temporary = tempfile.TemporaryDirectory(prefix="ExampleSourceRuntime-")
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name) / "ExampleProject"
        for name in ["scripts", ".git", ".agent", ".jig", "crates/jig/src",
                     "templates/project/scripts", "tools"]:
            (self.root / name).mkdir(parents=True, exist_ok=True)
        for name in ["jig", "install-jig.sh", "jig-source-runtime.py"]:
            shutil.copy2(REPO / "scripts" / name, self.root / "scripts" / name)
        (self.root / "templates/project/scripts/install-jig.sh.jinja").touch()
        (self.root / "crates/jig/Cargo.toml").write_text('[package]\nname = "jig-sh"\n')
        (self.root / "crates/jig/src/main.rs").write_text("fn main() {}\n")
        (self.root / ".jig.toml").write_text('_src_path = "embedded:jig-sh"\n')
        (self.root / ".jig/source-runtime-version").write_text("0.4.0\n")
        (self.root / ".agent/jig-contract.json").write_text('{"contract_version":8}\n')
        self.bin = self.root / "tools"
        # Supply only required tools, so a workstation's installed Jig cannot
        # accidentally satisfy a missing-runtime test.
        for name in ["dirname", "grep", "bash"]:
            (self.bin / name).symlink_to(shutil.which(name))
        (self.bin / "python3").symlink_to(sys.executable)
        self.cargo_log = self.root / "cargo-called"
        (self.bin / "cargo").write_text('#!/bin/sh\n: > "$EXAMPLE_CARGO_LOG"\nexit 99\n')
        (self.bin / "cargo").chmod(0o755)
        self.env = {k: v for k, v in os.environ.items() if not k.startswith("JIG_")}
        self.env.update(PATH=str(self.bin), EXAMPLE_CARGO_LOG=str(self.cargo_log))
        self.install()

    def install(self, name="release"):
        destination = self.bin / "jig"
        destination.unlink(missing_ok=True)
        shutil.copy2(self.binaries[name], destination)

    def launcher(self, *args, env=None):
        return subprocess.run([str(self.root / "scripts/jig"), *args],
                              cwd=self.root, env=env or self.env,
                              capture_output=True, text=True, timeout=20)

    def installer(self, *args, env=None):
        return subprocess.run([str(self.root / "scripts/install-jig.sh"), *args],
                              cwd=self.root, env=env or self.env,
                              capture_output=True, text=True, timeout=20)

    def assert_ok(self, result):
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)

    def tearDown(self):
        self.assertFalse(self.cargo_log.exists(), "Ordinary source launcher invoked Cargo")

    def test_source_edits_and_broken_code_do_not_rebuild_runner(self):
        result = self.launcher("--version")
        self.assert_ok(result)
        self.assertEqual(result.stdout, "jig 0.4.0\n")
        cached = self.root / ".git/jig-tools/source-release-0.4.0/bin/jig"
        original = cached.stat().st_mtime_ns
        (self.root / "crates/jig/src/main.rs").write_text("this is broken Rust\n")
        (self.root / "Cargo.toml").write_text("broken TOML [\n")
        self.install("newer")
        for command in [("--version",), ("work", "status"), ("mcp",), ("doctor",)]:
            self.assert_ok(self.launcher(*command))
        self.assertEqual(cached.stat().st_mtime_ns, original)
        self.assertEqual(self.launcher("--version").stdout, "jig 0.4.0\n")
        # Only the requested check may invoke a build/test tool, exactly once.
        check_log = self.root / "check-called"
        check_tool = self.bin / "example-check"
        check_tool.write_text('#!/bin/sh\necho test >> "$EXAMPLE_CHECK_LOG"\n')
        check_tool.chmod(0o755)
        self.assert_ok(self.launcher("check", "test", env=dict(
            self.env, EXAMPLE_CHECK_TOOL=str(check_tool), EXAMPLE_CHECK_LOG=str(check_log))))
        self.assertEqual(check_log.read_text(), "test\n")

    def test_missing_installed_runtime_fails_with_setup_guidance(self):
        (self.bin / "jig").unlink()
        result = self.launcher("--version")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("--version =0.4.0 --locked", result.stderr)
        self.assertIn("scripts/jig-dev", result.stderr)

    def test_python_startup_environment_cannot_corrupt_selected_path(self):
        customization = self.root / "python-customization"
        customization.mkdir()
        (customization / "sitecustomize.py").write_text('print("Example startup message")\n')
        result = self.launcher("--version", env=dict(self.env, PYTHONPATH=str(customization)))
        self.assert_ok(result)
        self.assertEqual(result.stdout, "jig 0.4.0\n")

    def test_wrong_version_or_incompatible_release_is_not_cached(self):
        for name, message in [("newer", "does not match pinned release"),
                              ("incompatible", "cannot run contract 8")]:
            with self.subTest(name=name):
                self.install(name)
                result = self.launcher("--version")
                self.assertNotEqual(result.returncode, 0)
                self.assertIn(message, result.stderr)
                self.assertFalse((self.root / ".git/jig-tools/source-release-0.4.0/bin/jig").exists())

    def test_path_wrapper_is_rejected_without_execution(self):
        marker = self.root / "wrapper-executed"
        (self.bin / "jig").write_text(f'#!/bin/sh\ntouch "{marker}"\necho "jig 0.4.0"\n')
        result = self.launcher("--version")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("Expected a native Jig executable", result.stderr)
        self.assertFalse(marker.exists())

    def test_mcp_and_resolve_only_never_populate_cache(self):
        for call in [lambda: self.installer("--resolve-only", "--profile", "runtime"),
                     lambda: self.launcher("mcp")]:
            self.assertNotEqual(call().returncode, 0)
            self.assertFalse((self.root / ".git/jig-tools").exists())
        self.assert_ok(self.launcher("--version"))
        self.assert_ok(self.launcher("mcp"))

    def test_explicit_override_remains_authoritative(self):
        env = dict(self.env, JIG_DEV_BIN=str(self.binaries["newer"]))
        result = self.launcher("--version", env=env)
        self.assert_ok(result)
        self.assertEqual(result.stdout, "jig 0.5.0\n")
        self.assertFalse((self.root / ".git/jig-tools").exists())
        env["JIG_DEV_BIN"] = str(self.root / "missing")
        self.assertNotEqual(self.launcher("--version", env=env).returncode, 0)
        self.assertFalse((self.root / ".git/jig-tools").exists())

    def test_refresh_failure_preserves_previous_runtime(self):
        self.assert_ok(self.launcher("--version"))
        cached = self.root / ".git/jig-tools/source-release-0.4.0/bin/jig"
        old = cached.read_bytes()
        self.install("newer")
        self.assertNotEqual(self.installer("--refresh", "--profile", "runtime").returncode, 0)
        self.assertEqual(cached.read_bytes(), old)
        self.assert_ok(self.launcher("--version"))
        self.install()
        self.assert_ok(self.installer("--refresh", "--profile", "runtime"))

    def test_pin_change_requires_the_selected_release(self):
        self.assert_ok(self.launcher("--version"))
        (self.root / ".jig/source-runtime-version").write_text("0.5.0\n")
        self.assertNotEqual(self.launcher("--version").returncode, 0)
        self.install("newer")
        result = self.launcher("--version")
        self.assert_ok(result)
        self.assertEqual(result.stdout, "jig 0.5.0\n")

    def test_missing_policy_half_fails_without_legacy_source_install(self):
        pin = self.root / ".jig/source-runtime-version"
        pin.unlink()
        self.assertNotEqual(self.launcher("--version").returncode, 0)
        pin.write_text("0.4.0\n")
        (self.root / "scripts/jig-source-runtime.py").unlink()
        self.assertNotEqual(self.launcher("--version").returncode, 0)

    def test_worktree_git_file_uses_ignored_cache(self):
        (self.root / ".git").rmdir()
        (self.root / ".git").write_text("gitdir: /example/ExampleWorktree\n")
        self.assert_ok(self.launcher("--version"))
        self.assertTrue((self.root / ".agent/.cache/jig/source-release-0.4.0/bin/jig").is_file())

    def test_concurrent_cold_calls_publish_one_complete_runtime(self):
        with ThreadPoolExecutor(max_workers=4) as pool:
            results = list(pool.map(lambda _: self.launcher("--version"), range(4)))
        for result in results:
            self.assert_ok(result)
            self.assertEqual(result.stdout, "jig 0.4.0\n")
        self.assertEqual(sum("cached installed binary" in r.stderr for r in results), 1)

    def test_cache_lock_contention_times_out(self):
        lock_path = self.root / ".git/jig-tools/source-release-0.4.0/import.lock"
        lock_path.parent.mkdir(parents=True)
        child = """
import importlib.util
import sys
from unittest import mock

spec = importlib.util.spec_from_file_location("source_runtime", sys.argv[1])
runtime = importlib.util.module_from_spec(spec)
spec.loader.exec_module(runtime)
with open(sys.argv[2], "a") as stream:
    with mock.patch.object(runtime.time, "monotonic", side_effect=[0.0, 31.0]):
        with mock.patch.object(runtime.time, "sleep"):
            try:
                runtime.acquire_lock(stream)
            except ValueError as error:
                print(error, file=sys.stderr)
                sys.exit(3)
"""
        with lock_path.open("a") as held:
            fcntl.flock(held, fcntl.LOCK_EX | fcntl.LOCK_NB)
            result = subprocess.run(
                [sys.executable, "-I", "-c", child,
                 str(self.root / "scripts/jig-source-runtime.py"), str(lock_path)],
                cwd=self.root, env=self.env, capture_output=True, text=True, timeout=5,
            )
        self.assertEqual(result.returncode, 3, result.stdout + result.stderr)
        self.assertIn("Timed out waiting for the source runtime cache lock", result.stderr)

    def test_info_reports_release_and_override_without_writes(self):
        script = self.root / "scripts/jig-source-runtime.py"
        args = [sys.executable, str(script), "--info"]
        missing = subprocess.run(args, env=self.env, capture_output=True, text=True)
        self.assertNotEqual(missing.returncode, 0)
        self.assertFalse((self.root / ".git/jig-tools").exists())
        self.assert_ok(self.launcher("--version"))
        released = subprocess.run(args, env=self.env, capture_output=True, text=True)
        self.assert_ok(released)
        data = json.loads(released.stdout)
        self.assertEqual(data["mode"], "released")
        self.assertEqual(data["runtime_version"], "0.4.0")
        override = subprocess.run(args, env=dict(self.env, JIG_DEV_BIN=str(self.binaries["newer"])),
                                  capture_output=True, text=True)
        self.assert_ok(override)
        data = json.loads(override.stdout)
        self.assertEqual(data["mode"], "development_override")
        self.assertEqual(data["runtime_version"], "0.5.0")


if __name__ == "__main__":
    unittest.main()
