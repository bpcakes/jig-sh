use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::fs;
use std::path::{Component, Path, PathBuf};

use anyhow::{Context, Result, bail};
use jig_contract::{TargetId, tool};
use serde::{Deserialize, Serialize};

use super::repository_model::{
    AuthoredRepositoryModel, RepositoryProjectionHint, action_uses_managed_rust_file_loc_checker,
    frontend_component_id,
};
use super::{
    AnswerOpts, DevApp, DevSettingsAnswers, FrontendApp, GENERATED_NODE_VERSION, ScaffoldOpts,
    ScaffoldPreset, generated_package_manager_spec, generated_package_manager_version,
};
use crate::backend::{
    BackendLanguage, GO_POSTGRES_MIGRATION_DIR, GO_TOOLCHAIN_AUTHORITY_PATH, GoDatabase,
};
use crate::context::{
    DEFAULT_CODEX_MARKETPLACE_ID, DEFAULT_CODEX_MARKETPLACE_SOURCE, ExecutionConfig,
    RustMigrationLayout, config_app_dirs_match, default_codex_marketplace_plugins,
    validate_gate_path_pattern, validate_schema_docs_dir, validate_web_package_manager,
};
use crate::frontend_metadata::resolve_frontend_metadata;
use crate::repository_path::{
    normalize_portable_repo_path, normalize_portable_repository_directory,
};
use crate::shell::quote as shell_quote;

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum HarnessFootprint {
    #[default]
    Full,
    Minimal,
}

impl HarnessFootprint {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Full => "full",
            Self::Minimal => "minimal",
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub(super) struct RenderAnswers {
    #[serde(skip)]
    authored_repository: Option<AuthoredRepositoryModel>,
    #[serde(skip)]
    authored_repository_commands: BTreeMap<String, String>,
    #[serde(skip)]
    scaffolded_frontend_contracts: bool,
    #[serde(skip)]
    go_postgres_integration_script: bool,
    #[serde(skip)]
    repository_projection_hint: RepositoryProjectionHint,
    repo_name: String,
    default_branch: String,
    ci_github_runner: String,
    /// Compatibility input for templates pinned before contract v4. Current
    /// templates intentionally do not persist a Jig product-version pin.
    #[serde(rename = "jig_version")]
    legacy_template_jig_version: String,
    template_source_url: String,
    #[serde(serialize_with = "serialize_harness_footprint")]
    harness_footprint: HarnessFootprint,
    backend_language: BackendLanguage,
    go_database: GoDatabase,
    go_toolchain_authority_path: &'static str,
    sqlx_enabled: bool,
    rust_crate_roots: Vec<String>,
    rust_migration_dir: Option<String>,
    migration_dir: Option<String>,
    rust_migration_layout: RustMigrationLayout,
    rust_sqlx_metadata_dir: Option<String>,
    schema_dump_enabled: bool,
    schema_dump_command: String,
    schema_docs_dir: String,
    schema_check_command: String,
    sqlx_check_command: String,
    migration_add_command: Option<String>,
    bootstrap_command: String,
    contract_check_command: String,
    legacy_dev_command: Option<String>,
    rust_fmt_check_command: String,
    rust_clippy_command: String,
    rust_test_command: String,
    rust_test_locked_command: String,
    go_fmt_check_command: String,
    go_lint_command: String,
    go_test_command: String,
    go_test_locked_command: String,
    sqlc_check_command: String,
    web_package_manager: String,
    application_contracts_enabled: bool,
    web_package_manager_spec: String,
    web_package_manager_version: String,
    node_version: String,
    web_install_command: String,
    web_run_command: String,
    typescript_lint_command: String,
    typescript_typecheck_command: String,
    typescript_build_command: String,
    typescript_coverage_command: String,
    dev: ResolvedDevSettings,
    dev_apps: Vec<DevApp>,
    frontend_apps: Vec<FrontendApp>,
    frontend_workspace_roots: Vec<String>,
    generated_frontend_dev_apps: Vec<FrontendApp>,
    vault: vault::VaultAnswers,
    execution: ExecutionConfig,
    agent_tooling: AgentToolingAnswers,
}

pub(super) fn has_go_postgres_integration_script(root: &Path) -> bool {
    fs::symlink_metadata(root.join("scripts/test-postgres.sh"))
        .is_ok_and(|metadata| metadata.is_file())
}

pub(super) struct AnswerResolution {
    answers: RenderAnswers,
    notes: Vec<String>,
}

#[derive(Debug)]
pub(super) struct AnswerInput {
    raw: RawAnswers,
    shape: AnswerInputShape,
    authored_repository_commands: Option<BTreeMap<String, String>>,
    preserve_repository_model: bool,
    migration_notes: Vec<String>,
}

#[derive(Debug)]
pub(crate) struct PreparedInitAnswers {
    input: AnswerInput,
    effective: Option<AnswerOpts>,
}

#[derive(Clone, Debug, Default)]
pub(super) struct AnswerInputShape {
    top_level_keys: BTreeSet<String>,
    keys: BTreeSet<String>,
    sqlx_enabled: Option<bool>,
    schema_dump_enabled: Option<bool>,
}

const SQLX_SHAPED_ANSWER_KEYS: &[&str] = &[
    "migration_dir",
    "rust_migration_dir",
    "rust_migration_layout",
    "rust_sqlx_metadata_dir",
    "schema_dump_command",
    "schema_check_command",
    "sqlx_check_command",
    "migration_add_command",
];

impl PreparedInitAnswers {
    pub(crate) fn from_opts_at(opts: &AnswerOpts, path_base: &Path) -> Result<Self> {
        let input = AnswerInput::from_init_opts_at(opts, path_base)?;
        let effective = input.effective_opts(opts)?;
        Ok(Self {
            input,
            effective: Some(effective),
        })
    }

