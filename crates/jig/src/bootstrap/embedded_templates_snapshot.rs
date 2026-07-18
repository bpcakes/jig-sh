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
      - <<[ (root ~ "/**") | tojson ]>>
[% endfor %]
      - "scripts/jig"
      - "scripts/install-jig.sh"
      - ".github/workflows/agent-map-check.yml"
  push:
    branches:
      - <<[ default_branch | tojson ]>>
    paths:
      - "AGENTS.md"
      - "**/AGENTS.md"
      - "agent-map.md"
[% for root in rust_crate_roots %]
      - <<[ (root ~ "/**") | tojson ]>>
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
    runs-on: <<[ ci_github_runner | tojson ]>>
    defaults:
      run:
        shell: bash
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
      - <<[ (root ~ "/**") | tojson ]>>
[% endfor %]
      - ".jig.toml"
      - ".agent/jig-contract.json"
      - "scripts/jig"
      - "scripts/install-jig.sh"
[% if sqlx_enabled %]
      - <<[ (rust_migration_dir ~ "/**") | tojson ]>>
[% endif %]
      - ".github/workflows/repo-policy.yml"
  push:
    branches:
      - <<[ default_branch | tojson ]>>
    paths:
[% for root in rust_crate_roots %]
      - <<[ (root ~ "/**") | tojson ]>>
[% endfor %]
      - ".jig.toml"
      - ".agent/jig-contract.json"
      - "scripts/jig"
      - "scripts/install-jig.sh"
[% if sqlx_enabled %]
      - <<[ (rust_migration_dir ~ "/**") | tojson ]>>
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
    runs-on: <<[ ci_github_runner | tojson ]>>
    defaults:
      run:
        shell: bash
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
    runs-on: <<[ ci_github_runner | tojson ]>>
    defaults:
      run:
        shell: bash
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
        env:
          JIG_DEFAULT_BRANCH: <<[ default_branch | tojson ]>>
        run: |
          set -euo pipefail
          if git rev-parse --verify "origin/$JIG_DEFAULT_BRANCH" >/dev/null 2>&1; then
            base_ref="$(git merge-base HEAD "origin/$JIG_DEFAULT_BRANCH")"
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
    runs-on: <<[ ci_github_runner | tojson ]>>
    defaults:
      run:
        shell: bash
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
    runs-on: <<[ ci_github_runner | tojson ]>>
    defaults:
      run:
        shell: bash
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
        env:
          JIG_DEFAULT_BRANCH: <<[ default_branch | tojson ]>>
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
            if git rev-parse --verify "origin/$JIG_DEFAULT_BRANCH" >/dev/null 2>&1; then
              ref="$(git merge-base HEAD "origin/$JIG_DEFAULT_BRANCH")"
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
      - <<[ (root ~ "/**") | tojson ]>>
[% endfor %]
[% if sqlx_enabled %]
      - <<[ (rust_migration_dir ~ "/**") | tojson ]>>
      - <<[ (rust_sqlx_metadata_dir ~ "/**") | tojson ]>>
[% endif %]
      - "Cargo.toml"
      - "Cargo.lock"
      - "rust-toolchain"
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
      - <<[ default_branch | tojson ]>>
    paths:
[% for root in rust_crate_roots %]
      - <<[ (root ~ "/**") | tojson ]>>
[% endfor %]
[% if sqlx_enabled %]
      - <<[ (rust_migration_dir ~ "/**") | tojson ]>>
      - <<[ (rust_sqlx_metadata_dir ~ "/**") | tojson ]>>
[% endif %]
      - "Cargo.toml"
      - "Cargo.lock"
      - "rust-toolchain"
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
    runs-on: <<[ ci_github_runner | tojson ]>>
    defaults:
      run:
        shell: bash
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
    runs-on: <<[ ci_github_runner | tojson ]>>
    defaults:
      run:
        shell: bash
[% if sqlx_enabled %]
    env:
      SQLX_OFFLINE: "true"
      SQLX_OFFLINE_DIR: <<[ ("${{ github.workspace }}/" ~ rust_sqlx_metadata_dir) | tojson ]>>
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
    runs-on: <<[ ci_github_runner | tojson ]>>
    defaults:
      run:
        shell: bash
[% if sqlx_enabled %]
    env:
      SQLX_OFFLINE: "true"
      SQLX_OFFLINE_DIR: <<[ ("${{ github.workspace }}/" ~ rust_sqlx_metadata_dir) | tojson ]>>
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
  # Classic required status checks can remain pending when this path-filtered
  # workflow is skipped. Prefer repository rules that require this workflow;
  # otherwise remove pull_request.paths before making its job a required check.
  pull_request:
    paths:
[% for app in frontend_apps %]
      - <<[ (app.dir ~ "/**") | tojson ]>>
[% endfor %]
      - "scripts/check-webapps.sh"
      - "scripts/check-webapp-scripts.mjs"
      - "scripts/enforce-coverage.cjs"
      - "package.json"
      - "**/package.json"
      - "**/package.json5"
      - "**/package.yaml"
      - "**/*.patch"
      - "**/*.diff"
      - ".node-version"
      - ".npmrc"
      - "**/.node-version"
      - "**/.npmrc"
      - ".yarnrc"
      - ".yarnrc.yml"
      - "**/.yarnrc"
      - "**/.yarnrc.yml"
      - ".yarn/patches/**"
      - ".yarn/cache/**"
      - ".yarn/install-state.gz"
      - ".yarn/plugins/**"
      - ".yarn/releases/**"
      - ".yarn/unplugged/**"
      - "**/.yarn/patches/**"
      - "**/.yarn/cache/**"
      - "**/.yarn/install-state.gz"
      - "**/.yarn/plugins/**"
      - "**/.yarn/releases/**"
      - "**/.yarn/unplugged/**"
      - ".pnp.cjs"
      - ".pnp.data.json"
      - ".pnp.js"
      - ".pnp.loader.mjs"
      - "patches/**"
      - "bunfig.toml"
      - "bun.lock"
      - "bun.lockb"
      - "npm-shrinkwrap.json"
      - "package-lock.json"
      - "pnpm-lock.yaml"
      - "pnpm-workspace.yaml"
      - ".pnpmfile.cjs"
      - "pnpmfile.cjs"
      - "yarn.lock"
      - ".github/workflows/webapp-checks.yml"
  push:
    branches:
      - <<[ default_branch | tojson ]>>
    paths:
[% for app in frontend_apps %]
      - <<[ (app.dir ~ "/**") | tojson ]>>
[% endfor %]
      - "scripts/check-webapps.sh"
      - "scripts/check-webapp-scripts.mjs"
      - "scripts/enforce-coverage.cjs"
      - "package.json"
      - "**/package.json"
      - "**/package.json5"
      - "**/package.yaml"
      - "**/*.patch"
      - "**/*.diff"
      - ".node-version"
      - ".npmrc"
      - "**/.node-version"
      - "**/.npmrc"
      - ".yarnrc"
      - ".yarnrc.yml"
      - "**/.yarnrc"
      - "**/.yarnrc.yml"
      - ".yarn/patches/**"
      - ".yarn/cache/**"
      - ".yarn/install-state.gz"
      - ".yarn/plugins/**"
      - ".yarn/releases/**"
      - ".yarn/unplugged/**"
      - "**/.yarn/patches/**"
      - "**/.yarn/cache/**"
      - "**/.yarn/install-state.gz"
      - "**/.yarn/plugins/**"
      - "**/.yarn/releases/**"
      - "**/.yarn/unplugged/**"
      - ".pnp.cjs"
      - ".pnp.data.json"
      - ".pnp.js"
      - ".pnp.loader.mjs"
      - "patches/**"
      - "bunfig.toml"
      - "bun.lock"
      - "bun.lockb"
      - "npm-shrinkwrap.json"
      - "package-lock.json"
      - "pnpm-lock.yaml"
      - "pnpm-workspace.yaml"
      - ".pnpmfile.cjs"
      - "pnpmfile.cjs"
      - "yarn.lock"
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
    runs-on: <<[ ci_github_runner | tojson ]>>
    defaults:
      run:
        shell: bash
    env:
      APP_DIR: ${{ matrix.app.dir }}
    strategy:
      fail-fast: false
      matrix:
        app:
[% for app in frontend_apps %]
          - name: <<[ app.name | tojson ]>>
            dir: <<[ app.dir | tojson ]>>
            coverage_threshold: <<[ app.coverage_threshold ]>>
[% endfor %]
    steps:
      - name: Checkout
        uses: actions/checkout@v6
      - name: Bootstrap Node for dependency metadata
        uses: actions/setup-node@v5
        with:
          node-version: <<[ node_version | tojson ]>>
      - name: Resolve Node version file
        id: node-version
        shell: bash
        run: |
          if node_version_file="$(scripts/check-webapps.sh node-version-file "$APP_DIR")"; then
            :
          else
            status=$?
            if [ "$status" -eq 1 ]; then
              : "${RUNNER_TEMP:?GitHub Actions did not provide RUNNER_TEMP}"
              fallback_node_version_dir="$(umask 077 && mktemp -d "$RUNNER_TEMP/jig-node-version.XXXXXX")" || exit $?
              node_version_file="$fallback_node_version_dir/node-version"
              (umask 077; set -o noclobber; printf '%s\n' '<<[ node_version ]>>' > "$node_version_file") || exit $?
            else
              exit "$status"
            fi
          fi
          printf 'path=%s\n' "$node_version_file" >> "$GITHUB_OUTPUT"
[% if web_package_manager == "bun" %]
      - name: Setup Bun
        uses: oven-sh/setup-bun@v2
        with:
          bun-version: <<[ web_package_manager_version | tojson ]>>
      - name: Setup Node
        uses: actions/setup-node@v5
        with:
          node-version-file: ${{ steps.node-version.outputs.path }}
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
          node-version-file: ${{ steps.node-version.outputs.path }}
          cache: <<[ web_package_manager ]>>
          cache-dependency-path: |
            npm-shrinkwrap.json
            package-lock.json
            ${{ matrix.app.dir }}/npm-shrinkwrap.json
            ${{ matrix.app.dir }}/package-lock.json
      - name: Pin npm
        run: npm install --global npm@<<[ web_package_manager_version ]>>
[% elif web_package_manager == "pnpm" %]
      - name: Setup Node
        uses: actions/setup-node@v5
        with:
          node-version-file: ${{ steps.node-version.outputs.path }}
      - name: Enable Corepack
        shell: bash
        run: |
          corepack enable
          package_manager_spec="$(scripts/check-webapps.sh package-manager-spec "$APP_DIR")" || exit $?
          corepack prepare "$package_manager_spec" --activate
      # setup-node cache detection for pnpm needs Corepack shims first.
      - name: Configure Node dependency cache
        uses: actions/setup-node@v5
        with:
          node-version-file: ${{ steps.node-version.outputs.path }}
          cache: <<[ web_package_manager ]>>
          cache-dependency-path: |
            pnpm-lock.yaml
            ${{ matrix.app.dir }}/pnpm-lock.yaml
[% elif web_package_manager == "yarn" %]
      - name: Setup Node
        uses: actions/setup-node@v5
        with:
          node-version-file: ${{ steps.node-version.outputs.path }}
      - name: Enable Corepack
        shell: bash
        run: |
          corepack enable
          package_manager_spec="$(scripts/check-webapps.sh package-manager-spec "$APP_DIR")" || exit $?
          corepack prepare "$package_manager_spec" --activate
      # setup-node cache detection for yarn needs Corepack shims first.
      - name: Configure Node dependency cache
        uses: actions/setup-node@v5
        with:
          node-version-file: ${{ steps.node-version.outputs.path }}
          cache: <<[ web_package_manager ]>>
          cache-dependency-path: |
            yarn.lock
            ${{ matrix.app.dir }}/yarn.lock
[% endif %]
[% endif %]
      - name: Validate package scripts
        run: node scripts/check-webapp-scripts.mjs "$APP_DIR" lint typecheck build:bundle test:coverage
      - name: Install dependencies
        shell: bash
        run: scripts/check-webapps.sh dependencies-install "$APP_DIR"
      - name: Run lint
        run: scripts/check-webapps.sh run-script "$APP_DIR" lint
      - name: Run typecheck
        run: scripts/check-webapps.sh run-script "$APP_DIR" typecheck
      - name: Run build
        run: scripts/check-webapps.sh run-script "$APP_DIR" build:bundle
      - name: Run tests with coverage
        run: scripts/check-webapps.sh run-script "$APP_DIR" test:coverage
      - name: Enforce coverage threshold
        run: |
          COVERAGE_DIR="$APP_DIR/coverage" \
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
    runs-on: <<[ ci_github_runner | tojson ]>>
    defaults:
      run:
        shell: bash
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

# Default local SQLite database and transient sidecars
/<<[ repo_name | replace("-", "_") ]>>.db
/<<[ repo_name | replace("-", "_") ]>>.db-*

# JavaScript and TypeScript
node_modules/
.pnp.*
.yarn/*
!.yarn/patches
!.yarn/plugins
!.yarn/releases
!.yarn/sdks
!.yarn/versions
coverage/
dist/
build/
.vite/
.turbo/
.astro/

# Jig local runtime cache. Keep durable agent state tracked.
.agent/.cache/*
!.agent/.cache/.gitignore
.agent/tmp/

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
kind = "<<[ app.kind | replace("\\", "\\\\") | replace("\"", "\\\"") ]>>"
role = "<<[ app.role | replace("\\", "\\\\") | replace("\"", "\\\"") ]>>"
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
[% if web_package_manager == "npm" %]
argv = ["npm", "--prefix=.", "--workspace=.", "--workspaces=true", "--include-workspace-root=true", "--global=false", "--location=project", "--if-present=false", "--include=dev", "--include=optional", "--include=peer", "run", "dev"]
[% else %]
argv = ["<<[ web_package_manager | replace("\\", "\\\\") | replace("\"", "\\\"") ]>>", "run", "dev"]
[% endif %]
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
Generated install steps select the package-manager project from workspace membership, not root-lock presence: npm, pnpm, Bun, and Yarn Classic members use the root project, while standalone apps and nested Yarn Berry projects use their app-local project.
Generated npm installs pin real writes, lock creation, workspace participation, platform packages, executable links, and all dependency classes despite ambient install-shaping variables; registry/authentication, dependency layout, peer resolution, and install-script approval remain project-owned.
Run package-local scripts through `scripts/check-webapps.sh run-script <app-dir> <script>` for the generated execution boundary; generated web and browser-E2E workflows do this for every package script. For npm it pins the exact current package and required-script behavior despite ambient npm selectors; explicit app environment plus registry, authentication, layout, peer, and lifecycle policy remain project-owned.
The dependency checker supports stock macOS Bash 3.2, treats Yarn authority-enumeration failures as hard errors, and follows Bash-owned worker-job identity after an interrupted `wait` rather than probing a reaped PID.
Dependency readiness treats a missing, empty, or exact cache-only real `node_modules` root equivalently. It ignores only real top-level tool-cache directories (`.cache`, `.vite`, `.vite-temp`, and `.tmp`) plus a regular `.DS_Store`; unknown entries, nested/type-replaced cache names, package entries, metadata, symlinks, and launchers remain attested.
`[[frontend_apps]]` keeps CI, coverage, and semantic role metadata; generated `[[dev.apps]]` drives `scripts/jig dev` and takes precedence for local dev settings. When both sections are present, Jig requires a matching dev app name and dir for every frontend app. `kind` selects `vite` versus `env-port`; `role` selects `spa`, `admin`, or `astro` without guessing from the directory name. Extra `[[dev.apps]]` entries without `[[frontend_apps]]` are treated as dev-only and are not covered by generated web CI.
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
    EmbeddedTemplateFile { relative_path: "scripts/check-webapps.sh.jinja", contents: r##"#!/usr/bin/env bash
set -euo pipefail

mode="${1:-}"
node_bin="${NODE:-node}"
install_lock_path=".agent/tmp/web-dependencies.lock"
install_lock_token=""
install_worker_pid=""
install_worker_group=""
install_worker_signal=""
recovery_claim_path=""
recovery_claim_token=""
dependency_stamp_dir=".agent/tmp/web-dependencies"

usage() {
  echo "Usage: scripts/check-webapps.sh bootstrap|dependencies-install <app-dir>|dependencies-bootstrap <app-dir>|dependencies-ready <app-dir>|run-script <app-dir> <script>|lint|typecheck|build|coverage|node-version-file <app-dir>[% if web_package_manager == "pnpm" or web_package_manager == "yarn" %]|package-manager-spec <app-dir>[% endif %]" >&2
}

# Public query-mode exit contract:
#   0: verified value/readiness
#   1: verified absence or stale dependency proof
#   2+: invalid or unverifiable repository/package-manager authority

root_lockfile() {
[% if web_package_manager == "bun" %]
  if [ -f bun.lock ]; then printf '%s\n' "bun.lock"; elif [ -f bun.lockb ]; then printf '%s\n' "bun.lockb"; else return 1; fi
[% elif web_package_manager == "npm" %]
  if [ -f npm-shrinkwrap.json ]; then printf '%s\n' "npm-shrinkwrap.json"; elif [ -f package-lock.json ]; then printf '%s\n' "package-lock.json"; else return 1; fi
[% elif web_package_manager == "pnpm" %]
  [ -f pnpm-lock.yaml ] && printf '%s\n' "pnpm-lock.yaml"
[% elif web_package_manager == "yarn" %]
  [ -f yarn.lock ] && printf '%s\n' "yarn.lock"
[% endif %]
}

app_lockfile() {
  local app_dir="$1"
[% if web_package_manager == "bun" %]
  if [ -f "$app_dir/bun.lock" ]; then printf '%s\n' "$app_dir/bun.lock"; elif [ -f "$app_dir/bun.lockb" ]; then printf '%s\n' "$app_dir/bun.lockb"; else return 1; fi
[% elif web_package_manager == "npm" %]
  if [ -f "$app_dir/npm-shrinkwrap.json" ]; then printf '%s\n' "$app_dir/npm-shrinkwrap.json"; elif [ -f "$app_dir/package-lock.json" ]; then printf '%s\n' "$app_dir/package-lock.json"; else return 1; fi
[% elif web_package_manager == "pnpm" %]
  [ -f "$app_dir/pnpm-lock.yaml" ] && printf '%s\n' "$app_dir/pnpm-lock.yaml"
[% elif web_package_manager == "yarn" %]
  [ -f "$app_dir/yarn.lock" ] && printf '%s\n' "$app_dir/yarn.lock"
[% endif %]
}

workspace_metadata() {
  local operation="$1"
  shift

  "$node_bin" - --jig-workspace-metadata "$operation" "$@" <<'NODE'
const { createHash } = require("node:crypto");
const { spawnSync } = require("node:child_process");
const fs = require("node:fs");
const path = require("node:path");

const manager = "<<[ web_package_manager ]>>";
const operation = process.argv[3];
const operationArguments = process.argv.slice(4);

function fail(message) {
  console.error(`Cannot determine ${manager} dependency scope: ${message}`);
  process.exit(2);
}

[% if web_package_manager == "pnpm" %]
// BEGIN JIG PACKAGE MANAGER METADATA LAUNCHER
const JIG_PACKAGE_MANAGER_METADATA_ARGUMENTS = [
  ["--version"],
  ["cache", "dir", "--silent"],
  ["config", "--json"],
  ["config", "list", "--json"],
  [
    "pkg",
    "get",
    "name",
    "dependencies",
    "devDependencies",
    "optionalDependencies",
    "peerDependencies",
    "pnpm",
    "--json",
  ],
];

function packageManagerMetadataEnvironmentValue(environment, name) {
  const key = Object.keys(environment).find((candidate) => candidate.toUpperCase() === name);
  return key ? environment[key] : undefined;
}

function validateWindowsCommandInterpreter(interpreter) {
  const driveAbsolute =
    typeof interpreter === "string" && /^[A-Za-z]:[\\/]/.test(interpreter);
  const uncAbsolute =
    typeof interpreter === "string" &&
    /^[\\/]{2}[^\\/]+[\\/][^\\/]+(?:[\\/].*)?$/.test(interpreter);
  if (
    !interpreter ||
    /[\0-\x1f\x7f"]/.test(interpreter) ||
    /^[\\/]{2}[?.][\\/]/.test(interpreter) ||
    (!driveAbsolute && !uncAbsolute)
  ) {
    throw new Error("unsafe Windows command interpreter");
  }
  return interpreter;
}

function validatePackageManagerMetadataExecutable(executable) {
  if (
    typeof executable !== "string" ||
    executable.length === 0 ||
    /[\0-\x1f\x7f"]/.test(executable)
  ) {
    throw new Error("unsafe package-manager metadata executable");
  }
  return executable;
}

function windowsPackageManagerMetadataPathEntries(value) {
  const entries = [];
  let entry = "";
  let quoted = false;
  for (const character of value) {
    if (character === '"') {
      quoted = !quoted;
    } else if (character === path.delimiter && !quoted) {
      entries.push(entry);
      entry = "";
    } else {
      entry += character;
    }
  }
  if (quoted) throw new Error("unsafe quoted Windows PATH");
  entries.push(entry);
  return entries;
}

function windowsPackageManagerMetadataExtensions(requested, configured) {
  if (path.extname(requested)) return [""];
  const raw = configured === undefined ? ".COM;.EXE;.BAT;.CMD" : configured;
  const extensions = raw.split(";").filter((extension) => extension !== "");
  if (
    extensions.length === 0 ||
    extensions.some((extension) => !/^\.[A-Za-z0-9]+$/.test(extension))
  ) throw new Error("unsafe Windows PATHEXT");
  return [...new Map(extensions.map((extension) => [extension.toUpperCase(), extension])).values()];
}

function resolveWindowsPackageManagerMetadataExecutable(executable, options = {}) {
  const fs = require("node:fs");
  const path = require("node:path");
  const requested = validatePackageManagerMetadataExecutable(executable);
  const environment = options.env || process.env;
  const workingDirectory = typeof options.cwd === "string"
    ? path.resolve(options.cwd)
    : process.cwd();
  let candidates = [];
  const searchedPath = !path.isAbsolute(requested);

  if (path.isAbsolute(requested)) {
    candidates = [requested];
  } else {
    if (/[\\/]/.test(requested)) {
      throw new Error("package-manager metadata executable must be absolute or a bare command");
    }
    const pathValue = packageManagerMetadataEnvironmentValue(environment, "PATH") || "";
    const configuredExtensions = packageManagerMetadataEnvironmentValue(environment, "PATHEXT");
    const extensions = windowsPackageManagerMetadataExtensions(requested, configuredExtensions);
    for (const entry of windowsPackageManagerMetadataPathEntries(pathValue)) {
      const directory = entry
        ? (path.isAbsolute(entry) ? entry : path.resolve(workingDirectory, entry))
        : workingDirectory;
      for (const extension of extensions) {
        candidates.push(path.resolve(directory, `${requested}${extension}`));
      }
    }
  }

  for (const candidate of candidates) {
    try {
      if (!fs.statSync(candidate).isFile()) continue;
      const resolved = fs.realpathSync.native(candidate);
      if (!fs.statSync(resolved).isFile()) continue;
      return validatePackageManagerMetadataExecutable(resolved);
    } catch (error) {
      if (!searchedPath) throw error;
    }
  }
  throw new Error("package-manager metadata executable was not found");
}

function encodePackageManagerMetadataBatchArgument(argument, forceQuote = false) {
  if (/[\0\r\n]/.test(argument)) {
    throw new Error("unsafe package-manager metadata batch argument");
  }
  const safeUnquoted = "#$*+-./:?@\\_";
  const quote = forceQuote || argument.length === 0 || argument.endsWith("\\") ||
    [...argument].some((character) => {
      const code = character.codePointAt(0);
      return code < 32 || code === 127 ||
        (code < 128 && !/[A-Za-z0-9]/.test(character) && !safeUnquoted.includes(character));
    });
  let encoded = quote ? '"' : "";
  let backslashes = 0;
  for (const character of argument) {
    if (character === "\\") {
      backslashes += 1;
      encoded += character;
      continue;
    }
    if (character === '"') {
      encoded += "\\".repeat(backslashes);
      encoded += '"';
    } else if (character === "%") {
      encoded += "%%cd:~,";
    }
    backslashes = 0;
    encoded += character;
  }
  if (quote) encoded += "\\".repeat(backslashes) + '"';
  return encoded;
}

function encodePackageManagerMetadataBatchInvocation(executable, args) {
  if (/^[\\/]{2}[?.][\\/]/.test(executable) || executable.endsWith("\\")) {
    throw new Error("unsafe package-manager metadata batch executable");
  }
  let commandLine = '"' + encodePackageManagerMetadataBatchArgument(executable, true);
  for (const argument of args) {
    commandLine += ` ${encodePackageManagerMetadataBatchArgument(argument)}`;
  }
  return commandLine + '"';
}

function spawnPackageManagerMetadata(executable, args, options) {
  const allowedArguments =
    Array.isArray(args) &&
    JIG_PACKAGE_MANAGER_METADATA_ARGUMENTS.some(
      (allowed) =>
        allowed.length === args.length &&
        allowed.every((value, index) => value === args[index])
    );
  if (!allowedArguments || args.some((value) => !/^[A-Za-z0-9._-]+$/.test(value))) {
    throw new Error("unsupported package-manager metadata arguments");
  }

  const spawnOptions = { timeout: 30_000, ...options, shell: false };
  if (process.platform !== "win32") {
    return spawnSync(
      validatePackageManagerMetadataExecutable(executable),
      args,
      spawnOptions
    );
  }

  const resolved = resolveWindowsPackageManagerMetadataExecutable(executable, spawnOptions);
  if (!/\.(?:cmd|bat)$/i.test(resolved)) {
    return spawnSync(resolved, args, spawnOptions);
  }

  const environment = spawnOptions.env || process.env;
  const commandInterpreter = validateWindowsCommandInterpreter(
    packageManagerMetadataEnvironmentValue(environment, "COMSPEC")
  );
  const commandLine = encodePackageManagerMetadataBatchInvocation(resolved, args);
  return spawnSync(
    commandInterpreter,
    ["/d", "/s", "/v:off", "/c", commandLine],
    { ...spawnOptions, windowsVerbatimArguments: true }
  );
}
// END JIG PACKAGE MANAGER METADATA LAUNCHER

function pnpmMetadataEnvironment() {
  const environment = { ...process.env };
  for (const key of Object.keys(environment)) {
    if (/^(?:npm|pnpm)_config_ignore_pnpmfile$/i.test(key)) delete environment[key];
  }
  environment.NPM_CONFIG_IGNORE_PNPMFILE = "true";
  environment.PNPM_CONFIG_IGNORE_PNPMFILE = "true";
  return environment;
}

[% endif %]
function normalizeRelative(value, trimValue = true) {
  if (typeof value !== "string") return null;
  const input = trimValue ? value.trim() : value;
  const slashPath = input.replaceAll("\\", "/").replace(/^\.\/+/, "").replace(/\/+$/, "");
  if (
    !slashPath ||
    slashPath.startsWith("/") ||
    /^[A-Za-z]:\//.test(slashPath) ||
    /[\0\r\n]/.test(slashPath)
  ) return null;
  const segments = slashPath.split("/").filter((segment) => segment && segment !== ".");
  if (segments.length === 0 || segments.some((segment) => segment === "..")) return null;
  return segments.join("/");
}

function workspacePatternsFromPackageJson() {
  if (!fs.existsSync("package.json")) return [];
  let packageJson;
  try {
    packageJson = JSON.parse(fs.readFileSync("package.json", "utf8"));
  } catch (error) {
    fail(`package.json is not valid JSON (${error.message})`);
  }
  const workspaces = packageJson.workspaces;
  if (workspaces === undefined) return [];
[% if web_package_manager != "yarn" %]
  // Yarn Classic's object form is not workspace authority for npm or Bun.
  // Treat it as unrelated metadata so it cannot pull an app into root scope.
  if (typeof workspaces === "object" && workspaces !== null && !Array.isArray(workspaces)) {
    return [];
  }
[% endif %]
  const entries = Array.isArray(workspaces)
    ? workspaces
[% if web_package_manager == "yarn" %]
    : workspaces?.packages;
[% else %]
    : undefined;
[% endif %]
  if (!Array.isArray(entries) || entries.some((entry) => typeof entry !== "string")) {
    fail("package.json workspaces must be an array of string globs[% if web_package_manager == "yarn" %] (or an object with a string-array packages field)[% endif %]");
  }
  return entries;
}

function stripUnquotedYamlComment(value) {
  let quote = null;
  for (let index = 0; index < value.length; index += 1) {
    const character = value[index];
    if (quote === '"' && character === "\\") {
      index += 1;
    } else if (quote === "'" && character === "'" && value[index + 1] === "'") {
      index += 1;
    } else if (quote && character === quote) {
      quote = null;
    } else if (!quote && (character === '"' || character === "'")) {
      quote = character;
    } else if (!quote && character === "#" && (index === 0 || /\s/.test(value[index - 1]))) {
      return value.slice(0, index).trimEnd();
    }
  }
  return value;
}

function yamlString(value, description, rejectNodeProperties = false) {
  const trimmed = stripUnquotedYamlComment(value).trim();
  if (!trimmed) return null;
  if (trimmed.startsWith('"')) {
    if (!trimmed.endsWith('"')) fail(`pnpm-workspace.yaml contains an unterminated double-quoted ${description}`);
    try {
      const parsed = JSON.parse(trimmed);
      if (typeof parsed !== "string") fail(`pnpm-workspace.yaml ${description} must be a string`);
      return parsed;
    } catch (error) {
      fail(`pnpm-workspace.yaml contains an invalid double-quoted ${description} (${error.message})`);
    }
  }
  if (trimmed.startsWith("'")) {
    if (!trimmed.endsWith("'")) fail(`pnpm-workspace.yaml contains an unterminated single-quoted ${description}`);
    return trimmed.slice(1, -1).replaceAll("''", "'");
  }
  if (rejectNodeProperties && /^(?:[&*!]|[>|])/.test(trimmed)) {
    fail(`pnpm-workspace.yaml ${description} uses unsupported YAML node properties or block scalars`);
  }
  if (
    rejectNodeProperties &&
    (/^[\[\]{}]/.test(trimmed) || /^-(?:\s|$)/.test(trimmed) || /:(?:\s|$)/.test(trimmed))
  ) {
    fail(`pnpm-workspace.yaml ${description} uses unsupported YAML collection syntax`);
  }
  if (/^(?:null|~|true|false|[-+]?(?:\d+\.?\d*|\.\d+))$/i.test(trimmed)) {
    fail(`pnpm-workspace.yaml ${description} must be a string`);
  }
  return trimmed || null;
}

function flowItems(value) {
  if (!value.trim()) return [];
  const items = [];
  let current = "";
  let quote = null;
  for (let index = 0; index < value.length; index += 1) {
    const character = value[index];
    if (quote) {
      current += character;
      if (character === quote && value[index - 1] !== "\\") quote = null;
    } else if (character === '"' || character === "'") {
      quote = character;
      current += character;
    } else if (character === ",") {
      items.push(current);
      current = "";
    } else {
      current += character;
    }
  }
  items.push(current);
  if (items.length > 1 && items.at(-1).trim() === "") items.pop();
  return items.map((item) => {
    const scalar = yamlString(item, "packages entry", true);
    if (scalar === null) fail("pnpm-workspace.yaml packages contains an empty glob");
    return scalar;
  });
}

function yamlMappingEntry(value, description) {
  let quote = null;
  for (let index = 0; index < value.length; index += 1) {
    const character = value[index];
    if (quote === '"' && character === "\\") {
      index += 1;
    } else if (quote === "'" && character === "'" && value[index + 1] === "'") {
      index += 1;
    } else if (quote && character === quote) {
      quote = null;
    } else if (!quote && (character === '"' || character === "'")) {
      quote = character;
    } else if (!quote && character === ":" && (index + 1 === value.length || /\s/.test(value[index + 1]))) {
      const key = yamlString(value.slice(0, index), `${description} selector`, true);
      const mappedValue = yamlString(value.slice(index + 1), `${description} path`, true);
      if (!key || !mappedValue) fail(`pnpm-workspace.yaml ${description} entries require a selector and path`);
      return [key, mappedValue];
    }
  }
  fail(`pnpm-workspace.yaml ${description} must be a block mapping of selectors to paths`);
}

function pnpmWorkspaceMetadata(workspacePath = "pnpm-workspace.yaml") {
  let stats;
  try {
    stats = fs.lstatSync(workspacePath);
  } catch (error) {
    if (error?.code === "ENOENT") {
      return { packages: [], patchedDependencies: [], hasPatchedDependencies: false };
    }
    throw error;
  }
  if (!stats.isFile() || stats.isSymbolicLink()) {
    fail(`${workspacePath} must be a real file`);
  }

  let source = fs.readFileSync(workspacePath, "utf8");
  if (source.startsWith("\uFEFF")) source = source.slice(1);
  const lines = source.split(/\r?\n/);
  const packages = [];
  const patchedDependencies = [];
  let foundPackages = false;
  let foundPatchedDependencies = false;

  function lineIndent(line, description) {
    const spaces = line.match(/^ */)[0].length;
    if (line[spaces] === "\t") {
      fail(`pnpm-workspace.yaml ${description} must use spaces for indentation`);
    }
    return spaces;
  }

  let sawLeadingDocumentMarker = false;
  let sawDocumentContent = false;
  for (const line of lines) {
    const content = stripUnquotedYamlComment(line).trim();
    if (!content) continue;
    if (content === "---") {
      if (sawLeadingDocumentMarker || sawDocumentContent || lineIndent(line, "document marker") !== 0) {
        fail("pnpm-workspace.yaml multiple YAML documents are unsupported");
      }
      sawLeadingDocumentMarker = true;
      continue;
    }
    if (content === "...") {
      fail("pnpm-workspace.yaml YAML document end markers are unsupported");
    }
    sawDocumentContent = true;
  }

  const substantiveLines = lines.filter((line) => {
    const content = stripUnquotedYamlComment(line).trim();
    return content && content !== "---" && content !== "...";
  });
  const rootIndent = substantiveLines.length === 0
    ? 0
    : Math.min(...substantiveLines.map((line) => lineIndent(line, "root mapping")));

  function rootMappingEntry(line) {
    const content = line.slice(rootIndent);
    if (content.startsWith("{") || content.startsWith("[")) {
      fail("pnpm-workspace.yaml root flow mappings and sequences are unsupported");
    }
    if (/^-(?:\s|$)/.test(content)) {
      fail("pnpm-workspace.yaml root block sequences are unsupported");
    }
    if (/^[?:](?:\s|$)/.test(content)) {
      fail("pnpm-workspace.yaml explicit mapping keys are unsupported");
    }
    let quote = null;
    for (let index = 0; index < content.length; index += 1) {
      const character = content[index];
      if (quote === '"' && character === "\\") {
        index += 1;
      } else if (quote === "'" && character === "'" && content[index + 1] === "'") {
        index += 1;
      } else if (quote && character === quote) {
        quote = null;
      } else if (!quote && (character === '"' || character === "'")) {
        quote = character;
      } else if (!quote && character === ":" && (index + 1 === content.length || /\s/.test(content[index + 1]))) {
        const key = yamlString(content.slice(0, index), "root mapping key", true);
        if (!key) fail("pnpm-workspace.yaml root mapping contains an empty key");
        return [key, content.slice(index + 1)];
      }
    }
    fail("pnpm-workspace.yaml root must be a block mapping");
  }

  function blockEntries(startIndex, description, parseEntry) {
    const entries = [];
    let itemIndent = null;
    let index = startIndex;
    for (; index < lines.length; index += 1) {
      const line = lines[index];
      if (!line.trim() || line.trimStart().startsWith("#")) continue;
      const indentation = lineIndent(line, description);
      if (indentation <= rootIndent) break;
      if (itemIndent === null) itemIndent = indentation;
      if (indentation !== itemIndent) {
        fail(`pnpm-workspace.yaml ${description} contains unsupported nested YAML`);
      }
      entries.push(parseEntry(line.slice(indentation).trimEnd()));
    }
    return [entries, index];
  }

  for (let index = 0; index < lines.length; index += 1) {
    const line = lines[index];
    const trimmed = stripUnquotedYamlComment(line).trim();
    if (!trimmed || trimmed === "---" || trimmed === "...") continue;
    const indentation = lineIndent(line, "root mapping");
    if (indentation !== rootIndent) continue;
    const [key, rawValue] = rootMappingEntry(line);
    if (key === "<<") {
      fail("pnpm-workspace.yaml root YAML merges are unsupported");
    }

    if (key === "packages") {
      if (foundPackages) fail("pnpm-workspace.yaml declares packages more than once");
      foundPackages = true;
      const inline = stripUnquotedYamlComment(rawValue).trim();
      if (inline.startsWith("[") && inline.endsWith("]")) {
        packages.push(...flowItems(inline.slice(1, -1)));
      } else if (inline) {
        fail("pnpm-workspace.yaml packages must be a block or flow sequence of string globs");
      } else {
        const [entries, nextIndex] = blockEntries(index + 1, "packages", (entry) => {
          const item = entry.match(/^-(?:\s+(.*?)\s*|\s*)$/);
          if (!item) fail("pnpm-workspace.yaml packages contains a non-string sequence entry");
          const scalar = yamlString(item[1] ?? "", "packages entry", true);
          if (!scalar) fail("pnpm-workspace.yaml packages contains an empty or invalid glob");
          return scalar;
        });
        packages.push(...entries);
        index = nextIndex - 1;
      }
      continue;
    }

    if (key === "patchedDependencies") {
      if (foundPatchedDependencies) {
        fail("pnpm-workspace.yaml declares patchedDependencies more than once");
      }
      foundPatchedDependencies = true;
      if (stripUnquotedYamlComment(rawValue).trim()) {
        fail("pnpm-workspace.yaml patchedDependencies must be a block mapping of selectors to paths");
      }
      const [entries, nextIndex] = blockEntries(index + 1, "patchedDependencies", (entry) =>
        yamlMappingEntry(entry, "patchedDependencies")
      );
      const selectors = new Set();
      for (const [selector] of entries) {
        if (selectors.has(selector)) {
          fail(`pnpm-workspace.yaml declares patchedDependencies selector ${JSON.stringify(selector)} more than once`);
        }
        selectors.add(selector);
      }
      patchedDependencies.push(...entries);
      index = nextIndex - 1;
    }
  }

  return { packages, patchedDependencies, hasPatchedDependencies: foundPatchedDependencies };
}

