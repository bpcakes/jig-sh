use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use anyhow::{Result, bail};
use serde_json::Value;

use crate::backend::{GO_POSTGRES_MIGRATION_DIR, GoDatabase};
use crate::context::validate_web_package_manager;

use super::{
    AnswerOpts, DevApp, FrontendApp, RUST_REACT_ADMIN_BACKEND_DEV_APP_NAME, ScaffoldDb,
    ScaffoldFrontend, ScaffoldFrontendKind, ScaffoldOpts, ScaffoldPreset,
};
use frontend::{
    FrontendBackendContext, FrontendDatabaseContext, FrontendScaffold,
    frontend_workspace_relative_paths_for_backend, render_frontend_workspace_files_for_backend,
    scaffold_bootstrap_command,
};
use names::{
    default_repo_name, normalize_package_name, normalize_rust_react_package_name,
    rust_react_repo_dns_label, validate_scaffold_relative_path,
};
pub(super) use project::RustOnlyArtifact;
use project::{
    GoReactScaffoldPlan, GoScaffoldPlan, ReactBackendRenderContext, ReactScaffoldPlan,
    RustOnlyScaffoldPlan, RustReactScaffoldPlan, RustScaffoldPlan, ScaffoldIdentity,
    ScaffoldProjectPlan,
};
pub(super) use write::ScaffoldFile;
use write::ScaffoldReport;

mod embedded_templates;
mod frontend;
mod go_workspace;
mod names;
mod project;
mod rust_only_workspace;
mod rust_workspace;
mod templates;
mod write;

#[derive(Clone, Debug)]
pub(super) struct InitScaffoldPlan {
    project: ScaffoldProjectPlan,
    /// The repo name exactly as requested or inferred from the destination path.
    requested_repo_name: String,
    /// The normalized repo name recorded in generated Jig answers.
    repo_name: String,
    /// The kebab-case package stem used for generated workspace members.
    package_name: String,
    /// The underscore code/module stem derived from `package_name`.
    module_name: String,
    default_branch: String,
    ci_github_runner: String,
}

pub(crate) fn validate_go_module(value: &str) -> Result<()> {
    names::validate_go_module(value)
}

pub(crate) fn default_go_module(value: &str) -> String {
    names::default_go_module(value)
}

impl InitScaffoldPlan {
    pub(super) fn from_opts(
        opts: &ScaffoldOpts,
        answers: &AnswerOpts,
        destination: &Path,
    ) -> Result<Option<Self>> {
        opts.validate_init_invariants(answers)?;
        if opts.preset.is_none()
            && opts.db.is_none()
            && opts.frontends.is_empty()
            && opts.frontend_list.is_empty()
        {
            return Ok(None);
        }
        let Some(preset) = opts.preset else {
            bail!(
                "Scaffold options require --preset rust-react, --preset go-react, --preset harness-only, or --preset rust-library"
            );
        };
        match preset {
            ScaffoldPreset::RustReact => Self::rust_react(opts, answers, destination).map(Some),
            ScaffoldPreset::GoReact => Self::go_react(opts, answers, destination).map(Some),
            ScaffoldPreset::HarnessOnly => Ok(None),
            ScaffoldPreset::RustLibrary => {
                Self::rust_only(RustOnlyArtifact::Library, answers, destination).map(Some)
            }
        }
    }