    #[cfg(test)]
    pub(crate) fn from_opts_at_with_reader(
        opts: &AnswerOpts,
        path_base: &Path,
        read: impl FnOnce(&Path) -> std::io::Result<String>,
    ) -> Result<Self> {
        let input = AnswerInput::from_init_opts_at_with_reader(opts, path_base, read)?;
        let effective = input.effective_opts(opts)?;
        Ok(Self {
            input,
            effective: Some(effective),
        })
    }

    pub(crate) fn move_effective_to(&mut self, answers: &mut AnswerOpts) -> Result<()> {
        *answers = self
            .effective
            .take()
            .ok_or_else(|| anyhow::anyhow!("prepared init answers were already installed"))?;
        Ok(())
    }

    pub(crate) fn validate_selected_preset(
        &self,
        scaffold: &ScaffoldOpts,
        answers: &AnswerOpts,
    ) -> Result<()> {
        if let Some(preset @ (ScaffoldPreset::RustLibrary | ScaffoldPreset::RustCli)) =
            scaffold.preset
        {
            return self.input.validate_rust_only(preset, scaffold, answers);
        }
        self.input.validate_explicit_file_semantics()
    }

    pub(super) fn into_input(self) -> Result<AnswerInput> {
        if self.effective.is_some() {
            bail!("prepared init answers were not installed before bootstrap");
        }
        Ok(self.input)
    }
}

include!("answers/input.rs");

fn nonempty_answer_string(value: &str) -> bool {
    !value.trim().is_empty()
}

fn reject_rust_only_input(preset: ScaffoldPreset, input: &str) -> Result<()> {
    bail!(
        "--preset {} cannot be combined with incompatible input `{input}`; remove that input or select a matching preset",
        preset.as_str()
    )
}

impl AnswerInputShape {
    pub(super) fn from_table(table: &toml::Table) -> Self {
        let top_level_keys = table.keys().cloned().collect::<BTreeSet<_>>();
        let mut keys = top_level_keys.clone();
        if let Some(repository) = table.get("repository").and_then(toml::Value::as_table) {
            keys.insert("backend_language".into());
            let has_adapter = |expected: &str| {
                repository
                    .get("components")
                    .and_then(toml::Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter_map(toml::Value::as_table)
                    .filter_map(|component| component.get("adapters"))
                    .filter_map(toml::Value::as_array)
                    .flatten()
                    .filter_map(toml::Value::as_str)
                    .any(|adapter| adapter == expected)
            };
            if has_adapter("go-postgres") {
                keys.insert("go_database".into());
            }
            if let Some(commands) = table.get("commands").and_then(toml::Value::as_table) {
                let represented: &[(&str, &str)] = if has_adapter("go") {
                    &[
                        ("api_fmt_command", "go_fmt_check_command"),
                        ("api_lint_command", "go_lint_command"),
                        ("api_test_command", "go_test_command"),
                        ("api_test_locked_command", "go_test_locked_command"),
                    ]
                } else {
                    &[
                        ("api_fmt_command", "rust_fmt_check_command"),
                        ("api_clippy_command", "rust_clippy_command"),
                        ("api_test_command", "rust_test_command"),
                        ("api_test_locked_command", "rust_test_locked_command"),
                    ]
                };
                for (command_key, answer_key) in represented {
                    if commands.contains_key(*command_key) {
                        keys.insert((*answer_key).into());
                    }
                }
                for (command_key, answer_key) in [
                    ("repo_bootstrap_command", "bootstrap_command"),
                    ("api_sqlx_command", "sqlx_check_command"),
                    ("api_schema_dump_command", "schema_dump_command"),
                    ("api_sqlc_command", "sqlc_check_command"),
                    (
                        "repo_compat_typescript_lint_command",
                        "typescript_lint_command",
                    ),
                    (
                        "repo_compat_typescript_typecheck_command",
                        "typescript_typecheck_command",
                    ),
                    (
                        "repo_compat_typescript_build_command",
                        "typescript_build_command",
                    ),
                    (
                        "repo_compat_typescript_coverage_command",
                        "typescript_coverage_command",
                    ),
                ] {
                    if commands.contains_key(command_key) {
                        keys.insert(answer_key.into());
                    }
                }
            }
        }
        for migration_key in ["migration_dir", "rust_migration_dir"] {
            if table.get(migration_key).and_then(toml::Value::as_str) == Some("") {
                keys.remove(migration_key);
            }
        }
        Self {
            top_level_keys,
            sqlx_enabled: table.get("sqlx_enabled").and_then(toml::Value::as_bool),
            schema_dump_enabled: table
                .get("schema_dump_enabled")
                .and_then(toml::Value::as_bool),
            keys,
        }
    }

    pub(super) fn contains_key(&self, key: &str) -> bool {
        self.keys.contains(key)
    }

    fn contains_top_level_key(&self, key: &str) -> bool {
        self.top_level_keys.contains(key)
    }

    pub(super) fn explicit_sqlx_enabled(&self, answers: &AnswerOpts) -> Option<bool> {
        answers.sqlx_enabled.or(self.sqlx_enabled)
    }

    pub(super) fn should_apply_inferred_sqlx_enabled(&self, answers: &AnswerOpts) -> bool {
        // schema_dump_enabled=true is SQLx-shaped because schema dumps require SQLx.
        // schema_dump_enabled=false is compatible with the inferred tooling-only profile.
        self.explicit_sqlx_enabled(answers).is_none()
            && !answer_opts_has_sqlx_shape(answers)
            && answers.schema_dump_enabled.or(self.schema_dump_enabled) != Some(true)
            && !self.has_sqlx_shape()
    }

    fn has_sqlx_shape(&self) -> bool {
        SQLX_SHAPED_ANSWER_KEYS
            .iter()
            .any(|key| self.keys.contains(*key))
    }
}

impl AnswerResolution {
    pub(super) fn from_opts(
        opts: &AnswerOpts,
        destination: &Path,
        use_defaults: bool,
    ) -> Result<Self> {
        Self::from_input(
            AnswerInput::from_opts(opts)?,
            opts,
            destination,
            use_defaults,
        )
    }

