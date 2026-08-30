use super::*;

#[derive(Clone, Debug, Default, Deserialize)]
pub(super) struct RawAnswers {
    pub(super) repository: Option<AuthoredRepositoryModel>,
    pub(super) repo_name: Option<String>,
    pub(super) go_module: Option<String>,
    pub(super) default_branch: Option<String>,
    pub(super) ci_github_runner: Option<String>,
    pub(super) jig_version: Option<String>,
    pub(super) template_source_url: Option<String>,
    #[serde(default)]
    pub(super) harness_footprint: Option<HarnessFootprint>,
    pub(super) backend_language: Option<BackendLanguage>,
    #[serde(skip)]
    pub(super) repository_projection_hint: RepositoryProjectionHint,
    pub(super) go_database: Option<GoDatabase>,
    pub(super) sqlx_enabled: Option<bool>,
    pub(super) rust_crate_roots: Option<Vec<String>>,
    pub(super) rust_migration_dir: Option<String>,
    pub(super) migration_dir: Option<String>,
    pub(super) rust_migration_layout: Option<RustMigrationLayout>,
    pub(super) rust_sqlx_metadata_dir: Option<String>,
    pub(super) schema_dump_enabled: Option<bool>,
    pub(super) schema_dump_command: Option<String>,
    pub(super) schema_docs_dir: Option<String>,
    pub(super) schema_check_command: Option<String>,
    pub(super) sqlx_check_command: Option<String>,
    pub(super) migration_add_command: Option<String>,
    pub(super) bootstrap_command: Option<String>,
    pub(super) contract_check_command: Option<String>,
    pub(super) dev_command: Option<String>,
    pub(super) rust_fmt_check_command: Option<String>,
    pub(super) rust_clippy_command: Option<String>,
    pub(super) rust_test_command: Option<String>,
    pub(super) rust_test_locked_command: Option<String>,
    pub(super) go_fmt_check_command: Option<String>,
    pub(super) go_lint_command: Option<String>,
    pub(super) go_test_command: Option<String>,
    pub(super) go_test_locked_command: Option<String>,
    pub(super) sqlc_check_command: Option<String>,
    pub(super) typescript_lint_command: Option<String>,
    pub(super) typescript_typecheck_command: Option<String>,
    pub(super) typescript_build_command: Option<String>,
    pub(super) typescript_coverage_command: Option<String>,
    pub(super) web_package_manager: Option<String>,
    pub(super) application_contracts_enabled: Option<bool>,
    pub(super) frontend_apps: Option<Vec<FrontendApp>>,
    #[serde(default)]
    pub(super) frontend_workspace_roots: Option<Vec<String>>,
    pub(super) dev: Option<dev::RawDevAnswers>,
    pub(super) vault: Option<vault::VaultAnswers>,
    pub(super) status: Option<StatusConfig>,
    pub(super) execution: Option<ExecutionConfig>,
    pub(super) agent_tooling: Option<AgentToolingAnswers>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub(super) struct AgentToolingAnswers {
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
pub(super) struct CodexMarketplaceAnswers {
    pub(super) id: String,
    pub(super) source: String,
    #[serde(default)]
    pub(super) plugins: Vec<String>,
}

impl RawAnswers {
    pub(super) fn normalize_repository_model(&mut self, table: &toml::Table) {
        let Some(repository) = table.get("repository").and_then(toml::Value::as_table) else {
            return;
        };
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
        let complete_authored_model = self.repository.as_ref().is_some_and(|repository| {
            !repository.components.is_empty()
                && !repository.actions.is_empty()
                && !repository.profiles.is_empty()
        });
        if complete_authored_model || self.backend_language.is_none() {
            // Legacy templates still need one compatibility value, but an
            // authored v6 model may contain both languages. Prefer Rust when
            // SQLx or a Rust adapter is present so the legacy Go + SQLx
            // validation cannot reject a valid mixed component model.
            self.backend_language = Some(
                if has_adapter("go") && !has_adapter("rust") && !has_adapter("sqlx") {
                    BackendLanguage::Go
                } else {
                    BackendLanguage::Rust
                },
            );
        }
        if complete_authored_model {
            self.go_database = has_adapter("go-postgres").then_some(GoDatabase::Postgres);
        } else if self.go_database.is_none() && has_adapter("go-postgres") {
            self.go_database = Some(GoDatabase::Postgres);
        }
        if complete_authored_model || self.sqlx_enabled.is_none() {
            self.sqlx_enabled = Some(has_adapter("sqlx"));
        }
        let Some(commands) = table.get("commands").and_then(toml::Value::as_table) else {
            return;
        };
        inherit_repository_command(
            &mut self.bootstrap_command,
            commands,
            "repo_bootstrap_command",
        );
        if self.backend_language == Some(BackendLanguage::Rust) {
            inherit_repository_command(
                &mut self.rust_fmt_check_command,
                commands,
                "api_fmt_command",
            );
            inherit_repository_command(
                &mut self.rust_clippy_command,
                commands,
                "api_clippy_command",
            );
            inherit_repository_command(&mut self.rust_test_command, commands, "api_test_command");
            inherit_repository_command(
                &mut self.rust_test_locked_command,
                commands,
                "api_test_locked_command",
            );
        } else {
            inherit_repository_command(&mut self.go_fmt_check_command, commands, "api_fmt_command");
            inherit_repository_command(&mut self.go_lint_command, commands, "api_lint_command");
            inherit_repository_command(&mut self.go_test_command, commands, "api_test_command");
            inherit_repository_command(
                &mut self.go_test_locked_command,
                commands,
                "api_test_locked_command",
            );
        }
        inherit_repository_command(&mut self.sqlx_check_command, commands, "api_sqlx_command");
        inherit_repository_command(
            &mut self.schema_dump_command,
            commands,
            "api_schema_dump_command",
        );
        inherit_repository_command(&mut self.sqlc_check_command, commands, "api_sqlc_command");
        inherit_repository_command(
            &mut self.typescript_lint_command,
            commands,
            "repo_compat_typescript_lint_command",
        );
        inherit_repository_command(
            &mut self.typescript_typecheck_command,
            commands,
            "repo_compat_typescript_typecheck_command",
        );
        inherit_repository_command(
            &mut self.typescript_build_command,
            commands,
            "repo_compat_typescript_build_command",
        );
        inherit_repository_command(
            &mut self.typescript_coverage_command,
            commands,
            "repo_compat_typescript_coverage_command",
        );
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
            .try_into::<Self>()
            .with_context(|| format!("Failed to parse {}", path.display()))?;
        raw.normalize_repository_model(&table);
        raw.normalize_app_dirs()?;
        raw.normalize_legacy_frontend_metadata(&table);
        Ok(raw)
    }

    pub(super) fn normalize_legacy_frontend_metadata(&mut self, table: &toml::Table) {
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

    pub(super) fn merge_opts(&mut self, opts: &AnswerOpts) {
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
        self.repository_projection_hint = opts.repository_projection_hint;
        merge_option(&mut self.go_database, opts.go_database);
        merge_option(&mut self.sqlx_enabled, opts.sqlx_enabled);
        if !opts.rust_crate_roots.is_empty() {
            self.rust_crate_roots = Some(opts.rust_crate_roots.clone());
        }
        merge_option(
            &mut self.rust_migration_dir,
            opts.rust_migration_dir.clone(),
        );
        merge_option(&mut self.migration_dir, opts.migration_dir.clone());
        merge_option(&mut self.rust_migration_layout, opts.rust_migration_layout);
        merge_option(
            &mut self.rust_sqlx_metadata_dir,
            opts.rust_sqlx_metadata_dir.clone(),
        );
        merge_option(&mut self.schema_dump_enabled, opts.schema_dump_enabled);
        merge_option(
            &mut self.schema_dump_command,
            opts.schema_dump_command.clone(),
        );
        merge_option(&mut self.schema_docs_dir, opts.schema_docs_dir.clone());
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
        merge_option(
            &mut self.application_contracts_enabled,
            opts.application_contracts_enabled,
        );
        if !opts.frontend_apps.is_empty() {
            self.frontend_apps = Some(opts.frontend_apps.clone());
        }
        if !opts.frontend_workspace_roots.is_empty() {
            self.frontend_workspace_roots = Some(opts.frontend_workspace_roots.clone());
        }
        if !opts.dev_apps.is_empty() {
            self.dev
                .get_or_insert_with(dev::RawDevAnswers::default)
                .apps = Some(opts.dev_apps.clone());
        }
        merge_option(&mut self.status, opts.status.clone());
        merge_option(&mut self.execution, opts.execution.clone());
    }

    pub(super) fn normalize_app_dirs(&mut self) -> Result<()> {
        if let Some(frontend_apps) = self.frontend_apps.as_mut() {
            for app in frontend_apps {
                app.dir = normalize_portable_repo_path(
                    &app.dir,
                    &format!("frontend app '{}' dir", app.name),
                )?;
            }
        }
        if let Some(workspace_roots) = self.frontend_workspace_roots.as_mut() {
            for root in workspace_roots.iter_mut() {
                *root = normalize_generated_gate_root(root, "frontend workspace root")?;
            }
            workspace_roots.sort();
            workspace_roots.dedup();
        }
        if let Some(schema_docs_dir) = self.schema_docs_dir.as_mut() {
            *schema_docs_dir = normalize_generated_gate_root(schema_docs_dir, "schema_docs_dir")?;
            validate_schema_docs_dir(schema_docs_dir)?;
        }
        if let Some(dev_apps) = self.dev.as_mut().and_then(|dev| dev.apps.as_mut()) {
            for app in dev_apps {
                if let Some(dir) = app.dir.as_mut() {
                    *dir =
                        normalize_portable_repo_path(dir, &format!("dev app '{}' dir", app.name))?;
                }
            }
        }
        Ok(())
    }

    pub(super) fn into_answer_opts(self, answers_file: Option<PathBuf>) -> AnswerOpts {
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
            repository_projection_hint: self.repository_projection_hint,
            go_database: self.go_database,
            scaffold_go_component_roots: Vec::new(),
            sqlx_enabled: self.sqlx_enabled,
            rust_crate_roots: self.rust_crate_roots.unwrap_or_default(),
            rust_migration_dir: self.rust_migration_dir.filter(|value| !value.is_empty()),
            migration_dir: self.migration_dir.filter(|value| !value.is_empty()),
            rust_migration_layout: self.rust_migration_layout,
            rust_sqlx_metadata_dir: self.rust_sqlx_metadata_dir,
            schema_dump_enabled: self.schema_dump_enabled,
            schema_dump_command: self.schema_dump_command,
            schema_docs_dir: self.schema_docs_dir,
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
            application_contracts_enabled: self.application_contracts_enabled,
            frontend_apps: self.frontend_apps.unwrap_or_default(),
            frontend_workspace_roots: self.frontend_workspace_roots.unwrap_or_default(),
            dev_apps,
            status: self.status,
            execution: self.execution,
        }
    }

    pub(super) fn normalize_legacy_sqlx_disabled_schema_dump(&mut self) {
        if self.sqlx_enabled == Some(false) && self.schema_dump_enabled == Some(true) {
            self.schema_dump_enabled = Some(false);
        }
    }

    pub(super) fn normalize_legacy_generated_cargo_command_defaults(&mut self) {
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

    pub(super) fn apply_sqlx_default_for_cli_defaults(&mut self) -> bool {
        // CLI `--defaults` should not block on optional feature setup. Without
        // a migration dir, resolve to the tooling-only profile instead of
        // making noninteractive adoption stop for SQLx configuration.
        if self.sqlx_enabled.is_none()
            && self.migration_dir.as_deref().is_none_or(str::is_empty)
            && self.rust_migration_dir.as_deref().is_none_or(str::is_empty)
            && self.schema_dump_enabled != Some(true)
        {
            self.sqlx_enabled = Some(false);
            return true;
        }
        false
    }

    pub(super) fn apply_existing_vault_default(
        &mut self,
        destination: &Path,
    ) -> Result<Option<String>> {
        if self.vault.is_some() {
            return Ok(None);
        }
        vault::apply_existing_default(&mut self.vault, destination)
    }

    #[cfg(test)]
    pub(super) fn resolve(self, default_repo_name: Option<String>) -> Result<RenderAnswers> {
        self.resolve_with_authored_repository(default_repo_name, None)
    }

    pub(super) fn resolve_with_authored_repository(
        mut self,
        default_repo_name: Option<String>,
        authored_repository: Option<AuthoredRepositoryModel>,
    ) -> Result<RenderAnswers> {
        self.normalize_app_dirs()?;
        let authored_has_rust_backend = authored_repository
            .as_ref()
            .is_some_and(|model| model.has_adapter("rust") || model.has_adapter("sqlx"));
        let authored_rust_crate_roots = authored_repository.as_ref().map(|model| {
            if model.rust_workspace_guidance_enabled() {
                self.rust_crate_roots
                    .clone()
                    .unwrap_or_else(|| vec!["crates".into()])
            } else {
                model
                    .components
                    .iter()
                    .filter(|component| {
                        component
                            .adapters
                            .iter()
                            .any(|adapter| matches!(adapter.as_str(), "rust" | "sqlx"))
                    })
                    .map(|component| component.root.clone())
                    .collect::<BTreeSet<_>>()
                    .into_iter()
                    .collect::<Vec<_>>()
            }
        });
        let backend_language = self.backend_language.unwrap_or_default();
        let go_database = self.go_database.unwrap_or_default();
        // A complete authored model owns backend capabilities. The singular
        // compatibility projection is authoritative only for legacy answers.
        let go_postgres_migrations_enabled = authored_repository.as_ref().map_or_else(
            || backend_language.is_go() && go_database.is_postgres(),
            |model| model.has_adapter("go-postgres"),
        );
        let repo_name = self
            .repo_name
            .filter(|value| !value.is_empty())
            .or(default_repo_name)
            .ok_or_else(|| anyhow::anyhow!("Missing required answer: repo_name"))?;
        let sqlx_enabled = self.sqlx_enabled.unwrap_or(true);
        let rust_migration_layout = self.rust_migration_layout.unwrap_or_default();
        if backend_language.is_go() && sqlx_enabled && !authored_has_rust_backend {
            bail!(
                "backend_language = \"go\" cannot be combined with sqlx_enabled = true; Go repositories use --go-database and Goose/sqlc, while SQLx is owned by the Rust backend"
            );
        }
        let migration_dir_answer = self
            .migration_dir
            .filter(|value| !value.is_empty())
            .map(|value| normalize_portable_repository_directory(&value, "migration_dir"))
            .transpose()?;
        let legacy_rust_migration_dir = self
            .rust_migration_dir
            .filter(|value| !value.is_empty())
            .map(|value| normalize_portable_repository_directory(&value, "rust_migration_dir"))
            .transpose()?;
        if sqlx_enabled
            && let (Some(migration_dir), Some(rust_migration_dir)) =
                (&migration_dir_answer, &legacy_rust_migration_dir)
            && migration_dir != rust_migration_dir
        {
            bail!(
                "migration_dir = {migration_dir:?} and rust_migration_dir = {rust_migration_dir:?} must identify the same SQLx migration directory; keep migration_dir canonical and remove or synchronize rust_migration_dir"
            );
        }
        if sqlx_enabled && migration_dir_answer.is_none() && legacy_rust_migration_dir.is_none() {
            bail!(
                "Missing required answer when sqlx_enabled is true (including when schema_dump_enabled implies SQLx): migration_dir. Pass --rust-migration-dir <dir> to populate the canonical SQLx migration directory, or pass --sqlx-enabled false with schema_dump_enabled = false for tooling-only repos."
            );
        }
        if !sqlx_enabled && self.schema_dump_enabled == Some(true) {
            bail!(
                "schema_dump_enabled cannot be true when sqlx_enabled is false; enable SQLx or set schema_dump_enabled = false"
            );
        }

        let frontend_apps = self.frontend_apps.unwrap_or_default();
        validate_frontend_apps(&frontend_apps)?;
        let mut frontend_workspace_roots = self.frontend_workspace_roots.unwrap_or_default();
        for root in &mut frontend_workspace_roots {
            *root = normalize_generated_gate_root(root, "frontend workspace root")?;
        }
        frontend_workspace_roots.retain(|root| {
            !frontend_apps
                .iter()
                .any(|app| config_app_dirs_match(&app.dir, root))
        });
        frontend_workspace_roots.sort();
        frontend_workspace_roots.dedup();
        let dev::ResolvedDevApps {
            dev_apps,
            generated_frontend_dev_apps,
        } = dev::resolve(frontend_apps.as_slice(), self.dev)?;
        let vault = self.vault.unwrap_or_else(vault::default_answers);
        vault::validate_answers(&vault)?;
        let status = self.status.unwrap_or_default();
        status.validate()?;
        let execution = self.execution.unwrap_or_default();
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
        let schema_docs_dir = normalize_generated_gate_root(
            self.schema_docs_dir.as_deref().unwrap_or("docs/schema"),
            "schema_docs_dir",
        )?;
        validate_schema_docs_dir(&schema_docs_dir)?;
        let rust_sqlx_metadata_dir = self.rust_sqlx_metadata_dir.or_else(|| Some(".sqlx".into()));
        let sqlx_check_command = self.sqlx_check_command.unwrap_or_else(|| {
            let metadata_dir = rust_sqlx_metadata_dir.as_deref().unwrap_or(".sqlx");
            format!(
                "CARGO=cargo SQLX_OFFLINE=false SQLX_OFFLINE_DIR={} sqlx prepare --check --workspace -- --workspace --all-targets",
                shell_quote(metadata_dir)
            )
        });
        let migration_add_command = self.migration_add_command;
        let migration_dir = migration_dir_answer.or_else(|| {
            if go_postgres_migrations_enabled {
                Some(GO_POSTGRES_MIGRATION_DIR.into())
            } else {
                legacy_rust_migration_dir.clone()
            }
        });
        let rust_migration_dir = if sqlx_enabled {
            migration_dir.clone()
        } else {
            legacy_rust_migration_dir
        };

        Ok(RenderAnswers {
            authored_repository,
            authored_repository_commands: BTreeMap::new(),
            scaffolded_frontend_contracts: false,
            go_postgres_integration_script: false,
            repository_projection_hint: self.repository_projection_hint,
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
            go_toolchain_authority_path: GO_TOOLCHAIN_AUTHORITY_PATH,
            sqlx_enabled,
            rust_crate_roots: authored_rust_crate_roots.unwrap_or_else(|| {
                if backend_language == BackendLanguage::Go {
                    Vec::new()
                } else {
                    self.rust_crate_roots
                        .unwrap_or_else(|| vec!["crates".into()])
                }
            }),
            rust_migration_dir,
            migration_dir,
            rust_migration_layout,
            rust_sqlx_metadata_dir,
            schema_dump_enabled,
            schema_dump_command,
            schema_docs_dir,
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
            go_fmt_check_command: self.go_fmt_check_command.unwrap_or_else(|| "set -o pipefail; files=$(git ls-files --cached --others --exclude-standard -z -- '*.go' | while IFS= read -r -d '' file; do if [ -f \"$file\" ]; then printf '%s\\0' \"$file\"; fi; done | xargs -0 gofmt -l --) || exit $?; test -z \"$files\" || { printf '%s\\n' \"$files\"; exit 1; }".into()),
            go_lint_command: self
                .go_lint_command
                .unwrap_or_else(|| "go vet ./...".into()),
            go_test_command: self
                .go_test_command
                .unwrap_or_else(|| "go test ./...".into()),
            go_test_locked_command: self.go_test_locked_command.unwrap_or_else(|| {
                "go mod verify && go test -mod=readonly ./...".into()
            }),
            sqlc_check_command: self
                .sqlc_check_command
                .unwrap_or_else(|| "go tool sqlc vet && go tool sqlc diff".into()),
            web_package_manager,
            application_contracts_enabled: self.application_contracts_enabled.unwrap_or(false),
            web_package_manager_spec,
            web_package_manager_version,
            node_version: GENERATED_NODE_VERSION.into(),
            web_install_command,
            web_run_command,
            typescript_lint_command: self
                .typescript_lint_command
                .unwrap_or_else(|| "scripts/check-webapps.sh lint".into()),
            typescript_typecheck_command: self
                .typescript_typecheck_command
                .unwrap_or_else(|| "scripts/check-webapps.sh typecheck".into()),
            typescript_build_command: self
                .typescript_build_command
                .unwrap_or_else(|| "scripts/check-webapps.sh build".into()),
            typescript_coverage_command: self
                .typescript_coverage_command
                .unwrap_or_else(|| "scripts/check-webapps.sh coverage".into()),
            dev_apps,
            generated_frontend_dev_apps,
            frontend_apps,
            frontend_workspace_roots,
            vault,
            status,
            execution,
            agent_tooling: self.agent_tooling.unwrap_or_default(),
        })
    }
}
