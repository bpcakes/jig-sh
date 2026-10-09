//! `jig init` scaffold choices: preset, database, frontends, metrics, and jobs.

use anyhow::{Result, bail};
use clap::{Args, ValueEnum};

use super::answers::HarnessFootprint;
use super::init::should_default_init_sqlx_disabled;
use super::opts::AnswerOpts;

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
    #[arg(
        long,
        value_enum,
        help_heading = "Project Shape",
        help = "Metrics export for --preset rust-react; otlp compiles Batter's bounded OTLP/HTTP exporter, enabled at runtime by METRICS_OTLP_ENDPOINT"
    )]
    pub metrics: Option<ScaffoldMetrics>,
    #[arg(
        long,
        value_enum,
        help_heading = "Project Shape",
        help = "Background jobs for --preset rust-react --db postgres; runledger adds a Batter-supervised Runledger worker"
    )]
    pub jobs: Option<ScaffoldJobs>,
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
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub enum ScaffoldMetrics {
    None,
    Otlp,
}

impl ScaffoldMetrics {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Otlp => "otlp",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub enum ScaffoldJobs {
    None,
    Runledger,
}

impl ScaffoldJobs {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Runledger => "runledger",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ScaffoldFrontend {
    pub(super) name: String,
    pub(super) kind: ScaffoldFrontendKind,
    pub(super) custom_default_name: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum ScaffoldFrontendKind {
    Spa,
    Admin,
    Astro,
}

pub fn parse_scaffold_frontend(value: &str) -> Result<ScaffoldFrontend, String> {
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
    pub fn custom_default_name_notice(&self) -> Option<String> {
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
    pub fn normalize_minimal_harness_shape(&mut self, answers: &AnswerOpts) {
        if answers.harness_footprint == Some(HarnessFootprint::Minimal) && self.preset.is_none() {
            self.preset = Some(ScaffoldPreset::HarnessOnly);
        }
    }

    pub fn has_frontends(&self) -> bool {
        !self.frontends.is_empty() || !self.frontend_list.is_empty()
    }

    pub fn has_service_options(&self) -> bool {
        self.metrics.is_some() || self.jobs.is_some()
    }

    pub fn custom_frontend_notices(&self) -> Vec<String> {
        self.frontends
            .iter()
            .chain(self.frontend_list.iter())
            .filter_map(ScaffoldFrontend::custom_default_name_notice)
            .collect()
    }

    pub fn validate_init_invariants(&self, answers: &AnswerOpts) -> Result<()> {
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
                || self.has_service_options()
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
            && (self.db.is_some()
                || self.has_frontends()
                || self.has_service_options()
                || answers.go_module.is_some())
        {
            bail!(
                "--preset harness-only cannot be combined with --db, --go-module, --frontend, --frontends, --metrics, or --jobs; remove the scaffold flags or use an application preset"
            );
        }
        if let Some(preset) = self.preset
            && self.metrics.is_some()
            && !preset.supports_metrics()
        {
            bail!(
                "--metrics requires --preset rust-react; --preset {} does not generate a Batter service",
                preset.as_str()
            );
        }
        if let Some(preset) = self.preset
            && self.jobs.is_some()
            && !preset.supports_jobs()
        {
            bail!(
                "--jobs requires --preset rust-react; --preset {} does not generate a Batter service",
                preset.as_str()
            );
        }
        if self.jobs == Some(ScaffoldJobs::Runledger) && self.db == Some(ScaffoldDb::None) {
            bail!(
                "--jobs runledger requires --db postgres because Runledger stores durable jobs in PostgreSQL"
            );
        }
        if !self.preset.is_some_and(ScaffoldPreset::supports_go_module)
            && answers.go_module.is_some()
        {
            bail!("--go-module requires --preset go-react");
        }
        if self.preset == Some(ScaffoldPreset::GoReact) {
            if let Some(go_module) = answers.go_module.as_deref() {
                super::scaffold::validate_go_module(go_module)?;
            }
            let go_component_root = super::scaffold::go_component_root(answers)?;
            super::scaffold::validate_go_component_root(go_component_root)?;
            let initial_migration_dir = super::scaffold::go_component_path(
                go_component_root,
                jig_context::backend::GO_POSTGRES_MIGRATION_DIR,
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

    pub fn apply_init_answer_defaults(&self, answers: &mut AnswerOpts) {
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
