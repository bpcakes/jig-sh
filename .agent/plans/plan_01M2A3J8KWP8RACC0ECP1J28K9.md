# Task 05: semantic guide validation

Implement `jig-sh-9wcn.5` on Git baseline `dc00cc215933716cf5367261a23710913fe00ba7`,
preserving the uncommitted task-04 implementation. Work ID:
`plan_01M2A3J8KWP8RACC0ECP1J28K9`.

## Progress

- [x] Verify task 04 is closed; claim task 05 and inspect current discovery/output paths.
- [x] Identify `ComponentSpec.guidance` as the existing authored owner-guide field.
- [x] Implement safe local reference validation and advisory style diagnostics.
- [x] Update generated guidance, public contract, and existing dependent assertions.
- [x] Verify parser/path/ownership/legacy output/CLI/work-finish behavior.
- [x] Complete configured gates, task records, and structured work.

Restart checkpoint: complete. Beads and structured work closed after all six required targets passed with fresh evidence. Existing task-04 changes are the starting
worktree and must remain. An accidentally generated Python bytecode cache from that work
was removed before validation. No blocked prerequisite.

## Surprises & Discoveries

The current guide checker selects no guides in this root-owned workspace, enforces five
literal headings and language-specific backticks, and never reads component `guidance`.
The map's substring parser counts code examples as links and cannot handle CommonMark
reference links. Neither is sufficient for the requested semantic validation.

Tracker exports and documentation are source inputs under this repository's current
freshness policy. Finish task bookkeeping before the final evidence refresh. Record
ongoing results here under `.agent/`, whose memory is excluded from source identity.

## Decision Log

2026-09-12: Validate existing AGENTS.md files (including root and nested guides) plus
explicit component.guidance paths. Keep optional guides optional. Authored guidance is
repository-relative; ordinary Markdown destinations resolve from the guide directory,
with `/` anchoring at repository root. Use the authored component IDs in owner diagnostics.

2026-09-12: Preserve legacy JSON fields/types. With no required headings or literal
entrypoint syntax, missing_sections/missing_entry_ref remain empty compatibility arrays.
Emit advisory style warnings and blocking reference failures through typed diagnostics;
only errors affect `ok` and exit status. Document this runtime-owned additive evolution.

2026-09-12: Use pulldown-cmark 0.13.4 with default features disabled. Its offset events
provide proper code-span/fence, reference-link, escaping and source-line handling. The
existing substring parser cannot satisfy these requirements; a narrow parser dependency
is preferable to inventing another incomplete Markdown dialect. Reuse repository path
normalization and existing capability/no-follow filesystem primitives. No HTTP requests
run during validation; external links are explicitly unverified information.

## Outcomes & Retrospective

Implementation and final verification complete. Beads and structured work are closed; all six required targets passed with fresh evidence. No commits or publishing performed.

## Implementation and acceptance

Keep discovery/map generation in policy/agent_map.rs; put reference checking in a focused
policy module and reusable Markdown/path helpers under agent_guides. Avoid exceeding the
800-line source budget. Reads must reject symlink guides and ancestors; local destinations
must never cause traversal outside the repository, including encoded or portable-path
tricks. Ignore fragments as filesystem paths and code examples as links. Missing links,
malformed paths, directories used as owner guides and explicit missing owners are errors.

Tests must cover concise Rust/Go guides and optional absence; local files/directories,
fragments, encoded names, balanced parentheses, reference links, code blocks/spans and
external references; symlink leaf/ancestor and traversal; deterministic typed JSON and
human diagnostics; a warnings-only policy check through an actual work gate/finish.
Render templates and preserve unmanaged guide updates. Run focused tests before the
configured six-target work check with a freshly built JIG_DEV_BIN. Do not change the
verification policy. No publishing or commits are requested.

## Verification log

- Initial current-source dev build succeeded before edits.

- First focused runs exposed missing required fields in the new v8 fixture, not a
  production failure; corrected fixture profile/source metadata. Map traversal error
  wording was aligned with existing diagnostics. The CLI tests already pass, including
  a warnings-only required work gate and successful work finish, before the remaining
  v8 fixture tests are rerun.
- Shared CommonMark extraction also reports undefined explicit reference labels while
  leaving ordinary bracketed prose alone. Map validation now uses the same contained
  path and no-follow target checks; map JSON field shapes remain unchanged.
- Defined a tracked-but-deleted optional guide as absent, not a new placeholder
  requirement. Explicitly declared missing owner guides still fail with component IDs.

- Focused nextest selection passed all 26 policy, bootstrap guide-root and CLI tests.
- Development binary rebuilt successfully. Actual repository check passes with 19 guides,
  no errors and one advisory root-guide structure warning.
- Full six-target configured run started: `run_01M2AAYM6CY9VGKH13EM1E9Y46`.
  Read-only gate inspection hit its default 2-second budget while compilation was active;
  this is not a failed test or proof of stale evidence. Inspect with 30 seconds after completion.

- Full configured run `run_01M2AAYM6CY9VGKH13EM1E9Y46` passed all six targets,
  including 4,098 Rust tests (three configured skips; two reported leaky) and 38 harness tests.
  Acceptance is recorded in docs/plans/astra-harness-modernization/05-implementation.md.

- Bead closed and sanitized export completed after the passing implementation run.
- Final refresh: `run_01M2ABF48AHXGFR4PG3BPGDHN8`. Source and tracker inputs are frozen.
  On resume, observe this run before executing anything again.
- Final guide check: 19 guides, zero errors, one warning. Agent map: 19 guides, no
  missing guides or broken links. Git whitespace and Beads privacy checks passed.

- Final run `run_01M2ABF48AHXGFR4PG3BPGDHN8` passed all six required targets: 4,098
  Rust tests, three configured skips, no leaky reports in this run, and 38 harness tests.
- Read-only 30-second inspection confirmed every target passed and fresh. Work finish
  succeeded after revalidation. No required work remains for task 05.