    pub(super) fn apply_answer_defaults(&self, answers: &mut AnswerOpts) {
        if answers.repo_name.as_deref() != Some(self.repo_name.as_str()) {
            answers.repo_name = Some(self.repo_name.clone());
        }
        answers.backend_language = Some(self.project.backend_language());
        match &self.project {
            ScaffoldProjectPlan::RustReact(project) => {
                let backend = &project.backend;
                if answers.sqlx_enabled.is_none() {
                    answers.sqlx_enabled = Some(backend.database != ScaffoldDb::None);
                }
                if backend.database != ScaffoldDb::None {
                    if answers.rust_migration_dir.is_none() {
                        answers.rust_migration_dir = Some(backend.migration_dir.clone());
                    }
                    if answers.rust_sqlx_metadata_dir.is_none() {
                        answers.rust_sqlx_metadata_dir = Some(backend.sqlx_metadata_dir.clone());
                    }
                    if answers.schema_dump_enabled.is_none() {
                        answers.schema_dump_enabled = Some(false);
                    }
                }
                if answers.rust_crate_roots.is_empty() {
                    answers.rust_crate_roots = vec!["apps".into(), "crates".into()];
                }
            }
            ScaffoldProjectPlan::GoReact(project) => {
                let backend = &project.backend;
                answers.go_database = Some(backend.database);
                answers.sqlx_enabled = Some(false);
                answers.rust_crate_roots.clear();
                answers.rust_migration_dir = None;
                if answers.migration_dir.is_none() {
                    answers.migration_dir = backend
                        .database
                        .is_postgres()
                        .then(|| backend.migration_dir.clone());
                }
                answers.rust_sqlx_metadata_dir = None;
                answers.schema_dump_enabled = Some(false);
            }
            ScaffoldProjectPlan::RustOnly(_) => {
                answers.go_database = None;
                answers.sqlx_enabled = Some(false);
                answers.rust_crate_roots = vec!["crates".into()];
                answers.rust_migration_dir = None;
                answers.migration_dir = None;
                answers.rust_sqlx_metadata_dir = None;
                answers.schema_dump_enabled = Some(false);
                answers.application_contracts_enabled = Some(false);
                answers.repository_projection_hint =
                    super::repository_model::RepositoryProjectionHint::RustWorkspace;
            }
        }
        if answers.bootstrap_command.is_none() {
            answers.bootstrap_command = Some(match &self.project {
                ScaffoldProjectPlan::RustReact(project) => scaffold_bootstrap_command(
                    &self.package_name,
                    project.backend.database,
                    &project.react.frontends,
                ),
                ScaffoldProjectPlan::GoReact(project) => {
                    self.go_scaffold_bootstrap_command(&project.backend)
                }
                ScaffoldProjectPlan::RustOnly(_) => {
                    optional_cargo_command("cargo fetch", "bootstrap")
                }
            });
        }
        if let Some(react) = self.project.react() {
            if answers.web_package_manager.is_none() {
                answers.web_package_manager = Some(react.package_manager.clone());
            }
            if answers.application_contracts_enabled.is_none() {
                answers.application_contracts_enabled = Some(!react.frontends.is_empty());
            }
            if answers.frontend_apps.is_empty() {
                answers.frontend_apps = react
                    .frontends
                    .iter()
                    .map(|frontend| FrontendApp {
                        name: frontend.name.clone(),
                        dir: frontend.dir.clone(),
                        coverage_threshold: frontend.coverage_threshold,
                        kind: frontend.dev_kind.clone(),
                        role: frontend.kind.as_str().into(),
                    })
                    .collect();
            }
            if answers.dev_apps.is_empty()
                && let Some(backend_app) =
                    self.project.application_backend_dev_app(&self.package_name)
            {
                answers.dev_apps.push(backend_app);
                if matches!(&self.project, ScaffoldProjectPlan::RustReact(_))
                    && self.has_admin_frontend()
                {
                    answers.dev_apps.push(DevApp {
                        name: RUST_REACT_ADMIN_BACKEND_DEV_APP_NAME.into(),
                        dir: Some(".".into()),
                        kind: "env-port".into(),
                        command: None,
                        argv: vec![
                            "cargo".into(),
                            "run".into(),
                            "-p".into(),
                            format!("{}-admin-api", self.package_name),
                        ],
                        port: None,
                        host: None,
                        proxy: true,
                    });
                }
            }
        }
    }

    pub(super) fn summary(&self) -> String {
        let mut parts = vec![format!(
            "{} for {}",
            self.project.summary_label(),
            self.repo_name
        )];
        match self.database() {
            ScaffoldDb::None => {}
            ScaffoldDb::Postgres => parts.push("postgres DB".to_string()),
            ScaffoldDb::Sqlite => parts.push("sqlite DB".to_string()),
        }
        if self.requested_repo_name != self.repo_name {
            parts.push(format!("repo name {}", self.repo_name));
        }
        if !self.frontends().is_empty() {
            parts.push(format!("{} frontend app(s)", self.frontends().len()));
        }
        parts.join(", ")
    }

