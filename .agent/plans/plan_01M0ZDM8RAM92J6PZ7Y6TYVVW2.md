# Implement change-aware, evidence-safe work gates

This ExecPlan is a living document. The sections `Progress`, `Surprises & Discoveries`, `Decision Log`, and `Outcomes & Retrospective` must be kept current while work proceeds. This plan follows `.agent/PLANS.md` and is the durable body for Jig work plan `plan_01M0ZDM8RAM92J6PZ7Y6TYVVW2`.

## Purpose / Big Picture

Jig currently requires every configured work gate for every structured plan, regardless of which files the plan changes. A frontend-only change in a Rust, TypeScript, and SQLx repository therefore runs backend tests and PostgreSQL checks; a backend-only change runs every frontend check. Receipts are tied to the entire dirty worktree and to one plan, so unrelated edits stale all evidence and an identical follow-up plan reruns the complete matrix.

After this change, repositories can attach repository-relative path rules to a required gate. `scripts/jig work check` will classify the plan's changes against those rules, run only required applicable gates, record explicit `not_applicable` evidence for skipped gates, retain evidence across unrelated edits through gate-scoped fingerprints, and optionally reuse a successful same-input check from another plan. Generated full-harness repositories will use these capabilities so Rust, SQLx, and individual frontend application checks run only for relevant changes. Human and JSON output will explain the plan baseline, changed-path classification, executed, reused, and not-applicable gates. The native wiring check will be presented as `jig-contract`, not the ambiguous `contract` gate.

## Progress

- [x] (2026-08-26 16:17Z) Inspected the existing gate configuration, plan event, receipt index, work-check execution, worktree fingerprint, bootstrap template, and human output paths.
- [x] (2026-08-26 16:17Z) Opened structured work plan `plan_01M0ZDM8RAM92J6PZ7Y6TYVVW2` with the current development binary.
- [x] (2026-08-26 16:17Z) Wrote this self-contained implementation and validation plan.
- [x] (2026-08-26 17:17Z) Added contract-v5 configuration, plan-baseline state, strict glob validation, and cancellation-aware baseline-relative Git snapshots.
- [x] (2026-08-26 17:17Z) Added applicability-aware execution, explicit not-applicable evidence, scoped freshness, gate-ID selectors, and opt-in exact-input evidence reuse.
- [x] (2026-08-26 17:17Z) Added per-app generated frontend tools and path-aware generated Rust, frontend, and SQLx gates; renamed the generated wiring gate to `jig-contract`.
- [x] (2026-08-26 17:17Z) Updated CLI/MCP/JSON/human/UI surfaces, adoption reconciliation, documentation, changelog, compatibility guidance, and embedded template snapshots.
- [x] (2026-08-26 17:58Z) Ran focused tests, formatting, strict workspace Clippy, contract validation, and the full configured test matrix through the development binary. The plan-linked run recorded fresh passing contract and test evidence in batch receipt `receipt_01M0ZM03MZYRP2HYK8T9GY529V`.
- [x] (2026-08-26 18:10Z) Delegated the complete 66-file working tree to a subagent using the merged Claude Code plus native Codex comprehensive-review workflow. The first review completed with eight actionable findings, so closure was rejected.
- [x] (2026-08-26 18:58Z) Loop 2 phase 2 implemented every first-review finding: explicit scaffold/application gates, hardened direct-only reuse, exact/alias-only gate migration, legacy default compatibility, Rust config scopes, force-on-unknown execution, and cached plan change discovery. Focused work/receipt/adoption tests, formatting, strict workspace Clippy, diff hygiene, and the dogfooded contract gate pass.
- [x] (2026-08-26 19:16Z) Reran the full matrix through the development binary: 2,195 primary tests plus the isolated vault shards passed, and both required gates are fresh in batch receipt `receipt_01M0ZQYAR2DZGB0V6B82REFBZR`.
- [x] (2026-08-26 19:33Z) Loop 2 phases 3-4 delegated and evaluated a second complete merged review. It found eight new actionable correctness, compatibility, performance, evidence, adoption, and schema issues, so closure was rejected again.
- [x] (2026-08-26 20:17Z) Loop 3 phase 2 addressed all eight second-review findings: one matcher-safe glob grammar, centralized command-authority scopes, legacy epoch rejection, streamed/cached scoped proofs, batched reuse discovery, truthful child exits, ID-and-tool adoption ownership, and selector-schema parity. Focused semantic, large-diff, cache-count, multi-reuse, refinement, adoption, schema, legacy, and scaffold tests pass.
- [x] (2026-08-26 20:58Z) Loop 3 phase 2 verification passed after correcting one legacy-template fixture exposed by the first full rerun: formatting, strict all-target/all-feature workspace Clippy, contract validation, diff hygiene, 2,199 primary tests, and all isolated vault shards are clean. Fresh gate evidence is in batch `receipt_01M0ZVY25P273XSC339HWV3B5T`.
- [x] (2026-08-26 21:18Z) Loop 3 phases 3-4 completed a third merged Claude Code plus native Codex review over the full working tree. It found ten actionable baseline, matcher/fingerprint, execution-authority, refinement, adoption, compatibility, metadata, and status-performance defects, so closure was rejected.
- [x] (2026-08-26 22:24Z) Loop 4 phase 2 implemented all ten third-review findings: unborn empty-tree baselines, one classifier with literal-path pinned Git proofs, global Jig authorities, refinement unknown-evidence aggregation, full application-contract app scopes, field-presence compatibility, migration-owned generated scopes, untracked mode bits, and request-batched status work.
- [x] (2026-08-26 22:24Z) Loop 4 focused verification passed: 50 context tests, 27 Git receipt tests, 67 work-runtime tests, targeted readoption/scaffold tests, formatting, strict all-target/all-feature workspace Clippy, diff hygiene, and embedded-template refresh.
- [x] (2026-08-26 22:43Z) Loop 4 complete verification passed: the development binary ran 2,208 primary tests plus isolated vault shards, and both required gates are fresh in batch `receipt_01M0ZZ4V7JN7EDHJXARY816YKR`.
- [x] (2026-08-26 23:02Z) Loop 4 phases 3-4 completed a fourth merged Claude Code plus native Codex full-tree review. It found seven actionable command-authority, Git-proof, native-identity, scale, status-performance, legacy-compatibility, and output defects, so closure was rejected.
- [x] (2026-08-27) Loop 5 phase 2 implemented all seven fourth-review findings: conservative generated-command scoping, fully pinned/chunked Git proofs, workspace-source native identities, same-baseline snapshot sharing, legacy explicit-tool compatibility, and empty-tree human output.
- [x] (2026-08-27) Loop 5 focused verification passed: 30 Git receipt tests, 70 work-runtime tests, 55 output tests, 50 context tests, 32 adoption tests, targeted scope/custom-wrapper tests, formatting, strict all-target/all-feature workspace Clippy, and diff hygiene.
- [x] (2026-08-27) Loop 5 complete verification passed through the development binary: 2,218 primary tests plus isolated shards, with fresh required-gate evidence in batch `receipt_01M102DVFWCZ5PVFJ8C87WP4EM`.
- [x] (2026-08-27) Loop 5 phases 3-4 delegated and evaluated the fifth full-tree merged review. It found seven actionable packaging, command-authority, schema, Git-proof, workspace-ownership, and receipt-scale defects, so closure was rejected.
- [x] (2026-08-27) Loop 6 phase 1 traced all seven findings to their owning boundaries and designed fail-closed remediations with explicit compatibility and migration behavior.
- [x] (2026-08-27) Loop 6 phase 2 implemented all seven fifth-review remediations: checkout/package native identities, closed generated-command grammars with app binding, schema-dump utility/schema-check gate separation, fail-closed nested Git proofs, persisted adopted frontend workspace ownership, and batch-level changed-path evidence with legacy hydration.
- [x] (2026-08-27) Loop 6 focused verification passed: 336 bootstrap tests, 70 work-runtime tests, 55 output tests, 50 context tests, 33 Git receipt tests, 32 adoption-mode tests, 20 inference tests, targeted package/scope/schema/workspace/receipt regressions, formatting, strict all-target/all-feature workspace Clippy, embedded-template refresh, and diff hygiene.
- [x] (2026-08-27) Loop 6 complete verification passed through the fresh development binary: 2,225 primary tests plus all isolated shards, contract-v5 validation, formatting, and diff hygiene are clean. Both required gates are fresh in batch receipt `receipt_01M1071E352GCGH3T0C09XYY5Z`.
- [x] (2026-08-27) Loop 6 phases 3-4 delegated and evaluated the sixth full-tree merged review. It found seven actionable staged-index, schema authority, generated-scope, stable-interval, and packaged-identity defects, so closure was rejected.
- [x] (2026-08-27) Loop 7 phase 1 traced all seven findings and designed fail-closed fixes that preserve ordinary unstaged work, old-config loading, disjoint-app selectivity, and package-local builds.
- [x] (2026-08-27) Loop 7 phase 2 implemented all seven sixth-review remediations: dual index/worktree classification and proof with partial-stage rejection, committed literal schema ownership, complete public/migration scopes, ancestry-safe frontend ignores, bracketed scope revalidation, and Jig-sentinel checkout identity.
- [x] (2026-08-27) Loop 7 focused verification passed: 34 Git receipt tests, 70 work-runtime tests, 51 context tests, 28 policy tests, 32 adoption-mode tests, 20 inference tests, 17 path/bootstrap tests, 20 scaffold-generation tests, frontend configuration/workflow suites, all new targeted regressions, package file listing, formatting, strict all-target/all-feature workspace Clippy, embedded-template refresh, and diff hygiene.
- [x] (2026-08-27) Loop 7 complete plan-linked verification passed after correcting the runtime signal-policy fixture for the newly literal Git invocation: 2,231 primary tests plus all configured isolated shards passed, and both required gates are fresh in batch `receipt_01M10AWZHYZB7VMGY3XQY9VXTS`.
- [x] (2026-08-27) Loop 7 phases 3-4 delegated and evaluated the seventh full-tree merged review. It verified the seven prior findings but found six further actionable schema, SQLx-scope, readoption, build-identity, frontend dependency, and changed-path scale defects, so closure was rejected.
- [x] (2026-08-27) Loop 8 phase 1 traced all six findings and selected fail-closed remediations with explicit committed-output, precedence, build-configuration, dependency-scope, and discovery-limit invariants.
- [x] (2026-08-27) Loop 8 phase 2 implemented the six seventh-review remediations: Git-attestable schema destinations and explicit ignored/untracked status, complete SQLx authorities, exact-before-alias readoption, build-configuration-bound native identity, deduplicated root/all-app contract dependency preparation, and hard Git-output/path-count discovery ceilings.
- [x] (2026-08-27) Loop 8 focused verification passed: 36 Git-receipt tests, 30 policy tests, 51 context tests, 33 adoption-mode tests, 20 scaffold-generation tests (one expected network test ignored), frontend configuration/workflow suites, four build-identity tests, all six new targeted regressions, embedded-template synchronization, package file listing, formatting, diff hygiene, compile-only coverage, and strict all-target/all-feature workspace Clippy.
- [x] (2026-08-27) Loop 8 complete plan-linked verification passed through the fresh development binary: 2,238 primary tests plus all configured isolated shards passed, and both required gates are fresh with no unresolved evidence in batch `receipt_01M10E3QC7CSBMMXKHBAJ1GM1V`.
- [x] (2026-08-27) Loop 8 phases 3-4 delegated and evaluated the eighth full-tree merged review. It verified the prior six fixes but found seven actionable Git-isolation, proof-bounding, shell-compatibility, reserved-path, build-input, privacy, and compatibility-guidance defects, so closure was rejected.
- [x] (2026-08-27) Loop 9 phase 1 traced all seven findings to their owning boundaries and designed fail-closed remediations, including prevention of future repository-root leakage in receipt previews and the required explicit historical-state privacy migration.
- [x] (2026-08-27) Loop 9 phase 2 implemented all seven eighth-review remediations: deny-by-default known-root Git environments, bounded incremental whole-worktree proof, Bash-3-safe dependency preparation, reserved schema aliases, symlink-free native identity inputs, receipt-time repository-root redaction plus the explicit historical privacy migration, and compatibility-neutral empty-selection guidance. Privacy decision `decision_01M10FXPA8V6Q0BMCJ0AMEG1QB` names all 234 affected receipt record IDs and six affected plan bodies without repeating the removed prefix.
- [x] (2026-08-27) Loop 9 focused verification passed: 51 context tests, 39 serialized Git-receipt tests, 31 policy tests, 70 work-runtime tests, 26 bootstrap-Git tests, 25 bootstrap-path tests, 20 scaffold-generation tests (one expected network test ignored), 12 receipt tests, six build-identity tests, package-file listing, embedded-template synchronization, formatting, diff hygiene, and the complete privacy-migration integrity checks.
- [x] (2026-08-27) Loop 9 complete plan-linked verification passed after correcting one stale test oracle for repository-root redaction: 2,246 primary tests, 445 isolated vault tests, and two serialized vault-TUI tests passed; both required gates are fresh in batch `receipt_01M10HSYG271QF6NYVYGVP3DSR`; JSONL, diff, and focused privacy audits are clean for the newly migrated root.
- [x] (2026-08-27) Loop 9 phases 3-4 delegated and evaluated the ninth full-tree merged review. It verified the eighth-review remediations but found nine actionable ambient-Git, failed-rerun, privacy, resource-bound, duplicate-state, redaction, readoption, and test-isolation defects, so closure was rejected.
- [x] (2026-08-27) Loop 10 phase 1 traced all nine findings to their owning boundaries and selected fail-closed designs that preserve receipt compatibility, append-only evidence semantics, cancellation cleanup, project-owned command overrides, and privacy-record identity.
- [x] (2026-08-27) Loop 10 phase 2 implemented all nine ninth-review remediations: fully canonical and bounded Git proofs, interruption-safe gate supersession, complete path-bounded receipt/session privacy, duplicate-plan-open rejection, removed-app command retirement, and serialized build-policy environment testing. The explicit historical migration replaced 1,248 affected receipt/session/plan record references plus 12 affected plan bodies without changing record-ID sequences; decision `decision_01M10N1MS6HFRK7601WNG2ES14` durably names every affected record and plan ID without repeating removed values.
- [x] (2026-08-27) Loop 10 complete verification rejected closure after the primary 2,254-test suite and the 445-test isolated vault shard passed twice but the serialized PTY browser test timed out twice in the same order-dependent transition. The test passed alone, proving a synchronization defect rather than a deterministic runtime failure; phase 3 review was not started on failed evidence.
- [x] (2026-08-27) Loop 11 phase 1 reduced the failure to a fast outer-TTY reproducer and found the actual isolation defect: the spawned TUI had the PTY slave on fd 0-2 but inherited the runner's controlling terminal because it never created a child session or claimed the slave. The first footer wait exposed but could not repair that boundary.
- [x] (2026-08-27) Loop 11 phase 2 makes both PTY children session leaders, claims fd 0's slave with `TIOCSCTTY` before exec, and waits for the complete activity footer before injecting its close key. Both serialized tests passed once under an outer TTY, then passed ten consecutive outer-TTY suite runs.
- [x] (2026-08-27) Loop 11 complete verification passed in the exact Jig-supervised outer-TTY order: formatting, strict all-target/all-feature workspace Clippy, diff hygiene, 2,254 primary tests, 445 isolated vault tests, and both serialized PTY tests are clean. Batch `receipt_01M10SPZ66JE6XHHV635P1M2YE` reports both required gates passed with no unknown evidence; all state JSONL and the final privacy audit are clean.
- [x] (2026-08-27) Loop 11 phases 3-4 delegated and evaluated the next merged full-tree review. It verified the PTY, Git proof, cancellation, duplicate-plan, readoption, and prior privacy-boundary fixes, but found five actionable archive, build-identity, residual privacy, redaction, and migration-documentation issues, so closure was rejected.
- [x] (2026-08-27) Loop 12 phase 1 designed all five remediations: retain explicit gate-supersession tombstones during archive compaction, derive Cargo rerun directives from the same build-identity inputs, finish the generic-fixture privacy migration with a non-repeating decision amendment, require both path-token boundaries for root redaction, and document the removed ambient schema destination override.
- [x] (2026-08-27) Loop 12 phase 2 implemented all five review findings: archive supersession tombstones with a retained-stream round trip, Cargo invalidators derived from native identity inputs, a completed generic-fixture privacy migration and durable amendment decision, two-sided root-token redaction, and explicit schema-output migration documentation.
- [x] (2026-08-27) Loop 12 complete verification passed: focused archive/privacy/build/status-provider regressions, strict all-target/all-feature workspace Clippy, formatting, diff hygiene, contract validation, JSONL/privacy audits, 2,257 primary tests, and all isolated shards are clean. Both required gates are fresh in batch `receipt_01M10W1RZ6957VG28VQETHNCKZ`.
- [x] (2026-08-27) Loop 12 phases 3-4 delegated and evaluated the merged full-tree review. It reverified every prior finding as closed but found three actionable right-boundary privacy, gate-summary, and refresh-build identity defects, so closure was rejected.
- [x] (2026-08-27) Loop 13 phase 1 designed symmetric delimiter-aware redaction, evidence-authoritative summary counts/details/verdicts, and an input-refresh-before-identity helper with a fixed-point regression.
- [x] (2026-08-27) Loop 13 phase 2 implemented and verified all three findings: symmetric diagnostic delimiters, failed/cancelled evidence counts/details/verdicts, and refresh-before-hash native identity. Targeted and broad suites, two real refresh-build signature comparisons, strict Clippy, formatting, diff/contract/JSONL/privacy checks, 2,260 primary tests, and all isolated shards pass. Both required gates are fresh in batch `receipt_01M10Y5HYS5H9FVD3XRZ390A8K`.
- [x] (2026-08-27) Loop 13 phases 3-4 delegated and evaluated the merged full-tree review. It verified the prior fixes but found three actionable build-source, adopted-checker-interface, and contextual-punctuation defects, so closure was rejected.
- [x] (2026-08-27) Loop 14 phase 1 designed one shared build layout for template selection and hashing, exact bounded v1 checker-marker inference with explicit opt-in, and Unicode-aware paired/contextual punctuation parsing.
- [x] (2026-08-27) Loop 14 phase 2 implemented and verified all three review findings: a single resolved build-source layout now controls both template selection and native hashing, adopted application-contract inference requires the exact bounded v1 interface marker (or explicit owner opt-in), and Unicode-aware contextual redaction preserves punctuation-prefixed siblings while accepting wrappers, sentence punctuation, and Unicode whitespace. Focused regressions, strict Clippy, formatting, diff hygiene, JSONL/privacy audits, two forced refresh builds with the identical gate signature, 2,263 primary tests, and all isolated shards are clean. Both required gates are fresh in batch `receipt_01M110R8A774Q5A0PZRCKQ8ZS8`.
- [x] (2026-08-27) Loop 14 phases 3-4 delegated and evaluated the merged full-tree review. It reverified the package/checkout identity, exact checker marker, and contextual redaction fixes but found three actionable reuse-supersession, non-UTF-8 legacy-fingerprint, and generated-path escaping defects, so closure was rejected.
- [x] (2026-08-27) Loop 15 phase 1 designed latest-exact-evidence dominance for reuse, a bounded raw-index Gitlink probe that preserves Unix path bytes and filters to changed entries, and uniform TOML escaping for every dynamic generated gate path.
- [x] (2026-08-27) Loop 15 phase 2 implemented and verified all three review findings: failed exact evidence now supersedes an older reusable pass, legacy whole-worktree Gitlink probes preserve raw Unix path bytes and inspect only changed entries, and every dynamic generated gate include/ignore value is TOML-escaped. Exact regressions plus 72 runtime-work, 42 Git-receipt, 13 receipt-state, and 13 bootstrap-configuration tests pass; strict Clippy, formatting, snapshot parity, diff, JSONL, privacy, 2,266 primary tests, and all isolated shards are clean. Both required gates are fresh in batch `receipt_01M1136JMK2NZA3MMXA5SNK5M5`.
- [x] (2026-08-27) Loop 15 phases 3-4 delegated and evaluated the merged full-tree review. It verified all three direct remediations but found six actionable adjacent reuse-state, untracked-framing, generated-root, Gitlink-scale, gate-signature, and preview-semantics defects, so closure was rejected.
- [x] (2026-08-27) Loop 16 phase 1 designed length-framed worktree fingerprint v4, explicit direct/tombstone/inert reuse transitions, bounded raw selected Git pathspec chunks, normalized literal generated-root validation, count-framed gate signature v2, and a preview derived from staged closure gates.
- [x] (2026-08-27) Loop 16 phase 2 implemented all six review findings and passed focused/broad Git-receipt, receipt-state, runtime-work, bootstrap, formatting, strict Clippy, snapshot, contract, diff, JSONL, and privacy verification. The first complete matrix passed 2,272 of 2,273 primary tests and exposed one empty optional migration-directory regression, so closure was rejected before review.
- [x] (2026-08-27) Loop 17 phase 1 traced the complete-matrix regression to generated-root normalization running before the established empty-as-omitted migration sentinel and designed a migration-only omission before literal validation.
- [x] (2026-08-27) Loop 17 phase 2 preserved the optional empty migration sentinel while retaining strict validation for every concrete generated root; all 25 init-wizard tests and adjacent answer, invalid-root, and scaffold-error regressions pass.
- [x] (2026-08-27) Loop 17 complete verification passed: formatting, strict all-target/all-feature workspace Clippy, template parity, contract, diff, JSONL, privacy, 2,273 primary tests, and every isolated Vault/PTY shard are clean. Both required gates are fresh in batch `receipt_01M117Z5H63CNNTCKJM7AA07SB`.
- [x] (2026-08-27) Loop 17 phases 3-4 delegated and evaluated the merged full-tree review. It reverified the six Loop 16 fixes and the empty migration sentinel, but found three actionable ignored-replacement, evidence-exit, and non-UTF-8 temporary-path defects, so closure was rejected.
- [x] (2026-08-27) Loop 18 phase 1 designed fail-closed staged-deletion replacement detection for both scoped and legacy whole-worktree evidence, OS-native canonical diff arguments, and status-faithful unknown evidence output. The existing Loop 17 regression already exercises both direct CLI and answers-file empty migration sentinels.
- [x] (2026-08-27) Loop 18 phase 2 implemented all three remediations. Scoped and legacy evidence now reject ignored same-path replacements after staged deletion, canonical order-file arguments preserve raw OS paths, and unknown evidence has no child exit status. All 46 Git-receipt tests, all 72 work-runtime tests, formatting, strict all-target/all-feature Clippy, and diff hygiene pass.
- [x] (2026-08-27) Loop 18 complete verification passed: formatting, strict all-target/all-feature Clippy, template parity, contract, diff, JSONL, privacy, 2,275 primary tests, and every configured isolated shard are clean. Both required gates are fresh in batch `receipt_01M11ADBTCV72VP7FK35N7HZGS`.
- [x] (2026-08-27) Loop 18 phase 3 delegated a fresh merged Claude Code plus independent native Codex review over the full 122-file working tree. Both passes completed and source-validated the Loop 18 fixes and adjacent evidence paths.
- [x] (2026-08-27) Loop 18 phase 4 accepted closure on the explicit verdict `No actionable findings`, with no open questions or material test gaps.

