# Define the open status-provider v1 contract

This ExecPlan is a living document. The sections `Progress`, `Surprises & Discoveries`, `Decision Log`, and `Outcomes & Retrospective` must remain current while work proceeds. Maintain this document in accordance with `.agent/PLANS.md`.

## Purpose / Big Picture

After this change, an executable written in Ruby, Rust, or another language can publish software-rewrite status through one public Jig protocol. The provider implementation may remain closed source, but its output is portable: Jig and any other consumer can validate the same JSON document, inspect work-package specification, implementation, verification, acceptance-check, dependency, blocker, and evidence observations, and tolerate compatible additions.

A human can see the contract working by running the `jig-contract` tests. The tests deserialize the committed example, validate it against the committed JSON Schema Draft 2020-12 document, compare that document with the schema generated from Rust, reject an unsupported protocol version, and exercise semantic validation that JSON Schema cannot express. This change defines only the open data and process contract. It does not yet execute a configured provider, change hocr2's private Ruby verifier, aggregate Git or lease state, add the TUI, or launch Codex.

## Progress

- [x] (2026-07-27 08:50Z) Inspected the clean worktree, repository guidance, public-contract documentation, existing DTO ownership, UI snapshot boundary, and downstream contract checker.
- [x] (2026-07-27 08:50Z) Opened structured work as `plan_01KYHC9JN9D752GGT36S5PT3YE` and resolved the v1 scope and ownership decisions.
- [x] (2026-07-27 09:04Z) Added the public Rust DTOs, semantic validation, schema generator, committed schema, and conformance example.
- [x] (2026-07-27 09:04Z) Added seven schema drift, JSON Schema conformance, compatibility, version, constructor, and semantic-validation tests.
- [x] (2026-07-27 09:04Z) Documented the provider process rules, open/closed-source boundary, versioning policy, normalized states, and field semantics.
- [x] (2026-07-27 09:33Z) Ran focused tests, package verification, format, strict Clippy, the full repository test gate, and structured work gates; all final evidence is fresh and passing.

## Surprises & Discoveries

- Observation: `crates/jig-contract` already has the exact ownership rule this protocol needs: it owns stable shared DTOs but may not perform process execution, filesystem mutation, or repository loading.
  Evidence: `crates/jig-contract/AGENTS.md`.
- Observation: the current `DashboardSnapshot` is a UI-owned, same-release presentation type containing command, timeline, and display-limit fields, so using it as an external provider payload would couple providers to one view.
  Evidence: `crates/jig-ui/src/model.rs` and `crates/jig-ui/AGENTS.md`.
- Observation: `jig check contract` validates generated-repository command manifests and runtime wiring. It runs in downstream repositories, where Jig source-release schema artifacts do not exist.
  Evidence: `crates/jig/src/policy.rs::contract_check` and `docs/public-contract.md`.
- Observation: a current Draft 2020-12 validator adds a substantial dependency graph even with file and HTTP resolution disabled.
  Evidence: `jsonschema` 0.48.5 is confined to `jig-contract` development dependencies; production crates depend only on Serde, Serde JSON, and Schemars for this protocol.
- Observation: keeping the complete public v1 DTO vocabulary and its semantic rules together makes the contract easy to audit but places `src/status_provider/v1.rs` just above Jig's normal 800-line source limit.
  Evidence: the file carries a narrow `agentic-loc-exception` annotation; tests and provider execution remain in separate files and crates.
- Observation: the first full workspace run exposed a pre-existing load-sensitive 20 ms timeout assertion while unrelated hocr2 verification and Cargo suites were running concurrently.
  Evidence: `doctor::tests::sqlx_driver_probe_invokes_shim_safely_and_times_out` was the only failure after 1,151 passing tests, passed immediately in isolation, and the unchanged full work check then passed with receipt `receipt_01KYHEQJP8XVSNGBBYM8GVY5X3`.
- Observation: placing the schema and example at the repository root would make them unavailable inside a published `jig-contract` crate.
  Evidence: moving them under `crates/jig-contract/contracts/status-provider/` produced a verified package containing both artifacts, as shown by `cargo package -p jig-contract --allow-dirty --locked --list`.

## Decision Log

- Decision: put the Rust source of truth in `jig-contract`, with generated artifacts under `crates/jig-contract/contracts/status-provider/`.
  Rationale: the DTO crate is the stable, dependency-downward contract layer. Keeping the JSON Schema and example inside its package root lets Ruby and non-Rust consumers conform without importing Rust and ensures the artifacts ship with a published crate.
  Date/Author: 2026-07-27 / Codex
