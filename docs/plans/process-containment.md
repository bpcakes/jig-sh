# Kernel containment for owned process trees

Design note, not yet implemented. Implementation phases below become ExecPlans
when they start.

## Purpose and acceptance

Replace the Linux process-group proof with kernel-tracked containment for the
children whose cleanup Jig must confirm, keep the proof as the fallback, and
stop paying for that proof on short read-only probes.

Acceptance:

- A contained command that starts a `setsid` daemon leaves nothing running when
  Jig reports the command finished, timed out, or was cancelled. Today's
  process-group proof cannot see that daemon.
- Confirming cleanup costs the same on a host with 50 or 10,000 processes.
- Killing Jig with `SIGKILL` mid-command does not leave the contained tree
  running indefinitely (exact guarantee per backend below).
- Hosts without a kernel backend behave as they do today, and every report
  names the backend that ran.

Non-goals: replacing the specialized process ownership in `jig-vault` and
`jig-dev-proxy` (their crate guides forbid it), Windows, and sandboxing
(filesystem or network restriction). Containment here only bounds lifetime.

## Context

After each command, [jig-owned-process](../../crates/jig-owned-process/AGENTS.md)
SIGKILLs the command's process group while the unreaped leader pins the group
ID, then proves the group is empty by scanning `/proc` twice. Linux has no call
that lists a group's members, so each scan visits every process on the host.

A downstream project on a busy Linux host (about 7,700 processes, HPET clock
source) saw intermittent "the process tree could not be cleaned up safely"
from `jig file-budget`. PR #150 made the scan about 10x cheaper (about 17 ms per
scan, about 60 ms per command there) and raised the budget to 2 s. Two
limits remain:

- Cost still grows with the size of the host, not the command.
- A descendant that calls `setsid()` or `setpgid()` leaves the group and is
  never seen. Daemonizing tools do exactly that.

Confirmed cleanup is load-bearing for checks:

- [Cargo resource coordination](../cargo-resource-coordination.md) holds a
  claim "through child cleanup, source postcondition, and durable target-result
  publication" (lines 130-131). A leftover process could change sources after
  the postcondition passed.
- Post-spawn metadata supervision failures, "including unconfirmed cleanup or
  incomplete capture", stop admission (lines 63-65). The
  [v06 plan](../../.agent/plans/v06-cargo-resource-coordination.md) records the
  same rule (line 359) and the lease lifetime (lines 112-118).

So the contained tier keeps a fail-closed guarantee; only short read-only
probes relax it.

## Tiers and call sites

**Probe tier: best-effort.** SIGKILL the pinned group once, reap, do not prove.
These run Git or a version/help probe, write nothing that later checks rely on,
and are called often.

| Call site | Runs | Status |
|---|---|---|
| [source identity](../../crates/jig-repository/src/source_identity/process.rs), [gitlinks](../../crates/jig-repository/src/source_identity/gitlinks.rs) | read-only Git | #151 |
| [doctor toolchains](../../crates/jig/src/doctor/toolchains.rs), [sqlx driver probe](../../crates/jig/src/doctor/sqlx_driver_probe.rs) | version/driver probes | #151 |
| [status Git](../../crates/jig/src/status/git.rs), [doctor run history](../../crates/jig/src/doctor/run_history.rs), [policy Git](../../crates/jig-policy/src/git.rs), [loop `git_is_dirty`](../../crates/jig-loops/src/codex_task.rs) | read-only Git | candidate |
| [doctor proxy list](../../crates/jig/src/doctor/proxy.rs) | `scripts/jig proxy list --json` | candidate |
| [Codex plugin capability probe](../../crates/jig/src/runtime/agent.rs) (`codex_supports_plugin_marketplaces…`) | Codex help probe | candidate |
| [policy schema sandbox](../../crates/jig-policy/src/schema.rs) clone/checkout | Git writing a Jig-private sandbox | candidate, confirm the sandbox is discarded |

**Contained tier: kernel backend, else fail-closed proof.** These run
repository-configured commands, agents, or tools whose descendants can outlive
them and touch the repository or held claims.

| Call site | Runs | Why contained |
|---|---|---|
| [check targets](../../crates/jig/src/runtime/run_execution/target.rs), [command tools](../../crates/jig/src/runtime/tool_execution/command_tool.rs) via [jig-execution](../../crates/jig-execution/src/lib.rs) | configured commands, nextest | source postcondition, receipts, Cargo claims |
| [Cargo discovery](../../crates/jig-repository/src/cargo_discovery.rs), [Cargo resources](../../crates/jig-repository/src/cargo_resources.rs), [Playwright resources](../../crates/jig-repository/src/playwright_resources.rs) | `cargo metadata`, `node` | unconfirmed cleanup must stop admission |
| [schema drift check](../../crates/jig-policy/src/schema.rs) | configured `bash` | repository code |
| [loop workers](../../crates/jig-loops/src/worker_runner.rs), [Codex task preparation](../../crates/jig-loops/src/codex_task/preparation.rs) | agents, `codex sandbox -- …` | long-running, untrusted, spawn helpers |
| [Codex app-server](../../crates/jig-agents/src/codex/app_server.rs), [Codex plugin bootstrap](../../crates/jig/src/runtime/agent.rs) | agent binary | may start MCP servers or Git |
| [dev dependency readiness](../../crates/jig/src/dev_proxy.rs) | repository package-manager shell | repository code; its typed "cleanup unconfirmed" error feeds dev sessions |
| [Claude keychain prompt](../../crates/jig-agents/src/claude/usage/keychain/prompt.rs) | `/usr/bin/security` | macOS only; stays on the process-group path |

