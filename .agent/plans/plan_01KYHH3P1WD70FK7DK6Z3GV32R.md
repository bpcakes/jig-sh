# Manage, stop, and safely replace Jig development sessions

This ExecPlan is a living document. The sections `Progress`, `Surprises & Discoveries`, `Decision Log`, and `Outcomes & Retrospective` must remain current while work proceeds. Maintain this document in accordance with `.agent/PLANS.md`.

## Purpose / Big Picture

After this change, a developer who encounters a hidden or abandoned `jig dev` run can inspect it with `jig dev status`, stop every development session owned by the current repository with `jig dev stop`, or replace only conflicting sessions from the same canonical repository with `jig dev --replace`. Jig will no longer require routine manual PID discovery for sessions it registered.

The safety boundary remains conservative. A route hostname alone does not authorize process termination because multiple repositories and ad-hoc proxy commands share the default proxy state directory. Jig records a private session identity containing the canonical repository identity, exact supervisor identity, planned apps, exact spawned app identities, route ownership, and an authenticated local control endpoint. Replacement is allowed only after that registry proves the conflicting session belongs to the same canonical repository. A different-repository or legacy unregistered process remains protected from automatic termination.

A human can observe the result by launching a fixture `jig dev`, running `jig dev status --json` from the same repository, stopping it with `jig dev stop`, and verifying that the supervisor, child process groups, routes, and registry entry disappear. Repeating the launch and running `jig dev --replace` must stop the conflicting registered session before the replacement publishes its route. Tests must also prove that a PID start-token mismatch, a cross-repository hostname collision, and an unattributed legacy route are never signaled.

## Progress

- [x] (2026-07-27 10:19Z) Inspected the clean worktree, repository and crate guides, existing signal/process-group cleanup, route storage, CLI parser/dispatch, human output, and test boundaries.
- [x] (2026-07-27 10:19Z) Opened structured work as `plan_01KYHH3P1WD70FK7DK6Z3GV32R` and wrote this self-contained implementation plan.
- [x] (2026-07-27 11:12Z) Implemented the versioned, private, repo-scoped dev-session registry and authenticated supervisor control endpoint.
- [x] (2026-07-27 11:12Z) Implemented status, graceful supervisor-owned stop, explicit cleanup confirmation, retained orphan evidence, and safe same-repository conflict replacement.
- [x] (2026-07-27 11:12Z) Added the optional-subcommand CLI, neutral command DTOs, human/JSON output, contextual conflict diagnostics, and no-feature behavior.
- [x] (2026-07-27 12:03Z) Added state-security, control-authentication, cleanup-accounting, CLI/output, same-root alias/selection, cross-repository, legacy-route, orphan, no-feature, and real-process lifecycle coverage.
- [x] (2026-07-27 11:12Z) Updated public documentation, command lists, and release notes; the final invariant audit remains.
- [x] (2026-07-27 13:08Z) Reproduced and fixed the full-suite signal regression caused by post-publication session-state locking; the complete `dev_sigint` integration now passes 9/9 and an independent lifecycle review found no remaining high- or medium-severity issue.
- [ ] Build the development binary, dogfood all relevant Jig checks, inspect receipts/gates, and complete the requirement-by-requirement audit.

## Surprises & Discoveries

- Observation: the existing route document cannot prove repository or session ownership.
  Evidence: `crates/jig-dev-proxy/src/types.rs::Route` stores hostname, target, optional owner PID/start token, mode, and creation time only.
- Observation: normal foreground cleanup is already deliberately stronger than an external numeric-PID kill.
  Evidence: `crates/jig-dev-proxy/src/processes/child_lifecycle.rs` retains an unreaped direct `Child`, pins its process-group generation, and performs platform-specific bounded confirmation. Persisted PID data must not pretend to provide that same proof after the supervisor dies.