## Surprises & Discoveries

- Observation: `required = false` affects closure only. A default `work check` still executes every configured check gate because `runtime/work/tools.rs::selected_tools` returns `RepoContext::work_check_tools()` without filtering by requiredness.
  Evidence: `crates/jig/src/runtime/work/tools.rs` lines 5-13 and `context/work_config.rs::check_tools` include all check gates.

- Observation: plan open events contain no repository baseline. They persist only identity, timestamp, title, and body path, so path applicability cannot currently mean “changed by this plan.”
  Evidence: `crates/jig/src/state/records.rs::PlanEvent::Open` and `state/plans.rs::plans_open_prepared`.

- Observation: current freshness is whole-worktree and does not include the current HEAD object. It hashes status, staged and unstaged diffs, and untracked contents relative to the current HEAD. The same clean shape at another commit can therefore have the same fingerprint, while any unrelated dirty edit stales every gate.
  Evidence: `crates/jig/src/git_receipts.rs::repo_worktree_fingerprint_inner`.

- Observation: check receipts are indexed by tool within a plan, while review receipts are indexed by gate ID. Path applicability, duplicate tools with different scopes, and per-gate reuse require check evidence to gain a gate-ID identity while retaining a legacy fallback.
  Evidence: `crates/jig/src/state/receipts.rs::WorkGateReceiptIndex`.

- Observation: the generated frontend tools are repository-wide no-argument commands. A `paths` field can skip that whole command but cannot make it validate only one app.
  Evidence: `templates/project/.jig.toml.jinja`, `.agent/jig-contract.json.jinja`, and `scripts/check-webapps.sh.jinja`.

- Observation: a failed `jig.work_check` batch can still contain successful or failed evidence for gates completed before the aggregate failure. Ignoring every nonzero batch loses valid per-gate outcomes and makes a failed gate look missing.
  Evidence: the new `failed_path_aware_check_is_indexed_as_failed_gate_evidence` fixture failed until v2 batch evidence was indexed independently from the legacy aggregate-success anchor.

- Observation: the generated shared frontend include `packages/**` can contain another configured app. Without per-app ignores, editing that app makes every sibling gate applicable even though the command execution itself is atomic.
  Evidence: the two-app generated-template fixture now asserts four reciprocal `paths_ignore` entries per app, while the runtime fixture proves a declared shared package selects both apps.

- Observation: live project templates and the crate's embedded template snapshot are separate release inputs.
  Evidence: `JIG_REFRESH_EMBEDDED_TEMPLATE_SNAPSHOT=1 cargo check -p jig-sh` refreshed the contract, config, AGENTS, and web-checker snapshots after the live templates changed.

- Observation: setting `GIT_OPTIONAL_LOCKS=0` does not isolate a known-root Git probe from ambient `GIT_DIR`, `GIT_WORK_TREE`, `GIT_INDEX_FILE`, replacement-ref, namespace, object-store, or command-scoped configuration variables. The bootstrap boundary already owns a deny-by-default scrubber for exactly this case, but receipt and schema probes did not call it.
  Evidence: the eighth review redirected whole-worktree and schema evidence to another repository while the process still reported success.

