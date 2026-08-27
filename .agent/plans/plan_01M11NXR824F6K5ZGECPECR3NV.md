# Bring branch Rust files within LOC policy

## Progress

- [x] Enumerate all 22 policy failures against the merge base.
- [x] Extract cohesive production-code sections without behavior changes.
- [x] Split oversized test suites at top-level test boundaries.
- [ ] Run formatting, focused tests, Clippy, full tests, and the LOC gate.
- [ ] Commit each verified structural slice and update the PR.

## Surprises & Discoveries

- The failures mix newly oversized files with grandfathered legacy files that grew.
- Moving path-based `mod tests;` declarations into included parts changes Rust's module-file resolution; those declarations must remain in their original owner files.
- The focused `cargo test -p jig-sh --lib` suite passed 1,691 tests after extraction.

## Decision Log

- Use same-module include parts where existing private-item coupling makes a child module unnecessarily invasive; use named modules where boundaries are already explicit.
- Keep every new Rust part below the hard limit and preserve all cfg and test attributes.

## Outcomes & Retrospective

- Pending.

## Context and orientation

The changed-file Rust LOC gate compares this branch with master. Existing oversized files may remain grandfathered only if they do not grow; newly added or newly oversized files must be split below policy limits.

## Plan of work

Move contiguous top-level items into focused part files, retain imports and module attributes in their owning files, and preserve item order through include declarations.

## Validation and acceptance

`scripts/check-rust-file-loc.sh master` reports zero errors; formatting, contract, Clippy, and test gates pass; generated fixture validation remains green.
