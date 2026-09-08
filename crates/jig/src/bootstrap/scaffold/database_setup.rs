use anyhow::Result;
use serde_json::json;

use super::ScaffoldPreset;
use super::frontend::FrontendBackendContext;
use super::templates::render_scaffold_template;
use super::write::{ScaffoldFile, scaffold_file};

pub(super) const DATABASE_CONFIG_GUARD: &str = r#"if [ -z "${DATABASE_URL:-}" ] && ! awk '/^[[:space:]]*(#|$)/ { next } /^[[:space:]]*(export[[:space:]]+)?DATABASE_URL[[:space:]]*=/ { value = $0; sub(/^[^=]*=[[:space:]]*/, "", value); sub(/^#.*$/, "", value); sub(/[[:space:]]+#.*$/, "", value); gsub(/^[[:space:]]+|[[:space:]]+$/, "", value); single_quote = sprintf("%c", 39); if (value != "" && value != "\"\"" && value != single_quote single_quote) found = 1 } END { exit found ? 0 : 1 }' .env 2>/dev/null; then printf '%s\n' 'Missing DATABASE_URL; export it or copy .env.example to .env before database setup.' >&2; exit 1; fi"#;

pub(super) fn render(
    backend: FrontendBackendContext<'_>,
    package_name: &str,
) -> Result<ScaffoldFile> {
    let command = match backend.preset {
        ScaffoldPreset::GoReact => {
            "go tool sqlc generate\nexec go run ./cmd/api --bootstrap-database".to_string()
        }
        _ => format!("exec cargo run -p {package_name}-api -- --bootstrap-database"),
    };
    Ok(scaffold_file(
        "scripts/setup-database.sh",
        render_scaffold_template(
            "database/setup.sh.jinja",
            &json!({
                "backend_root": crate::shell::quote(backend.root),
                "database_config_guard": DATABASE_CONFIG_GUARD,
                "database_setup_command": command,
            }),
        )?,
    ))
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::bootstrap::scaffold::ScaffoldDb;
    use crate::bootstrap::scaffold::frontend::FrontendDatabaseContext;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::process::Command;

    #[test]
    fn go_database_setup_uses_backend_dotenv_and_preserves_failure_status() {
        let temp = tempfile::tempdir().unwrap();
        let repo = temp.path().join("ExampleProject");
        let backend = repo.join("services/api");
        let bin = temp.path().join("bin");
        fs::create_dir_all(repo.join("scripts")).unwrap();
        fs::create_dir_all(&backend).unwrap();
        fs::create_dir_all(&bin).unwrap();
        let script = render(
            FrontendBackendContext {
                preset: ScaffoldPreset::GoReact,
                root: "services/api",
                database: FrontendDatabaseContext {
                    db: ScaffoldDb::Postgres,
                    migration_dir: "services/api/internal/database/migrations",
                    sqlx_metadata_dir: "",
                },
            },
            "example-project",
        )
        .unwrap();
        fs::write(repo.join(&script.relative), script.contents).unwrap();
        let go = bin.join("go");
        fs::write(
            &go,
            r#"#!/bin/sh
set -eu
[ "$PWD" = "$EXAMPLE_BACKEND" ] || exit 2
case "$*" in
  'tool sqlc generate') printf 'sqlc\n' >> "$EXAMPLE_CALLS" ;;
  'run ./cmd/api --bootstrap-database')
    printf 'database\n' >> "$EXAMPLE_CALLS"
    printf 'permission denied to create database\n' >&2
    exit 7 ;;
  *) exit 2 ;;
esac
"#,
        )
        .unwrap();
        fs::set_permissions(go, fs::Permissions::from_mode(0o755)).unwrap();
        let mut paths = vec![bin];
        paths.extend(std::env::split_paths(&std::env::var_os("PATH").unwrap()));
        let calls = temp.path().join("calls");
        let run = || {
            Command::new("bash")
                .arg(repo.join(&script.relative))
                .current_dir(temp.path())
                .env_remove("DATABASE_URL")
                .env("PATH", std::env::join_paths(&paths).unwrap())
                .env("EXAMPLE_BACKEND", &backend)
                .env("EXAMPLE_CALLS", &calls)
                .output()
                .unwrap()
        };
        let missing = run();
        assert!(!missing.status.success());
        assert!(String::from_utf8_lossy(&missing.stderr).contains("Missing DATABASE_URL"));
        assert!(!calls.exists());
        fs::write(
            backend.join(".env"),
            "DATABASE_URL=postgres://example@localhost/example\n",
        )
        .unwrap();
        let denied = run();
        assert_eq!(denied.status.code(), Some(7));
        assert!(String::from_utf8_lossy(&denied.stderr).contains("permission denied"));
        assert_eq!(fs::read_to_string(calls).unwrap(), "sqlc\ndatabase\n");
    }
}