- Observation: the whole-worktree receipt proof still captures full status and two binary diffs into memory and concatenates them into a fourth allocation. Changed-path discovery bounds do not protect this earlier proof step.
  Evidence: `git_receipts.rs::repo_worktree_fingerprint_inner` uses three unrestricted `git_output` calls and one aggregate `Vec` before every work check.

- Observation: Bash 3.2 treats expansion of a declared-but-empty array as an unbound variable under `set -u`. A generated project with independent app manifests and no root package therefore aborts while preparing application-contract dependencies.
  Evidence: `prepare_application_contract_dependencies` initializes `prepared_scopes=()` and immediately expands it in the first generated app loop.

- Observation: schema destination validation accepts the repository root and compares reserved components case-sensitively at only the first component. Root output makes ordinary dirty state indistinguishable from generated schema drift, while case aliases are unsafe on case-insensitive filesystems.
  Evidence: `validate_schema_docs_dir` accepts `.` and rejects only exact lowercase `.git`/`.agent` prefixes.

- Observation: native build identity silently skips symlink entries discovered below `src`, `crates`, or `templates`, even though Rust and template builds may consume their targets.
  Evidence: `build_identity.rs::collect_files` handles directories and regular files only and otherwise continues without error.

- Observation: pinning repository selection and output size does not by itself canonicalize Git diff semantics. Whole-worktree legacy fingerprints still omit `--no-ext-diff`, `--no-textconv`, and canonical file-mode configuration, so ambient or repository-local Git configuration can execute a program or make different contents hash identically.
  Evidence: the ninth review reproduced identical legacy fingerprints for distinct tracked contents under an external diff command.

- Observation: a work-check batch identifies selected tools but not every selected gate, and several pre-start cancellation or launch-error exits break before appending gate evidence. The receipt index only supersedes gate IDs that appear in evidence, so an older pass can survive a failed rerun and remain reusable.
  Evidence: `runtime/work/checks.rs` breaks before `gate_evidence_from_scope` in the cancellation and error branches; `state/receipts.rs` updates only hydrated gate entries.

- Observation: receipt-only repository-root redaction neither protects persisted session summaries nor repairs legacy roots and downstream operational material already present in the durable state and plan bodies.
  Evidence: the ninth review counted affected receipt/session records and plan bodies outside the first migration's exact prefix; `state/sessions.rs::build_summary` persists the raw source path.

- Observation: baseline-to-current scoped binary diffs stream into temporary files with no byte ceiling before hashing, and dirty-submodule status uses an unlimited generic capture even though it needs only one byte to prove dirtiness.
  Evidence: `git_output_to_temp` checks no stdout size while Git is running; `run_git_command_with_cancellation` permits `usize::MAX` on both streams.

- Observation: single-plan baseline lookup selects the first `Open` record while batch lookup overwrites with the last. Corrupt duplicate-open state can therefore produce caller-dependent applicability and fingerprints.
  Evidence: `plan_baseline*` uses `find_map`; `plan_baselines_with_cancellation` assigns every matching event.

- Observation: raw root substring replacement changes strings that merely begin with the same bytes, such as a sibling backup path, even though those strings are not beneath the repository.
  Evidence: `state/receipts.rs::redact_repository_root` uses unconditional `str::replace`.

- Observation: readoption retirement recognizes only fixed blanket frontend defaults. Exact generated per-app commands from an app removed during readoption are therefore mistaken for project-owned commands and reinserted.
  Evidence: `bootstrap/runtime_config.rs::GENERATED_FRONTEND_COMMAND_DEFAULTS` has no prior-app-derived entries.

- Observation: the native build-configuration environment test reads and may execute environment-selected compiler configuration without acquiring the environment lock used by sibling tests.
  Evidence: `lib.rs::build_configuration_records_the_final_template_pin_policy` calls `configuration_from_environment` directly.

- Observation: receipt previews can persist the absolute repository root emitted by compiler diagnostics, and hand-authored plans can repeat it. Because receipt state is append-only, correction requires an explicit privacy migration that preserves record IDs and all unrelated bytes, plus prevention at the recording boundary.
  Evidence: the eighth review found the workstation-specific prefix in active receipt records and this plan.

- Observation: the v5-specific empty-selection error was applied to legacy contracts even though pre-v5 selection is tool-oriented and has no required-gate applicability semantics.
  Evidence: `runtime/work/checks.rs::check_selected_with_failure_mode` emits only the v5 gate-oriented guidance.

- Observation: archival protection previously understood only successful legacy work-check batches and direct tool receipts. Native v2 `not_applicable` evidence has no child receipt, and reused evidence points at source receipts that can belong to a closed plan, so archival could otherwise remove evidence needed to close an open plan.
  Evidence: `state/receipts/archive.rs::ReceiptProtectionIndex` now indexes the latest v2 batch by open plan and configured gate ID and protects its direct or reuse-source receipt references; the focused archive-index test covers both N/A and reused outcomes.

- Observation: the first comprehensive review found that atomic app typecheck/build commands removed two checks that the prior all-app commands performed: generated application-contract freshness and public-artifact boundary scanning.
  Evidence: `scripts/check-webapps.sh.jinja` runs `scripts/contracts.mjs check` only in blanket `typecheck` and `public-check` only after blanket `build`; v5 retires those blanket gates in favor of atomic app gates.

- Observation: reuse eligibility is weaker than normal gate freshness. It accepts an exit-zero work-check batch even when its before/after worktree proof failed, accepts already-reused evidence without validating the chain, and can alternate evidence between two plans after either plan has already been checked.
  Evidence: `state/receipts.rs::reusable_work_check_evidence_with_cancellation` checks aggregate exit status, signature, scope fingerprint, and a successful child only; it does not reject `worktree_fingerprint_error`, require a stable batch fingerprint, restrict sources to direct `executed` evidence, or suppress reuse after current-plan gate evidence exists.

- Observation: v5 readoption reconciliation uses arbitrary same-tool matching as an ownership signal. A project-owned custom gate can therefore be consumed and renamed merely because a newly generated gate uses the same tool.
  Evidence: `bootstrap/runtime_config.rs::reconcile_work_gates` chooses a same-tool gate and only prefers an exact ID by sort order.

- Observation: the runtime supports contracts 2 through 5, but required-only default selection was applied to every supported contract. That changes v4 and older `work check` behavior before adoption.
  Evidence: `runtime/work/tools.rs::selected_checks` filters default gates by `required` without consulting `RepoContext::contract_version()`.

- Observation: generated Rust gate scopes omit tool-owned configuration, explicit force selection stops before command execution when scope discovery is unknown, and baseline path discovery repeats once per gate.
  Evidence: generated paths omit rustfmt, Clippy, and Nextest config files; `checks.rs` handles `scope.error` before `force`; `scope.rs` calls the full `gate_scope_snapshot` path separately for every gate.

- Observation: the accepted matcher language and Git's pathspec language are not identical. In particular, `globset` accepts brace alternation while Git treats the same braces literally, so applicability and tracked-diff fingerprinting can disagree and stale evidence can remain fresh.
  Evidence: the second review reproduced `crates/{api,cli}/**` matching in `globset` but not in `git diff ':(top,glob)...'`.

- Observation: generated frontend/application/public scopes duplicate incomplete command-authority lists. Package-manager configuration, patch/plugin/release inputs, helper scripts, Node version authorities, and Cargo/toolchain configuration can alter execution without making every consuming gate applicable.
  Evidence: `check-webapps.sh` fingerprints `.npmrc`, Yarn/Pnpm/Bun configuration, patches, plugins, releases, and nested authorities that the generated gate arrays omit; `public-artifacts` also rebuilds apps without the root lock/manifests in its policy.

- Observation: v5-only path/reuse fields deserialize under contracts 2-4 even though legacy default checks intentionally execute raw tools and cannot emit the scoped gate evidence those fields require.
  Evidence: legacy selection in `runtime/work/tools.rs` returns `SelectedCheck::Tool`, while `runtime/work/gates.rs` refuses legacy direct receipts for a gate with `paths` or `reuse`.

- Observation: plan-level changed-path discovery is cached, but scoped binary diff collection still repeats per gate and buffers the complete patch. Reuse lookup similarly reparses the complete receipt stream independently for every reusable gate.
  Evidence: `gate_scope_fingerprint` invokes `git diff --binary` for every evaluation and `reusable_work_check_evidence_with_cancellation` owns a full JSONL scan per gate.

- Observation: v2 failed gate evidence does not store the child exit status. A refinement batch intentionally exits zero while collecting failures, so gate JSON can report `status: failed` with `exit_status: 0` from the aggregate batch.
  Evidence: `EvaluatedReceipt::scoped` falls back to `status.batch.exit_status` for failed evidence.

- Observation: exact generated gate IDs are currently treated as migration ownership even when the existing gate uses another tool, and the work-check MCP schema rejects present-but-empty selector arrays that runtime accepts.
  Evidence: `generated_gate_matches_existing` returns true on ID alone; `work_check_input_schema` rejects both properties by presence while `selected_checks` rejects them only when both are nonempty.

- Observation: an implicit `HEAD` baseline is unavailable in a newly initialized repository before its first commit, making every generated path-aware gate unknown and blocking the generated quickstart.
  Evidence: the third review traced `state/plans.rs::plan_baseline_for_open` through `runtime/work/scope.rs`; the missing commit is persisted as an error and no current gate can classify.

- Observation: passing the original configured globs to Git preserves a second matcher with different directory semantics even after brace and mixed-double-star rejection. Bare directory excludes can remove a subtree from Git's patch while Globset still classifies its descendants as matching.
  Evidence: `paths = ["**"]` with `paths_ignore = ["docs"]` classifies `docs/file.md` in scope but Git treats `docs` recursively as an exclude pathspec.

- Observation: a gate-definition edit can produce fresh not-applicable evidence because `.jig.toml` and `.agent/jig-contract.json` are not universal gate authorities. Adopted application-contract scripts can similarly own generated clients beneath app trees omitted by the scaffold-shaped default scope.
  Evidence: generated scopes omit `.jig.toml`; the application-contract gate includes app manifests but not complete configured app trees.

- Observation: refinement's collect-failures mode still treats unknown applicability as a fatal command error, and its `checks_passed` helper examines only executed child results rather than unknown/failed gate evidence.
  Evidence: `runtime/work/checks.rs` breaks and returns `Err`; `runtime/work/review/evidence.rs::checks_passed` ignores `gate_evidence`.

- Observation: default-valued `reuse = false` and `paths_ignore = []` lose field-presence information during deserialization, so contracts 2-4 accept spellings the v5 migration boundary promises to reject.
  Evidence: `WorkGateConfig` stores concrete default values and epoch validation can observe values but not source presence.

- Observation: readoption preserves all exact-ID generated path policies forever. Authority fixes therefore reach new repositories but not an existing repository whose generated scope was never intentionally customized.
  Evidence: `bootstrap/runtime_config.rs` copies `paths` and `paths_ignore` from every exact ID/tool match over the newly rendered defaults.

- Observation: untracked file fingerprints omit Unix permission bits; legacy-plan status repeats whole-worktree hashing per gate and rereads plan state per open plan; scoped patch bytes also inherit ambient Git diff settings.
  Evidence: `append_untracked_file_fingerprint` hashes contents only; each legacy evaluation calls the full fingerprint path; `open_plan_snapshots_with_cancellation` reloads a baseline per plan; scoped `git diff` does not pin diff formatting.

- Observation: generated narrow path scopes are unsafe when a preserved project command invokes a wrapper or dependency outside the nominal Rust/frontend/SQLx paths. Updating generated path fields during readoption can replace a broad old scope while retaining that custom command.
  Evidence: the fourth review reproduced `rust_test_command = "scripts/test.sh"` with `rust-tests` not selecting a change to `scripts/test.sh`; the same structure exists for other command-backed generated gates.

- Observation: the pinned diff proof is incomplete. `core.fileMode=false` suppresses tracked mode-only changes during classification and proof, while `diff.context` still changes proof bytes.
  Evidence: the fourth review reproduced an invisible tracked `0644` to `0755` change and a fingerprint change caused only by `diff.context=0`.

- Observation: native gate signatures omit native implementation identity, matching paths are transported in one unbounded argument vector, and same-baseline open plans still build independent plan-change and scoped-proof snapshots.
  Evidence: `gate_signature` hashes resolved commands only for command tools; `gate_scope_input_fingerprint` adds every matching path to one Git invocation; multi-plan status constructs one eager `PlanGateContext` per plan.

- Observation: explicit `--tool` selection was unintentionally migrated for contracts 2-4, and unborn human summaries omit the empty-tree object already exposed by JSON and the UI.
  Evidence: `selected_checks` expands tools through gates before testing the epoch; work-start and work-goal formatters read only `commit_oid`.

- Observation: the workspace-wide native build identity assumes the source checkout layout from `CARGO_MANIFEST_DIR/../..`; a registry package contains only the `jig-sh` package inputs, so the build script panics before an installed binary can start.
  Evidence: `cargo package --list -p jig-sh` omits the workspace `crates/` tree while `build.rs::emit_build_identity` recursively opens it.

