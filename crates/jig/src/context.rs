use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};
use jig_contract::{
    ActionSpec, ComponentSpec, FeatureContext, ManifestTool, ProfileId, ProfileSpec,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::backend::{BackendLanguage, GoDatabase};
use crate::frontend_metadata::{ResolvedFrontendMetadata, resolve_frontend_metadata};
use crate::repository_path::{
    normalize_portable_repo_path, normalize_portable_repository_directory,
    normalize_repo_relative_path,
};

// agentic-loc-exception: repository configuration access remains centralized while runtime cache and launcher-context concerns live in context/runtime.rs.

pub(crate) use execution_config::{
    CommandOutputLimit, CommandTimeout, MAX_COMMAND_TIMEOUT_SECONDS,
};

pub(crate) use defaults::{
    DEFAULT_CODEX_MARKETPLACE_ID, DEFAULT_CODEX_MARKETPLACE_PLUGINS,
    DEFAULT_CODEX_MARKETPLACE_SOURCE, SUPPORTED_WEB_PACKAGE_MANAGERS,
};
pub(crate) use optional::REPO_CONTEXT_NOT_FOUND;
pub(crate) use runtime::{
    CURRENT_SESSION_FILE, JIG_REPO_ROOT_ENV, LAUNCHER_REPAIR_STAGING_PREFIX,
    MIN_SUPPORTED_CONTRACT_VERSION, RepoConfigProbe, RuntimeCacheProfile,
    is_supported_contract_version, runtime_cache_base, runtime_profile_cache_name,
    runtime_profile_cache_path,
};
use runtime::{ContractVersionProbe, non_empty_legacy_jig_version};
#[cfg(test)]
pub(crate) use runtime::{
    FALLBACK_RUNTIME_CACHE_BASE, GIT_RUNTIME_CACHE_BASE, RUNTIME_CACHE_PROFILE_SUFFIX,
};

pub(crate) use execution_config::ExecutionConfig;
pub(crate) use loop_config::{LoopConfig, LoopWorkflowConfig};
pub(crate) use status_config::{StatusConfig, StatusProviderConfig};
pub(crate) use work_config::{
    ReviewScopeArg, WorkConfig, WorkEvidenceGate, WorkEvidenceSelector, WorkGate,
    WorkRefinementConfig, WorkReviewGate, parse_review_scope_arg, parse_work_gate,
};

#[cfg_attr(not(feature = "dev-proxy"), allow(dead_code))]
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
    rust_sqlx_metadata_dir: String,
    #[allow(dead_code)]
    #[serde(default)]
    schema_dump_enabled: bool,
    #[allow(dead_code)]
    #[serde(default)]
    schema_dump_command: String,
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
    #[serde(default)]
    frontend_apps: Vec<FrontendAppConfig>,
    #[serde(default)]
    repository: Option<AuthoredRepositoryConfig>,
    #[serde(default)]
    vault: VaultConfig,
    #[serde(default)]
    dev: DevConfig,
    #[serde(default)]
    work: WorkConfig,
    #[serde(default, rename = "loop")]
    loop_config: LoopConfig,
    #[serde(default)]
    status: StatusConfig,
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
    components: Vec<ComponentSpec>,
    actions: Vec<ActionSpec>,
    profiles: Vec<ProfileSpec>,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "kebab-case")]
enum HarnessFootprintConfig {
    #[default]
    Full,
    Minimal,
}

