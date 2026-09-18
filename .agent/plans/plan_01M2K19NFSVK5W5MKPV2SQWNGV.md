# Add the optional Beads tracker adapter and workspace configuration

This ExecPlan is a living document. Keep `Progress`, `Surprises & Discoveries`,
`Decision Log`, and `Outcomes & Retrospective` current while executing it. Maintain it
in accordance with `.agent/PLANS.md`.

The outcome is an opt-in boundary from Jig to an external `br` binary. A configured
repository can discover its exact repository-local Beads store and obtain a normalized
snapshot of one exact issue through a bounded subprocess. The same crate-private adapter
provides typed comment, claim, and close primitives for later epic tasks, without
exposing those mutations through today's CLI or MCP commands. A repository without
`[work.tracker]` behaves exactly as before and does not need `br` installed.

Acceptance is observable in configuration, adapter, and Doctor tests: old `.jig.toml`
files still load; a configured generic fixture cannot be redirected to another database;
real `br 0.5.7` response shapes decode into allowlisted types; malformed, oversized,
timed-out, cancelled, failed, and unknown-profile invocations are distinct; write-capable
operations cannot start until discovery and storage-readiness checks pass; Doctor
diagnoses the adapter without initializing, importing, exporting, or mutating Beads data;
and required repository checks pass through the just-built Jig binary.

## Progress

- [x] (2026-09-15) Claimed `jig-sh-x8ow.2`, synchronized its sanitized export, and
  opened structured plan `plan_01M2K19NFSVK5W5MKPV2SQWNGV` at Git baseline
  `38a9624efee45f64e4346b43e772fc670c7442ac`.
- [x] (2026-09-15) Read repository/crate guidance, the epic design, strict config model,
  bootstrap reconciliation, owned-process APIs, Doctor composition, and representative
  local `br 0.5.7` JSON shapes.
- [x] (2026-09-15) Completed independent read-only configuration and process/Doctor
  implementation audits.
- [x] (2026-09-15) Milestone 1: implemented and verified strict optional tracker
  configuration, canonical authority representation, accessors, and bootstrap-update
  preservation, including the previously dropped `receipt_metadata` field.
- [x] (2026-09-15) Milestone 2: implemented the bounded adapter, explicit 0.5.7 profile,
  repository-local store validation, normalized snapshots, typed failures, and all six
  future-facing operations.
- [x] (2026-09-15) Milestone 3: added read-only Doctor diagnostics and documentation
  without a dependency or side effect for unconfigured repositories.
- [x] (2026-09-16) Milestone 4 verification: ran focused checks, inspected the cumulative
  diff, completed the approved bounded review/fix convergence, and passed every configured
  structured gate plus the required final backend test command.
- [ ] Task close, plan finish, and commit remain separate maintainer actions; this review
  did not infer authority to perform them.
- [x] (2026-09-16) Repaired the comprehensive-review findings as one boundary redesign:
  retain one immutable executable snapshot, make completion win over late cancellation,
  retry transient store-snapshot drift without calling it corruption, normalize all
  mutation ambiguity at the process boundary, and make invalid authored work metadata
  block update instead of disappearing.
- [x] (2026-09-16) Repaired the follow-up comprehensive-review findings at their owning
  boundaries: restricted provider discovery to external absolute-path authority, made
  conflicting `error` members invalid for every success shape, bounded mutation argv,
  documented and regression-tested fresh-per-operation store snapshot semantics, and
  closed the optional-work and supported-platform test gaps.
- [x] (2026-09-16) Resolved the final review findings by preserving the fixed public
  operation budgets while adding phase-specific executable and snapshot errors, bounded
  snapshot-retry backoff, PATH candidate-local recovery, and a definitive ambiguous-ID
  outcome.
- [x] (2026-09-16) Completed a same-fingerprint Claude/Codex re-review, replaced the
  provider's finite environment denylist with namespace isolation plus identity allowlist,
  made the no-DB discovery exception explicit, aligned hard-link/platform diagnostics,
  and documented the external CLI's non-atomic readiness handoff without weakening the
  required no-auto-import policy.
- [x] (2026-09-16) Repaired the final portability findings at the authority boundary:
  separated live-path namespace from retained inode identity, validated the full live
  database family around mutations, retained a private named executable snapshot on
  macOS, moved unsupported-platform diagnosis ahead of provider lookup, and corrected
  the public contract. Focused suites pass on Linux and macOS, and real macOS `br 0.5.7`
  accepts the corrected canonical-path mutation handoff.
- [x] (2026-09-16) Repaired the final multi-model findings by moving optional work-authority
  reconciliation into the shared staging boundary, making discovery no-DB from the version
  probe onward, deriving Doctor remediation from typed failures, and distinguishing unsafe
  hard links from generic database-boundary failures.

Restart checkpoint: the review repairs and verification are complete. Tracker-focused
suites pass on Linux and macOS. The final exact-tree Jig gate and required standalone
backend test both pass all 4,186 tests (three skipped).
Task close, plan finish, and commit remain explicit maintainer actions and were not
performed by this review.

## Surprises & Discoveries

- `WorkConfig` and the containing repository config already reject unknown fields.
  A defaulted optional field preserves legacy loads, while omitting `None` during
  serialization preserves authority digests for repositories that did not opt in.
- `bootstrap/runtime_config.rs::reconcile_work` preserves checks, gates, and refinements
  but omits both tracker and existing `receipt_metadata`. Preserve both validated
  ownership declarations; T8, not T2, owns generating a new opt-in.
