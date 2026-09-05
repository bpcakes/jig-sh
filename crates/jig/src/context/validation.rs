use super::*;

pub(super) fn validate_repository_source(
    config: &RepoConfig,
    manifest: &ContractManifest,
) -> Result<()> {
    config
        .work
        .validate_contract_version(manifest.contract_version)?;
    if manifest.contract_version < 6 {
        if config.repository.is_some() {
            bail!("[repository] requires jig contract version 6 or later");
        }
        return Ok(());
    }
    let source = config.repository.as_ref().ok_or_else(|| {
        anyhow::anyhow!("jig contract version 6 requires [repository] in .jig.toml")
    })?;
    if source.components != manifest.components {
        bail!(
            "repository components differ between .jig.toml and .agent/jig-contract.json at {}; run `jig update --recopy` to regenerate the resolved contract after reviewing the authored source",
            first_sequence_difference(&source.components, &manifest.components, |component| {
                format!("component '{}'", component.id)
            })
        );
    }
    if source.actions != manifest.actions {
        bail!(
            "repository actions differ between .jig.toml and .agent/jig-contract.json at {}; run `jig update --recopy` to regenerate the resolved contract after reviewing the authored source",
            first_sequence_difference(&source.actions, &manifest.actions, |action| {
                format!("target '{}'", action.target)
            })
        );
    }
    if source.profiles != manifest.profiles {
        bail!(
            "repository profiles differ between .jig.toml and .agent/jig-contract.json at {}; run `jig update --recopy` to regenerate the resolved contract after reviewing the authored source",
            first_sequence_difference(&source.profiles, &manifest.profiles, |profile| {
                format!("profile '{}'", profile.id)
            })
        );
    }
    if Some(&source.default_check_profile) != manifest.default_check_profile.as_ref() {
        bail!(
            "repository default_check_profile differs between .jig.toml ({:?}) and .agent/jig-contract.json ({:?}); run `jig update --recopy` to regenerate the resolved contract after reviewing the authored source",
            source.default_check_profile,
            manifest.default_check_profile
        );
    }
    if source.affected_ignore != manifest.affected_ignore {
        bail!(
            "repository affected_ignore differs between .jig.toml and .agent/jig-contract.json at {}; run `jig update --recopy` to regenerate the resolved contract after reviewing the authored source",
            first_sequence_difference(
                &source.affected_ignore,
                &manifest.affected_ignore,
                |pattern| format!("pattern {pattern:?}")
            )
        );
    }
    Ok(())
}

pub(super) fn first_sequence_difference<T: PartialEq>(
    source: &[T],
    manifest: &[T],
    describe: impl Fn(&T) -> String,
) -> String {
    for (index, (source_item, manifest_item)) in source.iter().zip(manifest).enumerate() {
        if source_item != manifest_item {
            let source_description = describe(source_item);
            let manifest_description = describe(manifest_item);
            return if source_description == manifest_description {
                format!("{source_description} (index {index})")
            } else {
                format!(
                    "index {index} (.jig.toml has {source_description}; resolved contract has {manifest_description})"
                )
            };
        }
    }
    if let Some(extra) = source.get(manifest.len()) {
        return format!(
            "index {} (.jig.toml has extra {})",
            manifest.len(),
            describe(extra)
        );
    }
    let extra = manifest
        .get(source.len())
        .expect("unequal sequences must contain a differing or extra item");
    format!(
        "index {} (resolved contract has extra {})",
        source.len(),
        describe(extra)
    )
}

pub(super) fn non_empty_command<'a>(key: &str, command: &'a str) -> Result<&'a str> {
    if command.trim().is_empty() {
        bail!("Command key {key} is empty in .jig.toml");
    }
    Ok(command)
}

pub(super) const fn default_true() -> bool {
    true
}

pub(super) const fn default_proxy_http_port() -> u16 {
    1355
}