function workspacePatternsFromPnpm() {
  return pnpmWorkspaceMetadata().packages;
}

function fallbackGlobMatches(pattern, value) {
  if (/[\[\]{}()]/.test(pattern)) {
    fail(`workspace glob ${JSON.stringify(pattern)} requires Node.js 20.17 or newer`);
  }
  let expression = "^";
  for (let index = 0; index < pattern.length; index += 1) {
    const character = pattern[index];
    if (character === "*") {
      if (pattern[index + 1] === "*") {
        index += 1;
        if (pattern[index + 1] === "/") {
          index += 1;
          expression += "(?:.*/)?";
        } else {
          expression += ".*";
        }
      } else {
        expression += "[^/]*";
      }
    } else if (character === "?") {
      expression += "[^/]";
    } else {
      expression += character.replace(/[|\\{}()[\]^$+?.]/g, "\\$&");
    }
  }
  return new RegExp(`${expression}$`).test(value);
}

function globMatches(pattern, value) {
  try {
    return typeof path.matchesGlob === "function"
      ? path.matchesGlob(value, pattern)
      : fallbackGlobMatches(pattern, value);
  } catch (error) {
    fail(`unsupported workspace glob ${JSON.stringify(pattern)} (${error.message})`);
  }
}

[% if web_package_manager == "pnpm" %]
const rawPatterns = workspacePatternsFromPnpm();
[% else %]
const rawPatterns = workspacePatternsFromPackageJson();
[% endif %]
const includes = [];
const excludes = [];
const hardIgnoredComponents = new Set([
  "node_modules",
  ...(manager === "pnpm" ? ["bower_components"] : []),
]);
for (const rawPattern of rawPatterns) {
  const excluded = rawPattern.startsWith("!") && !rawPattern.startsWith("!(");
  const pattern = normalizeRelative(excluded ? rawPattern.slice(1) : rawPattern, false);
  if (!pattern) fail(`workspace glob ${JSON.stringify(rawPattern)} is not repository-relative`);
  (excluded ? excludes : includes).push(pattern);
}

function isWorkspaceMember(relative) {
  if (relative.split("/").some((component) => hardIgnoredComponents.has(component))) return false;
  const manifestCandidate = `${relative}/package.json`;
  const matchesDirectory = (pattern) => globMatches(`${pattern}/package.json`, manifestCandidate);
  return includes.some(matchesDirectory) && !excludes.some(matchesDirectory);
}

if (operation === "contains") {
  const appDir = normalizeRelative(operationArguments[0]);
  if (!appDir) process.exit(1);
  process.exit(isWorkspaceMember(appDir) ? 0 : 1);
}

if (operation === "manifests") {
  const maximumEntries = 10_000;
  let visitedEntries = 0;
  const alwaysIgnoredDirectories = new Set([
    ".agent",
    ".git",
    ".yarn",
    ...hardIgnoredComponents,
  ]);
  const manifests = [];
  const manifestNames = manager === "pnpm"
    ? ["package.json", "package.json5", "package.yaml"]
    : ["package.json"];

  function staticGlobPrefix(pattern) {
    let index = pattern.search(/[\[\]{}()*?]/);
    if (index > 0 && pattern[index] === "(" && /[!+@?*]/.test(pattern[index - 1])) index -= 1;
    return index === -1 ? null : pattern.slice(0, index);
  }

  function patternMayMatchDescendant(pattern, relative) {
    const relativeDepth = relative ? relative.split("/").length : 0;
    const patternDepth = pattern.split("/").length;
    // Every supported glob token except a globstar is confined to one path
    // segment. Stop at the pattern's fixed depth even for braces, character
    // classes, and extglobs so terminal members are never recursively scanned.
    if (!pattern.includes("**") && relativeDepth >= patternDepth) return false;
    const directoryPrefix = `${relative}/`;
    const prefix = staticGlobPrefix(pattern);
    if (prefix === null) return pattern.startsWith(directoryPrefix);
    if (!prefix) return true;
    const normalizedPrefix = prefix.replace(/\/+$/, "");
    return normalizedPrefix === relative ||
      normalizedPrefix.startsWith(directoryPrefix) ||
      relative.startsWith(`${normalizedPrefix}/`);
  }

  function excludedWholeSubtree(relative) {
    return excludes.some((pattern) => {
      if (!pattern.endsWith("/**")) return false;
      const base = pattern.slice(0, -3).replace(/\/+$/, "");
      if (!base || /[\[\]{}()*?]/.test(base)) return false;
      return relative === base || relative.startsWith(`${base}/`);
    });
  }

  function mayContainWorkspaceMember(relative) {
    if (excludedWholeSubtree(relative)) return false;
    return includes.some((pattern) => patternMayMatchDescendant(pattern, relative));
  }

  function effectiveManifest(relative) {
    for (const name of manifestNames) {
      const manifest = `${relative}/${name}`;
      let stats;
      try {
        stats = fs.lstatSync(manifest);
      } catch (error) {
        if (error?.code === "ENOENT") continue;
        throw error;
      }
      if (stats.isSymbolicLink() || !stats.isFile()) {
        fail(`workspace manifest ${JSON.stringify(manifest)} must be a real file`);
      }
      return manifest;
    }
    return null;
  }

  function walk(directory, relativeDirectory = "") {
    const entries = fs
      .readdirSync(directory, { withFileTypes: true })
      .sort((left, right) => left.name < right.name ? -1 : left.name > right.name ? 1 : 0);
    for (const entry of entries) {
      const relative = relativeDirectory ? `${relativeDirectory}/${entry.name}` : entry.name;
      if (/[\0\r\n]/.test(relative)) fail("workspace paths must not contain control characters");
      if (alwaysIgnoredDirectories.has(entry.name)) continue;
      if (!entry.isDirectory() && !entry.isSymbolicLink()) continue;
      const member = isWorkspaceMember(relative);
      const mayContainMember = mayContainWorkspaceMember(relative);
      if (!member && !mayContainMember) continue;
      visitedEntries += 1;
      if (visitedEntries > maximumEntries) {
        fail(`workspace discovery exceeded ${maximumEntries} relevant filesystem entries`);
      }
      if (entry.isSymbolicLink()) {
        fail(`workspace member ${JSON.stringify(relative)} must not be a symbolic link`);
      }
      if (!entry.isDirectory()) continue;
      if (member) {
        const manifest = effectiveManifest(relative);
        if (manifest) manifests.push(manifest);
      }
      if (mayContainMember) walk(path.join(directory, entry.name), relative);
    }
  }
  walk(".");
  process.stdout.write(manifests.sort().map((manifest) => `${manifest}\n`).join(""));
  process.exit(0);
}

if (operation === "patch-fingerprint") {
  const scope = operationArguments[0] === "." ? "." : normalizeRelative(operationArguments[0]);
  if (!scope) fail("patch dependency scope is not repository-relative");
  const runtimeMajor = operationArguments[1];
  const sharedWorkspaceLockfile = operationArguments[2];
  if (!/^[0-9]+$/.test(runtimeMajor)) fail("pnpm runtime major must be a positive integer");
  if (sharedWorkspaceLockfile !== "true" && sharedWorkspaceLockfile !== "false") {
    fail("pnpm shared-workspace-lockfile must resolve to true or false");
  }
  const major = Number(runtimeMajor);
  const shared = sharedWorkspaceLockfile === "true";
  const manifestPaths = [...new Set(operationArguments.slice(3))];
  const repositoryRoot = fs.realpathSync.native(".");

  function withinRepository(candidate) {
    const relative = path.relative(repositoryRoot, candidate);
    return relative && relative !== ".." && !relative.startsWith(`..${path.sep}`) && !path.isAbsolute(relative);
  }

  function validatedRepositoryFile(candidatePath, description, optional = false) {
    if (
      typeof candidatePath !== "string" ||
      !candidatePath ||
      /[\0\r\n]/.test(candidatePath) ||
      path.isAbsolute(candidatePath) ||
      path.win32.isAbsolute(candidatePath) ||
      candidatePath.startsWith("\\\\")
    ) {
      fail(`${description} must be repository-relative`);
    }
    const absolute = path.resolve(repositoryRoot, candidatePath);
    if (!withinRepository(absolute)) fail(`${description} escapes the repository`);

    const repositoryRelative = path.relative(repositoryRoot, absolute);
    const components = repositoryRelative.split(path.sep);
    let current = repositoryRoot;
    let stats;
    for (let index = 0; index < components.length; index += 1) {
      current = path.join(current, components[index]);
      try {
        stats = fs.lstatSync(current);
      } catch (error) {
        if (error?.code === "ENOENT" || error?.code === "ENOTDIR") {
          if (optional && index === components.length - 1 && error.code === "ENOENT") return null;
          fail(`${description} does not exist`);
        }
        throw error;
      }
      if (stats.isSymbolicLink()) fail(`${description} must not contain symbolic links`);
      if (index + 1 < components.length && !stats.isDirectory()) {
        fail(`${description} must traverse real directories`);
      }
    }
    if (!stats.isFile()) fail(`${description} must be a file`);
    const realFile = fs.realpathSync.native(absolute);
    if (!withinRepository(realFile)) fail(`${description} resolves outside the repository`);
    return { absolute, repositoryRelative };
  }

  function plainObject(value) {
    return typeof value === "object" && value !== null && !Array.isArray(value);
  }

  function readManifest(manifestPath) {
    const { absolute } = validatedRepositoryFile(manifestPath, manifestPath);
    let manifest;
    if (path.basename(manifestPath) === "package.json") {
      try {
        manifest = JSON.parse(fs.readFileSync(absolute, "utf8"));
      } catch (error) {
        fail(`${manifestPath} is not valid JSON (${error.message})`);
      }
    } else {
      const queryOptions = {
        cwd: path.dirname(absolute),
        encoding: "utf8",
        env: pnpmMetadataEnvironment(),
        maxBuffer: 1024 * 1024,
        timeout: 30_000,
      };
      const result = spawnPackageManagerMetadata(
        "pnpm",
        [
          "pkg",
          "get",
          "name",
          "dependencies",
          "devDependencies",
          "optionalDependencies",
          "peerDependencies",
          "pnpm",
          "--json",
        ],
        queryOptions
      );
      if (result.error || result.signal || result.status !== 0) {
        fail(`pnpm could not safely read alternate manifest ${manifestPath}`);
      }
      const output = result.stdout.trim();
      if (!output || Buffer.byteLength(output) > 1024 * 1024) {
        fail(`pnpm returned invalid data for alternate manifest ${manifestPath}`);
      }
      try {
        manifest = JSON.parse(output);
      } catch (error) {
        fail(`pnpm returned malformed JSON for alternate manifest ${manifestPath} (${error.message})`);
      }
      const allowed = new Set([
        "name",
        "dependencies",
        "devDependencies",
        "optionalDependencies",
        "peerDependencies",
        "pnpm",
      ]);
      if (!plainObject(manifest) || Object.keys(manifest).some((key) => !allowed.has(key))) {
        fail(`pnpm returned an invalid object for alternate manifest ${manifestPath}`);
      }
    }
    if (!plainObject(manifest)) fail(`${manifestPath} must contain a package manifest object`);
    return manifest;
  }

  function declarationFromManifest(manifestPath, manifest) {
    const exists = plainObject(manifest.pnpm) && Object.hasOwn(manifest.pnpm, "patchedDependencies");
    return {
      exists,
      sourcePath: manifestPath,
      value: exists ? manifest.pnpm.patchedDependencies : undefined,
    };
  }

  let dependencyFree = true;
  const dependencyFields = [
    "dependencies",
    "devDependencies",
    "optionalDependencies",
    "peerDependencies",
  ];
  const manifests = manifestPaths.map((manifestPath) => {
    const manifest = readManifest(manifestPath);
    for (const field of dependencyFields) {
      const dependencies = manifest[field];
      if (dependencies !== undefined && !plainObject(dependencies)) {
        fail(`${manifestPath} ${field} must be an object`);
      }
      if (dependencies && Object.keys(dependencies).length > 0) dependencyFree = false;
    }
    return {
      path: manifestPath,
      context: path.dirname(manifestPath).split(path.sep).join("/") || ".",
      declaration: declarationFromManifest(manifestPath, manifest),
    };
  });

  if (scope !== ".") {
    const scopeWorkspacePath = `${scope}/pnpm-workspace.yaml`;
    const scopeWorkspace = pnpmWorkspaceMetadata(scopeWorkspacePath);
    if (scopeWorkspace.patchedDependencies.length > 0) {
      fail(
        `${scopeWorkspacePath} declares patchedDependencies, but standalone installs use ` +
        `--ignore-workspace and would silently ignore those patches; move pnpm 10 patches to ` +
        `the app manifest, or select the app in the root workspace and declare active patches there`
      );
    }
  }

  const workspace = pnpmWorkspaceMetadata("pnpm-workspace.yaml");
  const workspaceDeclaration = {
    exists: workspace.hasPatchedDependencies,
    sourcePath: "pnpm-workspace.yaml",
    value: Object.fromEntries(workspace.patchedDependencies),
  };
  const rootManifest = manifests.find((manifest) => manifest.context === ".");
  const rootDeclaration = rootManifest?.declaration ?? {
    exists: false,
    sourcePath: "package.json",
    value: undefined,
  };
  const baseDeclaration = rootDeclaration.exists ? rootDeclaration : workspaceDeclaration;
  const anyDeclaration = workspaceDeclaration.exists ||
    manifests.some((manifest) => manifest.declaration.exists);
  const activeSources = [];

  function activate(context, declaration, sourceKind) {
    activeSources.push({ context, declaration, sourceKind });
  }

  if (major === 10) {
    if (scope !== ".") {
      const project = manifests.find((manifest) => manifest.context === scope) ?? manifests[0];
      if (!project) fail(`pnpm dependency scope ${scope} has no effective package manifest`);
      activate(scope, project.declaration, "standalone-legacy");
    } else if (shared) {
      activate(".", baseDeclaration, rootDeclaration.exists ? "root-legacy" : "workspace-yaml");
    } else {
      for (const project of manifests) {
        activate(
          project.context,
          project.declaration.exists ? project.declaration : baseDeclaration,
          project.declaration.exists
            ? "project-legacy"
            : rootDeclaration.exists
              ? "inherited-root-legacy"
              : "inherited-workspace-yaml"
        );
      }
    }
  } else if (major === 11) {
    if (scope === ".") activate(".", workspaceDeclaration, "workspace-yaml");
    else activate(scope, { exists: false, sourcePath: "<none>", value: undefined }, "standalone-none");
  } else {
    if (anyDeclaration) {
      fail(`pnpm ${major} patch configuration is unsupported; remove patch declarations or update the generated harness`);
    }
    activate(scope, { exists: false, sourcePath: "<none>", value: undefined }, "unsupported-major-none");
  }

  const patchEntries = [];
  function addPatchEntries(context, sourceKind, declaration) {
    const { sourcePath, value } = declaration;
    if (declaration.exists && !plainObject(value)) {
      fail(`${sourcePath} pnpm.patchedDependencies must be an object of selectors to paths`);
    }
    const entries = declaration.exists ? Object.entries(value) : [];
    for (const [selector, patchPath] of entries) {
      if (typeof selector !== "string" || !selector || /[\0\r\n]/.test(selector)) {
        fail(`${sourcePath} patchedDependencies contains an invalid selector`);
      }
      if (typeof patchPath !== "string" || !patchPath.trim() || /[\0\r\n]/.test(patchPath)) {
        fail(`${sourcePath} patchedDependencies paths must be nonempty strings`);
      }
      patchEntries.push({ context, sourceKind, sourcePath, selector, patchPath });
    }
  }
  for (const source of activeSources) {
    addPatchEntries(source.context, source.sourceKind, source.declaration);
  }

  const records = [];

  for (const { context, sourceKind, sourcePath, selector, patchPath } of patchEntries) {
    if (path.isAbsolute(patchPath) || path.win32.isAbsolute(patchPath) || patchPath.startsWith("\\\\")) {
      fail(`${sourcePath} patch path ${JSON.stringify(patchPath)} must be repository-relative`);
    }
    const sourceDirectory = path.dirname(path.resolve(repositoryRoot, sourcePath));
    const absolutePatch = path.resolve(sourceDirectory, patchPath);
    if (!withinRepository(absolutePatch)) {
      fail(`${sourcePath} patch path ${JSON.stringify(patchPath)} escapes the repository`);
    }
    const description = `${sourcePath} patch path ${JSON.stringify(patchPath)}`;
    const { repositoryRelative } = validatedRepositoryFile(
      path.relative(repositoryRoot, absolutePatch),
      description
    );
    records.push({
      context,
      sourceKind,
      sourcePath: sourcePath.split(path.sep).join("/"),
      selector,
      patchPath: repositoryRelative.split(path.sep).join("/"),
      contents: fs.readFileSync(absolutePatch),
    });
  }

  records.sort((left, right) => {
    const leftKey = `${left.context}\0${left.sourceKind}\0${left.sourcePath}\0${left.selector}\0${left.patchPath}`;
    const rightKey = `${right.context}\0${right.sourceKind}\0${right.sourcePath}\0${right.selector}\0${right.patchPath}`;
    return leftKey < rightKey ? -1 : leftKey > rightKey ? 1 : 0;
  });
  const hash = createHash("sha256");
  hash.update("jig-pnpm-patches-v2\0");
  for (const source of activeSources) {
    hash.update(source.context);
    hash.update("\0");
    hash.update(source.sourceKind);
    hash.update("\0");
    hash.update(source.declaration.sourcePath);
    hash.update("\0");
  }
  for (const record of records) {
    hash.update(record.context);
    hash.update("\0");
    hash.update(record.sourceKind);
    hash.update("\0");
    hash.update(record.sourcePath);
    hash.update("\0");
    hash.update(record.selector);
    hash.update("\0");
    hash.update(record.patchPath);
    hash.update("\0");
    hash.update(record.contents);
    hash.update("\0");
  }
  process.stdout.write(`${hash.digest("hex")}\t${dependencyFree}\n`);
  process.exit(0);
}

fail(`unknown workspace metadata operation ${JSON.stringify(operation)}`);
NODE
}

root_workspace_contains_app() {
  local app_dir="$1"

  [ "$app_dir" = "." ] && return 0
  workspace_metadata contains "$app_dir" >/dev/null
}

root_workspace_manifest_paths() {
  workspace_metadata manifests
}

