use std::path::PathBuf;

use anyhow::Result;
use serde_json::json;

use super::super::super::answers::{web_install_command, web_run_command};
use super::super::super::{
    GENERATED_NODE_VERSION, generated_package_manager_spec, generated_package_manager_version,
};
use super::super::templates::{ensure_scaffold_template_paths, render_scaffold_template};
use super::super::write::{ScaffoldFile, scaffold_file};
use super::super::{ScaffoldDb, ScaffoldFrontendKind, ScaffoldPreset, optional_cargo_command};
use super::app::{FrontendBackendContext, FrontendDatabaseContext, FrontendScaffold};
use super::templates::frontend_workspace_template_files_for_backend;

pub(in crate::bootstrap::scaffold) const DATABASE_CONFIG_GUARD: &str = r#"if [ -z "${DATABASE_URL:-}" ] && ! awk '/^[[:space:]]*(#|$)/ { next } /^[[:space:]]*(export[[:space:]]+)?DATABASE_URL[[:space:]]*=/ { value = $0; sub(/^[^=]*=[[:space:]]*/, "", value); sub(/^#.*$/, "", value); sub(/[[:space:]]+#.*$/, "", value); gsub(/^[[:space:]]+|[[:space:]]+$/, "", value); single_quote = sprintf("%c", 39); if (value != "" && value != "\"\"" && value != single_quote single_quote) found = 1 } END { exit found ? 0 : 1 }' .env 2>/dev/null; then printf '%s\n' 'Missing DATABASE_URL; export it or copy .env.example to .env before bootstrap.' >&2; exit 1; fi"#;

pub(in crate::bootstrap::scaffold) fn scaffold_bootstrap_command(
    package_name: &str,
    db: ScaffoldDb,
    frontends: &[FrontendScaffold],
) -> String {
    let mut commands = Vec::new();
    commands.push(optional_cargo_command("cargo fetch", "bootstrap"));
    if !frontends.is_empty() {
        commands.push("scripts/check-webapps.sh bootstrap".into());
    }
    if db != ScaffoldDb::None {
        commands.push(DATABASE_CONFIG_GUARD.into());
        commands.push(format!(
            "cargo run -p {package_name}-api -- --bootstrap-database"
        ));
    }
    commands.join(" && ")
}

pub(in crate::bootstrap::scaffold) fn render_frontend_workspace_files_for_backend(
    backend: FrontendBackendContext<'_>,
    package_manager: &str,
    package_name: &str,
    default_branch: &str,
    ci_github_runner: &str,
    frontends: &[FrontendScaffold],
) -> Result<Vec<ScaffoldFile>> {
    let FrontendBackendContext {
        preset,
        root: backend_root,
        database,
    } = backend;
    let FrontendDatabaseContext {
        db,
        migration_dir,
        sqlx_metadata_dir,
    } = database;
    let template_files =
        frontend_workspace_template_files_for_backend(preset, package_manager, frontends);
    ensure_scaffold_template_paths(&template_files)?;
    if template_files.is_empty() {
        return Ok(Vec::new());
    }
    let default_branch_yaml = serde_json::to_string(default_branch)?;
    let admin_api_enabled = frontends
        .iter()
        .any(|frontend| frontend.kind == ScaffoldFrontendKind::Admin);
    let context = json!({
        "package_name": package_name,
        "backend_language": if preset == ScaffoldPreset::GoReact { "go" } else { "rust" },
        "go_backend_root": backend_root,
        "package_manager": package_manager,
        "package_manager_spec": generated_package_manager_spec(package_manager),
        "package_manager_version": generated_package_manager_version(package_manager),
        "node_version": GENERATED_NODE_VERSION,
        "web_install_command": web_install_command(package_manager),
        "web_run_command": web_run_command(package_manager),
        "db": match db {
            ScaffoldDb::None => "none",
            ScaffoldDb::Postgres => "postgres",
            ScaffoldDb::Sqlite => "sqlite",
        },
        "migration_dir": migration_dir,
        "sqlx_metadata_dir": sqlx_metadata_dir,
        "default_branch_yaml": default_branch_yaml,
        "ci_github_runner": ci_github_runner,
        "admin_api_enabled": admin_api_enabled,
        "react_frontend_enabled": frontends.iter().any(|frontend| matches!(
            frontend.kind,
            ScaffoldFrontendKind::Spa | ScaffoldFrontendKind::Admin
        )),
        "public_frontend_dirs": frontends.iter()
            .filter(|frontend| frontend.kind == ScaffoldFrontendKind::Spa)
            .map(|frontend| frontend.dir.as_str())
            .collect::<Vec<_>>(),
        "e2e_workflow_paths": e2e_workflow_paths_for_backend(
            preset,
            backend_root,
            db,
            migration_dir,
            sqlx_metadata_dir,
            frontends,
        ),
        "frontends": frontends.iter().map(|frontend| json!({
            "name": frontend.name,
            "dir": frontend.dir,
        })).collect::<Vec<_>>(),
        "e2e_frontends": frontends.iter()
            .filter(|frontend| frontend.kind == ScaffoldFrontendKind::Spa)
            .map(|frontend| json!({
                "name": frontend.name,
                "dir": frontend.dir,
            }))
            .collect::<Vec<_>>(),
    });
    template_files
        .iter()
        .map(|file| {
            Ok(scaffold_file(
                file.output,
                render_scaffold_template(file.template, &context)?,
            ))
        })
        .collect()
}