- The crate guide's `src/process.rs` pointer is historical. Generic bounded process-tree
  ownership is in `jig-owned-process`; the adapter can consume its fatal output-overflow
  API without changing that crate.
- The installed profile is `br 0.5.7`. Read-only probes showed object responses for
  `--no-db where --json` and `sync --status --json`, and arrays for `show ... --json` and
  `comments list ... --json`. This establishes only the 0.5.7 profile.
- T1 accepts a broader historical workspace identity contract in durable journals. T2
  must not retroactively tighten those readers when requiring canonical ULIDs in new
  configuration.
- `br 0.5.7` emits a structured `closed`/`skipped` stdout payload before returning the
  generic `NOTHING_TO_DO` error for an all-skipped close. Because the adapter invokes
  close for one exact issue, a matching single skipped entry with no closed entries is
  a definitive missing/blocked outcome; malformed or mixed output remains an
  indeterminate write.
- Independent review established that explicit `--db` does not disable Beads' separate
  issue-prefix routing and that effective `no-db`/startup-cache behavior remains
  configurable through higher-precedence environment and project layers. The adapter
  therefore rejects route/redirect/town routing artifacts, forces `BD_NO_DB=false`,
  disables startup caching, and removes the read-only-fast-open opt-out.
- The reusable adapter originally retained only the resolved executable path plus the
  discovered profile. A binary replacement at that path could therefore run under stale
  0.5.7 approval. The resolved executable now carries a bounded SHA-256 and stable file
  metadata identity; each call verifies the retained source while copying its bytes to an
  immutable anonymous executable snapshot, then launches that snapshot.
- A canonical descendant can still alias storage outside the repository through a hard
  link. Database and existing JSONL leaves now require exactly one filesystem link on
  supported hosts, with the check repeated around profiled operations.
- A successful add-comment payload originally correlated only the issue ID. It now must
  echo the exact requested actor and text or the already-started mutation is reported as
  indeterminate rather than falsely acknowledged.
- The retained executable descriptor still allowed an in-place writer to change verified
  bytes before launch. Calls now copy and verify the selected binary into a sealed or
  otherwise private anonymous executable snapshot before spawning it.
- Executable hashing and JSONL copying originally happened outside the advertised process
  timeout. Discovery and each profiled call now use one deadline and cancellation budget
  across resolution, hashing, snapshot preparation, provider execution, and cleanup.
- Revalidating the configured database pathname immediately before launch does not make
  the provider's later open atomic. Read operations therefore receive private snapshots;
  mutations receive the canonical live database pathname while a retained descriptor
  witnesses source identity before and after the provider call.
- Linux `/proc/self/fd/<n>` happened to preserve a usable database namespace, but macOS
  `/dev/fd/<n>` does not: real `br 0.5.7` failed while deriving its adjacent write-lock
  pathname even when the descriptor was writable. A real macOS probe confirmed that the
  canonical live path supports the same mutation. The external CLI exposes no atomic
  expected-inode handoff, so descriptor-as-namespace was false portability, not stronger
  security.
- `strace` established that real `br info` opens source lock/SQLite/WAL/SHM artifacts
  writable, so it cannot implement Doctor's observational contract. Discovery now uses
  `br --no-db where`, and sync status receives a verified private copy of the complete
  0.5.7 database family as well as private JSONL. A real trace showed no writable source
  store opens and identical source file/mtime identity before and after that sequence.
- The provider canonicalizes an inherited database-descriptor spelling before it acquires
  its database-family authority. The retained descriptor narrows replacement exposure but
  cannot hand an expected inode into the 0.5.7 lock protocol. The public contract now
  states the exact boundary: persistent changes are detected, cooperating provider writers
  are serialized by `br`, and adversarial same-user swap-and-restore races are not claimed.
- Mutation readiness initially received a separate read budget. Each public mutation now
  creates one deadline covering readiness, snapshotting, the write, and final validation.
- The provider's 0.5.7 file-state health classifier also consumes a legacy `.beads.lock`
  timestamp. Private readiness snapshots now include that file when present and preserve
  source modification times, so a stale-lock anomaly cannot disappear before the
  fail-closed mutation gate.
- Retaining and sealing the executable bytes did not prevent inherited platform loader
  variables from injecting other code at launch. Every tracker invocation now removes the
  Linux and macOS loader-injection variables already recognized by Jig's hardened runtime
  launch policy.
- On macOS, `/dev/fd/<n>` cannot be executed as the Linux `/proc/self/fd/<n>` snapshot is.
  The macOS executable snapshot now uses a private named file, closes every writable
  handle, reopens the real path read-only, verifies the access mode, and retains the
  private directory and pathname through launch.
- A deadline bounded snapshot duration but not temporary disk consumption, especially for
  sparse source files. Readiness now reserves a 512 MiB aggregate logical-byte budget for
  the JSONL and complete database family before copying and rejects growth beyond each
  captured file length.
- Opening an unvalidated FIFO sidecar with ordinary `File::open` can block the parent before
  another deadline checkpoint. Executable and store authority inputs now use nonblocking,
  no-follow, close-on-exec opens before their descriptor/path regular-file checks.
- Real 0.5.7 already-closed output is two JSON documents on stdout, not a skipped-result
  stdout plus error-envelope stderr. The close classifier and fixtures now require the
  exact skipped-result/`NOTHING_TO_DO` stdout sequence with empty stderr; nonexistent IDs
  retain their separate single `ISSUE_NOT_FOUND` response.
