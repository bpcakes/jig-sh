Upgrade the Batter scaffold from f1cafe9abeb9c08960523288c2c2c25da5e18202 to
39c1b6e3c1c75f808becb5a5e7c33f58001a2ee4.

Upstream inspection: fetched https://github.com/bpcakes/batter and compared the
four new commits, CHANGELOG.md, docs/usage.md, lifecycle/startup/cleanup changes,
and the new command API and report implementation. HTTP adapters and owned Startup
remain compatible. Managed native settlement is new but this scaffold has no native
worker runtime to adapt. PanicPayload::try_inspect replaces blocking inspection;
the scaffold does not inspect panic payloads. Shutdown phases now share a first-stop
clock, with independently budgeted cleanup.

Implemented the new finite Command/check_command path for database bootstrap in
the runtime crate, including signal cancellation followed by awaited finalization.
Removed the unowned application-state bootstrap helper. Retained database creation
policy, migration errors, boolean created/existing result, and nonzero failure exit.
Application state remains independent of Batter. Updated the pinned revision,
generated guidance, ownership assertions, and embedded snapshots.

Validation so far:

- Scaffold tests: 117 passed, one pre-existing registry-dependent test ignored.
  Includes formatting across package names, database branches and migration paths.
- Generated no-database, SQLite and PostgreSQL workspaces: cargo fmt --all --check,
  cargo test --workspace --all-features and strict all-target/all-feature Clippy pass.
- Live SQLite: bootstrap creates then reuses a database; successful finalization is
  observed. Corrupting only the disposable fixture migration table gives exit 1,
  retained database.migrate failure and database.close outcome=Succeeded, with zero
  unsuccessful/skipped cleanup hooks. Ready service exits 0 on SIGTERM.
- Live stalled PostgreSQL protocol handshake: both normal startup and finite
  bootstrap exit 1 within five seconds on each of SIGINT and SIGTERM.
- A disposable PostgreSQL 18 container also passed finite bootstrap with successful
  pool cleanup, real service readiness and SIGTERM exit 0. The container was removed.
- PostgreSQL runtime also compiles with --no-default-features (database disabled).
- Full repository test gate passed: 4,068 tests across 47 binaries, three skipped.
  Repository strict Clippy also passed. An accidentally concurrent work check was
  interrupted to avoid duplicating that suite; its interrupted receipts are retained.
  Final gate inspection reuses the completed test and Clippy receipts.

Completion: work check passed with all five required native targets current
(test, Clippy, formatting, contract and file budget). Work evidence reports fresh
inputs and no unresolved gates. Validation receipt:
receipt_01M2AX2T5EFXF2V8C5PXDGMNP6. Changes remain uncommitted.

Validation corrections: the empty scaffold intentionally has no application
migrations, so live checks assert creation of SQLx's migration table rather than
nonzero applied migration rows. Batter cleanup telemetry reports Succeeded rather
than a lowercase ok string. Corrected those smoke-test assumptions, not production
behavior. The first generated formatting check found two line-wrap differences;
the templates were corrected and the full formatting matrix subsequently passed.
