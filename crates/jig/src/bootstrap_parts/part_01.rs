fn generated_package_manager_spec(package_manager: &str) -> &'static str {
    match package_manager {
        "bun" => "bun@1.3.14",
        "npm" => "npm@12.0.2",
        "pnpm" => "pnpm@11.22.0",
        "yarn" => "yarn@4.18.0",
        _ => unreachable!("web package manager was already validated"),
    }
}

fn generated_package_manager_version(package_manager: &str) -> &'static str {
    generated_package_manager_spec(package_manager)
        .split_once('@')
        .expect("generated package manager specs contain @")
        .1
}

#[derive(Args, Clone, Debug)]
#[command(after_help = "\
For existing repositories, use:
  jig adopt .

Templates:
  Omit --template for the default jig-sh harness template.
  Release builds pin that template to this jig version's release tag.
  Unreleased local builds use templates embedded in the jig binary unless --vcs-ref is supplied.

Scaffold ownership:
  Presets create starter project code once. After creation, that project code is project-owned.
  `jig update` keeps the Jig harness current; it does not rewrite scaffolded app code.

Interaction modes:
  Interactive terminals prompt only for unresolved project-shape choices.
  --defaults uses rust-react, database none, and frontend web when those choices are omitted.
  --no-input and non-terminal execution require the project shape to be fully specified.

Examples:
  jig init /path/to/new-repo
  jig init /path/to/new-repo --preset harness-only --repo-name new-repo --sqlx-enabled false --no-input --no-vault
  jig init /path/to/new-repo --preset harness-only --no-input --no-vault
  jig init /path/to/new-repo --preset rust-library --no-input --no-vault
  jig init /path/to/new-repo --preset rust-cli --no-input --no-vault
  jig init /path/to/new-repo --preset rust-react
  jig init /path/to/new-repo --preset rust-react --db postgres --frontends web,landing,admin
  jig init /path/to/new-repo --preset go-react --db postgres --frontends web --go-module github.com/acme/new-repo
  jig presets
  jig init /path/to/new-repo --preset harness-only --template /path/to/jig-sh --template-mode committed --repo-name new-repo --sqlx-enabled false --no-input --no-vault")]
pub struct InitOpts {
    #[arg(help = "Destination directory for the new repository")]
    pub path: PathBuf,
    #[command(flatten)]
    pub scaffold: ScaffoldOpts,
    #[arg(
        long,
        help_heading = "Advanced Template Source",
        value_name = "PATH_OR_GIT_URL",
        help = "Template source to render; defaults to the official jig-sh template",
        long_help = "Template source to render. Release builds default to the official jig-sh template at https://github.com/bpcakes/jig-sh.git pinned to the release tag for this jig version; passing that canonical HTTPS URL explicitly, with or without .git, has the same pinned behavior unless --vcs-ref is also provided. Unreleased or dirty local builds use templates embedded in the jig binary for omitted --template, avoiding a stale release-tag lookup during local development. For checkout-driven template development, pass the path to your jig-sh checkout, for example /Users/you/src/jig-sh. For remote forks, SSH URLs, or private harnesses, pass a git URL. The source must contain templates/project."
    )]
    pub template: Option<String>,
    #[arg(
        long,
        value_enum,
        help_heading = "Advanced Template Source",
        help = "How to read a local git template checkout",
        long_help = "How to read a local git template checkout. The default for local git paths is committed, which renders from clean HEAD and refuses dirty template changes."
    )]
    pub template_mode: Option<TemplateMode>,
    #[arg(
        long,
        help_heading = "Advanced Template Source",
        help = "Git revision to render from the template source"
    )]
    pub vcs_ref: Option<String>,
    #[arg(
        long,
        help_heading = "Safety",
        help = "Allow init to write into a non-empty destination and overwrite existing scaffold files",
        long_help = "Allow init to write into a non-empty destination and overwrite existing scaffold files. Template-to-scaffold path collisions are still rejected because they indicate a preset/template ownership bug."
    )]
    pub force: bool,
    #[arg(
        long,
        help_heading = "Automation",
        help = "Skip the init wizard; omitted project shape defaults to rust-react, database none, and frontend web",
        long_help = "Skip the init wizard and resolve omitted project-shape choices to --preset rust-react, --db none, and --frontend web. Explicit scaffold flags are preserved, and effective frontend_apps from --answers-file prevent the default web scaffold from being added."
    )]
    pub defaults: bool,
    #[arg(
        long,
        help_heading = "Automation",
        help = "Skip the init wizard and require an explicit, complete project shape instead of prompting",
        long_help = "Skip the init wizard and require --preset. The rust-react and go-react application presets require an explicit --db choice plus --frontend/--frontends or effective frontend_apps from --answers-file; go-react also requires --go-module. The harness-only, rust-library, and rust-cli presets need no database or frontend choice and reject those scaffold flags. Non-terminal execution without --defaults follows this strict behavior."
    )]
    pub no_input: bool,
    #[arg(
        long,
        help_heading = "Vault",
        help = "Skip initial passphrase setup; generated repo metadata still declares a vault scope"
    )]
    pub no_vault: bool,
    #[command(flatten)]
    pub answers: AnswerOpts,
}

