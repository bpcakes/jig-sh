use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde_json::Value as JsonValue;
#[cfg(test)]
use toml::{Table, Value as TomlValue};

use super::AnswerOpts;
use super::InitMutationTransaction;
use super::answers::{
    AnswerInput, AnswerResolution, HarnessFootprint, RenderAnswers,
    has_go_postgres_integration_script,
};
use super::gate_preview::generated_gates;
use super::renderer::{RenderStageRequest, stage_render};
use super::sync::ApplyRenderReport;
use super::sync::{ApplyRenderConflictPolicy, ApplyRenderOptions, apply_staged_render};
use super::template_source::PreparedTemplateSource;
#[cfg(test)]
use super::template_source::PrivateAnswerOverrides;
#[cfg(test)]
use super::{TEMPLATE_LOCAL_PATH_KEY, TEMPLATE_MODE_KEY};
use crate::bootstrap::path::validate_portable_planned_file_collisions;
use crate::progress::CliProgress;

const ANSWERS_DETAIL: &str = ".jig.toml values and command defaults";
const REQUIRED_FRONTEND_SCRIPTS: &[&str] = &["lint", "typecheck", "build:bundle", "test:coverage"];

pub(super) struct BootstrapCopyRequest<'a> {
    pub(super) destination: &'a Path,
    pub(super) template: &'a PreparedTemplateSource,
    pub(super) answers: &'a AnswerOpts,
    pub(super) answer_input: Option<AnswerInput>,
    pub(super) use_defaults: bool,
    pub(super) force: bool,
    pub(super) dry_run: bool,
    pub(super) backup_root: Option<PathBuf>,
    pub(super) seed_repo_path: Option<&'a Path>,
    pub(super) prior_harness_footprint: Option<HarnessFootprint>,
    pub(super) prior_managed_paths: Option<&'a BTreeSet<PathBuf>>,
    pub(super) reconcile_runtime_config: bool,
    pub(super) allow_answers_overwrite: bool,
    pub(super) allow_contract_overwrite: bool,
    pub(super) reserved_output_paths: Vec<PathBuf>,
    pub(super) scaffolded_frontend_contracts: bool,
    pub(super) scaffolded_go_postgres_integration: bool,
    pub(super) init_transaction: Option<&'a mut InitMutationTransaction>,
    pub(super) progress: CliProgress,
}

pub(super) struct BootstrapCopyResult {
    pub(super) default_branch: Option<String>,
    pub(super) bootstrap_command_configured: bool,
    pub(super) frontend_apps_configured: bool,
    pub(super) dev_apps_configured: bool,
    pub(super) sqlx_enabled: bool,
    pub(super) schema_dump_enabled: bool,
    pub(super) minimal_footprint: bool,
    pub(super) full_to_minimal_transition: bool,
    pub(super) render_preview: AdoptionRenderPreview,
    pub(super) apply_report: ApplyRenderReport,
    pub(super) notes: Vec<String>,
}

#[derive(Clone, Debug, Default)]
pub(super) struct AdoptionRenderPreview {
    pub(super) generated_gates: Vec<String>,
    pub(super) managed_files: Vec<String>,
    pub(super) retired_managed_files: Vec<String>,
}

