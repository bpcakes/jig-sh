use std::path::PathBuf;

use anyhow::Result;
use serde_json::json;

use super::frontend::DATABASE_CONFIG_GUARD;
use super::names::bounded_postgres_identifier;
use super::templates::{
    ScaffoldTemplateFile, ensure_scaffold_template_paths, render_scaffold_template,
};
use super::write::{ScaffoldFile, scaffold_file};
use super::{GoScaffoldPlan, InitScaffoldPlan, go_component_path};

const GO_WORKSPACE_TEMPLATES: &[ScaffoldTemplateFile] = &[
    ScaffoldTemplateFile {
        template: "go-react/workspace/.env.example.jinja",
        output: ".env.example",
    },
    ScaffoldTemplateFile {
        template: "go-react/workspace/go.mod.jinja",
        output: "go.mod",
    },
    ScaffoldTemplateFile {
        template: "go-react/workspace/cmd/api/main.go.jinja",
        output: "cmd/api/main.go",
    },
    ScaffoldTemplateFile {
        template: "go-react/workspace/cmd/api/main_test.go.jinja",
        output: "cmd/api/main_test.go",
    },
    ScaffoldTemplateFile {
        template: "go-react/workspace/cmd/openapi/main.go.jinja",
        output: "cmd/openapi/main.go",
    },
    ScaffoldTemplateFile {
        template: "go-react/workspace/internal/config/config.go.jinja",
        output: "internal/config/config.go",
    },
    ScaffoldTemplateFile {
        template: "go-react/workspace/internal/config/config_test.go.jinja",
        output: "internal/config/config_test.go",
    },
    ScaffoldTemplateFile {
        template: "go-react/workspace/internal/httpapi/httpapi.go.jinja",
        output: "internal/httpapi/httpapi.go",
    },
    ScaffoldTemplateFile {
        template: "go-react/workspace/internal/httpapi/httpapi_test.go.jinja",
        output: "internal/httpapi/httpapi_test.go",
    },
    ScaffoldTemplateFile {
        template: "go-react/workspace/openapi/public.json.jinja",
        output: "openapi/public.json",
    },
];

const GO_POSTGRES_TEMPLATES: &[ScaffoldTemplateFile] = &[
    ScaffoldTemplateFile {
        template: "go-react/workspace/cmd/api/database_command.go.jinja",
        output: "cmd/api/database_command.go",
    },
    ScaffoldTemplateFile {
        template: "go-react/workspace/sqlc.yaml.jinja",
        output: "sqlc.yaml",
    },
    ScaffoldTemplateFile {
        template: "go-react/workspace/internal/database/database.go.jinja",
        output: "internal/database/database.go",
    },
    ScaffoldTemplateFile {
        template: "go-react/workspace/internal/database/database_test.go.jinja",
        output: "internal/database/database_test.go",
    },
    ScaffoldTemplateFile {
        template: "go-react/workspace/internal/database/queries/metadata.sql.jinja",
        output: "internal/database/queries/metadata.sql",
    },
    ScaffoldTemplateFile {
        template: "go-react/workspace/internal/database/sqlc/db.go.jinja",
        output: "internal/database/sqlc/db.go",
    },
    ScaffoldTemplateFile {
        template: "go-react/workspace/internal/database/sqlc/metadata.sql.go.jinja",
        output: "internal/database/sqlc/metadata.sql.go",
    },
    ScaffoldTemplateFile {
        template: "go-react/workspace/internal/database/sqlc/models.go.jinja",
        output: "internal/database/sqlc/models.go",
    },
    ScaffoldTemplateFile {
        template: "go-react/workspace/internal/database/migrations/00001_app_metadata.sql.jinja",
        output: "internal/database/migrations/00001_app_metadata.sql",
    },
    ScaffoldTemplateFile {
        template: "go-react/workspace/scripts/test-postgres.sh.jinja",
        output: "scripts/test-postgres.sh",
    },
];

impl InitScaffoldPlan {
    pub(super) fn render_go_workspace_files(
        &self,
        backend: &GoScaffoldPlan,
    ) -> Result<Vec<ScaffoldFile>> {
        ensure_scaffold_template_paths(GO_WORKSPACE_TEMPLATES)?;
        if backend.database.is_postgres() {
            ensure_scaffold_template_paths(GO_POSTGRES_TEMPLATES)?;
        }
        let context = self.go_workspace_template_context(backend);
        self.go_workspace_template_files(backend)
            .map(|file| {
                Ok(scaffold_file(
                    go_workspace_output_path(backend, file.output),
                    render_scaffold_template(file.template, &context)?,
                ))
            })
            .collect()
    }

    pub(super) fn go_workspace_relative_paths(&self, backend: &GoScaffoldPlan) -> Vec<PathBuf> {
        self.go_workspace_template_files(backend)
            .map(|file| PathBuf::from(go_workspace_output_path(backend, file.output)))
            .collect()
    }

    pub(super) fn go_scaffold_bootstrap_command(&self, backend: &GoScaffoldPlan) -> String {
        let in_component = |command: &str| {
            if backend.component_root == "." {
                command.to_owned()
            } else {
                format!(
                    "(cd {} && {command})",
                    crate::shell::quote(&backend.component_root)
                )
            }
        };
        let mut commands = vec![in_component("go mod tidy")];
        if !self.frontends.is_empty() {
            commands.push("scripts/check-webapps.sh bootstrap".into());
        }
        if backend.database.is_postgres() {
            commands.push(in_component(DATABASE_CONFIG_GUARD));
            commands.push(in_component(
                "go tool sqlc generate && go run ./cmd/api --bootstrap-database",
            ));
        }
        if !self.frontends.is_empty() {
            commands.push("node scripts/contracts.mjs generate".into());
        }
        commands.join(" && ")
    }

    fn go_workspace_template_files(
        &self,
        backend: &GoScaffoldPlan,
    ) -> impl Iterator<Item = &'static ScaffoldTemplateFile> {
        let postgres_templates = if backend.database.is_postgres() {
            GO_POSTGRES_TEMPLATES
        } else {
            &[]
        };
        GO_WORKSPACE_TEMPLATES.iter().chain(postgres_templates)
    }

    fn go_workspace_template_context(&self, backend: &GoScaffoldPlan) -> serde_json::Value {
        let database_name = bounded_postgres_identifier(&format!("{}_dev", self.module_name));
        let postgres_test_database_name =
            bounded_postgres_identifier(&format!("test_{}", self.module_name));
        json!({
            "repo_name": self.repo_name,
            "package_name": self.package_name,
            "go_module": backend.module,
            "go_backend_root": backend.component_root,
            "go_public_openapi_test_path": go_public_openapi_test_path(&backend.component_root),
            "db_enabled": backend.database.is_postgres(),
            "database_url_example": format!(
                "postgres://postgres:postgres@localhost:5432/{database_name}?sslmode=disable"
            ),
            "postgres_test_database_name": postgres_test_database_name,
        })
    }
}

fn go_public_openapi_test_path(component_root: &str) -> String {
    let component_depth = if component_root == "." {
        0
    } else {
        component_root.split('/').count()
    };
    let mut segments = vec![".."; component_depth + 2];
    segments.extend(["openapi", "public.json"]);
    segments.join("/")
}

fn go_workspace_output_path(backend: &GoScaffoldPlan, output: &str) -> String {
    // OpenAPI documents and the disposable-database runner are repository-level
    // integration artifacts. The remaining files are owned by the Go component
    // and must follow its authored root.
    match output {
        "openapi/public.json" | "scripts/test-postgres.sh" => output.to_owned(),
        _ => go_component_path(&backend.component_root, output),
    }
}
