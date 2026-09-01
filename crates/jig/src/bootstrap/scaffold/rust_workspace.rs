use std::path::PathBuf;

use anyhow::Result;
use serde_json::{Value, json};

use super::names::bounded_postgres_identifier;
use super::templates::{
    ScaffoldTemplateFile, ensure_scaffold_template_paths, render_scaffold_template,
};
use super::write::{ScaffoldFile, scaffold_file};
use super::{InitScaffoldPlan, RustScaffoldPlan, ScaffoldDb};

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
        template: "rust-common/workspace/clippy.toml.jinja",
        output: "clippy.toml",
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
        template: "rust-react/workspace/crates/http/src/public.rs.jinja",
        output: "crates/{package}-http/src/public.rs",
    },
    ScaffoldTemplateFile {
        template: "rust-react/workspace/crates/http-common/Cargo.toml.jinja",
        output: "crates/{package}-http-common/Cargo.toml",
    },
    ScaffoldTemplateFile {
        template: "rust-react/workspace/crates/http-common/AGENTS.md.jinja",
        output: "crates/{package}-http-common/AGENTS.md",
    },
    ScaffoldTemplateFile {
        template: "rust-react/workspace/crates/http-common/src/lib.rs.jinja",
        output: "crates/{package}-http-common/src/lib.rs",
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
        template: "rust-react/workspace/apps/api/src/bin/export-openapi.rs.jinja",
        output: "apps/{package}-api/src/bin/export-openapi.rs",
    },
    ScaffoldTemplateFile {
        template: "rust-react/workspace/openapi/public.json.jinja",
        output: "openapi/public.json",
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

const RUST_ADMIN_API_TEMPLATES: &[ScaffoldTemplateFile] = &[
    ScaffoldTemplateFile {
        template: "rust-react/workspace/crates/admin-http/Cargo.toml.jinja",
        output: "crates/{package}-admin-http/Cargo.toml",
    },
    ScaffoldTemplateFile {
        template: "rust-react/workspace/crates/admin-http/AGENTS.md.jinja",
        output: "crates/{package}-admin-http/AGENTS.md",
    },
    ScaffoldTemplateFile {
        template: "rust-react/workspace/crates/admin-http/src/lib.rs.jinja",
        output: "crates/{package}-admin-http/src/lib.rs",
    },
    ScaffoldTemplateFile {
        template: "rust-react/workspace/apps/admin-api/Cargo.toml.jinja",
        output: "apps/{package}-admin-api/Cargo.toml",
    },
    ScaffoldTemplateFile {
        template: "rust-react/workspace/apps/admin-api/src/main.rs.jinja",
        output: "apps/{package}-admin-api/src/main.rs",
    },
    ScaffoldTemplateFile {
        template: "rust-react/workspace/apps/admin-api/src/bin/export-openapi.rs.jinja",
        output: "apps/{package}-admin-api/src/bin/export-openapi.rs",
    },
    ScaffoldTemplateFile {
        template: "rust-react/workspace/openapi/admin.json.jinja",
        output: "openapi/admin.json",
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

const RUST_POSTGRES_TEMPLATES: &[ScaffoldTemplateFile] = &[
    ScaffoldTemplateFile {
        template: "rust-react/workspace/scripts/test-postgres.sh.jinja",
        output: "scripts/test-postgres.sh",
    },
    ScaffoldTemplateFile {
        template: "rust-react/workspace/crates/test-support/tests/postgres.rs.jinja",
        output: "crates/{package}-test-support/tests/postgres.rs",
    },
];

// The rust-react preset currently places the db crate at crates/<name>-db.
const DB_CRATE_TO_REPO_ROOT: &str = "../..";

impl InitScaffoldPlan {
    pub(super) fn render_rust_workspace_files(
        &self,
        backend: &RustScaffoldPlan,
    ) -> Result<Vec<ScaffoldFile>> {
        ensure_scaffold_template_paths(RUST_WORKSPACE_TEMPLATES)?;
        if backend.database != ScaffoldDb::None {
            ensure_scaffold_template_paths(RUST_DB_TEMPLATES)?;
        }
        if backend.database == ScaffoldDb::Postgres {
            ensure_scaffold_template_paths(RUST_POSTGRES_TEMPLATES)?;
        }
        if self.has_admin_frontend() {
            ensure_scaffold_template_paths(RUST_ADMIN_API_TEMPLATES)?;
        }
        let context = self.rust_workspace_template_context(backend);
        let mut files = self
            .rust_workspace_template_files(backend)
            .map(|file| {
                Ok(scaffold_file(
                    self.template_output_path(file),
                    render_scaffold_template(file.template, &context)?,
                ))
            })
            .collect::<Result<Vec<_>>>()?;
        if backend.database != ScaffoldDb::None {
            files.push(scaffold_file(
                format!("{}/.gitkeep", backend.migration_dir),
                String::new(),
            ));
        }
        Ok(files)
    }

    pub(super) fn rust_workspace_relative_paths(&self, backend: &RustScaffoldPlan) -> Vec<PathBuf> {
        let mut paths = self
            .rust_workspace_template_files(backend)
            .map(|file| PathBuf::from(self.template_output_path(file)))
            .collect::<Vec<_>>();
        if backend.database != ScaffoldDb::None {
            paths.push(PathBuf::from(format!("{}/.gitkeep", backend.migration_dir)));
        }
        paths
    }

    fn rust_workspace_template_files(
        &self,
        backend: &RustScaffoldPlan,
    ) -> impl Iterator<Item = &'static ScaffoldTemplateFile> {
        let db_templates = if backend.database != ScaffoldDb::None {
            RUST_DB_TEMPLATES
        } else {
            &[]
        };
        let admin_templates = if self.has_admin_frontend() {
            RUST_ADMIN_API_TEMPLATES
        } else {
            &[]
        };
        let postgres_templates = if backend.database == ScaffoldDb::Postgres {
            RUST_POSTGRES_TEMPLATES
        } else {
            &[]
        };
        RUST_WORKSPACE_TEMPLATES
            .iter()
            .chain(admin_templates)
            .chain(db_templates)
            .chain(postgres_templates)
    }

    pub(super) fn template_output_path(&self, file: &ScaffoldTemplateFile) -> String {
        file.output.replace("{package}", &self.package_name)
    }

    fn rust_workspace_template_context(&self, backend: &RustScaffoldPlan) -> Value {
        let database_url_example = match backend.database {
            ScaffoldDb::None => String::new(),
            ScaffoldDb::Postgres => {
                let database_name =
                    bounded_postgres_identifier(&format!("{}_dev", self.module_name));
                format!("postgres://postgres:postgres@localhost:5432/{database_name}")
            }
            ScaffoldDb::Sqlite => format!("sqlite:{}.db", self.module_name),
        };
        let postgres_test_database_name =
            bounded_postgres_identifier(&format!("test_db_{}", self.module_name));

        json!({
            "package_name": self.package_name,
            "module_name": self.module_name,
            "repo_name": self.repo_name,
            "db_enabled": backend.database != ScaffoldDb::None,
            "sqlx_driver": match backend.database {
                ScaffoldDb::None => "",
                ScaffoldDb::Postgres => "postgres",
                ScaffoldDb::Sqlite => "sqlite",
            },
            "db_pool": match backend.database {
                ScaffoldDb::None => "",
                ScaffoldDb::Postgres => "PgPool",
                ScaffoldDb::Sqlite => "SqlitePool",
            },
            "db_database": match backend.database {
                ScaffoldDb::None => "",
                ScaffoldDb::Postgres => "Postgres",
                ScaffoldDb::Sqlite => "Sqlite",
            },
            "migration_path": format!("{DB_CRATE_TO_REPO_ROOT}/{}", backend.migration_dir),
            "database_url_example": database_url_example,
            "postgres_test_database_name": postgres_test_database_name,
            "admin_api_enabled": self.has_admin_frontend(),
        })
    }

    pub(super) fn has_admin_frontend(&self) -> bool {
        self.frontends()
            .iter()
            .any(|frontend| frontend.kind == super::ScaffoldFrontendKind::Admin)
    }
}