    pub(super) fn sanitized_repo_name_note(&self) -> Option<String> {
        (self.requested_repo_name != self.repo_name).then(|| {
            format!(
                "requested repo name '{}' was normalized to '{}' for generated package compatibility",
                self.requested_repo_name, self.repo_name
            )
        })
    }

    const fn identity(&self) -> ScaffoldIdentity {
        self.project.identity()
    }

    pub(super) const fn database(&self) -> ScaffoldDb {
        self.project.database()
    }

    pub(in crate::bootstrap::scaffold) fn frontends(&self) -> &[FrontendScaffold] {
        self.project
            .react()
            .map_or(&[], |react| react.frontends.as_slice())
    }

    pub(in crate::bootstrap::scaffold) fn custom_frontend_notices(&self) -> &[String] {
        self.project
            .react()
            .map_or(&[], |react| react.custom_frontend_notices.as_slice())
    }

    pub(super) fn database_enabled(&self) -> bool {
        self.database() != ScaffoldDb::None
    }

    pub(super) fn scaffolds_frontend_contracts(&self) -> bool {
        !self.frontends().is_empty()
    }

    pub(super) fn scaffolds_go_postgres_integration(&self) -> bool {
        matches!(
            &self.project,
            ScaffoldProjectPlan::GoReact(GoReactScaffoldPlan {
                backend: GoScaffoldPlan {
                    database: GoDatabase::Postgres,
                    ..
                },
                ..
            })
        )
    }

    #[cfg(test)]
    pub(super) fn write(&self, destination: &Path, force: bool) -> Result<Value> {
        let files = self.render_files()?;
        let report = ScaffoldReport::write_files(destination, files, force)?;
        Ok(report.into_json(self))
    }

    pub(super) fn write_rendered_with_transaction(
        &self,
        destination: &Path,
        files: Vec<ScaffoldFile>,
        force: bool,
        transaction: Option<&mut super::InitMutationTransaction>,
    ) -> Result<Value> {
        let report =
            ScaffoldReport::write_files_with_transaction(destination, files, force, transaction)?;
        Ok(report.into_json(self))
    }

    pub(super) fn preflight(&self, destination: &Path, force: bool) -> Result<()> {
        ScaffoldReport::preflight_files(destination, self.render_files()?, force)
    }

    pub(super) fn render_files(&self) -> Result<Vec<ScaffoldFile>> {
        match &self.project {
            ScaffoldProjectPlan::RustReact(project) => self.render_react_files(
                self.render_rust_workspace_files(&project.backend)?,
                ReactBackendRenderContext {
                    preset: ScaffoldPreset::RustReact,
                    database: project.backend.database,
                    root: ".",
                    migration_dir: &project.backend.migration_dir,
                    sqlx_metadata_dir: &project.backend.sqlx_metadata_dir,
                },
                &project.react,
            ),
            ScaffoldProjectPlan::GoReact(project) => {
                let sqlx_metadata_dir =
                    go_component_path(&project.backend.component_root, "internal/database/sqlc");
                self.render_react_files(
                    self.render_go_workspace_files(&project.backend)?,
                    ReactBackendRenderContext {
                        preset: ScaffoldPreset::GoReact,
                        database: self.database(),
                        root: &project.backend.component_root,
                        migration_dir: &project.backend.migration_dir,
                        sqlx_metadata_dir: &sqlx_metadata_dir,
                    },
                    &project.react,
                )
            }
            ScaffoldProjectPlan::RustOnly(project) => {
                self.render_rust_only_workspace_files(project)
            }
        }
    }

    fn render_react_files(
        &self,
        mut files: Vec<ScaffoldFile>,
        backend: ReactBackendRenderContext<'_>,
        react: &ReactScaffoldPlan,
    ) -> Result<Vec<ScaffoldFile>> {
        let frontend_backend = FrontendBackendContext {
            preset: backend.preset,
            root: backend.root,
            database: FrontendDatabaseContext {
                db: backend.database,
                migration_dir: backend.migration_dir,
                sqlx_metadata_dir: backend.sqlx_metadata_dir,
            },
        };
        files.extend(render_frontend_workspace_files_for_backend(
            frontend_backend,
            &react.package_manager,
            &self.package_name,
            &self.default_branch,
            &self.ci_github_runner,
            &react.frontends,
        )?);
        for frontend in &react.frontends {
            files.extend(frontend.render_files_for_backend(
                &react.package_manager,
                &self.repo_name,
                &react.repo_dns_label,
                &self.module_name,
                frontend_backend,
            )?);
        }
        Ok(files)
    }

