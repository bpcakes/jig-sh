# Bootstrap guide

## Purpose

Own init, adoption, update, template rendering, and generated dependency installers. These rules also apply to the sibling [bootstrap.rs](../bootstrap.rs) entrypoint and the [project templates](../../../../templates/project).

## Key entrypoints

- [bootstrap.rs](../bootstrap.rs): CLI orchestration.
- [renderer.rs](renderer.rs): native template rendering.
- [tests](tests): adoption, rendering, installer, and ownership fixtures.

## Edit here for X

- Change destination publication and template identity in this module.
- Change generated installers and workflows in [project templates](../../../../templates/project), keeping [embedded snapshots](embedded_template_snapshots) aligned.
- See the [adoption reference](../../../../docs/adoption.md#frontend-dependency-installation) for consumer-facing installation behavior.

## Invariants

- Do not make template update flows switch source identity implicitly.
- Existing-destination init must budget retained generations before acquiring snapshots. Charge a possible preimage plus one generated version per planned leaf, count repeated publications explicitly, and include directory/staging identities plus transient headroom; apply the generation cap to unique leaves plus repeats without trusting a currently missing path to remain absent.
- Generated dependency installers must distinguish repository-owned package-manager policy from hostile ambient install shaping. A successful install may be stamped only after the selected scope, lock/config authority, real-write mode, complete workspace participation, platform, dependency classes, and executable-link behavior are pinned; preserve explicit registry/authentication and install-script approval policy. Keep the checker compatible with stock Bash 3.2, propagate authority-producer failures, and use shell-owned job identity after `wait` rather than recyclable PIDs.
- Generated npm package-script execution must select exactly the configured app, require the named script, and neutralize only ambient npm routing/dependency-class selectors. Preserve explicit application environment, registry/authentication, dependency layout, peer/lifecycle policy, and every user-authored development command. All generated web and E2E workflow package scripts must enter through the public checker boundary.
- Generated Rust/React source must be rustfmt-stable and pass its generated strict Clippy gate for every supported normalized package stem, database branch, and valid migration path. Validate the 216-byte Cargo artifact boundary before destination mutation, keep rendered identifiers behind fixed aliases, narrowly scope any lint acknowledgement required by intentional formatter-stability constructs, and keep long fallback API labels DNS-safe without changing short-name output.
- Classify each `node_modules` install root independently: missing, empty, and exact ignored-only real roots share the absent proof, while any unknown/type-replaced/nested entry makes the root present and fully attested. Preserve package metadata, links, member receipt-like files, launcher bytes/modes, and the v5/v3/v2 receipt formats.
- Rust/React scaffolds require Rust 1.94. Database-enabled variants pin SQLx 0.9 and use `.sqlx`; Doctor must enforce the active Rust floor and matching SQLx CLI minor line. PostgreSQL browser E2E owns its Linux service-container runner independently of the repository-wide runner; managed Rust workflow triggers and offline environments must follow configured migration and metadata authorities.

## Common commands

Run from the repository root:

- `cargo build -p jig-sh --bin jig`
- `JIG_DEV_BIN=target/debug/jig scripts/jig check source-frontend-test`
- `JIG_DEV_BIN=target/debug/jig scripts/jig check contract`
