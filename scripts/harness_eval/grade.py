"""Objective graders run outside the submitted checkout, using trusted probes."""

from contextlib import closing
import json
import os
from pathlib import Path
import shutil
import signal
import sqlite3
import subprocess
import sys
import tempfile
import time

from .fixtures import TASKS, starting_files
from .child_environment import command_environment

SQL_TIMEOUT_SECONDS = 30


def command(argv, cwd, timeout=30):
    """Kill the whole process group on timeout or cancellation (POSIX hosts)."""
    with subprocess.Popen(argv, cwd=cwd, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                          stdin=subprocess.DEVNULL, start_new_session=True,
                          env=command_environment()) as child:
        try:
            stdout, stderr = child.communicate(timeout=timeout)
        except BaseException:
            try:
                os.killpg(child.pid, signal.SIGKILL)
            except ProcessLookupError:
                pass
            child.communicate()
            raise
    if child.returncode:
        raise ValueError(f"{Path(argv[0]).name} exited {child.returncode}: "
                         + (stdout + stderr).decode(errors="replace")[-4000:])
    return stdout.decode()


def write_files(root, files):
    for name, content in files.items():
        path = root / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(content, encoding="utf-8")


def grade(name, checkout):
    task = TASKS[name]
    violations = []
    # Only task implementations are mutable; tests/reports cannot waive invariants.
    for path, content in starting_files(task).items():
        candidate = checkout / path
        if path not in task["solution"] and (
            not candidate.is_file() or candidate.is_symlink()
            or candidate.read_bytes() != content.encode()
        ):
            violations.append(f"protected file changed: {path}")
    try:
        with tempfile.TemporaryDirectory(prefix="example-grader-") as tmp:
            root = Path(tmp)
            # Check required inputs, then allow implementations to add local helper
            # modules. Never execute submitted tests or use cached build products.
            for path in starting_files(task).keys() | task["solution"].keys():
                candidate = checkout / path
                if not candidate.is_file() or candidate.is_symlink():
                    raise ValueError(f"missing or linked implementation: {path}")
            shutil.copytree(checkout, root, dirs_exist_ok=True, symlinks=True,
                            ignore=shutil.ignore_patterns(".git", "target", "node_modules", "__pycache__"))
            if any(path.is_symlink() for path in root.rglob("*")):
                raise ValueError("linked source is not supported by the isolated grader")
            if name in {"small-fix", "cross-crate"}:
                rust_grade(name, task, root)
            elif name == "migration":
                migration_grade(root)
            elif name == "frontend":
                frontend_grade(root)
            else:
                resume_grade(root)
    except (OSError, ValueError, sqlite3.Error, subprocess.TimeoutExpired) as error:
        return {"correct": False, "invariant_violations": violations,
                "failure": str(error), "passed": False}
    return {"correct": True, "invariant_violations": violations,
            "failure": None, "passed": not violations}


def rust_grade(name, task, root):
    external = []
    if name == "cross-crate":
        command(["rustc", "--edition=2021", "--crate-type=rlib", "--crate-name=example_core",
                 "crates/example-core/src/lib.rs", "-o", "libexample_core.rlib"], root)
        external = ["--extern", "example_core=libexample_core.rlib"]
        source = "crates/example-api/src/lib.rs"
    else:
        source = "src/lib.rs"
    command(["rustc", "--edition=2021", "--crate-type=rlib", "--crate-name=subject",
             source, "-o", "libsubject.rlib", *external], root)
    (root / "probe.rs").write_text("fn main() { " + task["probe"] + " }\n")
    command(["rustc", "--edition=2021", "probe.rs", "--extern", "subject=libsubject.rlib",
             "-L", ".", *external, "-o", "probe"], root)
    command([str(root / "probe")], root)