- Observation: prefix-only Cargo and SQLx command recognition accepts environment wrappers and path-bearing options, and per-app recognition checks only that an app directory exists plus the operation suffix. Those shapes let generated IDs execute repository inputs outside the rendered authority scope or another app's command.
  Evidence: `RUSTC_WRAPPER=tools/wrapper cargo test`, `CARGO=tools/cargo-wrapper sqlx prepare`, `cargo test --manifest-path ...`, and a mismatched `jig.typescript_<app>_<operation>` all pass the current recognizers.

- Observation: native-tool blanket trust makes `jig.schema_check` path-sensitive even though it runs the project-owned `schema_dump_command`; meanwhile the mutating `jig.schema_dump` utility is rendered as a required validation gate but deliberately rejected by the safety recognizer.
  Evidence: `policy.rs::schema_check` executes the dump command in a temporary copy; generated configuration renders both `schema` and `schema-dump` check gates.

- Observation: Git can represent a nested tracked repository with one gitlink and an untracked embedded repository as one directory. Parent diff text and the current directory fallback do not attest their mutable contents, and ambient `diff.ignoreSubmodules=all` can suppress classification entirely.
  Evidence: the fifth review reproduced equal fingerprints for different dirty submodule contents and the constant `dir` encoding for an untracked nested repository.

- Observation: frontend adoption expands arbitrary package-workspace declarations to discover apps but discards non-app members before rendering gate authorities. Changes under a shared `libs/*` or other declared workspace therefore bypass every frontend and application-contract scope.
  Evidence: `adopt_infer/frontend.rs::workspace_package_dirs` feeds only packages with the frontend script profile into `frontend_apps`; the renderer's shared authorities are fixed to `packages/**`.

- Observation: the same bounded plan-wide changed-path preview and digest are copied into every gate evidence entry in one batch. Receipt size therefore scales with gates multiplied by changed paths despite the data being identical for every gate.
  Evidence: `checks.rs::gate_evidence_from_scope` copies all four plan-wide changed-path fields for every `WorkCheckGateEvidence`.

- Observation: baseline-to-worktree classification can hide a staged index change when the worktree copy is restored to baseline. The same single diff is used for scoped proof, so a staged commit candidate can be both unclassified and unattested.
  Evidence: the sixth review reproduced an index-only change masked by baseline worktree content at `git_receipts.rs::changed_paths_since_baseline` and `gate_scope_input_fingerprint`.

- Observation: schema freshness reads ambient `SCHEMA_DOCS_DIR`, while generated policy hardcodes `docs/schema/**`; public-boundary gates omit `docs/public/**` and `public-docs/**`; and Rust tests omit the migration tree embedded by scaffolded `sqlx::migrate!` code.
  Evidence: `policy.rs::schema_check`, `.jig.toml.jinja`, and scaffold `contracts.mjs` disagree about their actual inputs and outputs.

- Observation: generated frontend ignores assume app and shared roots are disjoint. An ignored parent app can mask a nested app's own include or a nested shared workspace root because ignore precedence wins.
  Evidence: every per-app gate renders all other app trees in `paths_ignore` without testing path ancestry.

- Observation: gate applicability and scope fingerprints are collected before the work check's before-fingerprint. A concurrent change can therefore make recorded scope evidence describe a different tree than the command interval even when the outer fingerprints agree.
  Evidence: `runtime/work/checks.rs` loads and evaluates `PlanGateContext` before collecting `before_fingerprint`.

- Observation: checkout detection accepts any package located at `<workspace>/crates/jig` before it verifies Jig-specific workspace inputs. A packaged/path dependency in that layout is misclassified and fails while opening unrelated host `templates`/crate trees.
  Evidence: `build_identity.rs::checkout_workspace` tests only generic workspace files and the package's canonical location.

- Observation: schema freshness inherits Git status configuration and ordinary ignored-file behavior, so an ignored or ambient-hidden generated file can escape the stale-output check; `.git/**` is also syntactically accepted as an output directory even though Git cannot attest it.
  Evidence: the seventh review reproduced hidden output with `status.showUntrackedFiles=no` and `target/schema`, while `policy.rs::schema_check` used plain porcelain status and `context.rs::validate_schema_docs_dir` excluded only `.agent`.

- Observation: the SQLx gate is the only generated Rust-adjacent gate that omits the shared Cargo/toolchain command authorities, and readoption searches exact generated identities and legacy aliases in one order-sensitive pass.
  Evidence: `.jig.toml.jinja` omits `rust_gate_command_authority_paths` from `sqlx`; `runtime_config.rs::reconcile_work_gates` accepts the first alias-or-exact match.

- Observation: the native identity hashes source content but not the build target, enabled features, profile, compiler configuration, package version, or Jig's computed template-pin policy, even though those inputs can change native behavior with identical sources.
  Evidence: `build_identity.rs::compute` receives only the manifest directory and `build.rs` emits behavior-affecting environment values separately.

- Observation: application-contract execution installs one arbitrary configured app even though the root checker can own root dependencies and configured apps can occupy several independent dependency scopes.
  Evidence: `check-webapps.sh.jinja::run_application_contract_check` hardcodes `frontend_apps[0]`; the dependency resolver already distinguishes root workspace and standalone app scopes.

- Observation: plan changed-path discovery buffers complete Git name output and accumulates unbounded path vectors before receipt previews are truncated; scope revalidation repeats the same exposure.
  Evidence: `changed_paths_since_baseline` uses `git_output_with_cancellation` with `usize::MAX` capture and only bounds the later serialized preview.

## Decision Log

- Decision: bump the generated repository contract epoch from 4 to 5.
  Rationale: contract-compatible binary selection occurs before strict `.jig.toml` loading. A contract-v4 cache can predate and reject `paths`, `paths_ignore`, and `reuse`; a new epoch prevents selecting a runtime that cannot interpret the generated gate policy.
  Date/Author: 2026-08-26 / Codex.

- Decision: a new plan captures an exact commit OID as its baseline. The default ref is `HEAD`; `work start --base REF` resolves an explicit commit before appending state. The affected set is the difference from that commit through the current index and worktree, including untracked files.
  Rationale: this includes changes already dirty when work starts, survives commits made during the plan, and gives every classification an immutable reference. A legacy plan without a baseline may continue using unconditional gates but reports path-aware gates as unknown and cannot close until work is reopened under a baseline.
  Date/Author: 2026-08-26 / Codex.

- Decision: path patterns use documented repository-relative glob syntax with `*`, `?`, character classes, and `**`; ignores win per path. Empty path lists, absolute paths, parent traversal, NUL/newline input, and ignore-only applicability are invalid.
  Rationale: committed gate policy must be deterministic and repository-bounded. A dedicated glob matcher lets the same semantics classify tracked, deleted, renamed, and untracked paths.
  Date/Author: 2026-08-26 / Codex.

- Decision: absence of `paths` means always applicable. `paths_ignore` is only legal with a nonempty `paths` list. Required applicable gates run by default; optional gates and explicitly selected tools/gates run only on request.
  Rationale: this preserves existing repositories while making optional mean optional during both closure and default execution.
  Date/Author: 2026-08-26 / Codex.

- Decision: `not_applicable` is a successful gate outcome but never a successful tool execution. A native work-check batch records the baseline, rules, changed-path evidence, scope fingerprint, gate signature, and reason.
  Rationale: a skipped command must remain distinguishable from a command that ran and passed, while closure still needs durable, auditable evidence that the gate did not apply.
  Date/Author: 2026-08-26 / Codex.

- Decision: gate freshness uses a baseline-relative scope fingerprint containing the exact baseline OID, normalized rule/signature data, matching tracked diff bytes, and matching untracked path metadata and contents. The existing whole-worktree fingerprint remains in receipts for compatibility and broad diagnostics.
  Rationale: unrelated changes should not stale a gate, but changes entering, leaving, renaming within, or modifying its declared scope must. Including the baseline and tool signature prevents reuse across different base code or command definitions.
  Date/Author: 2026-08-26 / Codex.

- Decision: cross-plan reuse is opt-in with `reuse = true`; it may reuse only a successful check-gate evidence record with the same gate signature and scope fingerprint. The new plan records a fresh batch attestation referencing the source plan, batch receipt, and proving tool receipt. Reuse never applies to Codex review gates.
  Rationale: repository inputs alone do not prove that environment-sensitive or time-sensitive checks remain valid. Explicit policy makes deterministic gates reusable without silently weakening all repositories.
  Date/Author: 2026-08-26 / Codex.

- Decision: add `--gate` selection alongside legacy `--tool`. Explicit selection force-runs the named gate even when its path rules say not applicable; duplicate tools can therefore retain separate honest gate identities.
  Rationale: gate policy now contains behavior that a bare tool name cannot identify. Legacy callers remain supported.
  Date/Author: 2026-08-26 / Codex.

- Decision: path applicability controls whether an atomic tool runs; it never claims to narrow that tool internally. Generated frontend work policy will use per-app atomic commands and gates, while existing repository-wide TypeScript commands remain available as manual full-matrix checks.
  Rationale: a receipt named for one app must actually execute that app's check. Hiding selection inside the repository-wide tool would weaken evidence semantics.
  Date/Author: 2026-08-26 / Codex.

- Decision: keep implementation changes uncommitted through review.
  Rationale: the user explicitly requested a comprehensive review of all working changes. The ExecPlan skill normally encourages frequent commits, but preserving one inspectable working diff is the more specific requirement for this loop.
  Date/Author: 2026-08-26 / Codex.

- Decision: keep this source repository's existing Rust test gate unconditional for the current work plan while generated v5 repositories receive path-aware Rust gates.
  Rationale: this plan was opened by the pre-v5 binary and therefore has no baseline. Making the dogfood gate path-aware mid-plan would correctly fail closed as unknown and prevent the requested review loop from closing. Temporary v5 repositories prove the new behavior without weakening legacy-plan safety.
  Date/Author: 2026-08-26 / Codex.

- Decision: generated application-contract and public-artifact gates are opt-in repository capabilities, enabled automatically for the Rust/React scaffold that owns `scripts/contracts.mjs`, rather than universal no-op gates for every frontend repository. Persist an explicit enablement answer/config value, generate separately named command tools and required path-aware gates only when enabled, and make their commands perform the actual contract regeneration comparison and public build/boundary scan.
  Rationale: this restores the scaffold's retired checks without claiming that arbitrary adopted repositories have an application contract checker. Separate gate identities make the cross-layer and public-boundary evidence honest.
  Date/Author: 2026-08-26 / Codex.

- Decision: reuse sources must be direct `executed` v2 evidence from a stable exit-zero batch with a successful matching child receipt. Already-reused attestations are not chain sources; the scanner can find the original execution instead. Any existing current-plan evidence for the gate disables automatic reuse for that invocation.
  Rationale: this makes reuse no stronger than the source's normal freshness proof, removes transitive-chain ambiguity, and enforces the documented rule that rerunning a plan actually reruns its gate.
  Date/Author: 2026-08-26 / Codex.

- Decision: readoption preserves gate ownership by exact ID, plus a closed list of generated legacy aliases: `contract` to `jig-contract` for `jig.contract_check`, and `tests` to `rust-tests` for `jig.test`. Same-tool identity alone never consumes an existing gate.
  Rationale: tools are execution mechanisms and may intentionally back multiple project-owned scoped gates. Only known generated renames establish migration ownership.
  Date/Author: 2026-08-26 / Codex.

- Decision: contract versions before v5 retain their historical default of executing every configured check tool. Required-only applicability-aware default selection is a v5 behavior; explicit v5 selectors remain available without weakening legacy compatibility.
  Rationale: the runtime continues to advertise contracts 2–4 as supported, so upgrading the binary must not silently change their work-check matrix.
  Date/Author: 2026-08-26 / Codex.

- Decision: explicit force selection bypasses non-applicability, reuse, and scope-discovery failure for execution, but unknown scope evidence remains non-closable. Cache the resolved baseline plus changed/untracked path discovery once in `PlanGateContext`, while retaining per-gate scoped diff and untracked-content hashing.
  Rationale: force should run the requested command without fabricating freshness. Shared immutable discovery removes repeated Git work without conflating gate-specific fingerprints.
  Date/Author: 2026-08-26 / Codex.

- Decision: reject brace characters in gate globs and cache scoped input material by the exact normalized include/ignore policy. Stream each unique Git binary diff through a temporary file into the fingerprint instead of retaining it in memory; combine the cached input fingerprint with each gate signature afterward.
  Rationale: the documented matcher remains expressive while every accepted pattern has tested `globset`/Git parity. Policy-level caching removes duplicate Git work for the four same-scope frontend gates, and streamed hashing bounds memory without weakening binary-diff coverage.
  Date/Author: 2026-08-26 / Codex.

- Decision: define one renderer-owned frontend authority path set and reuse it in every per-app, application-contract, and public-artifact gate; reuse the Rust command-authority set in cross-layer gates. Add all package-manager configs, patches/plugins/releases, helper scripts, lock/manifests, Node authorities, Cargo config, and toolchain inputs consumed by those commands.
  Rationale: execution authorities should have one source of truth so a dependency or helper change selects every check whose behavior it can alter.
  Date/Author: 2026-08-26 / Codex.

