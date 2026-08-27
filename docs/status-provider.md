# Status-provider protocol

The Jig status-provider protocol is an open JSON boundary between a project-specific inspector and any status consumer. The provider may be a private Ruby executable, a public Rust binary, a container, or another implementation. Its source and discovery algorithms do not need to be published. Its report conforms to the public `jig.status-provider/v1` schema.

Version 1 defines the provider observation contract. Jig can configure and execute v1 providers, expose a fresh aggregate with `scripts/jig status --json`, and present that aggregate through `scripts/jig status --tui`; the aggregate is a separate, runtime-owned schema. Jig does not cache provider reports, expose them in its web flight-recorder UI, decide launchability, or launch an implementation agent.

Normative artifacts:

- [`crates/jig-contract/contracts/status-provider/v1.schema.json`](../crates/jig-contract/contracts/status-provider/v1.schema.json) is the JSON Schema Draft 2020-12 document.
- [`crates/jig-contract/contracts/status-provider/v1.example.json`](../crates/jig-contract/contracts/status-provider/v1.example.json) is a conforming software-rewrite example.
- [`crates/jig-contract/src/status_provider.rs`](../crates/jig-contract/src/status_provider.rs) and its `v1` module are the Rust source of truth.

## Provider and consumer boundary

A provider observes project-owned facts. For a software rewrite, those facts include the work-package specification, implementation and verification states, dependencies, acceptance checks, domain blockers, evidence references, and the exact source inputs inspected.

A consumer validates and combines observations. Jig's aggregator adds facts that a project inspector does not own: local checkout and tracking-ref freshness, worktree cleanliness, active plans and recent receipts, available gates, and loop leases and attempts. A later launcher may add launch policy. Final `launchability` is therefore deliberately absent from the provider report.

The provider report is also not a UI view model. Terminal and web presentations may reorder, filter, summarize, or join its data without requiring providers to emit colors, panel names, display limits, commands, or rendered text.

## Process contract

When an executable is invoked in status-provider mode, it must follow these rules:

- It performs read-only observation. It must not modify the repository, source inputs, or remote systems.
- It writes exactly one UTF-8 JSON document to standard output, with no banners, progress text, ANSI control sequences, or trailing documents.
- It writes logs and human diagnostics to standard error.
- Exit status `0` means standard output contains a trustworthy v1 report. This remains true when work packages have blockers or the report has `outcome: "partial"`.
- A nonzero exit status means the provider could not produce a trustworthy report. Consumers must not merge standard output from that invocation into current status.
- It treats cancellation or a closed output pipe as a request to stop promptly. The invoking runtime owns its timeout and process-tree cleanup.
- It does not place secrets, environment dumps, full source files, or unbounded command output in the report. Evidence is represented by stable references and optional digests.

This process behavior belongs to a dedicated provider mode. An existing verifier may retain different CI-oriented exit behavior for its human command.

## Jig runner and aggregate

Configure a provider with an exact argv array in `.jig.toml`:

```toml
[[status.providers]]
id = "example.vault.migration-readiness"
argv = ["ruby", "scripts/verify_migration_readiness.rb", "--status-provider-v1"]
timeout_seconds = 30
```

The configured `id` must exactly match `provider.id` in the report. Jig runs the argv directly from the repository root; it does not parse a shell command string. Arguments containing control characters are rejected so the renderer answer file remains safely round-trippable. Provider entries default to a 30-second timeout and accept values from 1 through 3,600 seconds. At most 32 providers may be configured.

The runner closes stdin, caps stdout at 8 MiB and stderr at 64 KiB, and owns the complete child process tree. Jig schedules at most four providers concurrently, retains configured result order, and stops queued providers from starting after cancellation; active providers receive the same cancellation through their owned-tree supervisor. Timeout, cancellation, capture failure, or process-tree cleanup failure invalidates that invocation. Jig also removes inherited Bash startup/option/trace controls, exported Bash functions, and all inherited `GIT_*` variables so ambient repository redirection cannot make the provider inspect another checkout. Other ordinary toolchain and project environment values remain inherited. Provider configuration is committed executable authority; review it like a project-owned script and never place credentials in argv.

Run:

```sh
scripts/jig status
scripts/jig status --json
scripts/jig status --tui
```

