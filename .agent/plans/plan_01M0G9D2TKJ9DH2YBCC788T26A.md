# Harden Go backend boundaries

This ExecPlan is a living document. Keep `Progress`, `Surprises & Discoveries`, `Decision Log`, and `Outcomes & Retrospective` current in accordance with `.agent/PLANS.md`.

## Purpose / Big Picture

The Go backend branch is functionally coherent, but the review found places where backend generalization stopped halfway: migration policy became backend-neutral while authoring and CLI paths remained SQLx-specific; shared scaffold validation still encoded Rust roots; and generated files copied implementation rather than sharing stable template sources. Other findings are boundary omissions: an unbounded authority-file read, non-portable shell loops, and a third-party default hook that remained active after its route was disabled.

After this work, migration path and authoring behavior will have one backend-neutral runtime boundary with backend-specific file formats behind it. Scaffold validation will receive backend identity explicitly. Version authorities will share one bounded-file reader. Generated shell and API behavior will be covered at their observable boundaries, and identical generated-client runtime templates will have one source. Each coherent slice will be committed separately and the complete configured repository gates will pass at the end.

## Progress

- [x] (2026-08-20) Reviewed `origin/master...HEAD`, classified nine findings by root cause, and opened structured work plan `plan_01M0G9D2TKJ9DH2YBCC788T26A`.
- [x] (2026-08-20) Made migration configuration, CLI authoring, native-tool registration, generated contracts, and backend-specific file formats one coherent abstraction; retained legacy command/config compatibility at one boundary.
- [x] (2026-08-20) Disabled Huma create hooks with schema routes, asserted response bodies and headers have no unresolved schema links, and regenerated the committed OpenAPI/client artifacts.
- [x] (2026-08-20) Reused one regular-file-only, size-capped numeric-version authority reader for Node and Go, with regression coverage for symlinks, invalid UTF-8, multiple tokens, and oversized/empty files.
- [x] (2026-08-20) Filtered missing tracked files before `gofmt`, proved the generated command against a deleted-file/ignored-file Git fixture, and replaced `seq` readiness loops with Bash arithmetic in both backend scaffolds.
- [x] (2026-08-20) Unified interactive/default Go module derivation, rejected migration configuration without Go/PostgreSQL, omitted that config from no-database scaffolds, and made reserved frontend roots preset-specific.
- [x] (2026-08-20) Split public-client registration into shared runtime and backend-specific contract templates, removed 13 byte-identical Go copies, and verified every removed file matches its canonical source byte-for-byte.
- [x] (2026-08-20) Rebuilt the development runtime; passed focused tests, all 2,159 complete-suite tests, format, Clippy, contract, and structured-work evidence with fresh receipts.

## Surprises & Discoveries

- Observation: Removing Huma's read-only `$schema` properties also removes Hey API's generated `*Writable` aliases; those aliases existed only to omit the read-only link field and become redundant once the response and writable shapes are identical.
  Evidence: `node scripts/contracts.mjs check` identified `index.ts`, `types.gen.ts`, and `zod.gen.ts` drift; regeneration removed only `$schema` members and the now-identical writable aliases.

- Observation: The first complete test run passed 2,158 of 2,159 tests; the sole failure was an integration fixture that still asserted command-inventory schema v2 and omitted the new visible `migration` root.
  Evidence: Nextest run `e78d411a-576c-4060-a3c4-6f4295400343` failed only `jig-sh::cli_json info_commands_exposes_versioned_json_and_grouped_human_output`; the focused rerun reported `left: 3, right: 2` at the stale fixture assertion.

- Observation: A prior hardening slice intentionally made `migration_dir` project-owned and backend-neutral, but SQLx migration creation, command inventory, Rust workflow triggers, and generated Rust guidance still read `rust_migration_dir` directly.
  Evidence: `RepoContext::migration_dir` prefers the neutral key, while `policy::migration_add`, `info::commands::sqlx_command`, and `templates/project/.github/workflows/repo-policy.yml.jinja` use the legacy key.

- Observation: Huma's default create hook uses the final empty `SchemasPath`, so the resulting broken links target root paths such as `/AppStatusResponse.json`, not the old `/schemas/...` path.
  Evidence: Huma v2.39.1 `DefaultConfig` installs `NewSchemaLinkTransformer(schemaPrefix, c.SchemasPath)` during adapter creation; the generated OpenAPI example is `https://example.com/AppStatusResponse.json` while route registration is disabled for an empty path.