// Serde requires this default function to return the field's `Option<u16>`
// type; `Some` means HTTPS is configured by default rather than mandatory.
#[allow(clippy::unnecessary_wraps)]
pub(super) const fn default_proxy_https_port() -> Option<u16> {
    Some(1443)
}

pub(crate) fn validate_dev_proxy_settings(
    http_port: u16,
    https_port: Option<u16>,
    tld: &str,
    allow_ephemeral_http: bool,
) -> Result<()> {
    if http_port == 0 && !allow_ephemeral_http {
        bail!("proxy HTTP port must be greater than 0");
    }
    if https_port == Some(0) {
        bail!("proxy HTTPS port must be greater than 0");
    }
    if https_port == Some(http_port) {
        bail!("proxy HTTP and HTTPS ports must be different");
    }
    validate_dev_tld(tld)
}

#[cfg(feature = "dev-proxy")]
fn validate_dev_tld(tld: &str) -> Result<()> {
    jig_dev_proxy::validate_tld(tld)
}

#[cfg(not(feature = "dev-proxy"))]
fn validate_dev_tld(tld: &str) -> Result<()> {
    // Keep this no-feature parser aligned with jig-dev-proxy::host. The vector
    // test below runs in both default and --no-default-features builds so the
    // shared init contract cannot silently widen when the proxy is absent.
    const ALLOWED_TLD_SUFFIXES: &[&str] = &["localhost", "local", "test", "internal"];

    if tld.is_empty() || tld.len() > 253 || tld.contains(':') {
        bail!("invalid hostname '{tld}'");
    }
    for label in tld.split('.') {
        if label.is_empty()
            || label.len() > 63
            || label.starts_with('-')
            || label.ends_with('-')
            || !label
                .chars()
                .all(|character| character.is_ascii_alphanumeric() || character == '-')
        {
            bail!("invalid hostname '{tld}'");
        }
    }
    let normalized = tld.to_ascii_lowercase();
    let labels = normalized.split('.').collect::<Vec<_>>();
    if (labels.len() == 1 && ALLOWED_TLD_SUFFIXES.contains(&labels[0]))
        || (labels.len() == 2 && ALLOWED_TLD_SUFFIXES.contains(&labels[1]))
    {
        return Ok(());
    }
    bail!(
        "dev proxy TLD '{normalized}' is not allowed. Use a private/local suffix such as localhost, local, test, or internal."
    )
}

pub(super) fn default_dev_tld() -> String {
    "localhost".into()
}

pub(super) fn default_dev_app_kind() -> String {
    "env-port".into()
}

pub(super) fn default_web_package_manager() -> String {
    "bun".into()
}

pub(super) fn configured_frontend_app_metadata<'a>(
    config: &'a RepoConfig,
    app: &'a FrontendAppConfig,
) -> ResolvedFrontendMetadata<'a> {
    let matching_dev_kind = config
        .dev
        .apps
        .iter()
        .find(|dev_app| {
            dev_app.name == app.name
                && dev_app
                    .dir
                    .as_deref()
                    .is_some_and(|dev_dir| config_app_dirs_match(dev_dir, &app.dir))
        })
        .map(|dev_app| dev_app.kind.as_str());
    resolve_frontend_metadata(
        &app.name,
        app.kind.as_deref(),
        app.role.as_deref(),
        matching_dev_kind,
    )
}