    pub(super) fn from_input(
        input: AnswerInput,
        opts: &AnswerOpts,
        destination: &Path,
        use_defaults: bool,
    ) -> Result<Self> {
        let (mut raw, authored_repository_commands, preserve_repository_model, mut notes) =
            input.into_parts();
        raw.merge_opts(opts);
        let vault_note = raw.apply_existing_vault_default(destination)?;
        let sqlx_defaulted_to_tooling_only = if use_defaults {
            raw.apply_sqlx_default_for_cli_defaults()
        } else {
            false
        };
        let answers = resolve_render_answers(
            raw,
            default_repo_name(destination),
            authored_repository_commands,
            preserve_repository_model,
        )?;
        if sqlx_defaulted_to_tooling_only {
            notes.push(
                "SQLx answers were omitted under --defaults, so Jig rendered a tooling-only profile. Pass --sqlx-enabled true --rust-migration-dir <dir> for SQLx repos, or pass --sqlx-enabled false for tooling-only repos."
                    .into(),
            );
        }
        if let Some(note) = vault_note {
            notes.push(note);
        }
        Ok(Self { answers, notes })
    }

    pub(super) fn into_parts(self) -> (RenderAnswers, Vec<String>) {
        (self.answers, self.notes)
    }
}

impl RenderAnswers {
    pub(super) fn from_answers_file(path: &Path) -> Result<Self> {
        let (mut raw, authored_repository_commands, _, _) =
            AnswerInput::from_file(path)?.into_parts();
        raw.normalize_legacy_sqlx_disabled_schema_dump();
        raw.normalize_legacy_generated_cargo_command_defaults();
        let mut answers = resolve_render_answers(raw, None, authored_repository_commands, true)?;
        answers.go_postgres_integration_script = path
            .parent()
            .is_some_and(has_go_postgres_integration_script);
        Ok(answers)
    }

