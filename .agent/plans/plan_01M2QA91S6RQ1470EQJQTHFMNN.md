# ExecPlan: Auditable non-success retirement for work plans

## Outcome

Operators can terminate an open structured work plan that will not be delivered
(cancelled, superseded, duplicate, obsolete) without claiming success and
without running required delivery gates. The closure stays append-only and
auditable. Successful `work finish` stays evidence-gated and unchanged.

## Scope

New CLI verb `jig work retire` and MCP tool `jig.work_retire`:

    jig work retire --plan-id ID --disposition <cancelled|superseded|duplicate|obsolete>
                    --reason "text" [--superseded-by REF] [--json]

## Design decisions

- **Event shape.** Retirement reuses the existing `close` plan event and adds an
  additive optional `retirement` object (`disposition`, `reason`,
  `superseded_by`). Older runtimes keep reading a retired plan as closed and
  keep their historical `resolution` meaning; new runtimes read the structured
  disposition. A new event name would decode as `Unknown` in older readers and
  leave the plan looking open, so it is rejected.
- **Disposition storage.** Persisted as a plain string so a record written by a
  newer runtime with an unknown disposition still decodes. Write paths validate
  against a closed `PlanDisposition` enum.
- **Receipt.** Keeps the historical `jig.plans_close` tool name (documented
  compatibility rule for state-operation receipts) with
  `args.operation = "plan_retire"` plus disposition, reason and
  superseded_by, so existing receipt filters and history keep working.
- **Safety.** Retire reuses `plans_close`'s exclusive plan-finish lease,
  open-state recheck before and under the lease, and linked-run rejection. It
  never evaluates work gates and never writes gate evidence.
- **Session ownership.** The plan-open receipt (`jig.plans_open`) durably
  records the session that opened the plan. Retire ends the current session only
  when the current session id equals that proven owner. Otherwise the session is
  left untouched and the response explains why.
- **Projection.** `work gates` / `work evidence` gain `plan_retirement` next to
  `plan_state` (null for a successful close). The TUI plan timeline labels a
  retired close as event `retire`. `plan_state` stays `open`/`closed` so
  existing readers keep their meaning.

## Progress

- [x] Inspect runtime/work.rs, cli/work.rs, state/plans.rs, state/records.rs,
      tool_defs.rs, runtime.rs MCP dispatch, UI plan collection.
- [ ] M1 State: `PlanRetirement` record, `PlanEvent::Close.retirement`,
      `plans_retire`, `plan_retirement`, `plan_open_session`.
- [ ] M2 Command/CLI/MCP surface: DTO, clap opts, tool descriptor, dispatch,
      human output.
- [ ] M3 Runtime operation with session-ownership proof.
- [ ] M4 Projections: gate report `plan_retirement`, TUI timeline label.
- [ ] M5 Tests (state, runtime, MCP parity, CLI help, race, replay).
- [ ] M6 Docs: public-contract, AGENTS template + snapshot, CHANGELOG.
- [ ] M7 `scripts/jig check test` / fmt / clippy with the dev binary.

Restart checkpoint: current milestone M1; next action is editing
`crates/jig/src/state/records.rs`. No blockers.

## Acceptance

1. Retire is the only way to close a plan with unsatisfied required gates, and
   requires a structured disposition plus a nonblank reason.
2. Retire runs no required gates and emits no successful gate evidence.
3. Retire takes the exclusive close lease, rejects unknown/already-closed plans
   and active linked runs, with deterministic race coverage.
4. The close event and receipt are additive and backward compatible; no history
   is rewritten.
5. Status/history outputs distinguish a successful close from a retirement, with
   matching CLI JSON and MCP semantics.
6. A session is ended only when the plan-open receipt proves ownership.
7. Help, workflow and state docs, and failure-path tests are updated.
8. Focused tests plus repository verification pass on the dev binary.


## Progress update

All milestones complete.

- [x] M1 State: `PlanRetirement`, additive `retirement` on the existing `close`
      event, `plans_retire`, `plan_lifecycle`, `plan_owner_session`.
- [x] M2 Command/CLI/MCP surface: `WorkRetireRequest`, `jig work retire`,
      `jig.work_retire` descriptor, dispatch, human output.
- [x] M3 Runtime operation with plan-open-receipt session-ownership proof.
- [x] M4 Projections: `plan_retirement` on gate/evidence JSON and the dashboard
      status view; TUI plan timeline labels a retired close `retire`.
- [x] M5 Tests: 7 state tests (structured disposition, historical close meaning,
      legacy and unknown-disposition replay, append-only round trip, linked-run
      rejection in both orderings, ownership proof), 6 runtime/MCP tests, 2 CLI
      help tests.
- [x] M6 Docs: public-contract, configuration, AGENTS template and snapshot,
      CHANGELOG, runtime-smoke fixture.
- [x] M7 Verification on the dev binary: fmt, clippy, contract, agent-guides,
      agent-map, and `check test` (4106 passed, 3 skipped).

Decision recorded during implementation: retirement reuses the existing `close`
event with an additive `retirement` object rather than a new event name, so
older runtimes still see a retired plan as closed instead of decoding it as an
unknown event and leaving the plan apparently open.