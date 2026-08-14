# Polish the Vault TUI interaction model

Implement context-aware commands, explicit item creation, full metadata line
editing, and metadata-only Quick Access search in four independently green
commits. The observable result is that a first-time operator can discover every
valid action from the current selection, create a new item without knowing the
field-derived storage model, edit ordinary form text with normal terminal
controls, and find any item or field quickly without exposing a value.

## Progress

- [x] Audit the current Vault, Codex-picker, Status, and shared TUI interaction
  patterns and research comparable keyboard-first interfaces.
- [x] Open structured work `plan_01M00SZ5VVZGTY6JGSN8VTKQ9V` and record the
  four delivery boundaries.
- [ ] Introduce a total command catalog, contextual and universal palettes, and
  catalog-derived action help/footer rendering.
- [ ] Add an explicit Create item + first field flow, contextual empty states,
  and Enter-driven actions.
- [ ] Add a metadata-only line editor with cursor movement, deletion, word
  editing, bounded insertion, visible cursor, and horizontal viewport behavior.
- [ ] Add metadata-only Quick Access with shared fuzzy ranking, result preview,
  scrolling, and action handoff.
- [ ] Build the development Jig binary, run configured gates, inspect evidence,
  update this plan, close structured work, and push the commits to PR #4.

## Surprises & Discoveries

- Canonical items are projections of canonical fields, not independently
  persisted records. A first-class item flow must therefore say "Create item +
  first field" and reuse atomic field creation rather than invent empty items or
  change the vault format.
- The browser footer and help are handwritten separately from runtime key
  dispatch. That drift surface explains why actions are numerous but difficult
  to discover. One closed command catalog should own labels, bindings,
  availability, and activation identity.
- Metadata inputs are append/backspace-only `String` values while the terminal
  cursor remains hidden. Protected input must remain a distinct zeroizing type;
  a reusable ordinary-text editor must never absorb passphrases or field values.
- `jig-codex-tui` already has ranked fuzzy subsequence matching and viewport
  behavior. Quick Access should move only the feature-neutral matcher into
  `jig-tui`, preserve Codex-picker behavior with characterization tests, and keep
  Vault result records metadata-only.

## Decision Log

- Keep the existing responsive Items / Fields / Details explorer. Add action and
  search overlays instead of replacing the proven browsing hierarchy.
- Use a closed `UiCommand` enum rather than a dynamic callback registry. Each
  variant owns its label, binding, safety class, availability, and exhaustive
  activation path, so additions fail to compile until fully described.
- `Enter` opens actions for the current selection; `:` opens the searchable
  universal palette. Navigation bindings remain outside the mutation command
  catalog but action help and footer content come from the catalog.
- Preserve all current direct shortcuts. Exact modifier matching is centralized
  so Ctrl/Alt variants cannot accidentally trigger ordinary commands.
- Keep `SecretInput` isolated and unchanged in responsibility. The new line
  editor is metadata-only, bounded by the existing per-surface limits, and never
  formats or stores secret bytes.
- Quick Access searches item, field, reference, kind, and legacy metadata. It
  never decrypts, previews, copies, or returns a value. Activating a result opens
  the same contextual action palette used by the browser.
- Do not add dependencies, clipboard/OSC52 behavior, persisted recency, a vault
  format change, or cross-scope navigation.

## Outcomes & Retrospective

Work is in progress. At completion this section will record the four commit
boundaries, behavior and compatibility effects, focused and full verification
receipts, and any intentionally deferred polish.

## Context and Orientation

`crates/jig-vault-tui/src/model.rs` owns screens, selection, and forms;
`runtime.rs` maps terminal events; `render.rs` owns wide/compact rendering;
`tools.rs` owns lifecycle forms; and `secret_input.rs` owns protected bytes.
New feature-specific command and line-editing models belong in this crate.
`crates/jig-codex-tui/src/model.rs` contains the existing fuzzy matcher, while
`crates/jig-tui/src/lib.rs` is the permitted home for a small feature-neutral
matching helper shared by two TUIs.

