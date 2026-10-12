#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)"
cd "$ROOT_DIR"

cargo fmt --all -- --check
# Sources spliced in with include! escape cargo fmt, so keep every file a module.
python3 scripts/check-rust-module-splits.py