- Observation: Jig already identifies the foreground supervisor in process listings as `jig dev --jig-project=<name>@<root>`, but that hidden argument is presentation metadata rather than a durable ownership record.
  Evidence: `crates/jig/src/cli/run.rs::exec_dev_with_process_identity`.
- Observation: current `jig proxy prune` filters only dead process routes, while `jig proxy stop` manages the shared proxy listener rather than development app processes.
  Evidence: `crates/jig-dev-proxy/src/lib.rs::proxy_prune`, `proxy_stop`, and `docs/configuration.md`.
- Observation: one canonical repository may intentionally run multiple disjoint `--app` selections at once.
  Evidence: app selection is per launch in `resolve_dev_request`; replacement must therefore stop only sessions whose planned app names or route hostnames overlap the requested launch, while plain `jig dev stop` may stop all sessions for the current repository.
- Observation: a persisted PID/start token cannot safely replace the live supervisor's unreaped child handle for external Unix signaling.
  Evidence: between any final token check and `kill(pid)` or `kill(-pgid)`, the process may exit and the numeric identity may be recycled; the existing `child_lifecycle` implementation deliberately pins process-group generation with an unreaped `Child`. The initial orphan-signaling prototype was removed after adversarial review.
- Observation: process cleanup can legitimately exceed a fixed eight-second management wait.
  Evidence: each child has bounded TERM, force, and confirmation phases, cleanup retries process trees, and route cleanup shares a 30-second lock deadline. The control retirement wait now scales without an artificial cap at a 35-second base plus 15 seconds per app in the largest concurrently targeted session.
- Observation: app names are repository-local identities, while hostnames are shared proxy identities.
  Evidence: two repositories routinely both configure an app named `web`; cross-repository claims now conflict only on hostname, with focused unit and CLI integration coverage.
- Observation: the session registry and route registry intentionally share one lock, so a session-phase write after publishing a route can block the foreground supervisor behind an observer that acquired the route lock.
  Evidence: the first full `scripts/jig work check` exposed four deterministic `dev_sigint` failures. Three stalled at the old post-publication `mark_running` mutation before child monitoring; the fourth completed forced route-cleanup cancellation but then stalled in non-cancelable session retirement.

## Decision Log

- Decision: preserve bare `jig dev` as the launch action and add optional `status` and `stop` subcommands plus a launch-only `--replace` flag.
  Rationale: existing scripts and muscle memory continue to work, while the management surface is discoverable as `jig dev status` and `jig dev stop`. Clap's optional-subcommand shape can reject launch flags when a management subcommand is present.
  Date/Author: 2026-07-27 / Codex
- Decision: persist a versioned `dev-sessions.json` document under the selected Jig proxy state directory and protect it with the same owner-only, symlink-hardened, lock-protected atomic-state rules as routes.
  Rationale: session state is mutable machine-local runtime state, not append-only repository memory. The state directory already supplies the correct trust boundary and configurable isolation.
  Date/Author: 2026-07-27 / Codex
- Decision: represent one launch as one session record and keep the registry plural.
  Rationale: different `--app` selections from one repository can coexist. Status and stop must not assume a singleton.
  Date/Author: 2026-07-27 / Codex
- Decision: identify a repository by a canonical-root identity derived from the platform path bytes, while retaining a display form in status output.
  Rationale: matching must treat symlink spellings of one repository as one owner without depending on lossy display text or requiring every valid filesystem path to be Unicode.
  Date/Author: 2026-07-27 / Codex
- Decision: use a random authenticated loopback control endpoint as the primary supervisor-stop mechanism.
  Rationale: the running supervisor owns the unreaped child handles and platform process-tree resources needed for the strongest cleanup. A private random token plus exact session registry entry allows `stop` and `--replace` to ask that supervisor to execute its normal cleanup without relying on a racy external group signal, and it works when an invoking agent has lost the original terminal.
  Date/Author: 2026-07-27 / Codex
