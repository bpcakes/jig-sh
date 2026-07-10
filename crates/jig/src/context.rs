use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};
use jig_contract::{FeatureContext, ManifestTool};
use serde::Deserialize;

mod loop_config;
mod work_config;

pub(crate) use loop_config::{LoopConfig, LoopWorkflowConfig};
pub(crate) use work_config::{
    ReviewScopeArg, WorkConfig, WorkGate, WorkRefinementConfig, WorkReviewGate,
    parse_review_scope_arg,
};

const CURRENT_SESSION_FILE: &str = "jig-current-session.txt";
const JIG_REPO_ROOT_ENV: &str = "JIG_REPO_ROOT";
pub(crate) const DEFAULT_CODEX_MARKETPLACE_ID: &str = "jig-skills";
// jig.sh generated repos default to the shared Jig skills marketplace; forks can
// override or opt out through agent_tooling.codex.marketplaces in .jig.toml.
pub(crate) const DEFAULT_CODEX_MARKETPLACE_SOURCE: &str = "bpcakes/jig-skills";
pub(crate) const DEFAULT_CODEX_MARKETPLACE_PLUGINS: &[&str] = &[
    "jig-rust@jig-skills",
    "jig-swift@jig-skills",
    "jig-typescript@jig-skills",
    "jig-exec-plans@jig-skills",
];
pub(crate) const SUPPORTED_WEB_PACKAGE_MANAGERS: &[&str] = &["bun", "npm", "pnpm", "yarn"];

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
    jig_version: String,
    #[allow(dead_code)]
    #[serde(default)]
    template_source_url: String,
    #[allow(dead_code)]
    #[serde(default)]
    harness_footprint: HarnessFootprintConfig,
    #[allow(dead_code)]
    #[serde(default)]
    sqlx_enabled: bool,
    #[allow(dead_code)]
    #[serde(default)]
    rust_crate_roots: Vec<String>,
    #[allow(dead_code)]
    #[serde(default)]
    rust_migration_dir: String,
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
    vault: VaultConfig,
    #[serde(default)]
    dev: DevConfig,
    #[serde(default)]
    work: WorkConfig,
    #[serde(default, rename = "loop")]
    loop_config: LoopConfig,
    #[serde(default)]
    agent_tooling: AgentToolingConfig,
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

    pub(crate) fn allow_global(&self) -> bool {
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
    jig_version: String,
    #[allow(dead_code)]
    #[serde(default)]
    required_commands: Vec<String>,
    tools: Vec<ManifestTool>,
}

#[derive(Clone, Debug)]
pub(crate) struct RepoConfigProbe {
    pub(crate) repo_name: String,
    pub(crate) jig_version: String,
}

#[derive(Clone, Debug)]
pub(crate) struct RepoContext {
    root: PathBuf,
    current_session_path: PathBuf,
    config: RepoConfig,
    manifest: ContractManifest,
}

impl RepoContext {
    pub(crate) fn load() -> Result<Self> {
        let root = find_repo_root_from_or_env(&std::env::current_dir()?)?;
        Self::load_from_root(root)
    }

    #[cfg_attr(not(feature = "dev-proxy"), allow(dead_code))]
    pub(crate) fn load_optional() -> Result<Option<Self>> {
        // Contextless cleanup/status commands should still work when a shell
        // inherited a stale JIG_REPO_ROOT. Required repo commands use load()
        // instead, where the env var remains an explicit override.
        match repo_root_from_env() {
            Ok(Some(root)) => {
                let display_root = root.display().to_string();
                match Self::load_from_root(root) {
                    Ok(ctx) => return Ok(Some(ctx)),
                    Err(error) => {
                        eprintln!(
                            "jig ignored invalid {JIG_REPO_ROOT_ENV}={display_root} for contextless command lookup: {error:#}"
                        );
                    }
                }
            }
            Ok(None) => {}
            Err(error) => {
                eprintln!(
                    "jig ignored invalid {JIG_REPO_ROOT_ENV} for contextless command lookup: {error:#}"
                );
            }
        }
        let Some(root) = find_optional_repo_root()? else {
            return Ok(None);
        };
        Self::load_from_root(root).map(Some)
    }

