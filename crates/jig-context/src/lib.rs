//! Repository context for Jig: the loaded `.jig.toml` configuration and
//! `.agent/jig-contract.json` manifest, their validation, and the repository
//! root and runtime-cache locations derived from them.

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use jig_contract::{
    ActionSpec, ComponentSpec, FeatureContext, ManifestTool, ProfileId, ProfileSpec,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::backend::{BackendLanguage, GO_TOOLCHAIN_AUTHORITY_PATH, GoDatabase};
use crate::frontend_metadata::{ResolvedFrontendMetadata, resolve_frontend_metadata};
use crate::repository_path::{
    normalize_portable_repo_path, normalize_portable_repository_directory,
    normalize_repo_relative_path, validate_repository_directory_path,
};

pub use defaults::{
    DEFAULT_CODEX_MARKETPLACE_ID, DEFAULT_CODEX_MARKETPLACE_PLUGINS,
    DEFAULT_CODEX_MARKETPLACE_SOURCE, SUPPORTED_WEB_PACKAGE_MANAGERS,
};
pub use execution_config::{CommandOutputLimit, CommandTimeout, MAX_COMMAND_TIMEOUT_SECONDS};
pub use optional::REPO_CONTEXT_NOT_FOUND;
use runtime::non_empty_legacy_jig_version;
#[cfg(any(test, feature = "test-support"))]
pub use runtime::{
    FALLBACK_RUNTIME_CACHE_BASE, GIT_RUNTIME_CACHE_BASE, RUNTIME_CACHE_PROFILE_SUFFIX,
};
pub use runtime::{
    JIG_REPO_ROOT_ENV, LAST_WORK_CONFIG_CONTRACT_VERSION, LAUNCHER_REPAIR_STAGING_PREFIX,
    RepoConfigProbe, RuntimeCacheProfile, WORK_CONFIG_RETIRED_CONTRACT_VERSION,
    active_contract_versions, active_contract_versions_label, is_active_contract_version,
    is_supported_contract_version, runtime_cache_base, runtime_profile_cache_name,
    runtime_profile_cache_path,
};

pub use execution_config::ExecutionConfig;
pub use inputs_policy::validate_inputs_policy;
pub use loop_config::{LoopConfig, LoopWorkflowConfig, parse_five_field_cron};
pub use migration::{MigrationBackend, RustMigrationLayout, native_migration_backend};
pub use vault_config::is_valid_vault_scope_id;
use vault_config::{VaultConfig, VaultScopeConfig};
pub use work_config::{WorkConfig, WorkEvidenceSelector, WorkGate, validate_gate_path_pattern};

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RepoConfig {
    #[serde(rename = "_src_path")]
    src_path: String,
    #[serde(rename = "_commit")]
    commit: String,
    #[allow(dead_code)]
    #[serde(default, rename = "_template_mode")]
    template_mode: String,
    #[allow(dead_code)]
    #[serde(default, rename = "_template_local_path")]
    template_local_path: String,
    repo_name: String,
    default_branch: String,
    #[allow(dead_code)]
    #[serde(default)]
    ci_github_runner: String,
    #[serde(default)]
    jig_version: Option<String>,
    #[allow(dead_code)]
    #[serde(default)]
    template_source_url: String,
    #[allow(dead_code)]
    #[serde(default)]
    harness_footprint: HarnessFootprintConfig,
    #[allow(dead_code)]
    #[serde(default)]
    backend_language: BackendLanguage,
    #[allow(dead_code)]
    #[serde(default)]
    go_database: GoDatabase,
    #[allow(dead_code)]
    #[serde(default)]
    sqlx_enabled: bool,
    #[allow(dead_code)]
    #[serde(default)]
    rust_crate_roots: Vec<String>,
    #[allow(dead_code)]
    #[serde(default)]
    rust_migration_dir: String,
    #[serde(default)]
    migration_dir: String,
    #[allow(dead_code)]
    #[serde(default)]
    rust_migration_layout: RustMigrationLayout,
    #[allow(dead_code)]
    #[serde(default)]
    rust_sqlx_metadata_dir: String,
    #[allow(dead_code)]
    #[serde(default)]
    schema_dump_enabled: bool,
    #[allow(dead_code)]
    #[serde(default)]
    schema_dump_command: String,
    #[serde(default = "default_schema_docs_dir")]
    schema_docs_dir: String,
    #[allow(dead_code)]
    #[serde(default)]
    schema_check_command: String,
    #[allow(dead_code)]
    #[serde(default)]
    sqlx_check_command: String,
    #[allow(dead_code)]
    #[serde(default)]
    migration_add_command: String,
    #[allow(dead_code)]
    #[serde(default)]
    bootstrap_command: String,
    #[allow(dead_code)]
    #[serde(default)]
    contract_check_command: String,
    #[allow(dead_code)]
    #[serde(default)]
    dev_command: String,
    #[allow(dead_code)]
    #[serde(default)]
    rust_fmt_check_command: String,
    #[allow(dead_code)]
    #[serde(default)]
    rust_clippy_command: String,
    #[allow(dead_code)]
    #[serde(default)]
    rust_test_command: String,
    #[allow(dead_code)]
    #[serde(default)]
    rust_test_locked_command: String,
    #[serde(default)]
    commands: BTreeMap<String, String>,
    #[serde(default = "default_web_package_manager")]
    web_package_manager: String,
    #[allow(dead_code)]
    #[serde(default)]
    application_contracts_enabled: bool,
    #[serde(default)]
    frontend_apps: Vec<FrontendAppConfig>,
    #[allow(dead_code)]
    #[serde(default)]
    frontend_workspace_roots: Vec<String>,
    #[serde(default)]
    repository: Option<AuthoredRepositoryConfig>,
    #[serde(default)]
    vault: VaultConfig,
    #[serde(default)]
    dev: DevConfig,
    /// Retired structured-work settings; only epochs through 8 may declare it.
    #[serde(default)]
    work: Option<WorkConfig>,
    #[serde(default, rename = "loop")]
    loop_config: LoopConfig,
    #[serde(default)]
    execution: execution_config::ExecutionConfig,
    #[serde(default)]
    agent_tooling: AgentToolingConfig,
}

type LegacyCommandAccessor = for<'a> fn(&'a RepoConfig) -> &'a str;