    fn rust_react(opts: &ScaffoldOpts, answers: &AnswerOpts, destination: &Path) -> Result<Self> {
        let requested_repo_name = answers
            .repo_name
            .clone()
            .unwrap_or_else(|| default_repo_name(destination));
        let package_name = normalize_rust_react_package_name(&requested_repo_name)?;
        let repo_name = package_name.clone();
        let repo_dns_label = rust_react_repo_dns_label(&repo_name);
        // Rust package normalization validates the underscore form before this replacement.
        let module_name = package_name.replace('-', "_");
        let db = opts.db.unwrap_or(ScaffoldDb::None);
        let package_manager = answers
            .web_package_manager
            .clone()
            .unwrap_or_else(|| "bun".into());
        validate_web_package_manager(&package_manager)?;
        let default_branch = answers
            .default_branch
            .clone()
            .unwrap_or_else(|| "main".into());
        let ci_github_runner = answers
            .ci_github_runner
            .clone()
            .unwrap_or_else(|| "ubuntu-latest".into());
        if db != ScaffoldDb::None && answers.sqlx_enabled == Some(false) {
            bail!("Scaffold --db requires SQLx; remove --sqlx-enabled false or use --db none");
        }
        let migration_dir = answers
            .rust_migration_dir
            .clone()
            .unwrap_or_else(|| "migrations".into());
        if db != ScaffoldDb::None || answers.rust_migration_dir.is_some() {
            validate_scaffold_relative_path("migration dir", &migration_dir)?;
        }
        let sqlx_metadata_dir = answers
            .rust_sqlx_metadata_dir
            .clone()
            .unwrap_or_else(|| ".sqlx".into());
        if db != ScaffoldDb::None {
            validate_scaffold_relative_path("SQLx metadata dir", &sqlx_metadata_dir)?;
            if sqlx_metadata_dir != ".sqlx" {
                bail!(
                    "Rust-react database scaffolds pin SQLx 0.9 and require rust_sqlx_metadata_dir = '.sqlx' because cargo sqlx prepare --check checks that committed directory. Use .sqlx for jig init; for an existing custom metadata layout, use jig adopt and configure an explicit sqlx_check_command."
                );
            }
        }
        let frontend_specs = collect_frontend_specs(opts);
        let custom_frontend_notices = frontend_specs
            .iter()
            .filter_map(ScaffoldFrontend::custom_default_name_notice)
            .collect();
        if !frontend_specs.is_empty() && !answers.frontend_apps.is_empty() {
            bail!(
                "Scaffold frontends cannot be combined with --frontend-app answers; use --frontend/--frontends for scaffold output or --frontend-app for existing app configuration"
            );
        }
        let frontends = if frontend_specs.is_empty() && answers.frontend_apps.is_empty() {
            vec![FrontendScaffold::from_spec(ScaffoldFrontend {
                name: "web".into(),
                kind: ScaffoldFrontendKind::Spa,
                custom_default_name: false,
            })?]
        } else if frontend_specs.is_empty() {
            answers
                .frontend_apps
                .iter()
                .map(FrontendScaffold::from_frontend_app)
                .collect::<Result<Vec<_>>>()?
        } else {
            frontend_specs
                .into_iter()
                .map(FrontendScaffold::from_spec)
                .collect::<Result<Vec<_>>>()?
        };
        let root_workspace_package_name = format!("{package_name}-workspace");
        validate_unique_frontends(
            &frontends,
            &root_workspace_package_name,
            ScaffoldPreset::RustReact,
        )?;
        Ok(Self {
            project: ScaffoldProjectPlan::RustReact(RustReactScaffoldPlan {
                backend: RustScaffoldPlan {
                    database: db,
                    migration_dir,
                    sqlx_metadata_dir,
                },
                react: ReactScaffoldPlan {
                    repo_dns_label,
                    package_manager,
                    frontends,
                    custom_frontend_notices,
                },
            }),
            requested_repo_name,
            repo_name,
            package_name,
            module_name,
            default_branch,
            ci_github_runner,
        })
    }

