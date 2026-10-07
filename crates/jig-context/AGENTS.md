# jig-context crate guide

## Purpose

`crates/jig-context` owns the loaded repository context: `RepoContext`, built from `.jig.toml` and `.agent/jig-contract.json`, the validation of both, and the repository root and runtime-cache locations derived from them. It also holds the repository-path, backend, and frontend metadata helpers that configuration parsing depends on.

## Key entrypoints

- `src/lib.rs`: `RepoContext`, `RepoConfig`, the contract-version constants, and module wiring.
- `src/loading.rs`, `src/repository_root.rs`, `src/optional.rs`: loading a context from a root, from discovery, or optionally.
- `src/validation.rs`: `.jig.toml` and manifest validation.
- `src/inputs_policy.rs`: contract-epoch rules for action `inputs_policy` and `source_state`.
- `src/runtime.rs`: supported contract versions, runtime-cache locations, and the launcher-validated context slot.
- `src/repository_path.rs`, `src/backend.rs`, `src/frontend_metadata.rs`, `src/strict_json.rs`: shared parsing helpers.
- `src/test_support.rs`: repository fixtures and environment guards for tests, behind the `test-support` feature.

## Edit here for X

- Add or change a `.jig.toml` setting: the config struct in `src/lib.rs` or its submodule (`execution_config.rs`, `loop_config.rs`, `vault_config.rs`, `work_config.rs`), then its rule in `src/validation.rs`.
- Change supported contract versions: `src/lib.rs` and `src/runtime.rs`. `scripts/check-launcher-template.sh` reads these constants from source, so keep them as single-line `pub const` items.
- Change which vault scope ids are valid: `is_valid_vault_scope_id` in `src/vault_config.rs`.
- Change dev-proxy TLD validation: `src/validation.rs`. With the `dev-proxy` feature it delegates to `jig-dev-proxy`; keep the no-feature fallback aligned.

## Invariants

- Keep this crate independent from CLI, runtime execution, state, bootstrap, and vault secret handling; dependents pass in what they need.
- Preserve generated-repo compatibility for `.jig.toml` and `.agent/jig-contract.json`.
- Production builds keep the launcher-validated context in a set-once `OnceLock`. The resettable slot and `RepoContext::load_from` exist only under `cfg(test)` or the `test-support` feature, which dependents enable from `[dev-dependencies]` only.

## Common commands

- `cargo test -p jig-context`
- `cargo clippy -p jig-context --all-targets --features dev-proxy -- -D warnings`
- `scripts/check-launcher-template.sh`
- `cargo test -p jig-sh`
