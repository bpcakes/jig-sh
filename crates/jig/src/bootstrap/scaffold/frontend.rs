use std::path::PathBuf;

use anyhow::Result;
use serde_json::json;

use super::super::answers::{web_install_command, web_run_command};
use super::super::{
    GENERATED_NODE_TYPES_VERSION, GENERATED_NODE_VERSION, generated_package_manager_spec,
    generated_package_manager_version,
};
use super::embedded_templates::EMBEDDED_SCAFFOLD_TEMPLATE_FILES;
use super::names::{bounded_postgres_identifier, sanitize_package_name, validate_scaffold_name};
use super::templates::{
    ScaffoldTemplateFile, ensure_scaffold_template_paths, render_scaffold_template,
};
use super::write::{ScaffoldFile, scaffold_file};
use super::{FrontendApp, ScaffoldDb, ScaffoldFrontend, ScaffoldFrontendKind};

const FRONTEND_WORKSPACE_TEMPLATES: &[ScaffoldTemplateFile] = &[
    ScaffoldTemplateFile {
        template: "rust-react/frontend/workspace/README.md.jinja",
        output: "README.md",
    },
    ScaffoldTemplateFile {
        template: "rust-react/frontend/workspace/package.json.jinja",
        output: "package.json",
    },
    ScaffoldTemplateFile {
        template: "rust-react/frontend/workspace/.node-version.jinja",
        output: ".node-version",
    },
    ScaffoldTemplateFile {
        template: "rust-react/frontend/workspace/contracts.mjs.jinja",
        output: "scripts/contracts.mjs",
    },
];

const REACT_ESLINT_TEMPLATE: ScaffoldTemplateFile = ScaffoldTemplateFile {
    template: "rust-react/frontend/workspace/eslint.config.shared.mjs.jinja",
    output: "eslint.config.shared.mjs",
};

const PNPM_WORKSPACE_TEMPLATE: ScaffoldTemplateFile = ScaffoldTemplateFile {
    template: "rust-react/frontend/workspace/pnpm-workspace.yaml.jinja",
    output: "pnpm-workspace.yaml",
};

const YARN_WORKSPACE_TEMPLATE: ScaffoldTemplateFile = ScaffoldTemplateFile {
    template: "rust-react/frontend/workspace/.yarnrc.yml.jinja",
    output: ".yarnrc.yml",
};

const E2E_WORKFLOW_TEMPLATE: ScaffoldTemplateFile = ScaffoldTemplateFile {
    template: "rust-react/frontend/workspace/e2e.yml.jinja",
    output: ".github/workflows/e2e.yml",
};

