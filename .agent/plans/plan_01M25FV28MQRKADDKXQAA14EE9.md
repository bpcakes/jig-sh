Move Claude and Codex homes, selection, launch preparation, and inspection adapters behind an internal provider boundary. Preserve provider JSON and native configuration semantics. Validate shared workflow with a third test provider, launcher integration tests, and full repository checks.
# Common agent-provider implementation

## Progress

- [x] Inspected provider, picker, launch, and usage boundaries at baseline 9d4bd0f27512c65b961df07768444d4125ef4309 (the merge of PR #25).
- [x] Added internal AgentProvider and optional SessionProvider interfaces; migrated both providers and CLI homes/launch dispatch.
- [x] Added explicit primary subscription bucket input to the picker.
- [x] Characterize third-provider orchestration and configuration identity; run integration and repository checks.
- [x] Update guides and extension documentation; inspect fresh evidence and finish structured work.

## Surprises & Discoveries

The public JSON reports intentionally differ: Codex always inspects accounts while Claude's plain listing performs directory discovery only. Retain report-building and inspection scheduling in provider adapters. The installed jig binary is distinct from target/debug/jig; validation must force JIG_DEV_BIN. At implementation start the workspace was on master after PR #25 merged, so this is a follow-up change rather than an update to that PR.

## Decision Log

Use an internal generic trait with an associated Home type so provider configuration modes remain typed. No dynamic plugin loading or Cursor implementation is part of this change. Discovery entries carry provider-independent display metadata and a separate provider-owned selection. An optional session capability supports Codex resume without imposing it on other providers. Keep existing public TUI APIs compatible while adding an explicit provider entrypoint; the legacy crate name is not changed.

## Outcomes & Retrospective

The final repository check passed all five targets, including 3,994 tests with 2 skipped, Clippy, formatting, contract validation, and file-budget checks. The final no-default-features all-target compilation passed. A third test provider exercises capability checks, exact configuration selection, report dispatch, dry-run preflight, and inspection cancellation/error forwarding. Both provider launcher integration suites passed; the Codex progress-header regression was corrected and verified. The first full test run passed 3,993 tests but its receipt was invalidated when the relocated warning test changed during execution; the final run covers the stable tree including that test. Beads task: jig-sh-o4f9.

## Context and implementation

crates/jig/src/agent_provider.rs owns provider interfaces and launch/discovery values. claude/provider.rs and codex/provider.rs implement them. cli/agent_run.rs owns shared argument checks, supervised reports, picker lifecycle, revalidation, dry runs, and transparent execution. Provider-specific CLI parsers and JSON renderers remain compatibility adapters. jig-codex-tui receives explicit quota identity instead of inferring the active provider.

## Validation and acceptance

Both existing CLI families must retain --usage --json and --dry-run, native argv/cwd/environment, output schema, exit status, and cancellation behavior. Claude native default and explicit override sharing one path must stay distinct. A test-only third provider must exercise the common workflow without adding a production provider-name branch. Unsupported usage must fail before provider work. Explicit unknown-provider quota IDs must support primary selection and recommendations; no primary subscription capability must not imply a recommendation. Run targeted TUI/provider/launcher tests, build the dev binary offline, then JIG_DEV_BIN=target/debug/jig scripts/jig check test and work check/gates/evidence/finish. Review the diff and no-default-feature compilation.

## Recovery

Edits are a same-release internal cutover; no persisted data migration is required. Preserve all existing append-only state records. Correct failures in place and rerun only affected verification until the complete required check passes.

Final evidence: required verify gate passed with all five target receipts fresh. Work plan and session closed with outcome success; Beads task jig-sh-o4f9 closed.
