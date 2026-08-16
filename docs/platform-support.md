# Platform Support

Jig supports running the CLI, generated harness, and machine-local runtime services on Linux and macOS.

| Host platform | Status | Continuous integration |
| --- | --- | --- |
| Linux | Supported | Rust-affecting pull requests run checks on `ubuntu-latest` |
| macOS | Supported | Rust-affecting pull requests run checks on `macos-latest` |
| Native Windows | Unsupported | No compatibility or CI guarantee |
| Android, BSD, and other targets | Unsupported | No compatibility or CI guarantee |

A failure that is reproducible on Linux or macOS is a supported-platform regression. Failures that occur only on an unsupported host do not block a Jig release.

## Feature-specific limits

Supported host does not mean that every platform-dependent capability is identical:

- Core CLI, init/adopt/update, generated repository commands, vault operation, and local development workflows are supported on Linux and macOS.
- Vault backup restore currently requires Linux's atomic absent-directory installation guarantee. Backup creation and the remaining vault workflow are supported on both supported hosts.
- Certificate trust, service installation, filesystem permissions, and process supervision use platform-specific implementations and prerequisites documented in [Configuration](configuration.md).
- Generated PostgreSQL browser E2E jobs use Linux because GitHub Actions service containers require it. During adoption, known `windows-*` runner labels are excluded from generated-check runner inference; Jig uses another detected non-Windows static runner or falls back to `ubuntu-latest`. Explicit custom or self-hosted runner choices remain project-owned.

## Windows and portability

Native Windows and Git Bash are not supported Jig hosts. WSL is not tested as a Windows integration environment; a workflow kept entirely inside its Linux userspace may follow the Linux path, but interoperability with Windows filesystems, shells, executables, and process management is outside the support contract.

The source tree still contains some Windows-specific implementations and tests from earlier compatibility work. Their presence, or a successful Windows compilation, does not make Windows a supported host. Those paths may change or be removed without compatibility guarantees.

Jig deliberately validates some paths and generated names for Windows portability. Those data-format checks protect repositories that may be consumed by other tools and operating systems; they do not expand Jig's host support policy.