- Decision: identify the major version with the exact wire token `jig.status-provider/v1` and keep the provider implementation version separate.
  Rationale: dispatch can inspect one unambiguous field before deserializing a version-specific type. Provider releases do not imply protocol changes.
  Date/Author: 2026-07-27 / Codex
- Decision: model software-rewrite work packages explicitly, while keeping provider-specific data in namespaced `extensions` maps.
  Rationale: the first consumers need reliable specification, implementation, verification, dependency, acceptance-check, blocker, and evidence semantics. An arbitrary fact bag would move interoperability problems into every consumer, while hocr2-only analysis details do not belong in the open core.
  Date/Author: 2026-07-27 / Codex
- Decision: preserve provider-native state strings beside a small normalized category enum.
  Rationale: Jig can consistently count and render `unknown`, `pending`, `ready`, `active`, `blocked`, `complete`, and `failed`, while a provider can retain meaningful states such as `ready_to_close` without forcing them into the protocol enum.
  Date/Author: 2026-07-27 / Codex
- Decision: keep final launchability outside the provider payload.
  Rationale: a project inspector cannot know Jig-owned Git freshness, active leases, worktree cleanliness, gate availability, or active runs. The provider reports domain observations; a later Jig aggregator calculates whether an implementation agent may start.
  Date/Author: 2026-07-27 / Codex
- Decision: make valid provider output exit successfully even when it contains blockers, and reserve process failure for the inability to produce a trustworthy report.
  Rationale: a blocked work package is status data, not a crashed provider. Existing CI-oriented verifier behavior can remain on its current command path while a future dedicated provider mode adopts this rule.
  Date/Author: 2026-07-27 / Codex
- Decision: enforce source-release schema drift through `jig-contract` tests rather than changing `jig check contract`.
  Rationale: the existing check has a different public responsibility and runs in generated downstream repositories. Cargo tests can compare generated and committed schema at the owning source boundary.
  Date/Author: 2026-07-27 / Codex
- Decision: keep the offline JSON Schema validator test-only and keep v1's public DTOs and semantic validator in one explicitly annotated source file.
  Rationale: consumers need executable schema conformance evidence without inheriting validator dependencies at runtime. The single version file remains below Jig's absolute limit, while the annotation makes its deliberate contract-audit role visible.
  Date/Author: 2026-07-27 / Codex

## Outcomes & Retrospective

Jig now publishes a versioned `jig.status-provider/v1` observation contract without taking ownership of a provider's implementation. The `jig-contract` crate exposes constructors, DTOs, normalized categories, semantic validation, and deterministic Draft 2020-12 schema generation. The packaged schema and example make the same boundary usable from Ruby or any other language.

Documentation now defines the read-only process protocol, stdout/stderr and exit behavior, provider versus consumer ownership, open core versus private extensions, path and evidence hygiene, and major-version compatibility rules. It explicitly keeps final launchability, provider execution, aggregation, TUI presentation, hocr2's Ruby adapter, and Codex launching as later milestones.

Seven focused contract tests, package verification, format, strict workspace Clippy, the generated command-contract check, agent guides, agent map, changed-file policy, and the final full workspace test run pass. Required plan gates are fresh and passing against worktree fingerprint `f28a7c1e78ee288c3c49002c9a1608ceeb58891f`.

## Context and Orientation

`crates/jig-contract/src/lib.rs` currently defines shared tool names, manifest DTOs, and feature metadata. Add a `status_provider` module here, but keep it free of command execution and repository access. The module will define the v1 wire types, constructors for Rust provider authors, a pure semantic validator, and a deterministic Draft 2020-12 schema generator.

`crates/jig-contract/contracts/status-provider/v1.schema.json` will be the committed language-neutral schema. `crates/jig-contract/contracts/status-provider/v1.example.json` will be a realistic conforming report that uses only repository-relative source paths and records the target and legacy Git revisions inspected. Tests inside `jig-contract` will consume both artifacts, and Cargo will package them with the crate.

`docs/public-contract.md` explains which Jig surfaces are stable. Add a status-provider section and link to a focused `docs/status-provider.md` document. The focused document must distinguish the provider observation from Jig's future aggregate state and from a TUI presentation model. It must also state the stdout, stderr, exit-code, ordering, path, extension, timestamp, version, and compatibility rules needed by a closed-source Ruby implementation.

The existing `.agent/jig-contract.json` is the generated command and MCP manifest for adopted repositories. It is unrelated to this provider payload and must not be changed by this work.

## Plan of Work

