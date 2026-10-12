# jig-bootstrap crate guide

## Purpose

`crates/jig-bootstrap` owns init, adoption, update, template rendering, the embedded template snapshots, and generated dependency installers. These rules also apply to the [project templates](../../templates/project) and [scaffold templates](../../templates/scaffolds). The `jig` CLI parses these commands and renders their results; `crates/jig/build.rs` decides the official template pin, which `jig::run` records with `record_build_template_pin_policy`.

## Key entrypoints

- [src/lib.rs](src/lib.rs): init, adopt, and update entry points.
- [build.rs](build.rs): generates the embedded template tables from the live `templates/` tree in a source checkout, or from the checked-in snapshots when packaged.
- [src/renderer.rs](src/renderer.rs): native template rendering.
- [src/tests](src/tests): adoption, rendering, installer, and ownership fixtures.

## Edit here for X

- Change destination publication and template identity in this module.
- Change generated installers and workflows in [project templates](../../templates/project), then refresh the [embedded snapshots](src/embedded_template_snapshots) with `JIG_REFRESH_EMBEDDED_TEMPLATE_SNAPSHOT=1 cargo check -p jig-bootstrap`.
- See the [adoption reference](../../docs/adoption.md#frontend-dependency-installation) for consumer-facing installation behavior.

## Invariants

- Do not make template update flows switch source identity implicitly.
- Generated root guidance keeps the operator-owned Vault section and the Start Here vault-setup exception in the managed block, gated on the repo-scoped `[vault]` answers, with no local links or angle-bracket placeholders. No check compares this repository's root `AGENTS.md` with the template, so mirror managed-block changes there by hand. The managed `.gitignore` keeps `.env.*`, including `.env.jig` refs files, ignored because `vault exec` env files may contain literal values.
- Existing-destination init must budget retained generations before acquiring snapshots. Charge a possible preimage plus one generated version per planned leaf, count repeated publications explicitly, and include directory/staging identities plus transient headroom; apply the generation cap to unique leaves plus repeats without trusting a currently missing path to remain absent.
- Generated dependency installers must distinguish repository-owned package-manager policy from hostile ambient install shaping. A successful install may be stamped only after the selected scope, lock/config authority, real-write mode, complete workspace participation, platform, dependency classes, and executable-link behavior are pinned; preserve explicit registry/authentication and install-script approval policy. Keep the checker compatible with stock Bash 3.2, propagate authority-producer failures, and use shell-owned job identity after `wait` rather than recyclable PIDs.
- Generated npm package-script execution must select exactly the configured app, require the named script, and neutralize only ambient npm routing/dependency-class selectors. Preserve explicit application environment, registry/authentication, dependency layout, peer/lifecycle policy, and every user-authored development command. All generated web and E2E workflow package scripts must enter through the public checker boundary.
- Generated Rust/React source must be rustfmt-stable and pass its generated strict Clippy gate for every supported normalized package stem, database branch, and valid migration path. Validate the 216-byte Cargo artifact boundary before destination mutation, keep rendered identifiers behind fixed aliases, narrowly scope any lint acknowledgement required by intentional formatter-stability constructs, and keep long fallback API labels DNS-safe without changing short-name output.
- Classify each `node_modules` install root independently: missing, empty, and exact ignored-only real roots share the absent proof, while any unknown/type-replaced/nested entry makes the root present and fully attested. Preserve package metadata, links, member receipt-like files, launcher bytes/modes, and the v5/v3/v2 receipt formats.
- Rust/React scaffolds require Rust 1.94. Database-enabled variants pin SQLx 0.9 and use `.sqlx`; Doctor must enforce the active Rust floor and matching SQLx CLI minor line. PostgreSQL browser E2E owns its Linux service-container runner independently of the repository-wide runner; managed Rust workflow triggers and offline environments must follow configured migration and metadata authorities.

## Common commands

Run from the repository root:

- `cargo test -p jig-bootstrap`
- `scripts/jig-dev check source-frontend-test`
- `scripts/jig-dev check contract`
