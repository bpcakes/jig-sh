# 05 — Validate useful guide references without mandatory heading templates

## Task identity

- Local task: 05.
- Beads issue: `jig-sh-9wcn.5`.
- Priority: P2.
- Parent: `jig-sh-9wcn`, Astra harness modernization.
- Depends on: `jig-sh-9wcn.4`
- Unblocks: 08, 15.
- Status: implemented; see [acceptance evidence](05-implementation.md).

## Context and outcome

Exact headings and literal entrypoint references mostly enforce document shape. Link and
ownership validation better protect agents from misleading guidance while allowing short
useful local guides.

This is work in the jig-sh Rust CLI and repository-template project.
Read the root AGENTS.md and the nearest existing guide before changing code.
The plan baseline is Git commit 03e9a9e4e5122b5bc12c66b1f635ae1faac05e15.
Resolve task-local facts against the actual worktree before implementation.

## Relevant entrypoints

- `crates/jig/src/policy/agent_map.rs`
- `crates/jig/src/agent_guides.rs`
- `crates/jig/src/runtime/tests/`
- `templates/project/AGENTS.md.jinja`
- `docs/public-contract.md`

## Scope

- Make exact five-heading conventions advisory for newly generated guidance.
- Validate repository-relative links and explicitly declared owner-guide references.
- Do not require placeholder AGENTS.md files for every crate or package.
- Keep existing JSON result fields readable and define any added diagnostics explicitly.
- Separate warnings about guide structure from blocking broken ownership references.
- Use existing path validation and symlink boundaries.
- Support both Rust and Go guide ownership using the authored component model.
- Preserve compatibility for existing clients consuming guide-check output.

## Implementation sequence

1. Inspect current guide discovery and required-heading tests.
2. Define stable warning/error categories and document their severity.
3. Reuse existing Markdown/path handling when available; avoid introducing a broad parser
   dependency without need.
4. Implement meaningful reference validation for repository-local guide links.
5. Treat external links as unverified references rather than making network requests during
   checks.
6. Keep unknown guide styles valid if the concrete reference checks pass.
7. Update generated conventions and fixtures for concise guides.
8. Document legacy field behavior and new diagnostic interpretation.

## Acceptance criteria

- A short valid guide with different headings passes.
- A declared owner guide pointing to a missing file is reported precisely.
- A malformed or escaping local reference is rejected or reported without filesystem
  traversal.
- Missing optional guides do not fail the repository.
- Rust and Go fixtures exercise the same semantic validation contract.
- JSON consumers retain existing required fields through a compatible transition.
- Checks do not depend on internet access.
- Warning-only style differences cannot block work finish indirectly.

## Verification

- Cover valid concise guides, broken links, fragments, relative paths, and missing optional
  guides.
- Exercise symlink and traversal cases using existing fixture conventions.
- Validate legacy-compatible response shapes.
- Run relevant policy tests and the required backend test gate.

Run commands from the repository root.
For runtime changes, build with `cargo build -p jig-sh --bin jig`.
Select that binary with `JIG_DEV_BIN=target/debug/jig` for harness commands.
Run change-appropriate checks and all applicable repository-required gates.
Record observed results, including failures and tests not run.

## Compatibility and boundaries

- Markdown fragments and code examples must not be mistaken for file references.
- A semantic validator must not pretend to judge whether prose is correct.
- Avoid introducing a new mandatory policy gate merely to replace an old one.
- Preserve unrelated worktree changes and unmanaged generated-file sections.
- Keep .agent/state JSONL history append-only; use explicit migrations for new persisted formats.
- Use generic fixtures such as ExampleProject and ExampleVault.
- This task does not authorize unrelated account setup, publishing, or external messages.

## Completion and handoff

Attach implementation revision, relevant result artifacts, and acceptance disposition to the bead.
Close only after the task's stated outcome is delivered; a plan or expected test result is not proof.
Leave an actionable restart checkpoint if interrupted.
Run `python3 scripts/beads-sync.py` after Beads mutations, using the canonical main-checkout database.
See README.md in this plan directory for shared rollout policy and the dependency matrix.