[% if web_package_manager == "yarn" %]
yarn_lockfile_kind() {
  local lockfile="$1"
  local status=0

  "$node_bin" - --jig-yarn-lockfile-kind "$lockfile" <<'NODE' || status=$?
const fs = require("node:fs");
const path = require("node:path");

const repositoryRoot = fs.realpathSync.native(".");
const input = process.argv[3];
const absolute = path.isAbsolute(input)
  ? path.resolve(input)
  : path.resolve(repositoryRoot, input);
const relative = path.relative(repositoryRoot, absolute);
if (
  !input ||
  /[\0\r\n]/.test(input) ||
  relative === "" ||
  relative === ".." ||
  relative.startsWith(`..${path.sep}`) ||
  path.isAbsolute(relative)
) {
  console.error(`Yarn lockfile must remain inside the repository: ${input}`);
  process.exit(1);
}

let current = repositoryRoot;
let stats;
for (const [index, component] of relative.split(path.sep).entries()) {
  current = path.join(current, component);
  try {
    stats = fs.lstatSync(current);
  } catch (error) {
    if (error?.code === "ENOENT" || error?.code === "ENOTDIR") {
      console.error(`Yarn lockfile does not exist: ${input}`);
      process.exit(1);
    }
    throw error;
  }
  if (stats.isSymbolicLink()) {
    console.error(`Yarn lockfile must not traverse a symbolic link: ${input}`);
    process.exit(1);
  }
  if (index + 1 < relative.split(path.sep).length && !stats.isDirectory()) {
    console.error(`Yarn lockfile must traverse real directories: ${input}`);
    process.exit(1);
  }
}
if (!stats.isFile()) {
  console.error(`Yarn lockfile must be a real file: ${input}`);
  process.exit(1);
}

const source = fs.readFileSync(absolute, "utf8").replaceAll("\r", "");
process.stdout.write(/^# yarn lockfile v1$/m.test(source) ? "classic\n" : "berry\n");
NODE
  [ "$status" -eq 0 ] && return 0
  [ "$status" -eq 1 ] && return 2
  return "$status"
}
[% endif %]

dependency_scope() {
  local app_dir="$1"
  local membership

  if [ "$app_dir" = "." ]; then
    printf '%s\n' "."
    return
  fi

  if root_workspace_contains_app "$app_dir"; then
    membership=0
  else
    membership=$?
    [ "$membership" -eq 1 ] || return "$membership"
  fi

[% if web_package_manager == "yarn" %]
  local lockfile lockfile_kind root_project_lock
  if lockfile="$(app_lockfile "$app_dir")"; then
    # Modern Yarn lockfiles define a nested Berry project. Yarn Classic still
    # walks to a containing workspace root even when the member has a v1 lock.
    lockfile_kind="$(yarn_lockfile_kind "$lockfile")" || return
    if [ "$lockfile_kind" = "classic" ] && [ "$membership" -eq 0 ]; then
      if root_project_lock="$(root_lockfile)"; then
        yarn_lockfile_kind "$root_project_lock" >/dev/null || return
      fi
      printf '%s\n' "."
    else
      printf '%s\n' "$app_dir"
    fi
  elif [ "$membership" -eq 0 ]; then
    if root_project_lock="$(root_lockfile)"; then
      yarn_lockfile_kind "$root_project_lock" >/dev/null || return
    fi
    printf '%s\n' "."
  else
    printf '%s\n' "$app_dir"
  fi
[% else %]
  # npm, pnpm, and Bun all make a declared workspace member part of the root
  # install boundary; a nested lock does not override that manager contract.
  if [ "$membership" -eq 0 ]; then
    printf '%s\n' "."
  elif app_lockfile "$app_dir" >/dev/null; then
    printf '%s\n' "$app_dir"
  else
    printf '%s\n' "$app_dir"
  fi
[% endif %]
}

node_version_candidate() {
  local candidate="$1"

  "$node_bin" - "$candidate" <<'NODE'
const fs = require("node:fs");
const path = require("node:path");

const candidate = process.argv[2];
const maximumBytes = 128;

function invalid(message) {
  console.error(`Invalid Node version authority ${JSON.stringify(candidate)}: ${message}`);
  process.exit(2);
}

process.on("uncaughtException", () => invalid("an unexpected filesystem validation error occurred"));

const repositoryRoot = fs.realpathSync.native(".");
const absolute = path.resolve(repositoryRoot, candidate);
const relative = path.relative(repositoryRoot, absolute);

if (
  !candidate ||
  /[\0\r\n]/.test(candidate) ||
  relative === "" ||
  relative === ".." ||
  relative.startsWith(`..${path.sep}`) ||
  path.isAbsolute(relative)
) {
  invalid("the path must identify a repository-relative file");
}

const components = relative.split(path.sep);
let current = repositoryRoot;
const trustedParents = [{ path: repositoryRoot, stats: fs.lstatSync(repositoryRoot) }];
for (const component of components.slice(0, -1)) {
  current = path.join(current, component);
  let stats;
  try {
    stats = fs.lstatSync(current);
  } catch (error) {
    invalid(error?.code === "ENOENT" ? "a parent directory is missing" : "a parent directory cannot be inspected");
  }
  if (stats.isSymbolicLink() || !stats.isDirectory()) {
    invalid("every parent must be a real directory, not a symbolic link or special file");
  }
  trustedParents.push({ path: current, stats });
}

function requireTrustedParents(action) {
  for (const parent of trustedParents) {
    let currentStats;
    try {
      currentStats = fs.lstatSync(parent.path);
    } catch {
      invalid(`a trusted parent disappeared ${action}`);
    }
    if (
      currentStats.isSymbolicLink() ||
      !currentStats.isDirectory() ||
      currentStats.dev !== parent.stats.dev ||
      currentStats.ino !== parent.stats.ino
    ) {
      invalid(`a trusted parent was replaced ${action}`);
    }
  }
}

let before;
try {
  before = fs.lstatSync(absolute);
} catch (error) {
  if (error?.code === "ENOENT") {
    requireTrustedParents("while confirming the authority is absent");
    process.exit(1);
  }
  invalid("the file cannot be inspected");
}
if (before.isSymbolicLink() || !before.isFile()) {
  invalid("the path must be a real regular file, not a symbolic link, directory, or special file");
}
if (before.size === 0 || before.size > maximumBytes) {
  invalid(`the file must contain between 1 and ${maximumBytes} bytes`);
}

let descriptor;
try {
  const noFollow = process.platform === "win32" ? 0 : (fs.constants.O_NOFOLLOW ?? 0);
  descriptor = fs.openSync(absolute, fs.constants.O_RDONLY | noFollow);
} catch {
  invalid("the regular file could not be opened without following symbolic links");
}

let opened;
let contents;
try {
  opened = fs.fstatSync(descriptor);
  if (!opened.isFile() || opened.size === 0 || opened.size > maximumBytes) {
    invalid(`the opened authority must be a 1-${maximumBytes} byte regular file`);
  }
  contents = Buffer.alloc(opened.size);
  let offset = 0;
  while (offset < contents.length) {
    const read = fs.readSync(descriptor, contents, offset, contents.length - offset, offset);
    if (read === 0) invalid("the authority changed while it was being read");
    offset += read;
  }
} finally {
  fs.closeSync(descriptor);
}

let after;
try {
  after = fs.lstatSync(absolute);
} catch {
  invalid("the authority disappeared while it was being read");
}
if (
  after.isSymbolicLink() ||
  !after.isFile() ||
  before.dev !== opened.dev ||
  before.ino !== opened.ino ||
  before.size !== opened.size ||
  after.dev !== opened.dev ||
  after.ino !== opened.ino ||
  after.size !== opened.size
) {
  invalid("the authority was replaced or changed while it was being read");
}

requireTrustedParents("while the authority was being read");

let version = contents.toString("utf8");
if (!Buffer.from(version, "utf8").equals(contents)) {
  invalid("the file is not valid UTF-8");
}
if (version.endsWith("\r\n")) version = version.slice(0, -2);
else if (version.endsWith("\n")) version = version.slice(0, -1);
if (
  version.length === 0 ||
  version.trim() !== version ||
  /\s/u.test(version) ||
  [...version].some((character) => {
    const code = character.codePointAt(0);
    return code < 32 || code === 127;
  })
) {
  invalid("the file must contain exactly one non-whitespace Node version token");
}
NODE
}

node_version_file() {
  local app_dir="$1"
  local scope status candidate

  scope="$(dependency_scope "$app_dir")" || {
    status=$?
    [ "$status" -eq 1 ] && status=2
    return "$status"
  }
  if [ "$scope" != "." ]; then
    candidate="$scope/.node-version"
    if node_version_candidate "$candidate"; then
      printf '%s\n' "$candidate"
      return
    else
      status=$?
    fi
    [ "$status" -eq 1 ] || return "$status"
  fi

  candidate=.node-version
  if node_version_candidate "$candidate"; then
    printf '%s\n' "$candidate"
    return
  else
    status=$?
  fi
  [ "$status" -eq 1 ] || return "$status"

  if [ "$app_dir" != "." ] && [ "$app_dir" != "$scope" ]; then
    # A root dependency scope can still adopt an app-specific version when the
    # repository has no root policy yet.
    candidate="$app_dir/.node-version"
    if node_version_candidate "$candidate"; then
      printf '%s\n' "$candidate"
      return
    else
      status=$?
    fi
    [ "$status" -eq 1 ] || return "$status"
  fi
  return 1
}

[% if web_package_manager == "pnpm" %]
pnpm_effective_manifest_path() {
  local directory="$1"

  "$node_bin" - "$directory" <<'NODE'
const fs = require("node:fs");
const path = require("node:path");

const directory = process.argv[2];
const repositoryRoot = fs.realpathSync.native(".");
const absoluteDirectory = path.resolve(repositoryRoot, directory);
const repositoryRelative = path.relative(repositoryRoot, absoluteDirectory);
if (
  repositoryRelative === ".." ||
  repositoryRelative.startsWith(`..${path.sep}`) ||
  path.isAbsolute(repositoryRelative)
) {
  console.error(`pnpm package manifest directory ${directory} escapes the repository.`);
  process.exit(2);
}
let current = repositoryRoot;
for (const component of repositoryRelative.split(path.sep).filter(Boolean)) {
  current = path.join(current, component);
  let stats;
  try {
    stats = fs.lstatSync(current);
  } catch {
    console.error(`pnpm package manifest directory ${directory} does not exist.`);
    process.exit(2);
  }
  if (stats.isSymbolicLink() || !stats.isDirectory()) {
    console.error(`pnpm package manifest directory ${directory} must not traverse symbolic links or non-directories.`);
    process.exit(2);
  }
}
for (const name of ["package.json", "package.json5", "package.yaml"]) {
  const candidate = path.join(absoluteDirectory, name);
  let stats;
  try {
    stats = fs.lstatSync(candidate);
  } catch (error) {
    if (error?.code === "ENOENT") continue;
    throw error;
  }
  if (stats.isSymbolicLink() || !stats.isFile()) {
    console.error(`pnpm package manifest ${candidate} must be a real file.`);
    process.exit(2);
  }
  process.stdout.write(`${path.relative(repositoryRoot, candidate) || name}\n`);
  process.exit(0);
}
console.error(`No pnpm package manifest exists in ${directory}.`);
process.exit(2);
NODE
}

pnpm_package_manager_spec() {
  local app_dir="$1"
  local scope status

  scope="$(dependency_scope "$app_dir")" || {
    status=$?
    [ "$status" -eq 1 ] && status=2
    return "$status"
  }
  pnpm_package_manager_spec_for_scope "$scope" || {
    status=$?
    [ "$status" -eq 1 ] && status=2
    return "$status"
  }
}

pnpm_package_manager_spec_for_scope() {
  local scope="$1"
  local status

  pnpm_effective_manifest_path "$scope" >/dev/null || {
    status=$?
    [ "$status" -eq 1 ] && status=2
    return "$status"
  }
  "$node_bin" - "$scope" "<<[ web_package_manager_spec ]>>" <<'NODE'
const { spawnSync } = require("node:child_process");
const fs = require("node:fs");
const path = require("node:path");

const scope = process.argv[2];
const fallbackSpec = process.argv[3];
const repositoryRoot = fs.realpathSync.native(".");
let current = path.resolve(repositoryRoot, scope);
const relativeScope = path.relative(repositoryRoot, current);

function fail(message) {
  console.error(`Cannot determine pnpm package-manager authority: ${message}`);
  process.exit(2);
}

if (
  relativeScope === ".." ||
  relativeScope.startsWith(`..${path.sep}`) ||
  path.isAbsolute(relativeScope)
) fail(`dependency scope ${JSON.stringify(scope)} escapes the repository`);

let verified = repositoryRoot;
for (const component of relativeScope.split(path.sep).filter(Boolean)) {
  verified = path.join(verified, component);
  let stats;
  try {
    stats = fs.lstatSync(verified);
  } catch (error) {
    fail(`dependency scope ${JSON.stringify(scope)} is unavailable (${error.message})`);
  }
  if (stats.isSymbolicLink() || !stats.isDirectory()) {
    fail(`dependency scope ${JSON.stringify(scope)} must traverse real directories`);
  }
}

function validSpec(value) {
  return typeof value === "string" &&
    value.startsWith("pnpm@") &&
    value.length > "pnpm@".length &&
    Buffer.byteLength(value) <= 512 &&
    !/[\0-\x1f\x7f]/.test(value);
}

function plainObject(value) {
  return typeof value === "object" &&
    value !== null &&
    !Array.isArray(value) &&
    Object.getPrototypeOf(value) === Object.prototype;
}

function effectiveManifest(directory, required) {
  for (const name of ["package.json", "package.json5", "package.yaml"]) {
    const candidate = path.join(directory, name);
    let stats;
    try {
      stats = fs.lstatSync(candidate);
    } catch (error) {
      if (error?.code === "ENOENT" || error?.code === "ENOTDIR") continue;
      fail(`could not inspect ${candidate} (${error.message})`);
    }
    if (stats.isSymbolicLink() || !stats.isFile()) fail(`${candidate} must be a real file`);
    return candidate;
  }
  if (required) fail(`no pnpm package manifest exists in ${directory}`);
  return null;
}

// BEGIN JIG PNPM MANIFEST QUERY LAUNCHER
function environmentValue(environment, name) {
  const key = Object.keys(environment).find((candidate) => candidate.toUpperCase() === name);
  return key ? environment[key] : undefined;
}

function validateWindowsCommandInterpreter(interpreter) {
  const driveAbsolute =
    typeof interpreter === "string" && /^[A-Za-z]:[\\/]/.test(interpreter);
  const uncAbsolute =
    typeof interpreter === "string" &&
    /^[\\/]{2}[^\\/]+[\\/][^\\/]+(?:[\\/].*)?$/.test(interpreter);
  if (
    !interpreter ||
    /[\0-\x1f\x7f"]/.test(interpreter) ||
    /^[\\/]{2}[?.][\\/]/.test(interpreter) ||
    (!driveAbsolute && !uncAbsolute)
  ) {
    throw new Error("unsafe Windows command interpreter");
  }
  return interpreter;
}

function windowsPathEntries(value) {
  const entries = [];
  let entry = "";
  let quoted = false;
  for (const character of value) {
    if (character === '"') quoted = !quoted;
    else if (character === path.delimiter && !quoted) {
      entries.push(entry);
      entry = "";
    } else entry += character;
  }
  if (quoted) throw new Error("unsafe quoted Windows PATH");
  entries.push(entry);
  return entries;
}

function executableExtensions(command, environment) {
  if (process.platform !== "win32" || path.extname(command)) return [""];
  const raw = environmentValue(environment, "PATHEXT") || ".COM;.EXE;.BAT;.CMD";
  const extensions = raw.split(";").filter(Boolean);
  if (extensions.length === 0 || extensions.some((extension) => !/^\.[A-Za-z0-9]+$/.test(extension))) {
    throw new Error("unsafe Windows PATHEXT");
  }
  return [...new Map(extensions.map((extension) => [extension.toUpperCase(), extension])).values()];
}

function resolveExecutable(command, cwd, environment) {
  if (typeof command !== "string" || !command || /[\0-\x1f\x7f"\\/]/.test(command)) {
    throw new Error("unsafe manifest-parser executable");
  }
  const pathValue = environmentValue(environment, "PATH") || "";
  const entries = process.platform === "win32"
    ? windowsPathEntries(pathValue)
    : pathValue.split(path.delimiter);
  for (const entry of entries) {
    const directory = entry
      ? (path.isAbsolute(entry) ? entry : path.resolve(cwd, entry))
      : cwd;
    for (const extension of executableExtensions(command, environment)) {
      const candidate = path.resolve(directory, `${command}${extension}`);
      try {
        if (!fs.statSync(candidate).isFile()) continue;
        if (process.platform !== "win32") fs.accessSync(candidate, fs.constants.X_OK);
        const resolved = fs.realpathSync.native(candidate);
        if (!fs.statSync(resolved).isFile() || /[\0-\x1f\x7f"]/.test(resolved)) continue;
        return resolved;
      } catch {
        // Bare PATH lookup skips an unusable entry and continues in order.
      }
    }
  }
  throw new Error(`${command} was not found on PATH`);
}

function encodeBatchArgument(argument, forceQuote = false) {
  if (/[\0\r\n]/.test(argument)) throw new Error("unsafe manifest-parser batch argument");
  const safeUnquoted = "#$*+-./:?@\\_";
  const quote = forceQuote || argument.length === 0 || argument.endsWith("\\") ||
    [...argument].some((character) => {
      const code = character.codePointAt(0);
      return code < 32 || code === 127 ||
        (code < 128 && !/[A-Za-z0-9]/.test(character) && !safeUnquoted.includes(character));
    });
  let encoded = quote ? '"' : "";
  let backslashes = 0;
  for (const character of argument) {
    if (character === "\\") {
      backslashes += 1;
      encoded += character;
      continue;
    }
    if (character === '"') {
      encoded += "\\".repeat(backslashes);
      encoded += '"';
    } else if (character === "%") {
      encoded += "%%cd:~,";
    }
    backslashes = 0;
    encoded += character;
  }
  if (quote) encoded += "\\".repeat(backslashes) + '"';
  return encoded;
}

function spawnManifestParser(executable, args, options) {
  const queryArguments = [
    "pkg",
    "get",
    "packageManager",
    "devEngines.packageManager",
    "--json",
    "--ignore-workspace",
  ];
  const allowed =
    args.length === queryArguments.length && args.every((value, index) => value === queryArguments[index]) ||
    args.length === queryArguments.length + 1 &&
      args[0] === fallbackSpec &&
      queryArguments.every((value, index) => value === args[index + 1]);
  if (!allowed || args.some((argument) => !/^[A-Za-z0-9.@_-]+$/.test(argument))) {
    throw new Error("unsupported pnpm manifest-parser arguments");
  }
  const spawnOptions = { timeout: 30_000, ...options, shell: false };
  if (process.platform !== "win32" || !/\.(?:cmd|bat)$/i.test(executable)) {
    return spawnSync(executable, args, spawnOptions);
  }
  if (/^[\\/]{2}[?.][\\/]/.test(executable) || executable.endsWith("\\")) {
    throw new Error("unsafe pnpm manifest-parser batch executable");
  }
  let commandLine = '"' + encodeBatchArgument(executable, true);
  for (const argument of args) commandLine += ` ${encodeBatchArgument(argument)}`;
  commandLine += '"';
  const interpreter = validateWindowsCommandInterpreter(
    environmentValue(spawnOptions.env, "COMSPEC")
  );
  return spawnSync(
    interpreter,
    ["/d", "/s", "/v:off", "/c", commandLine],
    { ...spawnOptions, windowsVerbatimArguments: true }
  );
}

function queryAlternateManifest(directory) {
  const environment = { ...process.env };
  for (const key of Object.keys(environment)) {
    if (
      /^(?:npm|pnpm)_config_ignore_pnpmfile$/i.test(key) ||
      /^(?:npm|pnpm)_config_manage_package_manager_versions$/i.test(key) ||
      /^(?:npm|pnpm)_config_(?:pm|runtime)_on_fail$/i.test(key) ||
      /^corepack_(?:enable_project_spec|enable_auto_pin|enable_download_prompt|env_file)$/i.test(key)
    ) delete environment[key];
  }
  environment.NPM_CONFIG_IGNORE_PNPMFILE = "true";
  environment.PNPM_CONFIG_IGNORE_PNPMFILE = "true";
  environment.NPM_CONFIG_MANAGE_PACKAGE_MANAGER_VERSIONS = "false";
  environment.PNPM_CONFIG_MANAGE_PACKAGE_MANAGER_VERSIONS = "false";
  environment.NPM_CONFIG_PM_ON_FAIL = "ignore";
  environment.PNPM_CONFIG_PM_ON_FAIL = "ignore";
  environment.NPM_CONFIG_RUNTIME_ON_FAIL = "ignore";
  environment.PNPM_CONFIG_RUNTIME_ON_FAIL = "ignore";
  environment.COREPACK_ENABLE_PROJECT_SPEC = "0";
  environment.COREPACK_ENABLE_AUTO_PIN = "0";
  environment.COREPACK_ENABLE_DOWNLOAD_PROMPT = "0";
  environment.COREPACK_ENV_FILE = "0";

  const queryArguments = [
    "pkg",
    "get",
    "packageManager",
    "devEngines.packageManager",
    "--json",
    "--ignore-workspace",
  ];
  let executable;
  let arguments_;
  try {
    executable = resolveExecutable("corepack", directory, environment);
    arguments_ = [fallbackSpec, ...queryArguments];
  } catch (error) {
    if (environment.CI) fail(`Corepack is required to inspect alternate pnpm manifests in CI (${error.message})`);
    try {
      executable = resolveExecutable("pnpm", directory, environment);
      arguments_ = queryArguments;
    } catch (fallbackError) {
      fail(`Corepack is unavailable and no local pnpm fallback can inspect the alternate manifest (${fallbackError.message})`);
    }
  }

  let result;
  try {
    result = spawnManifestParser(executable, arguments_, {
      cwd: directory,
      encoding: "utf8",
      env: environment,
      maxBuffer: 16 * 1024,
      timeout: 30_000,
    });
  } catch (error) {
    fail(`could not start the bounded alternate-manifest parser (${error.message})`);
  }
  if (
    result.error ||
    result.signal ||
    result.status !== 0 ||
    typeof result.stdout !== "string" ||
    Buffer.byteLength(result.stdout) > 16 * 1024 ||
    /\0/.test(result.stdout)
  ) fail("the bounded alternate-manifest parser failed");
  let parsed;
  try {
    parsed = JSON.parse(result.stdout);
  } catch (error) {
    fail(`the alternate-manifest parser returned malformed JSON (${error.message})`);
  }
  if (!plainObject(parsed)) fail("the alternate-manifest parser must return one JSON object");
  return parsed;
}
// END JIG PNPM MANIFEST QUERY LAUNCHER

function authoritySpec(manifestPath) {
  let manifest;
  if (path.basename(manifestPath) === "package.json") {
    try {
      manifest = JSON.parse(fs.readFileSync(manifestPath, "utf8"));
    } catch (error) {
      fail(`${manifestPath} is not valid JSON (${error.message})`);
    }
    if (!plainObject(manifest)) fail(`${manifestPath} must contain one JSON object`);
  } else {
    manifest = queryAlternateManifest(path.dirname(manifestPath));
  }

  if (Object.hasOwn(manifest, "packageManager")) {
    if (!validSpec(manifest.packageManager)) {
      fail(`${manifestPath} has invalid or non-pnpm packageManager authority`);
    }
    return manifest.packageManager;
  }

  const dottedDevManager = manifest["devEngines.packageManager"];
  const nestedDevManager = plainObject(manifest.devEngines) &&
      Object.hasOwn(manifest.devEngines, "packageManager")
    ? manifest.devEngines.packageManager
    : undefined;
  const hasDevAuthority = dottedDevManager !== undefined || nestedDevManager !== undefined;
  const devManager = dottedDevManager !== undefined ? dottedDevManager : nestedDevManager;
  if (!hasDevAuthority) return null;
  const devSpec = plainObject(devManager) && devManager.name === "pnpm" && typeof devManager.version === "string"
    ? `pnpm@${devManager.version}`
    : null;
  if (!validSpec(devSpec)) {
    fail(`${manifestPath} has invalid or non-pnpm devEngines.packageManager authority`);
  }
  return devSpec;
}

if (!validSpec(fallbackSpec)) fail("the generated fallback pnpm specification is invalid");
let first = true;
while (true) {
  const manifestPath = effectiveManifest(current, first);
  first = false;
  if (manifestPath) {
    const spec = authoritySpec(manifestPath);
    if (spec !== null) {
      process.stdout.write(`${spec}\n`);
      process.exit(0);
    }
  }
  if (current === repositoryRoot) break;
  const parent = path.dirname(current);
  if (parent === current) fail("could not reach repository root while resolving pnpm authority");
  current = parent;
}

process.stdout.write(`${fallbackSpec}\n`);
NODE
}

pnpm_runtime_snapshot() {
  local scope="$1"

  "$node_bin" - "$scope" <<'NODE'
const { createHash } = require("node:crypto");
const { spawnSync } = require("node:child_process");
const fs = require("node:fs");
const path = require("node:path");

// BEGIN JIG PACKAGE MANAGER METADATA LAUNCHER
const JIG_PACKAGE_MANAGER_METADATA_ARGUMENTS = [
  ["--version"],
  ["cache", "dir", "--silent"],
  ["config", "--json"],
  ["config", "list", "--json"],
  [
    "pkg",
    "get",
    "name",
    "dependencies",
    "devDependencies",
    "optionalDependencies",
    "peerDependencies",
    "pnpm",
    "--json",
  ],
];

function packageManagerMetadataEnvironmentValue(environment, name) {
  const key = Object.keys(environment).find((candidate) => candidate.toUpperCase() === name);
  return key ? environment[key] : undefined;
}

function validateWindowsCommandInterpreter(interpreter) {
  const driveAbsolute =
    typeof interpreter === "string" && /^[A-Za-z]:[\\/]/.test(interpreter);
  const uncAbsolute =
    typeof interpreter === "string" &&
    /^[\\/]{2}[^\\/]+[\\/][^\\/]+(?:[\\/].*)?$/.test(interpreter);
  if (
    !interpreter ||
    /[\0-\x1f\x7f"]/.test(interpreter) ||
    /^[\\/]{2}[?.][\\/]/.test(interpreter) ||
    (!driveAbsolute && !uncAbsolute)
  ) {
    throw new Error("unsafe Windows command interpreter");
  }
  return interpreter;
}

function validatePackageManagerMetadataExecutable(executable) {
  if (
    typeof executable !== "string" ||
    executable.length === 0 ||
    /[\0-\x1f\x7f"]/.test(executable)
  ) {
    throw new Error("unsafe package-manager metadata executable");
  }
  return executable;
}

function windowsPackageManagerMetadataPathEntries(value) {
  const entries = [];
  let entry = "";
  let quoted = false;
  for (const character of value) {
    if (character === '"') {
      quoted = !quoted;
    } else if (character === path.delimiter && !quoted) {
      entries.push(entry);
      entry = "";
    } else {
      entry += character;
    }
  }
  if (quoted) throw new Error("unsafe quoted Windows PATH");
  entries.push(entry);
  return entries;
}

function windowsPackageManagerMetadataExtensions(requested, configured) {
  if (path.extname(requested)) return [""];
  const raw = configured === undefined ? ".COM;.EXE;.BAT;.CMD" : configured;
  const extensions = raw.split(";").filter((extension) => extension !== "");
  if (
    extensions.length === 0 ||
    extensions.some((extension) => !/^\.[A-Za-z0-9]+$/.test(extension))
  ) throw new Error("unsafe Windows PATHEXT");
  return [...new Map(extensions.map((extension) => [extension.toUpperCase(), extension])).values()];
}

function resolveWindowsPackageManagerMetadataExecutable(executable, options = {}) {
  const fs = require("node:fs");
  const path = require("node:path");
  const requested = validatePackageManagerMetadataExecutable(executable);
  const environment = options.env || process.env;
  const workingDirectory = typeof options.cwd === "string"
    ? path.resolve(options.cwd)
    : process.cwd();
  let candidates = [];
  const searchedPath = !path.isAbsolute(requested);

  if (path.isAbsolute(requested)) {
    candidates = [requested];
  } else {
    if (/[\\/]/.test(requested)) {
      throw new Error("package-manager metadata executable must be absolute or a bare command");
    }
    const pathValue = packageManagerMetadataEnvironmentValue(environment, "PATH") || "";
    const configuredExtensions = packageManagerMetadataEnvironmentValue(environment, "PATHEXT");
    const extensions = windowsPackageManagerMetadataExtensions(requested, configuredExtensions);
    for (const entry of windowsPackageManagerMetadataPathEntries(pathValue)) {
      const directory = entry
        ? (path.isAbsolute(entry) ? entry : path.resolve(workingDirectory, entry))
        : workingDirectory;
      for (const extension of extensions) {
        candidates.push(path.resolve(directory, `${requested}${extension}`));
      }
    }
  }

  for (const candidate of candidates) {
    try {
      if (!fs.statSync(candidate).isFile()) continue;
      const resolved = fs.realpathSync.native(candidate);
      if (!fs.statSync(resolved).isFile()) continue;
      return validatePackageManagerMetadataExecutable(resolved);
    } catch (error) {
      if (!searchedPath) throw error;
    }
  }
  throw new Error("package-manager metadata executable was not found");
}

function encodePackageManagerMetadataBatchArgument(argument, forceQuote = false) {
  if (/[\0\r\n]/.test(argument)) {
    throw new Error("unsafe package-manager metadata batch argument");
  }
  const safeUnquoted = "#$*+-./:?@\\_";
  const quote = forceQuote || argument.length === 0 || argument.endsWith("\\") ||
    [...argument].some((character) => {
      const code = character.codePointAt(0);
      return code < 32 || code === 127 ||
        (code < 128 && !/[A-Za-z0-9]/.test(character) && !safeUnquoted.includes(character));
    });
  let encoded = quote ? '"' : "";
  let backslashes = 0;
  for (const character of argument) {
    if (character === "\\") {
      backslashes += 1;
      encoded += character;
      continue;
    }
    if (character === '"') {
      encoded += "\\".repeat(backslashes);
      encoded += '"';
    } else if (character === "%") {
      encoded += "%%cd:~,";
    }
    backslashes = 0;
    encoded += character;
  }
  if (quote) encoded += "\\".repeat(backslashes) + '"';
  return encoded;
}

function encodePackageManagerMetadataBatchInvocation(executable, args) {
  if (/^[\\/]{2}[?.][\\/]/.test(executable) || executable.endsWith("\\")) {
    throw new Error("unsafe package-manager metadata batch executable");
  }
  let commandLine = '"' + encodePackageManagerMetadataBatchArgument(executable, true);
  for (const argument of args) {
    commandLine += ` ${encodePackageManagerMetadataBatchArgument(argument)}`;
  }
  return commandLine + '"';
}

function spawnPackageManagerMetadata(executable, args, options) {
  const allowedArguments =
    Array.isArray(args) &&
    JIG_PACKAGE_MANAGER_METADATA_ARGUMENTS.some(
      (allowed) =>
        allowed.length === args.length &&
        allowed.every((value, index) => value === args[index])
    );
  if (!allowedArguments || args.some((value) => !/^[A-Za-z0-9._-]+$/.test(value))) {
    throw new Error("unsupported package-manager metadata arguments");
  }

  const spawnOptions = { timeout: 30_000, ...options, shell: false };
  if (process.platform !== "win32") {
    return spawnSync(
      validatePackageManagerMetadataExecutable(executable),
      args,
      spawnOptions
    );
  }

  const resolved = resolveWindowsPackageManagerMetadataExecutable(executable, spawnOptions);
  if (!/\.(?:cmd|bat)$/i.test(resolved)) {
    return spawnSync(resolved, args, spawnOptions);
  }

  const environment = spawnOptions.env || process.env;
  const commandInterpreter = validateWindowsCommandInterpreter(
    packageManagerMetadataEnvironmentValue(environment, "COMSPEC")
  );
  const commandLine = encodePackageManagerMetadataBatchInvocation(resolved, args);
  return spawnSync(
    commandInterpreter,
    ["/d", "/s", "/v:off", "/c", commandLine],
    { ...spawnOptions, windowsVerbatimArguments: true }
  );
}
// END JIG PACKAGE MANAGER METADATA LAUNCHER

const scope = process.argv[2];
const repositoryRoot = fs.realpathSync.native(".");
const cwd = path.resolve(repositoryRoot, scope);
const relative = path.relative(repositoryRoot, cwd);
if (relative === ".." || relative.startsWith(`..${path.sep}`) || path.isAbsolute(relative)) {
  console.error("pnpm dependency scope escapes the repository.");
  process.exit(1);
}

const layoutConfigurationNames = new Map([
  "enableGlobalVirtualStore",
  "sharedWorkspaceLockfile",
  "nodeLinker",
  "symlink",
  "enableModulesDir",
  "modulesDir",
  "virtualStoreDir",
  "verifyDepsBeforeRun",
  "virtualStoreOnly",
  "nodeExperimentalPackageMap",
  "nodePackageMapType",
  "preferSymlinkedExecutables",
  "extendNodePath",
  "dedupeDirectDeps",
  "dedupeInjectedDeps",
  "dedupePeers",
  "modulesCacheMaxAge",
  "storeDir",
  "sideEffectsCache",
  "sideEffectsCacheReadonly",
  "verifyStoreIntegrity",
  "strictStorePkgContentCheck",
  "hoist",
  "hoistPattern",
  "publicHoistPattern",
  "shamefullyHoist",
  "packageImportMethod",
  "virtualStoreDirMaxLength",
  "peersSuffixMaxLength",
  "dedupePeerDependents",
  "resolvePeersFromWorkspaceRoot",
  "supportedArchitectures",
  "ignoredOptionalDependencies",
  "injectWorkspacePackages",
  "hoistWorkspacePackages",
  "linkWorkspacePackages",
  "preferWorkspacePackages",
  "excludeLinksFromLockfile",
  "autoInstallPeers",
  "strictPeerDependencies",
  "dev",
  "optional",
  "production",
  "ignoreScripts",
  "enablePrePostScripts",
  "dangerouslyAllowAllBuilds",
  "onlyBuiltDependencies",
  "neverBuiltDependencies",
  "ignoredBuiltDependencies",
  "allowBuilds",
  "onlyBuiltDependenciesFile",
  "strictDepBuilds",
  "syncInjectedDepsAfterScripts",
  "requiredScripts",
  "executionEnv",
  "overrides",
  "packageExtensions",
  "patchedDependencies",
  "minimumReleaseAge",
  "minimumReleaseAgeStrict",
  "minimumReleaseAgeExclude",
  "minimumReleaseAgeIgnoreMissingTime",
  "trustPolicy",
  "trustPolicyExclude",
  "trustPolicyIgnoreAfter",
  "workspacePackagePatterns",
  "lockfileDir",
  "useLockfile",
].map((name) => [name.toLowerCase().replaceAll("-", ""), name]));

function normalizedPnpmEnvironmentOverride(key) {
  const normalized = key.toLowerCase().replaceAll("_", "").replaceAll("-", "");
  const prefix = ["npmconfig", "pnpmconfig"].find((candidate) => normalized.startsWith(candidate));
  return prefix ? normalized.slice(prefix.length) : null;
}

const layoutEnvironment = Object.create(null);
for (const key of Object.keys(process.env).sort()) {
  const normalized = normalizedPnpmEnvironmentOverride(key);
  if (normalized === null) continue;
  if (normalized === "ignorepnpmfile") {
    console.error(`Inherited pnpm metadata-hook environment override ${JSON.stringify(key)} is unsupported.`);
    process.exit(1);
  }
  const name = layoutConfigurationNames.get(normalized);
  if (!name) continue;
  if ([
    "enableGlobalVirtualStore",
    "virtualStoreOnly",
    "nodeExperimentalPackageMap",
    "nodePackageMapType",
    "lockfileDir",
  ].includes(name)) {
    const guidance = name === "enableGlobalVirtualStore"
      ? "remove it and add enableGlobalVirtualStore: false to pnpm-workspace.yaml."
      : `remove it because ${name} is not a supported dependency-receipt layout override.`;
    console.error(`Inherited pnpm setting ${JSON.stringify(key)} is an unsupported environment override; ${guidance}`);
    process.exit(1);
  }
  const value = process.env[key];
  if (
    Object.hasOwn(layoutEnvironment, name) ||
    typeof value !== "string" ||
    Buffer.byteLength(value) > 4096 ||
    /[\0\r\n]/.test(value)
  ) {
    console.error(`Inherited pnpm layout environment override ${JSON.stringify(key)} is ambiguous or invalid.`);
    process.exit(1);
  }
  layoutEnvironment[name] = value;
}

function executableIdentity(realPath) {
  const realStats = fs.statSync(realPath);
  if (!realStats.isFile() || realStats.size > 256 * 1024 * 1024 || /[\0-\x1f\x7f]/.test(realPath)) {
    throw new Error("pnpm executable identity is invalid");
  }
  const hash = createHash("sha256");
  const descriptor = fs.openSync(realPath, "r");
  const buffer = Buffer.allocUnsafe(64 * 1024);
  try {
    let offset = 0;
    while (offset < realStats.size) {
      const length = fs.readSync(descriptor, buffer, 0, buffer.length, offset);
      if (length <= 0) throw new Error("pnpm executable changed while hashing");
      hash.update(buffer.subarray(0, length));
      offset += length;
    }
    if (fs.fstatSync(descriptor).size !== realStats.size) {
      throw new Error("pnpm executable changed while hashing");
    }
  } finally {
    fs.closeSync(descriptor);
  }
  return { path: realPath, digest: hash.digest("hex") };
}

function resolveExecutable(command) {
  if (process.platform === "win32") {
    return executableIdentity(resolveWindowsPackageManagerMetadataExecutable(command, {
      cwd,
      env: process.env,
    }));
  }
  const pathKey = Object.keys(process.env).find((key) => key.toUpperCase() === "PATH");
  const pathValue = pathKey ? process.env[pathKey] : "";
  for (const entry of (pathValue || "").split(path.delimiter)) {
    const directory = entry || cwd;
    const candidate = path.resolve(directory, command);
    try {
      const stats = fs.statSync(candidate);
      if (!stats.isFile()) continue;
      fs.accessSync(candidate, fs.constants.X_OK);
      return executableIdentity(fs.realpathSync.native(candidate));
    } catch {}
  }
  throw new Error("pnpm executable was not found on PATH");
}

function pnpmMetadataEnvironment() {
  const environment = { ...process.env };
  for (const key of Object.keys(environment)) {
    if (normalizedPnpmEnvironmentOverride(key) === "ignorepnpmfile") delete environment[key];
  }
  environment.NPM_CONFIG_IGNORE_PNPMFILE = "true";
  environment.PNPM_CONFIG_IGNORE_PNPMFILE = "true";
  return environment;
}

let executable;
try {
  executable = resolveExecutable("pnpm");
} catch (error) {
  console.error(`Could not resolve pnpm executable identity for dependency scope ${scope} (${error.message}).`);
  process.exit(1);
}

const queryEnvironment = pnpmMetadataEnvironment();
function query(args, description) {
  const result = spawnPackageManagerMetadata(executable.path, args, {
    cwd,
    encoding: "utf8",
    env: queryEnvironment,
    maxBuffer: 16 * 1024,
    timeout: 30_000,
  });
  if (result.error || result.signal || result.status !== 0) {
    console.error(`Could not resolve pnpm ${description} for dependency scope ${scope}.`);
    process.exit(1);
  }
  const lines = result.stdout.replace(/\r/g, "").split("\n");
  if (lines.at(-1) === "") lines.pop();
  if (lines.length !== 1 || Buffer.byteLength(lines[0]) > 512 || /[\0-\x1f\x7f]/.test(lines[0])) {
    console.error(`pnpm returned invalid ${description} output for dependency scope ${scope}.`);
    process.exit(1);
  }
  return lines[0];
}

const version = query(["--version"], "version");
const semver = /^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)(?:-[0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*)?(?:\+[0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*)?$/;
const versionMatch = version.match(semver);
if (!versionMatch) {
  console.error(`pnpm returned a non-semver runtime version for dependency scope ${scope}.`);
  process.exit(1);
}

function configurationSnapshot() {
  const result = spawnPackageManagerMetadata(executable.path, ["config", "list", "--json"], {
    cwd,
    encoding: "utf8",
    env: queryEnvironment,
    maxBuffer: 64 * 1024,
    timeout: 30_000,
  });
  if (
    result.error ||
    result.signal ||
    result.status !== 0 ||
    typeof result.stdout !== "string" ||
    Buffer.byteLength(result.stdout) > 64 * 1024 ||
    /\0/.test(result.stdout)
  ) {
    console.error(`Could not resolve bounded pnpm configuration for dependency scope ${scope}.`);
    process.exit(1);
  }
  let configuration;
  try {
    configuration = JSON.parse(result.stdout);
  } catch {
    console.error(`pnpm returned malformed configuration JSON for dependency scope ${scope}.`);
    process.exit(1);
  }
  if (
    typeof configuration !== "object" ||
    configuration === null ||
    Array.isArray(configuration) ||
    Object.getPrototypeOf(configuration) !== Object.prototype
  ) {
    console.error(`pnpm returned an invalid configuration object for dependency scope ${scope}.`);
    process.exit(1);
  }
  return configuration;
}

const configuration = configurationSnapshot();
function configurationValue(name) {
  const normalizedName = name.toLowerCase().replaceAll("-", "");
  const keys = Object.keys(configuration).filter(
    (key) => key.toLowerCase().replaceAll("-", "").replaceAll("_", "") === normalizedName
  );
  if (keys.length > 1) {
    console.error(`pnpm returned ambiguous configuration spellings for ${name}.`);
    process.exit(1);
  }
  return keys.length === 1 ? configuration[keys[0]] : undefined;
}

function booleanConfiguration(name, defaultValue) {
  const value = configurationValue(name);
  if (value === undefined) return defaultValue;
  if (typeof value !== "boolean") {
    console.error(`pnpm returned an invalid boolean configuration value for ${name}.`);
    process.exit(1);
  }
  return value;
}

function boundedLayoutValue(name) {
  const value = configurationValue(name);
  if (value === undefined) return undefined;
  function normalize(candidate, depth = 0) {
    if (
      candidate === null ||
      typeof candidate === "boolean" ||
      (typeof candidate === "number" && Number.isSafeInteger(candidate)) ||
      (typeof candidate === "string" && Buffer.byteLength(candidate) <= 4096 && !/[\0\r\n]/.test(candidate))
    ) return candidate;
    if (depth >= 3) throw new Error("layout configuration is too deeply nested");
    if (Array.isArray(candidate)) {
      if (candidate.length > 256) throw new Error("layout configuration array is too large");
      return candidate.map((entry) => normalize(entry, depth + 1));
    }
    if (typeof candidate === "object" && candidate !== null && Object.getPrototypeOf(candidate) === Object.prototype) {
      const entries = Object.entries(candidate).sort(([left], [right]) => left.localeCompare(right));
      if (entries.length > 256) throw new Error("layout configuration object is too large");
      return Object.fromEntries(entries.map(([key, entry]) => {
        if (!key || Buffer.byteLength(key) > 1024 || /[\0\r\n]/.test(key)) {
          throw new Error("layout configuration object key is invalid");
        }
        return [key, normalize(entry, depth + 1)];
      }));
    }
    throw new Error("unsupported layout configuration value");
  }
  try {
    const normalized = normalize(value);
    if (Buffer.byteLength(JSON.stringify(normalized)) > 64 * 1024) throw new Error("layout configuration is too large");
    return normalized;
  } catch (error) {
    console.error(`pnpm returned an unsupported layout configuration value for ${name} (${error.message}).`);
    process.exit(1);
  }
}

const sharedValue = booleanConfiguration("sharedWorkspaceLockfile", true);
const shared = String(sharedValue);
const configuredGlobalVirtualStore = configurationValue("enableGlobalVirtualStore");
let globalVirtualStore = "false";
if (configuredGlobalVirtualStore === false) {
  // Explicit false is the only pnpm 11-safe global virtual-store contract.
} else if (configuredGlobalVirtualStore === true) {
  console.error(
    "pnpm enable-global-virtual-store=true is unsupported because installed dependencies " +
    "would resolve outside node_modules; add enableGlobalVirtualStore: false to pnpm-workspace.yaml."
  );
  process.exit(1);
} else if (configuredGlobalVirtualStore === undefined && Number(versionMatch[1]) < 11) {
  // pnpm 10's omitted legacy default is repository-local and normalizes false.
} else if (configuredGlobalVirtualStore === undefined) {
  console.error(
    `pnpm ${versionMatch[1]} requires an explicit enableGlobalVirtualStore: false setting in ` +
    "pnpm-workspace.yaml so dependency receipts attest a self-contained node_modules tree."
  );
  process.exit(1);
} else {
  console.error(
    `pnpm returned invalid enable-global-virtual-store value ${JSON.stringify(configuredGlobalVirtualStore)}.`
  );
  process.exit(1);
}

const nodeLinker = configurationValue("nodeLinker") ?? "isolated";
if (nodeLinker !== "isolated" && nodeLinker !== "hoisted") {
  console.error(`pnpm nodeLinker=${JSON.stringify(nodeLinker)} is unsupported; use isolated or hoisted node_modules.`);
  process.exit(1);
}
if (!booleanConfiguration("symlink", true) || !booleanConfiguration("enableModulesDir", true)) {
  console.error("pnpm dependency receipts require symlink=true and enableModulesDir=true.");
  process.exit(1);
}
const modulesDir = configurationValue("modulesDir") ?? "node_modules";
if (typeof modulesDir !== "string" || modulesDir.replaceAll("\\", "/").replace(/^\.\//, "") !== "node_modules") {
  console.error("A custom pnpm modulesDir is unsupported; dependency artifacts must use node_modules.");
  process.exit(1);
}
for (const name of ["virtualStoreOnly", "nodeExperimentalPackageMap"]) {
  const value = configurationValue(name);
  if (value !== undefined && value !== false) {
    console.error(`pnpm ${name} is unsupported because it can move executable authority outside the proved install tree.`);
    process.exit(1);
  }
}
for (const name of ["nodePackageMapType", "lockfileDir"]) {
  if (configurationValue(name) !== undefined) {
    console.error(`Custom pnpm ${name} is unsupported by dependency receipts.`);
    process.exit(1);
  }
}

function repositoryLocalVirtualStore(value) {
  if (
    typeof value !== "string" ||
    value.length === 0 ||
    Buffer.byteLength(value) > 4096 ||
    /[\0\r\n]/.test(value)
  ) throw new Error("invalid pnpm virtualStoreDir");
  const absolute = path.isAbsolute(value) ? path.resolve(value) : path.resolve(cwd, value);
  const relative = path.relative(repositoryRoot, absolute);
  if (
    relative === "" ||
    relative === ".." ||
    relative.startsWith(`..${path.sep}`) ||
    path.isAbsolute(relative)
  ) throw new Error("pnpm virtualStoreDir must be a dedicated repository-local path");
  let current = repositoryRoot;
  const components = relative.split(path.sep);
  for (let index = 0; index < components.length; index += 1) {
    current = path.join(current, components[index]);
    let stats;
    try {
      stats = fs.lstatSync(current);
    } catch (error) {
      if (error?.code === "ENOENT") break;
      throw error;
    }
    if (stats.isSymbolicLink() || (index + 1 < components.length && !stats.isDirectory())) {
      throw new Error("pnpm virtualStoreDir must not traverse symbolic links or non-directories");
    }
    const real = fs.realpathSync.native(current);
    const realRelative = path.relative(repositoryRoot, real);
    if (
      realRelative === ".." ||
      realRelative.startsWith(`..${path.sep}`) ||
      path.isAbsolute(realRelative)
    ) throw new Error("pnpm virtualStoreDir resolves outside the repository");
  }
  return relative.split(path.sep).join("/");
}

let virtualStoreDir = null;
const configuredVirtualStoreDir = configurationValue("virtualStoreDir");
if (configuredVirtualStoreDir !== undefined) {
  try {
    virtualStoreDir = repositoryLocalVirtualStore(configuredVirtualStoreDir);
  } catch (error) {
    console.error(`Unsupported pnpm virtualStoreDir for dependency scope ${scope} (${error.message}).`);
    process.exit(1);
  }
}

const normalizedLayout = {
  nodeLinker,
  symlink: true,
  enableModulesDir: true,
  modulesDir: "node_modules",
};
for (const name of layoutConfigurationNames.values()) {
  if ([
    "enableGlobalVirtualStore",
    "sharedWorkspaceLockfile",
    "nodeLinker",
    "symlink",
    "enableModulesDir",
    "modulesDir",
    "virtualStoreDir",
    "virtualStoreOnly",
    "nodeExperimentalPackageMap",
    "nodePackageMapType",
    "lockfileDir",
  ].includes(name)) continue;
  const value = boundedLayoutValue(name);
  if (value !== undefined) normalizedLayout[name] = value;
}
const layoutContract = Buffer.from(JSON.stringify({
  configuration: normalizedLayout,
  environment: Object.fromEntries(Object.entries(layoutEnvironment).sort(([left], [right]) => left.localeCompare(right))),
  virtualStoreDir,
})).toString("base64url");
const encodedPath = Buffer.from(executable.path).toString("base64url");
process.stdout.write(
  `${version}\t${versionMatch[1]}\t${shared}\t${globalVirtualStore}\t${encodedPath}\t${executable.digest}\t${layoutContract}\n`
);
NODE
}

pnpm_encode_contract() {
  "$node_bin" - "$@" <<'NODE'
const [
  spec,
  version,
  major,
  sharedWorkspaceLockfile,
  enableGlobalVirtualStore,
  executablePathEncoded,
  executableDigest,
  layoutEncoded,
  patchDigest,
  dependencyFree,
  inputDigest,
] = process.argv.slice(2);
let executablePath;
let layout;
try {
  executablePath = Buffer.from(executablePathEncoded, "base64url").toString("utf8");
  if (!/^[A-Za-z0-9_-]+$/.test(layoutEncoded) || layoutEncoded.length > 96 * 1024) process.exit(1);
  layout = JSON.parse(Buffer.from(layoutEncoded, "base64url").toString("utf8"));
} catch {
  process.exit(1);
}
const validObject = (value) =>
  typeof value === "object" && value !== null && !Array.isArray(value) &&
  Object.getPrototypeOf(value) === Object.prototype;
if (
  typeof spec !== "string" ||
  !spec.startsWith("pnpm@") ||
  spec.length > 512 ||
  /[\0-\x1f\x7f]/.test(spec) ||
  !/^[0-9A-Za-z.+-]+$/.test(version) ||
  !/^[0-9]+$/.test(major) ||
  !["true", "false"].includes(sharedWorkspaceLockfile) ||
  enableGlobalVirtualStore !== "false" ||
  !executablePath ||
  Buffer.byteLength(executablePath) > 4096 ||
  /[\0-\x1f\x7f]/.test(executablePath) ||
  !/^[0-9a-f]{64}$/.test(executableDigest) ||
  !validObject(layout) ||
  !validObject(layout.configuration) ||
  !validObject(layout.environment) ||
  (layout.virtualStoreDir !== null && (
    typeof layout.virtualStoreDir !== "string" ||
    layout.virtualStoreDir.length === 0 ||
    layout.virtualStoreDir.startsWith("/") ||
    /^[A-Za-z]:\//.test(layout.virtualStoreDir) ||
    layout.virtualStoreDir.split("/").some((segment) => !segment || segment === "." || segment === "..") ||
    /[\0\r\n]/.test(layout.virtualStoreDir)
  )) ||
  !/^[0-9a-f]{64}$/.test(patchDigest) ||
  !["true", "false"].includes(dependencyFree) ||
  (inputDigest !== "" && !/^[0-9a-f]{64}$/.test(inputDigest))
) process.exit(1);
const contract = {
  spec,
  version,
  major: Number(major),
  sharedWorkspaceLockfile: sharedWorkspaceLockfile === "true",
  enableGlobalVirtualStore: false,
  executablePath,
  executableDigest,
  layoutConfiguration: layout.configuration,
  layoutEnvironment: layout.environment,
  virtualStoreDir: layout.virtualStoreDir,
  patchDigest,
  dependencyFree: dependencyFree === "true",
  inputDigest: inputDigest || null,
};
process.stdout.write(`${Buffer.from(JSON.stringify(contract)).toString("base64url")}\n`);
NODE
}

pnpm_contract_is_dependency_free() {
  local contract="$1"

  "$node_bin" - "$contract" <<'NODE'
let contract;
try {
  contract = JSON.parse(Buffer.from(process.argv[2], "base64url").toString("utf8"));
} catch {
  process.exit(2);
}
if (contract?.dependencyFree === true) process.exit(0);
if (contract?.dependencyFree === false) process.exit(1);
process.exit(2);
NODE
}

pnpm_dependency_contract() {
  local app_dir="$1"
  local scope="$2"
  local spec runtime version major shared global_virtual_store executable_path executable_digest layout_contract semantics patch_digest dependency_free base_contract input_digest sentinel

  # Validate the authority path before invoking the package manager in it.
  pnpm_effective_manifest_path "$scope" >/dev/null || return
  spec="$(pnpm_package_manager_spec_for_scope "$scope")" || return
  runtime="$(pnpm_runtime_snapshot "$scope")" || return
  IFS=$'\t' read -r version major shared global_virtual_store executable_path executable_digest layout_contract <<< "$runtime"
  [ -n "$version" ] && [ -n "$major" ] && [ -n "$shared" ] && \
    [ "$global_virtual_store" = "false" ] && [ -n "$executable_path" ] && \
    [ -n "$executable_digest" ] && [ -n "$layout_contract" ] || return 1
  semantics="$(pnpm_patch_fingerprint "$scope" "$major" "$shared")" || return
  IFS=$'\t' read -r patch_digest dependency_free <<< "$semantics"
  [ -n "$patch_digest" ] && [ -n "$dependency_free" ] || return 1
  base_contract="$(pnpm_encode_contract \
    "$spec" "$version" "$major" "$shared" "$global_virtual_store" "$executable_path" "$executable_digest" "$layout_contract" \
    "$patch_digest" "$dependency_free" "")" || return
  sentinel=".agent/tmp/.jig-pnpm-no-lockfile.$$"
  [ ! -e "$sentinel" ] && [ ! -L "$sentinel" ] || return 1
  input_digest="$(dependency_fingerprint "$scope" "$sentinel" "$base_contract")" || return
  pnpm_encode_contract \
    "$spec" "$version" "$major" "$shared" "$global_virtual_store" "$executable_path" "$executable_digest" "$layout_contract" \
    "$patch_digest" "$dependency_free" "$input_digest"
}

[% endif %]
[% if web_package_manager == "yarn" %]
yarn_package_manager_spec() {
  local app_dir="$1"
  local scope status

  scope="$(dependency_scope "$app_dir")" || {
    status=$?
    [ "$status" -eq 1 ] && status=2
    return "$status"
  }
  yarn_package_manager_spec_for_scope "$app_dir" "$scope" || {
    status=$?
    [ "$status" -eq 1 ] && status=2
    return "$status"
  }
}

yarn_package_manager_spec_for_scope() {
  local scope="$1"
  local lock_scope="${2:-$scope}"

  validate_yarn_scope_authorities "$scope" || return
  "$node_bin" - "$scope" "$lock_scope" <<'NODE'
const fs = require("node:fs");
const path = require("node:path");

const scope = process.argv[2];
const lockScope = process.argv[3];
const repositoryRoot = fs.realpathSync.native(".");
function verifiedScope(value, description) {
  const absolute = path.resolve(repositoryRoot, value);
  const relative = path.relative(repositoryRoot, absolute);
  if (relative === ".." || relative.startsWith(`..${path.sep}`) || path.isAbsolute(relative)) {
    console.error(`Yarn ${description} escapes the repository.`);
    process.exit(1);
  }
  let verified = repositoryRoot;
  for (const component of relative.split(path.sep).filter(Boolean)) {
    verified = path.join(verified, component);
    const stats = fs.lstatSync(verified);
    if (stats.isSymbolicLink() || !stats.isDirectory()) {
      console.error(`Yarn ${description} must not traverse a symbolic link or non-directory: ${verified}`);
      process.exit(1);
    }
  }
  return absolute;
}
const absoluteScope = verifiedScope(scope, "authority scope");
const absoluteLockScope = verifiedScope(lockScope, "lockfile scope");

const packagePaths = [];
for (let current = absoluteScope; ; current = path.dirname(current)) {
  packagePaths.push(path.join(current, "package.json"));
  if (current === repositoryRoot) break;
}

function validSpec(value) {
  return typeof value === "string" &&
    value.startsWith("yarn@") &&
    value.length > "yarn@".length &&
    Buffer.byteLength(value) <= 512 &&
    !/[\0-\x1f\x7f]/.test(value);
}

for (const packagePath of packagePaths) {
  let stats;
  try {
    stats = fs.lstatSync(packagePath);
  } catch (error) {
    if (error?.code === "ENOENT") continue;
    throw error;
  }
  if (stats.isSymbolicLink() || !stats.isFile()) {
    console.error(`${packagePath} must be a real package.json file.`);
    process.exit(1);
  }
  let packageJson;
  try {
    packageJson = JSON.parse(fs.readFileSync(packagePath, "utf8"));
  } catch (error) {
    console.error(`${packagePath} is not valid JSON (${error.message}).`);
    process.exit(1);
  }
  const topLevel = packageJson.packageManager;
  if (validSpec(topLevel)) {
    process.stdout.write(`${topLevel}\n`);
    process.exit(0);
  }
  if (topLevel !== undefined) {
    console.error(`${packagePath} has invalid or non-Yarn packageManager authority.`);
    process.exit(1);
  }
  const devManager = packageJson.devEngines?.packageManager;
  const devSpec = devManager?.name === "yarn" && typeof devManager.version === "string"
    ? `yarn@${devManager.version}`
    : null;
  if (validSpec(devSpec)) {
    process.stdout.write(`${devSpec}\n`);
    process.exit(0);
  }
  if (devManager !== undefined) {
    console.error(`${packagePath} has invalid or non-Yarn devEngines.packageManager authority.`);
    process.exit(1);
  }
}

const lockPath = path.join(absoluteLockScope, "yarn.lock");
let lock = "";
try {
  const lockStats = fs.lstatSync(lockPath);
  if (lockStats.isSymbolicLink() || !lockStats.isFile()) {
    console.error(`${lockPath} must be a real yarn.lock file.`);
    process.exit(1);
  }
  const realLockPath = fs.realpathSync.native(lockPath);
  const lockRelative = path.relative(repositoryRoot, realLockPath);
  if (
    lockRelative === ".." ||
    lockRelative.startsWith(`..${path.sep}`) ||
    path.isAbsolute(lockRelative)
  ) {
    console.error(`${lockPath} resolves outside the repository.`);
    process.exit(1);
  }
  lock = fs.readFileSync(lockPath, "utf8");
} catch (error) {
  if (error?.code !== "ENOENT") throw error;
}
const fallback = /^# yarn lockfile v1\r?$/m.test(lock)
  ? "yarn@1.22.22"
  : "<<[ web_package_manager_spec ]>>";
process.stdout.write(`${fallback}\n`);
NODE
}

yarn_runtime_identity() {
  local scope="$1"
  local lock_scope="${2:-$scope}"
  local spec

  validate_yarn_scope_authorities "$scope" || return
  spec="$(yarn_package_manager_spec_for_scope "$scope" "$lock_scope")" || return
  "$node_bin" - --jig-yarn-runtime "$scope" "$spec" <<'NODE'
const { createHash } = require("node:crypto");
const { spawnSync } = require("node:child_process");
const fs = require("node:fs");
const path = require("node:path");

const cwd = path.resolve(process.argv[3]);
const spec = process.argv[4];
if (
  !spec.startsWith("yarn@") ||
  spec.length <= "yarn@".length ||
  Buffer.byteLength(spec) > 512 ||
  /[\0-\x1f\x7f]/.test(spec)
) process.exit(1);

function environmentValue(name) {
  const key = Object.keys(process.env).find((candidate) => candidate.toUpperCase() === name);
  return key ? process.env[key] : undefined;
}

function validateWindowsCommandInterpreter(interpreter) {
  const driveAbsolute =
    typeof interpreter === "string" && /^[A-Za-z]:[\\/]/.test(interpreter);
  const uncAbsolute =
    typeof interpreter === "string" &&
    /^[\\/]{2}[^\\/]+[\\/][^\\/]+(?:[\\/].*)?$/.test(interpreter);
  if (
    !interpreter ||
    /[\0-\x1f\x7f"]/.test(interpreter) ||
    /^[\\/]{2}[?.][\\/]/.test(interpreter) ||
    (!driveAbsolute && !uncAbsolute)
  ) {
    throw new Error("unsafe Windows command interpreter");
  }
  return interpreter;
}

function windowsPathEntries(value) {
  const entries = [];
  let entry = "";
  let quoted = false;
  for (const character of value) {
    if (character === '"') quoted = !quoted;
    else if (character === path.delimiter && !quoted) {
      entries.push(entry);
      entry = "";
    } else entry += character;
  }
  if (quoted) throw new Error("unsafe quoted Windows PATH");
  entries.push(entry);
  return entries;
}

function windowsExtensions(command) {
  if (path.extname(command)) return [""];
  const configured = environmentValue("PATHEXT");
  const raw = configured === undefined ? ".COM;.EXE;.BAT;.CMD" : configured;
  const extensions = raw.split(";").filter((extension) => extension !== "");
  if (extensions.length === 0 || extensions.some((extension) => !/^\.[A-Za-z0-9]+$/.test(extension))) {
    throw new Error("unsafe Windows PATHEXT");
  }
  return [...new Map(extensions.map((extension) => [extension.toUpperCase(), extension])).values()];
}

function resolveExecutable(command) {
  if (typeof command !== "string" || !command || /[\0-\x1f\x7f"]/.test(command) || /[\\/]/.test(command)) {
    throw new Error("unsafe Yarn executable name");
  }
  const pathValue = environmentValue("PATH") || "";
  const entries = process.platform === "win32" ? windowsPathEntries(pathValue) : pathValue.split(path.delimiter);
  const extensions = process.platform === "win32" ? windowsExtensions(command) : [""];
  for (const entry of entries) {
    const directory = entry
      ? (path.isAbsolute(entry) ? entry : path.resolve(cwd, entry))
      : cwd;
    for (const extension of extensions) {
      const candidate = path.resolve(directory, `${command}${extension}`);
      try {
        const stats = fs.statSync(candidate);
        if (!stats.isFile()) continue;
        if (process.platform !== "win32") fs.accessSync(candidate, fs.constants.X_OK);
        const resolved = fs.realpathSync.native(candidate);
        if (!fs.statSync(resolved).isFile()) continue;
        if (/[\0-\x1f\x7f"]/.test(resolved)) {
          throw new Error("unsafe Yarn executable path");
        }
        return resolved;
      } catch {
        // Bare PATH lookup skips an unusable entry and continues in order.
      }
    }
  }
  throw new Error("Yarn executable was not found on PATH");
}

const resolved = resolveExecutable("yarn");
function encodeBatchArgument(argument, forceQuote = false) {
  if (/[\0\r\n]/.test(argument)) throw new Error("unsafe Yarn batch argument");
  const safeUnquoted = "#$*+-./:?@\\_";
  const quote = forceQuote || argument.length === 0 || argument.endsWith("\\") ||
    [...argument].some((character) => {
      const code = character.codePointAt(0);
      return code < 32 || code === 127 ||
        (code < 128 && !/[A-Za-z0-9]/.test(character) && !safeUnquoted.includes(character));
    });
  let encoded = quote ? '"' : "";
  let backslashes = 0;
  for (const character of argument) {
    if (character === "\\") {
      backslashes += 1;
      encoded += character;
      continue;
    }
    if (character === '"') {
      encoded += "\\".repeat(backslashes);
      encoded += '"';
    } else if (character === "%") {
      encoded += "%%cd:~,";
    }
    backslashes = 0;
    encoded += character;
  }
  if (quote) encoded += "\\".repeat(backslashes) + '"';
  return encoded;
}

function runtimeVersion() {
  const options = {
    cwd,
    encoding: "utf8",
    maxBuffer: 1024,
    shell: false,
    timeout: 30_000,
  };
  let result;
  if (process.platform === "win32" && /\.(?:cmd|bat)$/i.test(resolved)) {
    const commandInterpreter = validateWindowsCommandInterpreter(environmentValue("COMSPEC"));
    const commandLine = `"${encodeBatchArgument(resolved, true)} --version"`;
    result = spawnSync(
      commandInterpreter,
      ["/d", "/s", "/v:off", "/c", commandLine],
      { ...options, windowsVerbatimArguments: true }
    );
  } else {
    result = spawnSync(resolved, ["--version"], options);
  }
  if (result.error || result.signal || result.status !== 0 || typeof result.stdout !== "string") {
    throw new Error("could not resolve Yarn runtime version");
  }
  const lines = result.stdout.replace(/\r/g, "").split("\n");
  if (lines.at(-1) === "") lines.pop();
  if (lines.length !== 1 || Buffer.byteLength(lines[0]) > 512 || !/^[0-9A-Za-z.+-]+$/.test(lines[0])) {
    throw new Error("invalid Yarn runtime version");
  }
  return lines[0];
}

const version = runtimeVersion();
const stats = fs.statSync(resolved);
if (!stats.isFile() || stats.size > 256 * 1024 * 1024 || /[\0-\x1f\x7f"]/.test(resolved)) {
  process.exit(1);
}
const hash = createHash("sha256");
hash.update("jig-yarn-runtime-v1\0");
for (const value of [spec, version, process.platform, process.arch, resolved]) {
  hash.update(value);
  hash.update("\0");
}
const descriptor = fs.openSync(resolved, "r");
const buffer = Buffer.allocUnsafe(64 * 1024);
try {
  let offset = 0;
  while (offset < stats.size) {
    const length = fs.readSync(descriptor, buffer, 0, buffer.length, offset);
    if (length <= 0) throw new Error("Yarn executable changed while hashing");
    hash.update(buffer.subarray(0, length));
    offset += length;
  }
  if (fs.fstatSync(descriptor).size !== stats.size) {
    throw new Error("Yarn executable changed while hashing");
  }
} finally {
  fs.closeSync(descriptor);
}
process.stdout.write(`${hash.digest("hex")}\n`);
NODE
}

[% endif %]
dependency_lockfile() {
  local scope="$1"

  if [ "$scope" = "." ]; then
    root_lockfile
  else
    app_lockfile "$scope"
  fi
}

dependency_stamp_path() {
  local scope="$1"

  if [ "$scope" = "." ]; then
    printf '%s\n' "$dependency_stamp_dir/root.sha256"
  else
    printf '%s/apps/%s.sha256\n' "$dependency_stamp_dir" "$scope"
  fi
}

fingerprint_files() {
  "$node_bin" - "${JIG_FINGERPRINT_SUPPLEMENTAL:-}" "$@" <<'NODE'
const { createHash } = require("node:crypto");
const fs = require("node:fs");
const path = require("node:path");

const repositoryRoot = fs.realpathSync.native(".");

function repositoryPath(input) {
  if (typeof input !== "string" || !input || /[\0\r\n]/.test(input)) {
    throw new Error("Dependency fingerprint inputs must be nonempty filesystem paths");
  }
  const absolute = path.isAbsolute(input)
    ? path.resolve(input)
    : path.resolve(repositoryRoot, input);
  const relative = path.relative(repositoryRoot, absolute);
  if (
    relative === "" ||
    relative === ".." ||
    relative.startsWith(`..${path.sep}`) ||
    path.isAbsolute(relative)
  ) {
    throw new Error(`Dependency fingerprint input must remain inside the repository: ${input}`);
  }
  return { absolute, relative };
}

function collectFiles(input) {
  const candidate = repositoryPath(input);
  const components = candidate.relative.split(path.sep);
  let current = repositoryRoot;
  let stats;
  for (let index = 0; index < components.length; index += 1) {
    current = path.join(current, components[index]);
    try {
      stats = fs.lstatSync(current);
    } catch (error) {
      if (error?.code === "ENOENT" || error?.code === "ENOTDIR") return [];
      throw error;
    }
    if (stats.isSymbolicLink()) {
      throw new Error(`Dependency fingerprint input must not traverse a symbolic link: ${input}`);
    }
    if (index + 1 < components.length && !stats.isDirectory()) {
      throw new Error(`Dependency fingerprint input must traverse real directories: ${input}`);
    }
  }
  if (stats.isFile()) return [candidate];
  if (!stats.isDirectory()) return [];

  return fs.readdirSync(candidate.absolute).flatMap((entry) =>
    collectFiles(path.join(candidate.absolute, entry))
  );
}

const hash = createHash("sha256");
hash.update("jig-web-dependencies-v5\0");
[% if web_package_manager == "pnpm" %]
hash.update("pnpm-runtime-contract-v1\0");
[% else %]
hash.update("<<[ web_package_manager_spec ]>>\0");
[% endif %]
hash.update(process.argv[2]);
hash.update("\0");
const files = process.argv.slice(3).flatMap(collectFiles).sort((left, right) =>
  left.relative < right.relative ? -1 : left.relative > right.relative ? 1 : 0
);
for (const file of files) {
  hash.update(file.relative.split(path.sep).join("/"));
  hash.update("\0");
  hash.update(fs.readFileSync(file.absolute));
  hash.update("\0");
}

process.stdout.write(`${hash.digest("hex")}\n`);
NODE
}

[% if web_package_manager == "yarn" %]
validate_yarn_scope_authorities() {
  local scope="$1"

  "$node_bin" - --jig-yarn-authority-preflight "$scope" <<'NODE'
const fs = require("node:fs");
const path = require("node:path");

const repositoryRoot = fs.realpathSync.native(".");
const inputScope = process.argv[3];
const absoluteScope = path.isAbsolute(inputScope)
  ? path.resolve(inputScope)
  : path.resolve(repositoryRoot, inputScope);
const relativeScope = path.relative(repositoryRoot, absoluteScope);
if (
  !inputScope ||
  /[\0\r\n]/.test(inputScope) ||
  relativeScope === ".." ||
  relativeScope.startsWith(`..${path.sep}`) ||
  path.isAbsolute(relativeScope)
) {
  console.error(`Yarn authority scope must remain inside the repository: ${inputScope}`);
  process.exit(1);
}

function validateExistingPath(candidate, description, expectedType = null) {
  const relative = path.relative(repositoryRoot, candidate);
  if (
    relative === "" ||
    relative === ".." ||
    relative.startsWith(`..${path.sep}`) ||
    path.isAbsolute(relative)
  ) {
    throw new Error(`${description} escapes the repository`);
  }
  const components = relative.split(path.sep);
  let current = repositoryRoot;
  let stats;
  for (let index = 0; index < components.length; index += 1) {
    current = path.join(current, components[index]);
    try {
      stats = fs.lstatSync(current);
    } catch (error) {
      if (error?.code === "ENOENT" || error?.code === "ENOTDIR") return false;
      throw error;
    }
    if (stats.isSymbolicLink()) {
      throw new Error(`${description} must not traverse a symbolic link`);
    }
    if (index + 1 < components.length && !stats.isDirectory()) {
      throw new Error(`${description} must traverse real directories`);
    }
  }
  if (
    (expectedType === "directory" && !stats.isDirectory()) ||
    (expectedType === "file" && !stats.isFile()) ||
    (expectedType === null && !stats.isFile() && !stats.isDirectory())
  ) {
    throw new Error(`${description} has an unsupported filesystem type`);
  }
  return true;
}

function stripUnquotedComment(value) {
  let quote = null;
  for (let index = 0; index < value.length; index += 1) {
    const character = value[index];
    if (quote === '"' && character === "\\") {
      index += 1;
    } else if (quote === "'" && character === "'" && value[index + 1] === "'") {
      index += 1;
    } else if (quote && character === quote) {
      quote = null;
    } else if (!quote && (character === '"' || character === "'")) {
      quote = character;
    } else if (!quote && character === "#" && (index === 0 || /\s/.test(value[index - 1]))) {
      return value.slice(0, index).trimEnd();
    }
  }
  if (quote) throw new Error("unterminated quoted Yarn configuration value");
  return value;
}

function configuredString(rawValue, description) {
  const value = stripUnquotedComment(rawValue).trim();
  if (!value) throw new Error(`${description} is empty`);
  if (/^(?:null|~)$/i.test(value)) return null;
  if (value.startsWith('"')) {
    if (!value.endsWith('"')) throw new Error(`${description} has an unterminated string`);
    let parsed;
    try {
      parsed = JSON.parse(value);
    } catch (error) {
      throw new Error(`${description} has an invalid quoted string (${error.message})`);
    }
    if (typeof parsed !== "string") throw new Error(`${description} must be a string`);
    return parsed;
  }
  if (value.startsWith("'")) {
    if (!value.endsWith("'")) throw new Error(`${description} has an unterminated string`);
    return value.slice(1, -1).replaceAll("''", "'");
  }
  if (["&", "*", "!", ">", "|", "[", "]", "{", "}"].includes(value[0]) || /:(?:\s|$)/.test(value)) {
    throw new Error(`${description} uses unsupported YAML indirection or collection syntax`);
  }
  return value;
}

function validateConfiguredRuntimePath(configFile, configuredPath, description) {
  if (configuredPath === null) return;
  if (
    typeof configuredPath !== "string" ||
    !configuredPath ||
    /[\0\r\n$%]/.test(configuredPath)
  ) {
    throw new Error(`${description} must be a literal repository path`);
  }
  const candidate = path.isAbsolute(configuredPath)
    ? path.resolve(configuredPath)
    : path.resolve(path.dirname(configFile), configuredPath);
  if (!validateExistingPath(candidate, description, "file")) {
    throw new Error(`${description} does not exist`);
  }
}

function validateBerryRuntimePaths(configFile) {
  const lines = fs.readFileSync(configFile, "utf8").replaceAll("\r", "").split("\n");
  let pluginsIndent = null;
  let sawYarnPath = false;
  let sawPlugins = false;
  for (const [lineIndex, rawLine] of lines.entries()) {
    if (rawLine.includes("\t")) {
      throw new Error(`${configFile}:${lineIndex + 1} uses unsupported YAML tabs`);
    }
    const line = stripUnquotedComment(rawLine);
    if (!line.trim()) continue;
    const indent = line.length - line.trimStart().length;
    const content = line.trim();
    if (indent === 0) {
      if (
        ["{", "}", "[", "]", "?", "&", "*", "!", "%", "\"", "'"].includes(content[0]) ||
        /^<<\s*:/.test(content) ||
        (content.startsWith("---") && content !== "---")
      ) {
        throw new Error(`${configFile}:${lineIndex + 1} uses unsupported top-level YAML collection, tag, alias, or merge syntax`);
      }
      pluginsIndent = null;
      const yarnPath = /^yarnPath\s*:(.*)$/.exec(content);
      if (yarnPath) {
        if (sawYarnPath) throw new Error(`${configFile} declares yarnPath more than once`);
        sawYarnPath = true;
        validateConfiguredRuntimePath(
          configFile,
          configuredString(yarnPath[1], `${configFile}:${lineIndex + 1} yarnPath`),
          `${configFile}:${lineIndex + 1} yarnPath`
        );
        continue;
      }
      if (/^["']?yarnPath["']?\s*:/.test(content)) {
        throw new Error(`${configFile}:${lineIndex + 1} uses unsupported yarnPath syntax`);
      }
      const plugins = /^plugins\s*:(.*)$/.exec(content);
      if (plugins) {
        if (sawPlugins) throw new Error(`${configFile} declares plugins more than once`);
        sawPlugins = true;
        const inlineValue = plugins[1].trim();
        if (inlineValue && inlineValue !== "[]") {
          throw new Error(`${configFile}:${lineIndex + 1} uses unsupported inline plugins syntax`);
        }
        pluginsIndent = inlineValue === "[]" ? null : indent;
        continue;
      }
      if (/^["']?plugins["']?\s*:/.test(content)) {
        throw new Error(`${configFile}:${lineIndex + 1} uses unsupported plugins syntax`);
      }
    } else if (pluginsIndent !== null && indent > pluginsIndent) {
      const pluginContent = content.replace(/^-\s*/, "");
      if (
        ["{", "}", "[", "]", "?", "&", "*", "!", "%", "\"", "'"].includes(pluginContent[0]) ||
        /^<<\s*:/.test(pluginContent)
      ) {
        throw new Error(`${configFile}:${lineIndex + 1} uses unsupported plugin collection, tag, explicit key, alias, or merge syntax`);
      }
      const pluginPath = /^(?:-\s*)?path\s*:(.*)$/.exec(content);
      if (pluginPath) {
        validateConfiguredRuntimePath(
          configFile,
          configuredString(pluginPath[1], `${configFile}:${lineIndex + 1} plugin path`),
          `${configFile}:${lineIndex + 1} plugin path`
        );
      } else if (/(?:^|[,{\s])["']?path["']?\s*:/.test(content)) {
        throw new Error(`${configFile}:${lineIndex + 1} uses unsupported plugin path syntax`);
      }
    } else if (/^["']?yarnPath["']?\s*:/.test(content)) {
      throw new Error(`${configFile}:${lineIndex + 1} uses unsupported nested yarnPath syntax`);
    }
  }
}

function validateClassicRuntimePaths(configFile) {
  const lines = fs.readFileSync(configFile, "utf8").replaceAll("\r", "").split("\n");
  for (const [lineIndex, rawLine] of lines.entries()) {
    const content = stripUnquotedComment(rawLine).trim();
    if (!content) continue;
    const yarnPath = /^(?:--)?yarn-path(?:\s+|\s*=\s*)(.+)$/i.exec(content);
    if (yarnPath) {
      validateConfiguredRuntimePath(
        configFile,
        configuredString(yarnPath[1], `${configFile}:${lineIndex + 1} yarn-path`),
        `${configFile}:${lineIndex + 1} yarn-path`
      );
    } else if (
      /(?:^|[^a-z0-9_-])(?:--)?yarn-path(?=$|[^a-z0-9_-])/i.test(
        content.replace(/["'\\]/g, "")
      )
    ) {
      throw new Error(`${configFile}:${lineIndex + 1} uses unsupported yarn-path syntax`);
    }
  }
}

try {
  for (const environmentName of [
    "YARN_RC_FILENAME",
    "YARN_YARN_PATH",
    "YARN_PLUGINS",
    "NPM_CONFIG_YARN_PATH",
  ]) {
    const key = Object.keys(process.env).find(
      (candidate) => candidate.toUpperCase() === environmentName
    );
    if (key && process.env[key]?.trim()) {
      throw new Error(`${environmentName} is not supported by the Yarn dependency harness`);
    }
  }
  if (
    absoluteScope !== repositoryRoot &&
    !validateExistingPath(absoluteScope, `Yarn authority scope ${inputScope}`, "directory")
  ) {
    throw new Error(`Yarn authority scope ${inputScope} does not exist`);
  }
  const directories = [];
  const classicConfigs = [];
  const berryConfigs = [];
  for (let current = absoluteScope; ; current = path.dirname(current)) {
    directories.push(current);
    if (current === repositoryRoot) break;
  }
  for (const directory of directories.reverse()) {
    for (const [authority, expectedType] of [
      ["package.json", "file"],
      [".node-version", "file"],
      [".npmrc", "file"],
      [".yarnrc", "file"],
      [".yarnrc.yml", "file"],
      [".yarn", "directory"],
      [".yarn/patches", "directory"],
      [".yarn/plugins", "directory"],
      [".yarn/releases", "directory"],
    ]) {
      const authorityPath = path.join(directory, authority);
      const exists = validateExistingPath(
        authorityPath,
        `Yarn authority ${path.relative(repositoryRoot, authorityPath)}`,
        expectedType
      );
      if (exists && authority === ".yarnrc") classicConfigs.push(authorityPath);
      if (exists && authority === ".yarnrc.yml") berryConfigs.push(authorityPath);
    }
  }
  for (const configFile of classicConfigs) validateClassicRuntimePaths(configFile);
  for (const configFile of berryConfigs) validateBerryRuntimePaths(configFile);
} catch (error) {
  console.error(error.message);
  process.exit(1);
}
NODE
}

yarn_scope_authority_paths() {
  local scope="$1"
  local relative_path="$2"
  local current parent repository_root index
  local -a paths=()

  repository_root="$(pwd -P)" || return
  current="$(cd "$scope" && pwd -P)" || return
  case "$current" in
    "$repository_root"|"$repository_root"/*) ;;
    *) return 1 ;;
  esac
  while :; do
    if [ -e "$current/$relative_path" ] || [ -L "$current/$relative_path" ]; then
      paths+=("$current/$relative_path")
    fi
    [ "$current" = "$repository_root" ] && break
    parent="${current%/*}"
    [ -n "$parent" ] || parent="/"
    current="$parent"
  done
  for ((index=${#paths[@]} - 1; index >= 0; index--)); do
    printf '%s\n' "${paths[$index]}"
  done
}
[% endif %]

dependency_scope_manifest_paths() {
  local scope="$1"
  local manifests manifest root_manifest

  if [ "$scope" = "." ]; then
[% if web_package_manager == "pnpm" %]
    root_manifest="$(pnpm_effective_manifest_path ".")" || return
    printf '%s\n' "$root_manifest"
[% else %]
    printf '%s\n' "package.json"
[% endif %]
    manifests="$(root_workspace_manifest_paths)" || return
    while IFS= read -r manifest; do
      [ -n "$manifest" ] && printf '%s\n' "$manifest"
    done <<< "$manifests"
  else
[% if web_package_manager == "pnpm" %]
    pnpm_effective_manifest_path "$scope"
[% else %]
    printf '%s/package.json\n' "$scope"
[% endif %]
  fi
}

[% if web_package_manager == "pnpm" %]
pnpm_patch_fingerprint() {
  local scope="$1"
  local runtime_major="$2"
  local shared_workspace_lockfile="$3"
  local manifests manifest
  local -a scope_manifests=()

  manifests="$(dependency_scope_manifest_paths "$scope")" || return
  while IFS= read -r manifest; do
    [ -n "$manifest" ] && scope_manifests+=("$manifest")
  done <<< "$manifests"
  [ "${#scope_manifests[@]}" -gt 0 ] || return 1
  workspace_metadata patch-fingerprint \
    "$scope" \
    "$runtime_major" \
    "$shared_workspace_lockfile" \
    "${scope_manifests[@]}"
}

[% endif %]
dependency_scope_is_dependency_free() {
  local scope="$1"
  local effective_config="${2:-}"
  local manifests manifest
  local -a inputs=()

[% if web_package_manager == "pnpm" %]
  [ -n "$effective_config" ] || return 2
  pnpm_contract_is_dependency_free "$effective_config"
  return
[% else %]
  manifests="$(dependency_scope_manifest_paths "$scope")" || return
  while IFS= read -r manifest; do
    [ -n "$manifest" ] && inputs+=("$manifest")
  done <<< "$manifests"
  [ "${#inputs[@]}" -gt 0 ] || return 1
  "$node_bin" - --jig-dependency-free "${inputs[@]}" <<'NODE'
const fs = require("node:fs");

const dependencyFields = [
  "dependencies",
  "devDependencies",
  "optionalDependencies",
  "peerDependencies",
];
for (const manifest of process.argv.slice(3)) {
  const stats = fs.lstatSync(manifest);
  if (!stats.isFile() || stats.isSymbolicLink()) process.exit(2);
  const packageJson = JSON.parse(fs.readFileSync(manifest, "utf8"));
  for (const field of dependencyFields) {
    const dependencies = packageJson[field];
    if (
      dependencies !== undefined &&
      (typeof dependencies !== "object" || dependencies === null || Array.isArray(dependencies))
    ) process.exit(2);
    if (dependencies && Object.keys(dependencies).length > 0) process.exit(1);
  }
}
process.exit(0);
NODE
[% endif %]
}

dependency_fingerprint() {
  local scope="$1"
  local lockfile="$2"
  local effective_config="${3:-}"
  local app_dir app_scope manifests manifest root_manifest workspace_dir authority authority_paths config runtime status
  local yarn_runtime_identities=""
  local -a inputs

  if [ "$scope" = "." ]; then
[% if web_package_manager == "pnpm" %]
    root_manifest="$(pnpm_effective_manifest_path ".")" || return
    inputs=("$lockfile" "$root_manifest" ".node-version" ".npmrc" "pnpm-workspace.yaml" ".pnpmfile.cjs" "pnpmfile.cjs" "patches")
[% else %]
    inputs=("$lockfile" "package.json" ".node-version"[% if web_package_manager == "npm" %] ".npmrc"[% elif web_package_manager == "pnpm" %] ".npmrc" "pnpm-workspace.yaml" ".pnpmfile.cjs" "pnpmfile.cjs" "patches"[% elif web_package_manager == "bun" %] ".npmrc" "bunfig.toml" "patches"[% elif web_package_manager == "yarn" %] ".npmrc" ".yarn/patches" ".yarn/plugins" ".yarn/releases"[% endif %])
[% endif %]
    manifests="$(root_workspace_manifest_paths)" || return
    while IFS= read -r manifest; do
      [ -n "$manifest" ] || continue
      inputs+=("$manifest")
[% if web_package_manager == "pnpm" or web_package_manager == "bun" %]
      workspace_dir="$(dirname "$manifest")"
      inputs+=("$workspace_dir/patches")
[% endif %]
    done <<< "$manifests"
[% for app in frontend_apps %]
    app_dir="<<[ app.dir ]>>"
    app_scope="$(dependency_scope "$app_dir")" || {
      status=$?
      [ "$status" -eq 1 ] && status=2
      return "$status"
    }
    if [ "$app_scope" = "." ]; then
[% if web_package_manager == "pnpm" %]
      inputs+=("$app_dir/.node-version")
[% else %]
      inputs+=("$app_dir/package.json" "$app_dir/.node-version")
[% endif %]
[% if web_package_manager == "yarn" %]
      for authority in package.json .node-version .npmrc .yarnrc .yarnrc.yml .yarn/patches .yarn/plugins .yarn/releases; do
        authority_paths="$(yarn_scope_authority_paths "$app_dir" "$authority")" || return
        while IFS= read -r config; do [ -n "$config" ] && inputs+=("$config"); done <<< "$authority_paths"
      done
      runtime="$(yarn_runtime_identity "$app_dir" "$scope")" || return
      yarn_runtime_identities="${yarn_runtime_identities}:${runtime}"
[% endif %]
    fi
[% endfor %]
[% if web_package_manager == "yarn" %]
    for authority in package.json .node-version .npmrc .yarnrc .yarnrc.yml .yarn/patches .yarn/plugins .yarn/releases; do
      authority_paths="$(yarn_scope_authority_paths "$scope" "$authority")" || return
      while IFS= read -r config; do [ -n "$config" ] && inputs+=("$config"); done <<< "$authority_paths"
    done
    runtime="$(yarn_runtime_identity "$scope")" || return
    JIG_FINGERPRINT_SUPPLEMENTAL="${effective_config}:${runtime}${yarn_runtime_identities}" fingerprint_files "${inputs[@]}"
[% elif web_package_manager == "pnpm" %]
    [ -n "$effective_config" ] || return 1
    JIG_FINGERPRINT_SUPPLEMENTAL="$effective_config" fingerprint_files "${inputs[@]}"
[% else %]
    fingerprint_files "${inputs[@]}"
[% endif %]
  else
[% if web_package_manager == "yarn" %]
    inputs=("$lockfile")
    for authority in package.json .node-version .npmrc .yarnrc .yarnrc.yml .yarn/patches .yarn/plugins .yarn/releases; do
      authority_paths="$(yarn_scope_authority_paths "$scope" "$authority")" || return
      while IFS= read -r config; do [ -n "$config" ] && inputs+=("$config"); done <<< "$authority_paths"
    done
    runtime="$(yarn_runtime_identity "$scope")" || return
    JIG_FINGERPRINT_SUPPLEMENTAL="${effective_config}:${runtime}" fingerprint_files "${inputs[@]}"
[% elif web_package_manager == "pnpm" %]
    manifest="$(pnpm_effective_manifest_path "$scope")" || return
    inputs=("$lockfile" "package.json" ".node-version" "$manifest" "$scope/.node-version" ".npmrc" "pnpm-workspace.yaml" ".pnpmfile.cjs" "pnpmfile.cjs" "patches" "$scope/.npmrc" "$scope/pnpm-workspace.yaml" "$scope/.pnpmfile.cjs" "$scope/pnpmfile.cjs" "$scope/patches")
    [ -n "$effective_config" ] || return 1
    JIG_FINGERPRINT_SUPPLEMENTAL="$effective_config" fingerprint_files "${inputs[@]}"
[% else %]
    fingerprint_files "$lockfile" "package.json" ".node-version" "$scope/package.json" "$scope/.node-version"[% if web_package_manager == "npm" %] ".npmrc" "$scope/.npmrc"[% elif web_package_manager == "bun" %] ".npmrc" "bunfig.toml" "patches" "$scope/.npmrc" "$scope/bunfig.toml" "$scope/patches"[% endif %]
[% endif %]
  fi
}

node_modules_receipt_path() {
  local scope="$1"

  printf '%s/node_modules/.jig-web-dependencies-v3\n' "$scope"
}

path_is_real_directory() {
  [ -d "$1" ] && [ ! -L "$1" ]
}

path_is_nonempty_real_file() {
  [ -s "$1" ] && [ ! -L "$1" ]
}

node_modules_structure_proof() {
  local scope="$1"
  local effective_config="${2:-}"
  local manifests manifest
  # Keep this argv array nonempty. Bash 3.2 treats an empty "${array[@]}"
  # expansion as an unbound variable under `set -u`.
  local -a proof_args=(- --jig-node-modules-proof "$scope" "$effective_config")

  if [ "$scope" = "." ]; then
    manifests="$(root_workspace_manifest_paths)" || return
    while IFS= read -r manifest; do
      [ -n "$manifest" ] && proof_args+=("$manifest")
    done <<< "$manifests"
  fi

  "$node_bin" "${proof_args[@]}" <<'NODE'
const { createHash } = require("node:crypto");
const fs = require("node:fs");
const path = require("node:path");

const repositoryRoot = fs.realpathSync.native(".");
const scope = process.argv[3];
const effectiveConfig = process.argv[4];
const memberManifests = process.argv.slice(5);
const receipt = ".jig-web-dependencies-v3";
// Vite, Vitest, TypeScript, and related tools write these caches inside the
// nearest install root. They are runtime output, not installed dependencies.
const runtimeCacheDirectories = new Set([".cache", ".vite", ".vite-temp", ".tmp"]);
[% if web_package_manager == "pnpm" %]
// pnpm 10 and 11 rewrite these validation caches (including a timestamp) during
// read-only commands. Their authoritative settings and installed package tree
// are attested separately below.
const volatilePnpmWorkspaceStates = new Set([
  ".pnpm-workspace-state.json",
  ".pnpm-workspace-state-v1.json",
]);
[% endif %]
const hash = createHash("sha256");
hash.update("jig-node-modules-structure-v2\0");
let meaningfulEntries = 0;

function record(kind, ...values) {
  hash.update(kind);
  hash.update("\0");
  for (const value of values) {
    const bytes = Buffer.isBuffer(value) ? value : Buffer.from(String(value));
    hash.update(String(bytes.length));
    hash.update("\0");
    hash.update(bytes);
    hash.update("\0");
  }
}

function repositoryPath(input, description) {
  const absolute = path.resolve(repositoryRoot, input);
  const relative = path.relative(repositoryRoot, absolute);
  if (
    relative === ".." ||
    relative.startsWith(`..${path.sep}`) ||
    path.isAbsolute(relative) ||
    /[\0\r\n]/.test(input)
  ) throw new Error(`${description} escapes the repository`);
  return { absolute, relative: relative || "." };
}

function verifiedRepositoryDirectory(candidate, description) {
  let current = repositoryRoot;
  for (const component of candidate.relative.split(path.sep)) {
    if (component === ".") continue;
    current = path.join(current, component);
    const stats = fs.lstatSync(current);
    if (stats.isSymbolicLink() || !stats.isDirectory()) {
      throw new Error(`${description} must not traverse symbolic links or non-directories`);
    }
    const real = fs.realpathSync.native(current);
    const relative = path.relative(repositoryRoot, real);
    if (
      relative === ".." ||
      relative.startsWith(`..${path.sep}`) ||
      path.isAbsolute(relative)
    ) throw new Error(`${description} resolves outside the repository`);
  }
}

const installRoots = new Map();
const scopePath = repositoryPath(scope, "dependency scope");
installRoots.set(scopePath.absolute, scopePath.relative);
for (const manifest of memberManifests) {
  const manifestPath = repositoryPath(manifest, "workspace manifest");
  const stats = fs.lstatSync(manifestPath.absolute);
  if (stats.isSymbolicLink() || !stats.isFile()) {
    throw new Error(`workspace manifest is not a real file: ${manifest}`);
  }
  const memberDirectory = path.dirname(manifestPath.absolute);
  installRoots.set(memberDirectory, path.relative(repositoryRoot, memberDirectory) || ".");
}

let customVirtualStore = null;
[% if web_package_manager == "pnpm" %]
if (effectiveConfig) {
  const contract = JSON.parse(Buffer.from(effectiveConfig, "base64url").toString("utf8"));
  if (contract?.virtualStoreDir !== null && typeof contract?.virtualStoreDir !== "string") {
    throw new Error("invalid pnpm virtual-store contract");
  }
  if (contract.virtualStoreDir) {
    customVirtualStore = repositoryPath(contract.virtualStoreDir, "pnpm virtual store");
  }
}
[% endif %]

function ignored(relative, stats, allowReceiptIgnore, allowRuntimeCacheIgnore) {
  const rootEntry = relative.split(path.sep, 1)[0];
  // Ignore only an exact, real cache root. A nested same-named entry or a
  // file/symlink replacing the expected directory remains part of the proof.
  if (
    allowRuntimeCacheIgnore &&
    relative === rootEntry &&
    runtimeCacheDirectories.has(rootEntry) &&
    stats.isDirectory()
  ) return true;
  if (allowRuntimeCacheIgnore && relative === ".DS_Store" && stats.isFile()) return true;
[% if web_package_manager == "pnpm" %]
  if (allowReceiptIgnore && volatilePnpmWorkspaceStates.has(relative) && stats.isFile()) return true;
[% endif %]
  return allowReceiptIgnore &&
    relative === rootEntry &&
    stats.isFile() &&
    (rootEntry === receipt || rootEntry.startsWith(`${receipt}.tmp.`));
}

function readDirectoryEntries(directory) {
  return fs.readdirSync(directory).sort().map((entry) => {
    const absolute = path.join(directory, entry);
    return { entry, absolute, stats: fs.lstatSync(absolute) };
  });
}

function walk(
  directory,
  treeLabel,
  relativeDirectory = "",
  allowReceiptIgnore = false,
  allowRuntimeCacheIgnore = false,
  prefetchedEntries = null,
) {
  const entries = prefetchedEntries ?? readDirectoryEntries(directory);
  for (const { entry, absolute, stats } of entries) {
    const relative = path.join(relativeDirectory, entry);
    const portable = relative.split(path.sep).join("/");
    if (ignored(relative, stats, allowReceiptIgnore, allowRuntimeCacheIgnore)) continue;
    if (stats.isSymbolicLink()) {
      record("link", treeLabel, portable, fs.readlinkSync(absolute));
      meaningfulEntries += 1;
    } else if (stats.isDirectory()) {
      record("directory", treeLabel, portable);
      walk(absolute, treeLabel, relative, allowReceiptIgnore, allowRuntimeCacheIgnore);
    } else if (stats.isFile()) {
      record("file", treeLabel, portable, stats.size);
      if (entry === "package.json" || entry === ".package-lock.json" || entry === ".modules.yaml") {
        record("metadata", treeLabel, portable, fs.readFileSync(absolute));
      }
      const segments = relative.split(path.sep);
      if (segments.includes(".bin")) {
        record("launcher", treeLabel, portable, stats.mode & 0o7777, fs.readFileSync(absolute));
      }
      meaningfulEntries += 1;
    } else {
      throw new Error(`unsupported dependency artifact type: ${treeLabel}/${portable}`);
    }
  }
}

const orderedRoots = [...installRoots.entries()].sort((left, right) => left[1].localeCompare(right[1]));
for (const [installRoot, label] of orderedRoots) {
  const nodeModules = path.join(installRoot, "node_modules");
  let stats;
  try {
    stats = fs.lstatSync(nodeModules);
  } catch (error) {
    if (error?.code !== "ENOENT") throw error;
    record("node-modules-absent", label);
    continue;
  }
  if (stats.isSymbolicLink() || !stats.isDirectory()) {
    throw new Error(`node_modules must be a real directory: ${label}`);
  }
  const entries = readDirectoryEntries(nodeModules);
  const allowReceiptIgnore = label === scopePath.relative;
  const hasAttestedTopLevelEntry = entries.some(({ entry, stats: entryStats }) =>
    !ignored(entry, entryStats, allowReceiptIgnore, true)
  );
  record(hasAttestedTopLevelEntry ? "node-modules-present" : "node-modules-absent", label);
  walk(nodeModules, `${label}/node_modules`, "", allowReceiptIgnore, true, entries);
}

if (customVirtualStore) {
  const covered = orderedRoots.some(([installRoot]) => {
    const nodeModules = path.join(installRoot, "node_modules");
    const relative = path.relative(nodeModules, customVirtualStore.absolute);
    return relative === "" || (!relative.startsWith(`..${path.sep}`) && relative !== ".." && !path.isAbsolute(relative));
  });
  if (!covered) {
    verifiedRepositoryDirectory(customVirtualStore, "pnpm virtual store");
    record("virtual-store-present", customVirtualStore.relative);
    walk(customVirtualStore.absolute, `virtual-store/${customVirtualStore.relative}`);
  }
}

if (meaningfulEntries === 0) process.exit(1);
process.stdout.write(`${hash.digest("hex")}\n`);
NODE
}

[% if web_package_manager == "yarn" %]
yarn_classic_config_payload() {
  local scope="$1"

  "$node_bin" - --jig-yarn-classic-config "$scope" <<'NODE'
const { spawnSync } = require("node:child_process");
const path = require("node:path");

// BEGIN JIG PACKAGE MANAGER METADATA LAUNCHER
const JIG_PACKAGE_MANAGER_METADATA_ARGUMENTS = [
  ["--version"],
  ["cache", "dir", "--silent"],
  ["config", "--json"],
  ["config", "list", "--json"],
  [
    "pkg",
    "get",
    "name",
    "dependencies",
    "devDependencies",
    "optionalDependencies",
    "peerDependencies",
    "pnpm",
    "--json",
  ],
];

function packageManagerMetadataEnvironmentValue(environment, name) {
  const key = Object.keys(environment).find((candidate) => candidate.toUpperCase() === name);
  return key ? environment[key] : undefined;
}

function validateWindowsCommandInterpreter(interpreter) {
  const driveAbsolute =
    typeof interpreter === "string" && /^[A-Za-z]:[\\/]/.test(interpreter);
  const uncAbsolute =
    typeof interpreter === "string" &&
    /^[\\/]{2}[^\\/]+[\\/][^\\/]+(?:[\\/].*)?$/.test(interpreter);
  if (
    !interpreter ||
    /[\0-\x1f\x7f"]/.test(interpreter) ||
    /^[\\/]{2}[?.][\\/]/.test(interpreter) ||
    (!driveAbsolute && !uncAbsolute)
  ) {
    throw new Error("unsafe Windows command interpreter");
  }
  return interpreter;
}

function validatePackageManagerMetadataExecutable(executable) {
  if (
    typeof executable !== "string" ||
    executable.length === 0 ||
    /[\0-\x1f\x7f"]/.test(executable)
  ) {
    throw new Error("unsafe package-manager metadata executable");
  }
  return executable;
}

function windowsPackageManagerMetadataPathEntries(value) {
  const entries = [];
  let entry = "";
  let quoted = false;
  for (const character of value) {
    if (character === '"') {
      quoted = !quoted;
    } else if (character === path.delimiter && !quoted) {
      entries.push(entry);
      entry = "";
    } else {
      entry += character;
    }
  }
  if (quoted) throw new Error("unsafe quoted Windows PATH");
  entries.push(entry);
  return entries;
}

function windowsPackageManagerMetadataExtensions(requested, configured) {
  if (path.extname(requested)) return [""];
  const raw = configured === undefined ? ".COM;.EXE;.BAT;.CMD" : configured;
  const extensions = raw.split(";").filter((extension) => extension !== "");
  if (
    extensions.length === 0 ||
    extensions.some((extension) => !/^\.[A-Za-z0-9]+$/.test(extension))
  ) throw new Error("unsafe Windows PATHEXT");
  return [...new Map(extensions.map((extension) => [extension.toUpperCase(), extension])).values()];
}

function resolveWindowsPackageManagerMetadataExecutable(executable, options = {}) {
  const fs = require("node:fs");
  const path = require("node:path");
  const requested = validatePackageManagerMetadataExecutable(executable);
  const environment = options.env || process.env;
  const workingDirectory = typeof options.cwd === "string"
    ? path.resolve(options.cwd)
    : process.cwd();
  let candidates = [];
  const searchedPath = !path.isAbsolute(requested);

  if (path.isAbsolute(requested)) {
    candidates = [requested];
  } else {
    if (/[\\/]/.test(requested)) {
      throw new Error("package-manager metadata executable must be absolute or a bare command");
    }
    const pathValue = packageManagerMetadataEnvironmentValue(environment, "PATH") || "";
    const configuredExtensions = packageManagerMetadataEnvironmentValue(environment, "PATHEXT");
    const extensions = windowsPackageManagerMetadataExtensions(requested, configuredExtensions);
    for (const entry of windowsPackageManagerMetadataPathEntries(pathValue)) {
      const directory = entry
        ? (path.isAbsolute(entry) ? entry : path.resolve(workingDirectory, entry))
        : workingDirectory;
      for (const extension of extensions) {
        candidates.push(path.resolve(directory, `${requested}${extension}`));
      }
    }
  }

  for (const candidate of candidates) {
    try {
      if (!fs.statSync(candidate).isFile()) continue;
      const resolved = fs.realpathSync.native(candidate);
      if (!fs.statSync(resolved).isFile()) continue;
      return validatePackageManagerMetadataExecutable(resolved);
    } catch (error) {
      if (!searchedPath) throw error;
    }
  }
  throw new Error("package-manager metadata executable was not found");
}

function encodePackageManagerMetadataBatchArgument(argument, forceQuote = false) {
  if (/[\0\r\n]/.test(argument)) {
    throw new Error("unsafe package-manager metadata batch argument");
  }
  const safeUnquoted = "#$*+-./:?@\\_";
  const quote = forceQuote || argument.length === 0 || argument.endsWith("\\") ||
    [...argument].some((character) => {
      const code = character.codePointAt(0);
      return code < 32 || code === 127 ||
        (code < 128 && !/[A-Za-z0-9]/.test(character) && !safeUnquoted.includes(character));
    });
  let encoded = quote ? '"' : "";
  let backslashes = 0;
  for (const character of argument) {
    if (character === "\\") {
      backslashes += 1;
      encoded += character;
      continue;
    }
    if (character === '"') {
      encoded += "\\".repeat(backslashes);
      encoded += '"';
    } else if (character === "%") {
      encoded += "%%cd:~,";
    }
    backslashes = 0;
    encoded += character;
  }
  if (quote) encoded += "\\".repeat(backslashes) + '"';
  return encoded;
}

function encodePackageManagerMetadataBatchInvocation(executable, args) {
  if (/^[\\/]{2}[?.][\\/]/.test(executable) || executable.endsWith("\\")) {
    throw new Error("unsafe package-manager metadata batch executable");
  }
  let commandLine = '"' + encodePackageManagerMetadataBatchArgument(executable, true);
  for (const argument of args) {
    commandLine += ` ${encodePackageManagerMetadataBatchArgument(argument)}`;
  }
  return commandLine + '"';
}

function spawnPackageManagerMetadata(executable, args, options) {
  const allowedArguments =
    Array.isArray(args) &&
    JIG_PACKAGE_MANAGER_METADATA_ARGUMENTS.some(
      (allowed) =>
        allowed.length === args.length &&
        allowed.every((value, index) => value === args[index])
    );
  if (!allowedArguments || args.some((value) => !/^[A-Za-z0-9._-]+$/.test(value))) {
    throw new Error("unsupported package-manager metadata arguments");
  }

  const spawnOptions = { timeout: 30_000, ...options, shell: false };
  if (process.platform !== "win32") {
    return spawnSync(
      validatePackageManagerMetadataExecutable(executable),
      args,
      spawnOptions
    );
  }

  const resolved = resolveWindowsPackageManagerMetadataExecutable(executable, spawnOptions);
  if (!/\.(?:cmd|bat)$/i.test(resolved)) {
    return spawnSync(resolved, args, spawnOptions);
  }

  const environment = spawnOptions.env || process.env;
  const commandInterpreter = validateWindowsCommandInterpreter(
    packageManagerMetadataEnvironmentValue(environment, "COMSPEC")
  );
  const commandLine = encodePackageManagerMetadataBatchInvocation(resolved, args);
  return spawnSync(
    commandInterpreter,
    ["/d", "/s", "/v:off", "/c", commandLine],
    { ...spawnOptions, windowsVerbatimArguments: true }
  );
}
// END JIG PACKAGE MANAGER METADATA LAUNCHER

const scope = path.resolve(process.argv[3]);
function run(args, label) {
  const result = spawnPackageManagerMetadata("yarn", args, {
    cwd: scope,
    encoding: "utf8",
    env: process.env,
    maxBuffer: 10 * 1024 * 1024,
  });
  if (result.error || result.signal || result.status !== 0) {
    throw new Error(`could not resolve Yarn Classic ${label}`);
  }
  return result.stdout;
}

try {
  const version = run(["--version"], "version").trim();
  if (!/^[0-9A-Za-z.+-]+$/.test(version)) throw new Error("invalid Yarn Classic version");

  const cacheLines = run(["cache", "dir", "--silent"], "cache directory")
    .split(/\r?\n/)
    .filter(Boolean);
  if (cacheLines.length !== 1 || !path.isAbsolute(cacheLines[0]) || /[\0\r\n]/.test(cacheLines[0])) {
    throw new Error("invalid Yarn Classic cache directory");
  }

  let yarnConfig;
  let section = "";
  for (const line of run(["config", "list", "--json"], "configuration").split(/\r?\n/)) {
    if (!line) continue;
    const entry = JSON.parse(line);
    if (entry.type === "info") {
      section = entry.data === "yarn config" ? "yarn" : "";
    } else if (section === "yarn" && entry.type === "inspect" && entry.data && typeof entry.data === "object") {
      if (yarnConfig) throw new Error("ambiguous Yarn Classic configuration");
      yarnConfig = entry.data;
    }
  }
  if (!yarnConfig) throw new Error("missing Yarn Classic configuration");

  const relevantKeys = [
    "plugnplay-override",
    "--pnp",
    "--enable-pnp",
    "--disable-pnp",
    "--install.pnp",
    "--install.enable-pnp",
    "--install.disable-pnp",
    "pnp",
    "enable-pnp",
    "disable-pnp",
    "install.pnp",
    "install.enable-pnp",
    "install.disable-pnp",
    "cache-folder",
    "preferred-cache-folder",
    "modules-folder",
    "yarn-path",
    "rc-filename",
    "ignore-path",
  ];
  const selected = Object.create(null);
  for (const key of relevantKeys) {
    if (Object.hasOwn(yarnConfig, key)) selected[key] = yarnConfig[key];
  }

  const relevantEnvironment = Object.create(null);
  for (const key of [
    "YARN_PLUGNPLAY_OVERRIDE",
    "YARN_PNP",
    "YARN_ENABLE_PNP",
    "YARN_DISABLE_PNP",
    "YARN_INSTALL_PNP",
    "YARN_INSTALL_ENABLE_PNP",
    "YARN_INSTALL_DISABLE_PNP",
    "YARN_CACHE_FOLDER",
    "YARN_PREFERRED_CACHE_FOLDER",
    "YARN_RC_FILENAME",
    "YARN_YARN_PATH",
    "YARN_IGNORE_PATH",
  ]) {
    if (Object.hasOwn(process.env, key)) relevantEnvironment[key] = process.env[key];
  }

  const normalized = {
    version,
    platform: process.platform,
    arch: process.arch,
    scope,
    cacheDirectory: path.normalize(cacheLines[0]),
    config: selected,
    environment: relevantEnvironment,
  };
  process.stdout.write(`classic:${Buffer.from(JSON.stringify(normalized)).toString("base64")}\n`);
} catch {
  process.exit(1);
}
NODE
}

yarn_classic_actual_artifact_kind() {
  local scope="$1"
  local has_node_modules="false" has_pnp="false"

  if [ -e "$scope/node_modules" ]; then
    path_is_real_directory "$scope/node_modules" || return 1
    has_node_modules="true"
  fi
  if [ -e "$scope/.pnp.js" ]; then
    path_is_nonempty_real_file "$scope/.pnp.js" || return 1
    has_pnp="true"
  fi
  [ ! -e "$scope/.pnp.cjs" ] || return 1

  if [ "$has_node_modules" = "true" ] && [ "$has_pnp" = "true" ]; then
    return 1
  elif [ "$has_pnp" = "true" ]; then
    printf '%s\n' "pnp-js"
  elif [ "$has_node_modules" = "true" ]; then
    printf '%s\n' "node-modules"
  elif dependency_scope_is_dependency_free "$scope"; then
    printf '%s\n' "empty"
  else
    return 1
  fi
}

yarn_berry_config_payload() {
  local scope="$1"

  "$node_bin" - --jig-yarn-berry-config "$scope" <<'NODE'
const { spawnSync } = require("node:child_process");
const path = require("node:path");

// BEGIN JIG PACKAGE MANAGER METADATA LAUNCHER
const JIG_PACKAGE_MANAGER_METADATA_ARGUMENTS = [
  ["--version"],
  ["cache", "dir", "--silent"],
  ["config", "--json"],
  ["config", "list", "--json"],
  [
    "pkg",
    "get",
    "name",
    "dependencies",
    "devDependencies",
    "optionalDependencies",
    "peerDependencies",
    "pnpm",
    "--json",
  ],
];

function packageManagerMetadataEnvironmentValue(environment, name) {
  const key = Object.keys(environment).find((candidate) => candidate.toUpperCase() === name);
  return key ? environment[key] : undefined;
}

function validateWindowsCommandInterpreter(interpreter) {
  const driveAbsolute =
    typeof interpreter === "string" && /^[A-Za-z]:[\\/]/.test(interpreter);
  const uncAbsolute =
    typeof interpreter === "string" &&
    /^[\\/]{2}[^\\/]+[\\/][^\\/]+(?:[\\/].*)?$/.test(interpreter);
  if (
    !interpreter ||
    /[\0-\x1f\x7f"]/.test(interpreter) ||
    /^[\\/]{2}[?.][\\/]/.test(interpreter) ||
    (!driveAbsolute && !uncAbsolute)
  ) {
    throw new Error("unsafe Windows command interpreter");
  }
  return interpreter;
}

function validatePackageManagerMetadataExecutable(executable) {
  if (
    typeof executable !== "string" ||
    executable.length === 0 ||
    /[\0-\x1f\x7f"]/.test(executable)
  ) {
    throw new Error("unsafe package-manager metadata executable");
  }
  return executable;
}

function windowsPackageManagerMetadataPathEntries(value) {
  const entries = [];
  let entry = "";
  let quoted = false;
  for (const character of value) {
    if (character === '"') {
      quoted = !quoted;
    } else if (character === path.delimiter && !quoted) {
      entries.push(entry);
      entry = "";
    } else {
      entry += character;
    }
  }
  if (quoted) throw new Error("unsafe quoted Windows PATH");
  entries.push(entry);
  return entries;
}

function windowsPackageManagerMetadataExtensions(requested, configured) {
  if (path.extname(requested)) return [""];
  const raw = configured === undefined ? ".COM;.EXE;.BAT;.CMD" : configured;
  const extensions = raw.split(";").filter((extension) => extension !== "");
  if (
    extensions.length === 0 ||
    extensions.some((extension) => !/^\.[A-Za-z0-9]+$/.test(extension))
  ) throw new Error("unsafe Windows PATHEXT");
  return [...new Map(extensions.map((extension) => [extension.toUpperCase(), extension])).values()];
}

function resolveWindowsPackageManagerMetadataExecutable(executable, options = {}) {
  const fs = require("node:fs");
  const path = require("node:path");
  const requested = validatePackageManagerMetadataExecutable(executable);
  const environment = options.env || process.env;
  const workingDirectory = typeof options.cwd === "string"
    ? path.resolve(options.cwd)
    : process.cwd();
  let candidates = [];
  const searchedPath = !path.isAbsolute(requested);

  if (path.isAbsolute(requested)) {
    candidates = [requested];
  } else {
    if (/[\\/]/.test(requested)) {
      throw new Error("package-manager metadata executable must be absolute or a bare command");
    }
    const pathValue = packageManagerMetadataEnvironmentValue(environment, "PATH") || "";
    const configuredExtensions = packageManagerMetadataEnvironmentValue(environment, "PATHEXT");
    const extensions = windowsPackageManagerMetadataExtensions(requested, configuredExtensions);
    for (const entry of windowsPackageManagerMetadataPathEntries(pathValue)) {
      const directory = entry
        ? (path.isAbsolute(entry) ? entry : path.resolve(workingDirectory, entry))
        : workingDirectory;
      for (const extension of extensions) {
        candidates.push(path.resolve(directory, `${requested}${extension}`));
      }
    }
  }

  for (const candidate of candidates) {
    try {
      if (!fs.statSync(candidate).isFile()) continue;
      const resolved = fs.realpathSync.native(candidate);
      if (!fs.statSync(resolved).isFile()) continue;
      return validatePackageManagerMetadataExecutable(resolved);
    } catch (error) {
      if (!searchedPath) throw error;
    }
  }
  throw new Error("package-manager metadata executable was not found");
}

function encodePackageManagerMetadataBatchArgument(argument, forceQuote = false) {
  if (/[\0\r\n]/.test(argument)) {
    throw new Error("unsafe package-manager metadata batch argument");
  }
  const safeUnquoted = "#$*+-./:?@\\_";
  const quote = forceQuote || argument.length === 0 || argument.endsWith("\\") ||
    [...argument].some((character) => {
      const code = character.codePointAt(0);
      return code < 32 || code === 127 ||
        (code < 128 && !/[A-Za-z0-9]/.test(character) && !safeUnquoted.includes(character));
    });
  let encoded = quote ? '"' : "";
  let backslashes = 0;
  for (const character of argument) {
    if (character === "\\") {
      backslashes += 1;
      encoded += character;
      continue;
    }
    if (character === '"') {
      encoded += "\\".repeat(backslashes);
      encoded += '"';
    } else if (character === "%") {
      encoded += "%%cd:~,";
    }
    backslashes = 0;
    encoded += character;
  }
  if (quote) encoded += "\\".repeat(backslashes) + '"';
  return encoded;
}

function encodePackageManagerMetadataBatchInvocation(executable, args) {
  if (/^[\\/]{2}[?.][\\/]/.test(executable) || executable.endsWith("\\")) {
    throw new Error("unsafe package-manager metadata batch executable");
  }
  let commandLine = '"' + encodePackageManagerMetadataBatchArgument(executable, true);
  for (const argument of args) {
    commandLine += ` ${encodePackageManagerMetadataBatchArgument(argument)}`;
  }
  return commandLine + '"';
}

function spawnPackageManagerMetadata(executable, args, options) {
  const allowedArguments =
    Array.isArray(args) &&
    JIG_PACKAGE_MANAGER_METADATA_ARGUMENTS.some(
      (allowed) =>
        allowed.length === args.length &&
        allowed.every((value, index) => value === args[index])
    );
  if (!allowedArguments || args.some((value) => !/^[A-Za-z0-9._-]+$/.test(value))) {
    throw new Error("unsupported package-manager metadata arguments");
  }

  const spawnOptions = { timeout: 30_000, ...options, shell: false };
  if (process.platform !== "win32") {
    return spawnSync(
      validatePackageManagerMetadataExecutable(executable),
      args,
      spawnOptions
    );
  }

  const resolved = resolveWindowsPackageManagerMetadataExecutable(executable, spawnOptions);
  if (!/\.(?:cmd|bat)$/i.test(resolved)) {
    return spawnSync(resolved, args, spawnOptions);
  }

  const environment = spawnOptions.env || process.env;
  const commandInterpreter = validateWindowsCommandInterpreter(
    packageManagerMetadataEnvironmentValue(environment, "COMSPEC")
  );
  const commandLine = encodePackageManagerMetadataBatchInvocation(resolved, args);
  return spawnSync(
    commandInterpreter,
    ["/d", "/s", "/v:off", "/c", commandLine],
    { ...spawnOptions, windowsVerbatimArguments: true }
  );
}
// END JIG PACKAGE MANAGER METADATA LAUNCHER

const scope = path.resolve(process.argv[3]);
const result = spawnPackageManagerMetadata("yarn", ["config", "--json"], {
  cwd: scope,
  encoding: "utf8",
  env: process.env,
  maxBuffer: 10 * 1024 * 1024,
});
if (result.error || result.signal || result.status !== 0) {
  console.error("Could not resolve the effective Yarn configuration for the dependency scope.");
  process.exit(1);
}

const keys = [
  "nodeLinker",
  "cacheFolder",
  "installStatePath",
  "pnpUnpluggedFolder",
  "pnpEnableInlining",
  "pnpEnableEsmLoader",
];
const selected = Object.create(null);
for (const line of result.stdout.split(/\r?\n/)) {
  if (!line) continue;
  let entry;
  try {
    entry = JSON.parse(line);
  } catch {
    console.error("Yarn returned malformed JSON while resolving dependency configuration.");
    process.exit(1);
  }
  if (keys.includes(entry.key) && Object.hasOwn(entry, "effective")) {
    selected[entry.key] = entry.effective;
  }
}
if (!Object.hasOwn(selected, "pnpEnableEsmLoader")) {
  selected.pnpEnableEsmLoader = false;
}
for (const key of keys) {
  if (!Object.hasOwn(selected, key)) {
    console.error(`Yarn omitted required effective configuration key ${key}.`);
    process.exit(1);
  }
}
if (!["pnp", "node-modules", "pnpm"].includes(selected.nodeLinker)) process.exit(1);
for (const key of ["cacheFolder", "installStatePath", "pnpUnpluggedFolder"]) {
  const value = selected[key];
  if (typeof value !== "string" || !path.isAbsolute(value) || /[\0\r\n]/.test(value)) process.exit(1);
  selected[key] = path.normalize(value);
}
for (const key of ["pnpEnableInlining", "pnpEnableEsmLoader"]) {
  if (typeof selected[key] !== "boolean") process.exit(1);
}
const normalized = Object.fromEntries(keys.map((key) => [key, selected[key]]));
process.stdout.write(`${Buffer.from(JSON.stringify(normalized)).toString("base64")}\n`);
NODE
}

yarn_berry_config_value() {
  local payload="$1"
  local key="$2"

  "$node_bin" - --jig-yarn-config-value "$payload" "$key" <<'NODE'
const payload = process.argv[3];
const key = process.argv[4];
let config;
try {
  config = JSON.parse(Buffer.from(payload, "base64").toString("utf8"));
} catch {
  process.exit(1);
}
const value = config[key];
if (typeof value === "boolean") process.stdout.write(`${value}\n`);
else if (typeof value === "string" && !/[\0\r\n]/.test(value)) process.stdout.write(`${value}\n`);
else process.exit(1);
NODE
}

dependency_yarn_config() {
  local scope="$1"
  local lockfile="$2"
  local lockfile_kind

  validate_yarn_scope_authorities "$scope" || return
  lockfile_kind="$(yarn_lockfile_kind "$lockfile")" || return
  if [ "$lockfile_kind" = "classic" ]; then
    yarn_classic_config_payload "$scope"
  else
    yarn_berry_config_payload "$scope"
  fi
}

dependency_artifact_kind() {
  local scope="$1"
  local lockfile="$2"
  local config_payload="${3:-}"
  local linker lockfile_kind

  lockfile_kind="$(yarn_lockfile_kind "$lockfile")" || return
  if [ "$lockfile_kind" = "classic" ]; then
    yarn_classic_actual_artifact_kind "$scope"
    return
  fi

  [ -n "$config_payload" ] || return 1
  linker="$(yarn_berry_config_value "$config_payload" "nodeLinker")" || return 1
  case "$linker" in
    pnp)
      if path_is_nonempty_real_file "$scope/.pnp.cjs"; then
        printf '%s\n' "pnp-cjs"
      elif path_is_nonempty_real_file "$scope/.pnp.js"; then
        printf '%s\n' "pnp-js"
      else
        printf '%s\n' "pnp-cjs"
      fi
      ;;
    node-modules|pnpm) printf '%s\n' "node-modules" ;;
    *)
      echo "Unsupported or dynamic Yarn nodeLinker '$linker' in dependency scope '$scope'." >&2
      return 1
      ;;
  esac
}
[% else %]
dependency_yarn_config() {
  return 0
}

dependency_artifact_kind() {
  printf '%s\n' "node-modules"
}
[% endif %]

[% if web_package_manager == "yarn" %]
yarn_classic_pnp_artifact_proof() {
  local scope="$1"
  local loader="$2"
  local config_payload="$3"

  "$node_bin" - --jig-yarn-classic-pnp-proof "$scope" "$loader" "$config_payload" <<'NODE'
const { createHash } = require("node:crypto");
const fs = require("node:fs");
const path = require("node:path");

const scope = path.resolve(process.argv[3]);
const loader = path.resolve(process.argv[4]);
let config;
try {
  const payload = process.argv[5];
  if (!payload.startsWith("classic:")) throw new Error("invalid Classic config payload");
  config = JSON.parse(Buffer.from(payload.slice("classic:".length), "base64").toString("utf8"));
  if (typeof config.cacheDirectory !== "string" || !path.isAbsolute(config.cacheDirectory)) {
    throw new Error("invalid Classic cache directory");
  }
} catch {
  process.exit(1);
}
const maximumEntries = 100_000;
let entries = 0;
const hash = createHash("sha256");
hash.update("jig-yarn-classic-pnp-artifacts-v1\0");

function realFile(file, label) {
  const stats = fs.lstatSync(file);
  if (!stats.isFile() || stats.isSymbolicLink() || stats.size === 0) throw new Error(`invalid ${label}`);
  if (label === "loader" && stats.size > 64 * 1024 * 1024) throw new Error("PnP loader is too large");
}

function jsonStringAt(source, start) {
  if (source[start] !== '"') throw new Error("expected JSON string literal");
  let cursor = start + 1;
  while (cursor < source.length) {
    const character = source[cursor];
    if (character === '"') {
      const literal = source.slice(start, cursor + 1);
      return { value: JSON.parse(literal), end: cursor };
    }
    if (character === "\\") {
      cursor += 1;
      if (cursor >= source.length) throw new Error("unterminated JSON string literal");
      const escape = source[cursor];
      if (escape === "u") {
        const digits = source.slice(cursor + 1, cursor + 5);
        if (!/^[0-9a-fA-F]{4}$/.test(digits)) throw new Error("invalid JSON unicode escape");
        cursor += 4;
      } else if (!/["\\/bfnrt]/.test(escape)) {
        throw new Error("invalid JSON string escape");
      }
    } else if (character.charCodeAt(0) <= 0x1f) {
      throw new Error("invalid control character in JSON string");
    }
    cursor += 1;
  }
  throw new Error("unterminated JSON string literal");
}

function packageLocations(source) {
  const startMarker = /(?:^|\n)let packageInformationStores\s*=\s*new Map\s*\(\s*\[\s*/g;
  const startMatches = [...source.matchAll(startMarker)];
  if (startMatches.length !== 1) throw new Error("ambiguous package information table");
  const start = startMatches[0].index + startMatches[0][0].length;
  const tail = source.slice(start);
  const endMarker = /\n\s*\]\s*\)\s*;\s*\n\s*let locatorsByLocations\s*=\s*new Map\s*\(\s*\[/g;
  const endMatches = [...tail.matchAll(endMarker)];
  if (endMatches.length !== 1) throw new Error("ambiguous package information table bound");
  const table = tail.slice(0, endMatches[0].index);
  const locations = [];

  for (let cursor = 0; cursor < table.length; cursor += 1) {
    if (table[cursor] === '"') {
      cursor = jsonStringAt(table, cursor).end;
      continue;
    }
    if (table[cursor] === "'" || table[cursor] === "`") {
      throw new Error("unexpected JavaScript string in package information table");
    }
    if (!table.startsWith("packageLocation", cursor)) continue;
    const before = table[cursor - 1] || "";
    const after = table[cursor + "packageLocation".length] || "";
    if (/[A-Za-z0-9_$]/.test(before) || /[A-Za-z0-9_$]/.test(after)) continue;
    cursor += "packageLocation".length;
    const prefix = /^\s*:\s*path\.resolve\s*\(\s*__dirname\s*,\s*/.exec(table.slice(cursor));
    if (!prefix) throw new Error("invalid packageLocation expression");
    cursor += prefix[0].length;
    const literal = jsonStringAt(table, cursor);
    cursor = literal.end + 1;
    const suffix = /^\s*\)\s*,/.exec(table.slice(cursor));
    if (!suffix) throw new Error("invalid packageLocation bound");
    locations.push(literal.value);
    if (locations.length > maximumEntries) throw new Error("too many Classic PnP package locations");
    cursor += suffix[0].length - 1;
  }
  if (locations.length === 0) throw new Error("package information table has no locations");
  return locations;
}

function hashFile(file, label) {
  realFile(file, label);
  hash.update(`file\0${label}\0`);
  hash.update(fs.readFileSync(file));
  hash.update("\0");
  entries += 1;
  if (entries > maximumEntries) throw new Error("too many Classic PnP artifact entries");
}

function hashDirectory(directory, label, relative = "") {
  const stats = fs.lstatSync(directory);
  if (!stats.isDirectory() || stats.isSymbolicLink()) throw new Error(`invalid ${label}`);
  if (relative === "") hash.update(`directory-root\0${label}\0${directory}\0`);
  for (const entry of fs.readdirSync(directory).sort()) {
    const absolute = path.join(directory, entry);
    const childRelative = relative ? `${relative}/${entry}` : entry;
    const childStats = fs.lstatSync(absolute);
    if (childStats.isSymbolicLink()) throw new Error(`symlink in ${label}`);
    if (childStats.isDirectory()) {
      hash.update(`directory\0${label}/${childRelative}\0`);
      hashDirectory(absolute, label, childRelative);
    } else if (childStats.isFile()) {
      hash.update(`file\0${label}/${childRelative}\0`);
      hash.update(fs.readFileSync(absolute));
      hash.update("\0");
    } else {
      throw new Error(`unsupported entry in ${label}`);
    }
    entries += 1;
    if (entries > maximumEntries) throw new Error("too many Classic PnP artifact entries");
  }
}

function within(parent, child) {
  const relative = path.relative(parent, child);
  return relative !== "" && relative !== ".." && !relative.startsWith(`..${path.sep}`) && !path.isAbsolute(relative);
}

try {
  hashFile(loader, "loader");
  const recursiveDirectories = new Set();
  const localWorkspaces = new Set();
  const source = fs.readFileSync(loader, "utf8");
  const cacheDirectory = path.normalize(config.cacheDirectory);
  const unpluggedDirectory = path.join(scope, ".pnp", "unplugged");
  for (const location of packageLocations(source)) {
    const directory = path.resolve(scope, location);
    if (directory === scope) continue;
    const stats = fs.lstatSync(directory);
    if (!stats.isDirectory() || stats.isSymbolicLink()) throw new Error("invalid referenced package directory");
    if (within(cacheDirectory, directory) || within(unpluggedDirectory, directory) || !within(scope, directory)) {
      recursiveDirectories.add(directory);
    } else {
      const manifest = path.join(directory, "package.json");
      realFile(manifest, "local workspace manifest");
      localWorkspaces.add(directory);
    }
  }
  for (const directory of [...localWorkspaces].sort()) {
    const relative = path.relative(scope, directory).split(path.sep).join("/");
    hash.update(`local-workspace\0${relative}\0`);
    hashFile(path.join(directory, "package.json"), `local-workspace/${relative}/package.json`);
  }
  for (const [index, directory] of [...recursiveDirectories].sort().entries()) {
    hashDirectory(directory, `package/${index}`);
  }
  process.stdout.write(`${hash.digest("hex")}\n`);
} catch {
  process.exit(1);
}
NODE
}

yarn_berry_pnp_artifact_proof() {
  local scope="$1"
  local loader="$2"
  local config_payload="$3"

  "$node_bin" - --jig-yarn-pnp-proof "$scope" "$loader" "$config_payload" <<'NODE'
const { createHash } = require("node:crypto");
const fs = require("node:fs");
const path = require("node:path");
const vm = require("node:vm");

const scope = path.resolve(process.argv[3]);
const loader = path.resolve(process.argv[4]);
let config;
try {
  config = JSON.parse(Buffer.from(process.argv[5], "base64").toString("utf8"));
} catch {
  process.exit(1);
}

const hash = createHash("sha256");
hash.update("jig-yarn-pnp-artifacts-v1\0");
let entries = 0;
const maximumEntries = 100_000;
const maximumParsedInputBytes = 64 * 1024 * 1024;

function realFile(file, label, maximumBytes) {
  const stats = fs.lstatSync(file);
  if (!stats.isFile() || stats.isSymbolicLink() || stats.size === 0) throw new Error(`invalid ${label}`);
  if (maximumBytes !== undefined && stats.size > maximumBytes) throw new Error(`${label} is too large`);
}

function optionalRealDirectory(directory, label) {
  if (!fs.existsSync(directory)) return false;
  const stats = fs.lstatSync(directory);
  if (!stats.isDirectory() || stats.isSymbolicLink()) throw new Error(`invalid ${label}`);
  return true;
}

function hashFile(file, label, maximumBytes) {
  realFile(file, label, maximumBytes);
  hash.update(`file\0${label}\0`);
  hash.update(fs.readFileSync(file));
  hash.update("\0");
  entries += 1;
  if (entries > maximumEntries) throw new Error("too many PnP artifact entries");
}

function hashDirectory(directory, label, relative = "") {
  const stats = fs.lstatSync(directory);
  if (!stats.isDirectory() || stats.isSymbolicLink()) throw new Error(`invalid ${label}`);
  if (relative === "") hash.update(`directory-root\0${label}\0`);
  for (const entry of fs.readdirSync(directory).sort()) {
    const absolute = path.join(directory, entry);
    const childRelative = relative ? `${relative}/${entry}` : entry;
    const childStats = fs.lstatSync(absolute);
    if (childStats.isSymbolicLink()) throw new Error(`symlink in ${label}`);
    if (childStats.isDirectory()) {
      hash.update(`directory\0${label}/${childRelative}\0`);
      hashDirectory(absolute, label, childRelative);
    } else if (childStats.isFile()) {
      hash.update(`file\0${label}/${childRelative}\0`);
      hash.update(fs.readFileSync(absolute));
      hash.update("\0");
    } else {
      throw new Error(`unsupported entry in ${label}`);
    }
    entries += 1;
    if (entries > maximumEntries) throw new Error("too many PnP artifact entries");
  }
}

function inlineRuntimeState(source) {
  const assignment = /(?:^|\n)const RAW_RUNTIME_STATE\s*=\s*/g;
  const matches = [...source.matchAll(assignment)];
  if (matches.length !== 1) throw new Error("missing inline PnP state");
  let cursor = matches[0].index + matches[0][0].length;
  if (source[cursor] !== "'") throw new Error("inline PnP state is not a string literal");
  const start = cursor;
  cursor += 1;
  let escaped = false;
  for (; cursor < source.length; cursor += 1) {
    const character = source[cursor];
    if (escaped) {
      escaped = false;
    } else if (character === "\\") {
      escaped = true;
    } else if (character === "'") {
      break;
    }
  }
  if (cursor >= source.length) throw new Error("unterminated inline PnP state");
  const literal = source.slice(start, cursor + 1);
  if (!/^\s*;/.test(source.slice(cursor + 1))) throw new Error("invalid inline PnP assignment");
  const decoded = vm.runInNewContext(literal, Object.create(null), { timeout: 50 });
  if (typeof decoded !== "string") throw new Error("invalid inline PnP state");
  return JSON.parse(decoded);
}

function jsonStringAt(source, start) {
  if (source[start] !== '"') throw new Error("expected JSON string literal");
  let cursor = start + 1;
  while (cursor < source.length) {
    const character = source[cursor];
    if (character === '"') {
      const literal = source.slice(start, cursor + 1);
      return { value: JSON.parse(literal), end: cursor };
    }
    if (character === "\\") {
      cursor += 1;
      if (cursor >= source.length) throw new Error("unterminated JSON string literal");
      const escape = source[cursor];
      if (escape === "u") {
        const digits = source.slice(cursor + 1, cursor + 5);
        if (!/^[0-9a-fA-F]{4}$/.test(digits)) throw new Error("invalid JSON unicode escape");
        cursor += 4;
      } else if (!/["\\/bfnrt]/.test(escape)) {
        throw new Error("invalid JSON string escape");
      }
    } else if (character.charCodeAt(0) <= 0x1f) {
      throw new Error("invalid control character in JSON string");
    }
    cursor += 1;
  }
  throw new Error("unterminated JSON string literal");
}

function objectLiteralEnd(source, start) {
  if (source[start] !== "{") throw new Error("missing inline PnP object");
  let depth = 0;
  for (let cursor = start; cursor < source.length; cursor += 1) {
    const character = source[cursor];
    if (character === '"') {
      cursor = jsonStringAt(source, cursor).end;
    } else if (character === "{") {
      depth += 1;
    } else if (character === "}") {
      depth -= 1;
      if (depth === 0) return cursor;
      if (depth < 0) break;
    } else if (character === "'" || character === "`") {
      throw new Error("unexpected JavaScript literal in inline PnP object");
    }
  }
  throw new Error("unterminated inline PnP object");
}

function objectLiteralPackageLocations(source, start, end) {
  const locations = [];
  for (let cursor = start; cursor <= end; cursor += 1) {
    if (source[cursor] !== '"') continue;
    const key = jsonStringAt(source, cursor);
    cursor = key.end;
    if (key.value !== "packageLocation") continue;
    let separator = cursor + 1;
    while (/\s/.test(source[separator] || "")) separator += 1;
    if (source[separator] !== ":") continue;
    let valueStart = separator + 1;
    while (/\s/.test(source[valueStart] || "")) valueStart += 1;
    if (source[valueStart] !== '"') throw new Error("invalid packageLocation value");
    const value = jsonStringAt(source, valueStart);
    locations.push(value.value);
    if (locations.length > maximumEntries) throw new Error("too many PnP package locations");
    cursor = value.end;
  }
  if (locations.length === 0) throw new Error("inline PnP object has no package locations");
  return locations;
}

function singleQuotedLiteralAt(source, start) {
  if (source[start] !== "'") throw new Error("expected single-quoted JavaScript string literal");
  let cursor = start + 1;
  let escaped = false;
  for (; cursor < source.length; cursor += 1) {
    const character = source[cursor];
    if (escaped) {
      escaped = false;
    } else if (character === "\\") {
      escaped = true;
    } else if (character === "'") {
      const literal = source.slice(start, cursor + 1);
      const decoded = vm.runInNewContext(literal, Object.create(null), { timeout: 50 });
      if (typeof decoded !== "string") throw new Error("invalid setup-state string literal");
      return { value: decoded, end: cursor };
    }
  }
  throw new Error("unterminated setup-state string literal");
}

function setupStatePackageLocations(source) {
  const marker = /(?:^|\n)function \$\$SETUP_STATE\s*\(\s*hydrateRuntimeState\s*,\s*basePath\s*\)\s*\{/g;
  const matches = [...source.matchAll(marker)];
  if (matches.length !== 1) throw new Error("ambiguous setup-state function");
  let cursor = matches[0].index + matches[0][0].length;
  const callPrefix = /^\s*return hydrateRuntimeState\s*\(\s*/.exec(source.slice(cursor));
  if (!callPrefix) throw new Error("invalid setup-state function prefix");
  cursor += callPrefix[0].length;

  let locations;
  if (source[cursor] === "{") {
    const end = objectLiteralEnd(source, cursor);
    locations = objectLiteralPackageLocations(source, cursor, end);
    cursor = end + 1;
  } else {
    const parsePrefix = /^JSON\.parse\s*\(\s*/.exec(source.slice(cursor));
    if (!parsePrefix) throw new Error("unsupported setup-state payload");
    cursor += parsePrefix[0].length;
    const literal = singleQuotedLiteralAt(source, cursor);
    let runtimeState;
    try {
      runtimeState = JSON.parse(literal.value);
    } catch {
      throw new Error("invalid setup-state JSON payload");
    }
    locations = collectPackageLocations(runtimeState);
    if (locations.length === 0) throw new Error("setup-state JSON has no package locations");
    cursor = literal.end + 1;
    const parseSuffix = /^\s*\)/.exec(source.slice(cursor));
    if (!parseSuffix) throw new Error("invalid setup-state JSON.parse bound");
    cursor += parseSuffix[0].length;
  }

  const functionSuffix = /^\s*,\s*\{\s*basePath\s*:\s*basePath\s*\|\|\s*__dirname\s*\}\s*\)\s*;\s*\}/.exec(source.slice(cursor));
  if (!functionSuffix) throw new Error("invalid setup-state function bound");
  return locations;
}

function inlinePackageLocations(source) {
  if (/(?:^|\n)const RAW_RUNTIME_STATE\s*=\s*/.test(source)) {
    return collectPackageLocations(inlineRuntimeState(source));
  }
  return setupStatePackageLocations(source);
}

function collectPackageLocations(root) {
  const locations = [];
  const pending = [root];
  let visited = 0;
  while (pending.length > 0) {
    const value = pending.pop();
    visited += 1;
    if (visited > maximumEntries) throw new Error("PnP state is too large");
    if (Array.isArray(value)) {
      pending.push(...value);
    } else if (value && typeof value === "object") {
      for (const [key, child] of Object.entries(value)) {
        if (key === "packageLocation" && typeof child === "string") {
          locations.push(child);
          if (locations.length > maximumEntries) throw new Error("too many PnP package locations");
        } else {
          pending.push(child);
        }
      }
    }
  }
  return locations;
}

function within(parent, child) {
  const relative = path.relative(parent, child);
  return relative !== "" && relative !== ".." && !relative.startsWith(`..${path.sep}`) && !path.isAbsolute(relative);
}

try {
  hashFile(loader, "loader", maximumParsedInputBytes);
  hashFile(path.normalize(config.installStatePath), "install-state");

  let packageLocations;
  const dataPath = path.join(scope, ".pnp.data.json");
  if (config.pnpEnableInlining === false) {
    hashFile(dataPath, "data", maximumParsedInputBytes);
    packageLocations = collectPackageLocations(JSON.parse(fs.readFileSync(dataPath, "utf8")));
  } else {
    packageLocations = inlinePackageLocations(fs.readFileSync(loader, "utf8"));
  }
  if (config.pnpEnableEsmLoader === true) {
    hashFile(path.join(scope, ".pnp.loader.mjs"), "esm-loader");
  }

  const cacheFolder = path.normalize(config.cacheFolder);
  const unpluggedFolder = path.normalize(config.pnpUnpluggedFolder);
  optionalRealDirectory(cacheFolder, "cache folder");
  optionalRealDirectory(unpluggedFolder, "unplugged folder");
  const cacheArchives = new Set();
  const unpluggedPackages = new Set();
  for (const location of packageLocations) {
    const absoluteLocation = path.resolve(scope, location);
    if (within(cacheFolder, absoluteLocation)) {
      const segments = path.relative(cacheFolder, absoluteLocation).split(path.sep);
      const archiveIndex = segments.findIndex((segment) => segment.endsWith(".zip"));
      if (archiveIndex >= 0) cacheArchives.add(path.join(cacheFolder, ...segments.slice(0, archiveIndex + 1)));
    }
    if (within(unpluggedFolder, absoluteLocation)) {
      const [packageDirectory] = path.relative(unpluggedFolder, absoluteLocation).split(path.sep);
      if (packageDirectory) {
        const directory = path.join(unpluggedFolder, packageDirectory);
        if (optionalRealDirectory(directory, "referenced unplugged package")) {
          unpluggedPackages.add(directory);
        }
      }
    }
  }
  for (const archive of [...cacheArchives].sort()) hashFile(archive, `cache/${path.basename(archive)}`);
  for (const directory of [...unpluggedPackages].sort()) {
    hashDirectory(directory, `unplugged/${path.basename(directory)}`);
  }
  process.stdout.write(`${hash.digest("hex")}\n`);
} catch {
  process.exit(1);
}
NODE
}
[% endif %]

artifact_proof() {
  local scope="$1"
  local kind="$2"
  local input_fingerprint="$3"
  local effective_config="${4:-}"
  local marker receipt_version receipt_input receipt_structure ignored actual_structure

  case "$kind" in
    node-modules)
      path_is_real_directory "$scope/node_modules" || return 1
[% if web_package_manager == "yarn" %]
      [ ! -e "$scope/.pnp.cjs" ] && [ ! -e "$scope/.pnp.js" ] || return 1
[% endif %]
      marker="$(node_modules_receipt_path "$scope")"
      [ -f "$marker" ] && [ ! -L "$marker" ] || return 1
      IFS=' ' read -r receipt_version receipt_input receipt_structure ignored < "$marker" || return 1
      [ -z "$ignored" ] && [ "$receipt_version" = "v2" ] && [ "$receipt_input" = "$input_fingerprint" ] && [ -n "$receipt_structure" ] || return 1
      actual_structure="$(node_modules_structure_proof "$scope" "$effective_config")" || return 1
      [ "$receipt_structure" = "$actual_structure" ] || return 1
      printf '%s\n' "$actual_structure"
      ;;
    empty)
      dependency_scope_is_dependency_free "$scope" "$effective_config" || return 1
      [ ! -e "$scope/.pnp.cjs" ] && [ ! -e "$scope/.pnp.js" ] || return 1
      if [ -e "$scope/node_modules" ]; then
        path_is_real_directory "$scope/node_modules" || return 1
        if node_modules_structure_proof "$scope" "$effective_config" >/dev/null 2>&1; then
          return 1
        fi
      fi
      printf 'empty:%s\n' "$input_fingerprint"
      ;;
    pnp-cjs)
      path_is_nonempty_real_file "$scope/.pnp.cjs" || return 1
      [ ! -e "$scope/.pnp.js" ] && [ ! -e "$scope/node_modules" ] || return 1
[% if web_package_manager == "yarn" %]
      if [ -n "$effective_config" ]; then
        yarn_berry_pnp_artifact_proof "$scope" "$scope/.pnp.cjs" "$effective_config"
      else
[% endif %]
        fingerprint_files \
          "$scope/.pnp.cjs" \
          "$scope/.pnp.data.json" \
          "$scope/.pnp.loader.mjs" \
          "$scope/.yarn/cache" \
          "$scope/.yarn/install-state.gz" \
          "$scope/.yarn/unplugged"
[% if web_package_manager == "yarn" %]
      fi
[% endif %]
      ;;
    pnp-js)
      path_is_nonempty_real_file "$scope/.pnp.js" || return 1
      [ ! -e "$scope/.pnp.cjs" ] && [ ! -e "$scope/node_modules" ] || return 1
[% if web_package_manager == "yarn" %]
      if [[ "$effective_config" == classic:* ]]; then
        yarn_classic_pnp_artifact_proof "$scope" "$scope/.pnp.js" "$effective_config"
      elif [ -n "$effective_config" ]; then
        yarn_berry_pnp_artifact_proof "$scope" "$scope/.pnp.js" "$effective_config"
      else
[% endif %]
        fingerprint_files \
          "$scope/.pnp.js" \
          "$scope/.pnp.data.json" \
          "$scope/.pnp.loader.mjs" \
          "$scope/.yarn/cache" \
          "$scope/.yarn/install-state.gz" \
          "$scope/.yarn/unplugged"
[% if web_package_manager == "yarn" %]
      fi
[% endif %]
      ;;
    *) return 1 ;;
  esac
}

clear_dependency_state() {
  local scope="$1"

  rm -f "$(dependency_stamp_path "$scope")" "$(node_modules_receipt_path "$scope")"
}

dependencies_present() {
  local app_dir="$1"
  local scope lockfile stamp expected version actual kind proof ignored configured_kind actual_proof effective_config status
[% if web_package_manager == "yarn" %]
  local lockfile_kind
[% endif %]

  scope="$(dependency_scope "$app_dir")" || {
    status=$?
    [ "$status" -eq 1 ] && status=2
    return "$status"
  }
  lockfile="$(dependency_lockfile "$scope")" || return $?
[% if web_package_manager == "pnpm" %]
  pnpm_package_manager_spec_for_scope "$scope" >/dev/null || {
    status=$?
    [ "$status" -eq 1 ] && status=2
    return "$status"
  }
[% endif %]
[% if web_package_manager == "yarn" %]
  # Validate the selected lockfile authority before consulting a possibly
  # absent/stale receipt so malformed or symlinked locks are always hard errors.
  lockfile_kind="$(yarn_lockfile_kind "$lockfile")" || return $?
  yarn_package_manager_spec_for_scope "$app_dir" "$scope" >/dev/null || {
    status=$?
    [ "$status" -eq 1 ] && status=2
    return "$status"
  }
[% endif %]
  stamp="$(dependency_stamp_path "$scope")"
  [ -f "$stamp" ] && [ ! -L "$stamp" ] || return 1
  IFS=' ' read -r version actual kind proof ignored < "$stamp" || return 1
  [ -z "$ignored" ] && [ "$version" = "v5" ] && [ -n "$actual" ] && [ -n "$kind" ] && [ -n "$proof" ] || return 1
[% if web_package_manager == "pnpm" %]
  effective_config="$(pnpm_dependency_contract "$app_dir" "$scope")" || {
    status=$?
    [ "$status" -eq 1 ] && status=2
    return "$status"
  }
[% else %]
  effective_config="$(dependency_yarn_config "$scope" "$lockfile")" || {
    status=$?
    [ "$status" -eq 1 ] && status=2
    return "$status"
  }
[% endif %]
  expected="$(dependency_fingerprint "$scope" "$lockfile" "$effective_config")" || {
    status=$?
    [ "$status" -eq 1 ] && status=2
    return "$status"
  }
  [ "$actual" = "$expected" ] || return 1
[% if web_package_manager == "yarn" %]
  if [ "$lockfile_kind" = "classic" ]; then
    case "$kind" in
      node-modules|pnp-js|empty) ;;
      *) return 1 ;;
    esac
  else
[% endif %]
    configured_kind="$(dependency_artifact_kind "$scope" "$lockfile" "$effective_config")" || {
      status=$?
      [ "$status" -eq 1 ] && status=2
      return "$status"
    }
    if [ "$kind" = "empty" ]; then
      [ "$configured_kind" = "node-modules" ] || return 1
    else
      [ "$kind" = "$configured_kind" ] || return 1
    fi
[% if web_package_manager == "yarn" %]
  fi
[% endif %]
  actual_proof="$(artifact_proof "$scope" "$kind" "$expected" "$effective_config")" || return $?
  [ "$proof" = "$actual_proof" ]
}

record_dependency_state() {
  local app_dir="$1"
  local fixed_scope="${2:-}"
  local provided_config="${3:-}"
  local scope lockfile stamp fingerprint kind proof structure marker temporary marker_temporary effective_config status

  if [ -n "$fixed_scope" ]; then
    scope="$fixed_scope"
  else
    scope="$(dependency_scope "$app_dir")" || {
      status=$?
      [ "$status" -eq 1 ] && status=2
      return "$status"
    }
  fi
  lockfile="$(dependency_lockfile "$scope")" || return
[% if web_package_manager == "pnpm" %]
  [ -n "$provided_config" ] || {
    echo "Missing frozen pnpm dependency contract while recording '$scope'." >&2
    return 1
  }
  effective_config="$provided_config"
[% else %]
  effective_config="$(dependency_yarn_config "$scope" "$lockfile")" || {
    status=$?
    [ "$status" -eq 1 ] && status=2
    return "$status"
  }
[% endif %]
  fingerprint="$(dependency_fingerprint "$scope" "$lockfile" "$effective_config")" || return
  kind="$(dependency_artifact_kind "$scope" "$lockfile" "$effective_config")" || {
    status=$?
    [ "$status" -eq 1 ] && status=2
    return "$status"
  }
  if [ "$kind" = "node-modules" ]; then
    if ! path_is_real_directory "$scope/node_modules" || ! structure="$(node_modules_structure_proof "$scope" "$effective_config")"; then
      if dependency_scope_is_dependency_free "$scope" "$effective_config" && { [ ! -e "$scope/node_modules" ] || path_is_real_directory "$scope/node_modules"; }; then
        kind="empty"
      else
        echo "Web dependency installation completed without a proved node_modules tree in '$scope'." >&2
        return 1
      fi
    fi
[% if web_package_manager == "yarn" %]
    if [ -e "$scope/.pnp.cjs" ] || [ -e "$scope/.pnp.js" ]; then
      echo "Yarn dependency installation left artifacts for both node-modules and PnP linkers in '$scope'." >&2
      return 1
    fi
[% endif %]
    if [ "$kind" = "node-modules" ]; then
      marker="$(node_modules_receipt_path "$scope")"
      marker_temporary="${marker}.tmp.$$"
      printf 'v2 %s %s\n' "$fingerprint" "$structure" > "$marker_temporary"
      mv -f "$marker_temporary" "$marker"
    fi
  fi
  proof="$(artifact_proof "$scope" "$kind" "$fingerprint" "$effective_config")" || {
    echo "Web dependency installation completed without the expected $kind artifact in '$scope'." >&2
    return 1
  }
  stamp="$(dependency_stamp_path "$scope")"
  temporary="${stamp}.tmp.$$"
  mkdir -p "$(dirname "$stamp")"
  printf 'v5 %s %s %s\n' "$fingerprint" "$kind" "$proof" > "$temporary"
  mv -f "$temporary" "$stamp"
}

is_cygwin_msys_shell() {
  local kernel

  # OSTYPE is a read-only Bash platform value, so prefer it over mutable
  # environment hints such as MSYSTEM when deciding whether PIDs belong to
  # the Cygwin/MSYS namespace rather than native Windows.
  case "${OSTYPE:-}" in
    cygwin*|msys*|mingw*|win32*) return 0 ;;
  esac
  kernel="$(uname -s 2>/dev/null)" || return 1
  case "$kernel" in
    CYGWIN*|MSYS*|MINGW*) return 0 ;;
    *) return 1 ;;
  esac
}

process_stat_start_ticks() {
  local expected_pid="$1"
  local process_stat="$2"

  case "$expected_pid" in
    ''|*[!0-9]*) return 1 ;;
  esac
  case "$expected_pid" in
    *[1-9]*) ;;
    *) return 1 ;;
  esac
  # Linux, Cygwin, and MSYS use the Linux-compatible /proc/<pid>/stat
  # layout. The parenthesized comm field can contain spaces and `)`, so strip
  # through its final delimiter before selecting field 22 (field 20 in the
  # remainder beginning with process state). Validate the PID, state, field
  # count, and start tick token before trusting the result.
  printf '%s\n' "$process_stat" | LC_ALL=C awk -v expected_pid="$expected_pid" '
    BEGIN { valid = 0 }
    NR != 1 { exit 1 }
    {
      first_space = index($0, " ")
      if (first_space < 2) exit 1
      actual_pid = substr($0, 1, first_space - 1)
      if (actual_pid !~ /^[0-9]+$/ || ("x" actual_pid) != ("x" expected_pid)) exit 1
      remainder = $0
      original = remainder
      sub(/^.*\) /, "", remainder)
      if (remainder == original) exit 1
      count = split(remainder, fields, /[[:space:]]+/)
      if (count < 20 || fields[1] !~ /^[A-Za-z]$/ || fields[20] !~ /^[0-9]+$/ || fields[20] !~ /[1-9]/) exit 1
      print fields[20]
      valid = 1
    }
    END { if (!valid) exit 1 }
  '
}

process_stat_group_id() {
  local expected_pid="$1"
  local process_stat="$2"

  case "$expected_pid" in
    ''|*[!0-9]*) return 1 ;;
  esac
  case "$expected_pid" in
    *[1-9]*) ;;
    *) return 1 ;;
  esac
  printf '%s\n' "$process_stat" | LC_ALL=C awk -v expected_pid="$expected_pid" '
    BEGIN { valid = 0 }
    NR != 1 { exit 1 }
    {
      first_space = index($0, " ")
      if (first_space < 2) exit 1
      actual_pid = substr($0, 1, first_space - 1)
      if (actual_pid !~ /^[0-9]+$/ || ("x" actual_pid) != ("x" expected_pid)) exit 1
      remainder = $0
      original = remainder
      sub(/^.*\) /, "", remainder)
      if (remainder == original) exit 1
      count = split(remainder, fields, /[[:space:]]+/)
      # pgrp is field 5, or field 3 in the remainder beginning with state.
      if (count < 3 || fields[1] !~ /^[A-Za-z]$/ || fields[3] !~ /^[0-9]+$/ || fields[3] !~ /[1-9]/) exit 1
      print fields[3]
      valid = 1
    }
    END { if (!valid) exit 1 }
  '
}

procfs_boot_time_from_stat() {
  local process_stat="$1"

  printf '%s\n' "$process_stat" | LC_ALL=C awk '
    BEGIN { valid = 1; found = 0 }
    $1 == "btime" {
      if (found || NF != 2 || $2 !~ /^[0-9]+$/ || $2 !~ /[1-9]/) {
        valid = 0
        next
      }
      value = $2
      found = 1
    }
    END {
      if (!valid || !found) exit 1
      print value
    }
  '
}

procfs_boot_time() {
  local process_stat

  [ -r /proc/stat ] || return 1
  process_stat="$(< /proc/stat)" || return 1
  procfs_boot_time_from_stat "$process_stat"
}

validated_positive_integer() {
  local value="$1"

  case "$value" in
    ''|*[!0-9]*) return 1 ;;
  esac
  case "$value" in
    *[1-9]*) ;;
    *) return 1 ;;
  esac
  printf '%s\n' "$value"
}