- Decision: reject `paths`, `paths_ignore`, or `reuse` in contract versions 2-4 during repository loading. Preserve their all-gate execution behavior only for configuration that those epochs can satisfy.
  Rationale: silently accepting a policy whose evidence model is unavailable creates permanently missing gates; failing early gives an explicit migration boundary.
  Date/Author: 2026-08-26 / Codex.

- Decision: collect all selected reusable-gate candidates in one receipt scan, retaining successful child receipts only for requested tools. Add an optional child `exit_status` to v2 gate evidence and make scoped evaluation prefer it.
  Rationale: one scan scales with receipt history instead of history times gate count, while child status makes failed evidence internally consistent even when aggregate refinement deliberately succeeds.
  Date/Author: 2026-08-26 / Codex.

- Decision: migrate gate policy only when exact ID and tool both agree or a documented alias agrees. An exact generated-ID collision with another kind/tool is a hard readoption conflict. Align the MCP schema with runtime by rejecting only simultaneous nonempty selector arrays.
  Rationale: an ID alone cannot prove ownership, and schema-valid clients must be able to serialize absent selectors as empty arrays without changing runtime meaning.
  Date/Author: 2026-08-26 / Codex.

- Decision: represent an unborn repository with an explicit empty-tree plan baseline rather than rejecting structured work before the first commit. Persist a baseline kind plus the repository's hash-algorithm-specific empty-tree OID, recreate that harmless object during evaluation when necessary, and conservatively classify the initial repository contents as changed.
  Rationale: generated repositories should be verifiable immediately; an explicit tree baseline is honest, deterministic, and makes all source-bearing gates applicable without inventing a commit or weakening unknown-state handling.
  Date/Author: 2026-08-26 / Codex.

- Decision: use Globset exactly once to select changed repository paths, then give Git only literal selected paths for tracked fingerprinting. Make `.jig.toml` and `.agent/jig-contract.json` non-ignorable global execution authorities, and pin diff algorithm, rename, heuristic, text-conversion, prefix, color, and index-format options.
  Rationale: classification and freshness must cover the same paths; literal pathspecs remove directory/glob ambiguity, global authorities prevent command-definition edits from closing as N/A, and pinned output avoids environment-only staleness.
  Date/Author: 2026-08-26 / Codex.

- Decision: in refinement collection mode, unknown applicability records evidence and continues; the final structured result fails when any gate evidence is failed or unknown. Normal `work check` remains fail-closed.
  Rationale: refinement must preserve a complete result and run independent later gates without turning an inability to classify into success.
  Date/Author: 2026-08-26 / Codex.

- Decision: preserve field presence for all v5-only policy keys until contract-epoch validation. Generated gate path/include-ignore scopes are migration-owned on readoption and update to current defaults; project policy for `required` and `reuse` remains preserved. A repository that needs custom scoping must use a distinct project-owned gate identity.
  Rationale: explicit legacy spellings must fail predictably, and correctness fixes to generated execution authorities must reach existing adopters without confusing generated IDs with permanent customization hooks.
  Date/Author: 2026-08-26 / Codex.

- Decision: include full configured app trees in application-contract scopes, include Unix mode bits in untracked regular-file fingerprints, cache one legacy fingerprint per request/report, and batch plan-baseline loading for multi-plan status.
  Rationale: adopted checkers can own clients anywhere beneath configured apps; executable state is an input; and status cost should scale with repository state rather than plans multiplied by gates.
  Date/Author: 2026-08-26 / Codex.

- Decision: for Jig's generated tool identities, apply configured path policy only when Jig can prove the command is a canonical implementation whose dependencies are covered by the declared scope. Native checks, direct/generated Cargo commands, canonical SQLx prepare commands, and exact generated web-checker commands qualify; preserved custom implementations behind those generated identities evaluate conservatively as unconditional and hash the complete baseline-relative change set. A separately named project-owned tool retains its explicitly owned path policy.
  Rationale: arbitrary shell behind a generated identity is not statically analyzable, while a distinct project-owned tool and path contract are deliberate repository policy. This protects adopted custom wrappers without adding a same-epoch configuration field or weakening project ownership.
  Date/Author: 2026-08-26 / Codex.

- Decision: force `core.fileMode=true` for tracked classification and proof and explicitly fix unified/inter-hunk context, relative behavior, ordering, algorithm, rename behavior, prefixes, color, and text conversion. Give Git only tracked matching paths in deterministic bounded chunks; hash the ordered literals plus each chunk boundary and output, while untracked paths remain in metadata/content hashing.
  Rationale: mode suppression and ambient formatting violate evidence integrity, while bounded canonical transport avoids `ARG_MAX` without relying on unsupported `git diff --pathspec-from-file` behavior.
  Date/Author: 2026-08-26 / Codex.

- Decision: bind native gate signatures to a build-time SHA-256 identity over every workspace crate source plus workspace package/lock metadata. During multi-plan status, share prepared plan-change snapshots by baseline OID so their normalized-policy cache also shares each scoped proof.
  Rationale: native implementation updates must invalidate old evidence, and identical immutable baselines should not repeat identical Git discovery/proof work in one request.
  Date/Author: 2026-08-26 / Codex.

- Decision: contracts 2-4 retain direct, ordered, duplicate-preserving explicit `--tool` execution. Human work-start/work-goal output labels an unborn plan's `empty_tree_oid` explicitly.
  Rationale: v5 selector semantics must not change supported legacy invocations, and the baseline visible in structured output must be clear in the default interface.
  Date/Author: 2026-08-26 / Codex.

- Decision: compute native identity from the complete workspace source set only when the manifest is demonstrably the checkout's `crates/jig` package. In a packaged build, hash the complete package-local build/runtime source set instead. Both layouts use the same deterministic content-and-relative-path algorithm and fail rather than emitting a placeholder.
  Rationale: a registry install must build without workspace siblings, while every source byte that can change the packaged native behavior must still invalidate old native evidence.
  Date/Author: 2026-08-27 / Codex.

- Decision: generated Cargo and SQLx identities receive path-sensitive treatment only for a closed, documented token grammar. Reject environment assignments except the exact generated SQLx values, reject wrapper/path-selection options and unfamiliar shapes, and require per-app tools to equal the gate key derived from the command's configured app plus its exact operation. Anything unproven becomes unconditional.
  Rationale: conservative applicability is preferable to a false proof; command prefixes and operation suffixes cannot establish execution authority.
  Date/Author: 2026-08-27 / Codex.

- Decision: treat `jig.schema_dump` as an explicit mutating utility, not a work-validation gate. Retire the generated `schema-dump` gate during readoption. Keep only `jig.schema_check` as a required gate, and permit its generated scope only when the configured dump command is a strict Cargo-run shape covered by Rust, Cargo/toolchain, migration, metadata, and SQL authorities; arbitrary dump wrappers make it unconditional.
  Rationale: a work gate must validate rather than mutate the repository, and native dispatch alone cannot prove the inputs of a nested project-owned command.
  Date/Author: 2026-08-27 / Codex.

- Decision: pin submodule handling on every classification and fingerprint command. For any selected tracked gitlink, accept only an uninitialized checkout or an initialized checkout whose HEAD equals the index object and whose recursively reported status is clean; otherwise fail applicability/freshness as unknown. Reject an untracked directory fingerprint because it can be an embedded repository whose contents Git intentionally collapses.
  Rationale: a bounded fail-closed boundary avoids pretending a one-line gitlink or directory marker attests arbitrary nested mutable state.
  Date/Author: 2026-08-27 / Codex.

- Decision: persist normalized, expanded non-app JavaScript workspace member directories as `frontend_workspace_roots` in render answers/configuration. Inferred exclusions are applied before persistence. Add those roots to every generated frontend/application/public authority scope, excluding configured app roots that already have app-specific include/ignore policy.
  Rationale: adoption must carry the ownership it discovered into durable generated policy, including shared packages outside `packages/**` and packages without development scripts.
  Date/Author: 2026-08-27 / Codex.

- Decision: store plan-wide changed-path preview/count/truncation/digest once on `WorkCheckBatchEvidence`. New per-gate records retain only matching-path and gate-policy evidence; readers hydrate the legacy in-memory view from the batch fields and continue accepting older receipts that embedded the data per gate.
  Rationale: this changes append cost from gates times changed paths to gates plus changed paths without losing compatibility or observable gate status.
  Date/Author: 2026-08-27 / Codex.

- Decision: classify the union of baseline-to-index and baseline-to-worktree changes and hash both canonical diff domains. For an in-scope path changed in the index, reject a differing worktree copy as unknown; allow ordinary unstaged-only and staged-only edits, but do not claim one command attests two partially staged versions.
  Rationale: this closes the masked-index bypass without requiring users to stage ordinary work before checking.
  Date/Author: 2026-08-27 / Codex.

- Decision: replace ambient `SCHEMA_DOCS_DIR` with a persisted, validated `schema_docs_dir` answer/config field defaulting to `docs/schema`. Render it into schema applicability, use literal Git pathspec semantics at runtime, and include all conventional public-doc trees plus configured migrations in every generated gate that consumes them.
  Rationale: runtime inputs, gate signatures, and affected-path policy must have one committed source of truth.
  Date/Author: 2026-08-27 / Codex.

- Decision: compute each frontend app's ignores in the renderer and omit any other-app ignore whose tree intersects the current app or any shared workspace root. Keep ignores for truly disjoint apps.
  Rationale: conservative over-selection for overlapping ownership is safe; ignore precedence that suppresses an included authority is not.
  Date/Author: 2026-08-27 / Codex.

- Decision: begin the work-check stability interval before plan/scope discovery, always close it after evidence collection, and independently reload/re-evaluate every selected gate before recording. Any changed classification, digest, signature, scope fingerprint, or error invalidates the batch receipt.
  Rationale: outer whole-tree snapshots alone cannot bind scope evidence collected outside their interval or detect an applicability snapshot that changed and returned.
  Date/Author: 2026-08-27 / Codex.

- Decision: recognize a source checkout only when the candidate root also contains Jig-specific workspace sentinels required by the source identity. Otherwise use package-local inputs even if the dependency happens to live at `crates/jig` in another workspace.
  Rationale: filesystem position is not sufficient provenance; missing source-only sentinels must select the packaged mode rather than fail the build.
  Date/Author: 2026-08-27 / Codex.

- Decision: reject `.git` schema destinations, preflight the configured destination against Git ignore rules, and query status with explicit all-untracked plus ignored reporting after the dump. An ignored destination or any tracked, untracked, or ignored output is a failing check result, independent of ambient Git status configuration.
  Rationale: schema freshness is meaningful only when every generated artifact is committable and explicitly visible to the attestation command.
  Date/Author: 2026-08-27 / Codex.

- Decision: add the renderer-owned Rust command-authority paths to SQLx scopes, and make generated-gate readoption select exact ID+tool identity before considering a documented alias. When both exist, the exact generated gate owns preserved policy and the retired alias is discarded.
  Rationale: SQLx execution changes with Cargo/toolchain policy, and migration aliases must never override an already-current generated identity due to file order.
  Date/Author: 2026-08-27 / Codex.

- Decision: version native build identity again and hash a sorted build-configuration map containing target/host, Cargo cfgs and enabled features, profile/codegen settings, compiler/wrapper/encoded flags, package metadata, and the final template-pin policy alongside source inputs. Keep a pure configuration-taking identity function for deterministic tests.
  Rationale: native evidence must be invalidated by compilation inputs that can alter behavior even when every source byte is unchanged.
  Date/Author: 2026-08-27 / Codex.

- Decision: prepare the root dependency scope when it owns `package.json`, then every distinct dependency scope represented by configured frontend apps before invoking the root application-contract checker. Reuse the same preparation for public-artifact checking and deduplicate shared workspace scopes.
  Rationale: the checker and generated clients can span root, shared, and independent package-manager projects; selecting the first app is neither ownership-aware nor complete.
  Date/Author: 2026-08-27 / Codex.

- Decision: capture each changed-path Git command with a fixed byte ceiling, enforce one aggregate discovered-path ceiling across worktree, index, manifest, and untracked domains, and fail scope classification closed with an actionable diagnostic on either overflow.
  Rationale: bounded receipt previews do not protect discovery memory; applicability must remain safe on generated or adversarially large worktrees without silently dropping paths.
  Date/Author: 2026-08-27 / Codex.

- Decision: route every known-root receipt and schema Git command through the existing deny-by-default repository-environment scrubber, then set `GIT_OPTIONAL_LOCKS=0` for observational probes. Keep only the scrubber's explicit read-only/authentication allowlist.
  Rationale: evidence must describe the repository passed by Jig, never a repository, index, object store, or ref graph selected by ambient process variables.
  Date/Author: 2026-08-27 / Codex.

- Decision: bound whole-worktree status and both binary-diff captures with supervised process-tree output limits, cap parsed status entries, hash the three proof fields incrementally with an explicit versioned domain, and retain the already-bounded untracked encoding as one final field. Any byte, entry, diagnostic, or completeness overflow fails evidence collection closed.
  Rationale: receipt preflight must have finite memory and proof cardinality without silently truncating evidence.
  Date/Author: 2026-08-27 / Codex.

- Decision: seed generated dependency-scope arrays with an impossible repository-relative sentinel and add an executable no-root-manifest regression. Reject schema root output and any case-insensitive `.git` or `.agent` path component. Reject every symlink encountered in required native build-identity inputs rather than guessing whether its target is complete or root-confined.
  Rationale: these are portable fail-closed contracts across Bash 3.2, case-insensitive filesystems, and package/checkout source layouts.
  Date/Author: 2026-08-27 / Codex.