    fn go_react(opts: &ScaffoldOpts, answers: &AnswerOpts, destination: &Path) -> Result<Self> {
        let requested_repo_name = answers
            .repo_name
            .clone()
            .unwrap_or_else(|| default_repo_name(destination));
        let package_name = normalize_package_name(&requested_repo_name)?;
        let repo_name = package_name.clone();
        let repo_dns_label = rust_react_repo_dns_label(&repo_name);
        let module_name = package_name.replace('-', "_");
        let go_module = answers
            .go_module
            .clone()
            .ok_or_else(|| anyhow::anyhow!("--preset go-react requires --go-module <module>"))?;
        validate_go_module(&go_module)?;
        let database = match opts.db.unwrap_or(ScaffoldDb::None) {
            ScaffoldDb::None => GoDatabase::None,
            ScaffoldDb::Postgres => GoDatabase::Postgres,
            ScaffoldDb::Sqlite => bail!(
                "--preset go-react does not support --db sqlite; use --db none or --db postgres"
            ),
        };
        let package_manager = answers
            .web_package_manager
            .clone()
            .unwrap_or_else(|| "bun".into());
        validate_web_package_manager(&package_manager)?;
        let default_branch = answers
            .default_branch
            .clone()
            .unwrap_or_else(|| "main".into());
        let ci_github_runner = answers
            .ci_github_runner
            .clone()
            .unwrap_or_else(|| "ubuntu-latest".into());
        let frontend_specs = collect_frontend_specs(opts);
        let custom_frontend_notices = frontend_specs
            .iter()
            .filter_map(ScaffoldFrontend::custom_default_name_notice)
            .collect();
        if !frontend_specs.is_empty() && !answers.frontend_apps.is_empty() {
            bail!(
                "Scaffold frontends cannot be combined with --frontend-app answers; use --frontend/--frontends for scaffold output or --frontend-app for existing app configuration"
            );
        }
        let frontends = if frontend_specs.is_empty() && answers.frontend_apps.is_empty() {
            vec![FrontendScaffold::from_spec(ScaffoldFrontend {
                name: "web".into(),
                kind: ScaffoldFrontendKind::Spa,
                custom_default_name: false,
            })?]
        } else if frontend_specs.is_empty() {
            answers
                .frontend_apps
                .iter()
                .map(FrontendScaffold::from_frontend_app)
                .collect::<Result<Vec<_>>>()?
        } else {
            frontend_specs
                .into_iter()
                .map(FrontendScaffold::from_spec)
                .collect::<Result<Vec<_>>>()?
        };
        if frontends
            .iter()
            .any(|frontend| frontend.kind == ScaffoldFrontendKind::Admin)
        {
            bail!(
                "--preset go-react does not yet support the admin frontend because it requires a separate privileged API and client boundary; use web and/or landing"
            );
        }
        validate_unique_frontends(
            &frontends,
            &format!("{package_name}-workspace"),
            ScaffoldPreset::GoReact,
        )?;
        let component_root = go_component_root(answers)?.to_owned();
        let migration_dir = answers
            .migration_dir
            .clone()
            .unwrap_or_else(|| go_component_path(&component_root, GO_POSTGRES_MIGRATION_DIR));
        Ok(Self {
            project: ScaffoldProjectPlan::GoReact(GoReactScaffoldPlan {
                backend: GoScaffoldPlan {
                    database,
                    module: go_module,
                    component_root,
                    migration_dir,
                },
                react: ReactScaffoldPlan {
                    repo_dns_label,
                    package_manager,
                    frontends,
                    custom_frontend_notices,
                },
            }),
            requested_repo_name,
            repo_name,
            package_name,
            module_name,
            default_branch,
            ci_github_runner,
        })
    }

    pub(super) fn rust_only(
        artifact: RustOnlyArtifact,
        answers: &AnswerOpts,
        destination: &Path,
    ) -> Result<Self> {
        let requested_repo_name = answers
            .repo_name
            .clone()
            .unwrap_or_else(|| default_repo_name(destination));
        let package_name = normalize_rust_react_package_name(&requested_repo_name)?;
        let repo_name = package_name.clone();
        let module_name = package_name.replace('-', "_");
        Ok(Self {
            project: ScaffoldProjectPlan::RustOnly(RustOnlyScaffoldPlan { artifact }),
            requested_repo_name,
            repo_name,
            package_name,
            module_name,
            default_branch: answers
                .default_branch
                .clone()
                .unwrap_or_else(|| "main".into()),
            ci_github_runner: answers
                .ci_github_runner
                .clone()
                .unwrap_or_else(|| "ubuntu-latest".into()),
        })
    }