- Observation: The legacy hidden `jig migration-add` command already routes through the generic manifest tool name `jig.migration_add`, but its public CLI DTO/runtime ownership and feature registration are still nested under SQLx.
  Evidence: `cli/sqlx.rs`, `command/sqlx.rs`, and `runtime/sqlx.rs` own the request even though `jig-contract` names the tool generically.

## Decision Log

- Decision: Complete the neutral migration cutover instead of adding equality checks around two active path authorities.
  Rationale: Keeping both keys active in different consumers guarantees future drift. `migration_dir` becomes the runtime authority; `rust_migration_dir` remains a persisted compatibility fallback and is rendered equal for SQLx repositories. Divergent values are rejected while the compatibility key exists.
  Date/Author: 2026-08-20 / Codex

- Decision: Expose `jig migration add` as the backend-neutral public command, retain `jig sqlx migration add` and hidden `jig migration-add` as compatibility aliases, and let one native tool emit SQLx or Goose format based on repository context.
  Rationale: Documentation alone would preserve duplicate authoring paths and an unused Goose CLI dependency. One tool boundary makes receipts, validation, slugging, timestamps, and path selection common while keeping only the actual file format backend-specific.
  Date/Author: 2026-08-20 / Codex

- Decision: Fix boundary omissions through reusable helpers or generated behavior tests, not call-site guards.
  Rationale: A shared bounded authority reader protects every version-file consumer; explicit preset-aware validation protects every frontend input path; and contract-level API tests prevent third-party defaults from silently returning.
  Date/Author: 2026-08-20 / Codex

- Decision: Share only byte-identical generated-client runtime templates, retaining backend-specific schema-derived files.
  Rationale: Runtime support files are generator-stable for the same generator version, while types, SDK, Zod, index, and React Query output legitimately differ with the backend OpenAPI contract.
  Date/Author: 2026-08-20 / Codex

## Outcomes & Retrospective

The review findings had a mixed cause. The dominant cluster was structural: the Go cutover introduced backend-neutral concepts without moving every consumer to the neutral boundary. Migration configuration, CLI ownership, feature registration, scaffold validation, and client template ownership consequently retained independent Rust/SQLx assumptions. The long-term fix was to make backend identity or backend-neutral policy explicit at those boundaries, retain compatibility only at ingestion/alias edges, and keep backend branching limited to genuinely different output formats.

The remaining defects were narrower boundary omissions: Huma's default hook stayed active after its schema routes were disabled; Go authority files lacked the hardened reader already used by Node; and generated shell assumed both present tracked files and an external `seq`. Those were fixed with observable boundary tests and shared helpers so equivalent paths cannot drift independently.

Implementation landed in separate commits for migration abstraction, schema-link behavior, authority-file hardening, generated shell behavior, init/scaffold validation, template deduplication, and the schema-v3 integration fixture. The final `jig.test` receipt `receipt_01M0GCWR44QYAJFBWFYPEHTMK2` records 2,159 passing tests. Format receipt `receipt_01M0GCWZKX8Q4JD48V3Y5AVNDR`, Clippy receipt `receipt_01M0GCXRFKR7A44CNDXRFYXK24`, and contract receipt `receipt_01M0GCXWA4CF292TSXV5V2TTBF` all passed with no changes. `scripts/jig work gates` and `scripts/jig work evidence` reported every required gate fresh and no unresolved gates.

## Context and Orientation

`crates/jig/src/context.rs` loads `.jig.toml` and owns effective backend/path queries. `crates/jig/src/policy.rs` implements native migration creation and immutability. `crates/jig-contract`, `jig-core`, `jig-sqlx`, `jig-go`, and `jig-features` define supported native tools and required contract tools. CLI parsing is under `crates/jig/src/cli/`, neutral runtime requests under `command/`, and execution under `runtime/`.

`crates/jig/src/bootstrap/scaffold.rs` builds Rust/React and Go/React scaffold plans. Source templates live under `templates/scaffolds`; byte-for-byte embedded mirrors live under `crates/jig/src/bootstrap/scaffold/embedded_template_snapshots` and must be refreshed through `JIG_REFRESH_EMBEDDED_TEMPLATE_SNAPSHOT=1 cargo check -p jig-sh`.