- Decision: never use persisted numeric PIDs as external signal authority after the authenticated supervisor is unavailable.
  Rationale: even a matching start token is only an observation, not a race-free handle. The supervisor owns the unreaped children and platform cleanup resources; if it cannot complete cleanup, Jig records `cleanup_required`, reports the session as orphaned, returns `ok: false`, and retains the evidence without signaling.
  Date/Author: 2026-07-27 / Codex
- Decision: make replacement same-canonical-repository and app-or-hostname-overlap scoped.
  Rationale: `--replace` should resolve the common hidden-session conflict without stopping unrelated app subsets or a process belonging to another repository. Unregistered legacy/ad-hoc routes remain manual recovery cases.
  Date/Author: 2026-07-27 / Codex
- Decision: keep the session registry compatible by treating a missing file as an empty registry, accepting only version 1 initially, and failing closed on malformed or unknown-version state.
  Rationale: existing installations have no file to migrate. Silently discarding corrupt ownership data could authorize an unsafe replacement or hide processes.
  Date/Author: 2026-07-27 / Codex
- Decision: make every foreground session-state wait cancellation-aware, publish the running phase before any app route becomes externally visible, and allow only repeated-signal forced cleanup to abandon a contended retirement write.
  Rationale: the supervisor must remain able to observe the first termination request and clean its owned children even when another process holds the shared route lock. On a later force request, retaining conservative cleanup-required evidence is safer than blocking exit or falsely claiming cleanup. The replacement stop engine also carries cancellation through its state waits and polling loop.
  Date/Author: 2026-07-27 / Codex

## Outcomes & Retrospective

The registry, authenticated control plane, CLI lifecycle actions, same-repository replacement, contextual conflicts, structured output, documentation, and focused end-to-end test are implemented. Adversarial review changed the original orphan fallback: no raw PID or PGID signal remains because persisted observations cannot pin a process generation. The implementation now carries an explicit cleanup-required marker and removes a session only after the foreground owner confirms dependency-preflight, app process-tree, and route cleanup. The first full repository gate exposed a shared-lock signal regression; foreground registry writes are now interruptible, the running transition precedes route publication, and forced retirement retains evidence instead of blocking. The complete signal and lifecycle integrations pass, and the final independent review found no high- or medium-severity findings. Full repository gates remain.

## Context and Orientation

`crates/jig-dev-proxy` owns the local reverse proxy and foreground app supervision. `src/types.rs` defines public request and route shapes. `src/state.rs` and its submodules own owner-only mutable files under `~/.jig/proxy` or an explicit state directory. `src/processes.rs` launches all selected apps as one tied foreground session. On Unix each app starts a new session/process group whose leader PID is the direct child PID. `src/processes/child_lifecycle.rs` and `src/processes/cleanup.rs` terminate and confirm those groups during ordinary exit or signals. `src/lib.rs` exposes same-version internal APIs to the main CLI crate.

`crates/jig` owns CLI parsing, repository configuration, output, and dispatch. `src/cli/proxy.rs` currently models `jig dev` as launch arguments only. `src/cli/run.rs` re-executes a launch with a hidden process-list identity. `src/command/proxy.rs` contains transport-neutral command DTOs. `src/dev_proxy.rs` translates repository configuration into `jig-dev-proxy` requests. `src/cli/output.rs` renders the JSON values returned by the runtime.

A “canonical repository identity” in this plan means an identifier calculated from the filesystem's canonical root path after symlinks are resolved. A “start token” means a platform process-creation identity that changes when a numeric PID is reused. A “control endpoint” means a loopback-only listener opened by the foreground supervisor; management clients must present the random token stored in the private session record before the listener requests normal shutdown. An “orphan” is a registered app whose exact process identity remains alive after its registered supervisor identity or control endpoint is gone.

The new registry is current ownership state, not history. Normal launch completion removes its exact session record. Status reconciles records into `running`, `orphaned`, or `stale` observations without signaling anything. Stop is idempotent when no matching session exists. Fully dead records that never armed cleanup may be removed under the session-state lock; cleanup-required records remain until the owning supervisor confirms cleanup.