pub(super) fn render_and_copy_bootstrap_template(
    mut request: BootstrapCopyRequest<'_>,
) -> Result<BootstrapCopyResult> {
    request.progress.step("resolve answers", ANSWERS_DETAIL);
    let preferred_rendered_commands = request
        .answer_input
        .as_ref()
        .map(|input| input.preferred_rendered_command_keys(request.answers))
        .unwrap_or_default();
    let answer_resolution = request
        .progress
        .log_blocked_on_err(match request.answer_input {
            Some(input) => AnswerResolution::from_input(
                input,
                request.answers,
                request.destination,
                request.use_defaults,
            ),
            None => AnswerResolution::from_opts(
                request.answers,
                request.destination,
                request.use_defaults,
            ),
        })?;
    let (mut answers, mut notes) = answer_resolution.into_parts();
    if request.scaffolded_frontend_contracts {
        answers.enable_scaffolded_frontend_contracts();
    }
    if request.scaffolded_go_postgres_integration {
        answers.enable_go_postgres_integration_script();
    }
    if request
        .seed_repo_path
        .is_some_and(has_go_postgres_integration_script)
    {
        answers.enable_go_postgres_integration_script();
    }
    let full_to_minimal_transition = request.prior_harness_footprint
        == Some(HarnessFootprint::Full)
        && answers.is_minimal_footprint();
    if request.seed_repo_path.is_some() && answers.frontend_harness_enabled() {
        request
            .progress
            .step("validate web apps", "package.json scripts for CI checks");
        request
            .progress
            .log_blocked_on_err(validate_frontend_app_scripts(request.destination, &answers))?;
    }
    let staged = stage_render(RenderStageRequest {
        template: request.template,
        answers: &answers,
        seed_repo_path: request.seed_repo_path,
        prior_managed_paths: request.prior_managed_paths,
        reconcile_runtime_config: request.reconcile_runtime_config,
        preferred_rendered_commands,
        contract_version: None,
        progress: request.progress,
    })?;
    let render_preview = AdoptionRenderPreview::from_staged_render(
        &answers,
        &staged.active_paths,
        &staged.retirement_paths,
    );
    request
        .progress
        .info("generated gates", render_preview.generated_gates.join(", "));
    request.progress.info(
        "managed files",
        format!("{} path(s)", render_preview.managed_files.len()),
    );
    request.progress.info(
        "retired managed files",
        format!("{} path(s)", render_preview.retired_managed_files.len()),
    );
    reject_reserved_output_collisions(&staged.active_paths, &request.reserved_output_paths)?;
    if let Some(transaction) = request.init_transaction.as_deref_mut() {
        transaction.plan_staged_render(&staged, &request.reserved_output_paths)?;
    }

    let apply_report = apply_staged_render(
        &staged,
        request.destination,
        ApplyRenderOptions {
            conflict_policy: if request.force {
                ApplyRenderConflictPolicy::Accept
            } else {
                ApplyRenderConflictPolicy::Reject(
                    "Adopt would overwrite template-managed paths. No files were changed. Re-run with --force or clear these paths first:",
                )
            },
            dry_run: request.dry_run,
            allow_answers_overwrite: request.allow_answers_overwrite,
            allow_contract_overwrite: request.allow_contract_overwrite,
            allow_manifest_overwrite: request.prior_managed_paths.is_some(),
            backup_root: request.backup_root.as_deref(),
            progress: request.progress,
            init_transaction: request.init_transaction,
        },
    )?;

    if answers.has_legacy_dev_command() {
        notes.push(
            "Preserved deprecated dev_command for migration; generated commands ignore it. Move that value into [dev] / [[dev.apps]] when ready."
                .into(),
        );
    }

    Ok(BootstrapCopyResult {
        default_branch: Some(answers.default_branch().to_string()),
        bootstrap_command_configured: answers.bootstrap_command_configured(),
        frontend_apps_configured: !answers.frontend_apps().is_empty(),
        dev_apps_configured: answers.dev_apps_configured(),
        sqlx_enabled: answers.sqlx_enabled(),
        schema_dump_enabled: answers.schema_dump_enabled(),
        minimal_footprint: answers.is_minimal_footprint(),
        full_to_minimal_transition,
        render_preview,
        apply_report,
        notes,
    })
}

fn reject_reserved_output_collisions(
    managed_paths: &BTreeSet<PathBuf>,
    reserved_output_paths: &[PathBuf],
) -> Result<()> {
    // This guards preset/template ownership bugs, so --force must not bypass it.
    validate_portable_planned_file_collisions(
        managed_paths.iter().chain(reserved_output_paths.iter()),
    )
}

impl AdoptionRenderPreview {
    fn from_staged_render(
        answers: &RenderAnswers,
        active_paths: &BTreeSet<PathBuf>,
        retirement_paths: &BTreeSet<PathBuf>,
    ) -> Self {
        Self {
            generated_gates: generated_gates(answers),
            managed_files: active_paths
                .iter()
                .map(|path| path.display().to_string())
                .collect(),
            retired_managed_files: retirement_paths
                .iter()
                .map(|path| path.display().to_string())
                .collect(),
        }
    }
}