Human output is a compact operator summary. JSON has `schema_version: 1` and includes:

- `repository`: local HEAD, branch, worktree cleanliness, and ahead/behind counts against the existing local tracking ref;
- `work`: Jig's structured work-state summary plus a read-only gate snapshot for every open plan;
- `loops`: configured workflows, active leases, attempts, backoff, and exhausted attempts;
- `providers`: one result per configured provider, in configuration order;
- `errors`: failures collecting Jig-owned sections.

A successful provider result keeps the original JSON document under `providers[].report`, including additive fields unknown to the current Rust DTO. `summary` contains generic package, blocker, diagnostic, facet, and acceptance-check counts. `input_freshness` compares each Git input revision with the root checkout or its validated repository-relative path and reports `current`, `dirty`, `stale`, `unknown`, or `unavailable`; non-Git inputs are `not_applicable`.

Provider execution failures use `status: "failed"`, a stable error `code`, a bounded stderr diagnostic when one exists, and `report: null`. Nonzero stdout is never merged. A trustworthy provider report with `outcome: "partial"` uses provider `status: "partial"`.

Top-level `ok: true` means Jig constructed the inspection snapshot. Top-level `outcome: "partial"` means at least one provider failed or was partial, or a Jig-owned section could not be collected. A dirty input, stale revision, domain blocker, or blocked gate is a complete observed fact and does not by itself make collection partial.

`jig status` is read-only: it records no receipt, writes no cache, and never fetches a remote. Ahead/behind values therefore describe the current local tracking ref and may be older than the remote server.

## Terminal dashboard

`scripts/jig status --tui` is an interactive consumer of the same aggregate schema version 1 returned by `--json`. It does not call project-specific discovery code directly. This keeps private providers private and makes the dashboard usable with any conforming provider.

The Overview view shows repository cleanliness, local tracking-ref state, configured provider outcome and duration, normalized specification/implementation/verification/acceptance progress, source-input freshness, Jig plan/gate/loop state, provider diagnostics, and aggregate collection errors. Packages presents every reported package with its native facet states and a compact preview. Enter opens a scrollable package-detail screen containing bounded facet summaries, sources and digests, dependencies, acceptance checks, blockers, evidence, and generic namespaced package extensions. The terminal view marks oversized fields and collections when it truncates or omits them, preventing a valid but unusually large provider report from making navigation unresponsive. This lets private providers supply richer detail without coupling the TUI to their extension schema. Blockers flattens package blockers into a directly navigable queue with full detail.

Keys are `q` or Ctrl-C to quit; `r` to refresh; Tab or Shift-Tab and `1`/`2`/`3` to change views; `j`/`k`, arrows, PageUp/PageDown, Home, and End to move; `[` and `]` to switch providers; and `b` to show only blocked packages. On Packages, Enter opens detail. Within detail, `j`/`k`, arrows, PageUp/PageDown, Home, and End scroll; Escape, Enter, or Backspace returns to the package list. Escape quits from the normal dashboard. The first snapshot runs in a background worker. Refreshes never overlap, run every 30 seconds after the prior collection completes by default, and can be changed with `--refresh-seconds 1..3600`. A refresh requested while another is active is queued once.

Both stdin and stdout must be terminals. Use the human summary or `--json` in a pipeline. `--tui` and `--json` are mutually exclusive. Quitting while a provider runs sends cancellation through Jig's owned process-tree supervisor, joins the refresh worker, and restores raw mode, the original screen, and cursor visibility before returning.

The terminal dashboard is implemented in the CLI-owned `jig-status-tui` crate behind a snapshot-source interface. It is distinct from `scripts/jig ui`, which serves browser pages over plans, gates, receipts, and loop history. Neither status UI adds launch policy. The implemented Codex-home launcher remains deliberately separate in `jig-codex-tui`, with its own exact-path discovery and app-server inspection adapter; it does not make status observations executable by implication.

## Report envelope

Every report has these required fields:

- `protocol` is exactly `jig.status-provider/v1`.
- `provider.id` is a stable provider identifier. Prefer a reverse-domain or similarly collision-resistant name.
- `provider.adapter_version` identifies the provider implementation release and is independent of the protocol.
- `observed_at_ms` is the Unix timestamp in milliseconds when observation completed.
- `outcome` is `complete` or `partial`.