const ADMIN_TEMPLATE_PREFIX: &str = "rust-react/frontend/admin-shadcn/";
const PUBLIC_API_CLIENT_TEMPLATES: &[ScaffoldTemplateFile] = &[
    ScaffoldTemplateFile {
        template: "rust-react/frontend/api-client-public/package.json.jinja",
        output: "packages/public-api-client/package.json",
    },
    ScaffoldTemplateFile {
        template: "rust-react/frontend/api-client-public/src/index.ts.jinja",
        output: "packages/public-api-client/src/index.ts",
    },
    ScaffoldTemplateFile {
        template: "rust-react/frontend/api-client-public/src/generated/client.gen.ts.jinja",
        output: "packages/public-api-client/src/generated/client.gen.ts",
    },
    ScaffoldTemplateFile {
        template: "rust-react/frontend/api-client-public/src/generated/client/client.gen.ts.jinja",
        output: "packages/public-api-client/src/generated/client/client.gen.ts",
    },
    ScaffoldTemplateFile {
        template: "rust-react/frontend/api-client-public/src/generated/client/index.ts.jinja",
        output: "packages/public-api-client/src/generated/client/index.ts",
    },
    ScaffoldTemplateFile {
        template: "rust-react/frontend/api-client-public/src/generated/client/types.gen.ts.jinja",
        output: "packages/public-api-client/src/generated/client/types.gen.ts",
    },
    ScaffoldTemplateFile {
        template: "rust-react/frontend/api-client-public/src/generated/client/utils.gen.ts.jinja",
        output: "packages/public-api-client/src/generated/client/utils.gen.ts",
    },
    ScaffoldTemplateFile {
        template: "rust-react/frontend/api-client-public/src/generated/core/auth.gen.ts.jinja",
        output: "packages/public-api-client/src/generated/core/auth.gen.ts",
    },
    ScaffoldTemplateFile {
        template: "rust-react/frontend/api-client-public/src/generated/core/bodySerializer.gen.ts.jinja",
        output: "packages/public-api-client/src/generated/core/bodySerializer.gen.ts",
    },
    ScaffoldTemplateFile {
        template: "rust-react/frontend/api-client-public/src/generated/core/params.gen.ts.jinja",
        output: "packages/public-api-client/src/generated/core/params.gen.ts",
    },
    ScaffoldTemplateFile {
        template: "rust-react/frontend/api-client-public/src/generated/core/pathSerializer.gen.ts.jinja",
        output: "packages/public-api-client/src/generated/core/pathSerializer.gen.ts",
    },
    ScaffoldTemplateFile {
        template: "rust-react/frontend/api-client-public/src/generated/core/queryKeySerializer.gen.ts.jinja",
        output: "packages/public-api-client/src/generated/core/queryKeySerializer.gen.ts",
    },
    ScaffoldTemplateFile {
        template: "rust-react/frontend/api-client-public/src/generated/core/serverSentEvents.gen.ts.jinja",
        output: "packages/public-api-client/src/generated/core/serverSentEvents.gen.ts",
    },
    ScaffoldTemplateFile {
        template: "rust-react/frontend/api-client-public/src/generated/core/types.gen.ts.jinja",
        output: "packages/public-api-client/src/generated/core/types.gen.ts",
    },
    ScaffoldTemplateFile {
        template: "rust-react/frontend/api-client-public/src/generated/core/utils.gen.ts.jinja",
        output: "packages/public-api-client/src/generated/core/utils.gen.ts",
    },
    ScaffoldTemplateFile {
        template: "rust-react/frontend/api-client-public/src/generated/types.gen.ts.jinja",
        output: "packages/public-api-client/src/generated/types.gen.ts",
    },
    ScaffoldTemplateFile {
        template: "rust-react/frontend/api-client-public/src/generated/index.ts.jinja",
        output: "packages/public-api-client/src/generated/index.ts",
    },
    ScaffoldTemplateFile {
        template: "rust-react/frontend/api-client-public/src/generated/sdk.gen.ts.jinja",
        output: "packages/public-api-client/src/generated/sdk.gen.ts",
    },
    ScaffoldTemplateFile {
        template: "rust-react/frontend/api-client-public/src/generated/zod.gen.ts.jinja",
        output: "packages/public-api-client/src/generated/zod.gen.ts",
    },
    ScaffoldTemplateFile {
        template: "rust-react/frontend/api-client-public/src/generated/@tanstack/react-query.gen.ts.jinja",
        output: "packages/public-api-client/src/generated/@tanstack/react-query.gen.ts",
    },
];
const ADMIN_API_CLIENT_TEMPLATES: &[ScaffoldTemplateFile] = &[
    ScaffoldTemplateFile {
        template: "rust-react/frontend/api-client-admin/package.json.jinja",
        output: "packages/admin-api-client/package.json",
    },
    ScaffoldTemplateFile {
        template: "rust-react/frontend/api-client-admin/src/index.ts.jinja",
        output: "packages/admin-api-client/src/index.ts",
    },
    ScaffoldTemplateFile {
        template: "rust-react/frontend/api-client-admin/src/generated/client.gen.ts.jinja",
        output: "packages/admin-api-client/src/generated/client.gen.ts",
    },
    ScaffoldTemplateFile {
        template: "rust-react/frontend/api-client-admin/src/generated/client/client.gen.ts.jinja",
        output: "packages/admin-api-client/src/generated/client/client.gen.ts",
    },
    ScaffoldTemplateFile {
        template: "rust-react/frontend/api-client-admin/src/generated/client/index.ts.jinja",
        output: "packages/admin-api-client/src/generated/client/index.ts",
    },
    ScaffoldTemplateFile {
        template: "rust-react/frontend/api-client-admin/src/generated/client/types.gen.ts.jinja",
        output: "packages/admin-api-client/src/generated/client/types.gen.ts",
    },
    ScaffoldTemplateFile {
        template: "rust-react/frontend/api-client-admin/src/generated/client/utils.gen.ts.jinja",
        output: "packages/admin-api-client/src/generated/client/utils.gen.ts",
    },
    ScaffoldTemplateFile {
        template: "rust-react/frontend/api-client-admin/src/generated/core/auth.gen.ts.jinja",
        output: "packages/admin-api-client/src/generated/core/auth.gen.ts",
    },
    ScaffoldTemplateFile {
        template: "rust-react/frontend/api-client-admin/src/generated/core/bodySerializer.gen.ts.jinja",
        output: "packages/admin-api-client/src/generated/core/bodySerializer.gen.ts",
    },
    ScaffoldTemplateFile {
        template: "rust-react/frontend/api-client-admin/src/generated/core/params.gen.ts.jinja",
        output: "packages/admin-api-client/src/generated/core/params.gen.ts",
    },
    ScaffoldTemplateFile {
        template: "rust-react/frontend/api-client-admin/src/generated/core/pathSerializer.gen.ts.jinja",
        output: "packages/admin-api-client/src/generated/core/pathSerializer.gen.ts",
    },
    ScaffoldTemplateFile {
        template: "rust-react/frontend/api-client-admin/src/generated/core/queryKeySerializer.gen.ts.jinja",
        output: "packages/admin-api-client/src/generated/core/queryKeySerializer.gen.ts",
    },
    ScaffoldTemplateFile {
        template: "rust-react/frontend/api-client-admin/src/generated/core/serverSentEvents.gen.ts.jinja",
        output: "packages/admin-api-client/src/generated/core/serverSentEvents.gen.ts",
    },
    ScaffoldTemplateFile {
        template: "rust-react/frontend/api-client-admin/src/generated/core/types.gen.ts.jinja",
        output: "packages/admin-api-client/src/generated/core/types.gen.ts",
    },
    ScaffoldTemplateFile {
        template: "rust-react/frontend/api-client-admin/src/generated/core/utils.gen.ts.jinja",
        output: "packages/admin-api-client/src/generated/core/utils.gen.ts",
    },
    ScaffoldTemplateFile {
        template: "rust-react/frontend/api-client-admin/src/generated/types.gen.ts.jinja",
        output: "packages/admin-api-client/src/generated/types.gen.ts",
    },
    ScaffoldTemplateFile {
        template: "rust-react/frontend/api-client-admin/src/generated/index.ts.jinja",
        output: "packages/admin-api-client/src/generated/index.ts",
    },
    ScaffoldTemplateFile {
        template: "rust-react/frontend/api-client-admin/src/generated/sdk.gen.ts.jinja",
        output: "packages/admin-api-client/src/generated/sdk.gen.ts",
    },
    ScaffoldTemplateFile {
        template: "rust-react/frontend/api-client-admin/src/generated/zod.gen.ts.jinja",
        output: "packages/admin-api-client/src/generated/zod.gen.ts",
    },
    ScaffoldTemplateFile {
        template: "rust-react/frontend/api-client-admin/src/generated/@tanstack/react-query.gen.ts.jinja",
        output: "packages/admin-api-client/src/generated/@tanstack/react-query.gen.ts",
    },
];
pub(super) const SHADCN_CLI_VERSION: &str = "4.18.0";
pub(super) const SHADCN_PRESET: &str = "nova";
pub(super) const SHADCN_BASE: &str = "radix";
pub(super) const SHADCN_STYLE: &str = "radix-nova";
pub(super) const SHADCN_TAILWIND_MAJOR: u8 = 4;
const DATABASE_CONFIG_GUARD: &str = r#"if [ -z "${DATABASE_URL:-}" ] && ! awk '/^[[:space:]]*(#|$)/ { next } /^[[:space:]]*(export[[:space:]]+)?DATABASE_URL[[:space:]]*=/ { value = $0; sub(/^[^=]*=[[:space:]]*/, "", value); sub(/^#.*$/, "", value); sub(/[[:space:]]+#.*$/, "", value); gsub(/^[[:space:]]+|[[:space:]]+$/, "", value); single_quote = sprintf("%c", 39); if (value != "" && value != "\"\"" && value != single_quote single_quote) found = 1 } END { exit found ? 0 : 1 }' .env 2>/dev/null; then printf '%s\n' 'Missing DATABASE_URL; export it or copy .env.example to .env before bootstrap.' >&2; exit 1; fi"#;

