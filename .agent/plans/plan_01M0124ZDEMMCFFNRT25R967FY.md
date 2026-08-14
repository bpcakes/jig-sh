# Harden Vault TUI interaction design

This plan addresses three review findings in the Vault TUI without broadening the
plaintext or lifecycle surface. The result should make platform-specific command
availability explicit, keep operation feedback visible while filtering, and make
Quick Access search work proportional to query changes rather than rendering and
selection reads. Each coherent refactoring or behavior change is committed
separately.

## Progress

- [x] Read the repository and crate guides plus the Fowler Rust refactoring
  principles and catalogs.
- [x] Establish the targeted formatting, test, and Clippy baseline.
- [x] Run the Fowler heuristic scanner against `master...HEAD` and validate its
  relevant candidates manually.
- [ ] Split command state eligibility from target-platform capability without
  changing existing behavior.
- [ ] Disable private-output commands on unsupported targets and document the
  limitation.
- [ ] Render browser filter state and operation feedback independently.
- [ ] Introduce prepared fuzzy-search text without changing match ordering.
- [ ] Move Quick Access terms and ranked indices into one cache-owning type.
- [ ] Run the configured repository gates, review the final diff and commit
  series, and close the structured work.

## Surprises & Discoveries

- The first configured `scripts/jig work check` run lost the local `target/`
  directory during a highly parallel Nextest invocation, after hundreds of tests
  had passed; the remaining unrelated test binaries then failed immediately.
  This happened before source edits. Targeted crate tests remain the per-slice
  behavior oracle, and the configured gate will be retried after implementation.
- A targeted baseline of `jig-vault` had one timing-sensitive failure in
  `brokered_run_repeatedly_cleans_an_immediate_background_wrapper` because a
  250 ms output-drain deadline expired under load. The exact isolated rerun
  passed. This test and process supervision code are outside this plan.
- The scanner reported the large `UiCommand::availability` match and the exposed
  Quick Access fields, but it did not identify the actual footer defect. File
  length and exhaustive enum matches are not treated as findings by themselves.

## Decision Log

- Keep `UiCommand` as a closed enum. Exhaustive matches are appropriate; no trait
  or dynamic dispatch is introduced.
- Apply Fowler's **Split Phase** and **Decompose Conditional** to command
  availability: state eligibility is evaluated first, then an explicit target
  capability requirement. A transitional capability value preserves existing
  behavior before the bug-fix commit changes it.
- Treat footer feedback as a local omission. Change its row accounting and
  independent rendering directly instead of extracting a new presentation
  framework.
- Apply **Replace Primitive with Object** to fuzzy normalization through a shared
  prepared-text value, then **Encapsulate Collection** and **Move Field** so Quick
  Access owns search terms, cached rankings, selection, and invalidation together.
- Preserve public fuzzy-score ordering and the existing `fuzzy_match_score`
  function as a compatibility wrapper. No existing public API is removed.

## Outcomes & Retrospective

To be completed after all checks pass. Record the final commit series, any
remaining risks, and whether the configured gates succeeded on retry.

## Context and orientation

`crates/jig-vault-tui/src/commands.rs` owns the closed command catalog and its
availability policy. It currently groups Export with portable Peek, and groups
1Password Import and Backup with portable passphrase rotation, even though the
core private-output facility is Unix-only.

`crates/jig-vault-tui/src/render.rs` owns Ratatui layout. The browser footer uses
an `if/else if` between filter and status lines, so a retained filter suppresses
operation results. The model intentionally retains filters across backend work.

`crates/jig-vault-tui/src/quick_access.rs` owns the metadata-only picker. It
currently allocates candidate strings, normalizes the same query repeatedly,
sorts every target, and returns a fresh index vector from read-only methods.
Rendering asks for that derived vector more than once per frame.

`crates/jig-tui/src/lib.rs` owns generic fuzzy scoring. It remains the sole source
of match semantics; the Vault TUI must not copy the algorithm.

## Plan of work

First introduce a private target-capability value and split `UiCommand`
availability into state and platform phases. Set the transitional private-output
capability to supported everywhere so this commit is behavior-preserving. Verify
all Vault TUI tests and commit.

Next change the current target capability to report private output only on Unix.
Add platform-independent tests using injected capabilities, retain state-error
precedence, update CLI help and operator documentation, verify, and commit.

Then change browser footer height calculation and render filter and status as
separate lines. Add render regressions for both error and informational feedback
under a retained filter, verify, and commit.

Introduce a normalized fuzzy-search text type in `jig-tui`. Make the existing
function delegate to the prepared implementation and add equivalence tests. This
commit must not change ordering, Unicode case folding, or the existing public
function. Verify all three TUI consumers and commit.

Finally replace Quick Access's public target vector and recomputed index query
with entries that own prepared terms and one cached ranked-index vector. Rebuild
rankings only when query text changes; cursor movement, rendering, selection, and
viewport bookkeeping must borrow cached state. Keep metadata-only rendering and
exact selection identity unchanged. Verify and commit.

## Concrete steps

1. Edit `crates/jig-vault-tui/src/commands.rs`; run
   `cargo test -p jig-vault-tui` and its strict Clippy command; commit the
   preparatory refactor.
2. Add capability regressions in `crates/jig-vault-tui/src/tests.rs`; update
   `crates/jig/src/cli/vault.rs` and `docs/configuration.md`; rerun tests, help
   tests where relevant, and Clippy; commit the behavior fix.
3. Edit `crates/jig-vault-tui/src/render.rs` and focused render tests; rerun the
   Vault TUI suite and Clippy; commit.
4. Edit `crates/jig-tui/src/lib.rs`; run `cargo test -p jig-tui` plus the Codex,
   status, and Vault TUI suites; commit.
5. Edit `crates/jig-vault-tui/src/quick_access.rs`, its rendering callers, and
   focused tests; run the Vault TUI suite and Clippy; commit.
6. Build `jig-sh`, force `JIG_DEV_BIN` to the new binary, run configured work
   evidence/gates, run formatting and relevant direct checks, inspect
   `master...HEAD`, then close the plan and session.

## Validation and acceptance

Acceptance requires all of the following:

- unsupported private-output commands are disabled before a user can open their
  forms, while Peek and passphrase rotation remain available;
- domain-state reasons still take precedence when a command is unavailable for a
  selected object or locked/legacy vault;
- a browser frame can show a retained filter and an operation error or success at
  the same time;
- prepared fuzzy matching produces exactly the same score ordering as the
  compatibility function;
- Quick Access rebuilds ranking only on text changes and preserves exact target
  selection, ordering, compact scrolling, and metadata-only frames;
- `cargo fmt --all -- --check`, relevant strict Clippy checks, Vault/TUI unit
  tests, PTY lifecycle tests, and configured Jig gates pass, apart from any
  independently reproduced pre-existing environmental failure documented here.

## Idempotence and recovery

Every source slice ends in a green commit and can be reverted independently.
The command capability refactor deliberately preserves old behavior so the
platform fix can be reverted without undoing the clearer phase boundary. The
existing fuzzy function remains as an adapter, so Quick Access can be reverted
without affecting other TUI callers. No vault file, audit format, public command
syntax, secret value, or persistent encrypted state is migrated.

## Interfaces and dependencies

No new dependency is added. `jig-tui` gains an additive prepared fuzzy-text API;
its existing scorer remains source-compatible. All command-capability types stay
inside `jig-vault-tui`. There are no unsafe, FFI, serialization, async, database,
or protocol changes. Platform behavior changes only for TUI command availability
on non-Unix targets, matching the existing core rejection contract.
