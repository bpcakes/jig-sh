# Built-in scheduling for Jig loops

## Outcome

An explicitly opted-in Git checkout or linked worktree runs its existing scheduled workflows after the terminal closes, while the machine is awake and its user service manager is available. Jig owns calendar evaluation and execution admission; systemd or launchd owns starting, restarting, and stopping the resident process. Foreground `jig loop watch` also works without a service manager.

This Critical implementation plan was prepared on 2026-09-22 against commit `82e6c1c0e0139497ab5e54d10b431695957a9732` (`0.4.1-dev`). Repository references below use that immutable baseline unless marked proposed. The initial worktree was clean. This delivery is planning only: no runtime implementation or host installation.

Completion requires evidence of bounded polling, fresh authority, quiet idle operation, preserved occurrence history, safe contention and shutdown, and host management independent of repository validity. Ordinary tests use fake Codex, not paid model requests.

## Scope

Include typed dispatch reporting, read-only readiness, automatic receipt policy, foreground scheduling, loop admission, parent-death hardening, neutral service infrastructure, durable host registration/runtime copies, Linux/macOS lifecycle adapters, environment diagnostics, upgrade/rebind, and operations documentation.

Exclude a global daemon, repository discovery, another cron parser, workflow-specific OS timers, parallel dispatch, a scheduler retry database, occurrence-schema redesign, root services, automatic linger enablement, arbitrary crontab edits, and exactly-once promises. Existing `loop run` stays bounded and rejects Codex tasks. Unrelated processes can still alter the checkout/journal; preserve existing fail-closed verification of those changes.

## Progress

- [x] Pin baseline; inspect scheduling, receipt, context, worker, CLI, and service boundaries.
- [x] Check official platform/authentication sources and identify runtime proof requirements.
- [x] Draft architecture, dependency graph, acceptance, rollout, and containment.
- [x] Complete focused review, structural validation, and tracker export.
- [x] Run the planning-only Jig verification profile; all six targets passed. Final artifact freshness and closure are authoritative in this plan ID's append-only work records.
- [ ] Implement T-01 through T-10 under separately started implementation work.

Restart checkpoint: planning artifact complete. Inspect this plan ID with `scripts/jig work gates` and `scripts/jig work evidence` for final artifact freshness and closure; do not automatically rerun checks from an older narrative snapshot. Implementation has not started. Jig plan ID: `plan_01M33YCQ8T6MDQPMHFJ70KJRYC`; baseline: the commit above. This task owns only this plan, its Beads execution index, and its append-only planning records; unrelated shared-worktree edits must be preserved. First implementation action: T-01's bounded ownership experiment; T-02 can begin independently. Platform experiments need disposable Linux/macOS environments and are future implementation gates, not prerequisites to delivering this plan.

## Surprises & Discoveries

- Typed `DispatchStep`/`DispatchSummary` already exist: extend them.
- Loop admission must include `run`, `clear-attempt`, and `acknowledge-occurrence`. A loop gate does not isolate unrelated receipt writers.
- Dispatch resolves all workflows up front; a refresh once per pass can still leave later workflow admissions stale.
- Context reload does not validate prompt files. Reuse secure prompt reading for read-only readiness.
- Every current `loop` command is repository-scoped; management of missing/invalid repositories requires early host routing.
- Proxy launcher-liveness protection explicitly excludes killing the owning worker. It is a useful precedent, not proof of scheduler-death cleanup.
- With systemd `KillMode=mixed`, main-process exit can trigger final cleanup before the stop timeout. Keep the scheduler alive until cooperative cleanup finishes.

## Decision Log

- 2026-09-22, D-01 accepted: resident per-checkout scheduler with user-service supervision. Native periodic invocations retain idle/health problems; a global daemon widens scope.
- 2026-09-22, D-02 accepted: preserve calendar, occurrence, checkout verification, and post-start at-most-once semantics; no ledger migration.
- 2026-09-22, D-03 accepted: gate all loop mutations, retain unrelated-writer ambiguity detection. A global receipt gate would require separate reentrancy/lock-order design and is outside this feature.
- 2026-09-22, D-04 accepted: automatic polling has no execution receipt; automatic dispatch records activity/durable transitions. Preserve admitted manual evidence. Busy rejection is an intentional structured, receipt-free behavior change.
- 2026-09-22, D-05 accepted: explicit host registration selects a content-identified durable runtime. Approval follows valid workflow edits in that checkout, not a replacement clone or silently changed executable/environment selection.
- 2026-09-22, D-06 proposed pending T-01: a trusted worker owner/guardian may watch scheduler liveness and use the existing process runner. Prove its placement, start handshake, and admission lifetime before integration. Failure blocks managed release rather than weakening the guarantee.

