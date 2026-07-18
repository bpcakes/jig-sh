use std::path::PathBuf;

use anyhow::Result;
use serde_json::{Value, json};

use super::names::bounded_postgres_identifier;
use super::templates::{
    ScaffoldTemplateFile, ensure_scaffold_template_paths, render_scaffold_template,
};
use super::write::{ScaffoldFile, scaffold_file};
use super::{InitScaffoldPlan, ScaffoldDb};

const RUST_WORKSPACE_TEMPLATES: &[ScaffoldTemplateFile] = &[
    ScaffoldTemplateFile {
        template: "rust-react/workspace/.env.example.jinja",
        output: ".env.example",
    },
    ScaffoldTemplateFile {
        template: "rust-react/workspace/Cargo.toml.jinja",
        output: "Cargo.toml",
    },
    ScaffoldTemplateFile {
        template: "rust-react/workspace/crates/core/Cargo.toml.jinja",
        output: "crates/{package}-core/Cargo.toml",
    },
    ScaffoldTemplateFile {
        template: "rust-react/workspace/crates/core/src/lib.rs.jinja",
        output: "crates/{package}-core/src/lib.rs",
    },
    ScaffoldTemplateFile {
        template: "rust-react/workspace/crates/app/Cargo.toml.jinja",
        output: "crates/{package}/Cargo.toml",
    },
    ScaffoldTemplateFile {
        template: "rust-react/workspace/crates/app/AGENTS.md.jinja",
        output: "crates/{package}/AGENTS.md",
    },
    ScaffoldTemplateFile {
        template: "rust-react/workspace/crates/app/src/lib.rs.jinja",
        output: "crates/{package}/src/lib.rs",
    },
    ScaffoldTemplateFile {
        template: "rust-react/workspace/crates/http/Cargo.toml.jinja",
        output: "crates/{package}-http/Cargo.toml",
    },
    ScaffoldTemplateFile {
        template: "rust-react/workspace/crates/http/AGENTS.md.jinja",
        output: "crates/{package}-http/AGENTS.md",
    },
    ScaffoldTemplateFile {
        template: "rust-react/workspace/crates/http/src/lib.rs.jinja",
        output: "crates/{package}-http/src/lib.rs",
    },
    ScaffoldTemplateFile {
        template: "rust-react/workspace/apps/api/Cargo.toml.jinja",
        output: "apps/{package}-api/Cargo.toml",
    },
    ScaffoldTemplateFile {
        template: "rust-react/workspace/apps/api/src/main.rs.jinja",
        output: "apps/{package}-api/src/main.rs",
    },
    ScaffoldTemplateFile {
        template: "rust-react/workspace/crates/test-support/Cargo.toml.jinja",
        output: "crates/{package}-test-support/Cargo.toml",
    },
    ScaffoldTemplateFile {
        template: "rust-react/workspace/crates/test-support/AGENTS.md.jinja",
        output: "crates/{package}-test-support/AGENTS.md",
    },
    ScaffoldTemplateFile {
        template: "rust-react/workspace/crates/test-support/src/lib.rs.jinja",
        output: "crates/{package}-test-support/src/lib.rs",
    },
    ScaffoldTemplateFile {
        template: "rust-react/workspace/crates/test-support/src/app.rs.jinja",
        output: "crates/{package}-test-support/src/app.rs",
    },
    ScaffoldTemplateFile {
        template: "rust-react/workspace/crates/test-support/src/http.rs.jinja",
        output: "crates/{package}-test-support/src/http.rs",
    },
    ScaffoldTemplateFile {
        template: "rust-react/workspace/crates/test-support/src/responses.rs.jinja",
        output: "crates/{package}-test-support/src/responses.rs",
    },
    ScaffoldTemplateFile {
        template: "rust-react/workspace/crates/test-support/tests/http.rs.jinja",
        output: "crates/{package}-test-support/tests/http.rs",
    },
];

