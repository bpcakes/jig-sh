//! The execution-authority digest: the canonical hash of every configuration
//! input that can change what a planned repository action executes.

use super::*;

#[derive(Serialize)]
struct RepositoryExecutionAuthority<'a> {
    schema_version: u32,
    manifest: &'a serde_json::Value,
    harness_footprint: HarnessFootprintConfig,
    backend_language: BackendLanguage,
    go_database: GoDatabase,
    sqlx_enabled: bool,
    rust_crate_roots: &'a [String],
    migration_dir: &'a str,
    rust_migration_dir: &'a str,
    rust_migration_layout: RustMigrationLayout,
    rust_sqlx_metadata_dir: &'a str,
    schema_dump_enabled: bool,
    commands: BTreeMap<String, &'a str>,
    frontend_apps: &'a [FrontendAppConfig],
    /// Epochs through 8 always hash a work authority, even for an absent
    /// `[work]`, so their digests stay stable; later epochs have none.
    #[serde(skip_serializing_if = "Option::is_none")]
    work: Option<work_config::WorkExecutionAuthority<'a>>,
    execution: &'a ExecutionConfig,
}

pub(super) fn contract_source_digest(
    config: &RepoConfig,
    manifest: &serde_json::Value,
) -> Result<String> {
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
        harness_footprint,
        backend_language,
        go_database,
        sqlx_enabled,
        rust_crate_roots,
        rust_migration_dir,
        migration_dir,
        rust_migration_layout,
        rust_sqlx_metadata_dir,
        schema_dump_enabled,
        schema_dump_command: _,
        schema_docs_dir: _,
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
        application_contracts_enabled: _,
        frontend_apps,
        frontend_workspace_roots: _,
        repository: _,
        vault: _,
        dev: _,
        work,
        loop_config: _,
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
    let declares_work_authority = manifest
        .get("contract_version")
        .and_then(serde_json::Value::as_u64)
        .is_none_or(|version| version <= u64::from(LAST_WORK_CONFIG_CONTRACT_VERSION));
    let absent_work = WorkConfig::default();
    let authority = RepositoryExecutionAuthority {
        schema_version: 2,
        manifest,
        harness_footprint: *harness_footprint,
        backend_language: *backend_language,
        go_database: *go_database,
        sqlx_enabled: *sqlx_enabled,
        rust_crate_roots,
        migration_dir,
        rust_migration_dir,
        rust_migration_layout: *rust_migration_layout,
        rust_sqlx_metadata_dir,
        schema_dump_enabled: *schema_dump_enabled,
        commands: effective_commands,
        frontend_apps,
        work: declares_work_authority
            .then(|| work.as_ref().unwrap_or(&absent_work).execution_authority()),
        execution,
    };
    let encoded = serde_json::to_vec(&authority)
        .context("Failed to canonicalize repository execution authority")?;
    let mut hasher = Sha256::new();
    hasher.update(b"jig-repository-execution-authority-v2\0");
    hasher.update((encoded.len() as u64).to_be_bytes());
    hasher.update(encoded);
    Ok(format!("sha256:{:x}", hasher.finalize()))
}
