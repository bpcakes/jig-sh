Address the comprehensive review and commit the completed Batter-only application
scaffold, including the prior upgrade to revision 39c1b6e.

Implemented:

- Rust-react planning rejects normalized package names batter and batter-axum
  before any destination writes, with a diagnostic asking for another --repo-name.
  The check is application-specific; standalone rust-library/rust-cli remain valid.
- Regression coverage exercises exact/case/underscore variants across all three
  database modes, absent destinations inferred from directory names, and explicit
  names with --force against an existing manifest. Failure leaves the destination
  absent or preserves its existing bytes and file inventory. A positive test renders
  both standalone presets and verifies their package manifests for the same names.
- Generated README and runtime guide explain jig dev's two-second SIGTERM grace,
  its ability to preempt Batter drain/cleanup, direct-binary shutdown testing, and
  production process-manager budget alignment. The dev proxy policy is unchanged.
- Embedded snapshots refreshed through the build script.

Focused collision and standalone-preservation regressions both pass. The initial
test implementation needed a qualified tempfile helper and correction from the
workspace manifest to the standalone crate manifest; corrected without weakening
the filesystem preservation assertions.

Repository formatting, strict Clippy, contract and file-budget checks passed.
The full repository test check passed against the final source: all 4,070 tests
across 47 binaries passed, with three skipped.
Its first run stopped after 3,582 passes because a CLI fixture clone failed with
ENOSPC on the shared /tmp filesystem, not an assertion against changed behavior.
Removed 8.6 GiB of this task's regenerable generated-workspace Cargo artifacts and
reran the full gate with a private TMPDIR on the home filesystem (48 GiB available).
The failed receipt remains preserved; no tests or production code were weakened.
All five required targets passed with current evidence; work check validation
receipt: receipt_01M2B2FS45BA1BJ89XCMKFJ7J1. The private test temporary directory was
empty after the successful run and has been removed. Commit the reviewed cutover,
upgrade, review fixes, tests, documentation, templates and append-only evidence.