## Plan of Work

First, finish the migration abstraction cutover. Move migration native-tool ownership to the generic feature boundary, add a visible `migration add` CLI while retaining aliases, use `migration_dir` everywhere, reject divergent compatibility keys, emit Goose single-file migrations for Go/PostgreSQL, expose the tool in generated Go contracts/guides, and remove the unused Goose CLI tool dependency.

Second, disable Huma's schema-link create hook when schema routes are disabled. Update generated tests to assert that responses do not contain `$schema` or `Link`, refresh the OpenAPI/client artifacts and embedded snapshots, and run generated Go tests.

Third, extract a bounded regular-file numeric-version authority helper and use it for `.node-version` and `.go-version`, preserving their distinct optionality and numeric grammar. Add filesystem-shape regressions.

Fourth, filter deleted tracked paths out of the generated Go formatting command and replace `seq` readiness loops in both generated backend scripts with stock Bash arithmetic. Add rendering assertions and refresh snapshots.

Fifth, derive the interactive Go module default through the same helper as `--defaults`, show it in the prompt, reject `migration_dir` when the Go preset has no PostgreSQL database, and pass preset-specific reserved backend roots into frontend validation. Cover direct frontend specs and answer-file directories.

Sixth, factor public-client template registration into shared runtime templates plus backend-specific contract templates, delete redundant Go source templates, refresh embedded snapshots, and prove generated output remains unchanged.

## Concrete Steps

Run from `/home/aa/.herdr/worktrees/jig-sh/feat-codex-resume`. After every template slice:

    JIG_REFRESH_EMBEDDED_TEMPLATE_SNAPSHOT=1 cargo check -p jig-sh
    cargo test -p jig-sh <focused filter>
    cargo fmt --all -- --check
    git diff --check

Commit each coherent slice only after focused verification. At the end:

    cargo build -p jig-sh --bin jig
    JIG_DEV_BIN=target/debug/jig scripts/jig work check --plan-id plan_01M0G9D2TKJ9DH2YBCC788T26A
    JIG_DEV_BIN=target/debug/jig scripts/jig check fmt
    JIG_DEV_BIN=target/debug/jig scripts/jig check clippy
    JIG_DEV_BIN=target/debug/jig scripts/jig check contract
    JIG_DEV_BIN=target/debug/jig scripts/jig work gates --plan-id plan_01M0G9D2TKJ9DH2YBCC788T26A
    JIG_DEV_BIN=target/debug/jig scripts/jig work evidence --plan-id plan_01M0G9D2TKJ9DH2YBCC788T26A
    JIG_DEV_BIN=target/debug/jig scripts/jig work receipts --plan-id plan_01M0G9D2TKJ9DH2YBCC788T26A

## Validation and Acceptance

A Rust SQLx repository must use one effective migration directory for contract validation, workflow triggers, CLI discovery, immutability, and authoring. Legacy-only configuration must continue working; divergent neutral and legacy values must fail clearly. A Go/PostgreSQL repository must declare the same native migration tool and `jig migration add NAME` must create a valid Goose file under its configured path.

Generated API responses must have neither `$schema` nor `describedBy` links when schema serving is disabled. Doctor must reject symlink, non-regular, oversized, empty, invalid UTF-8, and multi-token version authorities without following or unboundedly reading them. Formatting must ignore a deleted tracked Go file, and PostgreSQL readiness loops must use only supported Bash features.

Interactive and defaults initialization must derive the same module path. Go frontends must not occupy `cmd` or `internal`. Removing duplicate template sources must not change rendered client bytes. The final complete configured gate set must be fresh and passing.

## Idempotence and Recovery

Snapshot refresh, formatting, and tests are deterministic and safe to repeat. Each implementation slice is committed separately; repair later failures in a new commit instead of rewriting unrelated history. Use only generic temporary fixture names. `.agent/state/*.jsonl` remains append-only and must never be truncated or rewritten.

## Interfaces and Dependencies

No new external dependency is required. The existing generic tool identifier `jig.migration_add` remains stable. `jig sqlx migration add` and `jig migration-add` remain accepted aliases. Persisted `rust_migration_dir` remains readable as a fallback, while newly rendered SQLx configuration keeps it synchronized with canonical `migration_dir`.