- Decision: redact the exact repository-root byte spelling from stdout/stderr previews before appending receipts, using a stable `<repository-root>` marker, then perform the explicitly authorized historical privacy migration over every affected active receipt record and plan. Preserve record IDs and every unaffected field byte-for-byte, and append a durable decision naming all affected receipt IDs without reproducing the removed workstation path.
  Rationale: a one-time rewrite without prevention would leak again on the next Cargo diagnostic; prevention without migration would leave already-committed private state behind.
  Date/Author: 2026-08-27 / Codex.

- Decision: make the empty-selection diagnostic describe both configured gates and execution tools without v5-only required/optional semantics.
  Rationale: one truthful compatibility-neutral message avoids branching presentation on an implementation epoch while remaining actionable for every supported contract.
  Date/Author: 2026-08-27 / Codex.

- Decision: give whole-worktree legacy diffs the same canonical Git semantics as scoped diffs, including explicit external-diff/textconv disablement and `core.fileMode=true`, then advance the fingerprint domain. Replace scoped temporary-file output and submodule dirtiness collection with the supervised bounded-output runner; a byte overflow or incomplete capture is unknown evidence, while the first submodule status byte is sufficient to prove dirtiness.
  Rationale: repository selection, command output bounds, and diff semantics are separate proof obligations; all three must be pinned before a fingerprint can be an equality token.
  Date/Author: 2026-08-27 / Codex.

- Decision: every v2 work-check batch records the selected gate-ID set and superseding evidence for every selected gate. Completed failures use `failed`; cancellation, launch failure, or a prior gate abort uses `unknown` with an explicit batch reason. If the observer is already cancelled, append the small batch record through a non-cancellable cleanup path with expensive metadata disabled.
  Rationale: an attempted rerun must never leave an older pass authoritative merely because execution stopped before the child receipt existed.
  Date/Author: 2026-08-27 / Codex.

- Decision: fail every baseline reader closed on more than one `Open` event for a plan ID through one shared resolver. Move repository-root redaction into a shared state privacy module, redact only path-bounded matches, and apply it to persisted session source paths as well as receipts.
  Rationale: append-only corruption must not produce caller-dependent evidence, and state writers must share one privacy contract rather than evolving independently.
  Date/Author: 2026-08-27 / Codex.

- Decision: perform a second explicit privacy migration over every remaining durable workstation path in receipts, sessions, and plan bodies, replacing only matched private path tokens and preserving record IDs and all other bytes. Replace the one historical plan body containing intertwined downstream operational detail with a stable privacy-redacted placeholder while retaining its plan identity. Append a decision naming all affected receipt, session, and plan IDs without reproducing removed text.
  Rationale: the first migration covered only one root spelling; open-source hygiene requires complete active-state repair plus prevention on every future state writer.
  Date/Author: 2026-08-27 / Codex.

- Decision: derive prior per-app generated command keys and exact default values from the existing `[[frontend_apps]]` list during readoption, and retire them only when absent from the newly required command set and unchanged from that exact generated value. Acquire the shared environment lock in the build-configuration environment test.
  Rationale: generated ownership must survive app removal without consuming project overrides, and process-global environment tests must obey the same serialization boundary as environment mutators.
  Date/Author: 2026-08-27 / Codex.

## Outcomes & Retrospective

Eighteen implementation/review loops established contract-v5 baselines, applicability, explicit not-applicable evidence, scoped freshness, latest-outcome-safe exact-input reuse, per-app execution, honest `jig-contract` naming, matcher/proof alignment, bounded byte-safe Git evidence, shared checkout/package build-source identity, archive-safe supersession, ownership-safe readoption and checker inference, unambiguous fingerprint/signature framing, bounded byte-preserving Gitlink selection, literal generated policy roots, staged-policy preview parity, generic open-source fixtures, hermetic PTY integration tests, contextual delimiter-safe privacy redaction, complete evidence summaries, refresh-stable native identity, staged-deletion replacement rejection, OS-native temporary proof paths, and status-faithful unknown evidence. Loop 18 passed the complete plan-linked matrix with fresh evidence in batch `receipt_01M11ADBTCV72VP7FK35N7HZGS`; its 2,275 primary tests and all configured isolated shards are clean, as are strict Clippy, formatting, contract, template parity, JSONL, diff, and privacy checks. A fresh full-tree merged Claude Code plus independent Codex review returned `No actionable findings`, no open questions, and no material test gaps, so the work is ready for structured plan closure.

## Context and Orientation

The repository root is `.`. `crates/jig` owns the `jig` CLI, repository configuration, state streams, command execution, and work-gate evaluation. Its crate guide is `crates/jig/AGENTS.md`. The source repository dogfoods the generated harness, so runtime changes must be built with `cargo build -p jig-sh --bin jig` and exercised with `JIG_DEV_BIN=target/debug/jig scripts/jig ...`.

`.jig.toml` is committed repository policy. `crates/jig/src/context/work_config.rs` deserializes `[[work.gates]]` with unknown fields denied and resolves check and Codex-review gate types. `templates/project/.jig.toml.jinja` generates the default policy. `.agent/jig-contract.json` is a generated, versioned command/tool manifest; `templates/project/.agent/jig-contract.json.jinja` is its template. Before this implementation, contract version 4 was selected by `crates/jig/src/context.rs::CURRENT_CONTRACT_VERSION` and launcher templates; this plan advances those current surfaces to version 5.

A structured plan is durable append-only state. `crates/jig/src/state/records.rs::PlanEvent` owns its compatibility serializer, and `state/plans.rs` creates, reads, and summarizes plans. A “baseline” in this plan means the exact Git commit whose files are treated as the unchanged starting point. The current index, tracked working files, deletions, renames, and untracked files are compared to that commit to determine the plan's affected paths.

`crates/jig/src/runtime/work/checks.rs` executes no-argument tools and appends one child receipt per tool plus one `jig.work_check` batch receipt. `runtime/work/gates.rs` evaluates configured gates. `state/receipts.rs` scans append-only receipt JSONL and currently indexes check receipts by tool and plan. A “gate signature” means a stable hash of the gate ID, tool identity, actual configured command/native implementation identity, normalized applicability policy, required/reuse flags, and contract epoch. A “scope fingerprint” means a stable hash of relevant repository inputs relative to the stored baseline. These hashes are equality tokens, not public cryptographic APIs.

`crates/jig/src/git_receipts.rs` owns Git subprocesses and worktree fingerprints. Add baseline resolution, baseline-relative changed-path collection, and scope fingerprinting there or in a focused sibling module such as `git_gate_scope.rs`; keep cancellation-aware and blocking entrypoints aligned. `.agent/**` must remain excluded. Renames must classify both old and new names. Any inability to resolve the baseline, inspect Git, parse output, match a glob, or hash relevant untracked content is `unknown`, never `not_applicable`.

The generated frontend checker `templates/project/scripts/check-webapps.sh.jinja` already has an internal `run_check` function that prepares dependencies and applies coverage enforcement. Add a public, validated per-app operation generated from the configured frontend list. Generate per-app command keys, manifest tools, and path-aware gates so app-specific receipts correspond to app-specific execution. Keep the existing all-app lint, typecheck, build, and coverage commands and CLI checks for deliberate full-matrix use.

## Plan of Work

Milestone 1 establishes versioned configuration and durable baseline semantics. Add `globset` as a pinned workspace dependency and to `crates/jig`. Extend check gate configuration with `paths: Vec<String>`, `paths_ignore: Vec<String>`, and `reuse: bool`, preserving current defaults. Validate patterns and check-only field placement. Extend `WorkCheckGate` with normalized policy. Bump the contract epoch and update templates, snapshots, launcher checks, fixtures, public-contract statements, and changelog. Extend start/goal command DTOs, CLI flags, MCP schemas, plan request types, and the append-only plan open serializer with optional baseline metadata. Old plan records must deserialize unchanged. Explicit invalid refs must fail before the plan body or JSONL event is created; implicit HEAD collection failure may be persisted as a baseline error so diagnostic state remains visible and path gates fail closed.

Milestone 2 builds one authoritative classifier and fingerprint implementation. In the Git receipt boundary, resolve refs to full commit OIDs; collect baseline-to-worktree changed paths including staged, unstaged, committed-since-baseline, deleted, renamed, and untracked paths; compile validated glob sets; and produce a bounded changed-path preview plus full digest. For each gate, return `applicable`, `not_applicable`, or `unknown`, an explanation, and a scope fingerprint. Hash the baseline OID, gate/tool signature, matching binary diff, and matching untracked path metadata and contents. Add unit tests using temporary Git repositories for clean plans, preexisting dirty changes, post-start commits, deletes, renames across scope boundaries, ignored paths, unrelated edits, untracked content, invalid/missing baselines, and cancellation.

Milestone 3 makes execution and evidence applicability-aware. Replace tool-only default selection with configured required check-gate selection. Add `--gate` and matching MCP arguments; keep `--tool` as a force-run compatibility path. Classify every selected/default gate before spawning commands. Run applicable gates, skip and record not-applicable gates, and report unknown applicability as a failed batch without spawning that gate. Optional gates do not run by default. Preserve the existing invariant that checks must not mutate non-`.agent/` files. Store a versioned check-gate evidence array in the batch receipt. Include gate IDs in returned `checks` and include a separate `not_applicable` list so an all-not-applicable run is a successful classified run, not “no checks configured.”

Milestone 4 updates closure, scoped freshness, and reuse. Extend the receipt scan index with gate-ID check evidence while retaining legacy tool/batch fallback for gates without applicability fields. `work gates`, `work evidence`, `work finish`, UI snapshots, and human output must recognize `not_applicable`, `reused`, `unknown`, and scoped freshness reasons. A required not-applicable gate satisfies closure only when its classification evidence is fresh. An applicable receipt stays fresh after an unrelated edit and becomes stale after an in-scope edit or a rule/tool signature change. For `reuse = true`, scan successful prior plan evidence and reuse only an exact signature/fingerprint match, then append current-plan attestation evidence; default false reruns. Tests must prove same-plan freshness, cross-plan opt-in reuse, default no-reuse, source failure rejection, changed command/rules/baseline rejection, stale reuse chains, and append-only compatibility.

Milestone 5 makes generated policy proportional and names honest. Rename the generated wiring gate ID from `contract` to `jig-contract` while retaining the public tool name `jig.contract_check` and CLI `check contract` for compatibility. Adoption reconciliation must migrate the old generated ID without duplicating it and preserve project-owned required/path/reuse settings when the same generated tool survives. Generate Rust gate paths from configured Rust crate roots plus root Cargo/toolchain inputs. Generate SQLx paths from Rust roots, migration and metadata locations, and database configuration inputs, leaving project owners free to narrow query ownership. Generate per-app TypeScript command keys/tools/gates with app-directory paths and shared package-manager/tooling inputs; every per-app command must invoke only that app. Foundation or shared frontend files should make every app gate applicable. Update generated AGENTS guidance to explain focused work checks and deliberate full-matrix commands.

Milestone 6 updates documentation and observable output. Document exact glob, ignore, baseline, force-run, optional, reuse, freshness, and migration semantics in `docs/configuration.md`, `docs/public-contract.md`, `docs/developer-ux.md`, `docs/adoption.md`, README/landing material where the old blanket-gate claim appears, templates, help examples, and `CHANGELOG.md`. Rename human presentation to “Jig contract” or “harness contract” wherever bare “contract” could imply an application API contract. JSON must expose baseline metadata, applicability reason, matched changed paths/digest, gate signature, scope fingerprint, source receipt for reuse, and freshness diagnostics without leaking command bodies.

Milestone 7 proves behavior and closes review findings. Run focused crate tests while implementing. Format, build, and run strict Clippy. Build the development binary again, then use it for contract checks and plan-linked gates. Exercise a temporary generated Rust-plus-multi-frontend-plus-SQLx repository to show that one app edit runs only that app's four checks plus the Jig contract, a Rust edit skips frontend gates, a migration edit selects SQLx and Rust gates, and an unrelated later edit does not stale previously scoped evidence. Finally delegate a comprehensive review of the complete working diff to a subagent. Any actionable review finding returns the plan to the relevant milestone, followed by full revalidation and another delegated review.

Milestone 8 is the second-loop remediation required by the first comprehensive review. Add scaffold-owned, explicitly named `application-contracts` and `public-artifacts` command gates behind a persisted capability enabled by the Rust/React scaffold and inferred only when the owned checker exists. The first command runs staged OpenAPI/client comparison; the second builds public frontends and scans manifests, dependency graphs, and public artifacts. Give them path policies covering their Rust API inputs, OpenAPI/client outputs, scripts, manifests, and public frontend inputs. Update generated contract tools, preview, readoption, snapshots, docs, and scaffold tests.

In the same milestone, harden reuse to direct stable executions only and disable it after any current-plan evidence; restrict gate migration to exact IDs and explicit generated aliases; preserve all-gate defaults for contract versions 2–4; include rustfmt, Clippy, and Nextest configuration in generated Rust scopes; execute forced gates even when applicability is unknown while keeping their closure status unknown; and cache plan-level baseline/change discovery. Add focused regression tests for mutation-invalid reuse, follow-up-plan reruns, v4 optional defaults, v4 custom same-tool adoption, forced unknown scope, Rust config classification, application/public scaffold gates, discovery call count, and non-UTF-8 fail-closed behavior.

