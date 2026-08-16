# jig-dev-proxy

`jig-dev-proxy` is an internal support crate for the matching `jig-sh` CLI
release. It is published because crates.io requires published path dependencies
for `jig-sh`, not because it is intended as a stable third-party library.

Use the `jig-sh` CLI as the public interface. This crate's Rust API and JSON
command envelopes may change between matching `jig-sh` releases.

Development app commands are trusted repo-configured commands. They intentionally
inherit the caller environment so package managers, local credentials, and dev
tooling keep working. Jig-owned `JIG_DEV_<APP>_{HOST,PORT,ORIGIN,URL}` coordinates
describe only the current app selection, so inherited copies are removed before
current coordinates are injected. The long-running background proxy process is different:
it starts with a constrained environment and should not be used to run arbitrary
repo commands.

Jig supports this runtime on Linux and macOS. Windows-specific implementation
paths remain from earlier compatibility work, but native Windows is not a
supported or CI-tested host and those paths may change without compatibility
guarantees. See the repository's [platform support policy](../../docs/platform-support.md).

Foreground route cleanup keeps exact route ownership and shares one absolute
lock deadline across all children, retries, and the final Drop fallback.