pub(super) fn default_codex_marketplaces() -> Vec<CodexMarketplaceConfig> {
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

pub(super) fn validate_config(config: &RepoConfig) -> Result<()> {
    validate_backend_config(config)?;
    validate_command_map(&config.commands)?;
    validate_web_package_manager(&config.web_package_manager)?;
    validate_frontend_app_roles(config)?;
    for root in &config.frontend_workspace_roots {
        normalize_portable_repo_path(root, "frontend workspace root")?;
    }
    validate_schema_docs_dir(&config.schema_docs_dir)?;
    validate_vault_config(config)?;
    validate_dev_config(config)?;
    config.work.validate()?;
    config.loop_config.validate()
}

pub(super) fn default_schema_docs_dir() -> String {
    "docs/schema".into()
}

pub(crate) fn validate_schema_docs_dir(value: &str) -> Result<()> {
    let normalized = normalize_portable_repo_path(value, "schema_docs_dir")?;
    if normalized != value || value == "." {
        bail!(
            "schema_docs_dir must be a normalized repository-relative dedicated directory: {value}"
        );
    }
    if value.split('/').any(|component| {
        is_reserved_agent_state_component(component)
            || is_reserved_git_metadata_component(component)
    }) {
        bail!("schema_docs_dir must stay outside reserved .agent and .git directories");
    }
    if !value
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '/' | '.' | '-' | '_'))
    {
        bail!(
            "schema_docs_dir contains unsupported characters; use ASCII letters, numbers, '/', '.', '-' or '_'"
        );
    }
    Ok(())
}

pub(crate) fn is_reserved_git_metadata_component(component: &str) -> bool {
    is_hfs_component_alias(component, ['.', 'g', 'i', 't'])
}

fn is_reserved_agent_state_component(component: &str) -> bool {
    is_hfs_component_alias(component, ['.', 'a', 'g', 'e', 'n', 't'])
}

fn is_hfs_component_alias<const N: usize>(component: &str, expected: [char; N]) -> bool {
    let mut normalized = component
        .chars()
        .filter(|character| !matches!(character, '\u{200c}'..='\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{206a}'..='\u{206f}' | '\u{feff}'));
    expected.into_iter().all(|expected| {
        normalized
            .next()
            .is_some_and(|actual| actual.eq_ignore_ascii_case(&expected))
    }) && normalized.next().is_none()
}

pub(super) fn validate_backend_config(config: &RepoConfig) -> Result<()> {
    if !config.backend_language.is_go() && config.go_database.is_postgres() {
        bail!(
            "go_database = \"{}\" requires backend_language = \"go\" in .jig.toml",
            config.go_database.as_str()
        );
    }
    if config.backend_language.is_go() && config.sqlx_enabled {
        bail!(
            "backend_language = \"go\" cannot be combined with sqlx_enabled = true in .jig.toml; Go repositories use go_database and Goose/sqlc, while SQLx is owned by the Rust backend"
        );
    }
    let migration_dir = (!config.migration_dir.trim().is_empty())
        .then(|| normalize_portable_repository_directory(&config.migration_dir, "migration_dir"))
        .transpose()?;
    let rust_migration_dir = (!config.rust_migration_dir.trim().is_empty())
        .then(|| {
            normalize_portable_repository_directory(
                &config.rust_migration_dir,
                "legacy rust_migration_dir",
            )
        })
        .transpose()?;
    if config.sqlx_enabled
        && let (Some(migration_dir), Some(rust_migration_dir)) =
            (&migration_dir, &rust_migration_dir)
        && migration_dir != rust_migration_dir
    {
        bail!(
            "migration_dir = {:?} and legacy rust_migration_dir = {:?} must identify the same SQLx migration directory in .jig.toml; keep migration_dir as the canonical value and synchronize the compatibility key",
            config.migration_dir,
            config.rust_migration_dir
        );
    }
    Ok(())
}

pub(super) fn validate_frontend_app_roles(config: &RepoConfig) -> Result<()> {
    for app in &config.frontend_apps {
        normalize_portable_repo_path(
            &app.dir,
            &format!("dir for frontend app '{}' in [[frontend_apps]]", app.name),
        )?;
        if app
            .kind
            .as_deref()
            .is_some_and(|kind| !is_supported_frontend_kind(kind))
        {
            bail!(
                "Invalid frontend app kind '{}' for '{}'. Expected 'vite' or 'env-port'.",
                app.kind.as_deref().unwrap_or_default(),
                app.name
            );
        }
        if app
            .role
            .as_deref()
            .is_some_and(|role| !matches!(role, "spa" | "admin" | "astro"))
        {
            bail!(
                "Invalid frontend app role '{}' for '{}'. Expected 'spa', 'admin', or 'astro'.",
                app.role.as_deref().unwrap_or_default(),
                app.name
            );
        }
    }
    Ok(())
}