    fn authored_repository_has_adapter(&self, expected: &str) -> Option<bool> {
        self.authored_repository.as_ref().map(|repository| {
            repository
                .components
                .iter()
                .any(|component| component.adapters.iter().any(|adapter| adapter == expected))
        })
    }

    fn authored_repository_managed_target(
        &self,
        alias: &str,
        owning_adapters: &[&str],
    ) -> Option<Option<TargetId>> {
        self.authored_repository.as_ref().map(|repository| {
            let owning_components = repository
                .components
                .iter()
                .filter(|component| {
                    component
                        .adapters
                        .iter()
                        .any(|adapter| owning_adapters.contains(&adapter.as_str()))
                })
                .map(|component| component.id.clone())
                .collect::<BTreeSet<_>>();
            let mut matching = repository.actions.iter().filter(|action| {
                action
                    .legacy_aliases
                    .iter()
                    .any(|candidate| candidate == alias)
            });
            let action = matching.next()?;
            if matching.next().is_some() || !owning_components.contains(&action.target.component) {
                return None;
            }

            if crate::repository::validate_read_only_check_closure(
                &repository.actions,
                std::iter::once(&action.target),
            )
            .is_err()
            {
                return None;
            }
            Some(action.target.clone())
        })
    }

    fn managed_ci_target(
        &self,
        alias: &str,
        owning_adapters: &[&str],
        generated_target: &str,
    ) -> Option<String> {
        self.authored_repository_managed_target(alias, owning_adapters)
            .map_or_else(
                || Some(generated_target.to_owned()),
                |target| target.map(|target| target.to_string()),
            )
    }

    pub(super) fn go_fmt_ci_target(&self) -> Option<String> {
        self.managed_ci_target(tool::FMT_CHECK, &["go"], "api:fmt")
    }

    pub(super) fn go_lint_ci_target(&self) -> Option<String> {
        self.managed_ci_target(tool::LINT, &["go"], "api:lint")
    }

    pub(super) fn go_test_locked_ci_target(&self) -> Option<String> {
        self.managed_ci_target(tool::TEST_LOCKED, &["go"], "api:test-locked")
    }

    pub(super) fn rust_fmt_ci_target(&self) -> Option<String> {
        let generated = format!(
            "{}:fmt",
            self.repository_projection_hint.rust_component_id()
        );
        self.managed_ci_target(tool::FMT_CHECK, &["rust", "sqlx"], &generated)
    }

    pub(super) fn rust_clippy_ci_target(&self) -> Option<String> {
        let generated = format!(
            "{}:clippy",
            self.repository_projection_hint.rust_component_id()
        );
        self.managed_ci_target(tool::CLIPPY, &["rust", "sqlx"], &generated)
    }

    pub(super) fn rust_test_locked_ci_target(&self) -> Option<String> {
        let generated = format!(
            "{}:test-locked",
            self.repository_projection_hint.rust_component_id()
        );
        self.managed_ci_target(tool::TEST_LOCKED, &["rust", "sqlx"], &generated)
    }