const VITE_REACT_TEMPLATES: &[ScaffoldTemplateFile] = &[
    ScaffoldTemplateFile {
        template: "rust-react/frontend/vite-react/.gitignore.jinja",
        output: ".gitignore",
    },
    ScaffoldTemplateFile {
        template: "rust-react/frontend/vite-react/package.json.jinja",
        output: "package.json",
    },
    ScaffoldTemplateFile {
        template: "rust-react/frontend/vite-react/index.html.jinja",
        output: "index.html",
    },
    ScaffoldTemplateFile {
        template: "rust-react/frontend/vite-react/vite.config.ts.jinja",
        output: "vite.config.ts",
    },
    ScaffoldTemplateFile {
        template: "rust-react/frontend/vite-react/playwright.config.ts.jinja",
        output: "playwright.config.ts",
    },
    ScaffoldTemplateFile {
        template: "rust-react/frontend/vite-react/tsconfig.json.jinja",
        output: "tsconfig.json",
    },
    ScaffoldTemplateFile {
        template: "rust-react/frontend/vite-react/tsconfig.app.json.jinja",
        output: "tsconfig.app.json",
    },
    ScaffoldTemplateFile {
        template: "rust-react/frontend/vite-react/tsconfig.node.json.jinja",
        output: "tsconfig.node.json",
    },
    ScaffoldTemplateFile {
        template: "rust-react/frontend/vite-react/eslint.config.js.jinja",
        output: "eslint.config.js",
    },
    ScaffoldTemplateFile {
        template: "rust-react/frontend/vite-react/src/main.tsx.jinja",
        output: "src/main.tsx",
    },
    ScaffoldTemplateFile {
        template: "rust-react/frontend/vite-react/src/app/providers.tsx.jinja",
        output: "src/app/providers.tsx",
    },
    ScaffoldTemplateFile {
        template: "rust-react/frontend/vite-react/src/app/router-context.ts.jinja",
        output: "src/app/router-context.ts",
    },
    ScaffoldTemplateFile {
        template: "rust-react/frontend/vite-react/src/app/router.ts.jinja",
        output: "src/app/router.ts",
    },
    ScaffoldTemplateFile {
        template: "rust-react/frontend/vite-react/src/App.tsx.jinja",
        output: "src/App.tsx",
    },
    ScaffoldTemplateFile {
        template: "rust-react/frontend/vite-react/src/App.test.tsx.jinja",
        output: "src/App.test.tsx",
    },
    ScaffoldTemplateFile {
        template: "rust-react/frontend/vite-react/src/api.ts.jinja",
        output: "src/api.ts",
    },
    ScaffoldTemplateFile {
        template: "rust-react/frontend/vite-react/src/lib/query-client.ts.jinja",
        output: "src/lib/query-client.ts",
    },
    ScaffoldTemplateFile {
        template: "rust-react/frontend/vite-react/src/routes/__root.tsx.jinja",
        output: "src/routes/__root.tsx",
    },
    ScaffoldTemplateFile {
        template: "rust-react/frontend/vite-react/src/routes/index.tsx.jinja",
        output: "src/routes/index.tsx",
    },
    ScaffoldTemplateFile {
        template: "rust-react/frontend/vite-react/src/routeTree.gen.ts.jinja",
        output: "src/routeTree.gen.ts",
    },
    ScaffoldTemplateFile {
        template: "rust-react/frontend/vite-react/README.md.jinja",
        output: "README.md",
    },
    ScaffoldTemplateFile {
        template: "rust-react/frontend/admin-shadcn/src/index.css.jinja",
        output: "src/index.css",
    },
    ScaffoldTemplateFile {
        template: "rust-react/frontend/vite-react/src/test-setup.ts.jinja",
        output: "src/test-setup.ts",
    },
    ScaffoldTemplateFile {
        template: "rust-react/frontend/vite-react/e2e/app.spec.ts.jinja",
        output: "e2e/app.spec.ts",
    },
];

