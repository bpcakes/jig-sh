
fn initial_next_steps(
    command: InitialCommand,
    destination: &Path,
    result: &initial_copy::BootstrapCopyResult,
    database_config_required: bool,
) -> Vec<String> {
    let destination_for_cd = destination
        .canonicalize()
        .unwrap_or_else(|_| destination.to_path_buf());
    let mut steps = vec![format!(
        "cd {}",
        crate::shell::quote(&destination_for_cd.display().to_string())
    )];
    if command == InitialCommand::Adopt && result.apply_report.dry_run {
        steps.push("Review the adoption preview and managed-file diff.".into());
        if result.minimal_footprint {
            if result.full_to_minimal_transition {
                steps.push("jig adopt . --minimal --write --force".into());
            } else {
                steps.push(
                    "Re-run jig adopt . --minimal --write after reviewing the summary.".into(),
                );
            }
        } else {
            steps.push("Re-run jig adopt . --write after reviewing the summary.".into());
        }
        steps.push("No files were changed by this preview.".into());
        return steps;
    }
    if result.minimal_footprint {
        steps.push(
            "Add [[loop.workflows]] entries to .jig.toml, then run jig loop tick / jig loop run."
                .into(),
        );
        steps.push(
            "Re-run jig adopt . --write (without --minimal) when you want the full harness.".into(),
        );
        if command == InitialCommand::Adopt {
            steps.push("Commit the adoption diff after reviewing .jig.toml and .agent/.".into());
        }
        return steps;
    }
    if database_config_required {
        steps.push(
            "Export DATABASE_URL, or copy .env.example to .env and configure it before bootstrap."
                .into(),
        );
    }
    steps.push("scripts/jig setup".into());
    steps.push("scripts/jig check test".into());
    if result.dev_apps_configured {
        steps.push("scripts/jig dev".into());
    }
    if result.sqlx_enabled {
        steps.push(
            "Run scripts/jig check sqlx after database access is configured; doctor flags missing cargo-sqlx or a build that lacks the configured database driver."
                .into(),
        );
    }
    if result.schema_dump_enabled {
        steps.push("Provide scripts/dump-schema.sh, then run scripts/jig sqlx schema dump.".into());
    }
    if command == InitialCommand::Adopt {
        steps.push("Commit the adoption diff after generated checks pass.".into());
    }
    steps
}

fn initial_notes(
    extra_notes: Vec<String>,
    frontend_apps_configured: bool,
    scaffold_plan: Option<&scaffold::InitScaffoldPlan>,
    minimal_footprint: bool,
) -> Vec<String> {
    let mut notes = if minimal_footprint {
        vec![
            "Minimal adoption wrote .jig.toml and .agent/ scaffolding only; scripts/, workflows, AGENTS.md, agent-map.md, and .mcp.json were omitted.".into(),
            "harness_footprint = \"minimal\" is stored in .jig.toml so jig update keeps the same footprint until you re-adopt without --minimal.".into(),
            "Invoke the installed jig binary directly for loop commands; there is no scripts/jig launcher yet.".into(),
        ]
    } else {
        vec![
            "The first scripts/jig command may install or compile a compatible Jig runtime into this repo's contract/profile cache.".into(),
            "Review generated .jig.toml, AGENTS.md, agent-map.md, and check commands before relying on the harness.".into(),
            "Re-run scripts/jig doctor after setup changes to confirm readiness.".into(),
            "Full gates remain available through scripts/jig work gates or scripts/jig check <gate>.".into(),
        ]
    };
    if scaffold_plan.is_some() {
        notes.push(
            "Scaffolded project code is project-owned after creation. jig update keeps the Jig harness current and does not rewrite project code."
                .into(),
        );
    }
    if frontend_apps_configured && !minimal_footprint {
        notes.push(
            "Frontend checks expect package scripts for lint, typecheck, build:bundle, and test:coverage plus a package-manager lockfile; generated preset apps include them."
                .into(),
        );
        notes.push(
            "Frontend gates are available as scripts/jig check typescript-lint, typescript-typecheck, typescript-build, and typescript-coverage."
                .into(),
        );
    }
    if !minimal_footprint {
        notes.push(
            "Policy gates are available as scripts/jig check contract and scripts/jig check agent-guides when evidence is needed."
                .into(),
        );
    }
    if let Some(note) = scaffold_plan.and_then(scaffold::InitScaffoldPlan::sanitized_repo_name_note)
    {
        notes.push(note);
    }
    notes.extend(extra_notes);
    notes
}

