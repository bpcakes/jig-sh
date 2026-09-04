# Rust-only init presets plan review evidence

This ledger records the review history for
`docs/rust-only-presets-plan.md`. Rounds 13–16 explain the earlier design state;
round 17 records the implementation-readiness remediation; rounds 18–21 are the
current reproducible steady-state review. It distinguishes identified history
from recoverable reviewed bytes rather than treating an uncommitted digest as a
Git baseline.

## Reviewed artifact

- Review date: 2026-08-30
- Reviewer/model: Codex, GPT-5
- Recoverable input baseline: commit
  `78b54d438ea2a0a5bec863a93eab9cc19797db54`
- Current reviewed output: `docs/rust-only-presets-plan.md`
- Current reviewed output SHA-256:
  `3c8275ab4320f9d818d81ffc373447ce7d281861cba06f445e450c7f63c0b15b`
- Superseded round-13 output SHA-256:
  `80816bfcd427d6f854f06acbd5bf03f1baa8ba6fb436b303f68595af30259192`
- Structural diff: `git diff 78b54d4 -- docs/rust-only-presets-plan.md`

The current output bytes can be identified with:

```sh
sha256sum docs/rust-only-presets-plan.md
```

The superseded digest identifies the output reviewed in rounds 13–16, but those
bytes were never committed and were revised by round 17. They are retained as
design history, not claimed as an independently recoverable artifact. The
current plan, this ledger, and the Beads export are committed together after the
round-21 checks.

The Beads descriptions are synchronized copies of the owning plan task
specifications. They were updated through `br` in both the normally discovered
canonical store and the worktree-local database. Semantic task fields match;
store-local update timestamps are not part of the plan contract and need not be
byte-identical.

## Common validation procedure

Each counted round used the four-part planning validation loop:

1. Self-containment: read every affected delivery task as a standalone work
   order and look for missing inputs, external numbered-section dependencies,
   or duplicated artifact ownership.
2. Dependency integrity: inspect task dependencies, task-scoped readiness, and
   graph cycles.
3. Decision quality: sample at least five load-bearing decisions and verify that
   the plan states the reason and tradeoff, not only the chosen result.
4. Steady-state test: compare the resulting plan digest with the input to the
   round. A structural change requires another round.

Representative reproduction commands:

```sh
br ready --epic jig-sh-rust-only-init-presets-zc7 --type task --json \
  --db .beads/beads.db --no-auto-import
bv --robot-insights --format json
br show jig-sh-rust-only-init-presets-zc7.1.1 --json \
  --db .beads/beads.db --no-auto-import
br show jig-sh-rust-only-init-presets-zc7.3.2 --json \
  --db .beads/beads.db --no-auto-import
rg -n 'section [0-9]+|§[0-9]+|B05 owns|B07 owns' \
  docs/rust-only-presets-plan.md
git diff --check
```

The graph tool reports repository-wide roots as “orphans,” including this epic,
its organizational features, and B01. That label is not used as evidence that a
delivery task is disconnected. Direct dependency inspection proves the scoped
delivery path `B01 -> B02 -> B03 -> B04 -> B05 -> B06 -> B07`; B01 is the
intentional root and B07 the intentional terminal task.

## Round 13 — Audit remediation integration

Input was the plan at commit `78b54d4`. This round made structural changes.

Self-containment results:

- B01 now defines only the extensible capability and project-plan boundary and
  proves unchanged behavior for the two existing backend presets.
- B02 owns creation and testing of the first non-backend plan variant.
- B03 and B04 each contain the complete Rust-only CLI and merged answers-file
  policy, including unknown-key handling and pre-vault failure ordering.
- B05 owns help, wizard/strict diagnostics, and doctor recovery copy; B07 owns
  documentation and final verification only.

Dependency results:

- The seven-task chain was unchanged and acyclic.
- Task-scoped readiness returned only B01.
- No delivery task was disconnected; the epic and feature records remain
  organizational roots in the graph representation.

Decision-quality sample:

1. The first non-backend value belongs in B02 because B01 must remain a true
   behavior-preserving refactor with an existing-output oracle.
2. Answers-file validation is fail-closed because the generic deserializer can
   otherwise silently ignore shape-bearing keys.
3. `web_package_manager` remains accepted but inert for compatibility because
   it cannot affect a Rust-only filesystem or executable preflight.
4. Authored repository, command, work, and loop models are rejected because
   accepting them would let a supposedly fixed preset produce another shape.
5. Interaction text belongs to B05 because B03/B04 already own finalized preset
   descriptors and reports, while B07 is a documentation and release gate.

Steady-state result: the diff from `78b54d4` was structural. The resulting plan
digest became
`80816bfcd427d6f854f06acbd5bf03f1baa8ba6fb436b303f68595af30259192`, so
another full round was required.