procfs_pid_namespace() {
  local pid="$1"
  local namespace namespace_id

  validated_positive_integer "$pid" >/dev/null || return 1
  [ -L "/proc/$pid/ns/pid" ] || return 1
  namespace="$(readlink "/proc/$pid/ns/pid")" || return 1
  if [[ "$namespace" =~ ^pid:\[([0-9]+)\]$ ]]; then
    namespace_id="${BASH_REMATCH[1]}"
    if [[ "$namespace_id" =~ [1-9] ]]; then
      printf '%s\n' "$namespace_id"
      return
    fi
  fi
  return 1
}

process_start_identity() {
  local pid="$1"
  local boot_id boot_time identity pid_namespace process_stat start_ticks

  case "$pid" in
    ''|*[!0-9]*) return 1 ;;
  esac
  case "$pid" in
    *[1-9]*) ;;
    *) return 1 ;;
  esac
  if [ -r "/proc/$pid/stat" ]; then
    process_stat="$(< "/proc/$pid/stat")" || return 1
    start_ticks="$(process_stat_start_ticks "$pid" "$process_stat")" || return 1
    if [ -r /proc/sys/kernel/random/boot_id ]; then
      boot_id="$(tr -d '\r\n' < /proc/sys/kernel/random/boot_id)" || return 1
      case "$boot_id" in
        ''|*[!A-Za-z0-9-]*) return 1 ;;
      esac
      pid_namespace="$(procfs_pid_namespace "$pid")" || return 1
      printf 'linux.%s.%s.%s\n' "$boot_id" "$pid_namespace" "$start_ticks"
      return
    fi
    if is_cygwin_msys_shell; then
      boot_time="$(procfs_boot_time)" || return 1
      printf 'cygwin-msys.%s.%s\n' "$boot_time" "$start_ticks"
      return
    fi
  fi

  # Do not substitute a weaker wall-clock identity for Cygwin/MSYS procfs.
  # An incomplete Windows POSIX environment is unverified and therefore
  # cannot own or recover the dependency lock.
  is_cygwin_msys_shell && return 1

  # BSD ps exposes a wall-clock start value. Force UTC so two coordinators
  # with different TZ environments cannot classify the same live PID as reused.
  identity="$(TZ=UTC0 LC_ALL=C ps -p "$pid" -o lstart= 2>/dev/null | tr -d '[:space:]')" || return 1
  [ -n "$identity" ] || return 1
  case "$identity" in
    *[!A-Za-z0-9:._+-]*) return 1 ;;
  esac
  printf 'ps.%s\n' "$identity"
}