    pub(super) fn output_paths(&self) -> Vec<PathBuf> {
        let (mut paths, react, preset) = match &self.project {
            ScaffoldProjectPlan::RustReact(project) => (
                self.rust_workspace_relative_paths(&project.backend),
                &project.react,
                ScaffoldPreset::RustReact,
            ),
            ScaffoldProjectPlan::GoReact(project) => (
                self.go_workspace_relative_paths(&project.backend),
                &project.react,
                ScaffoldPreset::GoReact,
            ),
            ScaffoldProjectPlan::RustOnly(project) => {
                return self.rust_only_workspace_relative_paths(project);
            }
        };
        paths.extend(frontend_workspace_relative_paths_for_backend(
            preset,
            &react.package_manager,
            &react.frontends,
        ));
        paths.extend(
            react
                .frontends
                .iter()
                .flat_map(FrontendScaffold::relative_paths),
        );
        paths
    }
}

fn optional_cargo_command(command: &str, action: &str) -> String {
    format!(
        "if [ -f Cargo.toml ]; then {command}; else printf '%s\\n' 'No Cargo.toml found; skipping cargo {action}.'; fi"
    )
}

pub(super) fn validate_go_component_root(component_root: &str) -> Result<()> {
    if component_root == "." {
        Ok(())
    } else {
        validate_scaffold_relative_path("Go component root", component_root)
    }
}

pub(super) fn go_component_root(answers: &AnswerOpts) -> Result<&str> {
    match answers.scaffold_go_component_roots.as_slice() {
        [] => Ok("."),
        [root] => Ok(root),
        roots => bail!(
            "The authored repository model has multiple Go component roots ({}); a Go browser scaffold requires one canonical API component root",
            roots.join(", ")
        ),
    }
}

pub(super) fn go_component_path(component_root: &str, relative: &str) -> String {
    if component_root == "." {
        relative.to_owned()
    } else {
        format!("{}/{relative}", component_root.trim_end_matches('/'))
    }
}

impl ScaffoldFrontendKind {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Spa => "spa",
            Self::Admin => "admin",
            Self::Astro => "astro",
        }
    }
}

fn collect_frontend_specs(opts: &ScaffoldOpts) -> Vec<ScaffoldFrontend> {
    opts.frontends
        .iter()
        .chain(opts.frontend_list.iter())
        .cloned()
        .collect()
}

fn validate_unique_frontends(
    frontends: &[FrontendScaffold],
    root_workspace_package_name: &str,
    preset: ScaffoldPreset,
) -> Result<()> {
    let mut names = HashSet::new();
    let mut dirs = HashSet::new();
    let mut package_names = HashMap::new();
    for frontend in frontends {
        validate_scaffold_relative_path("frontend dir", &frontend.dir)?;
        if frontend.package_name() == root_workspace_package_name {
            bail!(
                "Scaffold frontend '{}' normalizes to reserved root workspace package name '{}'",
                frontend.name,
                root_workspace_package_name
            );
        }
        if !names.insert(frontend.name.as_str()) {
            bail!("Duplicate scaffold frontend '{}'", frontend.name);
        }
        if !dirs.insert(frontend.dir.as_str()) {
            bail!("Duplicate scaffold frontend dir '{}'", frontend.dir);
        }
        if let Some(existing_name) =
            package_names.insert(frontend.package_name(), frontend.name.as_str())
        {
            bail!(
                "Scaffold frontend names '{}' and '{}' normalize to duplicate workspace package name '{}'",
                existing_name,
                frontend.name,
                frontend.package_name()
            );
        }
        let root_dir = frontend.dir.split('/').next().unwrap_or_default();
        if preset.reserved_backend_roots().contains(&root_dir) {
            bail!(
                "Scaffold frontend '{}' uses reserved directory '{}'",
                frontend.name,
                frontend.dir
            );
        }
    }
    Ok(())
}
