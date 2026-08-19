## Progress

- [ ] Add regression coverage for review findings.
- [ ] Fix release normalization, legacy contract epoch consistency, launcher-only repair atomicity, refresh semantics, legacy PATH launcher rejection, and source-stamp fallback.
- [ ] Synchronize generated templates and embedded snapshots.
- [ ] Run focused and repository gates.

## Surprises & Discoveries

- None yet.

## Decision Log

- Preserve the destination contract epoch during ordinary updates; contract upgrades must change launcher and manifest together.

## Outcomes & Retrospective

- Pending.

## Context and orientation

The review found migration correctness issues across scripts/release.sh, crates/jig/src/bootstrap.rs, templates/project runtime scripts, and their checked-in generated copies.

## Plan of work

Add failing regression cases first, implement bounded fixes, synchronize all three installer copies and launcher copies, then validate via Jig.

## Validation and acceptance

Release launcher normalization has no diff; forced v3 updates keep launcher and manifest at v3; failed repair seeding leaves scripts unchanged; refresh cannot select PATH; legacy launchers are rejected; SHA fallback produces a stamp.

## Idempotence and recovery

Tests use temporary repositories. Launcher-only repair must roll back its two managed scripts if seeding fails.

## Interfaces and dependencies

Bash 3.2 compatibility remains required. Python 3.8+ is already an installer prerequisite.

## Progress

- [x] Added regression coverage for contract upgrades, seed rollback, Python hashing, refresh/PATH interaction, and legacy launcher rejection.
- [x] Implemented release normalization, v3-to-v4 full-update consistency, transactional launcher-only repair, refresh bypass, legacy launcher rejection, and Python SHA-256 fallback.
- [x] Synchronized installer source, template, and embedded snapshot.
- [ ] Run full repository gates and inspect final diff.

## Surprises & Discoveries

- Full updates should adopt the selected template contract epoch; preserving v2/v3 is specific to launcher-only repair because v4 templates intentionally remove legacy jig_version fields.

## Decision Log

- Reused the guarded existing-destination mutation transaction for the two repair scripts so late seed failure can restore exact preimages.
- Extracted launcher drift validation into scripts/check-launcher-template.sh so release validation and fixture validation execute the same check.
