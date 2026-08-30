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

    fn from_init_opts_at(opts: &AnswerOpts, path_base: &Path) -> Result<Self> {
        Self::from_init_opts_at_with_reader(opts, path_base, |path| fs::read_to_string(path))
    }

    fn from_init_opts_at_with_reader(
        opts: &AnswerOpts,
        path_base: &Path,
        read: impl FnOnce(&Path) -> std::io::Result<String>,
    ) -> Result<Self> {
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
        let mut input = Self::from_file_with_reader(&path, read)?;
        input.preserve_repository_model = true;
        Ok(input)
    }

    pub(super) fn from_file(path: &Path) -> Result<Self> {
        Self::from_file_with_reader(path, |path| fs::read_to_string(path))
    }

    fn from_file_with_reader(
        path: &Path,
        read: impl FnOnce(&Path) -> std::io::Result<String>,
    ) -> Result<Self> {
        let text = read(path).with_context(|| format!("Failed to read {}", path.display()))?;
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
        input.validate_explicit_file_semantics()?;
        input.preserve_repository_model = true;
        Ok(input)
    }

    fn validate_explicit_file_semantics(&self) -> Result<()> {
        if self
            .raw
            .repository
            .as_ref()
            .is_some_and(AuthoredRepositoryModel::is_complete)
            && self.authored_repository_commands.is_none()
        {
            bail!(
                "A complete authored [repository] model requires [commands] to be a table of string values"
            );
        }
        Ok(())
    }

    fn validate_rust_library(&self, scaffold: &ScaffoldOpts, answers: &AnswerOpts) -> Result<()> {
        let mut raw = self.raw.clone();
        raw.merge_opts(answers);

        for key in ["repository", "commands", "work", "loop"] {
            if self.shape.contains_top_level_key(key) {
                return reject_rust_library_input(key);
            }
        }
        if let Some(key) = self.raw.first_extra_top_level_key() {
            return reject_rust_library_input(&format!("unknown top-level answer key `{key}`"));
        }
        if !scaffold.frontends.is_empty() {
            return reject_rust_library_input("--frontend");
        }
        if !scaffold.frontend_list.is_empty() {
            return reject_rust_library_input("--frontends");
        }
        if scaffold.db.is_some() {
            return reject_rust_library_input("--db");
        }
        if raw.go_module.as_deref().is_some_and(nonempty_answer_string) {
            return reject_rust_library_input("--go-module / go_module");
        }
        if self.shape.contains_key("go_database") {
            return reject_rust_library_input("go_database");
        }
        if raw.backend_language == Some(BackendLanguage::Go) {
            return reject_rust_library_input("backend_language = \"go\"");
        }
        if raw.harness_footprint == Some(HarnessFootprint::Minimal) {
            return reject_rust_library_input("harness_footprint = \"minimal\"");
        }
        if let Some(roots) = raw.rust_crate_roots.as_deref()
            && (roots.len() != 1 || roots[0] != "crates")
        {
            return reject_rust_library_input("rust_crate_roots");
        }
        if raw.sqlx_enabled == Some(true) {
            return reject_rust_library_input("sqlx_enabled = true");
        }
        if raw.schema_dump_enabled == Some(true) {
            return reject_rust_library_input("schema_dump_enabled = true");
        }
        if raw.rust_migration_layout.is_some() || self.shape.contains_key("rust_migration_layout") {
            return reject_rust_library_input("rust_migration_layout");
        }

        for (key, value) in [
            ("rust_migration_dir", raw.rust_migration_dir.as_deref()),
            ("migration_dir", raw.migration_dir.as_deref()),
            (
                "rust_sqlx_metadata_dir",
                raw.rust_sqlx_metadata_dir.as_deref(),
            ),
            ("schema_dump_command", raw.schema_dump_command.as_deref()),
            ("schema_docs_dir", raw.schema_docs_dir.as_deref()),
            ("schema_check_command", raw.schema_check_command.as_deref()),
            ("sqlx_check_command", raw.sqlx_check_command.as_deref()),
            (
                "migration_add_command",
                raw.migration_add_command.as_deref(),
            ),
            ("go_fmt_check_command", raw.go_fmt_check_command.as_deref()),
            ("go_lint_command", raw.go_lint_command.as_deref()),
            ("go_test_command", raw.go_test_command.as_deref()),
            (
                "go_test_locked_command",
                raw.go_test_locked_command.as_deref(),
            ),
            ("sqlc_check_command", raw.sqlc_check_command.as_deref()),
            (
                "typescript_lint_command",
                raw.typescript_lint_command.as_deref(),
            ),
            (
                "typescript_typecheck_command",
                raw.typescript_typecheck_command.as_deref(),
            ),
            (
                "typescript_build_command",
                raw.typescript_build_command.as_deref(),
            ),
            (
                "typescript_coverage_command",
                raw.typescript_coverage_command.as_deref(),
            ),
            ("dev_command", raw.dev_command.as_deref()),
        ] {
            if value.is_some_and(nonempty_answer_string) {
                return reject_rust_library_input(key);
            }
        }
        if raw
            .frontend_apps
            .as_ref()
            .is_some_and(|apps| !apps.is_empty())
        {
            return reject_rust_library_input("frontend_apps");
        }
        if raw
            .frontend_workspace_roots
            .as_ref()
            .is_some_and(|roots| !roots.is_empty())
        {
            return reject_rust_library_input("frontend_workspace_roots");
        }
        if raw
            .dev
            .as_ref()
            .and_then(|dev| dev.apps.as_ref())
            .is_some_and(|apps| !apps.is_empty())
        {
            return reject_rust_library_input("dev.apps");
        }
        if raw.application_contracts_enabled == Some(true) {
            return reject_rust_library_input("application_contracts_enabled = true");
        }
        Ok(())
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