## Round 14 — Standalone delivery-task review

Input and output SHA-256 were both
`80816bfcd427d6f854f06acbd5bf03f1baa8ba6fb436b303f68595af30259192`.

Self-containment results:

- Every B01–B07 description has its own outcome, scope, acceptance criteria,
  dependencies, and shared execution contract.
- No delivery task depends normatively on a numbered plan section.
- B03/B04 specify acceptance and rejection behavior rather than relying on the
  feature-level summary.

Dependency results: the explicit chain remained acyclic, B01 remained the only
ready task, and B07 remained the intentional terminal task.

Decision-quality sample:

1. `rust-library` and `rust-cli` are explicit public names so artifact intent is
   discoverable without weakening existing numeric/default behavior.
2. There is no public `rust-workspace` preset because it would expose a vague
   container rather than a usable seed artifact.
3. A one-member virtual workspace gives both presets one stable Cargo shape and
   leaves deliberate room for additional members.
4. Generated packages use `publish = false` and omit license claims because the
   harness lacks authority to choose release and legal metadata.
5. Neutral root guidance derives from the authored workspace component so
   render, update, and recopy do not require persisted preset identity.

Steady-state result: no text, Beads edge, or product contract changed. This was
the first no-change pass after remediation.

## Round 15 — Dependency, rationale, and source-grounding review

Input and output SHA-256 were both
`80816bfcd427d6f854f06acbd5bf03f1baa8ba6fb436b303f68595af30259192`.

Source grounding re-read the current definitions of `InitScaffoldPlan`,
`ScaffoldPreset`, `AnswerOpts`, and `RawAnswers` under
`crates/jig/src/bootstrap/`. The current backend field, answer merge surface,
and authored-model families justify the B01/B02 split and the exhaustive
Rust-only input policy.

Dependency results: direct `br show` inspection reproduced all six delivery
edges, `br ready` returned only B01, and graph insights reported no cycle.

Decision-quality sample:

1. The repository's current Rust floor remains authoritative; a preset must not
   invent a separate toolchain policy.
2. Explicit Cargo workspace members are preferred over broad globs so generated
   ownership and exact file assertions remain stable.
3. The CLI seed stays on `std` argument handling because a dependency would add
   network and lockfile policy to an intentionally minimal artifact.
4. Root component `.` remains the neutral harness anchor because Cargo members
   are implementation children, not separate harness applications.
5. B03 precedes B04 because both edit the central preset dispatcher and their
   separation favors reusable integration over parallel conflicting edits.

Plan/Beads results: the seven delivery-task descriptions, their acceptance
criteria, and the affected F2 feature description expressed the same contracts.
No revision was required, producing a second consecutive no-change pass.

## Round 16 — Final reconstructable steady-state audit

Input and output SHA-256 were both
`80816bfcd427d6f854f06acbd5bf03f1baa8ba6fb436b303f68595af30259192`.

Self-containment and ownership results:

- B01 contains no non-backend construction or placeholder-test requirement.
- B03/B04 include `--answers-file`, shape-bearing answer families, unknown keys,
  and exact accept/reject oracles.
- B05 is the sole mutating owner of CLI guidance and diagnostics; B07 verifies
  those surfaces and owns user/developer/configuration documentation.

Dependency results: the path remained acyclic and fully connected, with only
B01 ready and B07 terminal.

Decision-quality sample:

1. Validation occurs after answers-file merge so CLI and file inputs cannot
   obtain different shape authority.
2. Incompatible input fails before template resolution, vault creation, or
   publication so rejection leaves no partial project.
3. Existing backend presets remain the B01 regression oracle rather than a new
   synthetic non-backend value.
4. B05 snapshots user-visible messages because wording and exit behavior are
   part of the guided CLI contract.
5. B07 uses the development binary during dogfooding because the repo-local
   cached launcher is not evidence for runtime changes.

The final extraction comparison caught a synchronization-only defect in which
B07's Beads description had absorbed the following level-two plan heading. The
record was rewritten through `br` in both stores using the correct heading
boundary, and all task comparisons were rerun. This changed no plan text,
product contract, status, or dependency edge.

Final checks then found valid formatting, no private fixture identifiers, no
stale ownership statement, and semantic agreement between the affected plan
and Beads records. The plan digest remained unchanged for a third consecutive
pass. That was the conclusion at the time, but the later
implementation-readiness audit found unresolved private/public identity,
answer-lifecycle, task-ownership, and Git baseline defects. Round 17 supersedes
the rounds 13–16 steady-state conclusion.

## Round 17 — Implementation-readiness remediation