fn adopt_backup_root(destination: &Path) -> PathBuf {
    destination
        .join(".agent/.cache/adopt/backups")
        .join(Ulid::new().to_string())
}

fn validate_adopt_output_ancestors(destination: &Path, backup_root: Option<&Path>) -> Result<()> {
    validate_adopt_receipt_paths(destination)?;
    if let Some(backup_root) = backup_root {
        let backup_relative = backup_root.strip_prefix(destination).with_context(|| {
            format!(
                "Backup destination {} must be contained by repository root {}",
                backup_root.display(),
                destination.display()
            )
        })?;
        validate_repository_relative_ancestors(destination, &backup_relative.join("preflight"))?;
    }
    Ok(())
}

fn validate_adopt_receipt_paths(destination: &Path) -> Result<()> {
    for relative in ADOPT_RECEIPT_PATHS.map(Path::new) {
        validate_repository_relative_ancestors(destination, relative)?;
        let receipt_path = destination.join(relative);
        match fs::symlink_metadata(&receipt_path) {
            Ok(metadata) if metadata.file_type().is_file() => {}
            Ok(_) => {
                bail!(
                    "Adopt receipt path must be missing or a regular file, not a symlink, directory, or other file type: {}",
                    receipt_path.display()
                );
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(error)
                    .with_context(|| format!("Failed to stat {}", receipt_path.display()));
            }
        }
    }
    Ok(())
}

fn confirm_adopt_write(opts: &AdoptOpts) -> Result<()> {
    if opts.defaults || opts.no_input {
        return Ok(());
    }
    let stdin = io::stdin();
    let mut stderr = io::stderr();
    if !stdin.is_terminal() || !stderr.is_terminal() {
        bail!(
            "Adopt write needs confirmation but stdin or stderr is not a terminal. Re-run interactively, or pass --defaults or --no-input for noninteractive execution."
        );
    }

    write!(stderr, "Proceed with adopt --write? [y/N] ")
        .context("Failed to write adopt confirmation prompt")?;
    stderr
        .flush()
        .context("Failed to flush adopt confirmation prompt")?;
    let mut answer = String::new();
    stdin
        .read_line(&mut answer)
        .context("Failed to read adopt confirmation")?;
    if matches!(answer.trim(), "y" | "Y" | "yes" | "YES" | "Yes") {
        return Ok(());
    }
    bail!("Adopt write cancelled; re-run with --defaults or --no-input to skip confirmation.");
}

fn write_adopt_last_receipt(
    destination: &Path,
    backup_root: Option<&Path>,
    result: &initial_copy::BootstrapCopyResult,
) -> Result<()> {
    validate_adopt_output_ancestors(destination, backup_root)?;
    let receipt = json!({
        "command": "adopt",
        "created_at_unix": OffsetDateTime::now_utc().unix_timestamp(),
        "destination": destination.display().to_string(),
        "backup_root": backup_root.map(|path| path.display().to_string()),
        "canonical_receipt_path": ADOPT_RECEIPT_PATH,
        "legacy_receipt_path": LEGACY_ADOPT_RECEIPT_PATH,
        "legacy_receipt_deprecated": true,
        "apply_report": &result.apply_report,
        "undo_hint": "Use apply_report.backups to restore modified or removed files, then delete paths listed in apply_report.files_created if you want to undo this adopt write. Delete backup_root when those backups are no longer needed.",
    });
    let text =
        serde_json::to_string_pretty(&receipt).context("Failed to serialize adopt receipt")?;
    let bytes = format!("{text}\n");
    write_adopt_receipt_atomic(destination, Path::new(ADOPT_RECEIPT_PATH), bytes.as_bytes())?;
    // TODO(jig-0.4): remove the legacy receipt copy after adopted repos have
    // had a release window to migrate readers to the canonical cache path.
    write_adopt_receipt_atomic(
        destination,
        Path::new(LEGACY_ADOPT_RECEIPT_PATH),
        bytes.as_bytes(),
    )?;
    Ok(())
}

