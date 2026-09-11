# Verification evidence as completion authority

Implements `jig-sh-9wcn.3` against Git baseline
`d89129a880042665751954f81e270e02073a67ee`, preserving the staged task-02 work.
Structured work: `plan_01M28H9PQKK9MP5ESXN60AWC0B`.

The source and generated guides, including the source-only dogfooding section,
now describe current verification evidence as the completion condition. Short
notes, routine edits, substantial implementation, and durable handoffs have
separate guidance. Help and documentation distinguish execution, evidence reuse,
affected selection, required reviews, and explicit plan association.

## Verification inventory

No configured requirement needed adding or removing. The generated model's
`finish.rs` selects every read-only check action except the duplicate locked-test
action and compatibility aggregates. The template's legacy branch explicitly
lists tool gates. Authored models and work configuration remain untouched.

| Preexisting obligation | Native generated policy | Legacy generated policy / retained guidance |
| --- | --- | --- |
| Backend tests | `api:test`, or neutral Rust `workspace:test` | `jig.test`; backend test evidence remains explicit in Done Means |
| Rust formatting and Clippy | `api:fmt`, `api:clippy` (or `workspace`) | No new legacy obligation; existing authored gates remain authoritative |
| Go formatting and analysis | `api:fmt`, `api:lint` | No new legacy obligation; existing authored gates remain authoritative |
| Jig contract and file budget | `repo:contract`, full-footprint `repo:file-budget` | `jig.contract_check`; existing authored policy preserved |
| SQLx or migration changes | `api:sqlx` | `jig.sqlx_check`; explicit SQLx evidence requirement retained |
| Schema documentation | `api:schema`; effectful `schema-dump` stays outside read-only profile | Both `jig.schema_check` and historical `jig.schema_dump` gates preserved |
| Go/Postgres generated SQL | `api:sqlc` | `jig.sqlc_check`; existing sqlc guidance retained |
| Frontend lint, types, bundle, coverage | Each app's `lint`, `typecheck`, `build`, `test` | Four `jig.typescript_*` gates; relevant frontend evidence requirement retained |
| Application contracts / public boundaries | Existing repository frontend contract targets and dependencies | Existing explicit checker commands remain in guidance |
| Migration immutability | Forward-only migration guidance and configured checks/CI remain unchanged | Same; SQLx metadata verification is not described as an immutability check |
| Custom profiles / required reviews | Existing authored profiles and `codex_review` gates | Same; `work review` must supply review evidence |

The source repo's six-member `verify` profile remains unchanged: Clippy, format,
tests, contract, file budget, and task-02's Python evaluation tests. Neither
`.jig.toml` nor its template was changed by task 03. The frozen historical
evaluation guidance is intentionally unchanged.

## Acceptance evidence

The following current implementation and test entrypoints establish the scope
of verification; execution results are recorded in the linked work plan and its
append-only receipts, not in a second gate ledger.

- `bootstrap::renderer::tests`: rendered Rust and Go/Postgres with frontend apps
  for legacy v5 and native v8, SQLx/schema enabled and disabled, flat migrations
  and versioned artifacts, and neutral Rust workspace recopy. The existing
  policy matrix now checks required target/tool membership, rather than merely
  checking prose. Embedded snapshot parity covers the shipped guide.
- `runtime::tests::work::evidence::retries`: actual invocation counts prove a
  second work check reuses original passes, failed targets rerun, changed inputs
  invalidate evidence, and a rerun prerequisite invalidates its dependent.
- `runtime::tests::work::evidence::scoped_freshness`: declared whole-repository
  and exhaustive policies, Git and worktree source authority, observation
  failures, and recovery previews retain their existing semantics.
- `runtime::tests::work::evidence::scoped_freshness::recovery::selected_legacy_tool_explains_native_gate_mismatch_without_creating_target_evidence`
  proves that a legacy tool pass cannot satisfy a native target gate.
- `runtime::tests::work::review::work_review_records_structured_codex_review_findings`
  now also checks that successful work checks leave a required review missing
  and finish blocked. Its existing assertions verify review failure evidence.
- Existing adoption and recopy tests preserve authored profiles, review gates,
  custom commands, required flags, and unmanaged guide sections.
- `check --help`, `work start --help`, `work check --help`, and
  `work finish --help` describe the current workflow. `docs/developer-ux.md` and
  `docs/configuration.md` correct obsolete claims that a profile requires a
  single run and explain that an affected no-op is only candidate selection.

## Verification commands and result location

From the repository root, build `cargo build -p jig-sh --bin jig` and select
`JIG_DEV_BIN=target/debug/jig` for all harness commands. The relevant commands are:

```sh
cargo test -p jig-sh --lib bootstrap::renderer::tests
cargo test -p jig-sh --lib runtime::tests::work:: -- --test-threads=4
JIG_DEV_BIN=target/debug/jig scripts/jig check --explain
JIG_DEV_BIN=target/debug/jig scripts/jig work check --plan-id plan_01M28H9PQKK9MP5ESXN60AWC0B
JIG_DEV_BIN=target/debug/jig scripts/jig work finish --plan-id plan_01M28H9PQKK9MP5ESXN60AWC0B --resolution 'Configured verification evidence policy implemented and verified'
```

The first expanded renderer run failed because its newly enabled SQLx fixture
omitted `rust_migration_dir`; the fixture now supplies generic `migrations`, and
all 12 renderer tests passed. Remaining command results, any failures, and final
acceptance disposition belong in the work plan's progress and closure records.
Use `work receipts --plan-id plan_01M28H9PQKK9MP5ESXN60AWC0B` when inspecting them.
The required profile supplies backend test evidence; the updated policy does not
require an additional test invocation solely for last-command ordering.

No migrations, receipt formats, gate scheduling, freshness implementation, or
adoption reconciliation were changed. No model evaluation, deployment, or
external messages are part of this task.
