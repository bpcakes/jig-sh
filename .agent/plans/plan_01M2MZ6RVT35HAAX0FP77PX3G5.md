# Reset-aware fleet usage projection for the Codex launcher

Implement reset-aware collective quota risk forecasting for the Codex launcher home picker
and validate its model and TUI.

The picker answers one added question: at the estimated aggregate pace, with work
transferable between the participating accounts, does the modeled account pool reach a
period in which no account can accept work before quota becomes available again? This is a
subscription-quota forecast, not a promise that a running session migrates or that
provider, model, credit, or concurrency limits are satisfied. Existing per-account usage,
projection, and recommendation behavior is unchanged.

## Progress

- [x] Reconcile the worktree against the planning baseline `bed5af6e`.
- [x] Define the forecast contract: cohort eligibility, provisional identity, capacity
      class, window schema, temporal origin, horizon, reset model, and outcome states.
- [x] Implement the pure reset-aware engine in `crates/jig-codex-tui/src/model/fleet/simulation.rs`.
- [x] Wire `App::fleet_assessment_at` over the complete row set with a per-generation cache.
- [x] Integrate the Codex-only header summary and detail-pane explanation.
- [x] Add engine tests (`src/model/fleet/tests.rs`, `.../tests/timeline.rs`) and
      application/rendering tests (`src/tests/fleet.rs`).
- [x] Update `docs/configuration.md`, `CHANGELOG.md`, and the crate guide.
- [x] Run focused checks and every required repository gate; record actual results.

Restart checkpoint: complete. Jig plan ID `plan_01M2MZ6RVT35HAAX0FP77PX3G5`, opened against
baseline `bed5af6eb0df67359197cba6e543d575a9912dfe`. No blockers. All five required gates
(`api:clippy`, `api:fmt`, `api:test`, `repo:contract`, `repo:file-budget`) passed, and
`work evidence` reported the receipts fresh against the worktree. Work is delivered on
branch `feature/codex-fleet-usage-projection` as two commits: the mechanical test
relocation, then the feature.

## Surprises & Discoveries

1. Gating the feature on the picker's subscription-bucket list leaked a Codex forecast into
   the Claude picker. `App::new` defaults `subscription_buckets` to `["codex", "claude"]`,
   and the legacy `select_inspected_configuration_with_cancellation` entrypoint keeps that
   default, so `inspected_configurations_show_subscription_limits_and_preserve_mode_identity`
   failed on its `!screen.contains("Codex")` assertion. The enable became an explicit
   `App::codex_fleet_forecast` flag set only by the Codex provider path.
2. Summing per-dimension rates only conserves demand when every account reports the same
   window layout. An account that omits a dimension would silently absorb workload that is
   never charged anywhere, so a mismatched schema is refused rather than partially pooled.
3. Bounding the simulation by event count is not enough. Event cost scales with cohort size,
   so a 2048-home list could make each of thousands of events O(N). The budget counts window
   visits instead, which bounds absolute work independently of cohort size.
4. Floating-point depletion that coincides with a restoring reset produced a spurious
   sub-second gap, because `remaining/rate` can land microseconds before the reset instant.
   The event selection now treats a depletion within `TIME_EPSILON` of a reset or the horizon
   as simultaneous.
5. `src/tests.rs` was already 188 lines past the `repo:file-budget` maximum, so merely adding
   a `mod fleet;` declaration failed the gate as debt growth. Extracting the search and
   navigation tests cleared the debt instead of waiving it.
6. Placing the fleet detail block after the per-account inspection errors broke
   `wrapped_detail_content_scrolls_to_its_final_rendered_row`, which scrolls to a tail marker
   inside `details.inspection_error`. The error lines moved after the fleet block so all
   diagnostics remain last.
7. Every required gate passed locally while six CI checks failed, because
   `scripts/check-supported-host-surface.sh` scans tracked content through `git grep`. The new
   module files were still untracked during the local gate run, so the scan could not see
   them. Run that policy check, or the gates, after `git add` rather than before.
8. The supported-host policy bans a bare capital `Windows`, so the `AmbiguousWindows`
   exclusion variant tripped it on substring alone. `crates/jig/tests/supported_host_surface.rs`
   asserts singular `Window` stays allowed, so the fix was renaming the variant to
   `DuplicateWindowDurations`, which also names the condition it detects more precisely.

