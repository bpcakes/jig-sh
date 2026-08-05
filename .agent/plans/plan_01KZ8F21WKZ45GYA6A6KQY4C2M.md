## Progress

- [x] Centralize root command metadata and render categorized root help.
- [ ] Add canonical SQLx namespace with compatibility shims.
- [ ] Consolidate SQLx CLI, DTO, and runtime modules.
- [ ] Run the full test suite before each of the three requested commits. First pre-commit run passed as receipt receipt_01KZ8G23Q29WJ731JXF9SS3NB1.

## Surprises & Discoveries

- The ambient shell umask is 0002, which makes tempfile roots mode 775 and intentionally fails the dev-proxy security tests. Full-suite runs use an owner-only umask of 0077; the unchanged configured command then passes.

## Decision Log

- Preserve existing migration-add and schema-dump invocations while adding the canonical namespace.
- Keep SQLx filesystem mutation and process execution in jig-sh per the jig-sqlx crate invariant.

## Outcomes & Retrospective

- Pending.

## Context and orientation

The root Clap command tree is defined in crates/jig/src/cli.rs. Repository-specific command inventory lives in crates/jig/src/info/commands.rs. SQLx contract metadata lives in crates/jig-sqlx, while execution remains in crates/jig.

## Plan of work

Implement the three reviewed issues as three independently tested commits. First introduce shared typed metadata for root command names, order, and categories and make root help expose those categories. Second add jig sqlx migration add and jig sqlx schema dump while retaining legacy root commands as compatibility shims and update generated documentation and tests. Third move SQLx parser, request, conversion, and runtime dispatch ownership into family modules without changing behavior.

## Concrete steps

After each implementation step, build the development jig binary, run JIG_DEV_BIN=target/debug/jig scripts/jig check test, inspect the diff and receipts, and commit only that step.

## Validation and acceptance

All legacy commands continue to parse. Canonical SQLx commands dispatch identically. Root help is categorized from shared metadata. Production SQLx command definitions and dispatch live in coherent family modules. The full configured test suite passes before every commit.

## Idempotence and recovery

Each step is a separate Git commit and can be reverted independently. Append-only state files are never rewritten.

## Interfaces and dependencies

Preserve stable MCP tool names, generated manifest tool names, JSON response shapes, and legacy CLI paths.
