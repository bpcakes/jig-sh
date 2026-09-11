"""Bounded POSIX command capture with an unreaped process-group leader."""

import ctypes
from functools import lru_cache
import os
from pathlib import Path
import signal
import subprocess
import sys
import tempfile
import time


def exit_status(child):
    """Observe without reaping: the child pins its process-group identifier."""
    info = os.waitid(os.P_PID, child.pid, os.WEXITED | os.WNOHANG | os.WNOWAIT)
    if info is None or info.si_pid == 0:
        return None
    return info.si_status if info.si_code == os.CLD_EXITED else -info.si_status


@lru_cache(maxsize=1)
def macos_group_query():
    query = ctypes.CDLL("/usr/lib/libproc.dylib", use_errno=True).proc_listpgrppids
    query.argtypes = [ctypes.c_int, ctypes.c_void_p, ctypes.c_int]
    query.restype = ctypes.c_int
    return query


def group_quiet(pid, deadline):
    if sys.platform == "darwin":
        # Like Jig's native owner, use an atomic, two-member kernel snapshot.
        # EPERM from signalling a zombie is not itself proof of group absence.
        members = (ctypes.c_int * 2)()
        count = macos_group_query()(pid, members, ctypes.sizeof(members))
        if count <= 0 or count > 2 or any(member <= 0 for member in members[:count]):
            raise RuntimeError("could not inspect the owned command process group")
        return count == 1 and members[0] == pid
    if sys.platform != "linux":
        raise RuntimeError("evaluation process supervision requires Linux or macOS")
    for path in Path("/proc").iterdir():
        if time.monotonic() >= deadline:
            raise RuntimeError("command process-group inspection timed out")
        if not path.name.isdigit():
            continue
        try:
            fields = (path / "stat").read_bytes().rsplit(b")", 1)[1].split()
        except (FileNotFoundError, ProcessLookupError):
            continue
        if int(fields[2]) == pid and fields[0] not in {b"Z", b"X"}:
            return False
    return True


def signal_group(child, signum):
    exit_status(child)  # A lost child handle must fail before a numeric signal.
    try:
        os.killpg(child.pid, signum)
    except ProcessLookupError:
        pass  # Confirmation below still requires a pinned, exited leader.
    except PermissionError:
        if sys.platform != "darwin":
            raise


def stop_group(child, grace_seconds=0):
    if grace_seconds:
        signal_group(child, signal.SIGTERM)
        until = time.monotonic() + grace_seconds
        while exit_status(child) is None and time.monotonic() < until:
            time.sleep(0.01)
    deadline = time.monotonic() + 5
    quiet = 0
    while time.monotonic() < deadline:
        signal_group(child, signal.SIGKILL)
        if exit_status(child) is not None and group_quiet(child.pid, deadline):
            if time.monotonic() >= deadline:
                break
            quiet += 1
            if quiet == 2:
                child.wait(timeout=max(0.001, deadline - time.monotonic()))
                return
        else:
            quiet = 0
        time.sleep(0.01)
    raise RuntimeError("could not retire the command process group within 5 seconds")


def capture_command(argv, cwd, env, timeout):
    if sys.platform not in {"darwin", "linux"} or not hasattr(os, "waitid"):
        raise ValueError("evaluation command supervision requires Linux or macOS waitid")
    # File capture avoids a descendant keeping communicate() blocked on a pipe.
    with tempfile.TemporaryFile() as output:
        child = subprocess.Popen(argv, cwd=cwd, env=env, stdin=subprocess.DEVNULL,
                                 stdout=output, stderr=subprocess.STDOUT, start_new_session=True)
        timed_out = False
        deadline = time.monotonic() + timeout
        try:
            while exit_status(child) is None:
                if time.monotonic() >= deadline:
                    timed_out = True
                    break
                time.sleep(0.01)
        finally:
            try:
                stop_group(child)
            except Exception as error:
                raise RuntimeError("command process-group cleanup failed") from error
        output.seek(0, os.SEEK_END)
        output.seek(max(0, output.tell() - 20000))
        result = {"returncode": child.returncode, "output": output.read().decode(errors="replace")}
    if timed_out:
        result.update(error="timeout", timeout_seconds=timeout)
    return result