#[derive(Args, Clone, Debug)]
#[command(after_help = "\
Templates:
  Release builds default to the official jig-sh harness template:
  https://github.com/bpcakes/jig-sh.git

  Release builds pin omitted --template to this jig version's release tag.
  Unreleased or dirty local builds use templates embedded in the jig binary unless --vcs-ref is supplied.

Adoption scans the existing repository before resolving answers. If SQLx is detected,
omitted SQLx answers resolve to migration defaults; if it is not detected, omitted SQLx
answers resolve to a tooling-only profile. Pass --sqlx-enabled true and --rust-migration-dir
<dir> to override.

Examples:
  jig adopt .
  jig adopt . --write
  jig adopt . --minimal --write
  jig adopt . --write --template /path/to/jig-sh --template-mode committed")]
pub struct AdoptOpts {
    #[arg(default_value = ".", help = "Existing repository directory to adopt")]
    pub path: PathBuf,
    #[arg(
        long,
        value_name = "PATH_OR_GIT_URL",
        help = "Template source to render; defaults to the official jig-sh template",
        long_help = "Template source to render. Release builds default to the official jig-sh template at https://github.com/bpcakes/jig-sh.git pinned to the release tag for this jig version; passing that canonical HTTPS URL explicitly, with or without .git, has the same pinned behavior unless --vcs-ref is also provided. Unreleased or dirty local builds use templates embedded in the jig binary for omitted --template, avoiding a stale release-tag lookup during local development. For checkout-driven template development, pass the path to your jig-sh checkout, for example /Users/you/src/jig-sh. For remote forks, SSH URLs, or private harnesses, pass a git URL. The source must contain templates/project."
    )]
    pub template: Option<String>,
    #[arg(
        long,
        value_enum,
        help = "How to read a local git template checkout",
        long_help = "How to read a local git template checkout. The default for local git paths is committed, which renders from clean HEAD and refuses dirty template changes."
    )]
    pub template_mode: Option<TemplateMode>,
    #[arg(long, help = "Git revision to render from the template source")]
    pub vcs_ref: Option<String>,
    #[arg(long, help = "Overwrite conflicting template-managed paths")]
    pub force: bool,
    #[arg(long, help = "Write rendered managed files; omit to preview only")]
    pub write: bool,
    #[arg(
        long,
        help = "Render only .jig.toml and .agent/ scaffolding (no scripts, workflows, or agent context files)",
        long_help = "Render a loop-ready minimal footprint: .jig.toml, .agent/jig-contract.json, and .agent/ scaffolding, plus block-managed .gitignore/.gitattributes. Omits scripts/, .github/workflows/, AGENTS.md, agent-map.md, and .mcp.json. Stores harness_footprint = \"minimal\" so jig update keeps the same footprint until you re-adopt without --minimal."
    )]
    pub minimal: bool,
    #[arg(
        long,
        help = "Use default answers for omitted configuration prompts and adopt write confirmation; vault setup captures credentials before rendering"
    )]
    pub defaults: bool,
    #[arg(
        long,
        help = "Fail instead of prompting for missing answers and skip adopt write confirmation; vault setup requires JIG_VAULT_PASSPHRASE or --no-vault"
    )]
    pub no_input: bool,
    #[arg(
        long,
        help = "Skip initial passphrase setup when --write is supplied; generated repo metadata still declares a vault scope"
    )]
    pub no_vault: bool,
    #[command(flatten)]
    pub answers: AnswerOpts,
}