- FrankenSQLite's migration bookkeeping uses the distinct dot-suffixed
  `<db>.fsqlite-migration-state` name rather than the dash-suffixed family convention.
  Readiness snapshots now retain that file and its source timestamp. Private snapshot
  placement also canonicalizes the ambient temp root and refuses it when it is inside the
  repository, while loader sanitization removes the complete `LD_*` and `DYLD_*`
  namespaces instead of a finite list of known keys.
- Real `br 0.5.7` read commands open the selected SQLite family with write-capable flags
  and can recreate SHM/opener artifacts. `show` and `comments list` now use verified
  private database-family copies, so only mutation operations receive the retained live
  database descriptor.
- Recognizing an error envelope from either output stream without constraining the other
  stream could turn a conflicting success/error mutation response into a definitive
  rejection. The profile now requires exclusive one-document framing and its code-specific
  retryability. Mutation option values use `--name=value` so a leading hyphen remains data.
- Stream exclusivity alone was insufficient because a partial error object or a single
  object containing both success and error fields could still be treated as definitive.
  The shared profile decoder now requires the sole top-level `error` and all five real
  0.5.7 fields; close no-op uses the same decoder and requires `retryable: false`.
- The tracker test file crossed the repository's 800-line hard budget during the boundary
  regressions. Its reusable fake-provider fixture was split into a sibling module without
  changing test behavior.
- A full Doctor unit-test run encountered one transient `ETXTBSY` in the existing proxy
  executable-replacement test; that test passed immediately in isolation. The full gate
  rerun remains the deciding evidence.
- Successful mutation framing still ignored stderr, and claim/close parsing admitted
  unobserved wrapper objects. Success now requires whitespace-only stderr, raw one-item
  claim/close arrays, and no conflicting `error` member in the acknowledged result.
- The manual-export readiness exception accepted duplicate `db_newer` anomalies and an
  inconsistent anomaly severity. It now requires the exact observed single degraded
  `db_newer` anomaly in addition to the existing flag and health checks.
- The first full configured gate run passed 4,166 tests, Clippy, formatting, and contract
  validation but exposed nine lines of legacy file-budget debt growth in Doctor test part
  08. Moving its shared `check_by_id` helper into the small fixture-only part 09 retires
  that growth without changing test behavior.
- Close no-op classification still ignored extra fields inside the first of its two
  documents, and readiness ignored a top-level `error` beside healthy fields. The pinned
  close decoder now requires the exact observed three-field result and two-field skipped
  entry, while readiness rejects a contradictory error before interpreting health.
- Executable snapshots still inherited Bash startup, option, tracing, directory, and
  exported-function controls, which can execute outside hashed script bytes. Tracker
  launches now reuse Jig's shared shell-environment sanitizer and retain ordinary identity
  variables; a Bash-provider regression poisons every supported control namespace.
- The final review findings are not seven unrelated omissions. `OperationBudget` was used
  after process completion even though its cancellation error means "before start";
  `ResolvedExecutable` retained source authority but rebuilt immutable execution authority
  on every call; and the store copier represented both hostile boundary replacement and
  ordinary SQLite sidecar churn as `InvalidWorkspace`. Those type/boundary mismatches
  caused the timeout, cancellation, performance, and misleading-diagnostic symptoms.
- A file-by-file copy can be a coherent observational SQLite snapshot only when the
  durable database and WAL family remain stable across the copy. The shared-memory
  `-shm` file is rebuildable coordination state and should not be copied. Ordinary source
  drift must trigger bounded recapture, while inode/path/type violations remain workspace
  boundary failures.
- The optional JSONL path is intentionally allowed to be absent, but readiness passed an
  absent private path to the provider. The private snapshot must materialize an empty
  read-only export for that valid state.
- Executable byte integrity is insufficient when discovery provenance is unconstrained.
  Resolving empty or relative `PATH` elements against the repository promoted
  checkout-controlled bytes into the trusted 0.5.7 profile; even an absolute `PATH`
  entry can point back into the checkout. External provider authority must therefore
  require an absolute entry whose canonical executable is outside the repository.
- Successful response validation had grown parser by parser, allowing version, workspace,
  and issue reads to accept a contradictory `error` member after mutations/comments had
  been tightened. The profile-wide invariant is that no supported success shape can also
  report an error.
- Reusing a private database snapshot is not a safe answer to its linear copy cost: real
  0.5.7 can mutate the private database it opens, and reuse could expose provider-modified
  or stale state. Fresh per-operation generations are the observational/freshness boundary;
  the aggregate byte cap and deadline bound cost, and the contract must state that this is
  linear work rather than imply the 512 MiB safety ceiling is a latency promise.
- The remaining snapshot-scaling concern was not evidence that the fixed deadline or
  fresh-copy architecture was wrong: both are explicit task guarantees. The actual gap
  was that deadline exhaustion during local snapshot preparation was reported as though
  the provider process had timed out, leaving no actionable diagnosis.
- Initial executable selection and post-discovery identity validation shared one
  `ExecutableChanged` error even though they imply different recovery. Candidate-local
  capture failures may be bypassed by a later `PATH` entry; immutable-snapshot facility
  failures are host-wide; only retained-source replacement means rediscovery is required.
- Immediate retry of a changing SQLite family repeated the same observation window. A
  small bounded backoff improves convergence without extending the public deadline or
  weakening required Doctor readiness.
- The pinned external CLI acquires its database-family lock internally and exposes no
  generation precondition or inherited-lock handoff. Readiness and mutation therefore
  cannot form one serializable provider epoch without replacing the external-adapter
  boundary. A parent-side lock would deadlock the child when it reacquires the lock, while
  enabling auto-import violates the explicit manual policy. The honest boundary is
  preflight plus the epic's persisted-intent/post-write reconciliation workflow, not an
  atomicity claim in T2.

