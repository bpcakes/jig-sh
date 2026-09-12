# Task 05 implementation and acceptance

Implemented in the working tree on baseline
`dc00cc215933716cf5367261a23710913fe00ba7`, preserving task 04 changes.
Bead: `jig-sh-9wcn.5`. Work record: `plan_01M2A3J8KWP8RACC0ECP1J28K9`.
No implementation commit or external model evaluation is claimed.

Guide checks inspect existing root/nested `AGENTS.md` files and authored component
`guidance` file paths. Concise or differently organized guides pass; the five headings
produce advice only. Missing optional guides remain optional. Broken local links and
missing explicit owner guides fail with structured locations and component identity.

CommonMark parsing handles links, images, definitions, titles, escaping and parentheses
without treating code examples as links. Local paths use existing portable-path and
capability filesystem primitives, reject escaping paths and symlinks, and bound guide
reads to 1 MiB. External links remain unverified without network access. Agent-map checks
share the parser and contained target validation. A narrow pulldown-cmark dependency
replaces the previous substring scanner; no manifest or MCP schema changes were needed.

The six previous JSON fields retain their types. The obsolete structural-failure arrays
remain empty; additive diagnostics distinguish errors, warnings and information.
[The public contract](../../public-contract.md) documents diagnostic codes, nullable
fields, strict-decoder projection, exit behavior and validation limits. Human diagnostics
are sanitized. Generated conventions and embedded snapshots describe optional headings.

| Acceptance | Evidence |
| --- | --- |
| Concise guides, optional absence, Rust and Go ownership | Policy fixtures for both adapters; missing and valid explicit owner files |
| Malformed, escaping and unsafe references | Encoded traversal, invalid encoding, symlink leaf/ancestor, unreadable and oversized guide fixtures |
| Markdown semantics and offline behavior | Inline/reference/image links, fragments, code/HTML examples and external-reference diagnostics |
| Compatible result fields | Legacy epochs 2–5 plus current authored-model assertions; documented strict projection |
| Warnings cannot block work finish | Actual CLI integration opens work, executes a required warnings-only guide gate, and finishes successfully |
| Generated consumers remain coherent | Bootstrap adoption/update tests, renderer snapshots and full workspace suite |

The focused selection passed 26 tests. The freshly built dev binary checks this repository's
19 guides successfully with one advisory warning and no errors. Full configured run
`run_01M2AAYM6CY9VGKH13EM1E9Y46` passed all six targets: Clippy, formatting,
4,098 Rust tests (three configured skips; two reported leaky), contract validation,
file budgets, and 38 harness tests. Initial fixture failures were corrected before this pass.

Task bookkeeping changes source identity under the current policy. The work record's
append-only receipts and narrative record the final evidence refresh and closure after
those changes; no verification gate or freshness policy was relaxed.
