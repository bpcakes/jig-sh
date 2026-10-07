# Runtime process supervision

Read this reference before changing [signal sessions](../crates/jig/src/signal_supervision/session.rs), [shared signal supervision](../crates/jig/src/signal_supervision.rs), or [Jig-owned Bash probes](../crates/jig-owned-process/src/bash_environment.rs). Generic process-tree execution belongs to [jig-owned-process](../crates/jig-owned-process/AGENTS.md).

## Signal sessions

Unix signal sessions, shared by doctor probes and every supervised command, are serialized and reusable only after clean retirement: hold the session guard through handler restoration and restored-signal redelivery, and publish permanent poison before snapshotting signals on an unsafe retirement.

## Bash probe environment

Jig-owned Bash probes such as dependency readiness, Codex capability checks, and launcher-backed doctor diagnostics must remove startup, directory, option, trace, and byte-exact exported-function controls. Do not apply that constrained environment to agent bootstrap, committed checks, or configured development commands, which intentionally inherit the caller's ordinary environment. That ordinary environment never contains `JIG_VAULT_PASSPHRASE` or `JIG_VAULT_NEW_PASSPHRASE`: every non-vault command removes both at CLI startup (see [passphrase withholding](configuration.md#passphrase-withholding)).
