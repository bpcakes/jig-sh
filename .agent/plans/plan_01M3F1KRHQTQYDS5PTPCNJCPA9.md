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

Remaining validation: run the complete Python launcher suite, the configured
local verification gates, and GitHub CI on the pushed branch. Inspect gate and
receipt freshness, finish this plan, and commit the resulting evidence.