## Decision Log

- Decision: support exactly observed `br 0.5.7`; unknown versions are reportable but
  cannot read or mutate. Rationale: upstream semver compatibility is not promised and no
  other real binary was exercised. Date: 2026-09-15.
- Decision: use a strict internally tagged `[work.tracker]` with only `kind = "beads"`,
  canonical uppercase ULID `workspace_id`, fixed `.beads` root, default/only export mode
  `manual`, and optional bounded nonblank display-only guidance. Rationale: selection is
  explicit and portable, with no executable helper or external-root channel. Date:
  2026-09-15.
- Decision: omit an absent tracker and canonicalize omitted/explicit `manual` export to
  the same representation. Rationale: legacy authority digests must not change, and
  omitted/default manual means the same configured authority. Date: 2026-09-15.
- Decision: keep adapter APIs crate-private and add no T2 work command, MCP field, or
  automatic mutation call site. Rationale: T3/T4/T6/T7 own workflows and reconciliation
  semantics. Date: 2026-09-15.
- Decision: discovery uses repository-root cwd, shell-free argv, narrowly sanitized
  routing variables, and no implicit import/flush. Accept only canonical regular
  database/export paths inside exact canonical `<repo>/.beads`; pass the validated DB
  explicitly thereafter. Date: 2026-09-15.
- Decision: never automatically retry mutations. Require a supported profile and
  structured read-only storage-readiness result first. Manual export may legitimately
  report only `db_newer = true`; reject JSONL-newer, sync conflict, and authority-health
  anomalies. Any process failure after spawn is indeterminate until the owning future
  workflow rereads the issue. Date: 2026-09-15.
- Decision: cross-project Beads routing is incompatible with a configured exact
  repository-local tracker authority. Reject local route/redirect files and active
  ancestor town route tables before discovery and around each profiled operation;
  route-aware multi-workspace orchestration is not inferred. Date: 2026-09-15.
- Decision: force version and workspace discovery into no-DB mode with `BD_NO_DB=true`,
  then force only store-backed profile operations into DB-backed mode with
  `BD_NO_DB=false`; disable the optional provider startup cache throughout. Rationale:
  the observational contract begins with the first provider invocation, while
  user/project/ambient Beads settings still must not redirect store-backed source of
  truth. Date: 2026-09-16.
- Decision: bind profile discovery to both canonical executable path and bounded
  executable identity (content digest plus stable metadata), retain its opened file, and
  execute later calls through an immutable anonymous snapshot made from reverified bytes.
  Reject replacement before preparation; in-place writes after verification cannot alter
  the snapshot selected for launch.
  Rationale: a reusable adapter must not grant an upgraded or swapped binary the
  previously discovered 0.5.7 capabilities. Date: 2026-09-15.
- Decision: require the database and existing JSONL export leaves to have one filesystem
  link on supported hosts. Rationale: path containment and symlink rejection alone cannot
  exclude an outside hard-link alias of the same store inode. Date: 2026-09-15.
- Decision: give read operations private database-family snapshots. Give mutations the
  canonical validated live database pathname because the provider needs its containing
  namespace for adjacent locks and temporary files; retain an open database descriptor
  solely as an identity witness and validate the live family before and after the call.
  For readiness, copy the durable database family and retained JSONL bytes to private
  files and pass the latter with
  `sync --allow-external-jsonl --status`; remove `BEADS_JSONL` for every issue operation.
  Rationale: Doctor/readiness must be observational, and the 0.5.7 external-JSONL opt-in
  is command-specific. A descriptor cannot supply the pathname namespace required by
  macOS `br 0.5.7`, and the external CLI offers no atomic expected-inode handoff. Date:
  2026-09-16.
- Decision: one operation deadline covers all adapter-controlled preprocessing as well as
  child execution, with cancellation checkpoints during bounded file reads. Rationale:
  hashing and snapshot work are part of the promised bounded operation. Date: 2026-09-15.
- Decision: use `br --no-db where` for supported-profile path discovery and run
  `sync --status` against private verified copies of the database family and JSONL export.
  Rationale: the 0.5.7 `info` startup path and a live status fallback may create or modify
  database-family artifacts, which violates Doctor's observational contract. Date:
  2026-09-15.
- Decision: describe the provider handoff boundary rather than claiming cross-process inode
  transfer that `br 0.5.7` does not implement. Retain descriptor/path identity hardening,
  depend on the provider's database-family authority for cooperating writers, and classify
  detected post-spawn change as indeterminate. Rationale: the task requires ambient-routing
  isolation, not defense from an uncooperative process with same-user filesystem mutation
  authority, and the external CLI has no expected-inode interface. Date: 2026-09-15.
- Decision: reproduce every 0.5.7 file-state health input in the private readiness snapshot,
  including the optional legacy lock and its source modification time, and remove inherited
  platform loader-injection variables before launching the verified executable snapshot.
  Rationale: readiness must classify the source store faithfully, and executable content
  identity is not meaningful if the loader can inject unverified code. Date: 2026-09-15.
- Decision: cap each private readiness snapshot at 512 MiB of aggregate logical source
  bytes and return a distinct pre-start size error; on macOS, create the executable copy at
  a private name, close the writable descriptor, reopen read-only, verify `F_GETFL`, and
  retain the private directory until launch completes. Rationale: deadlines do not bound
  sparse-file disk amplification, and Darwin cannot execute the Linux-style anonymous
  descriptor path. Date: 2026-09-16.