#[derive(Args, Clone, Debug)]
#[command(after_help = "\
Update modes:
  jig update advances to the resolved template source.
  jig update --recopy re-renders from the stored .jig.toml commit.
  jig update --launcher-only repairs only scripts/jig and scripts/install-jig.sh.
  Add --force only when changed template-managed files should be replaced.

Examples:
  jig update
  jig update --recopy
  jig update /path/to/repo --launcher-only --force
  jig update --template /path/to/jig-sh --template-mode committed --force")]
pub struct UpdateOpts {
    #[arg(default_value = ".", help = "Adopted repository directory to update")]
    pub path: PathBuf,
    #[arg(long, help = "Template source to render from for this update")]
    pub template: Option<String>,
    #[arg(long, value_enum, help = "How to read a local git template checkout")]
    pub template_mode: Option<TemplateMode>,
    #[arg(
        long,
        help = "Re-render from the stored .jig.toml commit instead of advancing"
    )]
    pub recopy: bool,
    #[arg(
        long,
        requires = "force",
        conflicts_with_all = [
            "template",
            "template_mode",
            "recopy",
            "vcs_ref",
            "defaults",
            "no_input"
        ],
        help = "Repair only the managed launcher and installer from this binary's embedded templates"
    )]
    pub launcher_only: bool,
    #[arg(long, help = "Overwrite changed template-managed files")]
    pub force: bool,
    #[arg(long, help = "Git revision to render from the template source")]
    pub vcs_ref: Option<String>,
    #[arg(long, help = "Use default answers for omitted configuration prompts")]
    pub defaults: bool,
    #[arg(long, help = "Fail instead of prompting for missing answers")]
    pub no_input: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct FrontendApp {
    pub name: String,
    pub dir: String,
    pub coverage_threshold: u32,
    pub kind: String,
    pub role: String,
}

impl<'de> Deserialize<'de> for FrontendApp {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct FrontendAppFields {
            name: String,
            dir: String,
            coverage_threshold: u32,
            #[serde(default)]
            kind: Option<String>,
            #[serde(default)]
            role: Option<String>,
        }

        let fields = FrontendAppFields::deserialize(deserializer)?;
        let metadata = resolve_frontend_metadata(
            &fields.name,
            fields.kind.as_deref(),
            fields.role.as_deref(),
            None,
        );
        Ok(Self {
            name: fields.name,
            dir: fields.dir,
            coverage_threshold: fields.coverage_threshold,
            kind: metadata.kind.into(),
            role: metadata.role.into(),
        })
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DevApp {
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

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, ValueEnum)]
#[serde(rename_all = "kebab-case")]
pub enum TemplateMode {
    Committed,
}

