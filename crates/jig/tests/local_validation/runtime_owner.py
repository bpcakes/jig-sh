#!/usr/bin/env python3
"""Own the real runtime so fixture cleanup never signals a recycled PID."""

import fcntl
import os
from pathlib import Path
import signal
import subprocess
import sys
import time


root = Path('.agent')
# The runtime shares our phase group and receives these signals directly.
# Keep its parent alive to reap it, including after fixture-driven SIGCONT.
for sig in (signal.SIGHUP, signal.SIGINT, signal.SIGTERM):
    signal.signal(sig, lambda *_: None)

with (root / 'runtime-owner.lock').open('w') as lock:
    fcntl.flock(lock, fcntl.LOCK_EX)
    child = subprocess.Popen([os.environ['EXAMPLE_JIG_BIN'], *sys.argv[1:]])
    suspended = False
    deadline = time.monotonic() + 30
    try:
        while child.poll() is None:
            if (root / 'release').exists() or time.monotonic() >= deadline:
                break
            if not suspended and (root / 'stall').exists() and (root / 'ready').exists():
                # This sole parent has not reaped the child; its PID is pinned.
                child.send_signal(signal.SIGSTOP)
                suspended = True
                (root / 'suspended').touch()
            time.sleep(.01)
    finally:
        if child.returncode is None:
            child.send_signal(signal.SIGCONT)
            child.terminate()
            try:
                child.wait(timeout=5)
            except subprocess.TimeoutExpired:
                child.kill()
                child.wait()
        (root / 'runtime-reaped').touch()
    sys.exit(child.returncode if child.returncode >= 0 else 128 - child.returncode)
