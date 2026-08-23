# Remove unsupported Windows implementation surface

Jig supports Linux and macOS hosts. This change removes native-Windows implementations and support-shaped artifacts so the source tree expresses that contract directly instead of preserving an untested third platform behind conditional compilation.

## Progress

- [x] (2026-08-22) Inventory Windows-specific source files, conditional branches, dependencies, tests, templates, and documentation.
- [x] (2026-08-22) Remove Windows process-supervision and runtime implementations from the owning crates.
- [x] (2026-08-22) Remove Windows bootstrap, launcher, shell, generated-template, and path-semantics behavior.
- [x] (2026-08-22) Reduce platform guidance to the supported Linux/macOS contract and remove support-shaped Windows details.
- [x] (2026-08-22) Run formatting, targeted checks, contract checks, and the full workspace test suite.
- [x] (2026-08-22) Inspect the final diff, record evidence, and close the work item.

## Surprises & Discoveries

- The repository policy already declared native Windows unsupported, but several crates still carried complete Windows process, terminal, filesystem, certificate, shell, and bootstrap implementations plus direct `windows-sys` dependencies. Those implementations materially enlarged the maintenance surface despite having no CI contract.
- Some dependency-lock entries are target metadata of otherwise portable third-party crates. They are not Jig implementations and Cargo may retain them after every direct Jig dependency is removed.
- The first full-suite run exposed a residual pair of drive/share pnpm fixtures plus NTFS Git metadata alias handling and a drive-prefix status-contract rule that a simple platform-token sweep did not reveal. Removing those semantics fixed the failing fixture at the abstraction boundary instead of weakening its assertion.

## Decision Log

- Decision: Model the host contract positively as Linux and macOS, with a generic unsupported-platform failure where a compile-time or runtime fallback is required.
  Rationale: A positive supported-platform boundary avoids recreating a Windows compatibility layer under a differently named fallback.
- Decision: Retain platform-neutral validation of repository data only when it protects a format independent of Jig host execution; remove tests, messages, and comments framed as Windows compatibility promises.
  Rationale: Portable data invariants and host support are separate concerns, but the latter must not leak back into the implementation.
- Decision: Do not edit transitive Windows packages out of `Cargo.lock` by hand.
  Rationale: Cargo owns the lock graph; target-specific metadata from portable dependencies does not advertise Jig host support.

## Outcomes & Retrospective

Jig-owned native-Windows process supervision, vault/process/file backends, proxy runtime behavior, CLI and bootstrap branches, generated launcher behavior, dependencies, tests, fixtures, and documentation were removed in coherent commits. Platform selection now names Linux and macOS positively, with only generic unsupported-host failures where a fallback is required. Repository path safety retains containment, canonical separator, and macOS HFS protections without modeling drive prefixes or NTFS aliases.

Targeted crate and regression tests passed. The authoritative `scripts/jig check test` run passed all 2,150 selected tests, and a second plan-linked `work check` run passed the same 2,150 tests plus the contract check. Required contract and test gates are fresh and passed under batch receipt `receipt_01M0NCCYXTFH90W62PTKGK7408`; formatting and workspace-wide Clippy also passed under receipts `receipt_01M0NCEDS1WD74X4BKW4858TE2` and `receipt_01M0NCF1VDYN9TNMATZXDVTRP6`.

## Context and orientation

The main platform branches live in `crates/jig-owned-process`, `crates/jig-dev-proxy`, `crates/jig-vault`, `crates/jig-vault-tui`, and `crates/jig`. Generated launcher behavior is owned by `templates/project/scripts` and mirrored into bootstrap snapshots under `crates/jig/src/bootstrap/embedded_template_snapshots`. Repository guidance lives in `AGENTS.md`, crate-level `AGENTS.md` files, `README.md`, `CHANGELOG.md`, and `docs`.

The supported Unix implementations are not being generalized to every Unix target. Linux and macOS remain the only supported hosts; other targets must either fail to compile at an explicit boundary or return an unsupported-platform error without platform-specific behavior.

## Plan of work

First remove the deepest process/runtime implementations and their direct dependencies. Then remove caller-side Windows branches, tests, launchers, and generated shell behavior. Finally rewrite guidance and documentation so no retained prose describes native-Windows operation. Keep each coherent slice in its own commit.

## Concrete steps

1. Remove Windows modules, cfg branches, target dependencies, and tests from owned-process, dev-proxy, vault, vault-tui, and the CLI.
2. Simplify platform dispatch to Linux/macOS implementations plus generic unsupported fallbacks where compilation requires them.
3. Remove Git Bash, PowerShell, Windows executable, path, dependency-checker, and runner-selection support logic from templates and bootstrap tests/snapshots.
4. Remove Windows-specific invariants and configuration details from repository guidance and docs; state only the supported host set and generic unsupported-host policy.
5. Run `cargo fmt --all`, regenerate the lockfile through Cargo, build the development binary, and validate via `JIG_DEV_BIN=target/debug/jig scripts/jig work check`, configured gates, and `scripts/jig check test`.

## Validation and acceptance

Acceptance requires no Jig-owned Windows implementation modules, direct `windows-sys` dependency, Windows-only test/spec files, PowerShell/Git-Bash support paths, or documentation that describes native-Windows behavior. `rg` may still find Cargo-owned transitive package names or generic prose saying only Linux/macOS are supported. All configured gates and the full workspace test suite must pass.

## Idempotence and recovery

File deletions and conditional-branch simplifications are ordinary Git changes and can be retried. Do not rewrite append-only `.agent/state/*.jsonl`; if a verification command fails, fix the source and append a new receipt by rerunning it.

## Interfaces and dependencies

No persisted state or public data format changes. The host-platform interface narrows from an untested implementation-shaped surface to the documented Linux/macOS contract. Direct `windows-sys` dependencies are removed; third-party lockfile target metadata remains Cargo-managed.