// Keep one canonical copy of registry-generated shadcn source while allowing each
// generated application to own the rendered component files independently.
const SPA_SHADCN_TEMPLATES: &[ScaffoldTemplateFile] = &[
    ScaffoldTemplateFile {
        template: "rust-react/frontend/admin-shadcn/components.json.jinja",
        output: "components.json",
    },
    ScaffoldTemplateFile {
        template: "rust-react/frontend/admin-shadcn/src/lib/utils.ts.jinja",
        output: "src/lib/utils.ts",
    },
    ScaffoldTemplateFile {
        template: "rust-react/frontend/admin-shadcn/src/components/ui/alert.tsx.jinja",
        output: "src/components/ui/alert.tsx",
    },
    ScaffoldTemplateFile {
        template: "rust-react/frontend/admin-shadcn/src/components/ui/badge.tsx.jinja",
        output: "src/components/ui/badge.tsx",
    },
    ScaffoldTemplateFile {
        template: "rust-react/frontend/admin-shadcn/src/components/ui/button.tsx.jinja",
        output: "src/components/ui/button.tsx",
    },
    ScaffoldTemplateFile {
        template: "rust-react/frontend/admin-shadcn/src/components/ui/card.tsx.jinja",
        output: "src/components/ui/card.tsx",
    },
    ScaffoldTemplateFile {
        template: "rust-react/frontend/admin-shadcn/src/components/ui/skeleton.tsx.jinja",
        output: "src/components/ui/skeleton.tsx",
    },
];

const ASTRO_TEMPLATES: &[ScaffoldTemplateFile] = &[
    ScaffoldTemplateFile {
        template: "rust-react/frontend/astro/package.json.jinja",
        output: "package.json",
    },
    ScaffoldTemplateFile {
        template: "rust-react/frontend/astro/astro.config.mjs.jinja",
        output: "astro.config.mjs",
    },
    ScaffoldTemplateFile {
        template: "rust-react/frontend/astro/tsconfig.json.jinja",
        output: "tsconfig.json",
    },
    ScaffoldTemplateFile {
        template: "rust-react/frontend/astro/src/pages/index.astro.jinja",
        output: "src/pages/index.astro",
    },
];

#[derive(Clone, Debug)]
pub(super) struct FrontendScaffold {
    pub(super) name: String,
    pub(super) dir: String,
    pub(super) kind: ScaffoldFrontendKind,
    pub(super) coverage_threshold: u32,
    pub(super) dev_kind: String,
    package_name: String,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct FrontendDatabaseContext<'a> {
    pub(super) db: ScaffoldDb,
    pub(super) migration_dir: &'a str,
    pub(super) sqlx_metadata_dir: &'a str,
}

impl FrontendScaffold {
    pub(super) fn package_name(&self) -> &str {
        &self.package_name
    }

