"""Environment shared by commands that execute submitted code."""

import os


def command_environment():
    # Host filesystem access is still available; this is an environment boundary,
    # not a security sandbox. Only the provider adapter inherits API credentials.
    allowed = {"PATH", "HOME", "TMPDIR", "RUSTUP_HOME", "CARGO_HOME", "SYSTEMROOT"}
    return {key: value for key, value in os.environ.items() if key in allowed}