The hard boundary is unchanged: ordinary models, frames, errors, debug output,
and action results contain authenticated metadata only. Peek remains the sole
bounded terminal reveal and bypasses Ratatui. One worker, fixed scope, explicit
and idle lock, authentication/audit fail-closed behavior, exact identities, and
atomic mutation preconditions remain compatibility-sensitive.

## Plan of Work

1. Add `commands.rs` with `UiCommand`, exact bindings, availability and safety
   metadata, contextual command selection, and palette state. Route browser
   action keys through it, render action rows and disabled reasons, and generate
   action help/footer text from the same catalog. Commit after focused tests.
2. Add `CreateItem` to the catalog and an explicit field-write intent that opens
   blank item and field inputs focused on Item. Add empty-item guidance and tests
   proving the resulting action is an atomic create under the new item. Commit.
3. Add a non-secret line-editor value and migrate every metadata/search/palette
   input from raw `String`. Support insertion at cursor, Backspace/Delete,
   Left/Right, Home/End, Ctrl-W and word movement, bounded atomic paste, and
   horizontally windowed cursor rendering. Keep protected input separate. Commit.
4. Move the existing feature-neutral fuzzy score helper into `jig-tui`, migrate
   the Codex picker without behavior change, then add a Quick Access overlay with
   ranked exact identities, safe metadata preview, navigation and contextual
   action handoff. Commit.
5. Build `jig-sh`, run the development binary through structured checks and
   gates, record receipts and outcomes, close the plan, and push the branch.

## Concrete Steps

For every slice, use `apply_patch`, run `cargo fmt --all -- --check`, the narrow
owning-crate tests, strict all-target Clippy for every changed crate, inspect
`git diff --check` and the staged diff, then create one non-interactive commit.
After all slices:

    cargo build -p jig-sh --bin jig
    JIG_DEV_BIN=target/debug/jig scripts/jig work check --plan-id plan_01M00SZ5VVZGTY6JGSN8VTKQ9V
    JIG_DEV_BIN=target/debug/jig scripts/jig work gates --plan-id plan_01M00SZ5VVZGTY6JGSN8VTKQ9V
    JIG_DEV_BIN=target/debug/jig scripts/jig check fmt
    JIG_DEV_BIN=target/debug/jig scripts/jig check clippy
    JIG_DEV_BIN=target/debug/jig scripts/jig check test
    JIG_DEV_BIN=target/debug/jig scripts/jig work evidence --plan-id plan_01M00SZ5VVZGTY6JGSN8VTKQ9V

## Validation and Acceptance

- `Enter` on Items, Fields, or Details opens a context menu containing only
  relevant commands; unavailable but relevant actions show a safe reason.
- `:` opens a type-to-filter catalog of every command applicable to the current
  vault state. Existing direct action shortcuts retain their behavior.
- Footer action hints and help action rows are derived from command metadata and
  cannot silently drift from dispatch.
- `I` opens "Create item + first field" with a blank Item field focused; saving
  creates the first field and selection moves to its exact reference.
- Empty vaults explain that an item begins with its first field.
- Every ordinary text surface has a visible cursor and supports conventional
  editing without exceeding its byte limit. Secret inputs remain redacted,
  bounded, zeroizing, and outside the ordinary editor type.
- Ctrl-P Quick Access fuzzy-searches safe metadata, keeps selection visible,
  shows no values, and hands the exact selected identity to contextual actions.
- Wide, compact, minimum-size, sentinel/no-plaintext, PTY lifecycle, formatting,
  Clippy, contract, and complete configured tests pass.

## Idempotence and Recovery

Each slice is a green revert boundary. Structural additions precede behavior
changes, no migration or external state is involved, and tests use synthetic
metadata or disposable vault homes. Append-only `.agent/state/*.jsonl` files
must never be rewritten or truncated. If a later slice fails, retain earlier
green commits and continue with a follow-up rather than destructive reset.

## Interfaces and Dependencies

No public vault/domain interface or dependency change is planned. New UI types
are crate-private. The only shared addition is a deterministic metadata string
fuzzy-score helper in `jig-tui`; existing Codex-picker behavior must remain
source-tested while Vault Quick Access consumes it. No plaintext-bearing type
may implement conversion into the new editor or search models.
