"""Detached trial working directories, with retained authority for crash recovery.

This prevents accidental ancestor discovery of answers, not adversarial host access.
"""

from pathlib import Path
import shutil
import tempfile
import uuid


def temporary_base(repository):
    base = Path(tempfile.gettempdir()).resolve()
    if base.is_relative_to(repository.resolve()) or any(
        (parent / "inputs/tasks.json").is_file() for parent in (base, *base.parents)
    ):
        raise ValueError("TMPDIR must be outside the repository and experiment directories")
    return base


def create_workspace(checkout, repository):
    container = Path(tempfile.mkdtemp(prefix="example-eval-", dir=temporary_base(repository)))
    token = uuid.uuid4().hex
    (container / ".owner").write_text(token)
    workspace = container / "checkout"
    try:
        shutil.copytree(checkout, workspace, symlinks=True)
    except BaseException:
        shutil.rmtree(container)
        raise
    return {"path": str(workspace), "owner": token}


def workspace_path(authority):
    path = Path(authority["path"])
    if (not path.is_absolute() or path.name != "checkout"
            or not path.parent.name.startswith("example-eval-")
            or path.is_symlink() or not path.is_dir()
            or (path.parent / ".owner").read_text() != authority["owner"]):
        raise ValueError("retained execution workspace is unavailable or changed")
    return path


def retain_workspace(authority, checkout):
    source = workspace_path(authority)
    # The detached source remains intact until a fully graded result is published.
    # An interrupted copy can therefore be repeated without rerunning the client.
    if checkout.exists():
        shutil.rmtree(checkout)
    shutil.copytree(source, checkout, symlinks=True,
                    ignore=shutil.ignore_patterns("target", "node_modules", "__pycache__"))


def release_workspace(authority):
    try:
        path = workspace_path(authority)
        shutil.rmtree(path.parent)
    except (OSError, ValueError):
        # The recorded path allows inspection if temporary-file cleanup failed.
        pass