    pub(super) fn go_sqlc_ci_target(&self) -> Option<String> {
        self.managed_ci_target(tool::SQLC_CHECK, &["go-postgres"], "api:sqlc")
    }

    pub(super) fn go_backend_enabled(&self) -> bool {
        self.authored_repository_has_adapter("go")
            .unwrap_or_else(|| self.backend_language.is_go())
    }

    pub(super) fn rust_backend_enabled(&self) -> bool {
        self.authored_repository
            .as_ref()
            .map(|repository| {
                repository.components.iter().any(|component| {
                    component
                        .adapters
                        .iter()
                        .any(|adapter| adapter == "rust" || adapter == "sqlx")
                })
            })
            .unwrap_or_else(|| self.backend_language == BackendLanguage::Rust)
    }

    pub(super) fn go_postgres_enabled(&self) -> bool {
        self.authored_repository_has_adapter("go-postgres")
            .unwrap_or_else(|| self.backend_language.is_go() && self.go_database.is_postgres())
    }

    pub(super) fn go_ci_workflow_enabled(&self) -> bool {
        self.go_backend_enabled()
            && self.go_fmt_ci_target().is_some()
            && self.go_lint_ci_target().is_some()
            && self.go_test_locked_ci_target().is_some()
    }

    pub(super) fn rust_ci_workflow_enabled(&self) -> bool {
        self.rust_backend_enabled()
            && self.rust_fmt_ci_target().is_some()
            && self.rust_clippy_ci_target().is_some()
            && self.rust_test_locked_ci_target().is_some()
    }

    pub(super) fn go_sqlc_ci_enabled(&self) -> bool {
        self.go_postgres_enabled()
            && self.go_ci_workflow_enabled()
            && self.go_sqlc_ci_target().is_some()
    }

    pub(super) fn go_postgres_integration_ci_enabled(&self) -> bool {
        self.go_postgres_enabled()
            && self.go_ci_workflow_enabled()
            && self.go_postgres_integration_script
    }

    pub(super) const fn go_database(&self) -> GoDatabase {
        self.go_database
    }

    pub(super) fn is_minimal_footprint(&self) -> bool {
        self.harness_footprint == HarnessFootprint::Minimal
    }

    pub(super) fn frontend_harness_enabled(&self) -> bool {
        !self.is_minimal_footprint() && !self.frontend_apps.is_empty()
    }

    pub(super) const fn scaffolded_frontend_contracts(&self) -> bool {
        self.scaffolded_frontend_contracts
    }

    pub(super) fn enable_scaffolded_frontend_contracts(&mut self) {
        self.scaffolded_frontend_contracts = true;
    }

    pub(super) fn enable_go_postgres_integration_script(&mut self) {
        self.go_postgres_integration_script = true;
    }

    pub(super) fn dev_apps_configured(&self) -> bool {
        !self.dev_apps.is_empty() || !self.generated_frontend_dev_apps.is_empty()
    }

    pub(super) const fn sqlx_enabled(&self) -> bool {
        self.sqlx_enabled
    }

    pub(super) const fn schema_dump_enabled(&self) -> bool {
        self.schema_dump_enabled
    }

    pub(super) fn schema_docs_dir(&self) -> &str {
        &self.schema_docs_dir
    }

    pub(super) const fn migration_add_enabled(&self) -> bool {
        !self.sqlx_enabled || self.rust_migration_layout.allows_migration_add()
    }

    pub(super) fn migration_dir(&self) -> Option<&str> {
        self.migration_dir
            .as_deref()
            .or(self.rust_migration_dir.as_deref())
    }

    pub(super) fn web_package_manager(&self) -> &str {
        &self.web_package_manager
    }

    pub(super) fn bootstrap_command_configured(&self) -> bool {
        !self.bootstrap_command.trim().is_empty()
    }

