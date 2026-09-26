# Local validation startup cancellation

PR #51 follow-up, baseline `c5c94256780709f5396095955c7c105517ebe186`.
Plan: `plan_01M3F1KRHQTQYDS5PTPCNJCPA9`.

The original wrapper only signals its immediate child. During runtime selection,
the real shell launcher is waiting for an installer in command substitution;
its descendants can survive cancellation and retain the import lock.

The regression uses the real launcher with a generic waiting installer and a
lock-holding helper. All five tests failed before the fix: PID-directed SIGINT
and SIGTERM never reached the helper, while a group-directed SIGINT let the
wrapper return before helper cleanup. All five passed after adding a separate
phase process group, descendant cleanup confirmation, and bounded escalation.
The existing five Rust local-validation integration tests also passed.

Retain the unreaped phase leader until cancellation cleanup finishes to pin the
process group identity. Signal handlers only record the first cancellation;
normal child polling blocks cancellation signals across the reap decision.
Use portable ps group/state columns because older macOS Python versions do not
provide waitid. Zombies no longer execute or hold locks.

Verification on implementation commit `a0c1854d`:

- `python3 -m unittest discover -s scripts/tests -p 'test_jig_*.py'`: 25 passed.
- `cargo test -p jig-sh --test local_validation`: 5 passed during implementation.
- `scripts/check-local --plan-id plan_01M3F1KRHQTQYDS5PTPCNJCPA9`: all six
  required targets passed. The final phase reused all five preflight receipts;
  `api:test` passed 4,684 tests across 53 binaries, with four configured skips.
  Test receipt: `receipt_01M3F2HAES812KPM3BBS4Q6T2J`.

The fix and local verification are complete. GitHub CI is being monitored on the
pushed PR branch; its final status is reported in the task response.