## Outcomes & Retrospective

Current result: source-grounded design and execution graph. Timing, parent-death behavior, authentication in an actual service session, and native lifecycle remain unverified; tasks below produce that evidence. Structural validation is not implementation proof. Observed planning checks and tracker IDs are recorded at the end before handoff.

## Current-state evidence

Unless fully qualified, `runtime/...` paths are under `crates/jig/src/`. Read nearest crate guides before implementation.

| Kind | Source at baseline | Consequence |
| --- | --- | --- |
| Fact | `runtime/loops/schedule/cron.rs:48`, `ScheduleSpec`; `schedule/tests.rs` | Reuse existing cron/timezone, coalescing, canonical identity, and DST tests. |
| Fact | `runtime/loops/schedule.rs:55,69,111` | Workflows resolve once, execute sequentially, and every completed dispatch receipts. One successful idle pass/minute yields 1,440/day or 525,600/365-day year (arithmetic, not measurement). |
| Fact | `runtime/loops/schedule/policy.rs:12,24`; `engine.rs:44` | Extend existing typed dispatch/completion types; JSON remains presentation. |
| Fact | `runtime/loops/codex_task/checkout.rs:518`, `expected_receipt_append` | The protected suffix permits only the expected worker receipt. Never weaken it to hide interference. |
| Fact | `crates/jig/tests/cli_json_parts/loop_commands.rs:437`, `repo_worker_rejects_a_nested_receipt_writing_jig_command` | Nested receipt-producing Jig commands already cause attention; preserve this regression. |
| Fact | `runtime/loops.rs:37`; `engine/maintenance.rs:74,140`; `schedule.rs:720` | Central loop admission must cover CLI/MCP tick, dispatch, run, and recovery mutations. |
| Fact | `runtime.rs:180`, `refreshed_repository_context`; `context/loading.rs:4`; `runtime/loops/codex_task.rs:326` | Context refresh rejects contract-version changes; prompt validation requires separate secure reading. |
| Fact | `runtime/loops/occurrence/persistence.rs:79`; `state.rs:551`; `occurrence.rs:641` | Read-only snapshots exist; malformed attempt-cache repair differs from invalid lease/ledger authority; stale running work becomes attention. |
| Fact | `runtime/loops/authority.rs:18,36` | Worktree-specific Git authority and common repository authority are distinct. |
| Fact | `runtime/worker_runner.rs`, `build_codex_command`, observer and worker receipts | Preserve provider-home/model/sandbox/approval, output, cancellation, and evidence semantics through this runner. |
| Fact | `crates/jig-owned-process/src/process.rs:671` and crate `AGENTS.md` | Workers use separate process groups; cleanup requires retained direct-child identity, not stored PIDs. |
| Fact | `crates/jig-dev-proxy/src/processes/cleanup/launcher.rs`, `DevLauncherWatch`, and crate guide | Liveness socket precedent exists; owning-worker SIGKILL is explicitly outside its guarantee. |
| Fact | `crates/jig-dev-proxy/src/service/`; `crates/jig/Cargo.toml:40` | Existing service helpers are proxy-specific and optional. Extract neutral code without introducing a dev-proxy dependency. |
| Fact | `crates/jig/src/cli/run.rs:159,443`; `cli.rs` launcher constants | Host service routing must bypass repository parsing/validation when managing an instance by ID. |
| Inference | Separate worker groups plus Apple's same-group cleanup documentation | launchd settings alone do not prove worker cleanup. T-01/T-05 require real platform evidence. |
| Unknown | Worst-case cleanup/publication duration; actual supported native-manager behavior | T-05/T-07/T-08 measure this; 90 seconds is a proposed stop budget only. |