Input was identified by the superseded SHA-256
`80816bfcd427d6f854f06acbd5bf03f1baa8ba6fb436b303f68595af30259192`.
The exact pre-remediation bytes were not committed, so this is a structural
change record rather than one of the current reproducible no-change rounds.

Source inspection found four blockers:

- B02 required an exact public preset identity even though B03/B04 owned the
  public `ScaffoldPreset` additions.
- Init loaded and merged the answers file for interaction, prepared the vault,
  and then loaded the file again for bootstrap rendering.
- B03/B04 and B05 both claimed ownership of explicit strict/default success.
- The plan and review ledger were not present in an exact committed baseline.

The remediation introduced a private `ScaffoldIdentity` owned by B02, assigned
the two public conversions to B03/B04, made B03 own a retained single-parse
answer handoff that B04 reuses, and restricted B05 to guided selection and
diagnostics. It also requires plan, evidence, and Beads export to land together
before B01 is claimed.

Steady-state result: structural change. The resulting current plan digest is
`3c8275ab4320f9d818d81ffc373447ce7d281861cba06f445e450c7f63c0b15b`,
so another full review was required.

## Round 18 — Standalone task and private/public identity review

Input and output SHA-256 were both
`3c8275ab4320f9d818d81ffc373447ce7d281861cba06f445e450c7f63c0b15b`.

Self-containment results:

- B02 can construct, render, and report the two internal artifact identities
  while asserting unchanged public Clap, wizard, descriptor, and presets output.
- B03 and B04 each own one public enum value, its private conversion, its
  explicit strict/default success path, and complete generation oracles.
- B05 treats those explicit paths as regression oracles and owns only guided
  selection, help, diagnostics, and interaction snapshots.
- Every delivery task retains an outcome, complete scope, acceptance criteria,
  dependency/unblock statement, and task-local execution workflow.

Dependency inspection kept the direct serial path unchanged and introduced no
new cross-task prerequisite. No plan or graph revision was required.

Steady-state result: first no-change pass after remediation.

## Round 19 — Answer lifecycle and source-ordering review

Input and output SHA-256 were both
`3c8275ab4320f9d818d81ffc373447ce7d281861cba06f445e450c7f63c0b15b`.

Source inspection reproduced the current ordering:

- `prepare_init_interaction` loads `AnswerInput` for interaction;
- `run_init_command` performs package-manager and vault preparation next; and
- `prepare_init` calls `AnswerInput::from_opts_at` again for rendering.

The revised B03 contract retains both raw top-level key shape and merged values,
validates the selected preset before package-manager/vault preflight, and passes
the same parsed input into bootstrap. Its tests require a one-read oracle and
prove that later source-file mutation cannot change render authority. B04 adds
only its policy branch. Recognized harness-only nested sections retain existing
typed behavior; whole shape-bearing model sections and unknown top-level keys
fail closed.

Steady-state result: no change, providing a second consecutive no-change pass.

## Round 20 — Dependency, rationale, and Beads alignment review

Input and output SHA-256 were both
`3c8275ab4320f9d818d81ffc373447ce7d281861cba06f445e450c7f63c0b15b`.

Graph and Beads results:

- direct records reproduce `B01 -> B02 -> B03 -> B04 -> B05 -> B06 -> B07`;
- task-scoped readiness returns only B01;
- graph insights report no dependency cycle;
- plan descriptions and acceptance criteria match both the canonical and
  worktree-local Beads stores after semantic timestamp fields are excluded.

Decision-quality sampling retained explicit reasons and tradeoffs for five
load-bearing choices: private identity before public exposure, a frozen answer
input before vault capture, explicit-versus-guided ownership, license-neutral
seed manifests, and neutral repository projection through existing contract
types.

Steady-state result: no plan or graph change, providing a third unchanged digest.

## Round 21 — Final committed-baseline readiness audit

Input and output SHA-256 were both
`3c8275ab4320f9d818d81ffc373447ce7d281861cba06f445e450c7f63c0b15b`.

Final checks:

```sh
sha256sum docs/rust-only-presets-plan.md
git diff --check
br ready --epic jig-sh-rust-only-init-presets-zc7 --type task --json \
  --db .beads/beads.db --no-auto-import
bv --robot-insights --format json
br sync --status --json
br sync --status --json --db .beads/beads.db --no-auto-import
```

These checks confirm the exact plan digest, valid diff formatting, B01 as the
only ready delivery task, an acyclic graph, and healthy synchronized Beads
stores. Description-only fixture scanning finds no local or private path and
the plan uses generic fixture identities. The plan, ledger, and exported issue
records form the resulting
committed baseline; post-commit status is checked before handoff.

Steady-state result: fourth unchanged digest. Rounds 18–21 are the four current
reproducible strong-model rounds and end at implementation-ready steady state.