def migration_grade(root):
    with closing(sqlite3.connect(":memory:")) as db:
        deadline = time.monotonic() + SQL_TIMEOUT_SECONDS
        cancelled = False

        def progress():
            nonlocal cancelled
            try:
                return time.monotonic() >= deadline
            except KeyboardInterrupt:
                # SQLite converts callback exceptions into OperationalError.
                # Restore operator cancellation outside the C callback boundary.
                cancelled = True
                return True

        db.set_progress_handler(progress, 1000)
        try:
            migration_checks(db, root)
        except sqlite3.Error:
            if cancelled:
                raise KeyboardInterrupt from None
            if time.monotonic() >= deadline:
                raise ValueError(f"migration grading exceeded {SQL_TIMEOUT_SECONDS}s deadline") from None
            raise


def migration_checks(db, root):
    # Seed historical state from the trusted original, not a rewritten migration.
    db.executescript(TASKS["migration"]["files"]["migrations/001.sql"])
    db.execute("INSERT INTO entries VALUES (17, 'ExampleEntry')")
    db.executescript((root / "migrations/002.sql").read_text())
    columns = {column[1]: column for column in db.execute("PRAGMA table_info(entries)")}
    # SQLite assigns INTEGER affinity to declarations containing INT,
    # including INT and BIGINT; spelling is not an implementation invariant.
    declared_type = columns.get("enabled", (None, None, ""))[2]
    if "INT" not in declared_type.upper():
        raise ValueError("enabled must have INTEGER affinity")
    assert_equal(db.execute("SELECT id, name, enabled FROM entries").fetchall(),
                 [(17, "ExampleEntry", 1)])
    db.execute("INSERT INTO entries(name) VALUES ('FutureEntry')")
    assert_equal(db.execute("SELECT enabled FROM entries WHERE name='FutureEntry'").fetchone(), (1,))
    try:
        db.execute("INSERT INTO entries(name, enabled) VALUES ('InvalidEntry', NULL)")
    except sqlite3.IntegrityError:
        return
    raise ValueError("enabled accepted NULL")


def frontend_grade(root):
    (root / "probe.mjs").write_text('''import assert from 'node:assert/strict';
import {renderList} from './web/list.mjs';
assert.equal(renderList([]), '<p role="status">No entries</p>');
assert.equal(renderList(['One', 'Two']), '<ul><li>One</li><li>Two</li></ul>');
assert.equal(renderList(['<&>"' + "'", '&amp;', '']), '<ul><li>&lt;&amp;&gt;&quot;&#39;</li><li>&amp;amp;</li><li></li></ul>');
const items = ['Example']; renderList(items); assert.deepEqual(items, ['Example']);
''')
    command(["node", "probe.mjs"], root)


def resume_grade(root):
    protected = [name for name in starting_files(TASKS["resume"])
                 if name not in TASKS["resume"]["solution"]]
    probe = """import json
from pathlib import Path
protected = PROTECTED_PATHS

def snapshot():
    result = {}
    for name in protected:
        path = Path(name)
        assert path.is_file() and not path.is_symlink(), 'protected input changed during execution'
        result[name] = path.read_bytes()
    return result

def unchanged(before):
    assert snapshot() == before, 'protected input changed during execution'

before = snapshot()
from export import summary
unchanged(before)
assert summary() == {'count': 1, 'total': 7}
unchanged(before)
assert summary() == {'count': 1, 'total': 7}
unchanged(before)
Path('state/exports.jsonl').write_text('')
before = snapshot()
assert summary() == {'count': 0, 'total': 0}
unchanged(before)
Path('state/exports.jsonl').write_text(json.dumps({'id': 'example-export-002', 'amount': -3}) + '\\n' + json.dumps({'id': 'example-export-003', 'amount': 12}) + '\\n')
before = snapshot()
assert summary() == {'count': 2, 'total': 9}
unchanged(before)
"""
    (root / "probe.py").write_text(probe.replace("PROTECTED_PATHS", repr(protected)))
    command([sys.executable, "-B", "probe.py"], root)


def assert_equal(actual, expected):
    if actual != expected:
        raise ValueError(f"expected {expected!r}, observed {actual!r}")