Milestone 9 is the third-loop remediation required by the second comprehensive review. Make accepted glob matching and Git fingerprint selection semantically safe, cache and stream unique scoped input proofs, centralize complete generated Rust/frontend command authorities, reject unsatisfiable v5 policy under legacy contract epochs, batch cross-plan reuse discovery, persist child failure exit statuses, reject generated-ID/tool adoption collisions, and align MCP selector schemas with empty-array runtime behavior. Add focused semantic-parity, authority-mutation, legacy-load, cache-call-count, large-diff, multi-reuse, refinement-exit, adoption-collision, and JSON Schema validation tests.

Milestone 10 is the fourth-loop remediation required by the third comprehensive review. Add explicit unborn empty-tree baselines; eliminate the second glob matcher from tracked fingerprints by hashing only literal paths selected by Globset; make runtime contract/config files universal authorities; include complete adopted app trees in contract checking; make refinement collect unknown gates; preserve legacy field presence; update generated scopes during readoption; hash Unix mode bits; cache legacy fingerprints and batch plan baselines; and pin Git diff output. Add focused tests for every reported reproducer and performance count.

Milestone 11 is the fifth-loop remediation required by the fourth comprehensive review. Make custom command-backed gates conservatively unconditional unless their command matches a proven canonical Cargo, SQLx, or generated web-checker shape; complete Git canonicalization for file modes and context; chunk literal tracked path proofs below process argument limits; bind native signatures to a build source identity; share plan-change and policy-proof snapshots across same-baseline plans; restore legacy explicit-tool order and duplication; and show empty-tree baselines in human start/goal summaries. Add every requested wrapper, ambient-config, tracked-mode, native-identity, many-path, cache-count, legacy-selector, and output regression.

Milestone 12 is the sixth-loop remediation required by the fifth comprehensive review. Support both workspace-checkout and registry-package native build identities; replace prefix command recognition with strict generated Cargo/SQLx grammars and exact app/tool binding; make schema dump a non-gate mutating utility and make schema-check scoping depend on its nested command; fail closed for dirty gitlinks and untracked embedded repositories while pinning submodule settings; persist adopted non-app frontend workspace roots into all relevant generated authorities; and move duplicated plan-wide change metadata to the batch evidence object with legacy receipt hydration. Add packaged-layout, wrapper/path-option, cross-app, schema migration, submodule/embedded-repo, arbitrary workspace/exclusion, and many-gate receipt-size regressions.

Milestone 13 is the seventh-loop remediation required by the sixth comprehensive review. Union baseline-to-index and baseline-to-worktree classification/proof and reject in-scope partially staged versions; replace ambient schema output ownership with validated committed configuration and literal Git paths; include public-doc and migration inputs in generated gates; make frontend ignores ancestry-aware; bracket and revalidate scope evidence inside the stable-worktree interval; and require Jig-specific sentinels before selecting checkout build identity. Add masked-index, custom-schema-dir, public-doc, migration-only, nested frontend/shared-root, scope-race, and unrelated-workspace package-layout regressions.

Milestone 14 is the eighth-loop remediation required by the seventh comprehensive review. Make schema freshness independent of ambient status settings and ignored destinations; add Cargo/toolchain authorities to SQLx; give exact generated gate identities precedence over aliases; bind native identity to target, features, profile, compiler configuration, package metadata, and template-pin policy; prepare every distinct frontend dependency scope used by the root contract checker; and put hard byte/path ceilings on changed-path discovery. Add ignored/hidden schema, SQLx-authority, exact-plus-alias, build-configuration, independent-dependency-scope, and discovery-overflow regressions.

Milestone 15 is the ninth-loop remediation required by the eighth comprehensive review. Scrub ambient Git repository redirection from receipt and schema probes; bound and incrementally hash whole-worktree proofs; make generated dependency preparation safe under Bash 3.2 without a root package; reject root/reserved schema aliases; reject symlinked native build inputs; prevent repository-root persistence in receipt previews and complete the explicit historical privacy migration; and make empty-selection guidance accurate across supported contract epochs. Add ambient `GIT_DIR`/`GIT_WORK_TREE`/`GIT_INDEX_FILE`, large-binary/many-status, executable independent-app shell, reserved-alias, package/checkout symlink, receipt-redaction, migration-integrity, and legacy-guidance regressions.

Milestone 16 is the tenth-loop remediation required by the ninth comprehensive review. Canonicalize every whole-worktree diff with pinned file-mode, external-diff, textconv, algorithm, context, rename, prefix, relative, and submodule semantics, and advance the legacy fingerprint domain. Replace unbounded scoped-diff temporary output and submodule status capture with supervised byte-limited execution that terminates on overflow or first proof of dirtiness. Make every new work-check batch name all selected gate IDs and append superseding failed, cancelled, or unknown evidence for gates that do not complete, including a minimal cancellation-safe batch append. Resolve plan baselines through one duplicate-detecting helper. Move repository-root privacy redaction to a shared state boundary, require a following separator or end-of-string boundary, redact persisted session source paths, and perform the authorized ID-preserving migration over all remaining workstation-path records and affected plan bodies; fully replace the one historical plan body whose downstream operational material is inseparable from its purpose. Derive retired per-app generated commands from the prior configured app set and remove only exact generated values. Serialize the environment-sensitive build-identity test. Add adversarial Git-config, cancelled/launch-error supersession, oversized committed-diff, dirty-submodule ceiling, duplicate-open, prefix-overlap, session-redaction, migration-integrity, removed-app readoption, and environment-lock regressions.

Milestone 17 is the twelfth-loop remediation required by the post-PTY comprehensive review. Preserve every selected-gate supersession record as an archive-protection tombstone before parsing its optional evidence so compaction cannot reveal an older pass. Generate Cargo `rerun-if-env-changed` directives from the same fixed and observed environment inputs hashed into native identity, excluding only derived fields. Complete the privacy migration by replacing the remaining downstream historical plan body and provider fixture, append a decision amendment naming affected records without removed values, and audit the repository for those identifiers. Require an explicit left token boundary as well as the existing right path boundary for repository-root redaction. Add a changelog and adoption/configuration migration note that the former ambient schema output variable is no longer read and its value must move into committed `schema_docs_dir`. Add archive-supersession, rerun-key parity, embedded-prefix, and generic-fixture regressions.

Milestone 18 is the thirteenth-loop remediation required by the next merged review. Treat whitespace, quotes, NUL, assignment/separator punctuation, and closing delimiters as valid right token boundaries while preserving sibling-prefix protection. Count and render failed/cancelled gate evidence and let those statuses determine the human batch verdict even when no child check exists. Refresh embedded template snapshots before computing native identity through one tested sequencing helper, and prove two identical refresh passes produce the same identity. Add delimiter, formatter-status, and refresh fixed-point regressions plus changelog coverage.

Milestone 19 is the fourteenth-loop remediation required by the next merged review. Resolve checkout versus package layout once and pass the same result to embedded-template selection and native identity hashing; package/path builds must use snapshots even below unrelated ambient template trees. Infer adopted application-contract gates only when a bounded regular `scripts/contracts.mjs` declares the exact v1 `check,public-check` interface marker, while preserving explicit owner opt-in. Parse repository-root token boundaries with Unicode-aware context: paired delimiters may close a wrapped root, punctuation runs must terminate at whitespace/NUL/end, and legal punctuation-prefixed sibling paths must stay untouched. Add ambient-host-template, unrelated/partial-checker, punctuation-sibling, sentence-period, Unicode-space, documentation, and changelog regressions.

Milestone 20 is the fifteenth-loop remediation required by the next merged review. While scanning reusable evidence, update the latest exact gate/tool/signature/scope outcome for successful and unsuccessful batches alike, and reuse only when that latest outcome is a clean direct execution backed by a successful child receipt. For legacy whole-worktree fingerprints, replace UTF-8 conversion of changed tracked paths with one bounded raw `ls-files --stage -z` index probe; parse ASCII mode/OID/stage metadata separately, preserve path bytes, and attest only changed Gitlinks. TOML-escape every dynamic generated gate include/ignore value, including Rust roots, migration/SQLx/schema directories, frontend workspace/app roots, shared authorities, and ignore paths. Add pass-then-fail reuse, Unix non-UTF-8 tracked-file fingerprint, and quoted-path renderer regressions, refresh embedded snapshots, and document the fixes in the changelog.

Milestone 21 is the sixteenth-loop remediation required by the next merged review. Length-frame each untracked path and encoded payload independently, advance the legacy whole-worktree fingerprint domain, and add the reproduced two-file NUL framing collision. Model reusable scan state explicitly: clean direct evidence replaces a candidate, exact failed/unknown evidence and malformed selected batches tombstone it, and reused attestations remain inert so later plans still reference the original direct proof; keep other signatures/scopes independent. Replace the full-index Gitlink probe with bounded `OsString` literal pathspec chunks over only changed tracked paths, retaining non-UTF-8 compatibility and conflict/submodule checks. Normalize every generated authority root, reject control or glob-metacharacter directory names that cannot be represented literally, and revalidate the resulting pattern with Jig's gate grammar while preserving quoted TOML paths. Frame gate signature path lists with domain tags and element counts and advance its domain. Derive bootstrap `generated_gates` from the staged work-gate set, excluding bootstrap, agent-guide, and repository-wide TypeScript utilities. Add framing-collision, multi-hop/malformed/different-scope reuse, low-limit large-index, invalid-root, signature-collision, and preview-parity regressions plus changelog coverage.

Milestone 22 is the seventeenth-loop remediation exposed by complete verification. Preserve the established empty-string-as-omitted semantics for the optional Rust migration directory before generated-root normalization, while continuing to reject empty or unsafe concrete values for every other generated policy root. Prove CLI and answers-file omission in both defaults and strict noninteractive modes, plus adjacent unsafe-root and scaffold validation behavior.

Milestone 23 is the eighteenth-loop remediation required by the next merged review. Before scoped evidence filters ordinary untracked files, detect every selected baseline-to-index deletion whose repository path still resolves to a filesystem leaf and fail closed, because Git can otherwise hide an ignored same-path replacement from both untracked discovery and index-to-worktree diffing. Apply the equivalent staged-deletion invariant to legacy whole-worktree fingerprints. Carry canonical binary-diff arguments, including the temporary order-file path, as OS-native values through the bounded Git runner so a valid non-UTF-8 `TMPDIR` cannot be lossy. Report no child exit status for `unknown` gate evidence; retain real child statuses for executed, failed, and cancelled evidence and synthetic zero only for reused and not-applicable evidence. Add ignored-replacement collision, non-UTF-8 temporary-directory, and refinement JSON regressions.

## Concrete Steps

All commands run from `.`.

During implementation, repeatedly run focused tests such as:

    cargo test -p jig-sh context::tests
    cargo test -p jig-sh runtime::tests::work
    cargo test -p jig-sh git_receipts
    cargo test -p jig-sh bootstrap::tests::basic::adoption_modes
    cargo test -p jig-sh --lib cli::output::tests

After runtime edits, rebuild and force dogfooding through the new binary:

    cargo build -p jig-sh --bin jig
    JIG_DEV_BIN=target/debug/jig scripts/jig check contract --no-receipt
    JIG_DEV_BIN=target/debug/jig scripts/jig work gates --plan-id plan_01M0ZDM8RAM92J6PZ7Y6TYVVW2

Before review, run repository gates in this order and attach receipts to the open plan where supported:

    scripts/jig check fmt
    cargo test -p jig-sh
    scripts/jig check clippy
    JIG_DEV_BIN=target/debug/jig scripts/jig check contract
    JIG_DEV_BIN=target/debug/jig scripts/jig work check --plan-id plan_01M0ZDM8RAM92J6PZ7Y6TYVVW2
    JIG_DEV_BIN=target/debug/jig scripts/jig work evidence --plan-id plan_01M0ZDM8RAM92J6PZ7Y6TYVVW2

The final repository requirement is:

    JIG_DEV_BIN=target/debug/jig scripts/jig check test

If the full test gate is too long for one terminal yield, keep its process session active, report progress at least once per minute, and poll it until completion.

## Validation and Acceptance

Configuration acceptance requires valid `paths`, `paths_ignore`, and `reuse` fields to load under contract v5, malformed or unsafe patterns to fail with the gate ID and field named, and the same config to be rejected by a pre-v5 runtime through contract incompatibility rather than accidental partial interpretation.

Baseline acceptance requires a new plan's JSON and append-only open event to contain an exact baseline commit. `--base REF` must resolve and store the OID. Existing dirty files at plan start and commits made afterward must remain in the affected set. Legacy plan records must still load, and path-aware gates on a legacy plan must report an actionable unknown-baseline error rather than pass or skip.

Applicability acceptance requires default `work check` to run only required applicable check gates. Optional gates must remain visible and explicitly runnable. A not-applicable required gate must show `status: not_applicable`, the rule and changed-path reason, no child tool receipt, and fresh batch classification evidence. Explicit `--gate` or `--tool` must force execution.