## Plan of Work

First, extend `crates/jig-dev-proxy/src/types.rs` so a resolved launch retains the canonical repository identity and the `replace` choice. Add public status and stop requests that contain the current repository name/root and selected state directory. Add an internal session-state module under `crates/jig-dev-proxy/src/state/` with versioned Serde records for the session, supervisor, planned apps, optional spawned app owners, route coordinates, control address, and token. Reuse the state directory's regular-file validation, bounded locking, atomic replacement, backup recovery, file-size caps, and owner-only permissions. Generate unpredictable session IDs and control tokens with `getrandom`; never include the control token in returned JSON or human output.

Second, add a small control module in `crates/jig-dev-proxy/src/processes/` or a focused top-level module. It binds only `127.0.0.1` on an ephemeral port, services a narrow authenticated stop request, records the request atomically, and shuts down cleanly with the foreground supervisor. The control token comparison must not leak useful prefix information. Compose this stop intent with the existing signal interruption probe so preflight, app readiness, the steady-state child loop, route operations, and cleanup all observe it. Extend the normalized dev result so a management-requested stop is a successful stopped state rather than a synthetic Unix signal failure.

Third, register the planned session only after termination handling and the control listener are active, but before dependency preflight or child spawn. Persist each spawned direct child identity immediately, before readiness can block, and update its route/port data when known. If registration or update fails after a child exists, use the existing owned `Child` cleanup before returning the error. Hold a generation-specific session guard for the entire launch and remove only its own record on normal completion, startup failure, configured child exit, or ordinary signal cleanup. Do not hold the session-state lock while acquiring route/certificate locks or waiting for processes.

Fourth, implement a read-only status API and one shared stop engine. Status canonicalizes the requested root, selects every record with the same repository identity, verifies supervisor and child start tokens, checks owned route presence, classifies each session, and returns a stable same-version diagnostic JSON document with secrets omitted. Stop snapshots matching records, sends an authenticated control request to every reachable supervisor, waits for normal retirement using the foreground cleanup budget, and then reconciles. If the authenticated supervisor is unavailable or cleanup remains unconfirmed, do not signal persisted numeric PIDs; retain the cleanup-required entry, return `ok: false`, and describe the incomplete cleanup without exposing the token.

Fifth, run the same stop engine before a launch marked `replace`, restricted to registered sessions from the same canonical root whose planned app names or proxied hostnames overlap the requested launch. After stop succeeds, launch normally and retain the existing route preflight as the final concurrent-race guard. A different-repository record, an unattributed process route, an identity mismatch, or an incomplete stop must abort replacement without launching new children.

Sixth, change the route conflict diagnostics in `crates/jig-dev-proxy/src/state.rs`. When session attribution proves a same-repository owner, recommend `jig dev stop` or `jig dev --replace`. When attribution proves another repository, state that replacement refuses cross-repository ownership and identify the non-secret owner display root. When no session safely attributes the live route, say that `--replace` will not terminate it and retain the manual process-stop plus `jig proxy prune` recovery.

Seventh, add the optional-subcommand CLI. Split launch arguments from `DevOpts`, add `DevSubcommand::{Status, Stop}`, add a launch `replace` flag, and use `args_conflicts_with_subcommands` so launch flags cannot be mixed with management actions. Convert this to `command::DevCommand::{Launch, Status, Stop}` and dispatch each action explicitly. Only launch re-executes with `--jig-project`. Give status and stop focused progress and human renderers while preserving existing launch JSON fields and summary behavior.

Finally, document the lifecycle and test it at state, process, CLI, output, feature-gate, and integration levels. Add an unreleased changelog entry. Keep source files under the repository policy limit by adding focused modules rather than extending the already-large process and state files.

## Concrete Steps