/// One source of truth for compatibility command fields. Both runtime command
/// resolution and the execution-authority digest consume this table so adding
/// or renaming a legacy binding cannot silently update only one boundary.
const LEGACY_COMMAND_BINDINGS: &[(&str, LegacyCommandAccessor)] = &[
    ("bootstrap_command", |config| &config.bootstrap_command),
    ("contract_check_command", |config| {
        &config.contract_check_command
    }),
    ("migration_add_command", |config| {
        &config.migration_add_command
    }),
    ("rust_clippy_command", |config| &config.rust_clippy_command),
    ("rust_fmt_check_command", |config| {
        &config.rust_fmt_check_command
    }),
    ("rust_test_command", |config| &config.rust_test_command),
    ("rust_test_locked_command", |config| {
        &config.rust_test_locked_command
    }),
    ("schema_check_command", |config| {
        &config.schema_check_command
    }),
    ("schema_dump_command", |config| &config.schema_dump_command),
    ("sqlx_check_command", |config| &config.sqlx_check_command),
];

fn legacy_command_for_key<'a>(config: &'a RepoConfig, key: &str) -> Option<&'a str> {
    LEGACY_COMMAND_BINDINGS
        .iter()
        .find(|(candidate, _)| *candidate == key)
        .map(|(_, accessor)| accessor(config))
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct AuthoredRepositoryConfig {
    default_check_profile: ProfileId,
    #[serde(default)]
    affected_ignore: Vec<String>,
    /// Issue-tracker state the repository keeps in its checkout. Checked
    /// commands never consume it, so it stays out of source identity.
    #[serde(default)]
    tracker: Option<RepositoryTracker>,
    components: Vec<ComponentSpec>,
    actions: Vec<ActionSpec>,
    profiles: Vec<ProfileSpec>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RepositoryTracker {
    Beads,
}

impl RepositoryTracker {
    pub const fn state_path(self) -> &'static str {
        match self {
            Self::Beads => ".beads",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum HarnessFootprintConfig {
    #[default]
    Full,
    Minimal,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FrontendAppConfig {
    pub name: String,
    pub dir: String,
    #[allow(dead_code)]
    #[serde(default)]
    pub coverage_threshold: u32,
    #[serde(default)]
    pub kind: Option<String>,
    #[serde(default)]
    pub role: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DevConfig {
    #[serde(default = "default_proxy_http_port")]
    pub proxy_port: u16,
    #[serde(default = "default_proxy_https_port")]
    pub https_port: Option<u16>,
    #[serde(default)]
    pub https: bool,
    #[serde(default = "default_true")]
    pub http2: bool,
    #[serde(default)]
    pub lan: bool,
    #[serde(default = "default_dev_tld")]
    pub tld: String,
    #[serde(default)]
    pub workspace_discovery: bool,
    #[serde(default)]
    pub apps: Vec<DevAppConfig>,
}

impl Default for DevConfig {
    fn default() -> Self {
        Self {
            proxy_port: default_proxy_http_port(),
            https_port: default_proxy_https_port(),
            https: false,
            http2: true,
            lan: false,
            tld: default_dev_tld(),
            workspace_discovery: false,
            apps: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DevAppConfig {
    pub name: String,
    #[serde(default)]
    pub dir: Option<String>,
    #[serde(default = "default_dev_app_kind")]
    pub kind: String,
    #[serde(default)]
    pub command: Option<String>,
    #[serde(default)]
    pub argv: Vec<String>,
    #[serde(default)]
    pub port: Option<u16>,
    #[serde(default)]
    pub host: Option<String>,
    #[serde(default = "default_true")]
    pub proxy: bool,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentToolingConfig {
    #[serde(default)]
    pub codex: CodexToolingConfig,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CodexToolingConfig {
    #[serde(default = "default_codex_marketplaces")]
    pub marketplaces: Vec<CodexMarketplaceConfig>,
}

impl Default for CodexToolingConfig {
    fn default() -> Self {
        Self {
            marketplaces: default_codex_marketplaces(),
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CodexMarketplaceConfig {
    pub id: String,
    pub source: String,
    #[serde(default)]
    pub plugins: Vec<String>,
}

#[derive(Clone, Debug, Deserialize)]
struct ContractManifest {
    contract_version: u32,
    tool_namespace: String,
    #[serde(default)]
    jig_version: Option<String>,
    #[allow(dead_code)]
    #[serde(default)]
    required_commands: Vec<String>,
    #[serde(default)]
    tools: Vec<ManifestTool>,
    #[serde(default)]
    components: Vec<ComponentSpec>,
    #[serde(default)]
    actions: Vec<ActionSpec>,
    #[serde(default)]
    profiles: Vec<ProfileSpec>,
    #[serde(default)]
    default_check_profile: Option<ProfileId>,
    #[serde(default)]
    affected_ignore: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct RepoContext {
    root: PathBuf,
    config: RepoConfig,
    manifest: ContractManifest,
    contract_digest: String,
    configuration_content_digests: [String; 2],
}

mod loading;

impl RepoContext {
    pub fn tool_specs(&self) -> &[ManifestTool] {
        &self.manifest.tools
    }

    pub const fn contract_version(&self) -> u32 {
        self.manifest.contract_version
    }

    pub fn required_commands(&self) -> &[String] {
        &self.manifest.required_commands
    }

    pub fn tool_spec(&self, name: &str) -> Option<&ManifestTool> {
        self.manifest.tools.iter().find(|tool| tool.name == name)
    }

    pub fn component_specs(&self) -> &[ComponentSpec] {
        &self.manifest.components
    }

    pub fn action_specs(&self) -> &[ActionSpec] {
        &self.manifest.actions
    }

    pub fn profile_specs(&self) -> &[ProfileSpec] {
        &self.manifest.profiles
    }

    pub fn default_check_profile(&self) -> Option<&ProfileId> {
        self.manifest.default_check_profile.as_ref()
    }

    pub fn affected_ignore(&self) -> &[String] {
        &self.manifest.affected_ignore
    }

    pub fn contract_digest(&self) -> &str {
        &self.contract_digest
    }

    pub fn configuration_content_digests(&self) -> &[String; 2] {
        &self.configuration_content_digests
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn repo_name(&self) -> &str {
        &self.config.repo_name
    }

    pub fn default_branch(&self) -> &str {
        &self.config.default_branch
    }

    pub fn is_go_backend(&self) -> bool {
        if self.contract_version() >= 6 {
            self.has_component_adapter("go")
        } else {
            self.config.backend_language.is_go()
        }
    }

    pub fn legacy_jig_version(&self) -> Option<&str> {
        self.config.jig_version.as_deref()
    }

    pub fn is_minimal_footprint(&self) -> bool {
        self.config.harness_footprint == HarnessFootprintConfig::Minimal
    }

    pub fn sqlx_enabled(&self) -> bool {
        if self.contract_version() >= 6 {
            self.has_component_adapter("sqlx")
        } else {
            self.config.sqlx_enabled
        }
    }

    pub const fn schema_dump_enabled(&self) -> bool {
        self.config.schema_dump_enabled
    }

    pub fn schema_docs_dir(&self) -> &str {
        &self.config.schema_docs_dir
    }

    pub fn rust_crate_roots(&self) -> &[String] {
        &self.config.rust_crate_roots
    }

    pub fn component_root_path(&self, component: &ComponentSpec) -> Result<PathBuf> {
        let normalized = normalize_portable_repo_path(
            &component.root,
            &format!("component '{}' root", component.id),
        )?;
        Ok(if normalized == "." {
            self.root.clone()
        } else {
            self.root.join(normalized)
        })
    }

    pub fn go_module_authority_paths(&self) -> Result<Vec<PathBuf>> {
        if self.contract_version() < 6 {
            return Ok(self
                .is_go_backend()
                .then(|| self.root.join(GO_TOOLCHAIN_AUTHORITY_PATH))
                .into_iter()
                .collect());
        }
        self.component_specs()
            .iter()
            .filter(|component| component.adapters.iter().any(|adapter| adapter == "go"))
            .map(|component| -> Result<PathBuf> {
                let component_relative = normalize_portable_repo_path(
                    &component.root,
                    &format!("component '{}' root", component.id),
                )?;
                if component_relative != "."
                    && let Err(error) = validate_repository_directory_path(
                        &self.root,
                        Path::new(&component_relative),
                    )
                {
                    bail!(
                        "Go component '{}' root must use real repository directories: {error}",
                        component.id
                    );
                }
                let component_root = self.component_root_path(component)?;
                let mut module_root = component_root.clone();
                loop {
                    let authority = module_root.join(GO_TOOLCHAIN_AUTHORITY_PATH);
                    match fs::symlink_metadata(&authority) {
                        Ok(_) => break Ok(authority),
                        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                        Err(error) => {
                            return Err(error).with_context(|| {
                                format!(
                                    "Failed to inspect Go module authority {}",
                                    authority.display()
                                )
                            });
                        }
                    }
                    if module_root == self.root {
                        break Ok(component_root.join(GO_TOOLCHAIN_AUTHORITY_PATH));
                    }
                    if !module_root.pop() || !module_root.starts_with(&self.root) {
                        break Ok(component_root.join(GO_TOOLCHAIN_AUTHORITY_PATH));
                    }
                }
            })
            .collect::<Result<BTreeSet<_>>>()
            .map(|paths| paths.into_iter().collect())
    }

    pub fn migration_dir(&self) -> &str {
        if self.config.migration_dir.trim().is_empty() {
            &self.config.rust_migration_dir
        } else {
            &self.config.migration_dir
        }
    }

    pub fn migration_relative_dir(&self) -> Result<PathBuf> {
        normalize_repo_relative_path(Path::new(self.migration_dir()), "migration_dir")
    }

    pub fn migration_policy_enabled(&self) -> bool {
        if self.contract_version() >= 6 {
            self.has_component_adapter("sqlx") || self.has_component_adapter("go-postgres")
        } else {
            self.sqlx_enabled() || (self.is_go_backend() && self.config.go_database.is_postgres())
        }
    }

    pub fn migration_backend(&self) -> Result<Option<MigrationBackend>> {
        if self.contract_version() >= 6 {
            return native_migration_backend(self.component_specs(), self.action_specs());
        }
        if !self.migration_policy_enabled() {
            return Ok(None);
        }
        Ok(Some(if self.is_go_backend() {
            MigrationBackend::Goose
        } else {
            MigrationBackend::Sqlx
        }))
    }

    pub fn sqlx_owns_migration_authoring(&self) -> bool {
        if self.contract_version() < 6 {
            return self.sqlx_enabled();
        }
        match self.migration_backend() {
            Ok(Some(MigrationBackend::Sqlx)) => true,
            Ok(Some(MigrationBackend::Goose)) | Err(_) => false,
            Ok(None) => self.sqlx_enabled() && !self.has_component_adapter("go-postgres"),
        }
    }

    pub const fn rust_migration_layout(&self) -> RustMigrationLayout {
        self.config.rust_migration_layout
    }

    pub fn migration_add_enabled(&self) -> bool {
        self.sqlx_enabled() && self.rust_migration_layout().allows_migration_add()
    }

    pub fn source_commit(&self) -> &str {
        &self.config.commit
    }

    pub fn source_path(&self) -> &str {
        &self.config.src_path
    }

    fn has_component_adapter(&self, adapter: &str) -> bool {
        self.manifest.components.iter().any(|component| {
            component
                .adapters
                .iter()
                .any(|candidate| candidate == adapter)
        })
    }

    pub fn template_mode(&self) -> &str {
        &self.config.template_mode
    }

    pub fn template_local_path(&self) -> &str {
        &self.config.template_local_path
    }

    pub fn command_for_key(&self, key: &str) -> Result<&str> {
        // Project-owned [commands] intentionally override legacy top-level fields so
        // adopted repos can customize generated command keys without changing contracts.
        if let Some(command) = self.config.commands.get(key) {
            return non_empty_command(key, command);
        }

        let Some(command) = legacy_command_for_key(&self.config, key) else {
            if jig_features::is_supported_command_key(key) {
                bail!("Command key {key} is missing in [commands] in .jig.toml");
            } else {
                bail!("Unsupported command key in jig contract: {key}");
            }
        };
        non_empty_command(key, command)
    }

    pub fn supports_command_key(&self, key: &str) -> bool {
        jig_features::is_supported_command_key(key) || self.config.commands.contains_key(key)
    }

    pub fn web_package_manager(&self) -> &str {
        &self.config.web_package_manager
    }

    pub fn frontend_apps(&self) -> &[FrontendAppConfig] {
        &self.config.frontend_apps
    }

    pub fn frontend_app_role<'a>(&'a self, app: &'a FrontendAppConfig) -> &'a str {
        configured_frontend_app_metadata(&self.config, app).role
    }

    pub fn frontend_app_kind<'a>(&'a self, app: &'a FrontendAppConfig) -> &'a str {
        configured_frontend_app_metadata(&self.config, app).kind
    }

    pub const fn vault_config(&self) -> &VaultConfig {
        &self.config.vault
    }

    pub const fn dev_config(&self) -> &DevConfig {
        &self.config.dev
    }

    pub fn work_gates(&self) -> Vec<WorkGate> {
        self.config
            .work
            .as_ref()
            .map(WorkConfig::gates)
            .unwrap_or_default()
    }

    pub fn work_check_tools(&self) -> Vec<String> {
        self.config
            .work
            .as_ref()
            .map(WorkConfig::check_tools)
            .unwrap_or_default()
    }

    pub const fn loop_config(&self) -> &LoopConfig {
        &self.config.loop_config
    }

    pub fn loop_workflows(&self) -> &[LoopWorkflowConfig] {
        self.config.loop_config.workflows()
    }

    pub fn codex_marketplaces(&self) -> &[CodexMarketplaceConfig] {
        &self.config.agent_tooling.codex.marketplaces
    }

    pub fn state_dir(&self) -> PathBuf {
        self.root.join(".agent/state")
    }

    pub fn state_file(&self, name: &str) -> PathBuf {
        self.state_dir().join(name)
    }
}

mod execution_authority;
use execution_authority::contract_source_digest;

include!("tail.rs");

mod validation;
use validation::*;
pub use validation::{
    config_app_dirs_match, default_codex_marketplace_plugins, is_reserved_git_metadata_component,
    validate_dev_proxy_settings, validate_schema_docs_dir, validate_web_package_manager,
};

mod repository_root;
use repository_root::{find_optional_repo_root, repo_root_from_env};
pub use repository_root::{find_repo_root_from, find_repo_root_from_or_env};

// Keep launcher protocol constants in this module shell: repository tooling
// reads their declarations directly without compiling the Rust include tree.
pub const CURRENT_CONTRACT_VERSION: u32 = 9;
pub const MAX_SUPPORTED_CONTRACT_VERSION: u32 = 9;
pub const LAST_VERSION_LOCKED_CONTRACT_VERSION: u32 = 3;
pub const INSTALLER_CACHE_LAYOUT_MARKER: &str =
    "git=.git/jig-tools;fallback=.agent/.cache/jig;runtime-suffix=-runtime";

mod config_snapshot;
#[cfg(test)]
mod contract_tests;
use config_snapshot::{load_config, load_config_snapshot};
mod defaults;
mod execution_config;
mod inputs_policy;
mod loop_config;
mod migration;
mod optional;
mod runtime;
#[cfg(test)]
mod tests;
mod vault_config;
mod work_config;

pub mod backend;
pub mod frontend_metadata;
pub mod repository_path;
pub mod strict_json;
#[cfg(any(test, feature = "test-support"))]
pub mod test_support;