process_group_id() {
  local pid="$1"
  local process_group process_stat

  case "$pid" in
    ''|*[!0-9]*) return 1 ;;
  esac
  case "$pid" in
    *[1-9]*) ;;
    *) return 1 ;;
  esac
  if [ -e "/proc/$pid/pgid" ]; then
    [ -r "/proc/$pid/pgid" ] || return 1
    process_group="$(tr -d '\r\n' < "/proc/$pid/pgid")" || return 1
  elif [ -r "/proc/$pid/stat" ]; then
    process_stat="$(< "/proc/$pid/stat")" || return 1
    process_group="$(process_stat_group_id "$pid" "$process_stat")" || return 1
  else
    # BSD ps is the portable fallback when Linux-compatible procfs does not
    # expose either a dedicated process-group file or a process stat record.
    process_group="$(TZ=UTC0 LC_ALL=C ps -p "$pid" -o pgid= 2>/dev/null | tr -d '[:space:]')" || return 1
  fi
  validated_positive_integer "$process_group"
}

process_liveness_state() {
  local pid="$1"
  local boot_time probe process_stat

  case "$pid" in
    ''|*[!0-9]*) return 2 ;;
  esac
  case "$pid" in
    *[1-9]*) ;;
    *) return 2 ;;
  esac

  if kill -0 "$pid" 2>/dev/null; then
    return 0
  fi

  if is_cygwin_msys_shell; then
    # Native Windows Node uses Windows process identifiers, which are not the
    # Cygwin/MSYS PID namespace used in lock metadata. Consult that shell's
    # procfs exclusively and fail closed if it cannot prove the state.
    boot_time="$(procfs_boot_time)" || return 2
    if [ -r "/proc/$pid/stat" ]; then
      process_stat="$(< "/proc/$pid/stat")" || return 2
      process_stat_start_ticks "$pid" "$process_stat" >/dev/null || return 2
      return 0
    fi
    [ ! -e "/proc/$pid" ] || return 2
    # Prove that this procfs represents the current MSYS/Cygwin namespace
    # before treating a missing per-process entry as ESRCH-equivalent.
    [ -r "/proc/$$/stat" ] || return 2
    process_stat="$(< "/proc/$$/stat")" || return 2
    process_stat_start_ticks "$$" "$process_stat" >/dev/null || return 2
    [ -n "$boot_time" ] || return 2
    return 1
  fi

  # Shell kill does not expose whether failure was ESRCH or EPERM. Node is a
  # required web-check dependency and preserves the exact errno. Any probe
  # execution failure or unexpected result is fail-closed and bounded by the
  # install-lock acquisition loop rather than misclassified as stale.
  probe="$("$node_bin" - --jig-process-probe "$pid" <<'NODE'