`inputs` records what the provider inspected. A Git input should include its exact revision. A path is included only when the input is inside the target checkout. A digest may identify non-Git specifications or configuration. Consumers use these identities to determine whether a cached observation is stale; the provider does not compare remotes or decide launchability.

`outcome: "partial"` means the emitted information is valid but one or more intended observations were unavailable. The provider should add an `error` diagnostic for each material gap. A blocked work package does not by itself make a report partial.

## Work-package observations

Each work package has a stable `id` and three independent facets:

- `specification` describes whether the implementation specification is ready.
- `implementation` describes implementation progress.
- `verification` describes executable verification progress.

A facet preserves the provider-native `state` and supplies one normalized `category`. Consumers display the native state and use the normalized value for generic grouping:

| Category | Meaning |
| --- | --- |
| `unknown` | The provider could not determine the state. |
| `pending` | Work has not reached a ready or active state. |
| `ready` | The facet is eligible to advance. |
| `active` | Work is currently underway. |
| `blocked` | The facet cannot advance. |
| `complete` | The facet completed successfully. |
| `failed` | The facet is invalid or completed unsuccessfully. |

Provider-native states may be more precise. For example, `ready_to_close` can remain the native verification state while its normalized category is `active`. Consumers must not infer provider-specific transitions from the normalized category alone.

Dependencies contain work-package ids. They may refer to packages absent from a partial report. Acceptance-check ordinals are one-based and unique within a work package. Blocker `code` values are stable machine identifiers; `message` is display text and may improve without changing the code. Evidence points to a test target, receipt, source location, or other stable reference rather than embedding its contents.

Provider-level `diagnostics` use `info`, `warning`, or `error`. Diagnostics describe observation quality or noteworthy conditions. Work-package `blockers` describe domain conditions that prevent that package from advancing. Consumers should not turn every warning or error diagnostic into a package blocker without an explicit policy.

## Paths and deterministic output

All `path` fields are relative to the target repository and use `/` separators. They must not be absolute, contain a drive prefix or backslash, include NUL, or contain empty, `.` or `..` components. Line and column values are one-based. The drive-prefix restriction remains part of the version 1 wire contract even though Jig hosts are limited to Linux and macOS; changing what existing v1 consumers accept requires a new protocol major.

Providers should emit deterministic arrays so reports are reviewable and cacheable: inputs by name, work packages by id, dependencies by id, acceptance checks by ordinal, and findings or evidence by stable code/reference. Consumers must not require physical JSON object-key order.

## Extensions and private implementations

The core schema and Rust DTOs are distributed under Jig's MIT license. A provider implementation remains under its own license. Private Rails discovery, compatibility heuristics, and customer-specific analysis may be translated into the public fields without publishing how they were calculated.

The report, provider, and work-package objects contain explicit `extensions` maps. Extension keys should use a collision-resistant namespace such as `example.rails-rewrite`. Extension contents may remain proprietary, but information required for generic status, blocker, dependency, and evidence rendering must also be represented in core fields. A consumer that requires a private extension is a specialized consumer, not a conforming generic one.

Consumers must ignore unknown fields in a supported major version. Unknown fields are tolerated for forward-compatible additions, but explicit extensions are preferred when provider-specific data must survive a deserialize/serialize round trip.

## Versioning

Consumers inspect `protocol` before deserializing a version-specific report. Version 1 allows additive optional fields and additional extension data. These changes require a new major protocol version:

- removing or renaming a field;
- making an optional field required;
- changing a field's type or meaning;
- adding a fixed enum value that the v1 schema would reject;
- changing process, path, exit-status, or normalized-category semantics incompatibly.

Provider releases change `adapter_version`, not `protocol`. A provider may support multiple major protocols through separate modes during a migration.

## Conformance

Run the owning checks from the Jig repository root:

```sh
cargo test -p jig-contract
cargo clippy -p jig-contract --all-targets -- -D warnings
```

The tests validate the committed example with an offline JSON Schema validator, deserialize it through the public Rust type, run semantic validation, prove compatible unknown fields remain accepted, reject another protocol major, and compare the committed schema with the schema generated from Rust.

To inspect regenerated schema output:

```sh
cargo run -q -p jig-contract --example status_provider_schema
```

Schema changes require compatibility review before replacing the committed artifact.
