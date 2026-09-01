# Fixture Answer Files

Keep these TOML fixtures in lockstep with their matching fixture-backed
`examples/*.toml` files; release and fixture checks verify matching contents so
fixture coverage and visible answer files do not drift. Additional freestanding
examples are smoke-rendered by `scripts/validate-fixtures.sh`.

## Behavioral Source Fixtures

`cognitive-complexity-over-threshold.rs` deliberately exceeds the cognitive
complexity threshold generated for Rust workspaces. The integration check in
`scripts/check-generated-rust-clippy.sh` injects it into generated Rust-only
and Rust/React members to prove that the effective Clippy policy, not just the
rendered configuration text, rejects a score above 20.

Rust-only scaffold checks stay offline because those starters have no registry
dependencies. Rust/React checks intentionally resolve current compatible
dependencies because a new `jig init` does not create `Cargo.lock`; this keeps
release and CI validation aligned with what a newly initialized app resolves.