const pid = Number(process.argv[3]);
try {
  process.kill(pid, 0);
  process.stdout.write("live\n");
} catch (error) {
  process.stdout.write(error?.code === "ESRCH" ? "stale\n" : "unverified\n");
}
NODE
)" || return 2
  case "$probe" in
    live) return 0 ;;
    stale) return 1 ;;
    *) return 2 ;;
  esac
}

process_group_liveness_state() {
  local process_group="$1"
  local probe

  validated_positive_integer "$process_group" >/dev/null || return 2
  if kill -0 -- "-$process_group" 2>/dev/null; then
    return 0
  fi

  # Native Windows Node and MSYS/Cygwin use different PID namespaces, so a
  # negative native PID cannot prove that a POSIX process group is absent.
  is_cygwin_msys_shell && return 2

  # Shell kill collapses ESRCH and EPERM into the same non-zero status. Only
  # an exact ESRCH result proves that every process in the installer's group
  # has exited; permissions and unexpected failures remain fail-closed.
  probe="$("$node_bin" - --jig-process-group-probe "$process_group" <<'NODE'
const processGroup = Number(process.argv[3]);
try {
  process.kill(-processGroup, 0);
  process.stdout.write("live\n");
} catch (error) {
  process.stdout.write(error?.code === "ESRCH" ? "stale\n" : "unverified\n");
}
NODE
)" || return 2
  case "$probe" in
    live) return 0 ;;
    stale) return 1 ;;
    *) return 2 ;;
  esac
}