Freshness acceptance requires an unrelated out-of-scope edit not to stale a passing or not-applicable gate, while an in-scope edit, rename, deletion, untracked-content change, baseline change, command change, or rule change does. Unknown Git/fingerprint state must block required closure.

Reuse acceptance requires default gates to rerun across plans. A gate with `reuse = true` may skip execution only when a prior successful evidence record has the identical gate signature and scope fingerprint; the new plan must contain a receipt referencing the exact source. Failed, stale, differently configured, differently based, or incomplete evidence must never be reused.

Granularity acceptance requires a generated repository with at least two frontend apps to expose distinct app-specific gate IDs and command-backed tools. Editing one app must not execute the other app's commands. Editing a declared shared frontend input must select both. The existing all-app CLI checks must still work for explicit full-matrix validation.

Naming acceptance requires newly generated/readopted policy and human gate output to say `jig-contract`; documentation must state that it validates Jig manifest/runtime wiring and not an application's OpenAPI or frontend/backend API compatibility.

Completion acceptance requires formatting, focused tests, strict Clippy, contract validation, the configured test gate, and a delegated comprehensive review to pass. The goal is not complete while the review has any correctness, compatibility, evidence-integrity, security, testing, or documentation finding that can be acted on in this repository.

## Idempotence and Recovery

All source edits are made with `apply_patch`; generated snapshots may be refreshed only through repository-provided formatting or generation commands after their source templates are correct. Tests use temporary repositories and must not mutate real Git state. Baseline resolution and classification are read-only Git operations. Receipt and plan changes append to `.agent/state`; never rewrite those streams to retry.

If a check fails after appending child receipts, fix the code and rerun the same plan-linked command. Gate evaluation already selects the latest valid evidence. If a contract bump makes the cached launcher unusable during development, rebuild with `cargo build -p jig-sh --bin jig` and set `JIG_DEV_BIN=target/debug/jig`. If the current plan predates the implemented baseline field, it remains usable because this source repository's configured gates are unconditional; feature behavior is proven in temporary v5 plan fixtures.

If implementation reveals that a stated hash or state shape cannot preserve an invariant, update the Decision Log before changing course. Additive optional fields must preserve old JSONL deserialization. Do not delete or rewrite user changes. Do not close the work plan until review and every required gate are clean.

## Artifacts and Notes

Initial development build:

    Finished `dev` profile [unoptimized + debuginfo] target(s) in 36.93s

Structured plan creation:

    JIG_DEV_BIN=target/debug/jig scripts/jig work start ... --print-plan-id
    plan_01M0ZDM8RAM92J6PZ7Y6TYVVW2

The downstream motivating measurement was approximately 4m21s for frontend-only closure, with unrelated Rust and SQLx work consuming about 70 percent. The end-to-end fixture should demonstrate that native selection removes those commands rather than merely relabeling them optional.

## Interfaces and Dependencies

Add `globset` to the workspace dependency table and `crates/jig/Cargo.toml`. Do not invoke shell globs or depend on host shell expansion.

In `crates/jig/src/context/work_config.rs`, `WorkCheckGate` must expose the gate ID, tool, required flag, normalized include and ignore patterns, and reuse policy. It must provide an always-applicable predicate when includes are absent and a stable serializable/signable view.

In `crates/jig/src/state/records.rs`, add a backward-compatible plan baseline value carrying the requested ref when explicit, resolved commit OID when available, and sanitized collection error when unavailable. Every serializer, envelope/read model, timeline conversion, open-plan summary, session summary, archive reader, and test fixture that matches `PlanEvent::Open` must preserve or intentionally expose the new optional field.

In the Git boundary, define cancellation-aware equivalents of a plan change snapshot and gate scope snapshot. A plan change snapshot must contain baseline OID, complete changed-path count/digest, bounded preview, and classification error. A gate scope snapshot must additionally contain applicability, reason, matching-path evidence, and the scope fingerprint.

In work-check receipts, use a versioned evidence object rather than overloading a successful child tool receipt. Each gate evidence entry must identify the gate and tool, status (`executed`, `not_applicable`, `reused`, or `unknown`), signature, scope fingerprint/error, baseline, paths/digests, direct proving receipt when executed, and source evidence when reused. Readers must ignore unknown future evidence fields and preserve legacy receipts.

In CLI and MCP command DTOs, add optional baseline selection to work start and optional gate-ID selection to work check. Preserve existing invocations and JSON field names. Human output must show executed/reused/not-applicable counts even when no child check ran.

In templates, per-app tools must remain no-argument command tools so the existing supervised execution and receipt machinery stays authoritative. Generated command keys and tool/gate IDs must be deterministic, unique, safe under existing validators, and stable for a stable frontend list. The checker must reject an unknown app selector and must execute the existing `run_check` path for exactly one configured app.

Plan revision note (2026-08-26 16:17Z): Replaced the seed body created by `work start` with the first complete ExecPlan after source inspection. The design adds a contract epoch, baseline-relative applicability, scoped evidence, opt-in reuse, required-only default execution, explicit gate selection, and per-app generated frontend tools because each is necessary to close a distinct finding without weakening receipt semantics.

Plan revision note (2026-08-26 17:17Z): Marked implementation milestones complete after focused tests and embedded-template refresh. Recorded the failed-batch indexing and multi-app `packages/**` overlap discoveries, plus the compatibility decision to keep this legacy plan's dogfood Rust gate unconditional until closure.

Plan revision note (2026-08-26 17:35Z): Added native v2 receipt-archive protection after the final evidence-lifecycle audit. Refreshed embedded snapshots again after narrowing generated root-crate Rust scope, and reran focused applicability, reuse, adoption, inference, formatting, and archive tests successfully.

Plan revision note (2026-08-26 17:58Z): Closed phase 2 after strict Clippy findings were fixed, including boxing large gate-evidence variants and removing one all-targets-blocking redundant clone. The full configured matrix passed and both required gates report fresh plan-linked evidence.

Plan revision note (2026-08-26 18:18Z): Phase 4 rejected closure after the first delegated merged review reported eight actionable findings. Restarted at phase 1 and added explicit design decisions for scaffold-owned contract/public gates, stable direct-only reuse, current-plan rerun suppression, alias-only adoption migration, legacy contract selection, force-on-unknown execution, Rust tool config paths, and per-plan Git discovery caching.

Plan revision note (2026-08-26 18:58Z): Completed the second implementation phase. Added and proved explicit application-contract/public-artifact gates, safe direct reuse semantics, ownership-safe readoption aliases, v2-v4 selection compatibility, forced unknown execution, Rust tool configuration classification, one-time plan change discovery, non-UTF-8 fail-closed behavior, updated public documentation, and refreshed embedded templates. Focused suites, strict Clippy, formatting, diff checks, and the development-binary contract gate pass; the full plan-linked matrix is next.

Plan revision note (2026-08-26 19:16Z): The final-worktree plan-linked matrix passed in 962.2 seconds with 2,195 primary tests and the configured isolated vault shards. Batch receipt `receipt_01M0ZQYAR2DZGB0V6B82REFBZR` is fresh for both required gates. Began the second delegated comprehensive review.

Plan revision note (2026-08-26 19:33Z): Phase 4 rejected closure after the second delegated merged review reported eight further actionable findings. Restarted at phase 1 with explicit designs for glob/Git parity, centralized authority scopes, legacy-policy rejection, policy-level scope caching with streamed hashing, batched reuse scans, child failure status evidence, strict ID/tool adoption ownership, and MCP empty-array compatibility.

Plan revision note (2026-08-26 22:24Z): Completed the fourth implementation phase after the third review. New plans in unborn repositories persist an explicit empty-tree OID; gate classification owns path semantics and feeds pinned literal Git proofs; `.jig.toml` and the contract manifest apply every gate; refinement records unknown evidence and continues; generated application-contract scopes cover every app; legacy policy presence, readoption ownership, untracked execution modes, and status batching are covered by focused regressions. The focused suites and strict Clippy pass; complete dogfooded verification and the fourth merged review remain.

Plan revision note (2026-08-26 23:08Z): Phase 4 rejected closure after the fourth delegated merged review reported seven actionable findings. Restarted at phase 1 with conservative scoping for unrecognized custom commands, fully pinned and bounded Git proofs, native build identity in signatures, request-wide same-baseline snapshot sharing, legacy explicit-tool compatibility, and visible empty-tree baselines.

Plan revision note (2026-08-27): Phase 4 rejected closure after the fifth delegated merged review reported seven actionable findings. Restarted loop 6 at phase 1 and fixed the design boundaries for packaged native identity, strict generated-command authority, schema validation versus mutation, nested Git repositories, durable JavaScript workspace ownership, and batch-level changed-path evidence.

Plan revision note (2026-08-27): Completed loop 9 phase 2 and final verification. Corrected one stale runtime-loop test oracle to expect the new `<repository-root>` receipt representation, rebuilt the development binary, and passed the complete plan-linked matrix under batch receipt `receipt_01M10HSYG271QF6NYVYGVP3DSR`. Both required gates are fresh; all append-only JSONL parses; `git diff --check` passes; and the exact repository root is absent outside ignored build output. Four reproducible ignored fixture trees produced by the full test run were moved to the system trash before the privacy audit. Began the ninth delegated comprehensive review.

Plan revision note (2026-08-27): Completed loop 12 phase 2 and full verification after the post-PTY review. Archive compaction now retains selected-gate tombstones even when batch evidence is malformed; native identity and Cargo rebuild invalidation share one input list; repository-root redaction has two token boundaries; residual historical/provider fixtures are generic with amendment decision `decision_01M10V4HK0DKRCCHQ2EME8TFNW`; and the removed ambient schema override has migration documentation. Focused tests, strict Clippy, formatting, diff/contract/JSONL/privacy checks, 2,257 primary tests, all isolated shards, and fresh required-gate batch `receipt_01M10W1RZ6957VG28VQETHNCKZ` pass. Began the next delegated comprehensive review.

Plan revision note (2026-08-27): Loop 12 review rejected closure on three actionable findings, and loop 13 repaired each one. Exact repository roots redact before common right-side delimiters without consuming sibling prefixes; human work-check output counts and renders failed/cancelled evidence and derives its verdict from all evidence; snapshot refresh now occurs before native identity hashing. Focused regressions, 56 output tests, eight build-identity tests, strict Clippy, two real refresh builds with identical gate signatures, all hygiene checks, 2,260 primary tests, isolated shards, and fresh batch `receipt_01M10Y5HYS5H9FVD3XRZ390A8K` pass. Began the next delegated comprehensive review.

Plan revision note (2026-08-27): Loop 13 review rejected closure on three actionable findings, and loop 14 repaired each one. Checkout/package source selection is resolved once and shared by template generation and hashing; adopted contract gates require an exact bounded v1 checker marker unless explicitly enabled; and contextual Unicode-aware punctuation parsing preserves legal sibling names. Focused regressions, strict Clippy, two forced refresh builds with identical gate signatures, all hygiene checks, 2,263 primary tests, isolated shards, and fresh batch `receipt_01M110R8A774Q5A0PZRCKQ8ZS8` pass. Began the next delegated comprehensive review.

Plan revision note (2026-08-27): Loop 14 review rejected closure on three actionable findings, and loop 15 repaired each one. The latest exact failed outcome now blocks older evidence reuse; legacy whole-worktree Gitlink inspection preserves Unix filename bytes through a bounded raw index probe; and all dynamic generated gate paths are TOML-escaped. Exact regressions, broad affected suites, strict Clippy, template parity, all hygiene checks, 2,266 primary tests, isolated shards, and fresh batch `receipt_01M1136JMK2NZA3MMXA5SNK5M5` pass. Began the next delegated comprehensive review.

Plan revision note (2026-08-27): Loop 15 review rejected closure on six actionable findings, and loop 16 repaired all six with framed fingerprints/signatures, explicit reuse scan states, bounded raw selected Gitlink probes, literal generated-root validation, and staged-policy preview derivation. The first complete matrix found one empty optional migration sentinel regression after 2,272 other primary passes; loop 17 preserved omission before validation and reverified every adjacent input path. Strict Clippy, formatting, template parity, contract, diff, JSONL/privacy checks, 2,273 primary tests, all isolated shards, and fresh batch `receipt_01M117Z5H63CNNTCKJM7AA07SB` pass. Began the next delegated comprehensive review.

Plan revision note (2026-08-27): Loop 17 review rejected closure on three actionable findings, and loop 18 repaired each one. Scoped and whole-worktree evidence fail closed on ignored same-path replacements after staged deletion; canonical Git order-file paths remain OS-native; and unknown gate evidence omits a fictitious successful exit. The existing empty migration regression directly covers both CLI and answers-file input. Focused and broad suites, strict Clippy, formatting, template parity, contract, diff, JSONL/privacy checks, 2,275 primary tests, all configured isolated shards, and fresh batch `receipt_01M11ADBTCV72VP7FK35N7HZGS` pass. Began the next delegated comprehensive review.

Plan revision note (2026-08-27): The Loop 18 merged Claude Code plus independent native Codex review completed over the full 122-file working tree with `No actionable findings`, no open questions, and no material test gaps. It reverified every Loop 18 remediation and rejected three additional raw candidates as documented intentional behavior. The implementation and review loop is complete.
