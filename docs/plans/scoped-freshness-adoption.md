# Scoped freshness and adoption

Reduce repeated gate execution after staging, commits, and edits outside an owned input set. Epic `jig-sh-0zi8` implements improvement #1 only; cross-plan reuse, scheduler changes, global configuration invalidation and gate deduplication are excluded.

## Progress

- [x] Create feature branch and epic with four dependent tasks.
- [ ] `jig-sh-0zi8.1`: conservative policy recommendations.
- [ ] `jig-sh-0zi8.2`: read-only preview and synchronized adoption patch.
- [ ] `jig-sh-0zi8.3`: qualified generated defaults.
- [ ] `jig-sh-0zi8.4`: behavioral validation and documentation.

Restart checkpoint: implement task 1 on `feat/agent-velocity-improvements`. Initial checkout was clean at `bed5af6e`. Jig plan: `plan_01M2QP69RTD9RQAPBWC2RYZXPH`, baseline `bed5af6e`. No blocking dependency outside this epic. Build dev runtime before harness commands and set `JIG_DEV_BIN=target/debug/jig`.

## Surprises & Discoveries

Existing Rust action inputs are affected-selection hints shared by formatting, clippy and tests. They cannot prove exhaustive formatter inputs: Cargo targets and Rust modules can use arbitrary extensions. A one-time repository scan does not establish durable coverage after future edits.

## Decision Log

- 2026-09-17: Scope is the new freshness/adoption epic, as explicitly selected by the user.
- 2026-09-17: Keep the two axes distinct. Recognized Git-independent formatter commands can default to worktree source state. Exhaustive inputs require an explicit owner assertion; never silently promote existing affected-file hints. Generated arbitrary tests and wrappers retain conservative policy.
- 2026-09-17: Adoption produces a read-only report and optional unified patch. Applying the paired patch with `git apply` changes both authoring and runtime projections or fails without partial application. No new write transaction protocol or contract epoch is needed.

## Outcomes & Retrospective

Implementation and verification remain unfinished. Qualification intentionally promises repository input ownership, not hermetic toolchain/environment attestation.

## Context and interfaces

`crates/jig/src/bootstrap/repository_model/freshness.rs` fills generated freshness defaults. `crates/jig/src/repository/freshness/` implements existing observations; `source_state = "worktree"` removes index/commit identity from source evidence, while `inputs_policy = "exhaustive"` scopes file evidence to declared inputs. Existing runtime guardrails stay intact. The adoption report must use a new explicit CLI surface without extending existing strict inspection projections.

## Milestones and acceptance

1. Implement shared recommendations with current/proposed policies and reason codes. Preserve authored policies/provenance, refuse legacy epochs and native Git-dependent runners, recognize only bounded known commands. Unit tests establish these boundaries.
2. Add a discoverable freshness preview with explicit target selection, owner assertion for exhaustive coverage, additional input patterns and patch output. Read no actions as executable code. Validate config/contract agreement; preserve unrelated fields; produce deterministic paired changes and a no-op after application. Reject ambiguous selectors and invalid declarations. Preview must not mutate repository files.
3. Reuse the shared classifier in generated defaults. Preserve overrides through adopt/update/recopy and legacy rendering. Generated formatting may receive worktree but does not receive exhaustive ownership by inference.
4. Test staging/commit reuse, unrelated-file reuse only after exhaustive assertion, included content/add/remove/rename invalidation, configuration/runner invalidation, and fail-closed observations. Document commands and retained limitations.

## Concrete steps and validation

For each unblocked task: verify a clean checkout, claim it with `br`, implement and stage, validate relevant behavior, then run the requested Codex-only review-fix loop at low severity in comprehensive fix mode. Repair verified findings and rerun a fresh review until convergence. Close the task, sync Beads with `scripts/beads-sync.py`, update this plan and commit before the next task. User authorization covers these commits. If repeated findings reveal architectural misalignment, use the user-requested Astra redesign delegation.

Run `cargo build -p jig-sh --bin jig` before runtime dogfooding. Run focused cargo tests during implementation, configured `JIG_DEV_BIN=target/debug/jig scripts/jig work check --plan-id plan_01M2QP69RTD9RQAPBWC2RYZXPH`, inspect gates/evidence, and finish backend verification with `JIG_DEV_BIN=target/debug/jig scripts/jig check test`. Record actual outcomes here. Use generic fixture identities exclusively.

## Compatibility, idempotence and recovery

No persisted receipt rewrite or policy downgrade. Older epochs remain conservative. Explicit owner policy always wins over generated recommendations. Reapplying adoption to already adopted actions yields no patch. A rejected patch leaves both files unchanged; regenerate after resolving concurrent edits. Retain existing source before/after execution checks and unknown observations. Future changes to custom action read sets remain the owner's responsibility.
