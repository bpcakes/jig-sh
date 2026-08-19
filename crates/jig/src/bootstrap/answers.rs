// agentic-loc-exception: legacy answer normalization remains centralized during contract-v4 rollout.
use std::collections::{BTreeSet, HashSet};
use std::fs;
use std::path::{Component, Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

use super::{
    AnswerOpts, DevApp, FrontendApp, GENERATED_NODE_VERSION, generated_package_manager_spec,
    generated_package_manager_version,
};
use crate::context::{
    DEFAULT_CODEX_MARKETPLACE_ID, DEFAULT_CODEX_MARKETPLACE_SOURCE, StatusConfig,
    config_app_dirs_match, default_codex_marketplace_plugins, normalize_config_app_dir,
    validate_web_package_manager,
};
use crate::frontend_metadata::resolve_frontend_metadata;
use crate::shell::quote as shell_quote;

mod dev;
mod vault;

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum HarnessFootprint {
    #[default]
    Full,
    Minimal,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum BackendLanguage {
    #[default]
    Rust,
    Go,
}

impl BackendLanguage {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Rust => "rust",
            Self::Go => "go",
        }
    }
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
    #[serde(serialize_with = "serialize_backend_language")]
    backend_language: BackendLanguage,
    go_database: String,
    sqlx_enabled: bool,
    rust_crate_roots: Vec<String>,
    rust_migration_dir: Option<String>,
    migration_dir: Option<String>,
    rust_sqlx_metadata_dir: Option<String>,
    schema_dump_enabled: bool,
    schema_dump_command: String,
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
    web_package_manager: String,
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
    generated_frontend_dev_apps: Vec<FrontendApp>,
    vault: vault::VaultAnswers,
    status: StatusConfig,
    agent_tooling: AgentToolingAnswers,
}

pub(super) struct AnswerResolution {
    answers: RenderAnswers,
    notes: Vec<String>,
}

pub(super) struct AnswerInput {
    raw: RawAnswers,
    shape: AnswerInputShape,
}

#[derive(Clone, Debug, Default)]
pub(super) struct AnswerInputShape {
    keys: BTreeSet<String>,
    sqlx_enabled: Option<bool>,
    schema_dump_enabled: Option<bool>,
}

const SQLX_SHAPED_ANSWER_KEYS: &[&str] = &[
    "rust_migration_dir",
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
            });
        };
        Self::from_file(path)
    }

    pub(super) fn from_opts_at(opts: &AnswerOpts, path_base: &Path) -> Result<Self> {
        let Some(path) = opts.answers_file.as_deref() else {
            return Ok(Self {
                raw: RawAnswers::default(),
                shape: AnswerInputShape::default(),
            });
        };
        let path = if path.is_absolute() {
            path.to_path_buf()
        } else {
            path_base.join(path)
        };
        Self::from_file(&path)
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
        raw.normalize_app_dirs()?;
        raw.normalize_legacy_frontend_metadata(&table);
        Ok(Self {
            raw,
            shape: AnswerInputShape::from_table(&table),
        })
    }

    pub(super) const fn shape(&self) -> &AnswerInputShape {
        &self.shape
    }

    pub(super) fn effective_opts(&self, cli: &AnswerOpts) -> Result<AnswerOpts> {
        let mut raw = self.raw.clone();
        raw.merge_opts(cli);
        raw.normalize_app_dirs()?;
        Ok(raw.into_answer_opts(cli.answers_file.clone()))
    }
}

