PR 27 macOS CI reported cleanup failure instead of output overflow. Reproduced EPERM with a short-lived Python output producer. Retry inconclusive EPERM within the existing deadline while retaining pinned identity and terminal plus sole-leader confirmation. Validate deterministic retry/deadline/identity tests, process and original failing tests, 2000 short-lived overflow commands, and repository gates.

## Evidence and implementation

Baseline: 50630c4a on feat/common-agent-providers (PR 27). The macOS locked test failed in argv_results_use_the_declared_parser_and_output_limit: expected failure with an output-limit finding, received blocked due to process cleanup. Forty focused repetitions passed, but a direct short-lived output producer reproduced EPERM at iteration 408. Temporary diagnostics were removed.

The macOS EPERM classifier now permits the existing bounded confirmation loop to retry while the direct child remains pinned, including when waitid still reports Running during exit. EPERM does not prove absence; successful cleanup still requires a fresh terminal observation and an atomic sole-leader snapshot. ECHILD and other observation errors stop retries. Persistent denial remains bounded by the original cleanup deadline. Linux behavior is unchanged.

Tests characterize Running and Exited observations after EPERM, deterministic delayed quiescence and deadline exhaustion, and 100 short-lived output-overflow commands. The process crate passed all 39 tests through the configured nextest runner. A preliminary unrestricted cargo test run passed the new regressions but hit an existing pipe test WouldBlock timing failure; the configured runner passed that test.

Beads: jig-sh-ruxv. Final repository gates, the original failing regression, and stress validation are recorded with this plan.

## Outcome

Twenty stress repetitions of the 100-command regression passed (2,000 short-lived output producers). The final repository check passed all five targets: 3,996 tests passed with 2 skipped, plus formatting, Clippy, contract, and file budgets. The original failing argv parser/output-limit test passed in that suite. New regressions live in process/tests/macos_exit.rs to respect the existing test-file budget.
