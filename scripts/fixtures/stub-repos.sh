#!/usr/bin/env bash

write_backend_stub_repo() {
  local repo_dir="$1"

  mkdir -p "$repo_dir/crates/demo/src"
  mkdir -p "$repo_dir/crates/acme-db/migrations"
  mkdir -p "$repo_dir/docs/schema"
  mkdir -p "$repo_dir/.sqlx"
  mkdir -p "$repo_dir/.agent/tmp/fixture-bin"

  # This controlled executable verifies that the generated default SQLx command
  # reaches the expected binary boundary. It is not a substitute for the real
  # SQLx integration exercised by fresh-scaffold QA.
  cat > "$repo_dir/.agent/tmp/fixture-bin/sqlx" <<'EOF'
#!/bin/sh
set -eu

fail() {
  printf '%s\n' "fixture sqlx invocation mismatch: $*" >&2
  exit 64
}

[ "${CARGO:-}" = "cargo" ] || fail "CARGO=${CARGO-<unset>}"
[ "${SQLX_OFFLINE:-}" = "false" ] || fail "SQLX_OFFLINE=${SQLX_OFFLINE-<unset>}"
[ "${SQLX_OFFLINE_DIR:-}" = ".sqlx" ] || fail "SQLX_OFFLINE_DIR=${SQLX_OFFLINE_DIR-<unset>}"
[ "$#" -eq 6 ] || fail "argc=$# argv=$*"
[ "$1" = "prepare" ] || fail "argv=$*"
[ "$2" = "--check" ] || fail "argv=$*"
[ "$3" = "--workspace" ] || fail "argv=$*"
[ "$4" = "--" ] || fail "argv=$*"
[ "$5" = "--workspace" ] || fail "argv=$*"
[ "$6" = "--all-targets" ] || fail "argv=$*"
: "${JIG_FIXTURE_SQLX_MARKER:?missing fixture SQLx marker path}"

{
  printf '%s\n' 'CARGO=cargo'
  printf '%s\n' 'SQLX_OFFLINE=false'
  printf '%s\n' 'SQLX_OFFLINE_DIR=.sqlx'
  printf '%s\n' 'argv=prepare --check --workspace -- --workspace --all-targets'
} > "$JIG_FIXTURE_SQLX_MARKER"
EOF
  chmod +x "$repo_dir/.agent/tmp/fixture-bin/sqlx"

  cat > "$repo_dir/.gitignore" <<'EOF'
/target/
/.agent/tmp/
node_modules/
coverage/
*/node_modules/
*/coverage/
EOF

  cat > "$repo_dir/Cargo.toml" <<'EOF'
[workspace]
members = ["crates/demo"]
resolver = "2"
EOF

  cat > "$repo_dir/crates/demo/Cargo.toml" <<'EOF'
[package]
name = "demo"
version = "0.1.0"
edition = "2024"
EOF

  cat > "$repo_dir/crates/demo/src/lib.rs" <<'EOF'
pub fn meaning() -> i32 {
    42
}
EOF

  cat > "$repo_dir/crates/demo/AGENTS.md" <<'EOF'
# demo agent guide

## Purpose
Demo crate used to validate the extracted agent guide checks.

## Key entrypoints
- `src/lib.rs`: demo entrypoint.

## Edit here for X
- Change the demo function: `src/lib.rs`.

## Invariants
- Keep this crate small and deterministic.

## Common commands
- `cargo check -p demo`
EOF

  cat > "$repo_dir/crates/acme-db/AGENTS.md" <<'EOF'
# acme-db agent guide

## Purpose
Demo persistence crate used to validate crate-guide coverage.

## Key entrypoints
- `src/lib.rs`: demo DB entrypoint.

## Edit here for X
- Change DB helpers: `src/lib.rs`.

## Invariants
- Keep migration history forward-only.

## Common commands
- `cargo check -p acme-db`
EOF

  mkdir -p "$repo_dir/crates/acme-db/src"
  cat > "$repo_dir/crates/acme-db/src/lib.rs" <<'EOF'
pub fn marker() -> &'static str {
    "db"
}
EOF

  cat > "$repo_dir/crates/acme-db/migrations/20260405120000_init.up.sql" <<'EOF'