- Decision: open external authority inputs with `O_NONBLOCK|O_NOFOLLOW|O_CLOEXEC` on Unix
  before validating descriptor/path identity, and model the exact 0.5.7 close error stream
  as two strict JSON documents. Rationale: deadlines must also cover malformed special-file
  inputs, and typed mutation outcomes require the real pinned provider framing. Date:
  2026-09-15.
- Decision: carry the dot-suffixed migration-state file as part of the database family,
  reject a canonical ambient temporary root inside the repository, and remove all inherited
  `LD_*`/`DYLD_*` variables. Rationale: readiness must not trigger engine repair from an
  incomplete family, supposedly private copies must stay outside repository state, and
  loader-control namespaces are open-ended. Date: 2026-09-15.
- Decision: run issue and comment reads against bounded private database-family snapshots
  while retaining the live descriptor for mutations. Rationale: real 0.5.7 performs
  write-capable SQLite housekeeping during read opens, which must not mutate source state
  or make reads depend on repository write access. Date: 2026-09-15.
- Decision: accept a typed mutation rejection only from one strict error document with an
  otherwise empty peer stream and the exact 0.5.7 retryability for that code; encode actor,
  comment, and close-reason values inside their option token. Rationale: mixed output is
  ambiguous after a write-capable spawn, and separate hyphen-leading values can be parsed
  as provider options. Date: 2026-09-15.
- Decision: require the pinned provider's full five-field error schema under a sole
  top-level `error` before classifying a mutation rejection, including exact false
  retryability for `NOTHING_TO_DO`. Rationale: a partial or success-plus-error object is
  malformed after a write-capable spawn and therefore requires reconciliation. Date:
  2026-09-16.
- Decision: treat mutation success as one exclusive profile frame: stderr must be empty
  apart from whitespace, comment results cannot contain `error`, and claim/close results
  must be raw one-item arrays whose issue object contains no `error`. Rationale: a
  write-capable exit-zero process can still emit contradictory or unobserved data, which
  cannot safely acknowledge the external effect. Date: 2026-09-16.
- Decision: permit manual-export readiness only for one `db_newer` anomaly with
  `severity = "degraded"`. Rationale: duplicate or internally inconsistent audit evidence
  is not the single known-safe pending-export state. Date: 2026-09-16.
- Decision: validate every field boundary that proves a close no-op, and reject a
  readiness object containing `error` before considering its health flags. Rationale:
  definitive no-effect and permission-to-write decisions cannot coexist with unprofiled
  mutation evidence or a provider error. Date: 2026-09-16.
- Decision: apply the shared Bash-environment sanitizer to every tracker process in
  addition to loader namespace removal. Rationale: the supported executable boundary
  admits scripts and therefore must bind shell startup behavior as well as file bytes and
  the native dynamic loader. Date: 2026-09-16.
- Decision: completion wins once a child has exited, its response is strictly decoded,
  and final workspace validation succeeds. Deadline/cancellation checkpoints remain
  before spawn and process supervision remains bounded, but a late signal cannot revoke a
  known read or convert a known successful write into ambiguity. Rename the budget API to
  make its pre-spawn semantics explicit. Date: 2026-09-16.
- Decision: create the immutable executable snapshot once during discovery, retain it for
  the adapter lifetime, and perform only source/path metadata identity checks before later
  calls. Rationale: the adapter should execute the exact bytes it profiled without paying
  repeated full-file hashing/copy costs; source replacement still forces rediscovery and
  a post-check replacement cannot affect the already prepared snapshot. Date: 2026-09-16.
- Decision: omit SQLite `-shm` from private snapshots, classify stable-identity metadata
  drift as transient snapshot change, and retry the whole private snapshot a small bounded
  number of times under the existing operation budget. Preserve `InvalidWorkspace` for
  path, inode, type, link, or containment violations. Date: 2026-09-16.
- Decision: unsupported execution platforms fail with a typed error before command
  construction; never pass placeholder database paths to the provider. Date: 2026-09-16.
- Decision: structured provider errors that remain semantically unsupported are converted
  at the process boundary to `IndeterminateWrite` for every mutation. Parsed definitive
  rejections remain their typed no-write outcomes. Date: 2026-09-16.
- Decision: if existing scalar work authority such as `tracker` or `receipt_metadata` is
  malformed, `jig update` refuses to proceed and names the invalid field. It must neither
  silently delete user-authored authority nor preserve it without a diagnostic. Date:
  2026-09-16.
- Decision: the external tracker executable cannot originate from an empty or relative
  `PATH` element or resolve inside the repository, even when its bytes are later snapshotted
  immutably. Continue past such entries so an absolute external installation remains
  discoverable. Rationale: integrity protects selected bytes; provenance decides whether
  checkout-controlled bytes may be selected at all. Date: 2026-09-16.
- Decision: reject `error` on every supported success shape at the pinned-profile parser
  boundary, and reject actor values above 256 bytes plus comment/close text above 64 KiB
  before readiness or spawn. Rationale: success/error exclusivity and finite external
  input are profile invariants, not caller-specific policy. Date: 2026-09-16.
- Decision: retain a fresh private database generation per read/readiness operation rather
  than caching a writable provider snapshot. Document O(database-family size) cost, the
  hard 512 MiB resource ceiling, and deadline behavior. Rationale: the supported provider
  may mutate any database it opens, while each public read must observe current source
  state. Date: 2026-09-16.
- Decision: preserve fixed 5/10/15-second operation budgets and fresh store generations;
  classify budget exhaustion during snapshot construction as `StoreSnapshotTimedOut` and
  add 10/25 ms retry backoff inside that same budget. Rationale: bounded execution and
  observational freshness are original guarantees, while phase-specific failure and
  retry pacing are the missing operational policy. Date: 2026-09-16.
