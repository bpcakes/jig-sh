# jig-git crate guide

## Purpose

`crates/jig-git` owns the Git primitives every Jig module shares: which program runs as `git`, which inherited `GIT_*` variables a Jig-owned Git command keeps, and byte-preserving reads of Git's on-disk metadata files.

## Key entrypoints

- `src/lib.rs`: `GIT_BIN_ENV`, `git_program`, and the known-repository environment scrubbing.
- `src/metadata.rs`: bounded, no-follow reads and parsing of `.git` pointer files, `commondir`, and `gitdir` back-links.

## Edit here for X

- Change how Jig selects the Git program (`JIG_GIT_BIN`): `git_program` in `src/lib.rs`.
- Change which `GIT_*` variables a command aimed at a known repository keeps: `scrub_known_repository_git_environment` in `src/lib.rs`.
- Change reads of Git metadata files: `src/metadata.rs`.
- Change the staged, ambient-config, or remote-template Git environments used by init/adopt/update: `crates/jig/src/bootstrap/git.rs`, which builds them on `scrub_git_repository_environment_except`.

## Invariants

- `GIT_*` scrubbing is deny-by-default: a variable survives only when the caller's allowlist names it, including variables the command set explicitly.
- Metadata reads never follow a final symlink, never block on a FIFO, and cap the bytes read independently of the reported file length.
- Keep this crate independent from repository context, state, CLI, templates, and vault secret handling.

## Common commands

- `cargo test -p jig-git`
- `cargo clippy -p jig-git --all-targets -- -D warnings`
- `cargo test -p jig-sh`
