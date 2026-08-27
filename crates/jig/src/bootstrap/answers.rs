// agentic-loc-exception: legacy answer normalization remains centralized during contract-v5 rollout.

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::fs;
use std::path::{Component, Path, PathBuf};

use anyhow::{Context, Result, bail};
use jig_contract::{TargetId, tool};
use serde::{Deserialize, Serialize};

use super::repository_model::{AuthoredRepositoryModel, frontend_component_id};
use super::{
    AnswerOpts, DevApp, FrontendApp, GENERATED_NODE_VERSION, generated_package_manager_spec,
    generated_package_manager_version,
};
use crate::backend::{
    BackendLanguage, GO_POSTGRES_MIGRATION_DIR, GO_TOOLCHAIN_AUTHORITY_PATH, GoDatabase,
};
use crate::context::{
    DEFAULT_CODEX_MARKETPLACE_ID, DEFAULT_CODEX_MARKETPLACE_SOURCE, ExecutionConfig,
    RustMigrationLayout, StatusConfig, config_app_dirs_match, default_codex_marketplace_plugins,
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
    dev_apps: Vec<DevApp>,
    frontend_apps: Vec<FrontendApp>,
    frontend_workspace_roots: Vec<String>,
    generated_frontend_dev_apps: Vec<FrontendApp>,
    vault: vault::VaultAnswers,
    status: StatusConfig,
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

pub(super) struct AnswerInput {
    raw: RawAnswers,
    shape: AnswerInputShape,
    authored_repository_commands: Option<BTreeMap<String, String>>,
    preserve_repository_model: bool,
}

#[derive(Clone, Debug, Default)]
pub(super) struct AnswerInputShape {
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

impl AnswerInput {
    pub(super) fn from_opts(opts: &AnswerOpts) -> Result<Self> {
        let Some(path) = opts.answers_file.as_deref() else {
            return Ok(Self {
                raw: RawAnswers::default(),
                shape: AnswerInputShape::default(),
                authored_repository_commands: Some(BTreeMap::new()),
                preserve_repository_model: false,
            });
        };
        Self::from_explicit_file(path)
    }

    pub(super) fn from_opts_at(opts: &AnswerOpts, path_base: &Path) -> Result<Self> {
        let Some(path) = opts.answers_file.as_deref() else {
            return Ok(Self {
                raw: RawAnswers::default(),
                shape: AnswerInputShape::default(),
                authored_repository_commands: Some(BTreeMap::new()),
                preserve_repository_model: false,
            });
        };
        let path = if path.is_absolute() {
            path.to_path_buf()
        } else {
            path_base.join(path)
        };
        Self::from_explicit_file(&path)
    }

    pub(super) fn from_file(path: &Path) -> Result<Self> {
        let text = fs::read_to_string(path)
            .with_context(|| format!("Failed to read {}", path.display()))?;
        let value = toml::from_str::<toml::Value>(&text)
            .with_context(|| format!("Failed to parse {}", path.display()))?;
        let table = value
            .as_table()
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("Failed to parse {} as TOML table", path.display()))?;
        let mut raw = value
            .try_into::<RawAnswers>()
            .with_context(|| format!("Failed to parse {}", path.display()))?;
        raw.normalize_repository_model(&table);
        raw.normalize_app_dirs()?;
        raw.normalize_legacy_frontend_metadata(&table);
        let authored_repository_commands = authored_repository_commands_from_table(&table);
        let preserve_repository_model =
            loaded_repository_model_is_custom(&raw, authored_repository_commands.as_ref());
        Ok(Self {
            raw,
            shape: AnswerInputShape::from_table(&table),
            authored_repository_commands,
            preserve_repository_model,
        })
    }

    fn from_explicit_file(path: &Path) -> Result<Self> {
        let mut input = Self::from_file(path)?;
        if input
            .raw
            .repository
            .as_ref()
            .is_some_and(AuthoredRepositoryModel::is_complete)
            && input.authored_repository_commands.is_none()
        {
            bail!(
                "A complete authored [repository] model requires [commands] to be a table of string values"
            );
        }
        input.preserve_repository_model = true;
        Ok(input)
    }

    pub(super) const fn shape(&self) -> &AnswerInputShape {
        &self.shape
    }

    pub(super) fn preferred_rendered_command_keys(&self, cli: &AnswerOpts) -> BTreeSet<String> {
        let mut keys = BTreeSet::new();
        for (answer_key, command_key, cli_supplied) in [
            (
                "bootstrap_command",
                "repo_bootstrap_command",
                cli.bootstrap_command.is_some(),
            ),
            (
                "rust_fmt_check_command",
                "api_fmt_command",
                cli.rust_fmt_check_command.is_some(),
            ),
            (
                "rust_clippy_command",
                "api_clippy_command",
                cli.rust_clippy_command.is_some(),
            ),
            (
                "rust_test_command",
                "api_test_command",
                cli.rust_test_command.is_some(),
            ),
            (
                "rust_test_locked_command",
                "api_test_locked_command",
                cli.rust_test_locked_command.is_some(),
            ),
            (
                "sqlx_check_command",
                "api_sqlx_command",
                cli.sqlx_check_command.is_some(),
            ),
            (
                "schema_dump_command",
                "api_schema_dump_command",
                cli.schema_dump_command.is_some(),
            ),
            ("go_fmt_check_command", "api_fmt_command", false),
            ("go_lint_command", "api_lint_command", false),
            ("go_test_command", "api_test_command", false),
            ("go_test_locked_command", "api_test_locked_command", false),
            ("sqlc_check_command", "api_sqlc_command", false),
            (
                "typescript_lint_command",
                "repo_compat_typescript_lint_command",
                false,
            ),
            (
                "typescript_typecheck_command",
                "repo_compat_typescript_typecheck_command",
                false,
            ),
            (
                "typescript_build_command",
                "repo_compat_typescript_build_command",
                false,
            ),
            (
                "typescript_coverage_command",
                "repo_compat_typescript_coverage_command",
                false,
            ),
        ] {
            if cli_supplied || self.shape.contains_key(answer_key) {
                keys.insert(command_key.to_owned());
            }
        }
        keys
    }

    pub(super) fn effective_opts(&self, cli: &AnswerOpts) -> Result<AnswerOpts> {
        let mut raw = self.raw.clone();
        raw.merge_opts(cli);
        raw.normalize_app_dirs()?;
        let scaffold_go_component_roots = raw
            .repository
            .as_ref()
            .map(AuthoredRepositoryModel::scaffold_go_component_roots)
            .unwrap_or_default();
        let mut answers = raw.into_answer_opts(cli.answers_file.clone());
        answers.scaffold_go_component_roots = scaffold_go_component_roots;
        Ok(answers)
    }
}

impl AnswerInputShape {
    pub(super) fn from_table(table: &toml::Table) -> Self {
        let mut keys = table.keys().cloned().collect::<BTreeSet<_>>();
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
        let AnswerInput {
            mut raw,
            authored_repository_commands,
            preserve_repository_model,
            ..
        } = input;
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
        let mut notes = Vec::new();
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
        let authored_repository_commands = authored_repository_commands(path)?;
        let mut raw = RawAnswers::from_file(path)?;
        raw.normalize_legacy_sqlx_disabled_schema_dump();
        raw.normalize_legacy_generated_cargo_command_defaults();
        let mut answers = resolve_render_answers(raw, None, authored_repository_commands, true)?;
        answers.go_postgres_integration_script = path
            .parent()
            .is_some_and(has_go_postgres_integration_script);
        Ok(answers)
    }

    pub(super) const fn authored_repository(&self) -> Option<&AuthoredRepositoryModel> {
        self.authored_repository.as_ref()
    }

    pub(super) const fn authored_repository_commands(&self) -> &BTreeMap<String, String> {
        &self.authored_repository_commands
    }

    pub(super) fn default_branch(&self) -> &str {
        &self.default_branch
    }

    pub(super) fn template_source_url(&self) -> &str {
        &self.template_source_url
    }

    pub(super) fn frontend_apps(&self) -> &[FrontendApp] {
        &self.frontend_apps
    }

    pub(super) fn frontend_workspace_roots(&self) -> &[String] {
        &self.frontend_workspace_roots
    }

    pub(super) const fn harness_footprint(&self) -> HarnessFootprint {
        self.harness_footprint
    }

    pub(super) const fn backend_language(&self) -> BackendLanguage {
        self.backend_language
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
        self.managed_ci_target(tool::FMT_CHECK, &["rust", "sqlx"], "api:fmt")
    }

    pub(super) fn rust_clippy_ci_target(&self) -> Option<String> {
        self.managed_ci_target(tool::CLIPPY, &["rust", "sqlx"], "api:clippy")
    }

    pub(super) fn rust_test_locked_ci_target(&self) -> Option<String> {
        self.managed_ci_target(tool::TEST_LOCKED, &["rust", "sqlx"], "api:test-locked")
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

fn inherit_repository_command(destination: &mut Option<String>, commands: &toml::Table, key: &str) {
    if destination.is_none() {
        *destination = commands
            .get(key)
            .and_then(toml::Value::as_str)
            .map(str::to_owned);
    }
}

fn authored_repository_commands(path: &Path) -> Result<Option<BTreeMap<String, String>>> {
    let text =
        fs::read_to_string(path).with_context(|| format!("Failed to read {}", path.display()))?;
    let value = toml::from_str::<toml::Value>(&text)
        .with_context(|| format!("Failed to parse {}", path.display()))?;
    let table = value
        .as_table()
        .ok_or_else(|| anyhow::anyhow!("Failed to parse {} as TOML table", path.display()))?;
    Ok(authored_repository_commands_from_table(table))
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
    let Ok(generated_answers) = generated_raw.resolve_with_authored_repository(None, None) else {
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
    let mut answers =
        raw.resolve_with_authored_repository(default_repo_name, authored_repository)?;
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

pub(super) fn validate_frontend_apps(apps: &[FrontendApp]) -> Result<()> {
    let mut names = HashSet::new();
    for app in apps {
        if !is_safe_frontend_app_name(&app.name) {
            bail!(
                "Invalid frontend app name '{}'. Use ASCII letters, numbers, '-' or '_'.",
                app.name
            );
        }
        frontend_component_id(&app.name)?;
        if !names.insert(app.name.as_str()) {
            bail!("Duplicate frontend app name '{}'", app.name);
        }
        if !is_supported_frontend_app_kind(&app.kind) {
            bail!(
                "Invalid frontend app kind '{}'. Expected 'vite' or 'env-port'.",
                app.kind
            );
        }
        if !is_supported_frontend_app_role(&app.role) {
            bail!(
                "Invalid frontend app role '{}'. Expected 'spa', 'admin', or 'astro'.",
                app.role
            );
        }
        validate_frontend_app_dir(&app.name, &app.dir)?;
    }
    Ok(())
}

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

fn normalize_generated_gate_root(value: &str, label: &str) -> Result<String> {
    let normalized = normalize_portable_repo_path(value, label)?;
    if normalized.chars().any(|character| {
        character.is_control() || matches!(character, '*' | '?' | '[' | ']' | '{' | '}')
    }) {
        bail!(
            "{label} '{value}' cannot be represented safely as a literal generated gate path; control characters and glob metacharacters (*, ?, [, ], {{, }}) are unsupported"
        );
    }
    let pattern = if normalized == "." {
        "**".to_string()
    } else {
        format!("{normalized}/**")
    };
    validate_gate_path_pattern("generated-policy", label, &pattern).with_context(|| {
        format!("{label} '{value}' cannot be represented safely as a generated gate path")
    })?;
    Ok(normalized)
}

pub(super) fn frontend_gate_key(name: &str) -> String {
    name.to_ascii_lowercase().replace('-', "_")
}

mod serialization;
use serialization::*;

mod dev;
#[cfg(test)]
#[path = "answers_tests.rs"]
mod tests;
mod vault;