First, add Schemars 1.2 to the workspace and `jig-contract`, keep Serde JSON available for extension values and schema generation, and add the offline-only `jsonschema` validator as a `jig-contract` development dependency. Pin versions compatible with the workspace Rust 1.85 floor.

Second, create `crates/jig-contract/src/status_provider.rs`. Expose the constant protocol token and schema identifier. Under a `v1` namespace, define the report envelope, provider identity, outcome, inspected input, work package, status facet, normalized category, acceptance check, blocker, evidence reference, diagnostic, diagnostic level, and source location types. Use Serde and Schemars derives. Use explicit `extensions` objects rather than flattening arbitrary keys into the stable namespace. Constructors must make the types usable by an external Rust provider even where `#[non_exhaustive]` protects future additive Rust fields.

Add pure semantic validation for constraints that generated JSON Schema cannot reliably express: nonblank identifiers and native states, unique work-package identifiers, unique acceptance-check ordinals within a package, and repository-relative forward-slash source paths without `.` or `..` components, backslashes, NUL bytes, drive prefixes, or leading slashes. Return all detected validation errors with JSON-style field paths rather than stopping at the first.

Third, expose a schema function that explicitly selects JSON Schema Draft 2020-12, inserts the stable `$id`, and returns a Schemars schema. Commit the pretty-printed generated schema and one example. Tests must compare parsed generated and committed schemas, validate the example with an offline validator, deserialize it into the Rust DTO, run semantic validation, and prove that compatible unknown fields are accepted. Negative tests must cover an unsupported protocol token, duplicate work-package identifiers, duplicate acceptance ordinals, and unsafe source paths.

Finally, document the public boundary and run validation. The documentation must explain that the JSON contract is MIT-licensed with Jig, while a provider's discovery implementation and extensions can remain private. It must identify core fields that every generic consumer may use and clarify that consumers must not require private extensions for basic status rendering.

## Concrete Steps

All commands run from `/Users/aa/Documents/jig-sh`.

1. Edit files with `apply_patch`, then format the Rust sources:

       cargo fmt --all

2. Run the owning crate tests while iterating:

       cargo test -p jig-contract

   Expect tests for schema drift, example conformance, additive-field compatibility, protocol rejection, and semantic validation to pass.

3. Run focused quality gates:

       cargo fmt --all -- --check
       cargo clippy -p jig-contract --all-targets -- -D warnings
       cargo test -p jig-contract

4. Build the development Jig binary before dogfooding repository checks:

       cargo build -p jig-sh --bin jig
       JIG_DEV_BIN=target/debug/jig scripts/jig check contract
       JIG_DEV_BIN=target/debug/jig scripts/jig check agent-guides
       JIG_DEV_BIN=target/debug/jig scripts/jig check agent-map

5. Run the repository's required full checks and structured gates:

       JIG_DEV_BIN=target/debug/jig scripts/jig check fmt
       JIG_DEV_BIN=target/debug/jig scripts/jig check clippy
       JIG_DEV_BIN=target/debug/jig scripts/jig check test
       JIG_DEV_BIN=target/debug/jig scripts/jig work check --plan-id plan_01KYHC9JN9D752GGT36S5PT3YE
       JIG_DEV_BIN=target/debug/jig scripts/jig work gates --plan-id plan_01KYHC9JN9D752GGT36S5PT3YE
       JIG_DEV_BIN=target/debug/jig scripts/jig work evidence --plan-id plan_01KYHC9JN9D752GGT36S5PT3YE

## Validation and Acceptance

The committed example must validate under JSON Schema Draft 2020-12 without network resolution, deserialize into the public Rust type, and pass semantic validation. Its protocol value must be exactly `jig.status-provider/v1`; changing that value to `v2` must fail v1 schema validation and v1 Rust deserialization.

The schema generated from the Rust DTO must equal the committed schema as parsed JSON. Changing a DTO without regenerating the schema must fail the owning test and show a clear regeneration command. Unknown additional fields at the report, provider, work-package, and extension levels must remain acceptable so v1 can gain optional fields compatibly.

The semantic validator must report every relevant error in one pass. Duplicate package IDs, duplicate acceptance ordinals, blank stable identifiers, and absolute or traversing source paths must fail with field paths that identify the offending item.

The public documentation must make it possible for a private Ruby executable to implement the protocol without reading Jig's Rust source. It must specify the process boundary, the meaning and ownership of every core field, the fixed normalized states, compatibility behavior, and which launchability facts are deliberately absent.

The full repository format, Clippy, test, command-contract, agent-guide, agent-map, and structured work gates must pass. No TUI, provider runner, `.jig.toml` provider configuration, hocr2 Ruby code, final launchability calculation, or Codex launcher is required for this plan.