linux_identity_pid_namespace() {
  local identity="$1"
  local namespace

  if [[ "$identity" =~ ^linux\.[A-Za-z0-9-]+\.([0-9]+)\.[0-9]+$ ]]; then
    namespace="${BASH_REMATCH[1]}"
    if [[ "$namespace" =~ [1-9] ]]; then
      printf '%s\n' "$namespace"
      return
    fi
  fi
  return 1
}

process_identity_state() {
  local pid="$1"
  local expected_start="$2"
  local actual_namespace actual_start current_namespace current_start expected_namespace liveness

  case "$expected_start" in
    linux.*)
      # A PID from another namespace cannot be classified with this namespace's
      # kill(2) result: the same number may name a different process or no local
      # process at all. Reject foreign and legacy identities before liveness.
      expected_namespace="$(linux_identity_pid_namespace "$expected_start")" || return 2
      current_start="$(process_start_identity "$$")" || return 2
      current_namespace="$(linux_identity_pid_namespace "$current_start")" || return 2
      [ "$expected_namespace" = "$current_namespace" ] || return 2
      ;;
  esac

  if process_liveness_state "$pid"; then
    :
  else
    liveness=$?
    return "$liveness"
  fi
  if [ "$expected_start" = "unknown" ]; then
    return 2
  fi
  actual_start="$(process_start_identity "$pid")" || return 2
  case "$expected_start" in
    linux.*)
      # PID numbers are meaningful only within one Linux PID namespace. A
      # cross-namespace collision (or a legacy identity that did not bind the
      # namespace) is unverified, never proof that an install owner is stale.
      actual_namespace="$(linux_identity_pid_namespace "$actual_start")" || return 2
      [ "$expected_namespace" = "$actual_namespace" ] || return 2
      ;;
  esac
  [ "$actual_start" = "$expected_start" ] && return 0
  return 1
}

install_lock_state() {
  local owner="$1"
  local token="$2"
  local owner_start="$3"
  local owner_group="" identity="$owner_start" state

  if [ "$owner" = "$$" ] && { [ -z "$install_lock_token" ] || [ "$token" != "$install_lock_token" ]; }; then
    return 1
  fi
  case "$owner_start" in
    *.g[0-9]*)
      owner_group="${owner_start##*.g}"
      identity="${owner_start%.*}"
      case "$owner_group" in
        ''|*[!0-9]*) return 2 ;;
      esac
      ;;
  esac
  if process_identity_state "$owner" "$identity"; then
    return 0
  else
    state=$?
  fi
  if [ "$state" -eq 1 ] && [ -n "$owner_group" ]; then
    # Dedicated installer groups retain their PGID while any installer
    # descendant survives, even if the worker leader was killed. Only exact
    # ESRCH proves the group is stale; EPERM and probe failures are unverified.
    process_group_liveness_state "$owner_group"
    return $?
  fi
  return "$state"
}

install_lock_is_stale() {
  local state

  if install_lock_state "$@"; then
    return 1
  else
    state=$?
  fi
  [ "$state" -eq 1 ]
}

recovery_claim_is_live() {
  local claimant="$1"
  local claimant_token="$2"
  local claimant_start="$3"
  local state

  if [ "$claimant" = "$$" ] && { [ -z "$recovery_claim_token" ] || [ "$claimant_token" != "$recovery_claim_token" ]; }; then
    return 1
  fi
  if process_identity_state "$claimant" "$claimant_start"; then
    return 0
  else
    state=$?
  fi
  [ "$state" -eq 2 ]
}

install_lock_metadata() {
  local path="${1:-$install_lock_path}"
  local owner token owner_start ignored

  [ -f "$path" ] || return 1
  IFS=' ' read -r owner token owner_start ignored < "$path" || return 1
  [ -z "$ignored" ] || return 1
  case "$owner" in
    ''|*[!0-9]*) return 1 ;;
  esac
  if [ -z "$token" ]; then
    token="legacy.$owner"
  fi
  case "$token" in
    *[!A-Za-z0-9._-]*) return 1 ;;
  esac
  owner_start="${owner_start:-unknown}"
  case "$owner_start" in
    *[!A-Za-z0-9:._+-]*) return 1 ;;
  esac
  printf '%s %s %s\n' "$owner" "$token" "$owner_start"
}