## Linux backends, in preference order

### 1. cgroup v2 (preferred)

- **Scope per Jig run, created lazily** on the first contained spawn: call
  systemd's `StartTransientUnit` on the user bus with `PIDs=[getpid()]` and
  `Delegate=yes` (equivalent to `systemd-run --user --scope -p Delegate=yes`,
  about 31 ms measured, once per run). The user manager moves Jig into
  `user@UID.service/app.slice/jig-….scope`, which the user owns.
- **Move Jig into a leaf** (`<scope>/jig`) so per-command siblings can be created.
- **Per command:** `mkdir <scope>/cmd-N`. Open `cmd-N/cgroup.procs` with
  `O_CLOEXEC` before spawning, and in `pre_exec` write `"0"`, which moves the
  writer (verified). This avoids `clone3(CLONE_INTO_CGROUP)`, which `std` does
  not expose. The process-group setup and pinned leader stay as they are.
- **Kill:** write `1` to `cmd-N/cgroup.kill` (Linux 5.14+). It is atomic
  against concurrent forks and catches `setsid` escapes (verified: three
  processes, one in another group, gone in about 4 ms).
- **Confirm:** wait for `populated 0` in `cmd-N/cgroup.events` (inotify on the
  file, which the kernel documents as emitting a modify event, with a bounded
  re-read fallback). Zombies do not count: we observed `populated 0` before the
  killed leader was reaped. Then reap the leader and `rmdir cmd-N`.
- **Identity is a path, not a PID**, so there is no PID-reuse race, and a later
  run can safely find and kill stale `cmd-N` cgroups.
- **Jig killed:** the cgroup survives with its processes. The direct child gets
  the #151 parent-death signal; the next Jig run sweeps `cmd-*` cgroups in
  scopes whose owner is gone. Open question 3 covers the interaction with Cargo
  lease semantics.
- **Not available when:** `/sys/fs/cgroup` is not cgroup2, `cgroup.kill` is
  missing, there is no user bus, or `StartTransientUnit` fails. A plain login
  shell's own `session-N.scope` is root-owned (verified), so systemd's help is
  required.

### 2. bubblewrap PID namespace

- **Launch:** `bwrap --unshare-user --unshare-pid --as-pid-1 --die-with-parent
  --bind / / --dev-bind /dev /dev --proc /proc --info-fd N -- <command>`.
  This is containment only, with no filesystem restriction (verified to run).
- **Omit `--new-session`** so bwrap and the namespace's PID 1 stay in Jig's
  pinned process group. Owned-process children use pipes, not a TTY, so the
  TIOCSTI concern that makes Codex use it does not apply.
- **Teardown:** when PID 1 exits, the kernel kills the rest of the namespace
  and finishes PID 1's exit only after it is empty (verified for a `setsid`
  daemon). `--die-with-parent` kills the namespace if Jig dies.
- **Confirm:** bwrap reaps PID 1 before exiting, so bwrap's exit status implies
  an empty namespace. On timeout or cancel, signal PID 1 (host PID from
  `--info-fd` `child-pid`, verified present) with `pidfd_send_signal(SIGKILL)`
  rather than killing bwrap first, then wait for bwrap. *Unverified:* the
  `pidfd_open` race when PID 1 exits immediately; fall back to "bwrap exited".
- **Costs and semantic changes:**
  - About 22 ms per launch (measured).
  - setuid tools break (`sudo`: "/etc/sudo.conf is owned by uid 65534").
  - The child sees its own `/proc` and PIDs.
  - Per pid_namespaces(7), PID 1 receives only signals it handles, except
    SIGKILL from the parent namespace. That matters only if Jig ever adds a
    graceful SIGTERM.
  - Nested bubblewrap, for example Codex inside a worker, worked here.
- **Not available when:** `bwrap` is missing or lacks `--as-pid-1`, or user
  namespaces are blocked (stock Ubuntu 23.10+ AppArmor). Resolve `bwrap` from
  `PATH` excluding the repository and current directory, as Codex does, so a
  repository cannot plant one.

### 3. Process-group proof (fallback, unchanged)

Today's behavior after PR #150. It stays fail-closed for the contained tier.
macOS always uses its process-group path with `proc_listpgrppids` (membership
cost scales with the group, not the host).

## Availability on Ubuntu 24.04+ and Debian 12/13

| | Ubuntu 24.04 | Debian 12 | Debian 13 |
|---|---|---|---|
| Kernel / systemd | 6.8 / 255 | 6.1 / 252 | 6.12 / 257 |
| cgroup v2 default, `cgroup.kill` | yes | yes | yes |
| Unprivileged user namespaces | AppArmor-restricted on stock installs (unverified here) | allowed (unverified here) | allowed (unverified here) |