#[derive(Args, Clone, Debug, Default)]
pub struct ScaffoldOpts {
    #[arg(
        long,
        value_enum,
        help_heading = "Project Shape",
        help = "Project scaffold to generate alongside the Jig harness; run `jig presets` to inspect available presets"
    )]
    pub preset: Option<ScaffoldPreset>,
    #[arg(
        long,
        value_enum,
        help_heading = "Project Shape",
        help = "Database scaffold for presets that support a backend"
    )]
    pub db: Option<ScaffoldDb>,
    #[arg(
        long = "frontend",
        help_heading = "Project Shape",
        value_parser = parse_scaffold_frontend,
        help = "Frontend scaffold as name[:kind], e.g. web:spa, landing:astro, admin-panel:admin; may be repeated. Bare web, landing, and admin use preset shorthands. Rust-react reserves api and admin-api for backend dev apps."
    )]
    pub frontends: Vec<ScaffoldFrontend>,
    #[arg(
        long = "frontends",
        help_heading = "Project Shape",
        value_delimiter = ',',
        value_parser = parse_scaffold_frontend,
        help = "Comma-separated frontend scaffolds, e.g. web,landing,admin. Bare web, landing, and admin use preset shorthands. Rust-react reserves api and admin-api for backend dev apps."
    )]
    pub frontend_list: Vec<ScaffoldFrontend>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub enum ScaffoldPreset {
    RustReact,
    GoReact,
    HarnessOnly,
    RustLibrary,
    RustCli,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub enum ScaffoldDb {
    None,
    Postgres,
    Sqlite,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ScaffoldFrontend {
    name: String,
    kind: ScaffoldFrontendKind,
    custom_default_name: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ScaffoldFrontendKind {
    Spa,
    Admin,
    Astro,
}

impl TemplateMode {
    pub(super) const fn as_str(self) -> &'static str {
        match self {
            Self::Committed => "committed",
        }
    }
}

pub(crate) fn prepare_init_answers_for_interaction(
    answers: &AnswerOpts,
) -> Result<PreparedInitAnswers> {
    let invocation_cwd = bootstrap_invocation_cwd()?;
    PreparedInitAnswers::from_opts_at(answers, &invocation_cwd)
}

pub(crate) fn should_default_init_sqlx_disabled(answers: &AnswerOpts) -> bool {
    answers::should_default_init_sqlx_disabled(answers)
}

#[derive(Debug, Deserialize, Serialize)]
pub(crate) struct InitReport {
    ok: bool,
    command: String,
    render_mode: String,
    template: String,
    destination: String,
    answers_file: String,
    git_initialized: bool,
    scaffold: Option<Value>,
    render_report: Value,
    next_steps: Vec<String>,
    notes: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    vault: Option<BootstrapVaultReport>,
    // Keep legacy JSON-style bootstrap assertions working without carrying a
    // second representation in production reports.
    #[cfg(test)]
    #[serde(skip)]
    serialized: std::sync::OnceLock<Value>,
}

impl InitReport {
    pub(crate) fn destination(&self) -> &str {
        &self.destination
    }

    pub(crate) fn template(&self) -> &str {
        &self.template
    }

    pub(crate) const fn git_initialized(&self) -> bool {
        self.git_initialized
    }

    pub(crate) fn scaffold(&self) -> Option<&Value> {
        self.scaffold.as_ref()
    }

    pub(crate) const fn render_report(&self) -> &Value {
        &self.render_report
    }

    pub(crate) fn next_steps(&self) -> &[String] {
        &self.next_steps
    }

    pub(crate) fn notes(&self) -> &[String] {
        &self.notes
    }

    pub(crate) fn vault(&self) -> Option<&BootstrapVaultReport> {
        self.vault.as_ref()
    }

    pub(crate) fn attach_vault(&mut self, vault: BootstrapVaultReport) -> Result<()> {
        if self.vault.is_some() {
            bail!("bootstrap::run_init output unexpectedly included a vault field");
        }
        self.vault = Some(vault);
        #[cfg(test)]
        {
            self.serialized = std::sync::OnceLock::new();
        }
        Ok(())
    }
}

#[cfg(test)]
impl std::ops::Deref for InitReport {
    type Target = Value;

    fn deref(&self) -> &Self::Target {
        self.serialized.get_or_init(|| {
            serde_json::to_value(self).expect("typed init report should serialize for legacy tests")
        })
    }
}

#[derive(Debug, Deserialize, Serialize)]
pub(crate) struct BootstrapVaultReport {
    requested: bool,
    initialized: bool,
    created: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    skipped_reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    vault_home: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    vault_scope: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    vault_scope_id: Option<Value>,
}

impl BootstrapVaultReport {
    pub(crate) fn disabled() -> Self {
        Self::skipped(false, "disabled")
    }

    pub(crate) fn missing_scope() -> Self {
        Self::skipped(true, "repo has no [vault] scope")
    }

    fn skipped(requested: bool, reason: &str) -> Self {
        Self {
            requested,
            initialized: false,
            created: false,
            skipped_reason: Some(reason.to_string()),
            vault_home: None,
            vault_scope: None,
            vault_scope_id: None,
        }
    }

    pub(crate) fn initialized(created: bool, runtime_report: &Value) -> Self {
        Self {
            requested: true,
            initialized: true,
            created,
            skipped_reason: None,
            vault_home: Some(runtime_report["vault_home"].clone()),
            vault_scope: Some(runtime_report["vault_scope"].clone()),
            vault_scope_id: Some(runtime_report["vault_scope_id"].clone()),
        }
    }

    pub(crate) const fn requested(&self) -> bool {
        self.requested
    }

    pub(crate) const fn initialized_status(&self) -> bool {
        self.initialized
    }

    pub(crate) const fn created(&self) -> bool {
        self.created
    }

    pub(crate) fn skipped_reason(&self) -> Option<&str> {
        self.skipped_reason.as_deref()
    }

    pub(crate) fn vault_scope(&self) -> Option<&str> {
        self.vault_scope.as_ref().and_then(Value::as_str)
    }
}

pub(crate) fn preflight_init_destination(opts: &InitOpts) -> Result<()> {
    let invocation_cwd = bootstrap_invocation_cwd()?;
    let destination = path::resolve_init_destination(&opts.path, &invocation_cwd)?;
    validate_init_destination(&destination, opts.force)?;
    ensure_init_destination_noreplace_supported(&destination)
}

fn ensure_init_destination_noreplace_supported(destination: &Path) -> Result<()> {
    let (existing_ancestor, _) = path::split_existing_ancestor(destination)?;
    path::ensure_atomic_noreplace_publication_supported(&existing_ancestor)
}

pub fn run_adopt(opts: AdoptOpts) -> Result<Value> {
    let invocation_cwd = bootstrap_invocation_cwd()?;
    let destination = absolute_path_from(&opts.path, &invocation_cwd)?;
    let progress = CliProgress::new("adopt");
    progress.header_for_path("render harness into existing repo", &destination);
    progress.step("validate destination", "existing repository directory");
    progress.log_blocked_on_err(validate_adopt_destination(&destination))?;
    progress.log_blocked_on_err(reject_newer_declared_contract(&destination))?;
    let prior_managed_paths =
        progress.log_blocked_on_err(managed_paths::load_manifest(&destination))?;
    progress.step(
        "resolve template",
        template_progress_label(opts.template.as_deref()),
    );
    let template_request = progress.log_blocked_on_err(resolve_initial_template_request(
        opts.template.as_deref(),
        &opts.vcs_ref,
    ))?;
    let template = progress.log_blocked_on_err(prepare_initial_template_source(
        &template_request,
        opts.template_mode,
        &invocation_cwd,
    ))?;
    progress.step("infer answers", "scan existing repository");
    let inference = adopt_infer::infer_adopt_answers(&destination);
    let prior_answers = recognized_prior_answers(&destination);
    let requested_harness_footprint = if opts.minimal {
        HarnessFootprint::Minimal
    } else {
        HarnessFootprint::Full
    };
    let expands_minimal_harness = prior_answers.as_ref().is_some_and(|prior| {
        prior.harness_footprint() == HarnessFootprint::Minimal
            && requested_harness_footprint == HarnessFootprint::Full
    });
    let changes_harness_footprint = prior_answers
        .as_ref()
        .is_some_and(|prior| prior.harness_footprint() != requested_harness_footprint);
    let establishes_manifest = prior_managed_paths.is_none() && prior_answers.is_some();
    if prior_managed_paths.is_none()
        && prior_answers.as_ref().is_some_and(|prior| {
            prior.harness_footprint() == HarnessFootprint::Full
                && requested_harness_footprint == HarnessFootprint::Minimal
        })
    {
        bail!(
            "Cannot switch this adopted repository from the full harness to --minimal because {} is missing. First run `jig adopt . --write` without --minimal to establish exact managed-path ownership, then retry the minimal adoption.",
            managed_paths::MANIFEST_PATH
        );
    }
    let mut answers = opts.answers.clone();
    answers.harness_footprint = Some(requested_harness_footprint);
    let answer_input = progress.log_blocked_on_err(
        if (changes_harness_footprint || establishes_manifest) && answers.answers_file.is_none() {
            AnswerInput::from_file(&destination.join(ANSWERS_FILE))
        } else {
            AnswerInput::from_opts_at(&answers, &invocation_cwd)
        },
    )?;
    let answer_shape = answer_input.shape().clone();
    progress.info("detected", inference.summary());
    progress.info("detected stack", inference.detected_stack_label());
    if opts.minimal {
        progress.info(
            "footprint",
            "minimal (.jig.toml + .agent/ scaffolding; no scripts/workflows/context files)",
        );
    }
    for warning in inference.warnings() {
        progress.info("warning", warning);
    }
    inference.apply_to_answers(&mut answers, &answer_shape);
    let review = inference.adoption_review(&answers, &opts.answers, &answer_shape);
    for item in &review.items {
        progress.info("review", item);
    }
    let mut runtime_warnings = Vec::new();
    if opts.write {
        confirm_adopt_write(&opts)?;
    } else {
        progress.info(
            "mode",
            "preview only; re-run with --write to apply managed files",
        );
    }
    let backup_root = opts.write.then(|| adopt_backup_root(&destination));
    if opts.write {
        progress.log_blocked_on_err(validate_adopt_output_ancestors(
            &destination,
            backup_root.as_deref(),
        ))?;
    }

    let copy_result = render_and_copy_bootstrap_template(BootstrapCopyRequest {
        destination: &destination,
        template: &template,
        answers: &answers,
        answer_input: Some(answer_input),
        use_defaults: opts.defaults,
        force: opts.force,
        dry_run: !opts.write,
        backup_root: backup_root.clone(),
        seed_repo_path: Some(&destination),
        prior_harness_footprint: prior_answers.as_ref().map(RenderAnswers::harness_footprint),
        prior_managed_paths: prior_managed_paths.as_ref(),
        reconcile_runtime_config: prior_answers.is_some(),
        allow_answers_overwrite: expands_minimal_harness || establishes_manifest,
        allow_contract_overwrite: expands_minimal_harness,
        reserved_output_paths: Vec::new(),
        scaffolded_frontend_contracts: false,
        scaffolded_go_postgres_integration: false,
        init_transaction: None,
        use_update_transaction: opts.write,
        progress,
    })?;
    if opts.write {
        if let Err(error) =
            write_adopt_last_receipt(&destination, backup_root.as_deref(), &copy_result)
        {
            progress.info(
                "warning",
                format!("adopt write completed but undo receipt could not be recorded: {error:#}"),
            );
        }
        let footprint = if copy_result.minimal_footprint {
            HarnessFootprint::Minimal
        } else {
            HarnessFootprint::Full
        };
        let runtime_policy = FullRefreshRuntimePolicy::for_render(footprint, template.source());
        runtime_warnings =
            finish_full_refresh(&destination, runtime_policy, progress, "adopt complete");
    } else {
        progress.done("adopt preview complete");
    }

    Ok(json!({
        "ok": true,
        "command": "adopt",
        "render_mode": if opts.write { "copy" } else { "preview" },
        "harness_footprint": if copy_result.minimal_footprint {
            "minimal"
        } else {
            "full"
        },
        "template": template.source(),
        "destination": destination.display().to_string(),
        "answers_file": ANSWERS_FILE,
        "git_initialized": false,
        "write": opts.write,
        "warnings": runtime_warnings,
        "detection_report": inference.report(),
        "adoption_profile": inference.adoption_profile_report(
            &copy_result.render_preview.generated_gates,
            &copy_result.render_preview.managed_files,
            &copy_result.render_preview.retired_managed_files,
            &copy_result.render_preview.file_budget,
            &opts.answers,
            &answer_shape,
        ),
        "adoption_review": review.items,
        "render_report": initial_render_report(&copy_result),
        "next_steps": initial_next_steps(
            InitialCommand::Adopt,
            &destination,
            &copy_result,
            false,
        ),
        "notes": initial_notes(
            copy_result.notes,
            copy_result.frontend_apps_configured,
            None,
            copy_result.minimal_footprint,
        ),
    }))
}

fn recognized_prior_answers(destination: &Path) -> Option<RenderAnswers> {
    let answers = RenderAnswers::from_answers_file(&destination.join(ANSWERS_FILE)).ok()?;
    crate::context::RepoContext::validate_config_file(destination).ok()?;
    Some(answers)
}

fn template_progress_label(template: Option<&str>) -> String {
    template.unwrap_or("default jig-sh template").to_string()
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum InitialCommand {
    Init,
    Adopt,
}
