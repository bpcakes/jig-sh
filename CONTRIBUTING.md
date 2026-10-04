# Contributing to jig.sh

## Local development

Choose checks that validate the affected behavior. Broaden verification for shared
behavior, failures, or unresolved risks; ordinary development does not require receipt
inspection or a full workspace test run. `scripts/jig file-budget audit`
provides standalone source-size diagnostics without creating runs or receipts.

The configured full Rust test commands use cargo-nextest 0.9.130 or newer so tests
that mutate process-global environment or working-directory state run in
separate processes. Install that prerequisite using the
[official cargo-nextest installation instructions](https://nexte.st/docs/installation/pre-built-binaries/),
confirm it with `cargo nextest --version`, then run `scripts/jig doctor` for the
remaining repository prerequisites. Focused `cargo test -p <package>` commands
remain supported for crate development.

Release `jig init` and `jig adopt` builds use the official remote template at the `vVERSION` tag for the running binary. Unreleased local builds use the templates embedded in the binary when `--template` is omitted. Repos rendered from embedded templates record `_src_path = "embedded:jig-sh"`; generated launchers reuse managed cached binaries that support the repository contract and requested profile. Reuse of a compatible binary found on `PATH` requires `JIG_INSTALL_ALLOW_PATH_BINARY=1` and prints the selected absolute path. Embedded renders require `JIG_INSTALL_ALLOW_EMBEDDED_SOURCE_FALLBACK=1` before installing from the configured source's current default branch because an embedded render has no immutable source revision. When you need checkout-driven template metadata during development, pass `--template /path/to/jig-sh --template-mode committed`, or pass `--vcs-ref main` to use the current official branch.

When editing `templates/project`, refresh the checked-in embedded-template snapshot before committing:

```sh
JIG_REFRESH_EMBEDDED_TEMPLATE_SNAPSHOT=1 cargo check -p jig-sh
```

During a release, the remote `vVERSION` tag is pushed after the crates publish step succeeds. If you install a freshly published binary before the tag is visible on GitHub, use `--vcs-ref main` or a local `--template` path for the first render, then retry the default release template after the tag is pushed.

## Release

Use the GitHub Actions `Release` workflow for the lowest-touch release path. The default branch carries the next patch as a `MAJOR.MINOR.PATCH-dev` workspace version. Leave `version` blank to release that patch version, choose a larger bump, or set the release version explicitly. The workflow prepares the release commit, updates `CHANGELOG.md`, creates a local tag, publishes the workspace crates in dependency order to crates.io through trusted publishing, pushes the tag to origin after every crate publishes, creates the GitHub Release, then advances the default branch to the following patch `-dev` version.

Keep in-progress release notes under `## Unreleased`. `scripts/release.sh prepare` promotes that curated section to `## vVERSION` when it contains `###` headings; otherwise it generates notes from git history. Conventional commit prefixes (`feat:`, `fix:`, `docs:`, `test:`, `refactor:`, `perf:`, `build:`, `ci:`, `chore:`) drive the generated categories; unprefixed commits land in `Other`. Do not hand-edit an upcoming `## vVERSION` section before running the workflow.

### Release binaries

After the main release job succeeds, `Release binaries` builds the exact tag on
Linux and macOS for x86-64 and ARM64. Linux builds use Ubuntu 22.04 (glibc 2.35);
macOS builds target macOS 13. The full-featured executables are packaged as
`jig-VERSION-TARGET.tar.gz`, containing only `jig`, with one `.tar.gz.sha256`
sidecar per archive. Native version/contract/profile checks and a cold installer
smoke test with no Cargo on PATH run before uploading. All four builds must
succeed before any assets are attached to the existing GitHub Release.

The binary workflow can also be dispatched for an existing stable `version` to
backfill assets or retry failed builds. It defaults to `dry_run: true`, which
keeps build artifacts for inspection without publishing. Set `dry_run: false`
to attach them. Compatibility probes use the contract from the selected release's
source checkout. The original `v0.1.0` predates repository compatibility probes,
so its smoke test covers standalone installation only; newer releases must also
pass repository installation and both cached profile checks.
Existing complete asset pairs are downloaded and checksum-verified,
then preserved; an incomplete pair fails with repair guidance. Published assets
are never overwritten automatically. The main Release workflow's dry run still
performs its existing local validation; use the separate binary dry run against
an existing tag to validate the platform matrix.

First-time users run the standalone `scripts/install.sh` from the README. It
resolves the latest GitHub Release by default, accepts `--version` and `--bin-dir`,
and atomically installs a verified native executable without Cargo. It deliberately
fails if binary assets are unavailable. The generated repository installer retains
its source fallback for compatibility with older release pins.

Consumers need the updated `scripts/install-jig.sh` and an explicit
`.jig/runtime-version` pin to a release containing assets. Keep template/source
selection unchanged for unpinned checkouts. Use `JIG_INSTALL_SOURCE=1` for an
explicit source build. Installer download or integrity errors fail visibly;
only an unavailable archive or unsupported host automatically falls back to Cargo.

### Local release steps

The local release script is the typed entrypoint for validation and manual recovery. The `github` subcommand requires the GitHub CLI (`gh`) with permission to create releases.

```sh
scripts/release.sh prepare 0.1.1
ALLOW_DIRTY=1 scripts/release.sh check 0.1.1
scripts/release.sh stage
git commit -m "Release v0.1.1"
scripts/release.sh check 0.1.1
scripts/release.sh tag 0.1.1
scripts/release.sh publish 0.1.1
scripts/release.sh github 0.1.1
scripts/release.sh prepare-development 0.1.2-dev
```

- `prepare` — updates workspace package versions and regenerates `CHANGELOG.md`
- `check` — requires a clean worktree, verifies version wiring and changelog coverage, runs the direct `scripts/jig` CI checks, validates rendered fixtures, and runs crates.io publish dry runs
- `tag` — creates the annotated local `vVERSION` tag after the same checks
- `publish` — requires the tag to point at `HEAD`, publishes every workspace crate in `scripts/release.sh` order, then pushes the tag to origin
- `github` — creates the GitHub Release from the matching `CHANGELOG.md` section
- `prepare-development` — advances workspace packages to the next `-dev` version without changing release notes

### crates.io trusted publishing setup

Before the first split-crate release, pre-create crates.io Trusted Publishing configuration for every publishable workspace package listed in `scripts/release.sh`, repository `bpcakes/jig-sh`, workflow `release.yml`, and environment `crates-io`. Protect that GitHub environment with required reviewers.

`publish` skips package versions already present on crates.io and pushes the remote tag only after every crate is published. If only part of the crate set was published, keep the same version for remaining packages; bump only when a published crate version itself must change, since crates.io versions cannot be overwritten after yank.

If a workflow run pushes the release commit but fails before the tag is pushed, rerun the workflow with the explicit prepared version instead of leaving `version` blank.

For the already-published `v0.1.0`, run the workflow with `backfill_v0_1_0=true` to create the missing GitHub Release without publishing or retagging.