| How Jig runs | cgroup | bubblewrap |
|---|---|---|
| Desktop or SSH login session | yes (verified on Pop!_OS 24.04) | if installed and namespaces allowed (verified here, 0.11.0) |
| `sudo`/`su` without a session, cron, system service | no user bus (verified) | as above |
| Default Docker/Podman container | no, cgroupfs read-only (verified) | usually no (unverified) |
| GitHub `ubuntu-24.04` runners | unverified | unverified, likely needs the AppArmor sysctl |

## Selection, override, and reporting

- `JIG_PROCESS_CONTAINMENT=auto|cgroup|bubblewrap|process-group`, default
  `auto`. An explicit backend that is unavailable is an error, not a silent
  fallback. A repository config key is deliberately omitted because
  containment depends on the host.
- `auto` detects once per process and caches the result. It tries cgroup, then
  bubblewrap (one 22 ms probe run, only when cgroup is unavailable), then the
  process group.
- `jig doctor` gains a `process_containment` check: the selected backend and,
  for each skipped one, why.
- Contained-tier errors name the backend, for example "process tree could not
  be cleaned up safely (process-group fallback): …". JSON reports and run
  history record the backend per target.

## Testing and CI

- Backend-specific tests run when their backend is available. A CI job sets
  `JIG_TEST_REQUIRE_CONTAINMENT=<backend>` so an unavailable backend fails
  instead of skipping.
- Shared fixtures for every backend:
  - a `setsid` daemon must be gone at completion (the process-group fallback
    documents it as a known miss);
  - a timeout with a forking tree;
  - the existing output-pipe escape fixtures;
  - a busy-host cost check: confirmation time must not grow with a few
    thousand dummy processes.
- Crash fixture: a helper process runs a contained command, then is SIGKILLed.
  - bubblewrap: the tree is gone within a bound.
  - cgroup: the direct child is gone (parent-death signal), and the next run's
    sweep removes the rest.
- CI jobs. All runner setup steps are unverified:
  - cgroup: `sudo loginctl enable-linger $USER`, export
    `XDG_RUNTIME_DIR=/run/user/$UID`, start the user manager.
  - bubblewrap: `apt-get install bubblewrap` and
    `sysctl kernel.apparmor_restrict_unprivileged_userns=0`.
  - Fallback: the existing jobs plus `JIG_PROCESS_CONTAINMENT=process-group`.
  - macOS jobs are unchanged.

## Rollout

1. #151: parent-death signal for owned children; best-effort
   cleanup for the probe-tier call sites marked #151.
2. Containment abstraction in `jig-owned-process` with only the process-group
   backend, plus the override variable, doctor check, and backend in errors. No
   behavior change.
3. cgroup backend with the scope, per-command cgroups, sweep, and CI job.
4. bubblewrap backend and CI job.
5. Move the contained-tier callers to `auto`, the remaining probe candidates to
   best-effort, and update [process supervision](../process-supervision.md) and
   the crate guide's invariants.

## Risks

- **D-Bus:** `zbus` adds dependencies and build time. Shelling out to
  `systemd-run` instead means re-executing Jig inside a scope (about 31 ms per
  run) and changes the process tree.
- **Cargo claim lifetime changes:** today a surviving child keeps its claim if
  Jig dies. bubblewrap's `--die-with-parent` and the cgroup sweep end that
  child sooner.
- **bubblewrap breaks setuid tools** in contained commands. A command that
  needs `sudo` must use the cgroup or process-group backend.
- **Two to three backends to keep correct;** each must stay covered in CI.

## Open questions

1. D-Bus client crate or `systemd-run` re-exec for the cgroup scope?
2. Keep the contained tier fail-closed on the process-group fallback (proposed:
   yes), or allow best-effort with a warning there too?
3. Should the cgroup sweep kill surviving children of a dead Jig run, changing
   today's "surviving child keeps the claim" behavior?
4. Is an environment variable enough for overrides, or is a user-level config
   needed?
5. bubblewrap: require a system `bwrap` with `--as-pid-1` only, or bundle one
   as Codex does? Ubuntu 24.04's packaged version and flags are unverified.
6. Are the probe-tier candidates and the Codex plugin bootstrap classified
   correctly?
7. Is a CI step that relaxes the AppArmor user-namespace sysctl acceptable?

## Decision log

- 2026-11-03: Keep confirmed cleanup for check targets and resource probes;
  relax only short read-only probes. Reason: the source postcondition and Cargo
  claim rules above.
- 2026-11-03: Prefer cgroups over bubblewrap: no change to the child's view,
  path identity instead of PIDs, and fork-atomic `cgroup.kill`. bubblewrap
  covers hosts without a user bus but with user namespaces.
- 2026-11-03: Omit `--new-session` for bubblewrap so the existing pinned-group
  kill still reaches PID 1.

## Progress

- [x] Survey of Codex's Linux approach and host availability checks.
- [ ] Operator answers to the open questions.
- [ ] Phases 2-5.