## Idempotence and Recovery

All source changes are additive. Schema generation is deterministic; rerunning it must produce the same parsed JSON. If a generated schema differs, inspect the Rust DTO change before replacing the committed artifact, because accepting drift is a compatibility decision.

If dependency resolution selects a crate requiring newer than Rust 1.85, retain the workspace floor and choose an older compatible dependency release. If the `jsonschema` development dependency creates an unacceptable runtime dependency, keep it under `[dev-dependencies]`; it is needed only by conformance tests. No durable user data or downstream repository manifest is migrated.

The `.agent/state/*.jsonl` files are append-only work memory. Do not edit their existing records. New plan and receipt records created by the commands above are expected.

## Artifacts and Notes

Initial repository doctor completed successfully. Required repo, configuration, runtime, generated contract, tools, and agent-skill checks passed; only optional vault initialization is absent.

The worktree was clean before `scripts/jig work start`. The branch was two commits ahead of `origin/master`; this work will preserve that history and will not rewrite existing commits.

Focused evidence at 2026-07-27 09:04Z:

    cargo fmt --all -- --check
    cargo test -p jig-contract --locked
    cargo clippy -p jig-contract --all-targets --locked -- -D warnings

All commands passed. The contract test target ran seven tests successfully.

Packaging evidence:

    cargo package -p jig-contract --allow-dirty --locked
    cargo package -p jig-contract --allow-dirty --locked --list

The verified package contains `contracts/status-provider/v1.schema.json`, `contracts/status-provider/v1.example.json`, the schema example program, public Rust modules, and conformance tests.

Final repository evidence at 2026-07-27 09:33Z:

    JIG_DEV_BIN=target/debug/jig scripts/jig check fmt --plan-id plan_01KYHC9JN9D752GGT36S5PT3YE
    JIG_DEV_BIN=target/debug/jig scripts/jig check clippy --plan-id plan_01KYHC9JN9D752GGT36S5PT3YE
    JIG_DEV_BIN=target/debug/jig scripts/jig work check --plan-id plan_01KYHC9JN9D752GGT36S5PT3YE
    JIG_DEV_BIN=target/debug/jig scripts/jig work gates --plan-id plan_01KYHC9JN9D752GGT36S5PT3YE --json
    JIG_DEV_BIN=target/debug/jig scripts/jig work evidence --plan-id plan_01KYHC9JN9D752GGT36S5PT3YE --json

The final work check passed with batch receipt `receipt_01KYHEQJP8XVSNGBBYM8GVY5X3`. Required `contract` and `tests` gates report `fresh`, `passed`, and `gates_ok: true`.

## Interfaces and Dependencies

`jig_contract::status_provider::V1_PROTOCOL` must equal `jig.status-provider/v1`. `jig_contract::status_provider::V1_SCHEMA_ID` must identify the committed v1 schema.

`jig_contract::status_provider::v1::Report` is the root wire type. It contains the protocol marker, provider identity, observation timestamp in Unix milliseconds, provider outcome, inspected inputs, work packages, provider-level diagnostics, and namespaced extensions. A `Report::validate()` method returns all semantic errors without filesystem access.

Each work package has stable `id`, optional `title`, explicit `specification`, `implementation`, and `verification` facets, dependencies, acceptance checks, blockers, evidence references, and extensions. A facet carries the provider-native `state`, one normalized category, optional summary, optional source location, and optional digest.

The normalized category values are `unknown`, `pending`, `ready`, `active`, `blocked`, `complete`, and `failed`. Provider outcome values are `complete` and `partial`. Diagnostic levels are `info`, `warning`, and `error`. These enums are fixed for v1; new values require a new major protocol version because older schemas reject unknown enum values.

All optional fields must use Serde defaults or omission rules consistently. Unknown fields must remain accepted. The `extensions` maps contain JSON values keyed by reverse-domain or similarly collision-resistant namespaces such as `factorish.rails-rewrite`.

Schemars must generate Draft 2020-12. The `jsonschema` crate is test-only with remote and file resolution disabled. No runtime, UI, filesystem, network, process, or project-configuration dependency may be added to `jig-contract`.

Plan revision note (2026-07-27 08:50Z): replaced the initial one-line body with a self-contained implementation and validation plan after auditing Jig's contract, UI, policy, documentation, and worktree boundaries.

Plan revision note (2026-07-27 09:33Z): recorded the complete implementation, packaged-artifact correction, focused and full validation, isolated timing-flake diagnosis, passing rerun, and final fresh gate receipts.
