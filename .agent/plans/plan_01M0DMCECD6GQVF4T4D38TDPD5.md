# Preserve all partial progress from failed dev stops

This work fixes a lifecycle reporting defect without broadening the public command contract. A stop operation can commit orphan recoveries and accumulate operator warnings before a later filesystem or lock operation fails. The current failure outcome carries recoveries but leaves warnings in local vectors, so the final JSON reports an empty warning list.

## Progress

- [x] Diagnose the failure path and review the applicable crate invariants.
- [x] Run the Fowler heuristic scanner and validate its relevant signal manually.
- [x] Establish the narrow code baseline (`cargo fmt --all -- --check`; 570 dev-proxy tests).
- [ ] Introduce a private `StopProgress` carrier while preserving existing output behavior.
- [ ] Preserve accumulated warnings in failed stop output and add regression coverage.
- [ ] Run all configured repository gates and close the work plan.

## Surprises & Discoveries

- The defect is not an isolated JSON typo. `stop_session_ids_interruptible_with_policy` already promotes recoveries into `StopSessionOutcome::Failed`, while `control_warnings` and `lifecycle_warnings` remain local to the inner function. The type boundary therefore makes it easy to preserve one category of partial progress and silently lose another.
- The refactoring scanner flags the raw `&mut Vec<OrphanRecoveryNotice>` parameter and the long stop function. The raw vector finding is relevant because it exposes the incomplete partial-progress concept; the file and function size findings alone are not sufficient reason for a wider rewrite.

## Decision Log

- Use Fowler's **Introduce Parameter Object** and **Preserve Whole Object** moves to create one private carrier for committed recoveries and accumulated warning candidates.
- Keep `StopSessionOutcome` as a closed enum. A trait or command-object rewrite would add variation that the domain does not have.
- Separate the behavior-preserving carrier extraction from the bug fix so each commit has one purpose and a green verification point.
- On failure, report all warnings accumulated so far. The normal success path may still filter warnings against the final snapshot; a failed path has no trustworthy final snapshot with which to discard them.

## Outcomes & Retrospective

Pending implementation and final verification.

## Context and orientation

The implementation is in `crates/jig-dev-proxy/src/dev_sessions/management.rs`. `stop_session_ids_interruptible_with_policy` owns the outer error boundary, while `stop_session_ids_interruptible_inner` performs authenticated stop requests, orphan retirement, state mutation, and final reporting. `StopSessionOutcome` transports complete, cancelled, or failed results to direct stop and replacement callers.

The compatibility-sensitive surface is the JSON returned by `dev stop`. Existing successful and cancelled behavior must remain unchanged during the refactoring slice. The behavior slice changes only failed results from `warnings: []` to the warnings actually accumulated before failure. Persistent state formats, signaling authority, process observation, cancellation, and route mutation ordering are out of scope.

## Plan of work

First, introduce `StopProgress` and migrate recovery ownership through the existing enum variants and callers without changing serialized output. Run focused dev-proxy tests and formatting, then commit the refactor.

Second, move control and lifecycle warning candidates into `StopProgress`, derive final success warnings from that carrier, and serialize its accumulated warnings on failure. Add a deterministic unit regression at the outcome boundary that proves recoveries and warnings survive together. Run focused tests and commit the behavior change.

Finally, rebuild the development Jig binary, run `scripts/jig work check`, configured gates, evidence, and receipts, then inspect the final diff and close the work plan.

## Concrete steps

1. Run `cargo test -p jig-dev-proxy` and `cargo fmt --all -- --check` as the narrow baseline.
2. Edit only `crates/jig-dev-proxy/src/dev_sessions/management.rs` for the structural slice.
3. Run `cargo fmt --all -- --check` and `cargo test -p jig-dev-proxy dev_sessions` after the structural slice; commit it separately.
4. Add warning storage and regression coverage in the same module.
5. Run `cargo fmt --all -- --check` and `cargo test -p jig-dev-proxy dev_sessions`; commit the behavior slice separately.
6. Build `jig`, set `JIG_DEV_BIN=target/debug/jig`, and run the configured work checks and gates.

## Validation and acceptance

- Failed direct-stop JSON retains every recovery and warning accumulated before a later error.
- Complete stop output keeps its current filtering: warnings for sessions that no longer remain are omitted.
- Replacement cancellation and failure continue to retain committed recovery notices.
- No persistent JSON schema, public Rust signature, process signaling, or route-mutation behavior changes.
- All configured test, format, Clippy, and contract gates pass.

## Idempotence and recovery

The code changes are internal and safe to reapply from a clean commit. Each implementation slice must be green before commit. If the behavior slice cannot be verified, stop on the committed behavior-preserving carrier extraction rather than mixing an unproven contract change into it. Jig state files are append-only and must not be rewritten.

## Interfaces and dependencies

No new dependency or public type is required. `StopProgress` remains private to `dev_sessions::management`. The existing `StopReport`, `StopSessionOutcome`, direct-stop JSON renderer, and replacement caller are migrated using ordinary owned values; no cloning, allocation class, locking, or cancellation boundary is added.
