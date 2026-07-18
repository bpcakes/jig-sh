# Jig Answer File Examples

These TOML files are starting points for `jig init --answers-file` or
`jig adopt --answers-file`.

Every `examples/*.toml` file is smoke-rendered by fixture validation. The
fixture-backed examples `backend-only.toml`, `full-stack.toml`, and
`tooling-only.toml` also stay in lockstep with `tests/fixtures/*.toml`; release
and fixture checks verify matching contents so visible answer files and fixture
coverage do not drift.

- `rust-backend-only.toml`: Rust backend with no SQLx.
- `backend-only.toml`: Rust + SQLx with schema dump checks disabled.
- `rust-sqlx-schema-dump.toml`: Rust + SQLx + schema dump.
- `vite-frontend.toml`: Vite frontend checks and dev proxy config; it maps the generic Rust test slot to `npm test` for frontend-only smoke coverage.
- `full-stack.toml`: backend + frontend behind the generated dev proxy.
- `tooling-only.toml`: repo workflow harness with no app assumptions.
- `adopted-custom-commands.toml`: adopted existing repo with custom commands.

Copy an example, adjust repository names, paths, and commands, then run
`jig init` or `jig adopt` with `--answers-file`. For example, render only the
configured harness without unattended vault setup:

```sh
jig init /path/to/new-repo --preset harness-only \
  --answers-file examples/tooling-only.toml --no-input --no-vault
```
