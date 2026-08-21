use std::path::PathBuf;

use anyhow::Result;
use serde_json::json;

use super::frontend::DATABASE_CONFIG_GUARD;
use super::names::bounded_postgres_identifier;
use super::templates::{
    ScaffoldTemplateFile, ensure_scaffold_template_paths, render_scaffold_template,
};
use super::write::{ScaffoldFile, scaffold_file};
use super::{InitScaffoldPlan, ScaffoldDb};

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
    pub(super) fn render_go_workspace_files(&self) -> Result<Vec<ScaffoldFile>> {
        ensure_scaffold_template_paths(GO_WORKSPACE_TEMPLATES)?;
        if self.db == ScaffoldDb::Postgres {
            ensure_scaffold_template_paths(GO_POSTGRES_TEMPLATES)?;
        }
        let context = self.go_workspace_template_context();
        self.go_workspace_template_files()
            .map(|file| {
                Ok(scaffold_file(
                    file.output,
                    render_scaffold_template(file.template, &context)?,
                ))
            })
            .collect()
    }

    pub(super) fn go_workspace_relative_paths(&self) -> Vec<PathBuf> {
        self.go_workspace_template_files()
            .map(|file| PathBuf::from(file.output))
            .collect()
    }

    pub(super) fn go_scaffold_bootstrap_command(&self) -> String {
        let mut commands = vec!["go mod tidy".to_string()];
        if !self.frontends.is_empty() {
            commands.push("scripts/check-webapps.sh bootstrap".into());
        }
        if self.db == ScaffoldDb::Postgres {
            commands.push(DATABASE_CONFIG_GUARD.into());
            commands.push("go tool sqlc generate".into());
            commands.push("go run ./cmd/api --bootstrap-database".into());
        }
        if !self.frontends.is_empty() {
            commands.push("node scripts/contracts.mjs generate".into());
        }
        commands.join(" && ")
    }

    fn go_workspace_template_files(&self) -> impl Iterator<Item = &'static ScaffoldTemplateFile> {
        let postgres_templates = if self.db == ScaffoldDb::Postgres {
            GO_POSTGRES_TEMPLATES
        } else {
            &[]
        };
        GO_WORKSPACE_TEMPLATES.iter().chain(postgres_templates)
    }

    fn go_workspace_template_context(&self) -> serde_json::Value {
        let database_name = bounded_postgres_identifier(&format!("{}_dev", self.module_name));
        let postgres_test_database_name =
            bounded_postgres_identifier(&format!("test_{}", self.module_name));
        json!({
            "repo_name": self.repo_name,
            "package_name": self.package_name,
            "go_module": self.go_module,
            "db_enabled": self.db == ScaffoldDb::Postgres,
            "database_url_example": format!(
                "postgres://postgres:postgres@localhost:5432/{database_name}?sslmode=disable"
            ),
            "postgres_test_database_name": postgres_test_database_name,
        })
    }
}
