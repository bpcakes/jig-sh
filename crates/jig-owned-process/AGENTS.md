# jig-owned-process crate guide

## Purpose

`crates/jig-owned-process` owns generic, bounded child-process execution and fail-closed process-tree cleanup for the Jig runtime.

## Key entrypoints

- `src/lib.rs`: public facade used by other Jig crates.
- `src/pipe.rs`: child stdout/stderr preparation and nonblocking reads shared by generic and secret-aware runners.
- `src/process.rs`: bounded output, cancellation, timeout, and process-tree supervision, including the cleanup policy.
- `src/process/group_cleanup.rs`: signaling the pinned process group and, for confirmed cleanup, proving it is empty on Linux and macOS.
- `src/process/interaction.rs`: cooperative stdin/stdout interaction with an owned child process.
- `src/bash_environment.rs`: environment hygiene for Jig-owned Bash probes.

## Edit here for X

- Change checked command-output helpers: `src/process.rs`.
- Change process-tree identity, waiting, or the cleanup policy: `src/process.rs`.
- Change how a process group is signaled or proven empty: `src/process/group_cleanup.rs`.
- Change long-lived child protocol interaction: `src/process/interaction.rs`.
- Change which inherited Bash controls a Jig-owned probe drops: `src/bash_environment.rs`.

## Invariants

- Pipe adapters retain no captured bytes and expose reads only after nonblocking preparation succeeds; callers own buffering, zeroization, and process cleanup.
- The generic owned-process runner must establish a verifiable process tree before starting work, retain the direct-child identity until descendant cleanup is confirmed, share one absolute cleanup deadline across normal/error/drop paths, and fail closed on incomplete output or unsupported supervision. While that unreaped exact child pins the PGID generation, forced confirmation must re-send group `SIGKILL` before every membership proof so a concurrently exposed member cannot outlive a one-shot signal; never retry after identity loss or reap. Linux procfs confirmation probes each PID's group first and reads `/proc/<pid>/stat` only for a PID that may be a member. It must check the deadline around every signal, stat read, and fallback membership probe, at least once per `LINUX_SCAN_CHEAP_STEPS_PER_BUDGET_CHECK` cheap enumeration or membership steps, and before accepting either a live or empty result, with a re-signal between its two required empty scans. Clock reads can cost microseconds on hosts using HPET, so do not reintroduce a clock read for every PID. On macOS, neither a cached leader exit, `ESRCH`, nor `EPERM` proves group absence; require a fresh exact terminal observation plus an atomic sole-leader membership snapshot.
- `ProcessTreeCleanup::Confirmed` is the default and the only policy for check targets, resource probes, workers, and agent processes, whose leftovers could change what a check reports or hold a Cargo claim. `ProcessTreeCleanup::BestEffort` is for short read-only probes only (Git source-identity commands and doctor version probes): it still signals the pinned group once on every outcome and reaps the direct child within the cleanup budget, but skips the membership proof. Never use it to let a probe's descendants keep running.
- On Linux every owned child sets `PR_SET_PDEATHSIG(SIGKILL)` in `pre_exec` and refuses to exec if its parent already changed, so a killed Jig does not leave the direct child running. Linux ties that signal to the spawning thread, so the thread that spawns an owned child must wait for it, as the runners here do.
- Keep this crate independent from repository context, state, CLI, templates, proxy routing, and vault secret handling.
- Do not replace the specialized process ownership in `jig-dev-proxy` or `jig-vault`; those crates have additional route and secret invariants.

## Common commands

- `cargo test -p jig-owned-process`
- `cargo clippy -p jig-owned-process --all-targets -- -D warnings`
- `cargo test -p jig-sh`
