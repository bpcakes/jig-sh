
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
            &[
                PUBLIC_API_CLIENT_SHARED_TEMPLATES,
                RUST_PUBLIC_API_CLIENT_CONTRACT_TEMPLATES,
            ],
        );
        assert_complete(
            "go-react/frontend/api-client-public/",
            &[GO_PUBLIC_API_CLIENT_CONTRACT_TEMPLATES],
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
                PUBLIC_API_CLIENT_SHARED_TEMPLATES,
                RUST_PUBLIC_API_CLIENT_CONTRACT_TEMPLATES,
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
                for preset in [ScaffoldPreset::RustReact, ScaffoldPreset::GoReact] {
                    let declared = frontend_workspace_relative_paths_for_backend(
                        preset,
                        package_manager,
                        &frontends,
                    );
                    let rendered = render_frontend_workspace_files_for_backend(
                        FrontendBackendContext {
                            preset,
                            root: ".",
                            database: FrontendDatabaseContext {
                                db: ScaffoldDb::None,
                                migration_dir: "migrations",
                                sqlx_metadata_dir: ".sqlx",
                            },
                        },
                        package_manager,
                        "demo",
                        "main",
                        "ubuntu-latest",
                        &frontends,
                    )
                    .unwrap()
                    .into_iter()
                    .map(|file| PathBuf::from(file.relative))
                    .collect::<Vec<_>>();

                    assert_eq!(
                        declared, rendered,
                        "{preset:?}: {package_manager}: {frontends:?}"
                    );
                }
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

        let go = e2e_workflow_paths_for_backend(
            ScaffoldPreset::GoReact,
            ".",
            ScaffoldDb::Postgres,
            "database/migrations",
            ".sqlx",
            &[],
        );
        for authority in [
            "go.mod",
            "**/go.mod",
            "go.work",
            "**/go.work",
            "vendor/modules.txt",
            "**/vendor/modules.txt",
            ".jig.toml",
            ".agent/jig-contract.json",
            "scripts/jig",
            "scripts/install-jig.sh",
        ] {
            assert!(go.iter().any(|path| path == authority), "missing {authority}");
        }
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