    pub(super) fn from_spec(spec: ScaffoldFrontend) -> Result<Self> {
        validate_scaffold_name("frontend name", &spec.name)?;
        let package_name = sanitize_package_name(&spec.name)?;
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

    pub(super) fn from_frontend_app(app: &FrontendApp) -> Result<Self> {
        validate_scaffold_name("frontend app name", &app.name)?;
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
            package_name: sanitize_package_name(&app.name)?,
        })
    }

    pub(super) fn relative_paths(&self) -> Vec<PathBuf> {
        self.template_files()
            .into_iter()
            .map(|file| PathBuf::from(format!("{}/{}", self.dir, file.output)))
            .collect()
    }

    pub(super) fn render_files(
        &self,
        package_manager: &str,
        repo_name: &str,
        repo_dns_label: &str,
        module_name: &str,
        db: ScaffoldDb,
    ) -> Result<Vec<ScaffoldFile>> {
        self.render_template_files(package_manager, repo_name, repo_dns_label, module_name, db)
    }

    fn render_template_files(
        &self,
        package_manager: &str,
        repo_name: &str,
        repo_dns_label: &str,
        module_name: &str,
        db: ScaffoldDb,
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
            "db": match db {
                ScaffoldDb::None => "none",
                ScaffoldDb::Postgres => "postgres",
                ScaffoldDb::Sqlite => "sqlite",
            },
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

    fn template_files(&self) -> Vec<ScaffoldTemplateFile> {
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

    pub(super) fn ui_provenance(&self) -> Option<serde_json::Value> {
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

fn admin_template_files() -> Vec<ScaffoldTemplateFile> {
    EMBEDDED_SCAFFOLD_TEMPLATE_FILES
        .iter()
        .filter_map(|file| {
            let output = file
                .relative_path
                .strip_prefix(ADMIN_TEMPLATE_PREFIX)?
                .strip_suffix(".jinja")?;
            Some(ScaffoldTemplateFile {
                template: file.relative_path,
                output,
            })
        })
        .collect()
}

const fn scaffold_frontend_defaults(kind: ScaffoldFrontendKind) -> (u32, &'static str) {
    match kind {
        ScaffoldFrontendKind::Spa | ScaffoldFrontendKind::Admin => (80, "vite"),
        ScaffoldFrontendKind::Astro => (0, "env-port"),
    }
}

pub(super) fn scaffold_bootstrap_command(
    package_name: &str,
    db: ScaffoldDb,
    frontends: &[FrontendScaffold],
) -> String {
    let mut commands = Vec::new();
    commands.push(optional_cargo_command("cargo fetch", "bootstrap"));
    if !frontends.is_empty() {
        commands.push("scripts/check-webapps.sh bootstrap".into());
    }
    if db != ScaffoldDb::None {
        commands.push(DATABASE_CONFIG_GUARD.into());
        commands.push(format!(
            "cargo run -p {package_name}-api -- --bootstrap-database"
        ));
    }
    commands.join(" && ")
}

fn scaffold_package_exec(package_manager: &str) -> &'static str {
    match package_manager {
        "bun" => "bunx",
        "npm" => "npx",
        "pnpm" => "pnpm dlx",
        "yarn" => "yarn dlx",
        _ => unreachable!("web package manager was already validated"),
    }
}

pub(super) fn render_frontend_workspace_files(
    package_manager: &str,
    package_name: &str,
    database: FrontendDatabaseContext<'_>,
    default_branch: &str,
    ci_github_runner: &str,
    frontends: &[FrontendScaffold],
) -> Result<Vec<ScaffoldFile>> {
    let FrontendDatabaseContext {
        db,
        migration_dir,
        sqlx_metadata_dir,
    } = database;
    let template_files = frontend_workspace_template_files(package_manager, frontends);
    ensure_scaffold_template_paths(&template_files)?;
    if template_files.is_empty() {
        return Ok(Vec::new());
    }
    let default_branch_yaml = serde_json::to_string(default_branch)?;
    let admin_api_enabled = frontends
        .iter()
        .any(|frontend| frontend.kind == ScaffoldFrontendKind::Admin);
    let context = json!({
        "package_name": package_name,
        "package_manager": package_manager,
        "package_manager_spec": generated_package_manager_spec(package_manager),
        "package_manager_version": generated_package_manager_version(package_manager),
        "node_version": GENERATED_NODE_VERSION,
        "web_install_command": web_install_command(package_manager),
        "web_run_command": web_run_command(package_manager),
        "db": match db {
            ScaffoldDb::None => "none",
            ScaffoldDb::Postgres => "postgres",
            ScaffoldDb::Sqlite => "sqlite",
        },
        "migration_dir": migration_dir,
        "sqlx_metadata_dir": sqlx_metadata_dir,
        "default_branch_yaml": default_branch_yaml,
        "ci_github_runner": ci_github_runner,
        "admin_api_enabled": admin_api_enabled,
        "react_frontend_enabled": frontends.iter().any(|frontend| matches!(
            frontend.kind,
            ScaffoldFrontendKind::Spa | ScaffoldFrontendKind::Admin
        )),
        "public_frontend_dirs": frontends.iter()
            .filter(|frontend| frontend.kind == ScaffoldFrontendKind::Spa)
            .map(|frontend| frontend.dir.as_str())
            .collect::<Vec<_>>(),
        "e2e_workflow_paths": e2e_workflow_paths(
            db,
            migration_dir,
            sqlx_metadata_dir,
            frontends,
        ),
        "frontends": frontends.iter().map(|frontend| json!({
            "name": frontend.name,
            "dir": frontend.dir,
        })).collect::<Vec<_>>(),
        "e2e_frontends": frontends.iter()
            .filter(|frontend| frontend.kind == ScaffoldFrontendKind::Spa)
            .map(|frontend| json!({
                "name": frontend.name,
                "dir": frontend.dir,
            }))
            .collect::<Vec<_>>(),
    });
    template_files
        .iter()
        .map(|file| {
            Ok(scaffold_file(
                file.output,
                render_scaffold_template(file.template, &context)?,
            ))
        })
        .collect()
}

fn e2e_workflow_paths(
    db: ScaffoldDb,
    migration_dir: &str,
    sqlx_metadata_dir: &str,
    frontends: &[FrontendScaffold],
) -> Vec<String> {
    let mut paths = frontends
        .iter()
        .filter(|frontend| frontend.kind == ScaffoldFrontendKind::Spa)
        .map(|frontend| format!("{}/**", frontend.dir))
        .collect::<Vec<_>>();
    paths.extend(["apps/**", "crates/**"].map(str::to_owned));
    if db != ScaffoldDb::None {
        paths.push(format!("{migration_dir}/**"));
    }
    paths.extend(
        [
            "Cargo.toml",
            "Cargo.lock",
            "rust-toolchain",
            "rust-toolchain.toml",
            ".cargo/**",
        ]
        .map(str::to_owned),
    );
    if db != ScaffoldDb::None {
        paths.push(format!("{sqlx_metadata_dir}/**"));
    }
    paths.extend(
        [
            "package.json",
            "**/package.json",
            "**/package.json5",
            "**/package.yaml",
            "**/*.patch",
            "**/*.diff",
            ".node-version",
            ".npmrc",
            "**/.node-version",
            "**/.npmrc",
            ".pnpmfile.cjs",
            "pnpmfile.cjs",
            ".yarnrc",
            ".yarnrc.yml",
            ".yarn/**",
            "**/.yarnrc",
            "**/.yarnrc.yml",
            "**/.yarn/**",
            ".pnp.cjs",
            ".pnp.data.json",
            ".pnp.js",
            ".pnp.loader.mjs",
            "patches/**",
            "bunfig.toml",
            "bun.lock",
            "bun.lockb",
            "npm-shrinkwrap.json",
            "package-lock.json",
            "pnpm-lock.yaml",
            "pnpm-workspace.yaml",
            "yarn.lock",
            "scripts/check-webapps.sh",
            "scripts/contracts.mjs",
            "eslint.config.shared.mjs",
            "openapi/**",
            "packages/public-api-client/**",
            ".github/workflows/e2e.yml",
        ]
        .map(str::to_owned),
    );
    let mut seen = std::collections::BTreeSet::new();
    paths.retain(|path| seen.insert(path.clone()));
    paths
}

pub(super) fn frontend_workspace_relative_paths(
    package_manager: &str,
    frontends: &[FrontendScaffold],
) -> Vec<PathBuf> {
    frontend_workspace_template_files(package_manager, frontends)
        .into_iter()
        .map(|file| PathBuf::from(file.output))
        .collect()
}

fn frontend_workspace_template_files(
    package_manager: &str,
    frontends: &[FrontendScaffold],
) -> Vec<ScaffoldTemplateFile> {
    if frontends.is_empty() {
        return Vec::new();
    }
    let has_spa = frontends
        .iter()
        .any(|frontend| frontend.kind == ScaffoldFrontendKind::Spa);
    let has_admin = frontends
        .iter()
        .any(|frontend| frontend.kind == ScaffoldFrontendKind::Admin);
    let has_react = frontends.iter().any(|frontend| {
        matches!(
            frontend.kind,
            ScaffoldFrontendKind::Spa | ScaffoldFrontendKind::Admin
        )
    });
    FRONTEND_WORKSPACE_TEMPLATES
        .iter()
        .copied()
        .chain(has_react.then_some(REACT_ESLINT_TEMPLATE))
        .chain((package_manager == "pnpm").then_some(PNPM_WORKSPACE_TEMPLATE))
        .chain((package_manager == "yarn").then_some(YARN_WORKSPACE_TEMPLATE))
        .chain(has_spa.then_some(E2E_WORKFLOW_TEMPLATE))
        .chain(PUBLIC_API_CLIENT_TEMPLATES.iter().copied())
        .chain(
            has_admin
                .then_some(ADMIN_API_CLIENT_TEMPLATES)
                .into_iter()
                .flatten()
                .copied(),
        )
        .collect()
}

fn optional_cargo_command(command: &str, action: &str) -> String {
    format!(
        "if [ -f Cargo.toml ]; then {command}; else printf '%s\\n' 'No Cargo.toml found; skipping cargo {action}.'; fi"
    )
}

fn title_case(value: &str) -> String {
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

fn repo_root_relative(frontend_dir: &str) -> String {
    let depth = PathBuf::from(frontend_dir).components().count();
    (0..depth).map(|_| "..").collect::<Vec<_>>().join("/")
}

fn e2e_database_name(module_name: &str, frontend_package_name: &str) -> String {
    let name = format!(
        "{module_name}_{}_e2e",
        frontend_package_name.replace('-', "_")
    );
    bounded_postgres_identifier(&name)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::fs;
    use std::process::Command;

    use super::*;
    use tempfile::tempdir;

    fn embedded_paths(prefix: &str) -> BTreeSet<&'static str> {
        EMBEDDED_SCAFFOLD_TEMPLATE_FILES
            .iter()
            .filter_map(|file| {
                file.relative_path
                    .starts_with(prefix)
                    .then_some(file.relative_path)
            })
            .collect()
    }

    fn registered_paths(
        prefix: &str,
        registries: &[&[ScaffoldTemplateFile]],
    ) -> BTreeSet<&'static str> {
        registries
            .iter()
            .flat_map(|files| files.iter())
            .filter_map(|file| file.template.starts_with(prefix).then_some(file.template))
            .collect()
    }

    fn assert_complete(prefix: &str, registries: &[&[ScaffoldTemplateFile]]) {
        let embedded = embedded_paths(prefix);
        let registered = registered_paths(prefix, registries);
        let missing = embedded
            .difference(&registered)
            .copied()
            .collect::<Vec<_>>();
        let unexpected = registered
            .difference(&embedded)
            .copied()
            .collect::<Vec<_>>();
        assert!(
            missing.is_empty() && unexpected.is_empty(),
            "frontend template registry mismatch under {prefix}: missing {missing:?}; unexpected {unexpected:?}"
        );
    }

    #[test]
    fn embedded_frontend_templates_are_registered() {
        let pnpm = std::slice::from_ref(&PNPM_WORKSPACE_TEMPLATE);
        let yarn = std::slice::from_ref(&YARN_WORKSPACE_TEMPLATE);
        let e2e = std::slice::from_ref(&E2E_WORKFLOW_TEMPLATE);
        let react_eslint = std::slice::from_ref(&REACT_ESLINT_TEMPLATE);
        let admin = admin_template_files();

        assert_complete("rust-react/frontend/vite-react/", &[VITE_REACT_TEMPLATES]);
        assert_complete("rust-react/frontend/astro/", &[ASTRO_TEMPLATES]);
        assert_complete(
            "rust-react/frontend/workspace/",
            &[FRONTEND_WORKSPACE_TEMPLATES, pnpm, yarn, e2e, react_eslint],
        );
        assert_complete("rust-react/frontend/admin-shadcn/", &[admin.as_slice()]);
        assert_complete(
            "rust-react/frontend/api-client-public/",
            &[PUBLIC_API_CLIENT_TEMPLATES],
        );
        assert_complete(
            "rust-react/frontend/api-client-admin/",
            &[ADMIN_API_CLIENT_TEMPLATES],
        );
        assert_complete(
            "rust-react/frontend/",
            &[
                VITE_REACT_TEMPLATES,
                SPA_SHADCN_TEMPLATES,
                ASTRO_TEMPLATES,
                FRONTEND_WORKSPACE_TEMPLATES,
                pnpm,
                yarn,
                e2e,
                react_eslint,
                admin.as_slice(),
                PUBLIC_API_CLIENT_TEMPLATES,
                ADMIN_API_CLIENT_TEMPLATES,
            ],
        );
    }

    #[test]
    fn frontend_workspace_declared_paths_match_rendered_outputs_for_all_shapes() {
        let spa = FrontendScaffold::from_spec(ScaffoldFrontend {
            name: "web".into(),
            kind: ScaffoldFrontendKind::Spa,
            custom_default_name: false,
        })
        .unwrap();
        let astro = FrontendScaffold::from_spec(ScaffoldFrontend {
            name: "landing".into(),
            kind: ScaffoldFrontendKind::Astro,
            custom_default_name: false,
        })
        .unwrap();

        for package_manager in ["bun", "npm", "pnpm", "yarn"] {
            for frontends in [
                Vec::new(),
                vec![spa.clone()],
                vec![astro.clone()],
                vec![spa.clone(), astro.clone()],
            ] {
                let declared = frontend_workspace_relative_paths(package_manager, &frontends);
                let rendered = render_frontend_workspace_files(
                    package_manager,
                    "demo",
                    FrontendDatabaseContext {
                        db: ScaffoldDb::None,
                        migration_dir: "migrations",
                        sqlx_metadata_dir: ".sqlx",
                    },
                    "main",
                    "ubuntu-latest",
                    &frontends,
                )
                .unwrap()
                .into_iter()
                .map(|file| PathBuf::from(file.relative))
                .collect::<Vec<_>>();

                assert_eq!(declared, rendered, "{package_manager}: {frontends:?}");
            }
        }
    }

    #[test]
    fn e2e_database_names_are_frontend_specific_and_postgres_safe() {
        assert_eq!(e2e_database_name("demo", "web"), "demo_web_e2e");
        assert_eq!(
            e2e_database_name("demo", "customer-portal"),
            "demo_customer_portal_e2e"
        );

        let first = e2e_database_name(&"module".repeat(12), "frontend-one");
        let second = e2e_database_name(&"module".repeat(12), "frontend-two");
        assert_eq!(first.len(), 63);
        assert_eq!(second.len(), 63);
        assert_ne!(first, second);
    }

    #[test]
    fn e2e_workflow_paths_have_one_role_and_database_aware_authority() {
        let spa = FrontendScaffold::from_spec(ScaffoldFrontend {
            name: "web".into(),
            kind: ScaffoldFrontendKind::Spa,
            custom_default_name: false,
        })
        .unwrap();
        let admin = FrontendScaffold::from_spec(ScaffoldFrontend {
            name: "admin".into(),
            kind: ScaffoldFrontendKind::Admin,
            custom_default_name: false,
        })
        .unwrap();
        let astro = FrontendScaffold::from_spec(ScaffoldFrontend {
            name: "landing".into(),
            kind: ScaffoldFrontendKind::Astro,
            custom_default_name: false,
        })
        .unwrap();

        let no_database = e2e_workflow_paths(
            ScaffoldDb::None,
            "migrations",
            ".sqlx",
            &[spa.clone(), admin.clone(), astro.clone()],
        );
        assert!(no_database.iter().any(|path| path == "web/**"));
        assert!(!no_database.iter().any(|path| path == "admin/**"));
        assert!(!no_database.iter().any(|path| path == "landing/**"));
        assert!(!no_database.iter().any(|path| path == "migrations/**"));
        assert!(!no_database.iter().any(|path| path == ".sqlx/**"));
        assert!(no_database.iter().any(|path| path == "rust-toolchain"));
        assert!(no_database.iter().any(|path| path == "rust-toolchain.toml"));
        assert!(no_database.iter().any(|path| path == "npm-shrinkwrap.json"));

        let sqlite = e2e_workflow_paths(
            ScaffoldDb::Sqlite,
            "db/migrations",
            "db/sqlx",
            &[spa, admin, astro],
        );
        assert!(sqlite.iter().any(|path| path == "web/**"));
        assert!(sqlite.iter().any(|path| path == "db/migrations/**"));
        assert!(sqlite.iter().any(|path| path == "db/sqlx/**"));
        assert_eq!(
            sqlite
                .iter()
                .collect::<std::collections::BTreeSet<_>>()
                .len(),
            sqlite.len(),
            "E2E path authority must not contain duplicate entries"
        );

        let overlapping_migration = e2e_workflow_paths(ScaffoldDb::Postgres, "apps", ".sqlx", &[]);
        assert_eq!(
            overlapping_migration
                .iter()
                .filter(|path| path.as_str() == "apps/**")
                .count(),
            1
        );
    }

    #[test]
    fn database_config_guard_requires_exported_url_or_dotenv_assignment() {
        let temp = tempdir().unwrap();
        let exported = Command::new("sh")
            .args(["-c", DATABASE_CONFIG_GUARD])
            .env("DATABASE_URL", "sqlite:demo.db")
            .current_dir(temp.path())
            .output()
            .unwrap();
        assert!(exported.status.success());

        let missing = Command::new("sh")
            .args(["-c", DATABASE_CONFIG_GUARD])
            .env_remove("DATABASE_URL")
            .current_dir(temp.path())
            .output()
            .unwrap();
        assert!(!missing.status.success());
        assert!(String::from_utf8_lossy(&missing.stderr).contains("Missing DATABASE_URL"));

        for invalid in [
            "",
            "# DATABASE_URL=sqlite:commented.db\n",
            "OTHER_SETTING=true\n",
            "DATABASE_URL=\n",
            "DATABASE_URL=   # still empty\n",
            "DATABASE_URL=\"\"\n",
            "export DATABASE_URL=''\n",
        ] {
            fs::write(temp.path().join(".env"), invalid).unwrap();
            let output = Command::new("sh")
                .args(["-c", DATABASE_CONFIG_GUARD])
                .env_remove("DATABASE_URL")
                .current_dir(temp.path())
                .output()
                .unwrap();
            assert!(
                !output.status.success(),
                "guard accepted invalid dotenv contents {invalid:?}"
            );
        }

        for valid in [
            "DATABASE_URL=sqlite:demo.db\n",
            " export DATABASE_URL = postgres://localhost/demo\n",
            "OTHER_SETTING=true\nDATABASE_URL='sqlite:quoted.db'\n",
        ] {
            fs::write(temp.path().join(".env"), valid).unwrap();
            let output = Command::new("sh")
                .args(["-c", DATABASE_CONFIG_GUARD])
                .env_remove("DATABASE_URL")
                .current_dir(temp.path())
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "guard rejected valid dotenv contents {valid:?}: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
    }
}
