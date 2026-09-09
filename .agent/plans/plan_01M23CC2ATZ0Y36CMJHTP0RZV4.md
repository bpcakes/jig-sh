# Preserve current verification through metadata and focused retries

Consumer: the ExampleProject maintainer completing a release after tracker bookkeeping or a focused failed-check retry. Feature: work gates accepting current original target receipts. Observed defects: whole-source freshness treats tracker updates as application changes; selecting one complete run discards unrelated passing target receipts. Delete this recovery plan after a successor replaces the verification behavior.

## Progress

- [x] Inspect current planner, source identity, gate evaluation and pending targeted-retry implementation.
- [x] Integrate pending retry implementation in an isolated checkout and resolve current CLI reporting conflicts.
- [x] Add explicit typed tracker metadata classification; preserve conservative default freshness.
- [x] Validate metadata/source/configuration/documentation boundaries, latest failures, dependency ordering and archive retention in 287 focused cases; repair the imported native retry fixture to open a real plan with a captured baseline.
- [x] Rebuild the runtime, run required harness checks, inspect evidence and finish only with current passing gates.

## Surprises & Discoveries

Current main already binds the work-plan ID during check planning. The pending retry implementation was based on an older CLI reporting API; retain current summaries alongside original receipt/run provenance. The shared tracker resolves to the canonical database; use its privacy-aware sync helper and do not overwrite another worktree's changes.

## Decision Log

Use `[work] receipt_metadata = ["beads"]` as an explicit declaration that the root tracker store is not an input consumed by gated application/build/test/policy commands. The default remains conservative. This enum does not accept arbitrary paths or globs; docs, source, runner/configuration bytes, nested fixture tracker directories and packaged instructions remain freshness authority. A configuration change invalidates earlier receipts. This bounded classification is separate from the larger canonical-action-input design and affected-selection ignores.

Targeted retries select the latest original outcome per required target and preserve dependency ordering, source/configuration/input/time freshness and plan identity. A failure or unverifiable outcome supersedes an older success. Revalidation has its own batch receipt but never creates replacement target successes. Archive protection retains the actual selected proof records.

## Implementation and validation

Work in crates/jig/src/context/work_config.rs, git_receipts/metadata.rs, git_receipts/worktree.rs and git_receipts.rs for source projection. Work receipt selection and retry scheduling live in state/receipts/target_evidence.rs and runtime/work/checks/targets.rs. Use real temporary Git repositories and command side effects to prove reuse and invalidation. The existing runtime evidence tests cover profile/target retries, newer failures, dependencies, time expiry and archive safety; run the entire relevant groups after integration, then required repository gates using a rebuilt `JIG_DEV_BIN=target/debug/jig`.

Do not edit the other pending worktrees or claim the separate canonical-input design complete. Preserve append-only state and source identity. Inspect every failing assertion and keep checks' meaning intact. No downstream identifiers belong in this change or its evidence.

The source repository also duplicated its complete verify profile through legacy contract, formatter, Clippy and four test-partition gates. Remove those legacy gate entries: the unchanged profile still runs the same formatter script, Clippy command, full Nextest workspace inventory, contract and file-budget policy. Named partition commands remain available for focused development.

The initial direct Cargo test invocation exceeded the process-heavy suite's documented concurrency ceiling and produced process-tree cleanup failures. An isolated receipt rerun passed; the configured four-worker Nextest run passed 286/287, with the remaining imported native retry fixture failing because its legacy seeded plan lacked a comparison baseline. Opening a real baseline-bound plan fixed that test; its focused rerun passed. Assertions and cleanup deadlines were not weakened. Required full-profile validation follows on the committed source.

## Outcomes & Retrospective

Full configured local workspace Nextest, formatting, strict Clippy, contract and baseline-bound file-budget checks passed on source 45449a67ba897fd924b92d6f24a7138c5dd159d2. A subsequent work check executed zero targets and reused all five original receipt/run identities; work evidence and gates reported fresh passing results.

PR CI exposed existing portability defects outside receipt selection: database-bootstrap fixtures probed host Bun before installing their own fake manager, and three argv tests compared macOS symlinked temporary paths lexically against the physical working directory. The fixtures now own the initialization probe and compare canonical directory identity while preserving literal argv/environment assertions. The Bun double proves bootstrap orchestration only. Five initial portability cases and the complete nine-case argv runner/schema group passed locally. The subsequent local workspace run passed all 3938 selected cases, but its receipt correctly failed freshness because the last macOS fixture correction was made while that run was executing. The fresh full-profile check on a25543211c6e524a7481777a31c29b4026f3a47b passed all five required targets, including all 3938 selected workspace tests (two configured skips). Revalidation then executed zero targets and retained all five original receipt/run IDs; evidence and gates were fresh and passed. All 16 Rust CI jobs passed in https://github.com/bpcakes/jig-sh/actions/runs/34376387757, including both macOS test configurations; repository policy and agent-guide workflows also passed. The work plan and both associated tracker items are closed; no independent review or upstream release is claimed.