#[cfg_attr(not(feature = "dev-proxy"), allow(dead_code))]
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct FrontendAppConfig {
    pub(crate) name: String,
    pub(crate) dir: String,
    #[allow(dead_code)]
    #[serde(default)]
    pub(crate) coverage_threshold: u32,
    #[serde(default)]
    pub(crate) kind: Option<String>,
    #[serde(default)]
    pub(crate) role: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct VaultConfig {
    #[serde(default)]
    scope: VaultScopeConfig,
    #[serde(default)]
    scope_id: Option<String>,
    #[serde(default)]
    allow_global: bool,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum VaultScopeConfig {
    #[default]
    Legacy,
    Repo,
}

impl VaultConfig {
    pub(crate) fn repo_scope_id(&self) -> Option<&str> {
        if self.scope == VaultScopeConfig::Repo {
            self.scope_id.as_deref()
        } else {
            None
        }
    }

    pub(crate) const fn allow_global(&self) -> bool {
        self.allow_global
    }
}

#[cfg_attr(not(feature = "dev-proxy"), allow(dead_code))]
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DevConfig {
    #[serde(default = "default_proxy_http_port")]
    pub(crate) proxy_port: u16,
    #[serde(default = "default_proxy_https_port")]
    pub(crate) https_port: Option<u16>,
    #[serde(default)]
    pub(crate) https: bool,
    #[serde(default = "default_true")]
    pub(crate) http2: bool,
    #[serde(default)]
    pub(crate) lan: bool,
    #[serde(default = "default_dev_tld")]
    pub(crate) tld: String,
    #[serde(default)]
    pub(crate) workspace_discovery: bool,
    #[serde(default)]
    pub(crate) apps: Vec<DevAppConfig>,
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

#[cfg_attr(not(feature = "dev-proxy"), allow(dead_code))]
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DevAppConfig {
    pub(crate) name: String,
    #[serde(default)]
    pub(crate) dir: Option<String>,
    #[serde(default = "default_dev_app_kind")]
    pub(crate) kind: String,
    #[serde(default)]
    pub(crate) command: Option<String>,
    #[serde(default)]
    pub(crate) argv: Vec<String>,
    #[serde(default)]
    pub(crate) port: Option<u16>,
    #[serde(default)]
    pub(crate) host: Option<String>,
    #[serde(default = "default_true")]
    pub(crate) proxy: bool,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct AgentToolingConfig {
    #[serde(default)]
    pub(crate) codex: CodexToolingConfig,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CodexToolingConfig {
    #[serde(default = "default_codex_marketplaces")]
    pub(crate) marketplaces: Vec<CodexMarketplaceConfig>,
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
pub(crate) struct CodexMarketplaceConfig {
    pub(crate) id: String,
    pub(crate) source: String,
    #[serde(default)]
    pub(crate) plugins: Vec<String>,
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
pub(crate) struct RepoContext {
    root: PathBuf,
    current_session_path: PathBuf,
    config: RepoConfig,
    manifest: ContractManifest,
    contract_digest: String,
}

impl RepoContext {
    pub(crate) fn load_from_root(root: PathBuf) -> Result<Self> {
        let config_path = root.join(".jig.toml");
        let loaded_config = load_config_snapshot(&config_path)?;

        let manifest_path = root.join(".agent/jig-contract.json");
        let manifest_text = fs::read_to_string(&manifest_path)
            .with_context(|| format!("Failed to read {}", manifest_path.display()))?;
        let manifest_authority: serde_json::Value = serde_json::from_str(&manifest_text)
            .with_context(|| format!("Failed to parse {}", manifest_path.display()))?;
        let manifest: ContractManifest = serde_json::from_value(manifest_authority.clone())
            .with_context(|| format!("Failed to parse {}", manifest_path.display()))?;
        let config = loaded_config.config;
        let contract_digest = contract_source_digest(&config, &manifest_authority)?;

        if !is_supported_contract_version(manifest.contract_version) {
            bail!(
                "Unsupported jig contract version: {}",
                manifest.contract_version
            );
        }
        if manifest.tool_namespace != "jig" {
            bail!("Unsupported tool namespace: {}", manifest.tool_namespace);
        }
        validate_repository_source(&config, &manifest)?;
        // Legacy contracts are command-backed. A native-only v6 repository is
        // valid because action runners, rather than a global command list,
        // define its executable surface.
        if manifest.contract_version <= 5 && manifest.required_commands.is_empty() {
            bail!("jig contract manifest does not declare required commands");
        }
        if manifest.contract_version <= LAST_VERSION_LOCKED_CONTRACT_VERSION {
            let config_version =
                non_empty_legacy_jig_version(config.jig_version.as_deref(), ".jig.toml")?;
            let manifest_version = non_empty_legacy_jig_version(
                manifest.jig_version.as_deref(),
                ".agent/jig-contract.json",
            )?;
            if config_version != manifest_version {
                bail!(
                    "jig version mismatch between .jig.toml ({config_version}) and manifest ({manifest_version})"
                );
            }
        }

        let current_session_path = resolve_current_session_path(&root);

        Ok(Self {
            root,
            current_session_path,
            config,
            manifest,
            contract_digest,
        })
    }

    pub(crate) fn supported_contract_version_from_root(root: &Path) -> Result<u32> {
        let contract_version = Self::declared_contract_version_from_root(root)?;
        if !is_supported_contract_version(contract_version) {
            bail!("Unsupported jig contract version: {contract_version}");
        }
        Ok(contract_version)
    }

    pub(crate) fn declared_contract_version_from_root(root: &Path) -> Result<u32> {
        let manifest_path = root.join(".agent/jig-contract.json");
        let manifest_text = fs::read_to_string(&manifest_path)
            .with_context(|| format!("Failed to read {}", manifest_path.display()))?;
        let probe: ContractVersionProbe = serde_json::from_str(&manifest_text)
            .with_context(|| format!("Failed to parse {}", manifest_path.display()))?;
        Ok(probe.contract_version)
    }

    pub(crate) fn validate_config_file(root: &Path) -> Result<RepoConfigProbe> {
        let config = load_config(&root.join(".jig.toml"))?;
        Ok(RepoConfigProbe {
            repo_name: config.repo_name,
            jig_version: config.jig_version,
        })
    }

    pub(crate) fn tool_specs(&self) -> &[ManifestTool] {
        &self.manifest.tools
    }

    pub(crate) const fn contract_version(&self) -> u32 {
        self.manifest.contract_version
    }

    pub(crate) fn required_commands(&self) -> &[String] {
        &self.manifest.required_commands
    }

    pub(crate) fn tool_spec(&self, name: &str) -> Option<&ManifestTool> {
        self.manifest.tools.iter().find(|tool| tool.name == name)
    }

    pub(crate) fn component_specs(&self) -> &[ComponentSpec] {
        &self.manifest.components
    }

    pub(crate) fn action_specs(&self) -> &[ActionSpec] {
        &self.manifest.actions
    }

    pub(crate) fn profile_specs(&self) -> &[ProfileSpec] {
        &self.manifest.profiles
    }

    pub(crate) fn default_check_profile(&self) -> Option<&ProfileId> {
        self.manifest.default_check_profile.as_ref()
    }

    pub(crate) fn affected_ignore(&self) -> &[String] {
        &self.manifest.affected_ignore
    }

    pub(crate) fn contract_digest(&self) -> &str {
        &self.contract_digest
    }

    pub(crate) fn root(&self) -> &Path {
        &self.root
    }

    pub(crate) fn repo_name(&self) -> &str {
        &self.config.repo_name
    }

    pub(crate) fn default_branch(&self) -> &str {
        &self.config.default_branch
    }

    pub(crate) fn is_go_backend(&self) -> bool {
        if self.contract_version() >= 6 {
            self.has_component_adapter("go")
        } else {
            self.config.backend_language.is_go()
        }
    }

    pub(crate) fn legacy_jig_version(&self) -> Option<&str> {
        self.config.jig_version.as_deref()
    }

    pub(crate) fn is_minimal_footprint(&self) -> bool {
        self.config.harness_footprint == HarnessFootprintConfig::Minimal
    }

    pub(crate) fn sqlx_enabled(&self) -> bool {
        if self.contract_version() >= 6 {
            self.has_component_adapter("sqlx")
        } else {
            self.config.sqlx_enabled
        }
    }

    pub(crate) const fn schema_dump_enabled(&self) -> bool {
        self.config.schema_dump_enabled
    }

    pub(crate) fn rust_crate_roots(&self) -> &[String] {
        &self.config.rust_crate_roots
    }

    pub(crate) fn backend_guide_roots(&self) -> Vec<&str> {
        if self.is_go_backend() {
            vec!["cmd", "internal"]
        } else {
            self.rust_crate_roots().iter().map(String::as_str).collect()
        }
    }

    pub(crate) fn migration_dir(&self) -> &str {
        if self.config.migration_dir.trim().is_empty() {
            &self.config.rust_migration_dir
        } else {
            &self.config.migration_dir
        }
    }

    pub(crate) fn migration_relative_dir(&self) -> Result<PathBuf> {
        normalize_repo_relative_path(Path::new(self.migration_dir()), "migration_dir")
    }

    pub(crate) fn migration_policy_enabled(&self) -> bool {
        if self.contract_version() >= 6 {
            self.has_component_adapter("sqlx") || self.has_component_adapter("go-postgres")
        } else {
            self.sqlx_enabled() || (self.is_go_backend() && self.config.go_database.is_postgres())
        }
    }

    pub(crate) fn schema_dump_command(&self) -> &str {
        &self.config.schema_dump_command
    }

    pub(crate) fn source_commit(&self) -> &str {
        &self.config.commit
    }

    pub(crate) fn source_path(&self) -> &str {
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

    pub(crate) fn template_mode(&self) -> &str {
        &self.config.template_mode
    }

    pub(crate) fn template_local_path(&self) -> &str {
        &self.config.template_local_path
    }

    pub(crate) fn command_for_key(&self, key: &str) -> Result<&str> {
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

    pub(crate) fn supports_command_key(&self, key: &str) -> bool {
        jig_features::is_supported_command_key(key) || self.config.commands.contains_key(key)
    }

    #[cfg_attr(not(feature = "dev-proxy"), allow(dead_code))]
    pub(crate) fn web_package_manager(&self) -> &str {
        &self.config.web_package_manager
    }

    #[cfg_attr(not(feature = "dev-proxy"), allow(dead_code))]
    pub(crate) fn frontend_apps(&self) -> &[FrontendAppConfig] {
        &self.config.frontend_apps
    }

    pub(crate) fn frontend_app_role<'a>(&'a self, app: &'a FrontendAppConfig) -> &'a str {
        configured_frontend_app_metadata(&self.config, app).role
    }

    pub(crate) fn frontend_app_kind<'a>(&'a self, app: &'a FrontendAppConfig) -> &'a str {
        configured_frontend_app_metadata(&self.config, app).kind
    }

    pub(crate) const fn vault_config(&self) -> &VaultConfig {
        &self.config.vault
    }

    #[cfg_attr(not(feature = "dev-proxy"), allow(dead_code))]
    pub(crate) const fn dev_config(&self) -> &DevConfig {
        &self.config.dev
    }

    pub(crate) fn work_gates(&self) -> Vec<WorkGate> {
        self.config.work.gates()
    }

    pub(crate) fn work_check_tools(&self) -> Vec<String> {
        self.config.work.check_tools()
    }

    pub(crate) fn work_refinements(&self) -> &[WorkRefinementConfig] {
        self.config.work.refinements()
    }

    pub(crate) const fn loop_config(&self) -> &LoopConfig {
        &self.config.loop_config
    }

    pub(crate) fn loop_workflows(&self) -> &[LoopWorkflowConfig] {
        self.config.loop_config.workflows()
    }

    pub(crate) fn codex_marketplaces(&self) -> &[CodexMarketplaceConfig] {
        &self.config.agent_tooling.codex.marketplaces
    }

    pub(crate) fn state_dir(&self) -> PathBuf {
        self.root.join(".agent/state")
    }

    pub(crate) fn state_file(&self, name: &str) -> PathBuf {
        self.state_dir().join(name)
    }

    pub(crate) fn plan_body_path(&self, plan_id: &str) -> PathBuf {
        self.root.join(".agent/plans").join(format!("{plan_id}.md"))
    }

    pub(crate) fn current_session_path(&self) -> PathBuf {
        self.current_session_path.clone()
    }
}

struct LoadedConfig {
    config: RepoConfig,
}

fn load_config_snapshot(config_path: &Path) -> Result<LoadedConfig> {
    let config_text = fs::read_to_string(config_path)
        .with_context(|| format!("Failed to read {}", config_path.display()))?;
    let config: RepoConfig = toml::from_str(&config_text).with_context(|| {
        format!(
            "Failed to parse {}. Jig rejects unknown .jig.toml keys during upgrades; remove typos or experimental keys and retry.",
            config_path.display()
        )
    })?;
    validate_config(&config)?;
    Ok(LoadedConfig { config })
}

fn load_config(config_path: &Path) -> Result<RepoConfig> {
    Ok(load_config_snapshot(config_path)?.config)
}

#[derive(Serialize)]
struct RepositoryExecutionAuthority<'a> {
    schema_version: u32,
    manifest: &'a serde_json::Value,
    backend_language: BackendLanguage,
    go_database: GoDatabase,
    sqlx_enabled: bool,
    rust_crate_roots: &'a [String],
    migration_dir: &'a str,
    rust_migration_dir: &'a str,
    rust_sqlx_metadata_dir: &'a str,
    schema_dump_enabled: bool,
    commands: BTreeMap<String, &'a str>,
    execution: &'a ExecutionConfig,
}

fn contract_source_digest(config: &RepoConfig, manifest: &serde_json::Value) -> Result<String> {
    // This deliberately exhaustive pattern is a compile-time review gate for
    // every new RepoConfig field: each addition must be classified here as
    // execution authority or explicitly unrelated runtime/config metadata.
    let RepoConfig {
        src_path: _,
        commit: _,
        template_mode: _,
        template_local_path: _,
        repo_name: _,
        default_branch: _,
        ci_github_runner: _,
        jig_version: _,
        template_source_url: _,
        harness_footprint: _,
        backend_language,
        go_database,
        sqlx_enabled,
        rust_crate_roots,
        rust_migration_dir,
        migration_dir,
        rust_sqlx_metadata_dir,
        schema_dump_enabled,
        schema_dump_command: _,
        schema_check_command: _,
        sqlx_check_command: _,
        migration_add_command: _,
        bootstrap_command: _,
        contract_check_command: _,
        dev_command: _,
        rust_fmt_check_command: _,
        rust_clippy_command: _,
        rust_test_command: _,
        rust_test_locked_command: _,
        commands,
        web_package_manager: _,
        frontend_apps: _,
        repository: _,
        vault: _,
        dev: _,
        work: _,
        loop_config: _,
        status: _,
        execution,
        agent_tooling: _,
    } = config;
    let mut effective_commands = commands
        .iter()
        .map(|(key, value)| (key.clone(), value.as_str()))
        .collect::<BTreeMap<_, _>>();
    for (key, accessor) in LEGACY_COMMAND_BINDINGS {
        effective_commands
            .entry((*key).into())
            .or_insert_with(|| accessor(config));
    }
    let authority = RepositoryExecutionAuthority {
        schema_version: 1,
        manifest,
        backend_language: *backend_language,
        go_database: *go_database,
        sqlx_enabled: *sqlx_enabled,
        rust_crate_roots,
        migration_dir,
        rust_migration_dir,
        rust_sqlx_metadata_dir,
        schema_dump_enabled: *schema_dump_enabled,
        commands: effective_commands,
        execution,
    };
    let encoded = serde_json::to_vec(&authority)
        .context("Failed to canonicalize repository execution authority")?;
    let mut hasher = Sha256::new();
    hasher.update(b"jig-repository-execution-authority-v1\0");
    hasher.update((encoded.len() as u64).to_be_bytes());
    hasher.update(encoded);
    Ok(format!("sha256:{:x}", hasher.finalize()))
}

impl FeatureContext for RepoContext {
    fn contract_version(&self) -> u32 {
        self.contract_version()
    }

    fn required_commands(&self) -> &[String] {
        self.required_commands()
    }

    fn sqlx_enabled(&self) -> bool {
        self.sqlx_enabled()
    }

    fn schema_dump_enabled(&self) -> bool {
        self.schema_dump_enabled()
    }

    fn frontend_app_count(&self) -> usize {
        if self.is_minimal_footprint() {
            0
        } else {
            self.frontend_apps().len()
        }
    }

    fn go_backend_enabled(&self) -> bool {
        self.is_go_backend()
    }

    fn go_postgres_enabled(&self) -> bool {
        if self.contract_version() >= 6 {
            self.has_component_adapter("go-postgres")
        } else {
            self.is_go_backend() && self.config.go_database.is_postgres()
        }
    }
}

mod validation;
use validation::*;
pub(crate) use validation::{
    config_app_dirs_match, default_codex_marketplace_plugins, validate_web_package_manager,
};

#[cfg_attr(not(feature = "dev-proxy"), allow(dead_code))]
fn find_optional_repo_root() -> Result<Option<PathBuf>> {
    find_optional_repo_root_from(&std::env::current_dir()?)
}

pub(crate) fn find_repo_root_from(start: &Path) -> Result<PathBuf> {
    find_optional_repo_root_from(start)?.context(REPO_CONTEXT_NOT_FOUND)
}

pub(crate) fn find_repo_root_from_or_env(start: &Path) -> Result<PathBuf> {
    if let Some(root) = repo_root_from_env()? {
        Ok(root)
    } else {
        find_repo_root_from(start)
    }
}

fn repo_root_from_env() -> Result<Option<PathBuf>> {
    let Some(root) = std::env::var_os(JIG_REPO_ROOT_ENV) else {
        return Ok(None);
    };
    let root = PathBuf::from(root);
    if root.as_os_str().is_empty() {
        return Ok(None);
    }
    if !root.join(".jig.toml").exists() {
        bail!(
            "{JIG_REPO_ROOT_ENV} does not contain .jig.toml: {}",
            root.display()
        );
    }
    fs::canonicalize(&root)
        .with_context(|| {
            format!(
                "Failed to canonicalize {JIG_REPO_ROOT_ENV}: {}",
                root.display()
            )
        })
        .map(Some)
}

fn find_optional_repo_root_from(start: &Path) -> Result<Option<PathBuf>> {
    let mut current = start.to_path_buf();
    loop {
        if current.join(".jig.toml").exists() {
            return Ok(Some(current));
        }
        if !current.pop() {
            return Ok(None);
        }
    }
}

fn resolve_current_session_path(root: &Path) -> PathBuf {
    let mut command = Command::new("git");
    command
        .current_dir(root)
        .args(["rev-parse", "--git-path", CURRENT_SESSION_FILE]);
    crate::bootstrap::scrub_known_repository_git_environment(&mut command);
    command.env("GIT_OPTIONAL_LOCKS", "0");
    let output = command.output();

    if let Ok(output) = output
        && output.status.success()
    {
        let resolved = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if !resolved.is_empty() {
            let path = PathBuf::from(&resolved);
            return if path.is_absolute() {
                path
            } else {
                root.join(path)
            };
        }
    }

    root.join(".agent/.cache").join(CURRENT_SESSION_FILE)
}

#[cfg(test)]
impl RepoContext {
    pub(crate) fn load_from(root: &Path) -> Result<Self> {
        let config_path = root.join(".jig.toml");
        let loaded_config = load_config_snapshot(&config_path)?;
        let manifest_text = fs::read_to_string(root.join(".agent/jig-contract.json"))?;
        let manifest_authority: serde_json::Value = serde_json::from_str(&manifest_text)?;
        let manifest: ContractManifest = serde_json::from_value(manifest_authority.clone())?;
        let contract_digest = contract_source_digest(&loaded_config.config, &manifest_authority)?;
        Ok(Self {
            root: root.to_path_buf(),
            current_session_path: root.join(".agent/.cache").join(CURRENT_SESSION_FILE),
            config: loaded_config.config,
            manifest,
            contract_digest,
        })
    }
}

// Keep launcher protocol constants in this module shell: repository tooling
// reads their declarations directly without compiling the Rust include tree.
pub(crate) const CURRENT_CONTRACT_VERSION: u32 = 6;
pub(crate) const LAST_VERSION_LOCKED_CONTRACT_VERSION: u32 = 3;
pub(crate) const INSTALLER_CACHE_LAYOUT_MARKER: &str =
    "git=.git/jig-tools;fallback=.agent/.cache/jig;runtime-suffix=-runtime";

#[cfg(test)]
mod contract_tests;
mod defaults;
mod execution_config;
mod loop_config;
mod optional;
mod runtime;
mod status_config;
#[cfg(test)]
mod tests;
mod work_config;