fn write_adopt_receipt_atomic(destination: &Path, relative: &Path, bytes: &[u8]) -> Result<()> {
    validate_adopt_receipt_paths(destination)?;
    let receipt_path = destination.join(relative);
    let parent = receipt_path.parent().with_context(|| {
        format!(
            "Adopt receipt path has no parent: {}",
            receipt_path.display()
        )
    })?;
    fs::create_dir_all(parent).with_context(|| format!("Failed to create {}", parent.display()))?;
    validate_adopt_receipt_paths(destination)?;

    let existing_permissions = match fs::symlink_metadata(&receipt_path) {
        Ok(metadata) => Some(metadata.permissions()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => None,
        Err(error) => {
            return Err(error)
                .with_context(|| format!("Failed to stat {}", receipt_path.display()));
        }
    };
    #[cfg(unix)]
    let temp_builder = {
        use std::os::unix::fs::PermissionsExt;

        let mut builder = TempFileBuilder::new();
        if existing_permissions.is_none() {
            builder.permissions(fs::Permissions::from_mode(0o666));
        }
        builder
    };
    #[cfg(not(unix))]
    let temp_builder = TempFileBuilder::new();
    let mut temp = temp_builder.tempfile_in(parent).with_context(|| {
        format!(
            "Failed to create temporary adopt receipt in {}",
            parent.display()
        )
    })?;
    if let Some(permissions) = existing_permissions {
        temp.as_file()
            .set_permissions(permissions)
            .with_context(|| {
                format!(
                    "Failed to preserve permissions for {}",
                    receipt_path.display()
                )
            })?;
    }
    temp.write_all(bytes).with_context(|| {
        format!(
            "Failed to write temporary adopt receipt for {}",
            receipt_path.display()
        )
    })?;
    temp.as_file().sync_all().with_context(|| {
        format!(
            "Failed to sync temporary adopt receipt for {}",
            receipt_path.display()
        )
    })?;

    validate_adopt_receipt_paths(destination)?;
    temp.persist(&receipt_path)
        .map(|_| ())
        .map_err(|error| error.error)
        .with_context(|| format!("Failed to write {}", receipt_path.display()))
}

fn initial_render_report(result: &initial_copy::BootstrapCopyResult) -> Value {
    json!({
        "dry_run": result.apply_report.dry_run,
        "active_managed_paths": &result.apply_report.active_managed_paths,
        "retired_managed_paths": &result.apply_report.retired_managed_paths,
        "files_created": &result.apply_report.files_created,
        "files_modified": &result.apply_report.files_modified,
        "files_removed": &result.apply_report.files_removed,
        "files_unchanged": &result.apply_report.files_unchanged,
        "managed_blocks_inserted": &result.apply_report.managed_blocks_inserted,
        "managed_blocks_rendered": &result.apply_report.managed_blocks_rendered,
        "backups": &result.apply_report.backups,
        "conflicts": &result.apply_report.conflicts,
        "commands_detected_or_skipped": initial_command_report(result),
        "todos": initial_todos(result),
        "suggested_jig_toml_edits": initial_suggested_jig_toml_edits(result),
    })
}

fn initial_command_report(result: &initial_copy::BootstrapCopyResult) -> Vec<String> {
    let launcher = gate_preview::jig_launcher(result.minimal_footprint);
    let mut commands = Vec::new();
    if result.bootstrap_command_configured {
        commands.push(format!(
            "bootstrap_command configured; run {launcher} bootstrap before checks"
        ));
    } else {
        commands.push(format!(
            "bootstrap_command not configured; skip {launcher} bootstrap"
        ));
    }
    commands.push(format!(
        "contract check available through {launcher} check contract"
    ));
    if result.dev_apps_configured {
        commands.push(format!("[[dev.apps]] configured; run {launcher} dev"));
    } else {
        commands.push(format!(
            "no [[dev.apps]] configured; {launcher} dev has no app to launch"
        ));
    }
    if result.frontend_apps_configured && !result.minimal_footprint {
        commands.push(format!(
            "frontend app checks available through {launcher} check typescript-*"
        ));
    }
    commands
}

fn initial_todos(result: &initial_copy::BootstrapCopyResult) -> Vec<String> {
    let mut todos = vec![
        "Review generated command strings in .jig.toml against this repo's actual setup.".into(),
        "Add or update crate-level AGENTS.md files for repo-owned business rules.".into(),
    ];
    if result.sqlx_enabled {
        todos.push("Confirm SQLx database access and committed metadata workflow.".into());
    }
    if result.schema_dump_enabled {
        todos.push("Provide the project-owned scripts/dump-schema.sh implementation.".into());
    }
    if result.frontend_apps_configured && !result.minimal_footprint {
        todos.push(
            "Confirm each frontend app has package scripts and starts on the injected PORT/HOST."
                .into(),
        );
    }
    todos
}

fn initial_suggested_jig_toml_edits(result: &initial_copy::BootstrapCopyResult) -> Vec<String> {
    let mut edits = vec![
        "Replace generated fallback Cargo commands if this repo uses nested workspaces or non-Cargo checks.".into(),
    ];
    if result.dev_apps_configured {
        edits.push("Tune [dev] ports, tld, HTTPS, LAN, and each [[dev.apps]] kind/argv if defaults do not match local development.".into());
    }
    if result.sqlx_enabled {
        edits.push("Set rust_migration_dir, rust_sqlx_metadata_dir, and sqlx_check_command to the repo-owned SQLx layout.".into());
    }
    edits
}

#[cfg(test)]
fn read_optional_answer_string(answers_path: &Path, key: &str) -> Result<Option<String>> {
    let answers = read_answers_toml(answers_path)?;
    Ok(answers
        .get(key)
        .and_then(TomlValue::as_str)
        .map(str::to_string)
        .filter(|value| !value.is_empty()))
}

fn read_answers_toml(path: &Path) -> Result<Table> {
    let text =
        fs::read_to_string(path).with_context(|| format!("Failed to read {}", path.display()))?;
    toml::from_str(&text).with_context(|| format!("Failed to parse {}", path.display()))
}

#[cfg(test)]
fn write_answers_toml(path: &Path, mapping: &Table) -> Result<()> {
    let toml = toml::to_string(mapping)
        .with_context(|| format!("Failed to serialize {}", path.display()))?;
    fs::write(path, toml).with_context(|| format!("Failed to write {}", path.display()))
}

fn parse_frontend_app(value: &str) -> Result<FrontendApp, String> {
    let parts = value.split(':').collect::<Vec<_>>();
    if !(3..=5).contains(&parts.len()) {
        return Err("expected <name>:<dir>:<coverage_threshold>[:kind[:role]]".into());
    }

    let coverage_threshold = parts[2]
        .parse::<u32>()
        .map_err(|error| format!("coverage_threshold must be a non-negative integer: {error}"))?;

    let metadata =
        resolve_frontend_metadata(parts[0], parts.get(3).copied(), parts.get(4).copied(), None);
    let app = FrontendApp {
        name: parts[0].to_string(),
        dir: parts[1].to_string(),
        coverage_threshold,
        kind: metadata.kind.to_string(),
        role: metadata.role.to_string(),
    };
    answers::validate_frontend_apps(std::slice::from_ref(&app))
        .map_err(|error| error.to_string())?;
    Ok(app)
}

pub(crate) fn parse_scaffold_frontend(value: &str) -> Result<ScaffoldFrontend, String> {
    let (raw_name, explicit_kind) = value
        .split_once(':')
        .map_or((value, None), |(name, kind)| (name, Some(kind)));
    let name = match raw_name {
        "admin" => "admin-panel",
        other => other,
    };
    // Generated JS and HTML interpolate frontend titles directly, so these
    // rules must stay narrow unless the scaffold templates add escaping.
    if name.is_empty()
        || !name
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_')
    {
        return Err("frontend name must use ASCII letters, numbers, '-' or '_'".into());
    }
    if !name.chars().any(|ch| ch.is_ascii_alphanumeric()) {
        return Err("frontend name must include at least one ASCII letter or number".into());
    }
    let kind = match explicit_kind {
        Some(kind) => parse_scaffold_frontend_kind(kind)?,
        None => match raw_name {
            "admin" | "admin-panel" => ScaffoldFrontendKind::Admin,
            "landing" | "marketing" | "astro" => ScaffoldFrontendKind::Astro,
            _ => ScaffoldFrontendKind::Spa,
        },
    };
    Ok(ScaffoldFrontend {
        name: name.to_string(),
        kind,
        custom_default_name: explicit_kind.is_none()
            && !matches!(
                raw_name,
                "web" | "admin" | "admin-panel" | "landing" | "marketing" | "astro"
            ),
    })
}

impl ScaffoldFrontend {
    pub(crate) fn custom_default_name_notice(&self) -> Option<String> {
        self.custom_default_name.then(|| {
            format!(
                "'{}' isn't a preset shorthand — scaffolding a {} in {}/.",
                self.name,
                self.kind.custom_scaffold_label(),
                self.name
            )
        })
    }
}

impl ScaffoldFrontendKind {
    const fn custom_scaffold_label(self) -> &'static str {
        match self {
            Self::Spa => "custom Vite SPA",
            Self::Admin => "custom Vite admin app",
            Self::Astro => "custom Astro site",
        }
    }
}