#[cfg(test)]
pub(super) fn e2e_workflow_paths(
    db: ScaffoldDb,
    migration_dir: &str,
    sqlx_metadata_dir: &str,
    frontends: &[FrontendScaffold],
) -> Vec<String> {
    e2e_workflow_paths_for_backend(
        ScaffoldPreset::RustReact,
        ".",
        db,
        migration_dir,
        sqlx_metadata_dir,
        frontends,
    )
}

pub(super) fn e2e_workflow_paths_for_backend(
    preset: ScaffoldPreset,
    backend_root: &str,
    db: ScaffoldDb,
    migration_dir: &str,
    sqlx_metadata_dir: &str,
    frontends: &[FrontendScaffold],
) -> Vec<String> {
    let mut paths = frontends
        .iter()
        .filter(|frontend| frontend.kind == ScaffoldFrontendKind::Spa)
        .map(|frontend| format!("{}/**", frontend.dir))
        .collect::<Vec<_>>();
    if preset == ScaffoldPreset::GoReact {
        paths.push(if backend_root == "." {
            "**".into()
        } else {
            format!("{}/**", backend_root.trim_end_matches('/'))
        });
    } else {
        paths.extend(["apps/**", "crates/**"].map(str::to_owned));
    }
    if db != ScaffoldDb::None {
        paths.push(format!("{migration_dir}/**"));
    }
    paths.extend(
        if preset == ScaffoldPreset::GoReact {
            vec![
                "go.mod",
                "go.sum",
                "go.work",
                "go.work.sum",
                "**/go.mod",
                "**/go.sum",
                "**/go.work",
                "**/go.work.sum",
                "vendor/modules.txt",
                "**/vendor/modules.txt",
                ".jig.toml",
                ".agent/jig-contract.json",
                "scripts/jig",
                "scripts/install-jig.sh",
            ]
        } else {
            vec![
                "Cargo.toml",
                "Cargo.lock",
                "rust-toolchain",
                "rust-toolchain.toml",
                ".cargo/**",
            ]
        }
        .into_iter()
        .map(str::to_owned),
    );
    if db != ScaffoldDb::None && preset != ScaffoldPreset::GoReact {
        paths.push(format!("{sqlx_metadata_dir}/**"));
    }
    paths.extend(
        crate::bootstrap::source_inputs::FRONTEND_SHARED_INPUTS
            .iter()
            .map(|input| (*input).to_owned()),
    );
    paths.push(".github/workflows/e2e.yml".into());
    let mut seen = std::collections::BTreeSet::new();
    paths.retain(|path| seen.insert(path.clone()));
    paths
}

pub(in crate::bootstrap::scaffold) fn frontend_workspace_relative_paths_for_backend(
    preset: ScaffoldPreset,
    package_manager: &str,
    frontends: &[FrontendScaffold],
) -> Vec<PathBuf> {
    frontend_workspace_template_files_for_backend(preset, package_manager, frontends)
        .into_iter()
        .map(|file| PathBuf::from(file.output))
        .collect()
}