## Decision Log

Decisions below were made on September 16, 2026. None are superseded.

- Decision: Add a fleet result and leave per-account results alone.
  Rationale: The fleet answers pool runway; account rows still choose a home and name a
  limiting window.
- Decision: Enable the forecast through an explicit provider flag rather than inferring it
  from subscription buckets.
  Rationale: Bucket inference silently reached the Claude and legacy configuration
  entrypoints, as discovery 1 shows.
- Decision: Require a shared reported plan and a shared window-duration set for the cohort;
  refuse mixed values and exclude unknown ones.
  Rationale: Equal-capacity normalization is an estimate. The normalized inspection payload
  carries no absolute quota sizes, so mixed or unknown capacity must stay visibly
  unsupported rather than be treated as equal.
- Decision: Use the account email as the provisional quota-pool key and collapse duplicates.
  Rationale: Home paths and sanitized labels are not proof of independent quota. Unknown
  identity keeps coverage incomplete instead of multiplying capacity.
- Decision: Keep the existing window-average estimator and label its provenance.
  Rationale: A second, inconsistent interpretation of usage would be worse than a labeled
  estimate. Persistent history and smoothing are separate work.
- Decision: Forecast one longest participating window duration ahead, and scan for capacity
  return up to one further duration.
  Rationale: The horizon covers every current reset; every window resets within its own
  duration, so recovery is always reachable inside the extended scan.
- Decision: Treat any nonzero-usage window inside the 10% warmup as `collecting` for the
  whole fleet.
  Rationale: Mirrors the per-account rule and never presents warmup noise as a rate or as
  zero burn.
- Decision: Bound the engine by window visits and report `forecast budget reached before the
  horizon` when exceeded.
  Rationale: Per discovery 3, a stated limit is honest and keeps the picker responsive.
- Decision: Cache the forecast per inspection generation and keep `now` out of the modeled
  timeline.
  Rationale: Redrawing is not a new provider sample. Only freshness and countdowns may move
  between frames.

## Outcomes & Retrospective

Delivered against the acceptance criteria:

- The engine retains a budget per account and per quota window, advances to the next reset,
  depletion, or horizon, replaces one window's allowance at reset without accumulating or
  refreshing a sibling, and conserves aggregate demand across exhaustion, account switches,
  and simulated resets.
- Outcomes cover collecting, no gap, gap risk, currently blocked, and explicit unsupported
  reasons, with coverage and staleness carried separately so a partial cohort is never
  phrased as an all-account result.
- `App::fleet_assessment_at` reads `rows`, not `visible_indices()`, so search and selection
  cannot change the modeled pool.
- Presentation uses the header's previously unused second row, so the supported 46x12
  minimum and the wide and stacked layouts are unchanged.

Verification evidence: 116 `jig-codex-tui` tests pass, including analytically checked cases
for pooled demand, staggered resets, allowance replacement, complementary cross-window
exhaustion, depletion exactly at a restoring reset, row-order invariance, and retained
demand from an exhausted account. All five required gates passed under this plan, with
`api:test` covering 4110 workspace tests. A 64-account cohort forecasts a full weekly
horizon of five-hour resets in roughly 10 ms in a debug build.

Remaining gaps, stated rather than closed:

- A reported gap is risk under the documented earliest-reset-first scenario. It is not proof
  that no allocation could avoid a gap; that claim needs a coupled feasibility model over
  reset intervals, common workload units, and validated capacity conversions.
- Heterogeneous-plan pooling stays unsupported until the inspection payload supplies
  comparable capacity weights. The pure engine accepts per-window weights later without
  changing the reset logic.
- The rate basis remains window-average pace, which can lag a recent burst or an account
  switch. A common-interval history estimator is separate work.

Lesson: the shared picker's existing regression tests were the most valuable design input.
Two of them caught real behavioral leaks, in provider scope and in detail-pane ordering,
that reasoning about the diff had not surfaced.

Lesson: local gate success does not cover repository policy that inspects tracked content.
Per discovery 7, stage new files before running gates, or the scan silently skips them and
CI reports the first failure.