impl ScaffoldOpts {
    pub(crate) fn normalize_minimal_harness_shape(&mut self, answers: &AnswerOpts) {
        if answers.harness_footprint == Some(HarnessFootprint::Minimal) && self.preset.is_none() {
            self.preset = Some(ScaffoldPreset::HarnessOnly);
        }
    }

    pub(crate) fn has_frontends(&self) -> bool {
        !self.frontends.is_empty() || !self.frontend_list.is_empty()
    }

    pub(crate) fn custom_frontend_notices(&self) -> Vec<String> {
        self.frontends
            .iter()
            .chain(self.frontend_list.iter())
            .filter_map(ScaffoldFrontend::custom_default_name_notice)
            .collect()
    }

    pub(crate) fn validate_init_invariants(&self, answers: &AnswerOpts) -> Result<()> {
        if let Some(preset) = self.preset
            && let Some(expected) = preset.generated_backend_language()
            && let Some(actual) = answers.backend_language
            && actual != expected
        {
            bail!(
                "--preset {} generates a {} backend but the effective answers select backend_language = \"{}\"; remove the conflicting answer or select a matching preset",
                preset.as_str(),
                expected.as_str(),
                actual.as_str()
            );
        }
        let has_project_scaffold = self
            .preset
            .is_some_and(ScaffoldPreset::has_project_scaffold);
        if answers.harness_footprint == Some(HarnessFootprint::Minimal)
            && (has_project_scaffold
                || self.db.is_some()
                || self.has_frontends()
                || answers.go_module.is_some())
        {
            let scaffold = self
                .preset
                .and_then(ScaffoldPreset::project_scaffold_label)
                .unwrap_or("Rust React");
            bail!(
                "Init cannot combine harness_footprint = \"minimal\" with a {scaffold} scaffold; remove the preset and its backend/frontend options, or use harness_footprint = \"full\""
            );
        }
        if self.preset == Some(ScaffoldPreset::HarnessOnly)
            && (self.db.is_some() || self.has_frontends() || answers.go_module.is_some())
        {
            bail!(
                "--preset harness-only cannot be combined with --db, --go-module, --frontend, or --frontends; remove the scaffold flags or use an application preset"
            );
        }
        if !self
            .preset
            .is_some_and(ScaffoldPreset::supports_go_module)
            && answers.go_module.is_some()
        {
            bail!("--go-module requires --preset go-react");
        }
        if self.preset == Some(ScaffoldPreset::GoReact) {
            if let Some(go_module) = answers.go_module.as_deref() {
                scaffold::validate_go_module(go_module)?;
            }
            let go_component_root = scaffold::go_component_root(answers)?;
            scaffold::validate_go_component_root(go_component_root)?;
            let initial_migration_dir = scaffold::go_component_path(
                go_component_root,
                crate::backend::GO_POSTGRES_MIGRATION_DIR,
            );
            if self.db == Some(ScaffoldDb::None) && answers.migration_dir.is_some() {
                bail!(
                    "migration_dir requires --preset go-react --db postgres; remove the answer or select PostgreSQL"
                );
            }
            if self.db == Some(ScaffoldDb::Postgres)
                && let Some(migration_dir) = answers.migration_dir.as_deref()
                && migration_dir != initial_migration_dir
            {
                bail!(
                    "--preset go-react owns its initial migration layout at {}; remove migration_dir from the answers file and customize the project-owned scaffold after init",
                    initial_migration_dir
                );
            }
            if self.db == Some(ScaffoldDb::Sqlite) {
                bail!(
                    "--preset go-react does not support --db sqlite; use --db none or --db postgres"
                );
            }
            if self
                .frontends
                .iter()
                .chain(self.frontend_list.iter())
                .any(|frontend| frontend.kind == ScaffoldFrontendKind::Admin)
                || answers
                    .frontend_apps
                    .iter()
                    .any(|frontend| frontend.role == "admin")
            {
                bail!(
                    "--preset go-react does not yet support the admin frontend because it requires a separate privileged API and client boundary; use web and/or landing"
                );
            }
        }
        if let Some(preset) = self.preset {
            for frontend_name in self
                .frontends
                .iter()
                .chain(self.frontend_list.iter())
                .map(|frontend| frontend.name.as_str())
                .chain(
                    answers
                        .frontend_apps
                        .iter()
                        .map(|frontend| frontend.name.as_str()),
                )
            {
                for backend_name in preset.reserved_backend_dev_app_names() {
                    let backend_prefix = jig_core::dev_app_env_prefix(backend_name);
                    if jig_core::dev_app_env_prefix(frontend_name) == backend_prefix {
                        bail!(
                            "{} frontend app name '{frontend_name}' conflicts with the reserved backend dev app '{backend_name}' because both derive dev environment prefix {backend_prefix}; choose another frontend name",
                            preset.as_str()
                        );
                    }
                }
            }
        }
        Ok(())
    }

