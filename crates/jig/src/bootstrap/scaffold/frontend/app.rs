use std::path::PathBuf;

use anyhow::Result;
use serde_json::json;

use super::super::super::GENERATED_NODE_TYPES_VERSION;
use super::super::super::answers::web_run_command;
use super::super::names::{
    bounded_postgres_identifier, normalize_package_name, validate_scaffold_name,
};
use super::super::templates::{
    ScaffoldTemplateFile, ensure_scaffold_template_paths, render_scaffold_template,
};
use super::super::write::{ScaffoldFile, scaffold_file};
use super::super::{
    FrontendApp, ScaffoldDb, ScaffoldFrontend, ScaffoldFrontendKind, ScaffoldPreset,
};
use super::templates::{
    ASTRO_TEMPLATES, SPA_SHADCN_TEMPLATES, VITE_REACT_TEMPLATES, admin_template_files,
};

pub(in crate::bootstrap::scaffold) const SHADCN_CLI_VERSION: &str = "4.18.0";
pub(in crate::bootstrap::scaffold) const SHADCN_PRESET: &str = "nova";
pub(in crate::bootstrap::scaffold) const SHADCN_BASE: &str = "radix";
pub(in crate::bootstrap::scaffold) const SHADCN_STYLE: &str = "radix-nova";
pub(in crate::bootstrap::scaffold) const SHADCN_TAILWIND_MAJOR: u8 = 4;

#[derive(Clone, Copy, Debug)]
pub(in crate::bootstrap::scaffold) struct FrontendDatabaseContext<'a> {
    pub(in crate::bootstrap::scaffold) db: ScaffoldDb,
    pub(in crate::bootstrap::scaffold) migration_dir: &'a str,
    pub(in crate::bootstrap::scaffold) sqlx_metadata_dir: &'a str,
}

#[derive(Clone, Copy, Debug)]
pub(in crate::bootstrap::scaffold) struct FrontendBackendContext<'a> {
    pub(in crate::bootstrap::scaffold) preset: ScaffoldPreset,
    pub(in crate::bootstrap::scaffold) root: &'a str,
    pub(in crate::bootstrap::scaffold) database: FrontendDatabaseContext<'a>,
}

#[derive(Clone, Debug)]
pub(in crate::bootstrap::scaffold) struct FrontendScaffold {
    pub(in crate::bootstrap::scaffold) name: String,
    pub(in crate::bootstrap::scaffold) dir: String,
    pub(in crate::bootstrap::scaffold) kind: ScaffoldFrontendKind,
    pub(in crate::bootstrap::scaffold) coverage_threshold: u32,
    pub(in crate::bootstrap::scaffold) dev_kind: String,
    pub(super) package_name: String,
}

impl FrontendScaffold {
    pub(in crate::bootstrap::scaffold) fn package_name(&self) -> &str {
        &self.package_name
    }

    pub(in crate::bootstrap::scaffold) fn from_spec(spec: ScaffoldFrontend) -> Result<Self> {
        validate_scaffold_name("frontend name", &spec.name)?;
        crate::bootstrap::repository_model::frontend_component_id(&spec.name)?;
        let package_name = normalize_package_name(&spec.name)?;
        let (coverage_threshold, dev_kind) = scaffold_frontend_defaults(spec.kind);
        Ok(Self {
            dir: spec.name.clone(),
            name: spec.name,
            kind: spec.kind,
            coverage_threshold,
            dev_kind: dev_kind.into(),
            package_name,
        })
    }

    pub(in crate::bootstrap::scaffold) fn from_frontend_app(app: &FrontendApp) -> Result<Self> {
        validate_scaffold_name("frontend app name", &app.name)?;
        crate::bootstrap::repository_model::frontend_component_id(&app.name)?;
        let kind = match app.role.as_str() {
            "spa" => ScaffoldFrontendKind::Spa,
            "admin" => ScaffoldFrontendKind::Admin,
            "astro" => ScaffoldFrontendKind::Astro,
            role => anyhow::bail!(
                "Unsupported frontend app role '{role}'. Expected spa, admin, or astro"
            ),
        };
        Ok(Self {
            name: app.name.clone(),
            dir: app.dir.clone(),
            kind,
            coverage_threshold: app.coverage_threshold,
            dev_kind: app.kind.clone(),
            package_name: normalize_package_name(&app.name)?,
        })
    }

    pub(in crate::bootstrap::scaffold) fn relative_paths(&self) -> Vec<PathBuf> {
        self.template_files()
            .into_iter()
            .map(|file| PathBuf::from(format!("{}/{}", self.dir, file.output)))
            .collect()
    }

    pub(in crate::bootstrap::scaffold) fn render_files_for_backend(
        &self,
        package_manager: &str,
        repo_name: &str,
        repo_dns_label: &str,
        module_name: &str,
        backend: FrontendBackendContext<'_>,
    ) -> Result<Vec<ScaffoldFile>> {
        self.render_template_files(
            package_manager,
            repo_name,
            repo_dns_label,
            module_name,
            backend,
        )
    }