    pub(crate) fn load_from_root(root: PathBuf) -> Result<Self> {
        let config_path = root.join(".jig.toml");
        let config = load_config(&config_path)?;

        let manifest_path = root.join(".agent/jig-contract.json");
        let manifest_text = fs::read_to_string(&manifest_path)
            .with_context(|| format!("Failed to read {}", manifest_path.display()))?;
        let manifest: ContractManifest = serde_json::from_str(&manifest_text)
            .with_context(|| format!("Failed to parse {}", manifest_path.display()))?;

        if !matches!(manifest.contract_version, 2..=3) {
            bail!(
                "Unsupported jig contract version: {}",
                manifest.contract_version
            );
        }
        if manifest.tool_namespace != "jig" {
            bail!("Unsupported tool namespace: {}", manifest.tool_namespace);
        }
        // Versions 2 and 3 share the command-backed manifest schema; v3 changes
        // the CLI command surface, not the required_commands contract shape.
        if manifest.required_commands.is_empty() {
            bail!("jig contract manifest does not declare required commands");
        }
        if config.jig_version != manifest.jig_version {
            bail!(
                "jig version mismatch between .jig.toml ({}) and manifest ({})",
                config.jig_version,
                manifest.jig_version
            );
        }

        let current_session_path = resolve_current_session_path(&root)?;

        Ok(Self {
            root,
            current_session_path,
            config,
            manifest,
        })
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

    pub(crate) fn contract_version(&self) -> u32 {
        self.manifest.contract_version
    }

    pub(crate) fn required_commands(&self) -> &[String] {
        &self.manifest.required_commands
    }

    pub(crate) fn tool_spec(&self, name: &str) -> Option<&ManifestTool> {
        self.manifest.tools.iter().find(|tool| tool.name == name)
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

    pub(crate) fn jig_version(&self) -> &str {
        &self.config.jig_version
    }

    pub(crate) fn is_minimal_footprint(&self) -> bool {
        self.config.harness_footprint == HarnessFootprintConfig::Minimal
    }

    pub(crate) fn sqlx_enabled(&self) -> bool {
        self.config.sqlx_enabled
    }

    pub(crate) fn schema_dump_enabled(&self) -> bool {
        self.config.schema_dump_enabled
    }

    pub(crate) fn rust_crate_roots(&self) -> &[String] {
        &self.config.rust_crate_roots
    }

    pub(crate) fn rust_migration_dir(&self) -> &str {
        &self.config.rust_migration_dir
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

    pub(crate) fn command_for_key(&self, key: &str) -> Result<&str> {
        // Project-owned [commands] intentionally override legacy top-level fields so
        // adopted repos can customize generated command keys without changing contracts.
        if let Some(command) = self.config.commands.get(key) {
            return non_empty_command(key, command);
        }

        let command = match key {
            "bootstrap_command" => &self.config.bootstrap_command,
            // Preserved for older contracts that still required the command
            // key before contract checking became native.
            "contract_check_command" => &self.config.contract_check_command,
            "migration_add_command" => &self.config.migration_add_command,
            "rust_clippy_command" => &self.config.rust_clippy_command,
            "rust_fmt_check_command" => &self.config.rust_fmt_check_command,
            "rust_test_command" => &self.config.rust_test_command,
            "rust_test_locked_command" => &self.config.rust_test_locked_command,
            "schema_check_command" => &self.config.schema_check_command,
            "schema_dump_command" => &self.config.schema_dump_command,
            "sqlx_check_command" => &self.config.sqlx_check_command,
            _ => {
                if jig_features::is_supported_command_key(key) {
                    bail!("Command key {key} is missing in [commands] in .jig.toml");
                } else {
                    bail!("Unsupported command key in jig contract: {key}");
                }
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

    pub(crate) fn vault_config(&self) -> &VaultConfig {
        &self.config.vault
    }

    #[cfg_attr(not(feature = "dev-proxy"), allow(dead_code))]
    pub(crate) fn dev_config(&self) -> &DevConfig {
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

    pub(crate) fn loop_config(&self) -> &LoopConfig {
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

fn load_config(config_path: &Path) -> Result<RepoConfig> {
    let config_text = fs::read_to_string(config_path)
        .with_context(|| format!("Failed to read {}", config_path.display()))?;
    let config: RepoConfig = toml::from_str(&config_text).with_context(|| {
        format!(
            "Failed to parse {}. Jig rejects unknown .jig.toml keys during upgrades; remove typos or experimental keys and retry.",
            config_path.display()
        )
    })?;
    validate_config(&config)?;
    Ok(config)
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
}

fn non_empty_command<'a>(key: &str, command: &'a str) -> Result<&'a str> {
    if command.trim().is_empty() {
        bail!("Command key {key} is empty in .jig.toml");
    }
    Ok(command)
}

fn default_true() -> bool {
    true
}

fn default_proxy_http_port() -> u16 {
    1355
}

fn default_proxy_https_port() -> Option<u16> {
    Some(1443)
}

fn default_dev_tld() -> String {
    "localhost".into()
}

fn default_dev_app_kind() -> String {
    "env-port".into()
}

fn default_web_package_manager() -> String {
    "bun".into()
}

fn default_codex_marketplaces() -> Vec<CodexMarketplaceConfig> {
    vec![CodexMarketplaceConfig {
        id: DEFAULT_CODEX_MARKETPLACE_ID.into(),
        source: DEFAULT_CODEX_MARKETPLACE_SOURCE.into(),
        plugins: default_codex_marketplace_plugins(),
    }]
}

pub(crate) fn default_codex_marketplace_plugins() -> Vec<String> {
    DEFAULT_CODEX_MARKETPLACE_PLUGINS
        .iter()
        .map(|plugin| (*plugin).into())
        .collect()
}

fn validate_config(config: &RepoConfig) -> Result<()> {
    validate_command_map(&config.commands)?;
    validate_web_package_manager(&config.web_package_manager)?;
    validate_vault_config(config)?;
    validate_dev_config(config)?;
    validate_runtime_config(config)
}

fn validate_command_map(commands: &BTreeMap<String, String>) -> Result<()> {
    for key in commands.keys() {
        if !is_safe_command_key(key) {
            bail!(
                "Invalid [commands] key '{key}'. Use lowercase ASCII letters, numbers, and underscores, start with a letter, and end command keys with '_command'."
            );
        }
    }
    Ok(())
}

fn is_safe_command_key(value: &str) -> bool {
    !value.is_empty()
        && value.ends_with("_command")
        && value
            .chars()
            .next()
            .is_some_and(|ch| ch.is_ascii_lowercase())
        && value
            .chars()
            .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '_')
}

pub(crate) fn validate_web_package_manager(value: &str) -> Result<()> {
    if SUPPORTED_WEB_PACKAGE_MANAGERS.contains(&value) {
        return Ok(());
    }

    bail!(
        "Unsupported web_package_manager '{value}'. Expected one of: {}.",
        SUPPORTED_WEB_PACKAGE_MANAGERS.join(", ")
    )
}

fn validate_vault_config(config: &RepoConfig) -> Result<()> {
    match config.vault.scope {
        VaultScopeConfig::Legacy => {
            if config.vault.scope_id.is_some() {
                bail!("[vault].scope_id requires scope = \"repo\"");
            }
        }
        VaultScopeConfig::Repo => {
            let Some(scope_id) = config.vault.scope_id.as_deref() else {
                bail!("[vault].scope_id is required when scope = \"repo\"");
            };
            validate_vault_scope_id(scope_id)?;
        }
    }
    Ok(())
}

fn validate_vault_scope_id(scope_id: &str) -> Result<()> {
    if !crate::command::is_valid_vault_scope_id(scope_id) {
        bail!(
            "[vault].scope_id must be 1 to 128 bytes and may only contain letters, digits, '_', or '-'"
        );
    }
    Ok(())
}

fn validate_dev_config(config: &RepoConfig) -> Result<()> {
    let mut app_names = HashSet::new();
    for app in &config.dev.apps {
        if !app_names.insert(app.name.as_str()) {
            bail!("Duplicate dev app name '{}' in [[dev.apps]]", app.name);
        }
    }
    if config.dev.apps.is_empty() {
        validate_dev_app_env_prefixes(
            config.frontend_apps.iter().map(|app| app.name.as_str()),
            "[[frontend_apps]]",
        )?;
    } else {
        validate_dev_app_env_prefixes(
            config.dev.apps.iter().map(|app| app.name.as_str()),
            "[[dev.apps]]",
        )?;
    }
    if !config.frontend_apps.is_empty() && !config.dev.apps.is_empty() {
        for frontend_app in &config.frontend_apps {
            let Some(dev_app) = config
                .dev
                .apps
                .iter()
                .find(|app| app.name == frontend_app.name)
            else {
                bail!(
                    "[dev.apps] entries take precedence when [[frontend_apps]] are also configured. Add a matching [[dev.apps]] entry for frontend app '{}' or remove it from [[frontend_apps]].",
                    frontend_app.name
                );
            };
            match dev_app.dir.as_deref() {
                Some(dev_dir) if dev_dir == frontend_app.dir => {}
                Some(dev_dir) => {
                    bail!(
                        "[dev.apps] entry '{}' uses dir '{}' but matching [[frontend_apps]] uses '{}'. Keep them aligned because [dev.apps] takes precedence for scripts/jig dev.",
                        frontend_app.name,
                        dev_dir,
                        frontend_app.dir
                    );
                }
                None => {
                    bail!(
                        "[dev.apps] entry '{}' matches [[frontend_apps]] and must set dir = '{}' because [dev.apps] takes precedence for scripts/jig dev.",
                        frontend_app.name,
                        frontend_app.dir
                    );
                }
            }
        }
    }
    Ok(())
}

fn validate_dev_app_env_prefixes<'a>(
    names: impl IntoIterator<Item = &'a str>,
    section: &str,
) -> Result<()> {
    let mut prefixes = BTreeMap::new();
    for name in names {
        let prefix = jig_core::dev_app_env_prefix(name);
        if let Some(previous) = prefixes.insert(prefix.clone(), name) {
            bail!(
                "{section} entries '{}' and '{}' share derived dev environment prefix {prefix}; rename one app so punctuation-normalized names are unique",
                previous,
                name
            );
        }
    }
    Ok(())
}

fn validate_runtime_config(config: &RepoConfig) -> Result<()> {
    config.work.validate()?;
    config.loop_config.validate()
}

#[cfg_attr(not(feature = "dev-proxy"), allow(dead_code))]
fn find_optional_repo_root() -> Result<Option<PathBuf>> {
    find_optional_repo_root_from(&std::env::current_dir()?)
}

pub(crate) fn find_repo_root_from(start: &Path) -> Result<PathBuf> {
    let Some(root) = find_optional_repo_root_from(start)? else {
        bail!("Could not find repo root containing .jig.toml");
    };
    Ok(root)
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

fn resolve_current_session_path(root: &Path) -> Result<PathBuf> {
    let output = Command::new("git")
        .current_dir(root)
        .args(["rev-parse", "--git-path", CURRENT_SESSION_FILE])
        .output();

    if let Ok(output) = output {
        if output.status.success() {
            let resolved = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !resolved.is_empty() {
                let path = PathBuf::from(&resolved);
                return Ok(if path.is_absolute() {
                    path
                } else {
                    root.join(path)
                });
            }
        }
    }

    Ok(root.join(".agent/.cache").join(CURRENT_SESSION_FILE))
}

#[cfg(test)]
impl RepoContext {
    pub(crate) fn load_from(root: &Path) -> Result<Self> {
        let config_text = fs::read_to_string(root.join(".jig.toml"))?;
        let config: RepoConfig = toml::from_str(&config_text)?;
        validate_config(&config)?;
        let manifest_text = fs::read_to_string(root.join(".agent/jig-contract.json"))?;
        let manifest: ContractManifest = serde_json::from_str(&manifest_text)?;
        Ok(Self {
            root: root.to_path_buf(),
            current_session_path: root.join(".agent/.cache").join(CURRENT_SESSION_FILE),
            config,
            manifest,
        })
    }
}

#[cfg(test)]
mod contract_tests;
#[cfg(test)]
mod tests;