impl AnswerInputShape {
    pub(super) fn from_table(table: &toml::Table) -> Self {
        let mut keys = table.keys().cloned().collect::<BTreeSet<_>>();
        if table
            .get("rust_migration_dir")
            .and_then(toml::Value::as_str)
            == Some("")
        {
            keys.remove("rust_migration_dir");
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
        let mut raw = input.raw;
        raw.merge_opts(opts);
        let vault_note = raw.apply_existing_vault_default(destination)?;
        let sqlx_defaulted_to_tooling_only = if use_defaults {
            raw.apply_sqlx_default_for_cli_defaults()
        } else {
            false
        };
        let answers = raw.resolve(default_repo_name(destination))?;
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
        let mut raw = RawAnswers::from_file(path)?;
        raw.normalize_legacy_sqlx_disabled_schema_dump();
        raw.normalize_legacy_generated_cargo_command_defaults();
        raw.resolve(None)
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

    pub(super) const fn harness_footprint(&self) -> HarnessFootprint {
        self.harness_footprint
    }

    pub(super) const fn backend_language(&self) -> BackendLanguage {
        self.backend_language
    }

    pub(super) fn is_minimal_footprint(&self) -> bool {
        self.harness_footprint == HarnessFootprint::Minimal
    }

    pub(super) fn frontend_harness_enabled(&self) -> bool {
        !self.is_minimal_footprint() && !self.frontend_apps.is_empty()
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

    pub(super) fn web_package_manager(&self) -> &str {
        &self.web_package_manager
    }

    pub(super) fn bootstrap_command_configured(&self) -> bool {
        !self.bootstrap_command.trim().is_empty()
    }

    pub(super) const fn has_legacy_dev_command(&self) -> bool {
        self.legacy_dev_command.is_some()
    }
}

#[derive(Clone, Debug, Default, Deserialize)]
struct RawAnswers {
    repo_name: Option<String>,
    go_module: Option<String>,
    default_branch: Option<String>,
    ci_github_runner: Option<String>,
    jig_version: Option<String>,
    template_source_url: Option<String>,
    #[serde(default)]
    harness_footprint: Option<HarnessFootprint>,
    backend_language: Option<BackendLanguage>,
    go_database: Option<String>,
    sqlx_enabled: Option<bool>,
    rust_crate_roots: Option<Vec<String>>,
    rust_migration_dir: Option<String>,
    rust_sqlx_metadata_dir: Option<String>,
    schema_dump_enabled: Option<bool>,
    schema_dump_command: Option<String>,
    schema_check_command: Option<String>,
    sqlx_check_command: Option<String>,
    migration_add_command: Option<String>,
    bootstrap_command: Option<String>,
    contract_check_command: Option<String>,
    dev_command: Option<String>,
    rust_fmt_check_command: Option<String>,
    rust_clippy_command: Option<String>,
    rust_test_command: Option<String>,
    rust_test_locked_command: Option<String>,
    web_package_manager: Option<String>,
    frontend_apps: Option<Vec<FrontendApp>>,
    dev: Option<dev::RawDevAnswers>,
    vault: Option<vault::VaultAnswers>,
    status: Option<StatusConfig>,
    agent_tooling: Option<AgentToolingAnswers>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
struct AgentToolingAnswers {
    #[serde(default)]
    codex: CodexToolingAnswers,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct CodexToolingAnswers {
    #[serde(default = "default_codex_marketplaces")]
    marketplaces: Vec<CodexMarketplaceAnswers>,
}

impl Default for CodexToolingAnswers {
    fn default() -> Self {
        Self {
            marketplaces: default_codex_marketplaces(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct CodexMarketplaceAnswers {
    id: String,
    source: String,
    #[serde(default)]
    plugins: Vec<String>,
}

impl RawAnswers {
    fn from_file(path: &Path) -> Result<Self> {
        let text = fs::read_to_string(path)
            .with_context(|| format!("Failed to read {}", path.display()))?;
        let value = toml::from_str::<toml::Value>(&text)
            .with_context(|| format!("Failed to parse {}", path.display()))?;
        let table = value
            .as_table()
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("Failed to parse {} as TOML table", path.display()))?;
        let mut raw = value
            .try_into::<Self>()
            .with_context(|| format!("Failed to parse {}", path.display()))?;
        raw.normalize_app_dirs()?;
        raw.normalize_legacy_frontend_metadata(&table);
        Ok(raw)
    }

    fn normalize_legacy_frontend_metadata(&mut self, table: &toml::Table) {
        let Some(frontend_apps) = self.frontend_apps.as_mut() else {
            return;
        };
        let Some(frontend_tables) = table.get("frontend_apps").and_then(toml::Value::as_array)
        else {
            return;
        };
        let dev_apps = self
            .dev
            .as_ref()
            .and_then(|dev| dev.apps.as_deref())
            .unwrap_or_default();

        for (frontend, source) in frontend_apps.iter_mut().zip(frontend_tables) {
            let Some(source) = source.as_table() else {
                continue;
            };
            let configured_kind = source.get("kind").and_then(toml::Value::as_str);
            let configured_role = source.get("role").and_then(toml::Value::as_str);
            let matching_dev_kind = if configured_kind.is_none() {
                dev_apps
                    .iter()
                    .find(|dev_app| {
                        dev_app.name == frontend.name
                            && dev_app.dir.as_deref().is_some_and(|dev_dir| {
                                config_app_dirs_match(dev_dir, &frontend.dir)
                            })
                    })
                    .map(|dev_app| dev_app.kind.as_str())
            } else {
                None
            };
            let metadata = resolve_frontend_metadata(
                &frontend.name,
                configured_kind,
                configured_role,
                matching_dev_kind,
            );
            frontend.kind = metadata.kind.into();
            frontend.role = metadata.role.into();
        }
    }

    fn merge_opts(&mut self, opts: &AnswerOpts) {
        merge_option(&mut self.repo_name, opts.repo_name.clone());
        merge_option(&mut self.go_module, opts.go_module.clone());
        merge_option(&mut self.default_branch, opts.default_branch.clone());
        merge_option(&mut self.ci_github_runner, opts.ci_github_runner.clone());
        merge_option(&mut self.jig_version, opts.jig_version.clone());
        merge_option(
            &mut self.template_source_url,
            opts.template_source_url.clone(),
        );
        merge_option(&mut self.harness_footprint, opts.harness_footprint);
        merge_option(&mut self.backend_language, opts.backend_language);
        merge_option(&mut self.go_database, opts.go_database.clone());
        merge_option(&mut self.sqlx_enabled, opts.sqlx_enabled);
        if !opts.rust_crate_roots.is_empty() {
            self.rust_crate_roots = Some(opts.rust_crate_roots.clone());
        }
        merge_option(
            &mut self.rust_migration_dir,
            opts.rust_migration_dir.clone(),
        );
        merge_option(
            &mut self.rust_sqlx_metadata_dir,
            opts.rust_sqlx_metadata_dir.clone(),
        );
        merge_option(&mut self.schema_dump_enabled, opts.schema_dump_enabled);
        merge_option(
            &mut self.schema_dump_command,
            opts.schema_dump_command.clone(),
        );
        merge_option(
            &mut self.schema_check_command,
            opts.schema_check_command.clone(),
        );
        merge_option(
            &mut self.sqlx_check_command,
            opts.sqlx_check_command.clone(),
        );
        merge_option(
            &mut self.migration_add_command,
            opts.migration_add_command.clone(),
        );
        merge_option(&mut self.bootstrap_command, opts.bootstrap_command.clone());
        merge_option(
            &mut self.contract_check_command,
            opts.contract_check_command.clone(),
        );
        merge_option(&mut self.dev_command, opts.dev_command.clone());
        merge_option(
            &mut self.rust_fmt_check_command,
            opts.rust_fmt_check_command.clone(),
        );
        merge_option(
            &mut self.rust_clippy_command,
            opts.rust_clippy_command.clone(),
        );
        merge_option(&mut self.rust_test_command, opts.rust_test_command.clone());
        merge_option(
            &mut self.rust_test_locked_command,
            opts.rust_test_locked_command.clone(),
        );
        merge_option(
            &mut self.web_package_manager,
            opts.web_package_manager.clone(),
        );
        if !opts.frontend_apps.is_empty() {
            self.frontend_apps = Some(opts.frontend_apps.clone());
        }
        if !opts.dev_apps.is_empty() {
            self.dev
                .get_or_insert_with(dev::RawDevAnswers::default)
                .apps = Some(opts.dev_apps.clone());
        }
        merge_option(&mut self.status, opts.status.clone());
    }

    fn normalize_app_dirs(&mut self) -> Result<()> {
        if let Some(frontend_apps) = self.frontend_apps.as_mut() {
            for app in frontend_apps {
                app.dir = normalize_config_app_dir(
                    &app.dir,
                    &format!("frontend app '{}' dir", app.name),
                )?;
            }
        }
        if let Some(dev_apps) = self.dev.as_mut().and_then(|dev| dev.apps.as_mut()) {
            for app in dev_apps {
                if let Some(dir) = app.dir.as_mut() {
                    *dir = normalize_config_app_dir(dir, &format!("dev app '{}' dir", app.name))?;
                }
            }
        }
        Ok(())
    }

    fn into_answer_opts(self, answers_file: Option<PathBuf>) -> AnswerOpts {
        let dev_apps = self.dev.and_then(|dev| dev.apps).unwrap_or_default();
        AnswerOpts {
            answers_file,
            repo_name: self.repo_name.filter(|value| !value.is_empty()),
            go_module: self.go_module.filter(|value| !value.is_empty()),
            default_branch: self.default_branch,
            ci_github_runner: self.ci_github_runner,
            jig_version: self.jig_version,
            template_source_url: self.template_source_url,
            harness_footprint: self.harness_footprint,
            backend_language: self.backend_language,
            go_database: self.go_database,
            sqlx_enabled: self.sqlx_enabled,
            rust_crate_roots: self.rust_crate_roots.unwrap_or_default(),
            rust_migration_dir: self.rust_migration_dir.filter(|value| !value.is_empty()),
            rust_sqlx_metadata_dir: self.rust_sqlx_metadata_dir,
            schema_dump_enabled: self.schema_dump_enabled,
            schema_dump_command: self.schema_dump_command,
            schema_check_command: self.schema_check_command,
            sqlx_check_command: self.sqlx_check_command,
            migration_add_command: self.migration_add_command,
            bootstrap_command: self.bootstrap_command,
            contract_check_command: self.contract_check_command,
            dev_command: self.dev_command,
            rust_fmt_check_command: self.rust_fmt_check_command,
            rust_clippy_command: self.rust_clippy_command,
            rust_test_command: self.rust_test_command,
            rust_test_locked_command: self.rust_test_locked_command,
            web_package_manager: self.web_package_manager,
            frontend_apps: self.frontend_apps.unwrap_or_default(),
            dev_apps,
            status: self.status,
        }
    }

    fn normalize_legacy_sqlx_disabled_schema_dump(&mut self) {
        if self.sqlx_enabled == Some(false) && self.schema_dump_enabled == Some(true) {
            self.schema_dump_enabled = Some(false);
        }
    }

    fn normalize_legacy_generated_cargo_command_defaults(&mut self) {
        let sqlx_metadata_dir = self.rust_sqlx_metadata_dir.as_deref().unwrap_or(".sqlx");
        let legacy_sqlx_check_command = format!(
            "SQLX_OFFLINE=false SQLX_OFFLINE_DIR={} cargo sqlx prepare --check --workspace -- --workspace --all-targets",
            shell_quote(sqlx_metadata_dir)
        );
        normalize_legacy_command_default(&mut self.sqlx_check_command, &legacy_sqlx_check_command);
        normalize_legacy_command_default(&mut self.bootstrap_command, "cargo fetch");
        normalize_legacy_command_default(
            &mut self.rust_fmt_check_command,
            "cargo fmt --all -- --check",
        );
        normalize_legacy_command_default(
            &mut self.rust_clippy_command,
            "cargo clippy --workspace --all-targets --locked -- -D warnings",
        );
        normalize_legacy_command_default(&mut self.rust_test_command, "cargo test --workspace");
        normalize_legacy_command_default(
            &mut self.rust_test_locked_command,
            "cargo test --workspace --locked",
        );
    }

    fn apply_sqlx_default_for_cli_defaults(&mut self) -> bool {
        // CLI `--defaults` should not block on optional feature setup. Without
        // a migration dir, resolve to the tooling-only profile instead of
        // making noninteractive adoption stop for SQLx configuration.
        if self.sqlx_enabled.is_none()
            && self.rust_migration_dir.as_deref().is_none_or(str::is_empty)
            && self.schema_dump_enabled != Some(true)
        {
            self.sqlx_enabled = Some(false);
            return true;
        }
        false
    }

    fn apply_existing_vault_default(&mut self, destination: &Path) -> Result<Option<String>> {
        if self.vault.is_some() {
            return Ok(None);
        }
        vault::apply_existing_default(&mut self.vault, destination)
    }

    fn resolve(mut self, default_repo_name: Option<String>) -> Result<RenderAnswers> {
        self.normalize_app_dirs()?;
        let backend_language = self.backend_language.unwrap_or_default();
        let go_database = self.go_database.unwrap_or_else(|| "none".into());
        if backend_language == BackendLanguage::Go
            && !matches!(go_database.as_str(), "none" | "postgres")
        {
            bail!("Invalid go_database '{go_database}'. Expected 'none' or 'postgres'");
        }
        let repo_name = self
            .repo_name
            .filter(|value| !value.is_empty())
            .or(default_repo_name)
            .ok_or_else(|| anyhow::anyhow!("Missing required answer: repo_name"))?;
        let sqlx_enabled = self.sqlx_enabled.unwrap_or(true);
        let rust_migration_dir = self.rust_migration_dir.filter(|value| !value.is_empty());
        if sqlx_enabled && rust_migration_dir.is_none() {
            bail!(
                "Missing required answer when sqlx_enabled is true (including when schema_dump_enabled implies SQLx): rust_migration_dir. Pass --rust-migration-dir <dir> for SQLx repos, or pass --sqlx-enabled false with schema_dump_enabled = false for tooling-only repos."
            );
        }
        if !sqlx_enabled && self.schema_dump_enabled == Some(true) {
            bail!(
                "schema_dump_enabled cannot be true when sqlx_enabled is false; enable SQLx or set schema_dump_enabled = false"
            );
        }

        let frontend_apps = self.frontend_apps.unwrap_or_default();
        validate_frontend_apps(&frontend_apps)?;
        let dev::ResolvedDevApps {
            dev_apps,
            generated_frontend_dev_apps,
        } = dev::resolve(frontend_apps.as_slice(), self.dev)?;
        let vault = self.vault.unwrap_or_else(vault::default_answers);
        vault::validate_answers(&vault)?;
        let status = self.status.unwrap_or_default();
        status.validate()?;
        let legacy_dev_command = self.dev_command.filter(|value| !value.trim().is_empty());

        let web_package_manager = self.web_package_manager.unwrap_or_else(|| "bun".into());
        validate_web_package_manager(&web_package_manager)?;
        let web_install_command = web_install_command(&web_package_manager).to_string();
        let web_run_command = web_run_command(&web_package_manager).to_string();
        let web_package_manager_spec = generated_package_manager_spec(&web_package_manager).into();
        let web_package_manager_version =
            generated_package_manager_version(&web_package_manager).into();
        let schema_dump_command_configured = self.schema_dump_command.is_some();
        let schema_dump_enabled = if sqlx_enabled {
            self.schema_dump_enabled
                .unwrap_or(schema_dump_command_configured)
        } else {
            false
        };
        let schema_dump_command = self
            .schema_dump_command
            .unwrap_or_else(|| "scripts/dump-schema.sh".into());
        let rust_sqlx_metadata_dir = self.rust_sqlx_metadata_dir.or_else(|| Some(".sqlx".into()));
        let sqlx_check_command = self.sqlx_check_command.unwrap_or_else(|| {
            let metadata_dir = rust_sqlx_metadata_dir.as_deref().unwrap_or(".sqlx");
            format!(
                "CARGO=cargo SQLX_OFFLINE=false SQLX_OFFLINE_DIR={} sqlx prepare --check --workspace -- --workspace --all-targets",
                shell_quote(metadata_dir)
            )
        });
        let migration_add_command = self.migration_add_command;
        let migration_dir = if backend_language == BackendLanguage::Go && go_database == "postgres"
        {
            Some("internal/database/migrations".into())
        } else {
            rust_migration_dir.clone()
        };

        Ok(RenderAnswers {
            repo_name,
            default_branch: self.default_branch.unwrap_or_else(|| "main".into()),
            ci_github_runner: self
                .ci_github_runner
                .unwrap_or_else(|| "ubuntu-latest".into()),
            legacy_template_jig_version: self
                .jig_version
                .unwrap_or_else(|| env!("CARGO_PKG_VERSION").into()),
            template_source_url: self.template_source_url.unwrap_or_default(),
            harness_footprint: self.harness_footprint.unwrap_or_default(),
            backend_language,
            go_database,
            sqlx_enabled,
            rust_crate_roots: if backend_language == BackendLanguage::Go {
                Vec::new()
            } else {
                self.rust_crate_roots
                    .unwrap_or_else(|| vec!["crates".into()])
            },
            rust_migration_dir,
            migration_dir,
            rust_sqlx_metadata_dir,
            schema_dump_enabled,
            schema_dump_command,
            schema_check_command: self.schema_check_command.unwrap_or_default(),
            sqlx_check_command,
            migration_add_command,
            bootstrap_command: self
                .bootstrap_command
                .unwrap_or_else(|| optional_cargo_command("cargo fetch", "bootstrap")),
            contract_check_command: self.contract_check_command.unwrap_or_default(),
            legacy_dev_command,
            rust_fmt_check_command: self
                .rust_fmt_check_command
                .unwrap_or_else(|| optional_cargo_command("cargo fmt --all -- --check", "fmt")),
            rust_clippy_command: self.rust_clippy_command.unwrap_or_else(|| {
                optional_cargo_command(
                    "cargo clippy --workspace --all-targets --locked -- -D warnings",
                    "clippy",
                )
            }),
            rust_test_command: self
                .rust_test_command
                .unwrap_or_else(|| optional_cargo_command("cargo test --workspace", "test")),
            rust_test_locked_command: self.rust_test_locked_command.unwrap_or_else(|| {
                optional_cargo_command("cargo test --workspace --locked", "test-locked")
            }),
            go_fmt_check_command: "files=$(find . -type f -name '*.go' -not -path './.git/*' -exec gofmt -l {} +); test -z \"$files\" || { printf '%s\\n' \"$files\"; exit 1; }".into(),
            go_lint_command: "go vet ./...".into(),
            go_test_command: "go test ./...".into(),
            go_test_locked_command: "go mod verify && go test -mod=readonly ./...".into(),
            web_package_manager,
            web_package_manager_spec,
            web_package_manager_version,
            node_version: GENERATED_NODE_VERSION.into(),
            web_install_command,
            web_run_command,
            typescript_lint_command: "scripts/check-webapps.sh lint".into(),
            typescript_typecheck_command: "scripts/check-webapps.sh typecheck".into(),
            typescript_build_command: "scripts/check-webapps.sh build".into(),
            typescript_coverage_command: "scripts/check-webapps.sh coverage".into(),
            dev_apps,
            generated_frontend_dev_apps,
            frontend_apps,
            vault,
            status,
            agent_tooling: self.agent_tooling.unwrap_or_default(),
        })
    }
}

fn answer_opts_has_sqlx_shape(answers: &AnswerOpts) -> bool {
    SQLX_SHAPED_ANSWER_KEYS.iter().any(|key| match *key {
        "rust_migration_dir" => answers
            .rust_migration_dir
            .as_deref()
            .is_some_and(|value| !value.is_empty()),
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

fn serialize_harness_footprint<S>(
    value: &HarnessFootprint,
    serializer: S,
) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    serializer.serialize_str(value.as_str())
}

fn serialize_backend_language<S>(value: &BackendLanguage, serializer: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    serializer.serialize_str(value.as_str())
}

fn default_codex_marketplaces() -> Vec<CodexMarketplaceAnswers> {
    vec![CodexMarketplaceAnswers {
        id: DEFAULT_CODEX_MARKETPLACE_ID.into(),
        source: DEFAULT_CODEX_MARKETPLACE_SOURCE.into(),
        plugins: default_codex_marketplace_plugins(),
    }]
}

fn merge_option<T>(target: &mut Option<T>, value: Option<T>) {
    if let Some(value) = value {
        *target = Some(value);
    }
}

fn default_repo_name(destination: &Path) -> Option<String> {
    destination
        .file_name()
        .and_then(|name| name.to_str())
        .map(str::to_string)
        .filter(|value| !value.is_empty())
}

#[cfg(test)]
#[path = "answers_tests.rs"]
mod tests;