    pub(super) fn render_template_files(
        &self,
        package_manager: &str,
        repo_name: &str,
        repo_dns_label: &str,
        module_name: &str,
        backend: FrontendBackendContext<'_>,
    ) -> Result<Vec<ScaffoldFile>> {
        let template_files = self.template_files();
        ensure_scaffold_template_paths(&template_files)?;
        let title = title_case(&self.name);
        let e2e_database_name = e2e_database_name(module_name, &self.package_name);
        let context = json!({
            "package_name": self.package_name,
            "frontend_dir": self.dir,
            "package_manager": package_manager,
            "node_types_version": GENERATED_NODE_TYPES_VERSION,
            "repo_name": repo_name,
            "public_api_client_package": format!("{repo_name}-public-api-client"),
            "admin_api_client_package": format!("{repo_name}-admin-api-client"),
            "repo_dns_label": repo_dns_label,
            "module_name": module_name,
            "e2e_database_name": e2e_database_name,
            "repo_root_relative": repo_root_relative(&self.dir),
            "db": match backend.database.db {
                ScaffoldDb::None => "none",
                ScaffoldDb::Postgres => "postgres",
                ScaffoldDb::Sqlite => "sqlite",
            },
            "backend_language": if backend.preset == ScaffoldPreset::GoReact { "go" } else { "rust" },
            "go_backend_root": backend.root,
            "title": title,
            "subtitle": if self.kind == ScaffoldFrontendKind::Admin {
                "Operational workspace"
            } else {
                "Product workspace"
            },
            "package_exec": scaffold_package_exec(package_manager),
            "web_run_command": web_run_command(package_manager),
            "shadcn_cli_version": SHADCN_CLI_VERSION,
            "shadcn_preset": SHADCN_PRESET,
            "shadcn_base": SHADCN_BASE,
            "shadcn_base_display": title_case(SHADCN_BASE),
            "shadcn_style": SHADCN_STYLE,
            "shadcn_tailwind_major": SHADCN_TAILWIND_MAJOR,
        });
        template_files
            .iter()
            .map(|file| {
                Ok(scaffold_file(
                    format!("{}/{}", self.dir, file.output),
                    render_scaffold_template(file.template, &context)?,
                ))
            })
            .collect()
    }

    pub(super) fn template_files(&self) -> Vec<ScaffoldTemplateFile> {
        match self.kind {
            ScaffoldFrontendKind::Spa => VITE_REACT_TEMPLATES
                .iter()
                .chain(SPA_SHADCN_TEMPLATES)
                .copied()
                .collect(),
            ScaffoldFrontendKind::Admin => admin_template_files(),
            ScaffoldFrontendKind::Astro => ASTRO_TEMPLATES.to_vec(),
        }
    }

    pub(in crate::bootstrap::scaffold) fn ui_provenance(&self) -> Option<serde_json::Value> {
        matches!(
            self.kind,
            ScaffoldFrontendKind::Spa | ScaffoldFrontendKind::Admin
        )
        .then(|| {
            json!({
                "system": "shadcn",
                "cli_version": SHADCN_CLI_VERSION,
                "preset": SHADCN_PRESET,
                "base": SHADCN_BASE,
                "style": SHADCN_STYLE,
                "tailwind_major": SHADCN_TAILWIND_MAJOR,
            })
        })
    }
}

pub(super) const fn scaffold_frontend_defaults(kind: ScaffoldFrontendKind) -> (u32, &'static str) {
    match kind {
        ScaffoldFrontendKind::Spa | ScaffoldFrontendKind::Admin => (80, "vite"),
        ScaffoldFrontendKind::Astro => (0, "env-port"),
    }
}

pub(super) fn scaffold_package_exec(package_manager: &str) -> &'static str {
    match package_manager {
        "bun" => "bunx",
        "npm" => "npx",
        "pnpm" => "pnpm dlx",
        "yarn" => "yarn dlx",
        _ => unreachable!("web package manager was already validated"),
    }
}

pub(super) fn title_case(value: &str) -> String {
    value
        .split(['-', '_'])
        .filter(|part| !part.is_empty())
        .map(|part| {
            let mut chars = part.chars();
            match chars.next() {
                Some(first) => first.to_ascii_uppercase().to_string() + chars.as_str(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

pub(super) fn repo_root_relative(frontend_dir: &str) -> String {
    let depth = PathBuf::from(frontend_dir).components().count();
    (0..depth).map(|_| "..").collect::<Vec<_>>().join("/")
}

pub(super) fn e2e_database_name(module_name: &str, frontend_package_name: &str) -> String {
    let name = format!(
        "{module_name}_{}_e2e",
        frontend_package_name.replace('-', "_")
    );
    bounded_postgres_identifier(&name)
}