CREATE TABLE demo_items (id integer PRIMARY KEY);
EOF

  cat > "$repo_dir/crates/acme-db/migrations/20260405120000_init.down.sql" <<'EOF'
DROP TABLE demo_items;
EOF
}

write_full_stack_stub_repo() {
  local repo_dir="$1"

  write_backend_stub_repo "$repo_dir"

  mkdir -p "$repo_dir/frontend" "$repo_dir/admin-panel"
  cat > "$repo_dir/docs/schema/tables.sql" <<'EOF'
CREATE TABLE demo_items (id integer PRIMARY KEY);
EOF
  cat > "$repo_dir/scripts/dump-schema.sh" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
mkdir -p docs/schema
cat > docs/schema/tables.sql <<'SQL'
CREATE TABLE demo_items (id integer PRIMARY KEY);
SQL
EOF
  chmod +x "$repo_dir/scripts/dump-schema.sh"

  cat > "$repo_dir/package.json" <<'EOF'
{"name":"fixture-web-root","version":"0.0.0","private":true}
EOF
  cat > "$repo_dir/package-lock.json" <<'EOF'
{
  "name": "fixture-web-root",
  "lockfileVersion": 3,
  "requires": true,
  "packages": {
    "": {
      "name": "fixture-web-root",
      "version": "0.0.0"
    }
  }
}
EOF

  for app_dir in frontend admin-panel; do
    cat > "$repo_dir/$app_dir/package.json" <<EOF
{
  "name": "$app_dir",
  "private": true,
  "scripts": {
    "lint": "node -e \"process.exit(0)\"",
    "typecheck": "node -e \"process.exit(0)\"",
    "build:bundle": "node -e \"process.exit(0)\"",
    "test:coverage": "node write-coverage.mjs",
    "dev": "node -e \"setInterval(() => {}, 1000)\""
  }
}
EOF
    cat > "$repo_dir/$app_dir/package-lock.json" <<EOF
{
  "name": "$app_dir",
  "lockfileVersion": 3,
  "requires": true,
  "packages": {
    "": {
      "name": "$app_dir"
    }
  }
}
EOF
    cat > "$repo_dir/$app_dir/write-coverage.mjs" <<'EOF'
import fs from "node:fs";

const summary = {
  total: {
    lines: { pct: 100 },
    functions: { pct: 100 },
    statements: { pct: 100 },
    branches: { pct: 100 },
  },
};

fs.mkdirSync("coverage", { recursive: true });
fs.writeFileSync("coverage/coverage-summary.json", JSON.stringify(summary));
EOF
  done
}

write_tooling_only_stub_repo() {
  local repo_dir="$1"

  mkdir -p "$repo_dir/crates/demo/src"

  cat > "$repo_dir/.gitignore" <<'EOF'
/target/
node_modules/
coverage/
*/node_modules/
*/coverage/
EOF

  cat > "$repo_dir/Cargo.toml" <<'EOF'
[workspace]
members = ["crates/demo"]
resolver = "2"
EOF

  cat > "$repo_dir/crates/demo/Cargo.toml" <<'EOF'
[package]
name = "demo"
version = "0.1.0"
edition = "2024"
EOF

  cat > "$repo_dir/crates/demo/src/lib.rs" <<'EOF'
pub fn meaning() -> i32 {
    42
}
EOF

  cat > "$repo_dir/crates/demo/AGENTS.md" <<'EOF'
# demo agent guide

## Purpose
Demo crate used to validate the extracted agent guide checks.

## Key entrypoints
- `src/lib.rs`: demo entrypoint.

## Edit here for X
- Change the demo function: `src/lib.rs`.

## Invariants
- Keep this crate small and deterministic.

## Common commands
- `cargo check -p demo`
EOF
}