    pub(super) fn repository_command(&self, legacy_key: &str) -> Option<&str> {
        match legacy_key {
            "bootstrap_command" => Some(&self.bootstrap_command),
            "rust_fmt_check_command" => Some(&self.rust_fmt_check_command),
            "rust_clippy_command" => Some(&self.rust_clippy_command),
            "rust_test_command" => Some(&self.rust_test_command),
            "rust_test_locked_command" => Some(&self.rust_test_locked_command),
            "go_fmt_check_command" => Some(&self.go_fmt_check_command),
            "go_lint_command" => Some(&self.go_lint_command),
            "go_test_command" => Some(&self.go_test_command),
            "go_test_locked_command" => Some(&self.go_test_locked_command),
            "sqlc_check_command" => Some(&self.sqlc_check_command),
            "typescript_lint_command" => Some(&self.typescript_lint_command),
            "typescript_typecheck_command" => Some(&self.typescript_typecheck_command),
            "typescript_build_command" => Some(&self.typescript_build_command),
            "typescript_coverage_command" => Some(&self.typescript_coverage_command),
            "sqlx_check_command" => Some(&self.sqlx_check_command),
            "schema_dump_command" => Some(&self.schema_dump_command),
            _ => None,
        }
    }

    pub(super) const fn has_legacy_dev_command(&self) -> bool {
        self.legacy_dev_command.is_some()
    }
}

mod raw_answers;
use raw_answers::*;

mod file_budget;

fn inherit_repository_command(destination: &mut Option<String>, commands: &toml::Table, key: &str) {
    if destination.is_none() {
        *destination = commands
            .get(key)
            .and_then(toml::Value::as_str)
            .map(str::to_owned);
    }
}

fn authored_repository_commands_from_table(
    table: &toml::Table,
) -> Option<BTreeMap<String, String>> {
    let Some(commands) = table.get("commands") else {
        return Some(BTreeMap::new());
    };
    let commands = commands.as_table()?;
    commands
        .iter()
        .map(|(key, value)| value.as_str().map(|value| (key.clone(), value.to_owned())))
        .collect()
}

fn loaded_repository_model_is_custom(
    raw: &RawAnswers,
    authored_repository_commands: Option<&BTreeMap<String, String>>,
) -> bool {
    let Some(authored_repository) = raw
        .repository
        .as_ref()
        .filter(|repository| repository.is_complete())
    else {
        return false;
    };
    let Some(authored_repository_commands) = authored_repository_commands else {
        return false;
    };
    if !authored_repository.command_references_resolve(authored_repository_commands) {
        return false;
    }

    let mut generated_raw = raw.clone();
    generated_raw.repository = None;
    let Ok(generated_answers) = generated_raw.resolve_with_authored_repository(None, None, None)
    else {
        return true;
    };
    let Ok(generated) =
        crate::bootstrap::repository_model::RepositoryRenderModel::from_answers(&generated_answers)
    else {
        return true;
    };
    !generated.matches_authored_projection(authored_repository, authored_repository_commands)
}

fn resolve_render_answers(
    mut raw: RawAnswers,
    default_repo_name: Option<String>,
    authored_repository_commands: Option<BTreeMap<String, String>>,
    preserve_repository_model: bool,
) -> Result<RenderAnswers> {
    let authored_repository = preserve_repository_model
        .then(|| raw.repository.take())
        .flatten()
        .filter(AuthoredRepositoryModel::is_complete)
        .filter(|_| authored_repository_commands.is_some());
    let mut answers = raw.resolve_with_authored_repository(
        default_repo_name,
        authored_repository,
        authored_repository_commands.as_ref(),
    )?;
    if let Some(authored_repository_commands) = authored_repository_commands
        && answers.authored_repository.is_some()
    {
        answers.authored_repository_commands = authored_repository_commands;
    }
    Ok(answers)
}

fn answer_opts_has_sqlx_shape(answers: &AnswerOpts) -> bool {
    SQLX_SHAPED_ANSWER_KEYS.iter().any(|key| match *key {
        "migration_dir" => answers
            .migration_dir
            .as_deref()
            .is_some_and(|value| !value.is_empty()),
        "rust_migration_dir" => answers
            .rust_migration_dir
            .as_deref()
            .is_some_and(|value| !value.is_empty()),
        "rust_migration_layout" => answers.rust_migration_layout.is_some(),
        "rust_sqlx_metadata_dir" => answers.rust_sqlx_metadata_dir.is_some(),
        "schema_dump_command" => answers.schema_dump_command.is_some(),
        "schema_check_command" => answers.schema_check_command.is_some(),
        "sqlx_check_command" => answers.sqlx_check_command.is_some(),
        "migration_add_command" => answers.migration_add_command.is_some(),
        _ => false,
    })
}

pub(super) fn should_default_init_sqlx_disabled(answers: &AnswerOpts) -> bool {
    answers.sqlx_enabled.is_none()
        && answers.schema_dump_enabled != Some(true)
        && !answer_opts_has_sqlx_shape(answers)
}

fn normalize_legacy_command_default(command: &mut Option<String>, legacy_default: &str) {
    if command.as_deref() == Some(legacy_default) {
        *command = None;
    }
}

fn optional_cargo_command(command: &str, label: &str) -> String {
    let skip_prefix = crate::CARGO_SKIP_OUTPUT_PREFIX;
    let skip_message = shell_quote(&format!("{skip_prefix}{label}."));
    // Runtime command dispatch sets CWD to the repo root, so this guard checks
    // for a root Cargo workspace without blocking harness-only repos.
    format!(
        "{}{command}{}printf '%s\\n' {skip_message}{}",
        crate::shell::OPTIONAL_CARGO_COMMAND_PREFIX,
        crate::shell::OPTIONAL_CARGO_COMMAND_ELSE,
        crate::shell::OPTIONAL_CARGO_COMMAND_SUFFIX,
    )
}

include!("answers/frontend_validation.rs");

fn is_safe_frontend_app_name(value: &str) -> bool {
    !value.is_empty()
        && value.trim() == value
        && value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_')
}

fn is_supported_frontend_app_kind(value: &str) -> bool {
    matches!(value, "vite" | "env-port")
}

fn is_supported_frontend_app_role(value: &str) -> bool {
    matches!(value, "spa" | "admin" | "astro")
}

fn validate_frontend_app_dir(app_name: &str, value: &str) -> Result<()> {
    if value.is_empty() || value.trim() != value {
        bail!("frontend app '{app_name}' dir must be a non-empty relative path");
    }
    if !value
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '/' | '.' | '-' | '_'))
    {
        bail!(
            "frontend app '{app_name}' dir '{value}' contains unsupported characters. Use a repo-relative path with ASCII letters, numbers, '/', '.', '-' or '_'; use forward slashes on every platform."
        );
    }