pub(super) fn validate_command_map(commands: &BTreeMap<String, String>) -> Result<()> {
    for key in commands.keys() {
        if !is_safe_command_key(key) {
            bail!(
                "Invalid [commands] key '{key}'. Use lowercase ASCII letters, numbers, and underscores, start with a letter, and end command keys with '_command'."
            );
        }
    }
    Ok(())
}

pub(super) fn is_safe_command_key(value: &str) -> bool {
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

pub(super) fn validate_vault_config(config: &RepoConfig) -> Result<()> {
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

pub(super) fn validate_vault_scope_id(scope_id: &str) -> Result<()> {
    if !crate::command::is_valid_vault_scope_id(scope_id) {
        bail!(
            "[vault].scope_id must be 1 to 128 bytes and may only contain letters, digits, '_', or '-'"
        );
    }
    Ok(())
}

pub(super) fn validate_dev_config(config: &RepoConfig) -> Result<()> {
    let mut app_names = HashSet::new();
    for app in &config.dev.apps {
        if let Some(dir) = app.dir.as_deref() {
            normalize_portable_repo_path(
                dir,
                &format!("dir for dev app '{}' in [[dev.apps]]", app.name),
            )?;
        }
        if !is_supported_frontend_kind(&app.kind) {
            bail!(
                "Invalid dev app kind '{}' for '{}' in [[dev.apps]]. Expected 'vite' or 'env-port'.",
                app.kind,
                app.name
            );
        }
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
                Some(dev_dir) if config_app_dirs_match(dev_dir, &frontend_app.dir) => {}
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

pub(crate) fn config_app_dirs_match(left: &str, right: &str) -> bool {
    match (
        normalize_portable_repo_path(left, "configured app dir"),
        normalize_portable_repo_path(right, "configured app dir"),
    ) {
        (Ok(left), Ok(right)) => left == right,
        _ => false,
    }
}

pub(super) fn is_supported_frontend_kind(kind: &str) -> bool {
    matches!(kind, "vite" | "env-port")
}

pub(super) fn validate_dev_app_env_prefixes<'a>(
    names: impl IntoIterator<Item = &'a str>,
    section: &str,
) -> Result<()> {
    let mut prefixes = BTreeMap::new();
    for name in names {
        let prefix = jig_core::dev_app_env_prefix(name);
        if let Some(previous) = prefixes.insert(prefix.clone(), name) {
            bail!(
                "{section} entries '{previous}' and '{name}' share derived dev environment prefix {prefix}; rename one app so punctuation-normalized names are unique"
            );
        }
    }
    Ok(())
}

#[cfg(test)]
mod dev_proxy_validation_tests {
    use super::*;

    #[test]
    fn dev_proxy_validation_vectors_match_the_runtime_contract() {
        for tld in ["localhost", "Example.TEST", "corp.internal", "local"] {
            validate_dev_proxy_settings(1355, Some(1443), tld, false).unwrap();
        }
        for tld in ["", "dev", "example.com", "too.deep.test", "bad,tld"] {
            assert!(
                validate_dev_proxy_settings(1355, Some(1443), tld, false).is_err(),
                "accepted invalid dev TLD {tld:?}"
            );
        }
        assert!(validate_dev_proxy_settings(0, Some(1443), "localhost", false).is_err());
        assert!(validate_dev_proxy_settings(0, Some(1443), "localhost", true).is_ok());
        assert!(validate_dev_proxy_settings(1355, Some(0), "localhost", false).is_err());
        assert!(validate_dev_proxy_settings(1355, Some(1355), "localhost", false).is_err());
    }
}