    pub(crate) fn apply_init_answer_defaults(&self, answers: &mut AnswerOpts) {
        if self
            .preset
            .is_some_and(|preset| !preset.supports_database())
            && should_default_init_sqlx_disabled(answers)
        {
            answers.sqlx_enabled = Some(false);
        }
        if let Some(backend_language) = self
            .preset
            .and_then(ScaffoldPreset::generated_backend_language)
        {
            answers.backend_language = Some(backend_language);
        }
        if self.preset == Some(ScaffoldPreset::GoReact) {
            answers.sqlx_enabled = Some(false);
        }
    }
}

fn parse_scaffold_frontend_kind(value: &str) -> Result<ScaffoldFrontendKind, String> {
    Ok(match value {
        "web" | "spa" => ScaffoldFrontendKind::Spa,
        "admin" | "admin-panel" => ScaffoldFrontendKind::Admin,
        "landing" | "marketing" | "astro" => ScaffoldFrontendKind::Astro,
        other => {
            return Err(format!(
                "unsupported frontend kind '{other}'. Expected spa, admin, or astro"
            ));
        }
    })
}
fn default_dev_app_kind() -> String {
    "env-port".into()
}

const fn default_true() -> bool {
    true
}

fn validate_init_destination(path: &Path, force: bool) -> Result<()> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => {
            return Err(error)
                .with_context(|| format!("Failed to inspect init destination {}", path.display()));
        }
    };
    if !metadata.file_type().is_dir() {
        bail!(
            "Init destination is not a real directory: {}",
            path.display()
        );
    }

    let first_entry = fs::read_dir(path)?
        .next()
        .transpose()
        .with_context(|| format!("Failed to enumerate {}", path.display()))?;
    if first_entry.is_none() || force {
        return Ok(());
    }

    bail!(
        "Init destination is not empty: {}. Re-run with --force to overwrite.",
        path.display()
    );
}

fn validate_adopt_destination(path: &Path) -> Result<()> {
    if !path.exists() {
        bail!("Adopt destination does not exist: {}", path.display());
    }
    if !path.is_dir() {
        bail!("Adopt destination is not a directory: {}", path.display());
    }
    Ok(())
}

fn validate_update_destination(path: &Path) -> Result<()> {
    validate_adopt_destination(path)?;
    let answers_path = path.join(ANSWERS_FILE);
    if !answers_path.exists() {
        bail!(
            "Update destination does not contain {}: {}",
            ANSWERS_FILE,
            path.display()
        );
    }
    Ok(())
}

fn reject_newer_declared_contract(path: &Path) -> Result<()> {
    let Ok(contract_version) = RepoContext::declared_contract_version_from_root(path) else {
        // Missing or damaged manifests remain repairable through adopt/update.
        return Ok(());
    };
    if contract_version > crate::context::CURRENT_CONTRACT_VERSION {
        bail!(
            "Refusing to rewrite repository contract {contract_version} with this older Jig runtime, which supports contracts through {}. Install a newer compatible Jig runtime and retry; --force does not permit contract downgrades.",
            crate::context::CURRENT_CONTRACT_VERSION
        );
    }
    Ok(())
}