Official sources checked 2026-09-22: [systemd kill semantics](https://raw.githubusercontent.com/systemd/systemd/main/man/systemd.kill.xml) specify main-process SIGTERM and cgroup-wide final cleanup for `mixed`, including escalation on main exit. [Service restart semantics](https://raw.githubusercontent.com/systemd/systemd/main/man/systemd.service.xml) distinguish explicit manager stops from automatic restart. [loginctl](https://raw.githubusercontent.com/systemd/systemd/main/man/loginctl.xml) documents linger across logout/boot. These upstream sources do not establish minimum supported installed versions; adapters must check capabilities.

[Apple's agent guide](https://developer.apple.com/library/archive/documentation/MacOSX/Conceptual/BPSystemStartup/Chapters/CreatingLaunchdJobs.html) scopes user agents to logged-in users. [Apple's launchd manual source](https://raw.githubusercontent.com/apple-oss-distributions/launchd/main/man/launchd.plist.5), `AbandonProcessGroup`, describes same-group cleanup. These historical sources require current host validation. [Codex authentication](https://learn.chatgpt.com/docs/auth) supports file and OS credential-store caching; service-session access needs a bounded probe without copying secrets.

## Decisions and design

### Scheduling, freshness, and evidence

Use wall time for `ScheduleSpec` and interruptible monotonic waits. Evaluate at startup, then wait until a future schedule boundary or 60 seconds, whichever comes first. The target is evaluation within 60 seconds plus measured local evaluation overhead for an awake, healthy, idle scheduler, not exact task-start timing. Sequential workers can delay later work. After dispatch/sleep read time afresh; never drain accumulated ticks. Keep existing coalescing and recovery semantics.

Proposed internal `DispatchReport` extends current policy types: evaluated time, next due, action/worker outcomes, coordination transitions, deferral/attention, revision changes, optional receipt references. Preserve admitted manual JSON fields and receipt linkage.

Proposed `PollDecision` is `Idle(next_check)`, `DispatchNeeded(reason)`, or `Blocked(diagnostic)`. Readiness inspects workflow/prompt authority and occurrence/lease snapshots without creating directories, claims, recovery writes, or receipts. Missing pristine state is empty; missing protected ledgers are not. Classify expired occurrences and repairable attempt-cache corruption as reconciliation work; malformed leases, divergent replicas, invalid prompts/configuration, and missing protected authority block. Include removed-workflow stale records where existing recovery permits reconciliation. Recheck snapshots and claim under existing rules after admission; polling is never execution authority.

Automatic dispatch records worker activity, meaningful attempts/failures, and durable coordination transitions. Repeated identical idle/busy/blocked states produce no receipts, including after restart. Use existing durable facts and bounded in-memory diagnostic keys, not a scheduler-owned execution ledger. Stable preflight failures stay health diagnostics and cannot become per-minute dispatch attempts. Use a future recheck time for an overdue blocked workflow; never sleep until its past due time. Mixed blocked/actionable workflows must not starve one another.

Refresh `RepoContext`, workflows, and secure prompt validation before each automatic pass. Before every subsequent workflow admission detect changes to execution authority, including prompt contents, and abort/re-poll instead of using stale resolved workflows. Preserve current repository-revision cutoff. Each running task keeps its captured context and existing in-flight cancellation/authority checks. Invalid configuration or missing prompt pauses admissions with precise sanitized diagnostics; no last-known-good fallback. Contract-version mismatch latches restart-required even if later edits appear valid.

### Lock ownership and shutdown

Use two owner-only OS locks in worktree-specific Git metadata: a scheduler singleton for watch lifetime, and a separate nonblocking admission gate for all loop mutations. Resolve paths using existing authority/Git-path helpers. Sibling worktrees have distinct singleton/admission locks while existing common-repository branch leases remain authoritative. Do not unlink a live lock inode.

Acquire admission before mutation/receipt publication for dispatch, tick, run, clear-attempt, acknowledge-occurrence, and automatic dispatch. Status/polling remain read-only. Busy returns `ok:false`, `status:"busy"`, actionable context, no receipt, and nonzero CLI exit; MCP retains a structured tool result. Internal ticks receive a private typed capability instead of reacquiring; no public bypass flag/environment variable. Lock order: singleton (watch only), admission, existing short transactional locks. Admission spans worker start, checkout verification, and permitted finalization; no transactional lock is held for daemon lifetime.

CLOEXEC prevents singleton/admission/liveness descriptors reaching Codex. T-01 must also prevent admission becoming free while an orphaned guardian-owned worker is still alive: trusted-supervisor ownership transfer or an equivalent tested lifetime protocol is required. Unrelated work/check/bootstrap/state commands and older runtimes remain outside this gate and can force attention under the unchanged checkout verifier. A global journal lock across a worker invoking Jig would risk deadlock and is not part of this plan.

SIGTERM/Ctrl-C stops admission, propagates cancellation through existing `ExecutionControl`/signal supervision, completes bounded cleanup and permitted finalization, publishes final local health, then releases ownership. Keep the main process alive through cleanup. Task failure alone is not daemon failure; unproved cleanup blocks further work and retains ambiguity.

The guardian candidate must directly own/reap the worker via `jig-owned-process`, survive scheduler death long enough to clean up, and hold only its required private descriptors. Only the scheduler holds the liveness endpoint whose closure signals death; no worker descendant may inherit it. Specify bounded result transport, private startup, and a handshake preserving the existing durable started boundary before external work is released. Death between spawn and acknowledgment cannot allow an unrecorded worker to execute and later replay. Test immediate restart during old cleanup and guardian placement outside launchd's scheduler-group cleanup. Never signal persisted PIDs/PGIDs after restart. Guardian loss itself needs an explicit tested containment boundary; a chain of guardians cannot guarantee arbitrary simultaneous process death.

### Host commands, runtime, and registration

Proposed CLI: `loop watch`; `loop service install --dry-run`; `install --accept-service-scope`; `status --json`; `start`, `stop`, `enable`, `disable`, `restart`, `uninstall`, `upgrade`, and `rebind`. Host commands accept `--instance ID`; add `service list --json` for discovery outside a checkout. Watch/service are CLI-only and stay outside MCP; existing MCP loop mutations share admission.

Install registers, enables, and starts; unchanged reinstall is a no-op. Start leaves autostart unchanged; if a manager cannot start a persistently disabled instance without changing that setting, return a structured refusal with explicit enable guidance. Do not silently enable it. Stop unloads/stops for this session without disabling future autostart. Enable changes future startup without implicit immediate execution; disable stops and prevents startup. Restart preserves enablement. Uninstall removes only owned registration/service artifacts after shutdown and preserves receipts, occurrence history, and retained worktrees. Report installed, enabled, loaded, and running independently.

Install warns that an eligible missed occurrence may run immediately. Dry-run does not create directories/copies, invoke auth or workers, claim occurrences, or mutate manager state. Show exact paths, executable selection, non-secret environment, scope, startup behavior, ownership/compatibility checks and outstanding service-session probes.

Route host management before `RepoContext` and generated-launcher repository validation. Host `jig ... --instance ID` must work from an unrelated directory after the repo disappears or becomes malformed. The service invokes an absolute managed runtime via a private instance-run entrypoint and stable host working directory, then validates registration before entering the checkout; a native WorkingDirectory pointing at a missing repo would prevent useful diagnostics.

Proposed host root: `~/.jig/loop-service/`, resolved explicitly rather than by arbitrary inherited overrides. Versioned registration records opaque ID/generation, canonical checkout, worktree Git/filesystem identity, private Git-metadata marker, owner UID, runtime version/digest/features, backend, and approved non-secret environment/executable selections. Path/remotes/HEAD alone cannot identify a replacement clone. Same-owner malicious filesystem modification is outside this boundary. Use owner-only no-follow storage, bounded parsing, atomic durable publication and a host mutation lock. Content-identified runtime copies are application-immutable, not protected against the user's deliberate replacement.

Upgrade stages a compatible runtime, stops admission and proves shutdown, atomically changes registration/manager generation, then restores prior enablement/running intent where the manager permits it; an unsupported disabled-but-running restoration remains stopped with an explicit diagnostic, never silently enabled. Preserve previous compatible generations for repair. Rebind explicitly approves changed checkout path/identity and never copies or invents history in a replacement clone. Rebind/scope changes require dry-run plus `--accept-service-scope`; ordinary approved lifecycle actions do not. Unknown schema/incompatible runtime blocks. On partial failure report actual state and roll back only owned transaction files; never silently downgrade incompatible state.

Extract a proposed `jig-service` crate for neutral specs, rendering, safe file publication, bounded manager invocation, and status parsing. Keep proxy routing/state in `jig-dev-proxy`, and loop registration/scheduling in `jig`. Add workspace/release wiring and a crate guide. Preserve proxy public behavior and tests; watch/services compile without default features.

### Platform, environment, and health

Linux: user service with `Type=exec`, `Restart=always`, restart delay/rate limit, `UMask=0077`, `KillMode=mixed`, and measured `TimeoutStopSec` (90 seconds initially). No timer. Check user-manager reachability/directive support. Explain session versus linger behavior without enabling linger or escalating privilege.

macOS: per-user LaunchAgent in `gui/UID`, absolute literal ProgramArguments, RunAtLoad, KeepAlive, throttle, measured ExitTimeOut, no calendar/interval keys/root daemon. Distinguish persistent disabled state from unloaded/stopped. Prove explicit stop is not undone by KeepAlive. Missing GUI domain is actionable; supported promise is logged-in user operation.

Resolve absolute Codex executable, deliberate PATH/HOME, and supported workflow-specific Codex homes. Preserve sandbox/approval policy. No shell startup files or full environment copies; scrub stale launcher/repository selectors from host entry. Unattended stdin must not prompt/hang; use the runner's explicit prompt pipe where needed. Changed executable/home selection requires explicit registration update/rebind; valid workflows using approved selections reload normally.

A bounded provider-owned auth-status probe runs in the actual service context before first admission and for newly selected approved homes. Missing executable/home/keyring blocks the affected workflow with remediation. Installer probes do not prove service-session access. Verify the installed Codex probe capability rather than assuming a command here. Never persist credentials/raw auth output. Auth-cache status is not proof of model-request success.

Atomic versioned health outside tracked receipts records instance/generation, installed/running runtime, manager/autostart state, last evaluation/next due, active workflow/occurrence, execution phase/progress time, blocked reason, and sanitized failure. Separate supervision progress from transcript output and heartbeat-thread liveness. Stale health means unknown/unresponsive, not authority to kill a PID. Use bounded metadata logs (proposed 3 × 1 MiB per instance), no prompts/transcripts/full environments/tokens, and no unbounded native stdout file bypass.

## Execution graph

Each task gets one implementation owner when claimed. Maintainer owns release decisions; operator owns installation approval. Dependencies below are authoritative. Main prerequisite chains: T-02 → T-03 → T-04 and T-01 → T-05; T-06 supplies host infrastructure. They converge before adapters and release. These are dependency paths, not elapsed-time estimates.

### T-01 — Prove scheduler-death worker ownership
- Outcome: A bounded Linux/macOS experiment selects a safe lifetime design or explicitly blocks managed release.
- Context: D-06, separate process groups and retained direct-child identity invariants.
- Changes: Generic disposable fixtures/prototype only; record chosen guardian placement, handshake, and admission lifetime here before production integration.
- Depends on: none
- Verify: Fake Codex with live grandchild; kill scheduler before spawn, during handshake, after start, during cleanup; immediately restart. Test foreground/native manager, descriptor inheritance, owner loss, and exact cleanup evidence. Record OS/manager versions. Limit spike to one lifecycle prototype per platform, not a full scheduler.
- Recovery: Remove only experiment-owned resources; unproved design leaves managed operation unavailable.
- Done when: Reproducible traces prove cleanup/no replay/no admission gap, or a concrete failure and revised safe design gate are documented before T-05. A failed prototype is evidence, not permission to ship.

### T-02 — Expose typed, quiet schedule readiness
- Outcome: Idle/unchanged-blocked automatic checks write no execution receipts or launch workers; meaningful evidence remains complete.
- Context: Existing policy types/manual JSON and read-only occurrence snapshots.
- Changes: `runtime/loops/schedule.rs`, `schedule/policy.rs`, proposed `schedule/poll.rs`, occurrence snapshot interfaces and schedule/receipt regressions.
- Depends on: none
- Verify: Existing calendar/DST/coalescing tests; simulated day idle/blocked including restart with exact journal-byte/count and launch assertions. Cover stale claims, removed workflows, malformed repairable cache versus invalid authority, mixed eligible/blocked tasks, pre-start failures and actual execution failures. Compare manual output fixtures.
- Recovery: Retain manual dispatch fallback and unchanged occurrence schema/calendar rules.
- Done when: Typed report drives presentation, readiness performs no writes, and automatic meaningful transitions retain receipt references without stable-state growth.

### T-03 — Serialize all loop mutations
- Outcome: Loop contenders return structured busy before protected-window receipt writes, without recursive deadlock.
- Context: D-03; unrelated writers still trigger existing attention behavior.
- Changes: `runtime/loops.rs`, engine/schedule/maintenance capability plumbing, Git-metadata lock helper, CLI/MCP contention regressions.
- Depends on: T-02
- Verify: Separate processes contend through tick/dispatch/run/recovery and MCP; assert zero contender receipts, nested tick progress, error release, sibling-worktree independence and branch leases. Preserve nested-bootstrap ambiguous-receipt test. Check CLOEXEC.
- Recovery: Never remove live locks/reset claims to break contention; preserve short transaction leases and verifier.
- Done when: Every loop mutation enters one gate and manual success/busy output/exit semantics are covered.

### T-04 — Run a fresh, interruptible foreground scheduler
- Outcome: `loop watch` repeatedly evaluates with bounded waits, fresh authority, singleton ownership, health, and graceful cancellation.
- Context: Per-pass refresh alone is insufficient for sequential workflow admission.
- Changes: `cli/loops.rs`, `command/loops.rs`, CLI signal/execution adapters, proposed `runtime/loops/scheduler.rs`, authority checks, health/log helpers and fake clocks.
- Depends on: T-02, T-03
- Verify: Startup, forward/backward clock jumps, simulated suspend, long task, no polling bursts, blocked backoff, duplicate watch, idle/active SIGTERM and bounded logs. First of two workers edits/disables second or corrupts prompt/config: second must not start stale. Contract mismatch latches; revision cutoff remains.
- Recovery: Preserve ambiguity on interrupted started work. Do not expose managed installation/claim hard-death containment before T-05.
- Done when: Poll target passes fake-time tests, invalid authority never admits, and foreground builds/tests pass without default features.

### T-05 — Integrate proven parent-death cleanup
- Outcome: Scheduler death/ordinary stop preserve worker ownership, admission lifetime, and post-start at-most-once evidence.
- Context: T-01's protocol must preserve existing runner output, cancellation, receipts, and start boundary.
- Changes: `jig-owned-process` primitives, `runtime/worker_runner.rs`, scheduler supervisor/private entrypoint, execution observer glue and fault-injection tests; generic code stays repository-independent.
- Depends on: T-01, T-04
- Verify: Death before/after claim/start/exit/receipt/finalization; full pipes, quiet worker, descendants, guardian loss, immediate restart, manager escalation and cancellation races. Prove no leaked endpoints; measure cleanup/publication against initial 90-second budget.
- Recovery: Unproved cleanup blocks admission and preserves attention; no stored-PID signaling or automatic replay.
- Done when: Actual Linux/macOS traces prove declared failure boundary, started occurrences never replay, and stop budget includes measured safety margin. Unavailable platform is an outstanding gate, not a passing skip.

### T-06 — Share services and register durable host instances
- Outcome: Loop services have safe repository-independent identity/runtime storage; proxy behavior remains compatible.
- Context: Proxy service code is optional/fixed-label; loop routing currently needs a valid repo.
- Changes: Proposed `crates/jig-service`, workspace/release wiring/guide, proxy adapters, Jig host registry/runtime store, early CLI/launcher classification/private instance routing and MCP exclusions.
- Depends on: none
- Verify: Proxy regression suite/no-default build; host list/status with missing/malformed repo; replaced clone/sibling worktree; digest/compatibility; unknown schema, wrong owner, symlink/parent substitution, partial writes, concurrent generations, non-UTF8 rejection and escaping. No native loop installation yet.
- Recovery: Retain proxy public semantics and old runtime generations; refuse foreign artifacts, never touch execution ledgers.
- Done when: Neutral APIs have no repository/proxy dependency, registry is readable without RepoContext, and staging fails closed with truthful partial state.

### T-07 — Deliver systemd lifecycle and background preflight
- Outcome: Explicitly approved Linux installs run/control scheduling with accurate scope and service-session diagnostics.
- Context: Proven T-05 cleanup and T-06 registry; manager capabilities vary.
- Changes: Linux renderer/adapter, Jig install/start/stop/enable/disable/restart/uninstall transactions, shared environment/auth preflight, isolated native tests.
- Depends on: T-05, T-06
- Verify: Strictly read-only dry-run; real user-manager lifecycle, absent bus/unsupported directives, linger reporting, repeated install, partial failure, restrictive permissions. Paths with spaces/quotes/percent/dollar/backslash/non-ASCII. Missing executable/home/keyring/minimal env, redacted output and main SIGKILL with live descendants. Remove source build and prove durable executable still works.
- Recovery: Stop/disable on readiness failure, retain repair state/evidence, no privilege escalation/automatic linger.
- Done when: Native lifecycle and cgroup evidence pass, warning explains immediate-due execution, and foreground still works without systemd.

### T-08 — Deliver launchd lifecycle with verified containment
- Outcome: Approved macOS agents schedule in documented user scope and remain controllable without repository parsing.
- Context: Shared lifecycle/environment interfaces come from T-07; same-group cleanup is not sufficient proof.
- Changes: macOS renderer/adapter, GUI-domain checks, persistent enable/disable/session stop semantics and isolated native tests.
- Depends on: T-05, T-06, T-07
- Verify: Real GUI-domain install/start/stop/disable/restart/uninstall; KeepAlive cannot undo stop. Test disabled → start/restart and disabled-but-running upgrade restoration explicitly: preserve disabled state across logout or refuse without changing it. Missing domain, bootstrap/bootout failures, encoding/permissions, service keyring, and scheduler death with live grandchildren. Enabled state is independent of loaded/running.
- Recovery: Boot out only owned labels; uncertain manager state cannot justify evidence deletion or a cleanup claim.
- Done when: Lifecycle and T-05 containment pass on declared supported macOS; no root daemon/OS schedule is installed.

### T-09 — Upgrade and rebind without losing authority
- Outcome: Runtime upgrades/repository moves are explicit, resumable host transactions with preserved history.
- Context: D-05; missing/invalid repo must not prevent management; incompatible schema cannot be downgraded.
- Changes: Upgrade/rebind CLI, generation transactions, runtime selection/retention and operator repair diagnostics.
- Depends on: T-07, T-08
- Verify: Move/remove/replace repo, invalid config, upgrade while active, crash after every manager/publication step, interrupted rebind, incompatible runtime/schema and preserved enablement. Compare history/retained evidence; failed mutation cannot automatically retry work.
- Recovery: Previous compatible generation or stopped/blocked forward repair. Never synthesize history or silently rebind.
- Done when: By-ID management works throughout failures; repeated mutations converge without duplicate instances, implicit scope expansion or evidence loss.

### T-10 — Validate native operation and publish opt-in rollout
- Outcome: Users migrate external schedules to Jig with demonstrated supported lifecycle and recovery behavior.
- Context: Safety blockers are release gates, not follow-up chores.
- Changes: `docs/codex-task-operations.md`, `docs/platform-support.md`, configuration/CLI help, native/release tests and generic fixtures; generated guidance only where scheduling is described.
- Depends on: T-09
- Verify: Full feature/no-default suites/repo gates; native logout/login, reboot, sleep/wake, terminal-close, upgrade/rebind on declared hosts. Migrate a harmless fixture after disabling old dispatch; verify occurrence identity/attention, bounded logs/zero idle receipts. Record versions/exclusions.
- Recovery: Stop/disable and prove quiescence before restoring compatible external dispatch; preserve history and attention.
- Done when: All release blockers have evidence, docs match actual commands, and maintainer approves the supported-platform declaration.

Initially independent: T-01, T-02, T-06. T-01 owns isolated experiments; T-02 owns schedule code; T-06 owns neutral services/host registry. Serialize T-06/T-04 edits to shared CLI/workspace files. T-08 follows T-07 because it consumes shared lifecycle/environment semantics. Do not concurrently mutate tracker state, common native labels, or unstable interfaces.

## Verification

From repository root, use existing focused commands: `cargo test -p jig-sh schedule`, `cargo test -p jig-sh codex_task`, `cargo test -p jig-owned-process`, `cargo test -p jig-dev-proxy`. Add scheduler filters once tests exist and inspect selected counts; zero tests is not proof. Preserve `runtime/loops/schedule/tests.rs` calendar/coalescing/lease/attention regressions; extend `runtime/tests/loops/scheduled_failures.rs`, `scheduled_attention_regressions.rs`, `codex_task/checkout_tests.rs` and CLI/MCP regressions.

Run `cargo test -p jig-sh --no-default-features` and `cargo build -p jig-sh --no-default-features`. Changed source/launcher validation uses `scripts/jig-dev check contract` and `scripts/jig-dev --json info`. Open new structured implementation work against its actual baseline; use `scripts/jig work check --plan-id ID`, inspect `work gates`/`work evidence`, and finish backend work with `scripts/jig check test`. These are future implementation checks, not tests claimed for this planning change.

Planning verification: run planning-workflow's `scripts/validate_plan.py` with `--profile critical`, review G1–G6, verify tracker edges/ready roots, `python3 scripts/beads-sync.py --check`, `git diff --check`, and this planning work's applicable Jig gates. Use only generic fixtures such as ExampleProject. Record observed results at the end.

Release blockers: duplicate started occurrences; an unrecorded worker escaping the start handshake; stale admission; uncontrolled idle/blocked receipts; loop contenders modifying protected receipt windows; evidence deletion by service lifecycle; unproved scheduler-death behavior; or manager liveness reported as application readiness.

## Rollout and recovery

Ship internal refactors with manual compatibility coverage, then foreground plus process hardening, then opt-in services. Do not enable a platform's installation before its gates pass. Operator approves persistence; maintainer approves release. Registration/health are versioned; occurrence history stays unchanged.

Older runtimes do not understand admission. Migration requires disabling old cron/timers and confirming no old dispatch remains; mixed-version scheduling is unsupported. Existing claims do not imply receipt isolation. Operator sequence: inspect/disable old schedule, wait for or cancel its work, dry-run, review scope/immediate-due warning, approve installation, inspect service health/next due, validate a harmless fixture. Detect recognizable registrations where practical, but never rewrite arbitrary crontabs/user units.

Contain incidents by stop/disable, prove owner cleanup, inspect health and existing loop status, then use explicit occurrence recovery where appropriate. Never clear attention to make health green. Uninstall preserves execution/retained-worktree evidence. Restoring external dispatch requires compatible runtime and quiescent managed instance; incompatible state requires forward repair.

## Risks and open decisions

| Risk / unknown | Owner and resolution gate | Safe default |
| --- | --- | --- |
| Guardian killed with launchd job or admission released early | T-01/T-05 owner, before managed release | No release; retain uncertainty. |
| Wider journal gate appears necessary | T-03 owner/maintainer, before expanding scope | Preserve verifier/unrelated-writer limitation; explicit replan for wider isolation. |
| Insufficient runtime/build or checkout identity | T-06 owner, registry acceptance | Block and explicitly repair/rebind; never trust path/HEAD alone. |
| Freshness misses prompt/home authority | T-04/T-07 owner, reload/environment tests | Block new admissions, no cached fallback. |
| Poll/stop budgets fail measurement | T-05/platform owners, native gates | Revise budgets/docs from evidence before release. |
| Minimum manager/OS support and GUI/headless behavior | T-07/T-08 owners and maintainer, T-10 | Capability rejection; untested platform remains blocked. |
| Auth probe succeeds but real request fails | T-07/T-08 owner, diagnostics | Report cached auth only; preserve worker failure without replay. |
| Stable blocked/config failure grows CPU/logs/receipts | T-02/T-04 owner, day/restart tests | Bounded rechecks/rotating diagnostics, no receipt flood. |

Implementers may choose reversible module/helper/clock details locally. Changing occurrence semantics, approval scope, identity guarantees, supported death boundary, or platform gates requires a recorded material plan revision.

## Tracker index and planning validation

Beads is the execution index; this file is canonical. Issues preserve T-ID, plan link, focused acceptance/recovery and native dependency edges. Preparing this plan does not claim or close implementation tasks. Mapping and observed planning verification follow after export.

Epic: `jig-sh-jaop`. All ten delivery tasks remain open/unclaimed.

| Plan task | Beads issue |
| --- | --- |
| T-01 | `jig-sh-jaop.1` |
| T-02 | `jig-sh-jaop.2` |
| T-03 | `jig-sh-jaop.3` |
| T-04 | `jig-sh-jaop.4` |
| T-05 | `jig-sh-jaop.5` |
| T-06 | `jig-sh-jaop.6` |
| T-07 | `jig-sh-jaop.7` |
| T-08 | `jig-sh-jaop.8` |
| T-09 | `jig-sh-jaop.9` |
| T-10 | `jig-sh-jaop.10` |

Observed planning validation (2026-09-22): Critical-profile structural validator passed with 10 tasks, zero errors/warnings. G1–G6 self-review and independent fresh-executor review of ownership/host lifecycle found no remaining blocker/material defect after clarifying disabled-service start/upgrade behavior. Re-read every exported issue and blocker edge; all matched this plan, acceptance/recovery remained intact, and ready roots were exactly T-01, T-02, T-06. `br dep cycles --json` reported no cycles. Beads export was synchronized through the privacy helper. First Jig work check (`run_01M33YZVRBV5120Q6K732NDN3B`) was rejected by effect policy because Beads export changed `.beads/issues.jsonl` during the read-only run. All underlying command targets exited zero and file-budget reported zero findings, but this is not valid completion evidence. The settled-file rerun `run_01M33ZP7RH6YGRXRMWF5F1Q5WV` passed all six targets: api:clippy, api:fmt, api:test, repo:contract, repo:file-budget, repo:source-runtime-check. `work gates` reported overall passed, fresh evidence, and no missing/failed required gates. `work evidence` was inspected. Plan validation, export privacy check, and diff whitespace checks passed. These results validate the planning delivery/current baseline; scheduler and native lifecycle acceptance remains future work.

Final evidence handling: the released harness fingerprints this narrative as well as source. Updating the report after successful validation made the previous target evidence stale, despite no source changes. The final attempted validation ran without edits from this task, but concurrent unrelated planning/Beads changes appeared in the shared checkout and invalidated the global execution proof. Final validation/closure is recorded in append-only Jig state. Treat that structured state as authoritative for the final check result and planning completion, without changing the future implementation status above.

Final observed result (2026-09-22): the last suite ran 4,528 tests, all passed, with four skipped; Clippy, formatting, contract, and source-runtime command checks exited zero, and file-budget had zero findings. The enclosing work gate rejected this run with `execution_mutated` and a `.beads/issues.jsonl` source race. Unrelated plan/document files also appeared during the run; they were preserved. Planning delivery and tracker export are complete, but structured work remains open because `work finish` requires fresh evidence on a stable shared checkout. Do not claim that closure succeeded. No further full rerun was attempted while concurrent changes were occurring. Once the checkout is stable, inspect `scripts/jig work gates --plan-id plan_01M33YCQ8T6MDQPMHFJ70KJRYC --json`, refresh only what its guidance requires, then finish the planning work.