fn validate_frontend_app_scripts(destination: &Path, answers: &RenderAnswers) -> Result<()> {
    for app in answers.frontend_apps() {
        let app_dir = destination.join(&app.dir);
        let package_path = app_dir.join("package.json");
        if !package_path.is_file() {
            bail!(
                "Configured frontend app '{}' in {} is missing package.json. Add the app package.json, or remove the entry from frontend_apps until web CI checks are ready.",
                app.name,
                app.dir
            );
        }
        let package = fs::read_to_string(&package_path)
            .with_context(|| format!("Failed to read {}", package_path.display()))?;
        let package: JsonValue = serde_json::from_str(&package)
            .with_context(|| format!("Failed to parse {}", package_path.display()))?;
        let scripts = package
            .get("scripts")
            .and_then(JsonValue::as_object)
            .cloned()
            .unwrap_or_default();
        let missing = REQUIRED_FRONTEND_SCRIPTS
            .iter()
            .copied()
            .filter(|script| {
                !matches!(
                    scripts.get(*script).and_then(JsonValue::as_str),
                    Some(command) if !command.trim().is_empty()
                )
            })
            .collect::<Vec<_>>();
        if !missing.is_empty() {
            bail!(
                "Configured frontend app '{}' in {} is missing package.json scripts required by generated web CI: {}. Add those scripts, or remove the entry from frontend_apps until the app is CI-ready.",
                app.name,
                app.dir,
                missing.join(", ")
            );
        }
        validate_frontend_app_lockfile(destination, &app_dir, answers.web_package_manager(), app)?;
    }
    Ok(())
}

fn validate_frontend_app_lockfile(
    destination: &Path,
    app_dir: &Path,
    package_manager: &str,
    app: &super::FrontendApp,
) -> Result<()> {
    let lockfiles = frontend_lockfile_names(package_manager);
    let has_repo_lockfile = destination.join("package.json").is_file()
        && lockfiles
            .iter()
            .any(|lockfile| destination.join(lockfile).is_file());
    let has_app_lockfile = lockfiles
        .iter()
        .any(|lockfile| app_dir.join(lockfile).is_file());
    if has_repo_lockfile || has_app_lockfile {
        return Ok(());
    }

    bail!(
        "Configured frontend app '{}' in {} does not have a lockfile for {} at the repo root or app directory. Add one, or remove the entry from frontend_apps until web CI is ready.",
        app.name,
        app.dir,
        package_manager
    )
}

fn frontend_lockfile_names(package_manager: &str) -> &'static [&'static str] {
    match package_manager {
        "bun" => &["bun.lock", "bun.lockb"],
        "npm" => &["npm-shrinkwrap.json", "package-lock.json"],
        "pnpm" => &["pnpm-lock.yaml"],
        "yarn" => &["yarn.lock"],
        _ => unreachable!("web package manager was already validated"),
    }
}

