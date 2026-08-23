# Platform Support

Jig supports running the CLI, generated harness, and machine-local runtime services on Linux and macOS.

| Host platform | Status | Continuous integration |
| --- | --- | --- |
| Linux | Supported | Rust-affecting pull requests run checks on `ubuntu-latest` |
| macOS | Supported | Rust-affecting pull requests run checks on `macos-latest` |

A failure that is reproducible on Linux or macOS is a supported-platform regression. Other hosts are outside Jig's compatibility contract and do not block a release.

## Feature-specific limits

Supported host does not mean that every platform-dependent capability is identical:

- Core CLI, init/adopt/update, generated repository commands, vault operation, and local development workflows are supported on Linux and macOS.
- Vault backup restore currently requires Linux's atomic absent-directory installation guarantee. Backup creation and the remaining vault workflow are supported on both supported hosts.
- Certificate trust, service installation, filesystem permissions, and process supervision use host-specific implementations and prerequisites documented in [Configuration](configuration.md).
- Generated PostgreSQL browser E2E jobs use Linux because GitHub Actions service containers require it. During adoption, Jig selects a statically detected Linux or macOS runner, or falls back to `ubuntu-latest`; explicit custom or self-hosted runner choices remain project-owned.
