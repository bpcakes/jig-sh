// Generated from templates/project. Update with JIG_REFRESH_EMBEDDED_TEMPLATE_SNAPSHOT=1 cargo check -p jig-sh.
#[cfg(test)]
#[allow(dead_code)]
pub(super) const EMBEDDED_TEMPLATE_FILES_FROM_SNAPSHOT: bool = true;
pub(super) static EMBEDDED_TEMPLATE_FILES: &[EmbeddedTemplateFile] = &[
    EmbeddedTemplateFile { relative_path: ".agent/.cache/.gitignore.jinja", contents: r#"*
!.gitignore
"# },
    EmbeddedTemplateFile { relative_path: ".agent/PLANS.md.jinja", contents: r#"# Codex Execution Plans (ExecPlans)

This document defines the contract for a self-contained execution plan that another engineer or agent can implement without prior context.

## Required Properties

- Every ExecPlan must be self-contained.
- Every ExecPlan must be a living document.
- Every ExecPlan must let a novice implement the work end to end.
- Every ExecPlan must describe observable outcomes, not just code edits.

## Required Sections

Every ExecPlan must contain these sections and keep them current:

- `Progress`
- `Surprises & Discoveries`
- `Decision Log`
- `Outcomes & Retrospective`

## Writing Rules

- Write for a reader who has only the current worktree and the ExecPlan.
- Define non-obvious terms in plain language.
- Name exact paths, modules, commands, and expected outcomes.
- Include commands to run, what success looks like, and how to recover from partial failure.
- Treat durable state and compatibility-sensitive changes explicitly.

## Suggested Skeleton

Use this shape:

1. Title and purpose
2. `Progress`
3. `Surprises & Discoveries`
4. `Decision Log`
5. `Outcomes & Retrospective`
6. Context and orientation
7. Plan of work
8. Concrete steps
9. Validation and acceptance
10. Idempotence and recovery
11. Interfaces and dependencies

## Maintenance Rule

When revising an ExecPlan, update every affected section so the file remains restartable from scratch.
"# },
    EmbeddedTemplateFile { relative_path: ".agent/jig-contract.json.jinja", contents: r#"{
  "contract_version": 3,
  "tool_namespace": "jig",
  "jig_version": "<<[ jig_version ]>>",
  "required_commands": [
    "bootstrap_command",
    "rust_fmt_check_command",
    "rust_clippy_command",
    "rust_test_command",
    "rust_test_locked_command"[% if frontend_harness_enabled %],
    "typescript_lint_command",
    "typescript_typecheck_command",
    "typescript_build_command",
    "typescript_coverage_command"[% endif %][% if sqlx_enabled %],
    "sqlx_check_command"[% endif %][% if sqlx_enabled and schema_dump_enabled %],
    "schema_dump_command"[% endif %]
  ],
  "tools": [
    {
      "name": "jig.bootstrap",
      "kind": "command",
      "description": "Run the configured project bootstrap command.",
      "command": "bootstrap_command"
    },
    {
      "name": "jig.fmt_check",
      "kind": "command",
      "description": "Run the configured format check command.",
      "command": "rust_fmt_check_command"
    },
    {
      "name": "jig.clippy",
      "kind": "command",
      "description": "Run the configured clippy command.",
      "command": "rust_clippy_command"
    },
    {
      "name": "jig.test",
      "kind": "command",
      "description": "Run the configured default test command.",
      "command": "rust_test_command"
    },
    {
      "name": "jig.test_locked",
      "kind": "command",
      "description": "Run the configured locked test command.",
      "command": "rust_test_locked_command"
    },
[% if frontend_harness_enabled %]
    {
      "name": "jig.typescript_lint",
      "kind": "command",
      "description": "Run the configured TypeScript lint command.",
      "command": "typescript_lint_command"
    },
    {
      "name": "jig.typescript_typecheck",
      "kind": "command",
      "description": "Run the configured TypeScript typecheck command.",
      "command": "typescript_typecheck_command"
    },
    {
      "name": "jig.typescript_build",
      "kind": "command",
      "description": "Run the configured TypeScript build command.",
      "command": "typescript_build_command"
    },
    {
      "name": "jig.typescript_coverage",
      "kind": "command",
      "description": "Run the configured TypeScript coverage command.",
      "command": "typescript_coverage_command"
    },
[% endif %]
[% if sqlx_enabled %]
[% if schema_dump_enabled %]
    {
      "name": "jig.schema_check",
      "kind": "native",
      "description": "Run the native schema drift check."
    },
    {
      "name": "jig.schema_dump",
      "kind": "command",
      "description": "Run the configured schema dump command.",
      "command": "schema_dump_command"
    },
[% endif %]
    {
      "name": "jig.sqlx_check",
      "kind": "command",
      "description": "Run the configured SQLx check command.",
      "command": "sqlx_check_command"
    },
    {
      "name": "jig.migration_add",
      "kind": "native",
      "description": "Add timestamped SQL migration stubs."
    },
[% endif %]
    {
      "name": "jig.contract_check",
      "kind": "native",
      "description": "Run the native Jig contract check."
    }
  ]
}
"# },
    EmbeddedTemplateFile { relative_path: ".agent/plans/.gitkeep.jinja", contents: r#"
"# },
    EmbeddedTemplateFile { relative_path: ".agent/state/.gitkeep.jinja", contents: r#"
"# },
    EmbeddedTemplateFile { relative_path: ".gitattributes.jinja", contents: r#"# BEGIN JIG MANAGED BLOCK
.agent/plans/*.md merge=union
.agent/state/*.jsonl merge=union
# END JIG MANAGED BLOCK
"# },
    EmbeddedTemplateFile { relative_path: ".github/workflows/agent-map-check.yml.jinja", contents: r#"name: Agent Map Check

on:
  pull_request:
    paths:
      - "AGENTS.md"
      - "**/AGENTS.md"
      - "agent-map.md"
[% for root in rust_crate_roots %]
      - "<<[ root ]>>/**"
[% endfor %]
      - "scripts/jig"
      - "scripts/install-jig.sh"
      - ".github/workflows/agent-map-check.yml"
  push:
    branches:
      - <<[ default_branch ]>>
    paths:
      - "AGENTS.md"
      - "**/AGENTS.md"
      - "agent-map.md"
[% for root in rust_crate_roots %]
      - "<<[ root ]>>/**"
[% endfor %]
      - "scripts/jig"
      - "scripts/install-jig.sh"
      - ".github/workflows/agent-map-check.yml"
  merge_group:
    types:
      - checks_requested
  workflow_dispatch:

permissions:
  contents: read

concurrency:
  group: agent-map-ci-${{ github.event.pull_request.number || github.ref }}
  cancel-in-progress: true

jobs:
  agent-map-check:
    name: Verify AGENTS map drift
    runs-on: <<[ ci_github_runner ]>>
    steps:
      - name: Checkout
        uses: actions/checkout@v6

      - name: Install Rust toolchain
        uses: actions-rust-lang/setup-rust-toolchain@v1
        with:
          cache: false
          rustflags: ""

      - name: Validate agent-map links and coverage
        shell: bash
        run: |
          scripts/jig check agent-map
"# },
    EmbeddedTemplateFile { relative_path: ".github/workflows/repo-policy.yml.jinja", contents: r#"name: Repo Policy

on:
  pull_request:
    paths:
[% for root in rust_crate_roots %]
      - "<<[ root ]>>/**"
[% endfor %]
      - ".jig.toml"
      - ".agent/jig-contract.json"
      - "scripts/jig"
      - "scripts/install-jig.sh"
[% if sqlx_enabled %]
      - "<<[ rust_migration_dir ]>>/**"
[% endif %]
      - ".github/workflows/repo-policy.yml"
  push:
    branches:
      - <<[ default_branch ]>>
    paths:
[% for root in rust_crate_roots %]
      - "<<[ root ]>>/**"
[% endfor %]
      - ".jig.toml"
      - ".agent/jig-contract.json"
      - "scripts/jig"
      - "scripts/install-jig.sh"
[% if sqlx_enabled %]
      - "<<[ rust_migration_dir ]>>/**"
[% endif %]
      - ".github/workflows/repo-policy.yml"
  merge_group:
    types:
      - checks_requested
  workflow_dispatch:

permissions:
  contents: read

concurrency:
  group: repo-policy-ci-${{ github.event.pull_request.number || github.ref }}
  cancel-in-progress: true

jobs:
  no-mod-rs:
    name: Check for disallowed mod.rs files
    runs-on: <<[ ci_github_runner ]>>
    steps:
      - name: Checkout
        uses: actions/checkout@v6
      - name: Install Rust toolchain
        uses: actions-rust-lang/setup-rust-toolchain@v1
        with:
          cache: false
          rustflags: ""
      - name: Detect disallowed mod.rs files
        run: |
          scripts/jig check no-mod-rs

  rust-file-loc:
    name: Enforce agentic-first Rust file size policy
    runs-on: <<[ ci_github_runner ]>>
    steps:
      - name: Checkout
        uses: actions/checkout@v6
        with:
          fetch-depth: 0
      - name: Install Rust toolchain
        uses: actions-rust-lang/setup-rust-toolchain@v1
        with:
          cache: false
          rustflags: ""
      - name: Check Rust file LOC policy
        run: |
          set -euo pipefail
          if git rev-parse --verify "origin/<<[ default_branch ]>>" >/dev/null 2>&1; then
            base_ref="$(git merge-base HEAD "origin/<<[ default_branch ]>>")"
          elif git rev-parse --verify HEAD^ >/dev/null 2>&1; then
            base_ref="HEAD^"
          else
            base_ref="4b825dc642cb6eb9a060e54bf8d69288fbee4904"
          fi
          echo "Using Rust LOC base ref: $base_ref"
          scripts/jig check rust-file-loc --changed-against "$base_ref"

[% if sqlx_enabled %]
  sqlx-unchecked-queries:
    name: Verify non-test SQLx queries are compile-time checked
    runs-on: <<[ ci_github_runner ]>>
    steps:
      - name: Checkout
        uses: actions/checkout@v6
      - name: Install Rust toolchain
        uses: actions-rust-lang/setup-rust-toolchain@v1
        with:
          cache: false
          rustflags: ""
      - name: Check unchecked SQLx query usage in non-test code
        run: |
          scripts/jig check sqlx-unchecked-non-test

  migration-immutability:
    name: Enforce migration immutability
    runs-on: <<[ ci_github_runner ]>>
    steps:
      - name: Checkout
        uses: actions/checkout@v6
        with:
          fetch-depth: 0
      - name: Install Rust toolchain
        uses: actions-rust-lang/setup-rust-toolchain@v1
        with:
          cache: false
          rustflags: ""
      - name: Determine base ref
        id: base_ref
        shell: bash
        run: |
          set -euo pipefail

          ref=""
          event_name="${{ github.event_name }}"

          if [[ "$event_name" == "pull_request" ]]; then
            ref="${{ github.event.pull_request.base.sha }}"
          elif [[ "$event_name" == "push" ]]; then
            ref="${{ github.event.before }}"
            if [[ "$ref" == "0000000000000000000000000000000000000000" ]]; then
              ref=""
            fi
          elif [[ "$event_name" == "merge_group" ]]; then
            ref="${{ github.event.merge_group.base_sha }}"
          fi

          if [[ -z "$ref" ]]; then
            if git rev-parse --verify origin/<<[ default_branch ]>> >/dev/null 2>&1; then
              ref="$(git merge-base HEAD origin/<<[ default_branch ]>>)"
            elif git rev-parse --verify HEAD^ >/dev/null 2>&1; then
              ref="HEAD^"
            else
              ref="4b825dc642cb6eb9a060e54bf8d69288fbee4904"
            fi
          fi

          echo "ref=$ref" >>"$GITHUB_OUTPUT"
      - name: Check migration immutability
        run: |
          scripts/jig check migration-immutability --changed-against "${{ steps.base_ref.outputs.ref }}"
[% endif %]
"# },
    EmbeddedTemplateFile { relative_path: ".github/workflows/rust-tests.yml.jinja", contents: r#"name: Rust Tests

on:
  pull_request:
    paths:
[% for root in rust_crate_roots %]
      - "<<[ root ]>>/**"
[% endfor %]
      - "Cargo.toml"
      - "Cargo.lock"
      - "rust-toolchain.toml"
      - ".clippy.toml"
      - "clippy.toml"
      - ".jig.toml"
      - ".agent/jig-contract.json"
      - "scripts/jig"
      - "scripts/install-jig.sh"
      - ".cargo/**"
      - ".github/workflows/rust-tests.yml"
  push:
    branches:
      - <<[ default_branch ]>>
    paths:
[% for root in rust_crate_roots %]
      - "<<[ root ]>>/**"
[% endfor %]
      - "Cargo.toml"
      - "Cargo.lock"
      - "rust-toolchain.toml"
      - ".clippy.toml"
      - "clippy.toml"
      - ".jig.toml"
      - ".agent/jig-contract.json"
      - "scripts/jig"
      - "scripts/install-jig.sh"
      - ".cargo/**"
      - ".github/workflows/rust-tests.yml"
  merge_group:
    types:
      - checks_requested
  workflow_dispatch:

permissions:
  contents: read

concurrency:
  group: rust-ci-${{ github.event.pull_request.number || github.ref }}
  cancel-in-progress: true

jobs:
  fmt:
    name: scripts/jig check fmt
    runs-on: <<[ ci_github_runner ]>>
    steps:
      - name: Checkout
        uses: actions/checkout@v6
      - name: Install Rust toolchain
        uses: actions-rust-lang/setup-rust-toolchain@v1
        with:
          cache: false
          rustflags: ""
          components: rustfmt
      - name: Run rustfmt check
        run: scripts/jig check fmt

  clippy:
    name: scripts/jig check clippy
    runs-on: <<[ ci_github_runner ]>>
[% if sqlx_enabled %]
    env:
      SQLX_OFFLINE: "true"
[% endif %]
    steps:
      - name: Checkout
        uses: actions/checkout@v6
      - name: Install Rust toolchain
        uses: actions-rust-lang/setup-rust-toolchain@v1
        with:
          cache: false
          rustflags: ""
          components: clippy
      - name: Cache Rust artifacts
        uses: Swatinem/rust-cache@v2
      - name: Run clippy
        run: scripts/jig check clippy

  test:
    name: scripts/jig check test-locked
    runs-on: <<[ ci_github_runner ]>>
[% if sqlx_enabled %]
    env:
      SQLX_OFFLINE: "true"
[% endif %]
    steps:
      - name: Checkout
        uses: actions/checkout@v6
      - name: Install Rust toolchain
        uses: actions-rust-lang/setup-rust-toolchain@v1
        with:
          cache: false
          rustflags: ""
      - name: Cache Rust artifacts
        uses: Swatinem/rust-cache@v2
      - name: Run locked Rust tests
        run: scripts/jig check test-locked
"# },
    EmbeddedTemplateFile { relative_path: ".github/workflows/webapp-checks.yml.jinja", contents: r#"[% if frontend_apps | length > 0 %]
name: Webapp Checks

on:
  pull_request:
    paths:
[% for app in frontend_apps %]
      - "<<[ app.dir ]>>/**"
[% endfor %]
      - "scripts/check-webapps.sh"
      - "scripts/check-webapp-scripts.mjs"
      - "scripts/enforce-coverage.cjs"
      - ".github/workflows/webapp-checks.yml"
  push:
    branches:
      - <<[ default_branch ]>>
    paths:
[% for app in frontend_apps %]
      - "<<[ app.dir ]>>/**"
[% endfor %]
      - "scripts/check-webapps.sh"
      - "scripts/check-webapp-scripts.mjs"
      - "scripts/enforce-coverage.cjs"
      - ".github/workflows/webapp-checks.yml"
  merge_group:
    types:
      - checks_requested
  workflow_dispatch:

permissions:
  contents: read

concurrency:
  group: webapp-ci-${{ github.event.pull_request.number || github.ref }}
  cancel-in-progress: true

jobs:
  checks:
    name: webapp checks
    runs-on: <<[ ci_github_runner ]>>
    strategy:
      fail-fast: false
      matrix:
        app:
[% for app in frontend_apps %]
          - name: <<[ app.name ]>>
            dir: <<[ app.dir ]>>
            coverage_threshold: <<[ app.coverage_threshold ]>>
[% endfor %]
    steps:
      - name: Checkout
        uses: actions/checkout@v6
[% if web_package_manager == "bun" %]
      - name: Setup Bun
        uses: oven-sh/setup-bun@v2
      - name: Setup Node
        uses: actions/setup-node@v5
        with:
          node-version: 22
      - name: Cache Bun dependencies
        uses: actions/cache@v5
        with:
          path: |
            ~/.bun/install/cache
            node_modules
            ${{ matrix.app.dir }}/node_modules
          key: ${{ runner.os }}-bun-${{ matrix.app.dir }}-${{ hashFiles('bun.lock', 'bun.lockb', format('{0}/bun.lock', matrix.app.dir), format('{0}/bun.lockb', matrix.app.dir)) }}
[% else %]
[% if web_package_manager == "npm" %]
      - name: Setup Node
        uses: actions/setup-node@v5
        with:
          node-version: 22
          cache: <<[ web_package_manager ]>>
          cache-dependency-path: |
            package-lock.json
            ${{ matrix.app.dir }}/package-lock.json
[% elif web_package_manager == "pnpm" %]
      - name: Setup Node
        uses: actions/setup-node@v5
        with:
          node-version: 22
      - name: Enable Corepack
        run: corepack enable
      # setup-node cache detection for pnpm needs Corepack shims first.
      - name: Configure Node dependency cache
        uses: actions/setup-node@v5
        with:
          node-version: 22
          cache: <<[ web_package_manager ]>>
          cache-dependency-path: |
            pnpm-lock.yaml
            ${{ matrix.app.dir }}/pnpm-lock.yaml
[% elif web_package_manager == "yarn" %]
      - name: Setup Node
        uses: actions/setup-node@v5
        with:
          node-version: 22
      - name: Enable Corepack
        run: corepack enable
      # setup-node cache detection for yarn needs Corepack shims first.
      - name: Configure Node dependency cache
        uses: actions/setup-node@v5
        with:
          node-version: 22
          cache: <<[ web_package_manager ]>>
          cache-dependency-path: |
            yarn.lock
            ${{ matrix.app.dir }}/yarn.lock
[% endif %]
[% endif %]
      - name: Validate package scripts
        run: node scripts/check-webapp-scripts.mjs "${{ matrix.app.dir }}" lint typecheck build:bundle test:coverage
      - name: Install dependencies
        shell: bash
        run: |
[% if web_package_manager == "bun" %]
          if [ -f package.json ] && { [ -f bun.lock ] || [ -f bun.lockb ]; }; then
            <<[ web_install_command ]>>
          else
            cd "${{ matrix.app.dir }}"
            <<[ web_install_command ]>>
          fi
[% elif web_package_manager == "npm" %]
          if [ -f package.json ] && [ -f package-lock.json ]; then
            <<[ web_install_command ]>>
          else
            cd "${{ matrix.app.dir }}"
            <<[ web_install_command ]>>
          fi
[% elif web_package_manager == "pnpm" %]
          if [ -f package.json ] && [ -f pnpm-lock.yaml ]; then
            <<[ web_install_command ]>>
          else
            cd "${{ matrix.app.dir }}"
            <<[ web_install_command ]>>
          fi
[% elif web_package_manager == "yarn" %]
          if [ -f package.json ] && [ -f yarn.lock ]; then
            <<[ web_install_command ]>>
          else
            cd "${{ matrix.app.dir }}"
            <<[ web_install_command ]>>
          fi
[% endif %]
      - name: Run lint
        run: cd "${{ matrix.app.dir }}" && <<[ web_run_command ]>> lint
      - name: Run typecheck
        run: cd "${{ matrix.app.dir }}" && <<[ web_run_command ]>> typecheck
      - name: Run build
        run: cd "${{ matrix.app.dir }}" && <<[ web_run_command ]>> build:bundle
      - name: Run tests with coverage
        run: cd "${{ matrix.app.dir }}" && <<[ web_run_command ]>> test:coverage
      - name: Enforce coverage threshold
        run: |
          COVERAGE_DIR="${{ matrix.app.dir }}/coverage" \
            COVERAGE_THRESHOLD="${{ matrix.app.coverage_threshold }}" \
            node scripts/enforce-coverage.cjs
[% else %]
name: Webapp Checks (Disabled)

on:
  workflow_dispatch:

permissions:
  contents: read

jobs:
  disabled:
    runs-on: <<[ ci_github_runner ]>>
    steps:
      - name: No configured web apps
        run: echo "No web apps configured in .jig.toml"
[% endif %]
"# },
    EmbeddedTemplateFile { relative_path: ".gitignore.jinja", contents: r#"# BEGIN JIG MANAGED BLOCK
# OS and editor noise
.DS_Store
.idea/
.vscode/
*.swp
*.swo

# Local environment and secrets
.env
.env.*
!.env.example
!.env.*.example

# Rust
target/

# JavaScript and TypeScript
node_modules/
coverage/
dist/
build/
.vite/
.turbo/
.astro/

# Jig local runtime cache. Keep durable agent state tracked.
.agent/.cache/*
!.agent/.cache/.gitignore

# Local logs and scratch files
*.log
tmp/
temp/
# END JIG MANAGED BLOCK
"# },
    EmbeddedTemplateFile { relative_path: ".jig.toml.jinja", contents: r#"_commit = "<<[ _jig.commit | replace("\\", "\\\\") | replace("\"", "\\\"") ]>>"
_src_path = "<<[ _jig.src_path | replace("\\", "\\\\") | replace("\"", "\\\"") ]>>"
_template_mode = "<<[ _jig.template_mode | replace("\\", "\\\\") | replace("\"", "\\\"") ]>>"
_template_local_path = "<<[ _jig.template_local_path | replace("\\", "\\\\") | replace("\"", "\\\"") ]>>"
repo_name = "<<[ repo_name | replace("\\", "\\\\") | replace("\"", "\\\"") ]>>"
default_branch = "<<[ default_branch | replace("\\", "\\\\") | replace("\"", "\\\"") ]>>"
ci_github_runner = "<<[ ci_github_runner | replace("\\", "\\\\") | replace("\"", "\\\"") ]>>"
jig_version = "<<[ jig_version | replace("\\", "\\\\") | replace("\"", "\\\"") ]>>"
template_source_url = "<<[ template_source_url | replace("\\", "\\\\") | replace("\"", "\\\"") ]>>"
harness_footprint = "<<[ harness_footprint | replace("\\", "\\\\") | replace("\"", "\\\"") ]>>"
sqlx_enabled = [% if sqlx_enabled %]true[% else %]false[% endif %]
rust_crate_roots = [[% for root in rust_crate_roots %]"<<[ root | replace("\\", "\\\\") | replace("\"", "\\\"") ]>>"[% if not loop.last %], [% endif %][% endfor %]]
[% if sqlx_enabled %]
rust_migration_dir = "<<[ rust_migration_dir | replace("\\", "\\\\") | replace("\"", "\\\"") ]>>"
rust_sqlx_metadata_dir = "<<[ rust_sqlx_metadata_dir | replace("\\", "\\\\") | replace("\"", "\\\"") ]>>"
[% endif %]
schema_dump_enabled = [% if schema_dump_enabled %]true[% else %]false[% endif %]
[% if sqlx_enabled and schema_dump_enabled %]
schema_dump_command = "<<[ schema_dump_command | replace("\\", "\\\\") | replace("\"", "\\\"") ]>>"
[% endif %]
[% if sqlx_enabled %]
sqlx_check_command = "<<[ sqlx_check_command | replace("\\", "\\\\") | replace("\"", "\\\"") ]>>"
[% endif %]
# Command values are project-owned. Generated Cargo defaults skip cleanly when
# no manifests are found; with nested manifests they run each one in turn.
# Review them against this repo's workspace layout and replace them when custom
# orchestration is needed.
bootstrap_command = "<<[ bootstrap_command | replace("\\", "\\\\") | replace("\"", "\\\"") ]>>"
[% if legacy_dev_command %]
# Deprecated and ignored by generated commands; preserved only so you can migrate it into [dev] / [[dev.apps]].
dev_command = "<<[ legacy_dev_command | replace("\\", "\\\\") | replace("\"", "\\\"") ]>>"
[% endif %]
rust_fmt_check_command = "<<[ rust_fmt_check_command | replace("\\", "\\\\") | replace("\"", "\\\"") ]>>"
rust_clippy_command = "<<[ rust_clippy_command | replace("\\", "\\\\") | replace("\"", "\\\"") ]>>"
rust_test_command = "<<[ rust_test_command | replace("\\", "\\\\") | replace("\"", "\\\"") ]>>"
rust_test_locked_command = "<<[ rust_test_locked_command | replace("\\", "\\\\") | replace("\"", "\\\"") ]>>"
web_package_manager = "<<[ web_package_manager | replace("\\", "\\\\") | replace("\"", "\\\"") ]>>"
[% if frontend_apps | length == 0 %]
frontend_apps = []
[% else %]
[% for app in frontend_apps %]
[[frontend_apps]]
name = "<<[ app.name | replace("\\", "\\\\") | replace("\"", "\\\"") ]>>"
dir = "<<[ app.dir | replace("\\", "\\\\") | replace("\"", "\\\"") ]>>"
coverage_threshold = <<[ app.coverage_threshold ]>>
[% endfor %]
[% endif %]

[vault]
scope = "<<[ vault.scope | replace("\\", "\\\\") | replace("\"", "\\\"") ]>>"
scope_id = "<<[ vault.scope_id | replace("\\", "\\\\") | replace("\"", "\\\"") ]>>"
allow_global = [% if vault.allow_global %]true[% else %]false[% endif %]

[% if frontend_harness_enabled %]
# Extra command keys must use *_command names so contract required_commands
# stay distinct from tool names and gate ids. Entries here override same-named
# legacy top-level command fields.
[commands]
typescript_lint_command = "<<[ typescript_lint_command | replace("\\", "\\\\") | replace("\"", "\\\"") ]>>"
typescript_typecheck_command = "<<[ typescript_typecheck_command | replace("\\", "\\\\") | replace("\"", "\\\"") ]>>"
typescript_build_command = "<<[ typescript_build_command | replace("\\", "\\\\") | replace("\"", "\\\"") ]>>"
typescript_coverage_command = "<<[ typescript_coverage_command | replace("\\", "\\\\") | replace("\"", "\\\"") ]>>"

[% endif %]
[dev]
proxy_port = 1355
https_port = 1443
https = false
# HTTPS listener ALPN only; cleartext proxy traffic remains HTTP/1.1.
http2 = true
lan = false
# Must be localhost, local, test, internal, or a subdomain below one of them.
tld = "localhost"
# Set true to discover JavaScript workspace packages that expose dev scripts.
workspace_discovery = false
[% for app in dev_apps %]

# Repo-local dev service.
[[dev.apps]]
name = "<<[ app.name | replace("\\", "\\\\") | replace("\"", "\\\"") ]>>"
[% if app.dir %]dir = "<<[ app.dir | replace("\\", "\\\\") | replace("\"", "\\\"") ]>>"
[% endif %]kind = "<<[ app.kind | replace("\\", "\\\\") | replace("\"", "\\\"") ]>>"
[% if app.port %]port = <<[ app.port ]>>
[% endif %][% if app.host %]host = "<<[ app.host | replace("\\", "\\\\") | replace("\"", "\\\"") ]>>"
[% endif %][% if not app.proxy %]proxy = false
[% endif %][% if app.command %]command = "<<[ app.command | replace("\\", "\\\\") | replace("\"", "\\\"") ]>>"
[% else %]argv = [[% for arg in app.argv %]"<<[ arg | replace("\\", "\\\\") | replace("\"", "\\\"") ]>>"[% if not loop.last %], [% endif %][% endfor %]]
[% endif %][% endfor %]
[% for app in generated_frontend_dev_apps %]

# Generated from [[frontend_apps]] so local dev uses explicit app settings while
# web CI keeps its coverage threshold metadata above. Jig validates that name
# and dir stay aligned with the matching frontend app.
[[dev.apps]]
name = "<<[ app.name | replace("\\", "\\\\") | replace("\"", "\\\"") ]>>"
dir = "<<[ app.dir | replace("\\", "\\\\") | replace("\"", "\\\"") ]>>"
kind = "<<[ app.kind | replace("\\", "\\\\") | replace("\"", "\\\"") ]>>"
argv = ["<<[ web_package_manager | replace("\\", "\\\\") | replace("\"", "\\\"") ]>>", "run", "dev"]
[% endfor %]

[[work.gates]]
id = "contract"
kind = "check"
tool = "jig.contract_check"

[[work.gates]]
id = "tests"
kind = "check"
tool = "jig.test"

[% if frontend_harness_enabled %]
[[work.gates]]
id = "typescript-lint"
kind = "check"
tool = "jig.typescript_lint"

[[work.gates]]
id = "typescript-typecheck"
kind = "check"
tool = "jig.typescript_typecheck"

[[work.gates]]
id = "typescript-build"
kind = "check"
tool = "jig.typescript_build"

[[work.gates]]
id = "typescript-coverage"
kind = "check"
tool = "jig.typescript_coverage"

[% endif %]
[% if sqlx_enabled %]
[[work.gates]]
id = "sqlx"
kind = "check"
tool = "jig.sqlx_check"

[% endif %]
[% if sqlx_enabled and schema_dump_enabled %]
[[work.gates]]
id = "schema"
kind = "check"
tool = "jig.schema_check"

[[work.gates]]
id = "schema-dump"
kind = "check"
tool = "jig.schema_dump"

[% endif %]
[% if agent_tooling.codex.marketplaces | length == 0 %]
[agent_tooling.codex]
marketplaces = []
[% else %]
[% for marketplace in agent_tooling.codex.marketplaces %]
[[agent_tooling.codex.marketplaces]]
id = "<<[ marketplace.id | replace("\\", "\\\\") | replace("\"", "\\\"") ]>>"
source = "<<[ marketplace.source | replace("\\", "\\\\") | replace("\"", "\\\"") ]>>"
plugins = [[% for plugin in marketplace.plugins %]"<<[ plugin | replace("\\", "\\\\") | replace("\"", "\\\"") ]>>"[% if not loop.last %], [% endif %][% endfor %]]
[% endfor %]
[% endif %]
"# },
    EmbeddedTemplateFile { relative_path: ".mcp.json.jinja", contents: r#"{
  "mcpServers": {
    "jig": {
      "command": "./scripts/jig",
      "args": ["mcp"]
    }
  }
}
"# },
    EmbeddedTemplateFile { relative_path: "AGENTS.md.jinja", contents: r#"# Repository Guidelines

<!-- BEGIN JIG MANAGED BLOCK -->
This repository uses the shared `jig.sh` workflow. Keep repo-local business rules and ownership guidance in crate-level guides; keep generic agent workflow and repo policy here.

## Start Here

- Use this file for repo-wide defaults.
- Open [agent-map.md](./agent-map.md) before backend work.
- Read the nearest crate-level `AGENTS.md` before changing a crate when one exists.
- Use `.agent/PLANS.md` when writing an ExecPlan for a complex feature or refactor.
- Use `scripts/jig` for the typed repo contract and `scripts/jig mcp` for MCP clients.
- On a fresh machine, run `scripts/jig doctor`; follow its next step, including `scripts/jig agent bootstrap` when Jig Codex skills are missing.
- For substantial work, use `scripts/jig work start`, `scripts/jig work check`, `scripts/jig work evidence`, `scripts/jig work gates`, and `scripts/jig work finish` to keep plans, receipts, and required gates connected.
- Treat `.agent/state/*.jsonl` as append-only repo memory.

## Compatibility And Cutovers

- Prefer direct cutovers only for internal code-only changes that can ship in one coordinated deploy.
- Preserve compatibility or stage rollouts for persisted database state, queued job types, public API contracts, bookmarked routes, webhook boundaries, or source-of-truth moves that can straddle deploys.
[% if sqlx_enabled %]
- Never overwrite an existing database migration; add a new forward-only migration instead.
[% endif %]

## Backend Defaults

- Treat [% for root in rust_crate_roots %]`<<[ root ]>>`[% if not loop.last %], [% endif %][% endfor %] as Rust crate roots.
- Add crate-level `AGENTS.md` files when a crate has meaningful ownership, entrypoint, or invariant guidance that should travel with that crate.
[% if sqlx_enabled %]
- SQL migrations live under `<<[ rust_migration_dir ]>>`.
- SQLx metadata is committed in `<<[ rust_sqlx_metadata_dir ]>>`.
[% endif %]
- Keep transport logic thin and business logic in the owning crate.
[% if sqlx_enabled %]
- Keep transaction boundaries explicit and deterministic.
[% endif %]

## Frontend Defaults

[% if frontend_apps | length > 0 -%]
Configured web apps:

[% for app in frontend_apps -%]
- `<<[ app.name ]>>` in `<<[ app.dir ]>>`
[% endfor %]

Each configured app is expected to support `lint`, `typecheck`, `build:bundle`, and `test:coverage`.
`test:coverage` must write `coverage/coverage-summary.json` in the app directory for threshold enforcement.
Jig validates those scripts during adoption; generated web CI validates them again before running.
Generated install steps use a repo-root lockfile when one exists, otherwise the app-local lockfile.
`[[frontend_apps]]` keeps CI and coverage metadata; generated `[[dev.apps]]` drives `scripts/jig dev` and takes precedence for local dev settings. When both sections are present, Jig requires a matching dev app name and dir for every frontend app. Use the optional fourth `--frontend-app` field or answers-file `kind` for `vite` versus `env-port`; extra `[[dev.apps]]` entries without `[[frontend_apps]]` are treated as dev-only and are not covered by generated web CI.
Remove legacy `dev_command` keys; local dev now runs through `[dev]` and `[[dev.apps]]`.
[% else -%]
No web apps are configured in `.jig.toml`.
[% endif %]

## Preferred Commands

- `scripts/jig bootstrap`
- `scripts/jig doctor`
- `scripts/jig dev`
- `scripts/jig check test`
- `scripts/jig check fmt`
- `scripts/jig check clippy`
- `scripts/jig work status`
- `scripts/jig work evidence`
[% if frontend_apps | length > 0 %]
- `scripts/jig check typescript-lint`
- `scripts/jig check typescript-typecheck`
- `scripts/jig check typescript-build`
- `scripts/jig check typescript-coverage`
[% endif %]
[% if sqlx_enabled %]
- `scripts/jig check sqlx`
[% if schema_dump_enabled %]
- `scripts/jig check schema`
- `scripts/jig schema-dump`
[% endif %]
- `scripts/jig migration-add NAME`
[% endif %]
- `scripts/jig check contract`

## Done Means

- Run the relevant local verification for the area you changed.
- For backend changes, finish with `scripts/jig check test`.
[% if frontend_apps | length > 0 %]
- For frontend changes, run the relevant `scripts/jig check typescript-*` gates.
[% endif %]
[% if sqlx_enabled %]
- For SQLx or migration changes, run `scripts/jig check sqlx`.
[% if schema_dump_enabled %]
- For schema-doc-enabled repos, run `scripts/jig check schema`.
[% endif %]
[% endif %]
- Review the generated diff for stale docs, policy drift, or missing dependent updates.

## Crate Guide Conventions

When a backend crate has a crate-level `AGENTS.md`, use these sections:

- `## Purpose`
- `## Key entrypoints`
- `## Edit here for X`
- `## Invariants`
- `## Common commands`
<!-- END JIG MANAGED BLOCK -->
"# },
    EmbeddedTemplateFile { relative_path: "agent-map.md.jinja", contents: r#"# Agent Map

Fast jump index for agent-facing guidance in this repository.

## Root guide

- [Repository AGENTS.md](./AGENTS.md)

## Project guides

Run `scripts/jig agent-map generate` to rebuild this file from tracked `AGENTS.md` files.
"# },
    EmbeddedTemplateFile { relative_path: "scripts/check-webapp-scripts.mjs.jinja", contents: r#"#!/usr/bin/env node

// Rendered through Jinja so generated repos manage this helper with the rest
// of the shared Jig template, even though this file has no template variables.
import fs from "node:fs";
import path from "node:path";

const [, , appDir, ...requiredScripts] = process.argv;

if (!appDir || requiredScripts.length === 0) {
  console.error("Usage: check-webapp-scripts.mjs <app-dir> <script>...");
  process.exit(2);
}

const packagePath = path.join(appDir, "package.json");
let packageJson;

try {
  packageJson = JSON.parse(fs.readFileSync(packagePath, "utf8"));
} catch (error) {
  console.error(`Failed to read ${packagePath}: ${error.message}`);
  process.exit(1);
}

const scripts = packageJson.scripts ?? {};
const missing = requiredScripts.filter((script) => {
  const command = scripts[script];
  return typeof command !== "string" || command.trim().length === 0;
});

if (missing.length > 0) {
  console.error(
    `Missing package.json scripts in ${appDir}: ${missing.join(", ")}. ` +
      "Add them or remove this app from [[frontend_apps]] until web CI is ready.",
  );
  process.exit(1);
}
"# },
    EmbeddedTemplateFile { relative_path: "scripts/check-webapps.sh.jinja", contents: r#"#!/usr/bin/env bash
set -euo pipefail

mode="${1:-}"
node_bin="${NODE:-node}"

usage() {
  echo "Usage: scripts/check-webapps.sh lint|typecheck|build|coverage" >&2
}

install_dependencies() {
  local app_dir="$1"
[% if web_package_manager == "bun" %]
  if [ -f package.json ] && { [ -f bun.lock ] || [ -f bun.lockb ]; }; then
    <<[ web_install_command ]>>
  else
    (cd "$app_dir" && <<[ web_install_command ]>>)
  fi
[% elif web_package_manager == "npm" %]
  if [ -f package.json ] && [ -f package-lock.json ]; then
    <<[ web_install_command ]>>
  else
    (cd "$app_dir" && <<[ web_install_command ]>>)
  fi
[% elif web_package_manager == "pnpm" %]
  if [ -f package.json ] && [ -f pnpm-lock.yaml ]; then
    <<[ web_install_command ]>>
  else
    (cd "$app_dir" && <<[ web_install_command ]>>)
  fi
[% elif web_package_manager == "yarn" %]
  if [ -f package.json ] && [ -f yarn.lock ]; then
    <<[ web_install_command ]>>
  else
    (cd "$app_dir" && <<[ web_install_command ]>>)
  fi
[% endif %]
}

run_package_script() {
  local app_dir="$1"
  local script_name="$2"

  (cd "$app_dir" && <<[ web_run_command ]>> "$script_name")
}

run_check() {
  local app_dir="$1"
  local coverage_threshold="$2"
  local script_name="$3"

  "$node_bin" scripts/check-webapp-scripts.mjs "$app_dir" "$script_name"
  install_dependencies "$app_dir"
  run_package_script "$app_dir" "$script_name"

  if [ "$mode" = "coverage" ]; then
    COVERAGE_DIR="$app_dir/coverage" COVERAGE_THRESHOLD="$coverage_threshold" \
      "$node_bin" scripts/enforce-coverage.cjs
  fi
}

case "$mode" in
  lint)
[% if frontend_apps | length > 0 %]
[% for app in frontend_apps %]
    run_check "<<[ app.dir ]>>" "<<[ app.coverage_threshold ]>>" "lint"
[% endfor %]
[% else %]
    echo "No web apps configured."
[% endif %]
    ;;
  typecheck)
[% if frontend_apps | length > 0 %]
[% for app in frontend_apps %]
    run_check "<<[ app.dir ]>>" "<<[ app.coverage_threshold ]>>" "typecheck"
[% endfor %]
[% else %]
    echo "No web apps configured."
[% endif %]
    ;;
  build)
[% if frontend_apps | length > 0 %]
[% for app in frontend_apps %]
    run_check "<<[ app.dir ]>>" "<<[ app.coverage_threshold ]>>" "build:bundle"
[% endfor %]
[% else %]
    echo "No web apps configured."
[% endif %]
    ;;
  coverage)
[% if frontend_apps | length > 0 %]
[% for app in frontend_apps %]
    run_check "<<[ app.dir ]>>" "<<[ app.coverage_threshold ]>>" "test:coverage"
[% endfor %]
[% else %]
    echo "No web apps configured."
[% endif %]
    ;;
  *)
    usage
    exit 2
    ;;
esac
"# },
    EmbeddedTemplateFile { relative_path: "scripts/enforce-coverage.cjs.jinja", contents: r#"#!/usr/bin/env node
const fs = require("node:fs");
const path = require("node:path");

const coverageDir = process.env.COVERAGE_DIR ?? "coverage";
const threshold = Number(process.env.COVERAGE_THRESHOLD ?? "0");
const summaryPath = path.join(coverageDir, "coverage-summary.json");

if (!fs.existsSync(summaryPath)) {
  console.log("No coverage summary generated; creating an empty summary.");
  fs.mkdirSync(coverageDir, { recursive: true });
  const empty = {
    total: {
      lines: { pct: 0 },
      functions: { pct: 0 },
      statements: { pct: 0 },
      branches: { pct: 0 },
    },
  };
  fs.writeFileSync(summaryPath, JSON.stringify(empty, null, 2));
}

const summary = JSON.parse(fs.readFileSync(summaryPath, "utf8"));
const total = summary.total ?? {};
const metrics = ["lines", "functions", "statements", "branches"];
const below = [];

for (const metric of metrics) {
  const pct = Number(total[metric]?.pct ?? 0);
  console.log(`${metric}: ${pct}%`);
  if (pct < threshold) {
    below.push(`${metric} (${pct}%)`);
  }
}

if (below.length > 0) {
  console.error(`Coverage below threshold ${threshold}%: ${below.join(", ")}`);
  process.exit(1);
}
"# },
    EmbeddedTemplateFile { relative_path: "scripts/install-jig.sh.jinja", contents: r##"#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)"
ANSWERS_FILE="$ROOT_DIR/.jig.toml"

read_field() {
  python3 -c '
import ast
import pathlib
import re
import sys

try:
    import tomllib
except ModuleNotFoundError:
    tomllib = None

text = pathlib.Path(sys.argv[1]).read_text()
key = sys.argv[2]

if tomllib is not None:
    value = tomllib.loads(text).get(key, "")
    if value is None:
        value = ""
    if not isinstance(value, str):
        print(f"Unsupported non-string value for {key}.", file=sys.stderr)
        raise SystemExit(1)
    print(value)
    raise SystemExit(0)

# The fallback intentionally reads only top-level scalar string answers used by
# this launcher. tomllib remains authoritative when available.
def strip_inline_comment(value):
    quote = None
    escaped = False
    for index, char in enumerate(value):
        if escaped:
            escaped = False
            continue
        if char == "\\":
            escaped = True
            continue
        if quote is not None:
            if char == quote:
                quote = None
            continue
        if char in {chr(39), chr(34)}:
            quote = char
            continue
        if char == "#":
            return value[:index].rstrip()
    return value.strip()

pattern = re.compile(rf"^\s*{re.escape(key)}\s*=\s*(.*?)\s*$")
for line in text.splitlines():
    stripped = line.strip()
    if not stripped or stripped.startswith("#"):
        continue
    if stripped.startswith("["):
        break
    match = pattern.match(line)
    if match:
        print(ast.literal_eval(strip_inline_comment(match.group(1))))
        break
else:
    print("")
' "$ANSWERS_FILE" "$1"
}

JIG_VERSION="$(read_field jig_version)"
SRC_PATH="$(read_field _src_path)"
TEMPLATE_COMMIT="$(read_field _commit)"
TEMPLATE_SOURCE_URL="$(read_field template_source_url)"
OFFICIAL_JIG_SOURCE="https://github.com/bpcakes/jig-sh.git"

if [[ -z "$JIG_VERSION" ]]; then
  echo "Failed to read jig_version from $ANSWERS_FILE." >&2
  exit 1
fi

if [[ -z "$SRC_PATH" ]]; then
  echo "Failed to read _src_path from $ANSWERS_FILE." >&2
  exit 1
fi

is_remote_source() {
  local source="$1"
  [[ "$source" == *"://"* || "$source" == git@*:* ]]
}

is_embedded_source() {
  local source="$1"
  # Keep this sentinel in sync with EMBEDDED_TEMPLATE_SOURCE in the Rust runtime.
  [[ "$source" == "embedded:jig-sh" ]]
}

# JIG_INSTALL_PROFILE is for direct installer calls. The scripts/jig launcher
# passes --profile explicitly so command-aware routing wins over ambient env.
INSTALL_PROFILE="${JIG_INSTALL_PROFILE:-default}"
INSTALL_ROOT_ARG=""
while [[ "$#" -gt 0 ]]; do
  case "$1" in
    --profile)
      if [[ "$#" -lt 2 ]]; then
        echo "--profile requires a value." >&2
        exit 2
      fi
      INSTALL_PROFILE="$2"
      shift 2
      ;;
    --profile=*)
      INSTALL_PROFILE="${1#--profile=}"
      shift
      ;;
    -*)
      echo "Unknown install-jig option: $1" >&2
      exit 2
      ;;
    *)
      if [[ -n "$INSTALL_ROOT_ARG" ]]; then
        echo "Unexpected extra install root argument: $1" >&2
        exit 2
      fi
      INSTALL_ROOT_ARG="$1"
      shift
      ;;
  esac
done

case "$INSTALL_PROFILE" in
  default | runtime | mcp)
    ;;
  *)
    echo "Unsupported jig install profile: $INSTALL_PROFILE" >&2
    exit 2
    ;;
esac

if [[ -d "$ROOT_DIR/.git" ]]; then
  DEFAULT_INSTALL_BASE="$ROOT_DIR/.git/jig-tools"
else
  DEFAULT_INSTALL_BASE="$ROOT_DIR/.agent/.cache/jig"
fi

case "$INSTALL_PROFILE" in
  default)
    DEFAULT_INSTALL_ROOT="$DEFAULT_INSTALL_BASE/$JIG_VERSION"
    CARGO_INSTALL_FEATURE_ARGS=()
    ;;
  runtime | mcp)
    DEFAULT_INSTALL_ROOT="$DEFAULT_INSTALL_BASE/$JIG_VERSION-runtime"
    CARGO_INSTALL_FEATURE_ARGS=(--no-default-features)
    ;;
esac

INSTALL_ROOT="${INSTALL_ROOT_ARG:-$DEFAULT_INSTALL_ROOT}"
BIN_PATH="$INSTALL_ROOT/bin/jig"
INSTALL_LOCK_DIR="$INSTALL_ROOT.lock"
INSTALL_LOCK_ATTEMPTS=30
STALE_INSTALL_LOCK_SECONDS=300

binary_version() {
  local bin_path="$1"
  "$bin_path" --version 2>/dev/null | awk '{print $2}'
}

assert_exact_version() {
  local bin_path="$1"
  local actual_version
  actual_version="$(binary_version "$bin_path" || true)"
  if [[ "$actual_version" != "$JIG_VERSION" ]]; then
    echo "Expected jig version $JIG_VERSION, found ${actual_version:-<missing>} at $bin_path." >&2
    return 1
  fi
}

hash_stdin() {
  local digest
  if command -v sha256sum >/dev/null 2>&1; then
    digest="$(sha256sum | awk '{print $1}')"
    printf 'sha256:%s\n' "$digest"
    return
  fi
  if command -v shasum >/dev/null 2>&1; then
    digest="$(shasum -a 256 | awk '{print $1}')"
    printf 'sha256:%s\n' "$digest"
    return
  fi
  if command -v openssl >/dev/null 2>&1; then
    digest="$(openssl dgst -sha256 -r | awk '{print $1}')"
    printf 'sha256:%s\n' "$digest"
    return
  fi
  echo "No SHA-256 utility found; local jig source installs will not be cache-stamped." >&2
  return 1
}

local_source_stamp() {
  local source_root="$1"
  # Keep this path list aligned with the crates and manifests that feed the jig
  # binary; omitted build inputs can make the source-cache stamp stale.
  {
    git -C "$source_root" rev-parse HEAD 2>/dev/null || printf 'unknown-head\n'
    git -C "$source_root" diff HEAD -- Cargo.toml Cargo.lock crates/jig crates/jig-dev-proxy 2>/dev/null || true
  } | hash_stdin
}

local_source_install_is_current() {
  local source_root="$1"
  local stamp_path="$INSTALL_ROOT/.jig-source-stamp"

  [[ -x "$BIN_PATH" ]] || return 1
  assert_exact_version "$BIN_PATH" >/dev/null || return 1
  [[ -f "$stamp_path" ]] || return 1
  local current_stamp
  current_stamp="$(local_source_stamp "$source_root")" || return 1
  [[ "$(cat "$stamp_path")" == "$current_stamp" ]]
}

write_local_source_stamp() {
  local source_root="$1"
  local current_stamp
  local stamp_path="$INSTALL_ROOT/.jig-source-stamp"
  local temp_stamp="$stamp_path.$$"
  current_stamp="$(local_source_stamp "$source_root")" || {
    rm -f "$stamp_path"
    return 0
  }
  printf '%s\n' "$current_stamp" >"$temp_stamp"
  mv "$temp_stamp" "$stamp_path"
}

install_from_dev_bin() {
  local dev_bin
  dev_bin="$(resolve_executable_path "$JIG_DEV_BIN")" || {
    echo "Failed to resolve JIG_DEV_BIN: $JIG_DEV_BIN" >&2
    exit 1
  }
  if [[ ! -x "$dev_bin" ]]; then
    echo "JIG_DEV_BIN is set but is not executable: $dev_bin" >&2
    exit 1
  fi

  if ! assert_exact_version "$dev_bin"; then
    echo "JIG_DEV_BIN must match jig version $JIG_VERSION; refusing to install a fallback binary." >&2
    echo "Rebuild from the jig source checkout with: cargo build -p jig-sh --bin jig" >&2
    echo "Then set JIG_DEV_BIN=target/debug/jig, unset JIG_DEV_BIN, or run scripts/jig so the normal cached installer path can select a compatible runtime." >&2
    exit 1
  fi
  # scripts/jig captures stdout from this installer and execs the printed path.
  printf '%s\n' "$dev_bin"
}

resolve_executable_path() {
  local input="$1"
  if command -v realpath >/dev/null 2>&1; then
    realpath "$input"
    return
  fi
  if command -v python3 >/dev/null 2>&1; then
    python3 -c '
import os
import sys

print(os.path.realpath(sys.argv[1]))
' "$input"
    return
  fi

  local input_dir
  input_dir="$(cd "$(dirname "$input")" && pwd -P)" || return 1
  local resolved="$input_dir/$(basename "$input")"
  case "$resolved" in
    /*)
      printf '%s\n' "$resolved"
      ;;
    *)
      echo "Resolved executable path is not absolute: $resolved" >&2
      return 1
      ;;
  esac
}

acquire_install_lock() {
  mkdir -p "$(dirname "$INSTALL_ROOT")"
  local attempt
  attempt=1
  while [[ "$attempt" -le "$INSTALL_LOCK_ATTEMPTS" ]]; do
    if mkdir "$INSTALL_LOCK_DIR" 2>/dev/null; then
      trap release_install_lock EXIT
      return 0
    fi
    if install_lock_is_stale; then
      rmdir "$INSTALL_LOCK_DIR" 2>/dev/null || true
      continue
    fi
    sleep 1
    attempt=$((attempt + 1))
  done
  echo "Timed out waiting for jig installer lock: $INSTALL_LOCK_DIR" >&2
  if [[ -d "$INSTALL_LOCK_DIR" ]]; then
    # Downstream harnesses intentionally omit jig-sh source-checkout recovery advice.
    echo "Another scripts/jig install may still be running." >&2
  else
    echo "Could not create jig installer lock; check permissions for $(dirname "$INSTALL_LOCK_DIR")." >&2
  fi
  exit 1
}

install_lock_is_stale() {
  [[ -d "$INSTALL_LOCK_DIR" ]] || return 1
  local now mtime
  now="$(date +%s)"
  # macOS/BSD stat uses -f, GNU stat uses -c.
  if mtime="$(stat -f %m "$INSTALL_LOCK_DIR" 2>/dev/null)"; then
    :
  elif mtime="$(stat -c %Y "$INSTALL_LOCK_DIR" 2>/dev/null)"; then
    :
  else
    return 1
  fi
  [[ $((now - mtime)) -gt $STALE_INSTALL_LOCK_SECONDS ]]
}

release_install_lock() {
  if [[ -d "$INSTALL_LOCK_DIR" ]]; then
    rmdir "$INSTALL_LOCK_DIR" 2>/dev/null || true
  fi
}

install_from_local_source() {
  local source_root="$1"
  local crate_path="$source_root/crates/jig"
  if [[ ! -d "$crate_path" ]]; then
    echo "Expected local jig source at $crate_path." >&2
    return 1
  fi

  cargo install \
    --path "$crate_path" \
    --root "$INSTALL_ROOT" \
    --locked \
    --force \
    "${CARGO_INSTALL_FEATURE_ARGS[@]}"

  assert_exact_version "$BIN_PATH"
  write_local_source_stamp "$source_root"
}

is_jig_source_checkout() {
  local source_root="$1"
  [[ -n "$source_root" ]] || return 1
  # This helper is rendered into downstream harnesses too so the same template
  # can repair the jig-sh source repo; ordinary projects fail these checks and
  # fall through to the configured template source.
  local manifest="$source_root/crates/jig/Cargo.toml"
  [[ -f "$source_root/templates/project/scripts/install-jig.sh.jinja" ]] || return 1
  [[ -f "$manifest" ]] || return 1
  grep -Eq '^[[:space:]]*name[[:space:]]*=[[:space:]]*"jig-sh"' "$manifest"
}

install_from_git_source() {
  local git_ref_args=(--tag "v$JIG_VERSION")
  if [[ "$TEMPLATE_COMMIT" =~ ^[0-9a-fA-F]{7,40}$ ]]; then
    # Adopted repos pin the exact template revision in .jig.toml. Treat that
    # commit as trusted repo configuration: a hex value intentionally overrides
    # the release tag so updates install the same source revision that rendered
    # the repo-local harness.
    git_ref_args=(--rev "$TEMPLATE_COMMIT")
  fi

  cargo install \
    --git "$SRC_PATH" \
    "${git_ref_args[@]}" \
    --root "$INSTALL_ROOT" \
    --locked \
    --force \
    "${CARGO_INSTALL_FEATURE_ARGS[@]}" \
    jig-sh

  assert_exact_version "$BIN_PATH"
}

resolve_installed_jig_for_embedded_source() {
  local candidate
  candidate="$(command -v jig 2>/dev/null || true)"
  [[ -n "$candidate" ]] || return 1
  candidate="$(resolve_executable_path "$candidate")" || return 1
  assert_exact_version "$candidate" >/dev/null || return 1
  printf '%s\n' "$candidate"
}

if [[ -n "${JIG_DEV_BIN:-}" ]]; then
  install_from_dev_bin
  exit 0
fi

# The jig-sh source repo dogfoods generated harness files. Prefer a cache that
# was built from the current checkout over an older same-version release cache.
# Explicit install roots keep the lower-level installer behavior so callers can
# populate exactly the root they requested.
if [[ -z "$INSTALL_ROOT_ARG" ]] && is_jig_source_checkout "$ROOT_DIR"; then
  if local_source_install_is_current "$ROOT_DIR"; then
    printf '%s\n' "$BIN_PATH"
    exit 0
  fi

  acquire_install_lock

  if local_source_install_is_current "$ROOT_DIR"; then
    printf '%s\n' "$BIN_PATH"
    exit 0
  fi

  install_from_local_source "$ROOT_DIR"
  printf '%s\n' "$BIN_PATH"
  exit 0
fi

if is_embedded_source "$SRC_PATH"; then
  if BIN_PATH="$(resolve_installed_jig_for_embedded_source)"; then
    printf '%s\n' "$BIN_PATH"
    exit 0
  elif [[ "${JIG_INSTALL_ALLOW_EMBEDDED_SOURCE_FALLBACK:-}" != "1" ]]; then
    echo "This repo was rendered from embedded Jig templates, but no same-version jig binary was found on PATH." >&2
    echo "Install the matching jig binary or set JIG_DEV_BIN to it. To knowingly install from ${TEMPLATE_SOURCE_URL:-$OFFICIAL_JIG_SOURCE} instead, set JIG_INSTALL_ALLOW_EMBEDDED_SOURCE_FALLBACK=1." >&2
    exit 1
  else
    SRC_PATH="${TEMPLATE_SOURCE_URL:-$OFFICIAL_JIG_SOURCE}"
    echo "Warning: installing from $SRC_PATH instead of the embedded template payload that adopted this repo." >&2
  fi
fi

if [[ "$INSTALL_PROFILE" != "default" && -z "$INSTALL_ROOT_ARG" ]]; then
  # Runtime and MCP profiles are subsets of the default binary. Reuse a matching
  # full build instead of compiling a stripped binary when it already exists.
  FULL_BIN_PATH="$DEFAULT_INSTALL_BASE/$JIG_VERSION/bin/jig"
  if [[ -x "$FULL_BIN_PATH" ]] && assert_exact_version "$FULL_BIN_PATH"; then
    printf '%s\n' "$FULL_BIN_PATH"
    exit 0
  fi
fi

if [[ -x "$BIN_PATH" ]] && assert_exact_version "$BIN_PATH"; then
  printf '%s\n' "$BIN_PATH"
  exit 0
fi

acquire_install_lock

if [[ -x "$BIN_PATH" ]] && assert_exact_version "$BIN_PATH"; then
  printf '%s\n' "$BIN_PATH"
  exit 0
fi

if [[ -d "$SRC_PATH/crates/jig" ]] || [[ "$SRC_PATH" == /* && -d "$SRC_PATH" ]]; then
  install_from_local_source "$SRC_PATH"
elif [[ -n "$TEMPLATE_SOURCE_URL" ]]; then
  SRC_PATH="$TEMPLATE_SOURCE_URL"
  install_from_git_source
elif is_remote_source "$SRC_PATH"; then
  install_from_git_source
else
  echo "Cannot resolve jig source from _src_path='$SRC_PATH'." >&2
  echo "Re-render from an absolute committed template path or set template_source_url." >&2
  exit 1
fi

printf '%s\n' "$BIN_PATH"
"## },
    EmbeddedTemplateFile { relative_path: "scripts/jig.jinja", contents: r#"#!/bin/sh
set -eu

# Keep launcher behavior synchronized with scripts/jig in the jig-sh source
# tree; this template may gate source-checkout-only user messages.
SCRIPT_DIR="$(dirname "$0")"
ROOT_DIR="$(CDPATH= cd "$SCRIPT_DIR/.." && pwd -P)"
INSTALLER="$ROOT_DIR/scripts/install-jig.sh"
JIG_VERSION="<<[ jig_version ]>>"

if [ ! -x "$INSTALLER" ]; then
  printf '%s\n' "Missing $INSTALLER." >&2
  exit 1
fi

jig_help_requested_before_separator() {
  for arg in "$@"; do
    case "$arg" in
      --)
        return 1
        ;;
      -h | --help)
        return 0
        ;;
    esac
  done
  return 1
}

binary_version() {
  "$1" --version 2>/dev/null | awk '{print $2}'
}

use_matching_binary() {
  candidate_bin="$1"

  [ -x "$candidate_bin" ] || return 1
  candidate_version="$(binary_version "$candidate_bin" || true)"
  [ "$candidate_version" = "$JIG_VERSION" ] || return 1

  printf '%s\n' "$candidate_bin"
}

is_jig_source_checkout() {
  [ -f "$ROOT_DIR/crates/jig/Cargo.toml" ] && [ -f "$ROOT_DIR/templates/project/scripts/jig.jinja" ]
}

default_install_base() {
  if [ -d "$ROOT_DIR/.git" ]; then
    printf '%s\n' "$ROOT_DIR/.git/jig-tools"
  else
    # Git worktrees have .git as a file; keep their launcher cache repo-local.
    printf '%s\n' "$ROOT_DIR/.agent/.cache/jig"
  fi
}

resolve_cached_binary() {
  install_base="$(default_install_base)"

  if is_jig_source_checkout && use_matching_binary "$ROOT_DIR/target/debug/jig"; then
    # In the jig-sh source checkout, prefer a freshly built dev binary so
    # launcher/help dogfooding exercises the current workspace before a cache.
    return 0
  fi

  # use_matching_binary validates the binary-reported version and prints the
  # selected path on success for the caller's command substitution.
  if use_matching_binary "$install_base/$JIG_VERSION/bin/jig"; then
    return 0
  fi
  if use_matching_binary "$install_base/$JIG_VERSION-runtime/bin/jig"; then
    return 0
  fi

  return 1
}

resolve_help_binary() {
  if [ -n "${JIG_DEV_BIN:-}" ]; then
    # Dev mode intentionally lets the installer resolve the requested profile
    # so local binary overrides stay consistent across help and execution.
    "$INSTALLER" --profile runtime
    return
  fi

  resolve_cached_binary
}

resolve_or_install_help_binary() {
  # Sets bin_path and, when an existing binary was version-checked,
  # version_checked in caller scope. Fresh installs still take the final
  # post-case version check below.
  if bin_path="$(resolve_help_binary)"; then
    version_checked=true
  else
    printf '%s\n' "Preparing jig $JIG_VERSION for help output; first run may install the repo-local runtime." >&2
    bin_path="$("$INSTALLER" --profile runtime)"
  fi
}

resolve_mcp_binary() {
  if [ -n "${JIG_DEV_BIN:-}" ]; then
    "$INSTALLER" --profile mcp
    return
  fi

  if resolve_cached_binary; then
    return
  fi

  printf '%s\n' \
    "No prebuilt jig $JIG_VERSION binary is available for MCP startup." \
    "Refusing to run cargo install during MCP initialization because it can block the client startup path." \
    "" \
    "Run a normal Jig command once to populate the cache:" \
    "  scripts/jig check contract" \
    "" >&2
[% if repo_name == "jig-sh" %]
  printf '%s\n' \
    "For the jig-sh source checkout, you can also build directly:" \
    "  cargo build -p jig-sh --bin jig" >&2
[% endif %]
  exit 1
}

# Keep repo-contract, work, and agent commands on stripped builds. MCP startup
# resolves a prebuilt binary without invoking the installer.
version_checked=false
case "${1:-}" in
  mcp)
    bin_path="$(resolve_mcp_binary)" || exit $?
    # resolve_mcp_binary either uses the installer for JIG_DEV_BIN or returns
    # a candidate whose version has already been checked.
    version_checked=true
    ;;
  dev | proxy)
    if jig_help_requested_before_separator "$@"; then
      resolve_or_install_help_binary
    else
      bin_path="$("$INSTALLER" --profile default)"
    fi
    ;;
  *)
    if jig_help_requested_before_separator "$@"; then
      resolve_or_install_help_binary
    else
      bin_path="$("$INSTALLER" --profile runtime)"
    fi
    ;;
esac

if [ "$version_checked" != true ]; then
  actual_version="$(binary_version "$bin_path" || true)"

  if [ "$actual_version" != "$JIG_VERSION" ]; then
    printf '%s\n' "Expected jig version $JIG_VERSION but resolved $actual_version from $bin_path." >&2
    exit 1
  fi
fi

# Repo-local commands run with the binary's working directory set to the owning
# repository, even when this launcher is invoked by absolute path from another
# cwd. Commands that accept caller-relative paths must explicitly opt into
# JIG_INVOKE_CWD before the cd below. This switch assumes the subcommand is
# the first positional argument; update it if global flags before subcommands
# are added.
case "${1:-}" in
  init | adopt | update)
    export JIG_INVOKE_CWD="$PWD"
    ;;
  *)
    unset JIG_INVOKE_CWD
    ;;
esac
cd "$ROOT_DIR"
exec "$bin_path" "$@"
"# },
    EmbeddedTemplateFile { relative_path: "scripts/new-checkout.sh.jinja", contents: r#"#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
PARENT_DIR="$(dirname "$REPO_ROOT")"
REPO_BASENAME="$(basename "$REPO_ROOT")"

REMOTE_URL="$(git -C "$REPO_ROOT" remote get-url origin)"
CURRENT_BRANCH="$(git -C "$REPO_ROOT" rev-parse --abbrev-ref HEAD)"

n=1
while [[ -d "$PARENT_DIR/${REPO_BASENAME}-checkout-$n" ]]; do
  ((n++))
done

CHECKOUT_DIR="$PARENT_DIR/${REPO_BASENAME}-checkout-$n"

echo "==> Cloning $REMOTE_URL (branch: $CURRENT_BRANCH) into $CHECKOUT_DIR"
git clone --branch "$CURRENT_BRANCH" "$REMOTE_URL" "$CHECKOUT_DIR"

if [[ -f "$REPO_ROOT/.env" ]]; then
  echo "==> Copying .env"
  cp "$REPO_ROOT/.env" "$CHECKOUT_DIR/.env"
fi

echo "==> Running scripts/jig bootstrap in $CHECKOUT_DIR"
(cd "$CHECKOUT_DIR" && scripts/jig bootstrap)

echo
echo "Done! Checkout ready at: $CHECKOUT_DIR"
"# },
];
