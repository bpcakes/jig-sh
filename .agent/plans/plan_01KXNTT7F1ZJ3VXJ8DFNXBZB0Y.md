# Make fresh scaffold bootstrap sufficient before `jig dev`

After this change, a freshly initialized Rust + React project has one explicit setup path: copy `.env.example` to `.env`, run `scripts/jig bootstrap`, then run `scripts/jig dev`. Development startup never installs packages implicitly. Missing frontend dependencies stop before any app launches, and database-backed scaffold bootstrap creates the configured database and applies migrations.

## Progress

- [x] Reproduce the missing Postgres database and missing Vite dependency sequence from the reported `hocr2` repository.
- [x] Add a read-only frontend dependency preflight before dev app startup.
- [x] Add idempotent SQLx database creation and migration to generated scaffold bootstrap.
- [x] Refresh embedded templates and add focused regression tests.
- [x] Render and compile fresh SQLite and Postgres scaffold variants.
- [x] Run repository gates and finish structured work evidence.

## Surprises & Discoveries

- The generated init summary already listed bootstrap before dev, but dev launched backend processes before detecting that frontend bootstrap had been skipped.
- The existing bootstrap command installed frontend dependencies but did not provision a selected Postgres database, so following the documented bootstrap step alone was not sufficient.
- SQLx's `MigrateDatabase` implementation supports both generated Postgres and SQLite backends, avoiding shell URL parsing and external `createdb` assumptions.

## Decision Log

- Keep package installation explicit in `scripts/jig bootstrap`; `jig dev` performs only filesystem preflight checks.
- Put database provisioning behind the generated API's private `--bootstrap-database` startup mode so it loads the same `.env` and uses the same typed config as normal startup.
- Require `.env` before Cargo fetch for database-backed preset bootstrap so missing configuration fails immediately.

## Outcomes & Retrospective

Fresh dev now fails before launching any app when selected frontend dependencies are absent and names `scripts/jig bootstrap` as the recovery. Generated database bootstrap validates `.env`, creates Postgres or SQLite through SQLx, applies migrations, then installs the root frontend workspace. A clean generated SQLite project completed the full bootstrap and launched both API and Vite apps; repeated SQLite database bootstrap was idempotent. A clean generated Postgres workspace compiled successfully. The complete `jig-sh` suite passed 817 tests, and configured contract, test, fmt, and clippy checks passed. The temporary Git fixture also now disables local-clone hardlinks, fixing a repeatable macOS bare-clone failure uncovered by the full suite.

## Context and Plan

The runtime entrypoint is `crates/jig/src/dev_proxy.rs`. Scaffold answer defaults are composed in `crates/jig/src/bootstrap/scaffold.rs` and rendered from `templates/scaffolds/rust-react/workspace`. Update those paths, refresh both embedded template snapshots with `JIG_REFRESH_EMBEDDED_TEMPLATE_SNAPSHOT=1 cargo check -p jig-sh`, then validate focused unit tests, a generated SQLite bootstrap run, a generated Postgres compile, and the configured Jig gates.

All operations are idempotent: frontend preflight is read-only, SQLx database creation skips an existing database, migrations are SQLx-managed, and snapshot refresh can be rerun after any template change.