- Decision: distinguish unusable executable candidates, unavailable immutable-snapshot
  support, and retained-source replacement. Continue PATH search only for candidate-local
  invalidity. Rationale: selection, platform capability, and later provenance drift have
  different scopes and remedies. Date: 2026-09-16.
- Decision: classify a strict pinned-profile `AMBIGUOUS_ID` envelope as a definitive
  ambiguous issue-ID error. Rationale: resolution failed before the provider could select
  a mutation target; converting this known rejection to `IndeterminateWrite` discarded
  useful certainty. Date: 2026-09-16.
- Decision: keep configured tracker Doctor failures required, including diagnostic-session
  unavailability and cancellation. Rationale: `required` describes the repository's
  configured contract, not whether a probe happened to complete; downgrading it would let
  Doctor pass without validating an opted-in authority. Date: 2026-09-16.
- Decision: scrub the provider's open-ended `BD_*`, `BR_*`, `BEADS_*`, and `TOON_*`
  configuration namespaces, then retain only the explicit actor/session attribution
  allowlist and profile-owned values. Rationale: a finite routing denylist cannot preserve
  isolation as the pinned provider adds configuration variables, while indiscriminately
  removing identity would violate the adapter contract. Date: 2026-09-16.
- Decision: do not claim atomic readiness-to-mutation serialization for the external
  0.5.7 CLI. Preserve the required no-auto-import policy and require later linked workflows
  to persist intent and reconcile issue/export state. Rationale: the provider offers no
  lock handoff or generation precondition; pretending otherwise would be a false guarantee,
  and importing automatically would change task authority. Date: 2026-09-16.

## Outcomes & Retrospective

Configuration, bootstrap preservation, adapter operations, Doctor diagnostics, docs, and
focused fixtures are implemented. Focused tracker/config/bootstrap/Doctor tests and
strict library Clippy have passed during development. One review-fix round repaired
provider routing and ambient no-DB/cache boundary gaps, a second bound the selected
profile to executable identity, and a third closed hard-link and verify-to-launch alias
gaps. The first approved continuation repair sealed the in-place executable race, made
Doctor/readiness observational through private store snapshots, applied one public
operation budget, and documented the provider handoff limit that 0.5.7 cannot eliminate;
later repairs in this continuation preserved stale-lock health evidence, removed inherited
loader injection controls, corrected the macOS immutable snapshot, and bounded temporary
store copies. The final permitted repair also made authority opens nonblocking and aligned
close classification with a reproduced real 0.5.7 two-document stdout sequence. Focused
tests, real generic `br 0.5.7` probes, source budgets, and strict Clippy pass. The second
approved continuation then restored migration-state evidence, rejected repository-local
temporary roots, and replaced the finite loader denylist with namespace sanitization. Its
next repair moved issue and comment reads onto the same private database-family boundary
after a real 0.5.7 review probe observed live-store SQLite housekeeping. The third repair
made mutation error framing exclusive and option values parser-safe; the fourth required
the complete non-conflicting 0.5.7 envelope for every definitive rejection. The next
continuation repair made exit-zero mutation framing exclusive, removed unobserved
claim/close wrapper success shapes, and narrowed manual-export readiness to one internally
consistent degraded anomaly. The first full gate run then passed 4,166 tests plus Clippy,
formatting, and contract validation; its sole failure was repaired legacy line-debt growth
in a Doctor test part. The following review tightened close no-op result framing and
readiness contradiction handling. The final permitted repair removed inherited shell
startup and exported-function authority from every tracker launch. The final
same-fingerprint Claude/Codex review and one bounded repair pass are complete. Configured
gates and structured-work receipts are current; task close/commit remain intentionally
separate from this review handoff.

The comprehensive-review repair resolved the remaining issues at their owning
boundaries rather than adding caller-side exceptions. Process lifecycle now distinguishes
pre-spawn cancellation from terminal child results; executable discovery retains one
immutable approved image; observational store copies omit rebuildable SHM and retry a
bounded coherent generation when cooperating SQLite activity changes WAL metadata;
mutation response normalization owns all post-spawn ambiguity; and bootstrap update
refuses malformed authored tracker/receipt authority. The final exact-tree run passed all
4,173 tests (3 skipped), Clippy, formatting, contract, and file-budget targets, with fresh
receipt `receipt_01M2MWE7VTWRTZZ62Q4QNAMBMX`. `python3 scripts/beads-sync.py --check`
and `git diff --check` also passed. Task close/commit remain intentionally separate from
this review-repair handoff.

The final exact-tree verification passed all configured targets. `api:test` ran 4,180
tests successfully (three skipped); `api:clippy`, `api:fmt`, `repo:contract`, and
`repo:file-budget` also passed. The fresh target-validation receipt is
`receipt_01M2N3KH4JNEM9F4ZQ0E2F52PE`, with test receipt
`receipt_01M2N3K96ANAVAM6VR25VHWFVG`. A separate required
`JIG_DEV_BIN=target/debug/jig scripts/jig check test` run then passed the same 4,180 tests.
`python3 scripts/beads-sync.py --check` and `git diff --check` pass. The repaired diff was
not sent through an additional external review because the comprehensive-review workflow
permits one repair pass after the frozen same-fingerprint review.