#[cfg(test)]
pub(super) fn seed_answers_toml(
    opts: &AnswerOpts,
    private_answers: &PrivateAnswerOverrides,
) -> TomlValue {
    let mut mapping = Table::new();
    insert_string(&mut mapping, "repo_name", opts.repo_name.as_deref());
    insert_string(
        &mut mapping,
        "default_branch",
        opts.default_branch.as_deref(),
    );
    insert_string(
        &mut mapping,
        "ci_github_runner",
        opts.ci_github_runner.as_deref(),
    );
    insert_string(
        &mut mapping,
        "template_source_url",
        opts.template_source_url.as_deref(),
    );
    insert_bool(&mut mapping, "sqlx_enabled", opts.sqlx_enabled);
    insert_string(
        &mut mapping,
        "rust_migration_dir",
        opts.rust_migration_dir.as_deref(),
    );
    insert_string(
        &mut mapping,
        "rust_migration_layout",
        opts.rust_migration_layout.map(|layout| layout.as_str()),
    );
    insert_string(
        &mut mapping,
        "rust_sqlx_metadata_dir",
        opts.rust_sqlx_metadata_dir.as_deref(),
    );
    insert_bool(
        &mut mapping,
        "schema_dump_enabled",
        opts.schema_dump_enabled,
    );
    insert_string(
        &mut mapping,
        "schema_dump_command",
        opts.schema_dump_command.as_deref(),
    );
    insert_string(
        &mut mapping,
        "schema_check_command",
        opts.schema_check_command.as_deref(),
    );
    insert_string(
        &mut mapping,
        "sqlx_check_command",
        opts.sqlx_check_command.as_deref(),
    );
    insert_string(
        &mut mapping,
        "migration_add_command",
        opts.migration_add_command.as_deref(),
    );
    insert_string(
        &mut mapping,
        "bootstrap_command",
        opts.bootstrap_command.as_deref(),
    );
    insert_string(
        &mut mapping,
        "contract_check_command",
        opts.contract_check_command.as_deref(),
    );
    insert_string(&mut mapping, "dev_command", opts.dev_command.as_deref());
    insert_string(
        &mut mapping,
        "rust_fmt_check_command",
        opts.rust_fmt_check_command.as_deref(),
    );
    insert_string(
        &mut mapping,
        "rust_clippy_command",
        opts.rust_clippy_command.as_deref(),
    );
    insert_string(
        &mut mapping,
        "rust_test_command",
        opts.rust_test_command.as_deref(),
    );
    insert_string(
        &mut mapping,
        "rust_test_locked_command",
        opts.rust_test_locked_command.as_deref(),
    );
    insert_string(
        &mut mapping,
        "web_package_manager",
        opts.web_package_manager.as_deref(),
    );
    insert_string(
        &mut mapping,
        TEMPLATE_MODE_KEY,
        private_answers.template_mode_answer(),
    );
    insert_string(
        &mut mapping,
        TEMPLATE_LOCAL_PATH_KEY,
        private_answers.template_local_path_answer(),
    );

    if !opts.rust_crate_roots.is_empty() {
        mapping.insert(
            "rust_crate_roots".into(),
            TomlValue::Array(
                opts.rust_crate_roots
                    .iter()
                    .cloned()
                    .map(TomlValue::String)
                    .collect(),
            ),
        );
    }
    if !opts.frontend_apps.is_empty() {
        mapping.insert(
            "frontend_apps".into(),
            TomlValue::Array(
                opts.frontend_apps
                    .iter()
                    .map(|app| {
                        let mut app_table = Table::new();
                        app_table.insert("name".into(), TomlValue::String(app.name.clone()));
                        app_table.insert("dir".into(), TomlValue::String(app.dir.clone()));
                        app_table.insert(
                            "coverage_threshold".into(),
                            TomlValue::Integer(app.coverage_threshold.into()),
                        );
                        TomlValue::Table(app_table)
                    })
                    .collect(),
            ),
        );
    }

    TomlValue::Table(mapping)
}

#[cfg(test)]
fn insert_string(mapping: &mut Table, key: &str, value: Option<&str>) {
    if let Some(value) = value {
        mapping.insert(key.to_string(), TomlValue::String(value.to_string()));
    }
}

#[cfg(test)]
fn insert_bool(mapping: &mut Table, key: &str, value: Option<bool>) {
    if let Some(value) = value {
        mapping.insert(key.to_string(), TomlValue::Boolean(value));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_reserved_output_collisions_before_apply() {
        let managed_paths = BTreeSet::from([PathBuf::from("Cargo.toml")]);
        let error = reject_reserved_output_collisions(
            &managed_paths,
            &["Cargo.toml".into(), "apps/demo-api/src/main.rs".into()],
        )
        .unwrap_err()
        .to_string();

        assert!(error.contains("Portable planned repository file collision"));
        assert!(error.matches("Cargo.toml").count() >= 2, "{error}");
    }

    #[test]
    fn rejects_case_folded_managed_path_collisions_without_scaffold_outputs() {
        let managed_paths = BTreeSet::from([PathBuf::from("Owned"), PathBuf::from("owned/child")]);
        let error = reject_reserved_output_collisions(&managed_paths, &[])
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("Portable planned repository file collision"),
            "{error}"
        );
    }
}