All commands run from `/Users/aa/Documents/jig-sh`.

1. Maintain this plan after every material design or implementation discovery. Edit source only with `apply_patch`, then format:

       cargo fmt --all

2. Iterate on the state and process owner:

       cargo test -p jig-dev-proxy state
       cargo test -p jig-dev-proxy dev_session
       cargo test -p jig-dev-proxy processes

3. Iterate on the CLI and integration owner:

       cargo test -p jig-sh cli::
       cargo test -p jig-sh dev_proxy
       cargo test -p jig-sh --test dev_sessions
       cargo test -p jig-sh --test dev_sigint

4. Build the development runtime before any dogfooding command:

       cargo build -p jig-sh --bin jig
       export JIG_DEV_BIN=target/debug/jig

5. Run focused policy and quality checks:

       JIG_DEV_BIN=target/debug/jig scripts/jig check fmt
       JIG_DEV_BIN=target/debug/jig scripts/jig check clippy
       JIG_DEV_BIN=target/debug/jig scripts/jig check contract
       JIG_DEV_BIN=target/debug/jig scripts/jig check agent-guides
       JIG_DEV_BIN=target/debug/jig scripts/jig check agent-map
       JIG_DEV_BIN=target/debug/jig scripts/jig check rust-file-loc

6. Run the required full test and structured work gates:

       JIG_DEV_BIN=target/debug/jig scripts/jig check test
       JIG_DEV_BIN=target/debug/jig scripts/jig work check --plan-id plan_01KYHH3P1WD70FK7DK6Z3GV32R
       JIG_DEV_BIN=target/debug/jig scripts/jig work gates --plan-id plan_01KYHH3P1WD70FK7DK6Z3GV32R
       JIG_DEV_BIN=target/debug/jig scripts/jig work evidence --plan-id plan_01KYHH3P1WD70FK7DK6Z3GV32R
       JIG_DEV_BIN=target/debug/jig scripts/jig work receipts --plan-id plan_01KYHH3P1WD70FK7DK6Z3GV32R

7. Review the final diff, update this plan's outcome and evidence, then close structured work only when every required gate is fresh:

       JIG_DEV_BIN=target/debug/jig scripts/jig work finish --plan-id plan_01KYHH3P1WD70FK7DK6Z3GV32R

## Validation and Acceptance

Bare `jig dev` with every existing flag must parse and behave exactly as before. `jig dev --replace` must remain a launch. `jig dev status [--state-dir PATH]` and `jig dev stop [--state-dir PATH]` must parse, while combinations such as `jig dev --app web status`, `jig dev stop --replace`, or proxy-listener flags on management commands must fail at argument parsing.

While one fixture session is starting, ready, or running, status from the same canonical repository must report one running session, its non-secret supervisor identity, planned apps, spawned app identities when present, route coordinates, and effective state directory. A symlink spelling of the same root must select the same session. Status from a different root must not claim it. A missing registry must return `ok: true`, `running: false`, and an empty session list.

Stop with no matching sessions must exit successfully and say that nothing was running. Stop with one or more sessions from the current root must request each supervisor's ordinary cleanup, wait for all tied child trees, remove exact owned routes and registry entries, and return success. An injected dead supervisor with cleanup-required state, a start-token mismatch, or an unverified identity must never receive a signal and must return incomplete cleanup with `ok: false`.

Replacement must stop only same-root sessions whose planned proxied hostnames overlap the new selected apps. A disjoint same-root `--app` session must survive. A cross-root session using the same hostname, an unregistered legacy process route, or a registered identity that cannot be stopped must block replacement. After successful replacement, only the new session owns the hostname and no old route or registry entry remains.

Normal child exit, startup failure, dependency-preflight failure, SIGINT, SIGHUP, SIGTERM, externally requested stop, and route-publication failure must each retire the exact session record and preserve existing process/route cleanup behavior. A hard-killed supervisor may leave a cleanup-required orphan record; status must classify it as requiring attention and stop must fail closed without converting numeric PID observations into signal authority.