create_install_lock() {
  local token="$$.${RANDOM}.${RANDOM}"
  local candidate="${install_lock_path}.candidate.${token}"
  local owner_start

  owner_start="$(process_start_identity "$$")" || owner_start="unknown"
  if ! (set -o noclobber; printf '%s %s %s\n' "$$" "$token" "$owner_start" > "$candidate") 2>/dev/null; then
    return 1
  fi
  if ln "$candidate" "$install_lock_path" 2>/dev/null; then
    install_lock_token="$token"
    # A recovery killed after moving an older generation may leave harmless
    # sidecars. The new generation cannot share their token, so retire them.
    rm -f "${install_lock_path}.candidate."* "${install_lock_path}.recover."* "${install_lock_path}.observed."* "${install_lock_path}.stale."*
    return 0
  fi
  rm -f "$candidate"
  return 1
}

recovery_claim_metadata() {
  local path="$1"
  local claimant claimant_token claimant_start lock_owner lock_token lock_owner_start ignored

  [ -f "$path" ] || return 1
  IFS=' ' read -r claimant claimant_token claimant_start lock_owner lock_token lock_owner_start ignored < "$path" || return 1
  [ -z "$ignored" ] || return 1
  case "$claimant" in
    ''|*[!0-9]*) return 1 ;;
  esac
  case "$lock_owner" in
    ''|*[!0-9]*) return 1 ;;
  esac
  [ -n "$claimant_token" ] && [ -n "$lock_token" ] && [ -n "$claimant_start" ] && [ -n "$lock_owner_start" ] || return 1
  case "$claimant_token$lock_token" in
    *[!A-Za-z0-9._-]*) return 1 ;;
  esac
  case "$claimant_start$lock_owner_start" in
    *[!A-Za-z0-9:._+-]*) return 1 ;;
  esac
  printf '%s %s %s %s %s %s\n' "$claimant" "$claimant_token" "$claimant_start" "$lock_owner" "$lock_token" "$lock_owner_start"
}

create_recovery_claim() {
  local path="$1"
  local lock_owner="$2"
  local lock_token="$3"
  local lock_owner_start="$4"
  local claimant_token="$$.${RANDOM}.${RANDOM}"
  local candidate="${path}.candidate.${claimant_token}"
  local claimant_start

  claimant_start="$(process_start_identity "$$")" || claimant_start="unknown"
  if ! (set -o noclobber; printf '%s %s %s %s %s %s\n' "$$" "$claimant_token" "$claimant_start" "$lock_owner" "$lock_token" "$lock_owner_start" > "$candidate") 2>/dev/null; then
    return 1
  fi
  if ln "$candidate" "$path" 2>/dev/null; then
    recovery_claim_path="$path"
    recovery_claim_token="$claimant_token"
    rm -f "$candidate"
    return 0
  fi
  rm -f "$candidate"
  return 1
}

acquire_recovery_claim() {
  local lock_owner="$1"
  local lock_token="$2"
  local lock_owner_start="$3"
  local root path metadata claimant claimant_token claimant_start claim_owner claim_lock_token claim_lock_owner_start next

  root="${install_lock_path}.recover.${lock_token}"
  path="$root"
  while [ -f "$path" ]; do
    metadata="$(recovery_claim_metadata "$path")" || return 1
    read -r claimant claimant_token claimant_start claim_owner claim_lock_token claim_lock_owner_start <<< "$metadata"
    if [ "$claim_owner" != "$lock_owner" ] || [ "$claim_lock_token" != "$lock_token" ] || [ "$claim_lock_owner_start" != "$lock_owner_start" ]; then
      return 1
    fi
    next="${root}.next.${claimant_token}"
    if [ -f "$next" ]; then
      path="$next"
      continue
    fi
    if recovery_claim_is_live "$claimant" "$claimant_token" "$claimant_start"; then
      return 1
    fi
    path="$next"
    break
  done

  create_recovery_claim "$path" "$lock_owner" "$lock_token" "$lock_owner_start"
}

release_recovery_claim() {
  local metadata claimant claimant_token claimant_start lock_owner lock_token lock_owner_start root next

  [ -n "$recovery_claim_path" ] || return
  metadata="$(recovery_claim_metadata "$recovery_claim_path")" || return
  read -r claimant claimant_token claimant_start lock_owner lock_token lock_owner_start <<< "$metadata"
  root="${install_lock_path}.recover.${lock_token}"
  next="${root}.next.${claimant_token}"
  if [ "$claimant" = "$$" ] && [ "$claimant_token" = "$recovery_claim_token" ] && [ ! -e "$next" ]; then
    rm -f "$recovery_claim_path"
  fi
  recovery_claim_path=""
  recovery_claim_token=""
}

cleanup_recovery_artifacts() {
  local lock_token="$1"
  local root="${install_lock_path}.recover.${lock_token}"

  rm -f "${install_lock_path}.candidate.${lock_token}" "$root" "$root".candidate.* "$root".next.* "${install_lock_path}.observed.${lock_token}."*
  recovery_claim_path=""
  recovery_claim_token=""
}

recover_stale_install_lock() {
  local metadata owner token owner_start current current_owner current_token current_owner_start observation observation_metadata observation_owner observation_token observation_owner_start stale

  metadata="$(install_lock_metadata)" || return 1
  read -r owner token owner_start <<< "$metadata"
  install_lock_is_stale "$owner" "$token" "$owner_start" || return 1

  acquire_recovery_claim "$owner" "$token" "$owner_start" || return 1
  current="$(install_lock_metadata)" || {
    release_recovery_claim
    return 1
  }
  read -r current_owner current_token current_owner_start <<< "$current"
  if [ "$current_owner" != "$owner" ] || [ "$current_token" != "$token" ] || [ "$current_owner_start" != "$owner_start" ] || ! install_lock_is_stale "$owner" "$token" "$owner_start"; then
    release_recovery_claim
    return 1
  fi

  observation="${install_lock_path}.observed.${token}.${recovery_claim_token}"
  if ! ln "$install_lock_path" "$observation" 2>/dev/null; then
    release_recovery_claim
    return 1
  fi
  observation_metadata="$(install_lock_metadata "$observation")" || {
    rm -f "$observation"
    release_recovery_claim
    return 1
  }
  read -r observation_owner observation_token observation_owner_start <<< "$observation_metadata"
  if [ "$observation_owner" != "$owner" ] || [ "$observation_token" != "$token" ] || [ "$observation_owner_start" != "$owner_start" ] || [ ! "$install_lock_path" -ef "$observation" ] || ! install_lock_is_stale "$owner" "$token" "$owner_start"; then
    rm -f "$observation"
    release_recovery_claim
    return 1
  fi

  stale="${install_lock_path}.stale.${token}.${recovery_claim_token}"
  if mv "$install_lock_path" "$stale" 2>/dev/null; then
    rm -f "$stale"
    cleanup_recovery_artifacts "$token"
    return 0
  fi
  rm -f "$observation"
  release_recovery_claim
  return 1
}

release_install_lock() {
  local metadata owner token owner_start

  metadata="$(install_lock_metadata)" || {
    install_lock_token=""
    trap - EXIT
    return
  }
  read -r owner token owner_start <<< "$metadata"
  if [ "$owner" = "$$" ] && [ "$token" = "$install_lock_token" ]; then
    rm -f "$install_lock_path"
  fi
  install_lock_token=""
  trap - EXIT
}

acquire_install_lock() {
  local app_dir="$1"
  local metadata owner token owner_start owner_state
  local unresolved_attempts=0
  local max_unresolved_attempts="${JIG_WEB_INSTALL_LOCK_UNRESOLVED_ATTEMPTS:-600}"
  local poll_seconds="${JIG_WEB_INSTALL_LOCK_POLL_SECONDS:-0.1}"

  case "$max_unresolved_attempts" in
    ''|*[!0-9]*|0)
      echo "JIG_WEB_INSTALL_LOCK_UNRESOLVED_ATTEMPTS must be a positive integer." >&2
      return 2
      ;;
  esac
  if [[ ! "$poll_seconds" =~ ^[0-9]+([.][0-9]+)?$ ]]; then
    echo "JIG_WEB_INSTALL_LOCK_POLL_SECONDS must be a non-negative number." >&2
    return 2
  fi

  mkdir -p .agent/tmp
  while :; do
    if create_install_lock; then
      trap 'release_install_lock' EXIT
      return 0
    fi
    if metadata="$(install_lock_metadata)"; then
      read -r owner token owner_start <<< "$metadata"
      if install_lock_state "$owner" "$token" "$owner_start"; then
        # A verified owner may legitimately spend longer than a fixed timeout
        # downloading packages. Ctrl-C remains the escape hatch for a wedged
        # install; never steal or recommend deleting its live lock.
        sleep "$poll_seconds"
        continue
      else
        owner_state=$?
      fi
      if [ "$owner_state" -eq 1 ] && recover_stale_install_lock; then
        unresolved_attempts=0
        continue
      fi
    fi
    unresolved_attempts=$((unresolved_attempts + 1))
    if [ "$unresolved_attempts" -ge "$max_unresolved_attempts" ]; then
      # Close the race where an owner releases between the final recovery
      # attempt and this diagnostic without rerunning a full dependency proof.
      if create_install_lock; then
        trap 'release_install_lock' EXIT
        return 0
      fi
      if metadata="$(install_lock_metadata)"; then
        read -r owner token owner_start <<< "$metadata"
        if install_lock_state "$owner" "$token" "$owner_start"; then
          unresolved_attempts=0
          sleep "$poll_seconds"
          continue
        fi
      fi
      echo "Could not safely acquire the web dependency install lock '$install_lock_path'. Its owner metadata could not be validated or recovered; inspect the lock and retry after confirming that no web dependency install is active." >&2
      return 1
    fi
    sleep "$poll_seconds"
  done
}

transfer_install_lock_to_worker() {
  local worker="$1"
  local worker_start="$2"
  local metadata owner token owner_start temporary

  metadata="$(install_lock_metadata)" || return 1
  read -r owner token owner_start <<< "$metadata"
  [ "$owner" = "$$" ] && [ "$token" = "$install_lock_token" ] || return 1
  temporary="${install_lock_path}.handoff.${token}"
  if ! (set -o noclobber; printf '%s %s %s.g%s\n' "$worker" "$token" "$worker_start" "$worker" > "$temporary") 2>/dev/null; then
    return 1
  fi
  metadata="$(install_lock_metadata)" || {
    rm -f "$temporary"
    return 1
  }
  read -r owner token owner_start <<< "$metadata"
  if [ "$owner" != "$$" ] || [ "$token" != "$install_lock_token" ]; then
    rm -f "$temporary"
    return 1
  fi
  if ! mv -f "$temporary" "$install_lock_path"; then
    rm -f "$temporary"
    return 1
  fi
  install_lock_token=""
  trap - EXIT
}

preserve_install_lock_for_group_recovery() {
  # Once the worker owns a generation, only its controlled completion path may
  # remove the lock. An unexpected exit can leave package-manager descendants
  # alive, so retain the worker PGID in the lock for stale recovery to inspect.
  install_lock_token=""
  trap - EXIT
}

forward_install_worker_signal() {
  local signal="$1"

  [ -n "$install_worker_signal" ] || install_worker_signal="$signal"
  if [ -n "$install_worker_pid" ]; then
    if [ "$install_worker_group" = "$install_worker_pid" ]; then
      kill -TERM -- "-$install_worker_group" 2>/dev/null || true
    else
      kill -TERM "$install_worker_pid" 2>/dev/null || true
    fi
  fi
}

install_worker_signal_status() {
  case "$1" in
    HUP) printf '%s\n' 129 ;;
    INT) printf '%s\n' 130 ;;
    TERM) printf '%s\n' 143 ;;
    *) printf '%s\n' 1 ;;
  esac
}

wait_for_install_worker_exit() {
  local active_jobs active_pid job_state status=0

  while :; do
    if wait "$install_worker_pid"; then
      status=0
    else
      status=$?
    fi
    # A trapped signal interrupts Bash's wait before the child necessarily
    # exits. Consult Bash's owned job table rather than probing the numeric PID:
    # after a successful wait the PID may already name an unrelated process.
    active_jobs="$(jobs -p)" || return 1
    job_state=1
    while IFS= read -r active_pid; do
      if [ "$active_pid" = "$install_worker_pid" ]; then
        job_state=0
        break
      fi
    done <<< "$active_jobs"
    [ "$job_state" -eq 0 ] || return "$status"
  done
}

terminate_install_worker() {
  [ -n "$install_worker_pid" ] || return
  if [ "$install_worker_group" = "$install_worker_pid" ]; then
    kill -TERM -- "-$install_worker_group" 2>/dev/null || true
  else
    kill -TERM "$install_worker_pid" 2>/dev/null || true
  fi
}

finish_install_worker() {
  local status="$1"
  local signal

  trap - INT TERM HUP
  signal="$install_worker_signal"
  install_worker_pid=""
  install_worker_group=""
  install_worker_signal=""
  if [ -n "$signal" ]; then
    status="$(install_worker_signal_status "$signal")"
  fi
  return "$status"
}

[% if web_package_manager == "npm" %]
run_managed_npm_command() {
  local operation="$1"
  local app_dir="$2"
  local operation_argument="$3"

  "$node_bin" - --jig-managed-npm "$operation" "$app_dir" "$operation_argument" <<'NODE'
const { spawnSync } = require("node:child_process");
const path = require("node:path");

const launcherOperation = process.argv[3];
const appDir = path.resolve(process.argv[4]);
const operationArgument = process.argv[5];

const dependencySelectionEnvironment = new Set([
  "omit",
  "include",
  "production",
  "optional",
  "only",
  "dev",
  "also",
]);
const projectSelectionEnvironment = new Set([
  "global",
  "location",
  "if-present",
  "workspace",
  "workspaces",
  "include-workspace-root",
  "prefix",
]);
const installShapingEnvironment = new Set([
  ...dependencySelectionEnvironment,
  ...projectSelectionEnvironment,
  "bin-links",
  "dry-run",
  "package-lock-only",
  "package-lock",
  "cpu",
  "os",
  "libc",
]);

function normalizedNpmConfigKey(key) {
  const match = /^npm_config_(.*)$/i.exec(key);
  if (!match) return null;
  return match[1].replaceAll("_", "-").toLowerCase();
}

function managedNpmEnvironment(managedSettings, clearNodeEnvironment) {
  const environment = { ...process.env };
  if (clearNodeEnvironment) delete environment.NODE_ENV;
  for (const key of Object.keys(environment)) {
    const configKey = normalizedNpmConfigKey(key);
    if (configKey !== null && managedSettings.has(configKey)) {
      delete environment[key];
    }
  }
  return environment;
}

let args;
let environment;
let description;
if (launcherOperation === "install") {
  const npmOperation = operationArgument === "frozen"
    ? "ci"
    : operationArgument === "bootstrap"
      ? "install"
      : null;
  if (!npmOperation) process.exit(2);
  args = [
    npmOperation,
    "--include=dev",
    "--include=optional",
    "--include=peer",
    "--bin-links=true",
    "--dry-run=false",
    "--package-lock-only=false",
    "--package-lock=true",
    "--global=false",
    "--location=project",
    `--prefix=${appDir}`,
    `--cpu=${process.arch}`,
    `--os=${process.platform}`,
  ];
  if (process.platform === "linux") {
    const report = process.report?.getReport?.();
    args.push(report?.header?.glibcVersionRuntime ? "--libc=glibc" : "--libc=musl");
  }
  if (appDir === process.cwd()) {
    args.push("--workspaces=true", "--include-workspace-root=true");
  } else {
    args.push("--workspaces=false");
  }
  environment = managedNpmEnvironment(installShapingEnvironment, true);
  description = npmOperation;
} else if (launcherOperation === "run-script") {
  if (!operationArgument || /[\0\r\n]/.test(operationArgument)) process.exit(2);
  args = [
    "--prefix=.",
    "--workspace=.",
    "--workspaces=true",
    "--include-workspace-root=true",
    "--global=false",
    "--location=project",
    "--if-present=false",
    "--include=dev",
    "--include=optional",
    "--include=peer",
    "run",
    operationArgument,
  ];
  environment = managedNpmEnvironment(
    new Set([...dependencySelectionEnvironment, ...projectSelectionEnvironment]),
    false,
  );
  description = `run ${operationArgument}`;
} else {
  process.exit(2);
}

const result = spawnSync("npm", args, {
  cwd: appDir,
  env: environment,
  stdio: "inherit",
  shell: false,
});
if (result.error) {
  console.error(`Could not run npm ${description}: ${result.error.message}`);
  process.exit(1);
}
if (result.signal) {
  console.error(`npm ${description} terminated by ${result.signal}`);
  process.exit(1);
}
process.exit(result.status ?? 1);
NODE
}

run_npm_dependency_install() {
  run_managed_npm_command install "$1" "$2"
}

run_npm_package_script() {
  run_managed_npm_command run-script "$1" "$2"
}

[% endif %]
dependency_install_worker() {
  local app_dir="$1"
  local install_kind="$2"
  local expected_token="${JIG_WEB_INSTALL_WORKER_TOKEN:-}"
  local coordinator="${JIG_WEB_INSTALL_COORDINATOR_PID:-}"
  local coordinator_start="${JIG_WEB_INSTALL_COORDINATOR_START:-unknown}"
  local metadata owner token owner_start coordinator_state install_status=0 scope worker_start status
  local unresolved_handoff_attempts=0
  local max_unresolved_handoff_attempts=600
[% if web_package_manager == "pnpm" %]
  local lockfile pre_contract post_contract post_scope pre_lock_fingerprint post_lock_fingerprint
[% elif web_package_manager == "npm" %]
  local lockfile post_lockfile post_scope pre_input_fingerprint post_input_fingerprint pre_lock_fingerprint post_lock_fingerprint
[% endif %]

  [ -n "$expected_token" ] && [ -n "$coordinator" ] || return 2
  worker_start="$(process_start_identity "$$")" || return 2
  install_lock_token="$expected_token"
  while :; do
    if metadata="$(install_lock_metadata)"; then
      read -r owner token owner_start <<< "$metadata"
      [ "$token" = "$expected_token" ] || return 1
      if [ "$owner" = "$$" ]; then
        [ "$owner_start" = "${worker_start}.g$$" ] || return 1
        if process_identity_state "$owner" "$worker_start"; then
          trap 'preserve_install_lock_for_group_recovery' EXIT
          break
        fi
        return 1
      fi
      [ "$owner" = "$coordinator" ] || return 1
      if process_identity_state "$coordinator" "$coordinator_start"; then
        # A verified coordinator may be descheduled between spawning this
        # worker and atomically publishing the handoff. Keep waiting while its
        # exact process generation remains live; signals still terminate this
        # dedicated worker/process group.
        unresolved_handoff_attempts=0
      else
        coordinator_state=$?
        [ "$coordinator_state" -eq 2 ] || return 1
        unresolved_handoff_attempts=$((unresolved_handoff_attempts + 1))
        [ "$unresolved_handoff_attempts" -lt "$max_unresolved_handoff_attempts" ] || return 1
      fi
    else
      return 1
    fi
    sleep 0.01
  done

  scope="$(dependency_scope "$app_dir")" || {
    status=$?
    [ "$status" -eq 1 ] && status=2
    return "$status"
  }
[% if web_package_manager == "yarn" %]
  # Yarn configuration can select executable runtime and plugin paths. Prove
  # every inherited authority path before invoking any Yarn subprocess.
  validate_yarn_scope_authorities "$scope" || {
    status=$?
    [ "$status" -eq 1 ] && status=2
    return "$status"
  }
[% endif %]
[% if web_package_manager == "pnpm" %]
  # Once the worker owns the generation, remove stale proof before any
  # potentially slow runtime/config/full-contract queries. Readiness may still
  # perform the bounded manifest-authority query, but it will not probe the
  # installed pnpm runtime when no receipt exists.
  clear_dependency_state "$scope"
  pre_contract="$(pnpm_dependency_contract "$app_dir" "$scope")" || {
    status=$?
    [ "$status" -eq 1 ] && status=2
    return "$status"
  }
  if [ "$install_kind" = "frozen" ]; then
    lockfile="$(dependency_lockfile "$scope")" || return 1
    pre_lock_fingerprint="$(fingerprint_files "$lockfile")" || return 1
  fi
[% elif web_package_manager == "npm" %]
  clear_dependency_state "$scope"
  if [ "$install_kind" = "frozen" ]; then
    lockfile="$(dependency_lockfile "$scope")" || return 1
    pre_lock_fingerprint="$(fingerprint_files "$lockfile")" || return 1
    pre_input_fingerprint="$(dependency_fingerprint "$scope" "$lockfile")" || return 1
  fi
[% else %]
  clear_dependency_state "$scope"
[% endif %]
[% if web_package_manager == "npm" %]
  # Run npm through one no-shell launcher. It removes only inherited settings
  # that can turn success into a partial/no-op install while preserving
  # registry, authentication, peer-resolution, layout, and script policy.
  run_npm_dependency_install "$scope" "$install_kind" || install_status=$?
[% else %]
  case "$install_kind" in
    frozen)
      if [ "$scope" = "." ]; then
        <<[ web_install_command ]>> || install_status=$?
      else
[% if web_package_manager == "pnpm" %]
        (cd "$scope" && pnpm install --ignore-workspace --frozen-lockfile) || install_status=$?
[% else %]
        (cd "$scope" && <<[ web_install_command ]>>) || install_status=$?
[% endif %]
      fi
      ;;
    bootstrap)
      if [ "$scope" = "." ]; then
        <<[ web_package_manager ]>> install || install_status=$?
      else
[% if web_package_manager == "pnpm" %]
        (cd "$scope" && pnpm install --ignore-workspace) || install_status=$?
[% else %]
        (cd "$scope" && <<[ web_package_manager ]>> install) || install_status=$?
[% endif %]
      fi
      ;;
    *) install_status=2 ;;
  esac
[% endif %]
  if [ "$install_status" -eq 0 ]; then
[% if web_package_manager == "pnpm" %]
    post_scope="$(dependency_scope "$app_dir")" || install_status=$?
    if [ "$install_status" -eq 0 ] && [ "$post_scope" != "$scope" ]; then
      echo "pnpm dependency scope changed from '$scope' to '$post_scope' during installation; refusing to record stale artifacts." >&2
      install_status=1
    fi
    if [ "$install_status" -eq 0 ]; then
      if post_contract="$(pnpm_dependency_contract "$app_dir" "$scope")"; then
        :
      else
        install_status=$?
        [ "$install_status" -eq 1 ] && install_status=2
      fi
    fi
    if [ "$install_status" -eq 0 ] && [ "$install_kind" = "frozen" ]; then
      post_lock_fingerprint="$(fingerprint_files "$lockfile")" || install_status=$?
      if [ "$install_status" -eq 0 ] && [ "$post_lock_fingerprint" != "$pre_lock_fingerprint" ]; then
        echo "pnpm frozen install changed its selected lockfile; refusing to record stale artifacts." >&2
        install_status=1
      fi
    fi
    if [ "$install_status" -eq 0 ] && [ "$post_contract" != "$pre_contract" ]; then
      echo "pnpm runtime, configuration, manifest, or active patch inputs changed during installation; refusing to record stale artifacts." >&2
      install_status=1
    fi
    if [ "$install_status" -eq 0 ]; then
      record_dependency_state "$app_dir" "$scope" "$post_contract" || install_status=$?
    fi
[% elif web_package_manager == "npm" %]
    post_scope="$(dependency_scope "$app_dir")" || install_status=$?
    if [ "$install_status" -eq 0 ] && [ "$post_scope" != "$scope" ]; then
      echo "npm dependency scope changed from '$scope' to '$post_scope' during installation; refusing to record stale artifacts." >&2
      install_status=1
    fi
    if [ "$install_status" -eq 0 ] && [ "$install_kind" = "frozen" ]; then
      post_lockfile="$(dependency_lockfile "$scope")" || install_status=$?
      if [ "$install_status" -eq 0 ] && [ "$post_lockfile" != "$lockfile" ]; then
        echo "npm frozen install changed its selected lockfile from '$lockfile' to '$post_lockfile'; refusing to record stale artifacts." >&2
        install_status=1
      fi
      if [ "$install_status" -eq 0 ]; then
        post_lock_fingerprint="$(fingerprint_files "$post_lockfile")" || install_status=$?
      fi
      if [ "$install_status" -eq 0 ]; then
        post_input_fingerprint="$(dependency_fingerprint "$scope" "$post_lockfile")" || install_status=$?
      fi
      if [ "$install_status" -eq 0 ] && [ "$post_lock_fingerprint" != "$pre_lock_fingerprint" ]; then
        echo "npm frozen install changed its selected lockfile; refusing to record stale artifacts." >&2
        install_status=1
      fi
      if [ "$install_status" -eq 0 ] && [ "$post_input_fingerprint" != "$pre_input_fingerprint" ]; then
        echo "npm frozen install changed its manifest, configuration, or lockfile authority; refusing to record stale artifacts." >&2
        install_status=1
      fi
    fi
    if [ "$install_status" -eq 0 ]; then
      record_dependency_state "$app_dir" "$scope" || install_status=$?
    fi
[% else %]
    record_dependency_state "$app_dir" || install_status=$?
[% endif %]
  fi
  release_install_lock
  return "$install_status"
}

start_install_worker() {
  local app_dir="$1"
  local install_kind="$2"
  local metadata owner token coordinator_start worker_start worker_group bash_bin status=0

  metadata="$(install_lock_metadata)" || return 1
  read -r owner token coordinator_start <<< "$metadata"
  [ "$owner" = "$$" ] && [ "$token" = "$install_lock_token" ] || return 1
  bash_bin="${BASH:-bash}"
  install_worker_pid=""
  install_worker_group=""
  install_worker_signal=""
  # Install forwarding before spawning so a signal cannot land between lock
  # handoff and trap setup. A pre-spawn signal is remembered and delivered as
  # soon as the worker PID is available.
  trap 'forward_install_worker_signal HUP' HUP
  trap 'forward_install_worker_signal INT' INT
  trap 'forward_install_worker_signal TERM' TERM

  set -m
  JIG_WEB_INSTALL_WORKER_TOKEN="$token" \
    JIG_WEB_INSTALL_COORDINATOR_PID="$$" \
    JIG_WEB_INSTALL_COORDINATOR_START="$coordinator_start" \
    "$bash_bin" "$0" __dependency-install-worker "$app_dir" "$install_kind" &
  install_worker_pid=$!
  set +m
  if [ -n "$install_worker_signal" ]; then
    forward_install_worker_signal "$install_worker_signal"
  fi

  worker_start="$(process_start_identity "$install_worker_pid")" || {
    terminate_install_worker
    wait_for_install_worker_exit 2>/dev/null || true
    finish_install_worker 1 || return $?
    return
  }
  worker_group="$(process_group_id "$install_worker_pid")" || {
    terminate_install_worker
    wait_for_install_worker_exit 2>/dev/null || true
    finish_install_worker 1 || return $?
    return
  }
  if [ "$worker_group" != "$install_worker_pid" ]; then
    terminate_install_worker
    wait_for_install_worker_exit 2>/dev/null || true
    finish_install_worker 1 || return $?
    return
  fi
  install_worker_group="$worker_group"
  if [ -n "$install_worker_signal" ]; then
    terminate_install_worker
    wait_for_install_worker_exit 2>/dev/null || true
    finish_install_worker 1 || return $?
    return
  fi
  if ! transfer_install_lock_to_worker "$install_worker_pid" "$worker_start"; then
    terminate_install_worker
    wait_for_install_worker_exit 2>/dev/null || true
    finish_install_worker 1 || return $?
    return
  fi

  wait_for_install_worker_exit || status=$?
  finish_install_worker "$status"
}

run_dependency_install() {
  local app_dir="$1"
  local install_kind="$2"
  local readiness_status worker_status

  if dependencies_present "$app_dir"; then
    return
  else
    readiness_status=$?
  fi
  [ "$readiness_status" -eq 1 ] || return "$readiness_status"
  acquire_install_lock "$app_dir"
  if [ -z "$install_lock_token" ]; then
    return
  fi
  if dependencies_present "$app_dir"; then
    release_install_lock
    return
  else
    readiness_status=$?
  fi
  if [ "$readiness_status" -ne 1 ]; then
    release_install_lock
    return "$readiness_status"
  fi
  if start_install_worker "$app_dir" "$install_kind"; then
    return
  else
    worker_status=$?
    # The coordinator still owns the generation when handoff fails. Once a
    # handoff succeeds the worker owns cleanup and this call is a harmless no-op.
    release_install_lock
    return "$worker_status"
  fi
}

install_dependencies() {
  run_dependency_install "$1" frozen
}

bootstrap_dependencies() {
  run_dependency_install "$1" bootstrap
}

run_package_script() {
  local app_dir="$1"
  local script_name="$2"
[% if web_package_manager == "npm" %]
  run_npm_package_script "$app_dir" "$script_name"
[% else %]
[% if web_package_manager == "yarn" %]
  # Revalidate immediately before `yarn run`; dependency readiness performs
  # Yarn metadata queries earlier and configuration may have changed since.
  # Yarn runs from the app directory even when its install scope is the root.
  validate_yarn_scope_authorities "$app_dir" || return
[% endif %]

  (cd "$app_dir" && <<[ web_run_command ]>> "$script_name")
[% endif %]
}

configured_frontend_app() {
  case "$1" in
[% for app in frontend_apps %]
    "<<[ app.dir ]>>") return 0 ;;
[% endfor %]
    *)
      echo "Web app '$1' is not configured in [[frontend_apps]]." >&2
      return 2
      ;;
  esac
}

run_public_package_script() {
  local app_dir="$1"
  local script_name="$2"
  local status

  configured_frontend_app "$app_dir" || return
  dependency_scope "$app_dir" >/dev/null || {
    status=$?
    [ "$status" -eq 1 ] && status=2
    return "$status"
  }
  "$node_bin" scripts/check-webapp-scripts.mjs "$app_dir" "$script_name"
  run_package_script "$app_dir" "$script_name"
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
  __dependency-install-worker)
    if [ "$#" -ne 3 ]; then
      exit 2
    fi
    dependency_install_worker "$2" "$3"
    ;;
  dependencies-ready)
    if [ "$#" -ne 2 ]; then
      usage
      exit 2
    fi
    if dependencies_present "$2"; then
      exit 0
    else
      status=$?
    fi
    exit "$status"
    ;;
  dependencies-install)
    if [ "$#" -ne 2 ]; then
      usage
      exit 2
    fi
    install_dependencies "$2"
    ;;
  dependencies-bootstrap)
    if [ "$#" -ne 2 ]; then
      usage
      exit 2
    fi
    bootstrap_dependencies "$2"
    ;;
  run-script)
    if [ "$#" -ne 3 ]; then
      usage
      exit 2
    fi
    run_public_package_script "$2" "$3"
    ;;
  node-version-file)
    if [ "$#" -ne 2 ]; then
      usage
      exit 2
    fi
    node_version_file "$2"
    ;;
[% if web_package_manager == "pnpm" or web_package_manager == "yarn" %]
  package-manager-spec)
    if [ "$#" -ne 2 ]; then
      usage
      exit 2
    fi
[% if web_package_manager == "pnpm" %]
    pnpm_package_manager_spec "$2"
[% else %]
    yarn_package_manager_spec "$2"
[% endif %]
    ;;
[% endif %]
  bootstrap)
[% if frontend_apps | length > 0 %]
[% for app in frontend_apps %]
    bootstrap_dependencies "<<[ app.dir ]>>"
[% endfor %]
[% else %]
    echo "No web apps configured."
[% endif %]
    ;;
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
"## },
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
