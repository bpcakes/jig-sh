"""Exercise cancellation while the real launcher is still running its installer."""

import fcntl
import os
from pathlib import Path
import shutil
import signal
import subprocess
import sys
import tempfile
import textwrap
import time
import unittest


SCRIPTS = Path(__file__).resolve().parents[1]


class LocalCheckCancellationTests(unittest.TestCase):
    def exercise_startup(self, signum, stubborn=False, terminal=False):
        with tempfile.TemporaryDirectory(prefix="ExampleLocalCheck-") as directory:
            root = Path(directory)
            scripts = root / "scripts"
            scripts.mkdir()
            for name in ["check-local", "jig"]:
                shutil.copy2(SCRIPTS / name, scripts / name)

            def executable(name, source):
                path = scripts / name
                path.write_text("#!" + sys.executable + "\n" + textwrap.dedent(source))
                path.chmod(0o755)

            executable("install-jig.sh", """\
                import os, pathlib, signal, subprocess, sys
                # Exit promptly, leaving the helper to finish its own cleanup.
                for sig in (signal.SIGHUP, signal.SIGINT, signal.SIGTERM):
                    signal.signal(sig, lambda sig, frame: os._exit(128 + sig))
                with open('installer-calls', 'a') as calls:
                    calls.write('startup\\n')
                subprocess.run([sys.executable, 'scripts/helper'], check=True)
                print(pathlib.Path('scripts/runtime').resolve())
                """)
            executable("helper", """\
                import fcntl, os, pathlib, signal, time
                def cancel(sig, frame):
                    with open('signals', 'a') as received:
                        received.write(str(sig) + '\\n')
                    if os.environ['EXAMPLE_STUBBORN'] != '1':
                        while not pathlib.Path('release').exists():
                            time.sleep(0.01)
                        raise SystemExit(128 + sig)
                for sig in (signal.SIGHUP, signal.SIGINT, signal.SIGTERM):
                    signal.signal(sig, cancel)
                with open('import.lock', 'w') as lock:
                    fcntl.flock(lock, fcntl.LOCK_EX)
                    pathlib.Path('heartbeat').write_text(str(time.monotonic_ns()))
                    pathlib.Path('ready').touch()
                    while not pathlib.Path('release').exists():
                        pathlib.Path('heartbeat').write_text(str(time.monotonic_ns()))
                        time.sleep(0.02)
                """)
            executable("runtime", "import pathlib\npathlib.Path('runtime-started').touch()\n")
            environment = {key: value for key, value in os.environ.items()
                           if not key.startswith("JIG_")}
            environment["EXAMPLE_STUBBORN"] = "1" if stubborn else "0"
            with tempfile.TemporaryFile() as output:
                child = subprocess.Popen(
                    [sys.executable, str(scripts / "check-local"), "--plan-id", "example"],
                    cwd=root, env=environment, stdin=subprocess.DEVNULL,
                    stdout=output, stderr=output, start_new_session=True,
                )
                try:
                    self.wait_for(root / "ready")
                    # PID-directed cancellation must work even before Jig's
                    # native runtime takes ownership of any process trees.
                    if terminal:
                        os.killpg(child.pid, signum)
                    else:
                        child.send_signal(signum)
                    self.wait_for(root / "signals")
                    if not stubborn:
                        time.sleep(0.1)
                        self.assertIsNone(child.poll(), "returned before helper cleanup")
                        (root / "release").touch()
                    self.assertEqual(child.wait(timeout=15), 1 if stubborn else 128 + signum)
                    if stubborn:
                        output.seek(0)
                        self.assertIn(b"descendant cleanup could not be confirmed", output.read())
                    self.assertEqual((root / "signals").read_text(), f"{signum}\n")
                    # The helper must have stopped and released the startup lock
                    # before the wrapper reports cancellation complete.
                    with (root / "import.lock").open("w") as lock:
                        fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
                    heartbeat = (root / "heartbeat").read_text()
                    time.sleep(0.1)
                    self.assertEqual((root / "heartbeat").read_text(), heartbeat)
                    self.assertEqual((root / "installer-calls").read_text(), "startup\n")
                    self.assertFalse((root / "runtime-started").exists())
                finally:
                    # Release all fixture processes even when testing a broken
                    # wrapper that never forwards the signal to descendants.
                    (root / "release").touch()
                    try:
                        child.wait(timeout=5)
                    except subprocess.TimeoutExpired:
                        child.kill()
                        child.wait(timeout=5)

    def wait_for(self, path):
        deadline = time.monotonic() + 5
        while not path.exists() or (path.name == "signals" and not path.read_text()):
            self.assertLess(time.monotonic(), deadline, f"timed out waiting for {path.name}")
            time.sleep(0.01)

    def test_sigterm_waits_for_startup_descendant_cleanup(self):
        self.exercise_startup(signal.SIGTERM)

    def test_sigint_waits_for_startup_descendant_cleanup(self):
        self.exercise_startup(signal.SIGINT)

    def test_sigterm_kills_uncooperative_startup_descendants(self):
        self.exercise_startup(signal.SIGTERM, stubborn=True)

    def test_sigint_kills_uncooperative_startup_descendants(self):
        self.exercise_startup(signal.SIGINT, stubborn=True)

    def test_terminal_sigint_reaches_startup_descendants_once(self):
        self.exercise_startup(signal.SIGINT, terminal=True)

    def test_sighup_waits_for_startup_descendant_cleanup(self):
        self.exercise_startup(signal.SIGHUP)

    def test_sighup_kills_uncooperative_startup_descendants(self):
        self.exercise_startup(signal.SIGHUP, stubborn=True)

    def test_terminal_sighup_reaches_startup_descendants_once(self):
        self.exercise_startup(signal.SIGHUP, terminal=True)


if __name__ == "__main__":
    unittest.main()