JSON output must contain no control token or raw configured command. Human output must distinguish launch, status, successful/idempotent stop, and incomplete stop. A non-default-feature build must still parse all three actions and return the existing clear “without the dev-proxy feature” error.

The complete `jig-dev-proxy` and `jig-sh` tests, format, strict Clippy, contract, agent guides/map, Rust file-length policy, full repository tests, and required work gates must pass with the newly built `target/debug/jig`.

## Idempotence and Recovery

Status is read-only except for optional removal of records proven fully stale under the session lock. Stop is idempotent: repeating it after success finds no matching owner and performs no signals. Session and route removal always compare the complete persisted ownership tuple, so an old cleanup cannot remove a newer replacement.

If a launch fails after registering but before spawning, its guard removes the empty session. If it fails after spawning, the existing `Child`-backed cleanup runs before the guard retires the record. If a stop client loses its connection after the supervisor accepted the token, retrying stop observes the same record or its completed removal and remains safe.

If `dev-sessions.json` is malformed, too large, a symlink, or has an unknown version, management and replacement fail without signaling. The user may inspect and move that private state file aside manually, but Jig must not silently overwrite untrusted ownership state. Missing pre-feature state is an empty registry and needs no migration.

The `.agent/state/*.jsonl` files are append-only work memory. Do not edit or truncate existing records. New plan, session, decision, and receipt records produced by Jig commands are expected.

## Artifacts and Notes

Initial evidence:

    git status --short
    # clean before work start

    scripts/jig doctor
    Jig doctor: ready

    scripts/jig work status
    Plans: 0 open

The initial structured-work command created this plan and append-only plan/session/receipt entries:

    scripts/jig work start \
      --title "Manage and replace Jig dev sessions" \
      --body "..." \
      --print-plan-id
    plan_01KYHH3P1WD70FK7DK6Z3GV32R

## Interfaces and Dependencies

In `crates/jig/src/cli/proxy.rs`, define an optional `DevSubcommand` with `Status` and `Stop`. Keep launch arguments in a dedicated structure and add `replace: bool`. In `crates/jig/src/command/proxy.rs`, define `DevCommand::{Launch(DevRequest), Status(DevStatusRequest), Stop(DevStopRequest)}`. `RuntimeCommand::Dev` must carry that enum.

In `crates/jig-dev-proxy/src/types.rs`, extend `DevRequest` with `replace` and retain canonical root/name in `ResolvedDevRequest`. Define explicit management requests that provide repository root/name and an optional state directory without accepting irrelevant HTTP, HTTPS, LAN, or TLD flags.

The internal version-1 session document must contain:

    version
    sessions[]
      session_id
      repo_name
      repo_root_display
      repo_root_identity
      started_at_ms
      cleanup_required
      supervisor.pid
      supervisor.start_token
      control.port
      control.token
      apps[]
        name
        hostname
        target_host
        target_port
        process.pid
        process.start_token

Optional owner/port fields represent planned or starting apps. The persisted control token is sensitive and must remain inside the owner-only state file. Returned JSON maps internal records to redacted observation DTOs.

The process owner must expose one shared stop engine used by both `dev stop` and `--replace`. It must separate snapshot/decision, authenticated control request, cleanup-budget-aware wait, and final registry reconciliation so no state-file lock is held during network or process operations. Persisted process observations are diagnostic only after the supervisor is unavailable.

No new external service or daemon is introduced. Existing dependencies (`getrandom`, `serde`, `serde_json`, `sha2`, `subtle`, `fs4`, `libc`, and platform APIs already used by the crate) are sufficient.

Plan revision note (2026-07-27 10:19Z): replaced the initial one-line work body with a self-contained lifecycle, safety, compatibility, implementation, and validation plan after auditing the existing route, state, process, CLI, and documentation boundaries.