The follow-up independent review found three real boundary omissions and one deliberate
cost tradeoff. Provider discovery now rejects cwd-dependent and repository-owned `PATH`
entries; every pinned-profile success decoder rejects a conflicting `error`; mutation
actor/text values have explicit pre-spawn byte limits; and Doctor's tracker tests use the
same Linux/macOS support gate as the adapter. Fresh private store generations remain
intentional because the provider can mutate the database it opens; docs and regressions
now make the O(database-family size) cost, 512 MiB safety ceiling, deadline, and
non-reuse invariant explicit. Optional-work update coverage now includes invalid tracker
authority in a full harness and invalid receipt metadata in a minimal harness. The final
exact-tree run passed all 4,176 tests (3 skipped), Clippy, formatting, contract, and
file-budget targets under receipt `receipt_01M2MZ5XG7FKFK0C1Q828DGH3T`.

The portability review exposed one deeper model error and three bounded omissions. The
model error was conflating an identity witness with a provider namespace: a retained
database descriptor can prove which inode Jig inspected, but `br` needs a real parent
pathname to create adjacent locks and temporary files. Linux `/proc` obscured the error;
real macOS `br 0.5.7` made it observable. Mutations now use the canonical validated live
path, retain the descriptor only for identity, and validate the complete live family
(including SHM and fixed lock files) before launch and after completion. Read operations
remain observational private snapshots and intentionally omit rebuildable SHM. macOS
executable authority is likewise represented honestly as a read-only private named image,
while Linux keeps its sealed anonymous memfd. Unsupported hosts now fail before PATH
lookup, and documentation distinguishes one-time byte capture from later metadata
revalidation. Focused verification passed 55 Linux and 56 macOS tracker tests; the real
macOS canonical-path mutation probe also passed. The final exact-tree gate passed all
4,184 tests (three skipped), Clippy, formatting, contract, and file-budget targets under
receipt `receipt_01M2NAGC58Z0CJMRYD5ENK779F`; the separately required
`JIG_DEV_BIN=target/debug/jig scripts/jig check test` invocation passed the same 4,184
tests.

## Context and orientation

The owning task is `jig-sh-x8ow.2`; `docs/plans/beads-work-integration.md` contains the
epic design. `crates/jig/src/context.rs` loads `.jig.toml` and constructs
`RepositoryExecutionAuthority`. Its included `context/work_config.rs` owns strict
`[work]` deserialization and the new tracker accessor. Do not alter T1's journal
validator in `state/tracker_identity.rs`; it must keep reading committed identities.

`bootstrap/runtime_config.rs` merges valid existing runtime config into newly rendered
updates. Carry existing `receipt_metadata` and `tracker` without enabling a tracker in an
unconfigured repository. Templates/setup prompts remain unchanged until T8.

Create crate-private `crates/jig/src/tracker.rs` with process, path, explicit-profile,
and test submodules, splitting fixtures further if file budgets require it.
Wire only the facade in `lib.rs`. It owns executable resolution, profile selection, store
discovery, subprocess invocation, strict JSON decoding, normalization, semantic revision,
readiness interpretation, and typed errors. It uses `jig-owned-process`, never SQLite or
a shell, and persists no raw responses.

Add a tracker `DoctorCheck` only after repository context loads and include its process
work in Doctor's cancellable signal session and unsafe-retirement invalidation.
Unconfigured means an
optional successful "not configured" result and performs no executable/path probe.
Configured means a required capability: report version/profile/supported operations or
a value-free actionable failure. The check may run discovery/status only, never issue or
mutation commands, init/import/export, or implicit auto import/flush.

Normalized issue fields are exact ID, title, description, acceptance criteria, status,
optional assignee, and deterministic semantic revision. Hash a domain-separated,
length-prefixed sequence of ID/title/description/acceptance criteria; exclude comments,
mutable status/assignment, audit fields, and timestamps. Reject an ID not byte-equal to
the requested ID. Keep missing and tombstoned distinct from adapter unavailability.

The internal operations are discovery, show issue, list comments, add comment, claim,
and close. Each accepts typed scalar inputs and shell-free argv. Values containing spaces
or metacharacters stay one argument. Mutations return normalized acknowledgements, not
raw JSON, leaving reconciliation to callers.

Centralize finite budgets in `TrackerProcessPolicy`: 5 seconds for discovery, 10 seconds
for reads, 15 seconds for mutations, with 1 MiB stdout and 64 KiB stderr ceilings.
Tests inject shorter budgets and cancellation. Overflow is fatal; stdout and stderr
remain separate; stdout must contain exactly one JSON value plus whitespace; errors
never embed captured issue/store content.

## Plan of work

### Milestone 1: configuration and update preservation

Add `tracker: Option<WorkTrackerConfig>` with serde default/omission. Define a strict
tagged Beads variant, validated workspace-ID newtype, manual export enum, and bounded
nonblank guidance validation. Keep `.beads` a code constant. Expose a read-only
`RepoContext` accessor.

Tests prove legacy absence, valid config, canonical ULID rejection, unknown key/kind,
unsupported export, guidance validation, equal omitted/explicit-manual authority, and
unchanged legacy authority digest. Bootstrap tests prove valid tracker plus
`receipt_metadata` survive update while an unconfigured repository stays unconfigured.

    cargo test -p jig-sh context::tests -- --nocapture
    cargo test -p jig-sh bootstrap::tests -- --nocapture

### Milestone 2: bounded adapter and operations

Resolve one executable per adapter. Run `br version`; select 0.5.7. Discover with
`--no-db where --json`, no-auto flags, root cwd, and routing overrides removed. Inspect local
help/source or reproducible hostile-environment tests before fixing the exact routing
denylist; preserve unrelated actor/auth variables.

Canonicalize repo and existing `.beads` without creating it. Strictly decode `where`,
canonicalize database/JSONL paths, require regular files and containment inside exact
store, and reject aliases/external paths before retaining the adapter. Later commands
pass explicit `--db` plus no-auto flags.

