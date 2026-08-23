#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)"
cd "$ROOT_DIR"

cargo fmt --all -- --check
rustfmt --edition 2024 --check \
  crates/jig/src/doctor_parts/*.rs \
  crates/jig/src/doctor/tests_parts/*.rs