    let path = Path::new(value);
    if path.is_absolute() {
        bail!("frontend app '{app_name}' dir '{value}' must be relative");
    }
    if value.split('/').any(str::is_empty) {
        bail!("frontend app '{app_name}' dir '{value}' must not contain empty path components");
    }
    if value == "." {
        return Ok(());
    }
    if value.split('/').any(|segment| segment == ".") {
        bail!("frontend app '{app_name}' dir '{value}' must not contain '.' path components");
    }

    for component in path.components() {
        match component {
            Component::Normal(_) => {}
            Component::CurDir => {
                bail!(
                    "frontend app '{app_name}' dir '{value}' must not contain '.' path components"
                );
            }
            Component::ParentDir => {
                bail!("frontend app '{app_name}' dir '{value}' must not contain '..'");
            }
            Component::RootDir | Component::Prefix(_) => {
                bail!("frontend app '{app_name}' dir '{value}' must be relative");
            }
        }
    }
    Ok(())
}

pub(super) fn web_install_command(package_manager: &str) -> &'static str {
    match package_manager {
        "bun" => "bun install --frozen-lockfile",
        "pnpm" => "pnpm install --frozen-lockfile",
        "npm" => {
            "npm ci --include=dev --include=optional --include=peer --bin-links=true --dry-run=false --package-lock-only=false --package-lock=true --global=false"
        }
        "yarn" => "yarn install --frozen-lockfile",
        _ => unreachable!("web package manager was already validated"),
    }
}

pub(super) fn web_run_command(package_manager: &str) -> &'static str {
    match package_manager {
        "bun" => "bun run",
        "pnpm" => "pnpm run",
        "npm" => "npm run",
        "yarn" => "yarn run",
        _ => unreachable!("web package manager was already validated"),
    }
}

mod generated_gate;
pub(super) use generated_gate::frontend_gate_key;
use generated_gate::normalize_generated_gate_root;

mod accessors;
mod serialization;
use serialization::*;

mod dev;
pub(crate) use dev::{ResolvedDevSettings, resolve_settings as resolve_dev_settings};
#[cfg(test)]
#[path = "answers_tests.rs"]
mod tests;
mod vault;