Implement profile-specific strict decoders for show, comments list/add, claim, and close
from generic 0.5.7 fixtures. Before writes, decode `sync --status --json` and require a
fixture-backed safe state. Permit the expected manual-export state where only the
database is newer; reject JSONL-newer, sync conflicts, and unhealthy authority. Never
infer readiness from warning prose.

Typed errors distinguish missing binary, unsupported version/response, missing/tombstone,
blocked/assignment conflict, stale storage, timeout, cancellation before/after start,
nonzero, fatal overflow, and indeterminate mutation. Include safe operation/issue context
but no paths/bodies. Reads fail definitively; mutation process failures after start are
indeterminate and never retried.

Fake-executable tests cover fixtures, identity/revision, absent/unknown binary, malformed
or trailing JSON, missing fields, timeout/cancellation/overflow/nonzero, stale readiness,
ambient redirects, external/symlink paths, and argv boundaries. Unknown/rejected states
must prove no mutation invocation. Successful show preserves store bytes and mtimes.

    cargo test -p jig-sh tracker -- --nocapture
    cargo clippy -p jig-sh --all-targets -- -D warnings

### Milestone 3: Doctor and documentation

Reuse adapter discovery/status diagnosis in Doctor. Unconfigured must not probe; configured
failures are required and actionable. Output only portable `.beads`, observed profile,
supported operation names, and generic recovery—not absolute DB/export paths.

Doctor tests snapshot config/store bytes and mtimes and prove only version/where/status run
with no-auto flags. Cover missing binary, unknown profile, malformed discovery, external
routing, cancellation, and unconfigured no-probe behavior.

Update `docs/configuration.md` and `docs/public-contract.md` for opt-in configuration,
portable identity, fixed root, manual-only export, independence from `receipt_metadata`,
external 0.5.7 requirement, read-only Doctor behavior, and no linked lifecycle yet. Do not
add T8 setup prompts.

    cargo test -p jig-sh doctor::tests -- --nocapture
    cargo test -p jig-sh bootstrap::tests -- --nocapture

### Milestone 4: integration verification and convergence

Build and dogfood the current runtime:

    cargo build -p jig-sh --bin jig
    export JIG_DEV_BIN=target/debug/jig
    scripts/jig work check --plan-id plan_01M2K19NFSVK5W5MKPV2SQWNGV
    scripts/jig work gates --plan-id plan_01M2K19NFSVK5W5MKPV2SQWNGV
    scripts/jig work evidence --plan-id plan_01M2K19NFSVK5W5MKPV2SQWNGV
    scripts/jig work receipts --plan-id plan_01M2K19NFSVK5W5MKPV2SQWNGV
    scripts/jig check test

Review the complete diff from baseline `38a9624e` and run the approved bounded
review/fix loop. Each iteration needs independent review, scoped repair, focused tests,
and re-review. Record real residuals as Beads issues rather than expanding T2 without a
correctness reason. Then update this plan, append the structured-work summary, run
`python3 scripts/beads-sync.py --check`, close/sync `jig-sh-x8ow.2`, finish the plan,
inspect status/diff, and create the requested task-level commit. Do not push.

## Validation and acceptance

Completion requires every owning-task acceptance criterion, focused config/adapter/
bootstrap/Doctor regressions, strict `jig-sh` Clippy, configured structured gates, and
final successful `JIG_DEV_BIN=target/debug/jig scripts/jig check test`. Record actual
commands/counts/receipts; expected results are not evidence.

Decisive negative cases are: unconfigured never resolves `br`; ambient routing cannot
select another store; external/symlink escape stops before writes; unknown profile cannot
mutate; Doctor changes no config/database/JSONL/comment/issue state; and failed mutation
is indeterminate rather than unchanged.

## Idempotence and recovery

Parsing, discovery, show, list-comments, and Doctor are repeatable reads. Adapter writes
are never automatically retried. After timeout, cancellation-after-start, cleanup failure,
overflow-after-start, or nonzero from add-comment/claim/close, a future caller must reread
the exact issue and reconcile durable intent before deciding on retry. T2 classifies but
does not invent that workflow.

If canonical serialization changes unconfigured authority digests, preserve the prior
representation instead of accepting a global migration. If actual 0.5.7 differs from a
fixture, reproduce read-only against generic temporary data before updating the profile.
Never mutation-probe this repository's `.beads`. Preserve append-only journal changes and
unrelated worktree content.

## Interfaces and dependencies

Configuration shape:

    [work.tracker]
    kind = "beads"
    workspace_id = "01ARZ3NDEKTSV4RRFFQ69G5FAV"
    export = "manual" # optional/default and the only T2 value
    manual_export_guidance = "Run the repository's documented Beads export step."

The crate-private facade should provide equivalents of `WorkTrackerConfig`,
`BeadsAdapterPolicy`, `BeadsIssueSnapshot`, `BeadsOperation`, and a typed adapter error.
Names/factoring may follow Rust style, but distinctions and safety semantics remain
test-visible. Use existing `serde`, `serde_json`, `ulid`, hashing,
`jig-owned-process`, and test-temp dependencies; add no Beads library/database dependency.

Plan revision note (2026-09-15): replaced the structured-work placeholder after
repository inspection and two read-only design audits.


Authored the self-contained T2 ExecPlan after repository inspection and independent configuration/process audits. Implementation is split into non-overlapping configuration/bootstrap, adapter, and Doctor/documentation milestones; exact supported external profile is br 0.5.7.