const RUST_DB_TEMPLATES: &[ScaffoldTemplateFile] = &[
    ScaffoldTemplateFile {
        template: "rust-react/workspace/crates/db/Cargo.toml.jinja",
        output: "crates/{package}-db/Cargo.toml",
    },
    ScaffoldTemplateFile {
        template: "rust-react/workspace/crates/db/AGENTS.md.jinja",
        output: "crates/{package}-db/AGENTS.md",
    },
    ScaffoldTemplateFile {
        template: "rust-react/workspace/crates/db/src/lib.rs.jinja",
        output: "crates/{package}-db/src/lib.rs",
    },
    ScaffoldTemplateFile {
        template: "rust-react/workspace/crates/test-support/src/db.rs.jinja",
        output: "crates/{package}-test-support/src/db.rs",
    },
];

// The rust-react preset currently places the db crate at crates/<name>-db.
const DB_CRATE_TO_REPO_ROOT: &str = "../..";

impl InitScaffoldPlan {
    pub(super) fn render_rust_workspace_files(&self) -> Result<Vec<ScaffoldFile>> {
        ensure_scaffold_template_paths(RUST_WORKSPACE_TEMPLATES)?;
        if self.db != ScaffoldDb::None {
            ensure_scaffold_template_paths(RUST_DB_TEMPLATES)?;
        }
        let context = self.rust_workspace_template_context();
        let mut files = self
            .rust_workspace_template_files()
            .map(|file| {
                Ok(scaffold_file(
                    self.template_output_path(file),
                    render_scaffold_template(file.template, &context)?,
                ))
            })
            .collect::<Result<Vec<_>>>()?;
        if self.db != ScaffoldDb::None {
            files.push(scaffold_file(
                format!("{}/.gitkeep", self.migration_dir),
                String::new(),
            ));
        }
        Ok(files)
    }

    pub(super) fn rust_workspace_relative_paths(&self) -> Vec<PathBuf> {
        let mut paths = self
            .rust_workspace_template_files()
            .map(|file| PathBuf::from(self.template_output_path(file)))
            .collect::<Vec<_>>();
        if self.db != ScaffoldDb::None {
            paths.push(PathBuf::from(format!("{}/.gitkeep", self.migration_dir)));
        }
        paths
    }

    fn rust_workspace_template_files(&self) -> impl Iterator<Item = &'static ScaffoldTemplateFile> {
        let db_templates = if self.db != ScaffoldDb::None {
            RUST_DB_TEMPLATES
        } else {
            &[]
        };
        RUST_WORKSPACE_TEMPLATES.iter().chain(db_templates)
    }

    fn template_output_path(&self, file: &ScaffoldTemplateFile) -> String {
        file.output.replace("{package}", &self.package_name)
    }

    fn rust_workspace_template_context(&self) -> Value {
        let database_url_example = match self.db {
            ScaffoldDb::None => String::new(),
            ScaffoldDb::Postgres => {
                let database_name =
                    bounded_postgres_identifier(&format!("{}_dev", self.module_name));
                format!("postgres://postgres:postgres@localhost:5432/{database_name}")
            }
            ScaffoldDb::Sqlite => format!("sqlite:{}.db", self.module_name),
        };

        json!({
            "package_name": self.package_name,
            "module_name": self.module_name,
            "repo_name": self.repo_name,
            "db_enabled": self.db != ScaffoldDb::None,
            "sqlx_driver": match self.db {
                ScaffoldDb::None => "",
                ScaffoldDb::Postgres => "postgres",
                ScaffoldDb::Sqlite => "sqlite",
            },
            "db_pool": match self.db {
                ScaffoldDb::None => "",
                ScaffoldDb::Postgres => "PgPool",
                ScaffoldDb::Sqlite => "SqlitePool",
            },
            "db_database": match self.db {
                ScaffoldDb::None => "",
                ScaffoldDb::Postgres => "Postgres",
                ScaffoldDb::Sqlite => "Sqlite",
            },
            "migration_path": format!("{DB_CRATE_TO_REPO_ROOT}/{}", self.migration_dir),
            "database_url_example": database_url_example,
        })
    }
}
