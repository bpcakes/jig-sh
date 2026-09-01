#[cfg(unix)]
#[derive(Clone, Copy)]
struct RustOnlyAcceptanceCase {
    preset: ScaffoldPreset,
    destination_name: &'static str,
    package: &'static str,
    source: &'static str,
    opposite_source: &'static str,
    package_manager: Option<&'static str>,
}

#[cfg(unix)]
impl RustOnlyAcceptanceCase {
    fn library() -> Self {
        Self {
            preset: ScaffoldPreset::RustLibrary,
            destination_name: "ExampleLibrary",
            package: "examplelibrary",
            source: "src/lib.rs",
            opposite_source: "src/main.rs",
            package_manager: None,
        }
    }

    fn cli() -> Self {
        Self {
            preset: ScaffoldPreset::RustCli,
            destination_name: "ExampleCli",
            package: "examplecli",
            source: "src/main.rs",
            opposite_source: "src/lib.rs",
            package_manager: Some("npm"),
        }
    }

    fn source_path(self) -> String {
        format!("crates/{}/{}", self.package, self.source)
    }

    fn opposite_source_path(self) -> String {
        format!("crates/{}/{}", self.package, self.opposite_source)
    }

    fn crate_path(self, relative: &str) -> String {
        format!("crates/{}/{relative}", self.package)
    }
}

#[cfg(unix)]
fn assert_rust_only_command(repo: &Path, program: &str, args: &[&str]) {
    let output = Command::new(program)
        .current_dir(repo)
        .env("CARGO_NET_OFFLINE", "true")
        .env("RUSTDOCFLAGS", "-D warnings")
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{program} {} failed with {}\nstdout:\n{}\nstderr:\n{}",
        args.join(" "),
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[cfg(unix)]
fn assert_rust_only_init_report(report: &serde_json::Value, case: RustOnlyAcceptanceCase) {
    let scaffold = &report["scaffold"];
    assert_eq!(scaffold["preset"], case.preset.as_str());
    assert_eq!(scaffold["repo_name"], case.package);
    assert_eq!(scaffold["db"], "none");
    assert_eq!(scaffold["frontends"], serde_json::json!([]));
    assert_eq!(scaffold["frontend_notices"], serde_json::json!([]));
    assert_eq!(scaffold["files_modified"], serde_json::json!([]));
    assert_eq!(scaffold["files_unchanged"], serde_json::json!([]));
    assert_eq!(
        scaffold["files_created"],
        serde_json::json!([
            "Cargo.toml",
            "README.md",
            "clippy.toml",
            case.crate_path("Cargo.toml"),
            case.crate_path("AGENTS.md"),
            case.source_path(),
        ])
    );
    assert_eq!(
        report["next_steps"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(serde_json::Value::as_str)
            .filter(|step| step.contains("scripts/jig"))
            .collect::<Vec<_>>(),
        ["scripts/jig setup", "scripts/jig check test"]
    );
    assert!(
        report["next_steps"]
            .as_array()
            .unwrap()
            .iter()
            .all(|step| step.as_str() != Some("scripts/jig dev"))
    );
}

#[cfg(unix)]
fn assert_rust_only_layout(destination: &Path, case: RustOnlyAcceptanceCase) {
    let expected = [
        "Cargo.toml".to_string(),
        "README.md".to_string(),
        "clippy.toml".to_string(),
        case.crate_path("AGENTS.md"),
        case.crate_path("Cargo.toml"),
        case.source_path(),
        ".jig.toml".to_string(),
        ".agent/jig-contract.json".to_string(),
        ".agent/PLANS.md".to_string(),
        ".github/workflows/rust-tests.yml".to_string(),
        ".github/workflows/repo-policy.yml".to_string(),
        "AGENTS.md".to_string(),
        "agent-map.md".to_string(),
        "scripts/jig".to_string(),
        "scripts/install-jig.sh".to_string(),
        ".jig/file-budget.toml".to_string(),
    ];
    for path in &expected {
        assert!(destination.join(path).is_file(), "missing {path}");
    }
    let forbidden = [
        "Cargo.lock".to_string(),
        "LICENSE".to_string(),
        "LICENSE.md".to_string(),
        "LICENSE.txt".to_string(),
        "COPYING".to_string(),
        ".env.example".to_string(),
        "migrations".to_string(),
        ".sqlx".to_string(),
        "apps".to_string(),
        "web".to_string(),
        "admin-panel".to_string(),
        "openapi".to_string(),
        "package.json".to_string(),
        "bun.lock".to_string(),
        "bun.lockb".to_string(),
        "package-lock.json".to_string(),
        "pnpm-lock.yaml".to_string(),
        "yarn.lock".to_string(),
        "scripts/contracts.mjs".to_string(),
        "scripts/check-webapps.sh".to_string(),
        "scripts/check-rust-file-loc.sh".to_string(),
        ".github/workflows/webapp-checks.yml".to_string(),
        ".github/workflows/release.yml".to_string(),
        format!("crates/{}-db", case.package),
        case.opposite_source_path(),
    ];
    for path in &forbidden {
        assert!(!destination.join(path).exists(), "unexpected {path}");
    }
}

#[cfg(unix)]
fn assert_rust_only_manifests_and_source(destination: &Path, case: RustOnlyAcceptanceCase) {
    let root_manifest =
        toml::from_str::<toml::Value>(&fs::read_to_string(destination.join("Cargo.toml")).unwrap())
            .unwrap();
    assert_eq!(root_manifest["workspace"]["resolver"].as_str(), Some("3"));
    assert_eq!(
        root_manifest["workspace"]["members"].as_array().unwrap(),
        &[toml::Value::String(format!("crates/{}", case.package))]
    );
    assert_eq!(
        root_manifest["workspace"]["package"]["edition"].as_str(),
        Some("2024")
    );
    assert_eq!(
        root_manifest["workspace"]["package"]["rust-version"].as_str(),
        Some(env!("CARGO_PKG_RUST_VERSION"))
    );
    assert_eq!(
        root_manifest["workspace"]["lints"]["clippy"]["cognitive_complexity"].as_str(),
        Some("warn")
    );
    assert_eq!(
        fs::read_to_string(destination.join("clippy.toml")).unwrap(),
        "cognitive-complexity-threshold = 20\n"
    );
    assert!(root_manifest.get("package").is_none());
    assert!(
        root_manifest["workspace"]["package"]
            .get("license")
            .is_none()
    );

    let crate_manifest = toml::from_str::<toml::Value>(
        &fs::read_to_string(destination.join(case.crate_path("Cargo.toml"))).unwrap(),
    )
    .unwrap();
    assert_eq!(crate_manifest["package"]["publish"].as_bool(), Some(false));
    assert_eq!(crate_manifest["lints"]["workspace"].as_bool(), Some(true));
    assert_rust_only_crate_source(destination, &crate_manifest, case);
}

#[cfg(unix)]
fn assert_rust_only_crate_source(
    destination: &Path,
    crate_manifest: &toml::Value,
    case: RustOnlyAcceptanceCase,
) {
    assert!(crate_manifest["package"].get("license").is_none());
    assert!(crate_manifest["package"].get("license-file").is_none());
    assert!(crate_manifest.get("dependencies").is_none());
    match case.preset {
        ScaffoldPreset::RustLibrary => {
            assert!(crate_manifest.get("bin").is_none());
            let source = fs::read_to_string(destination.join(case.source_path())).unwrap();
            assert!(source.starts_with("//! Library entry point"));
            assert!(!source.contains("pub "));
        }
        ScaffoldPreset::RustCli => {
            assert!(crate_manifest.get("dev-dependencies").is_none());
            assert!(crate_manifest.get("test").is_none());
            let bins = crate_manifest["bin"].as_array().unwrap();
            assert_eq!(bins.len(), 1);
            assert_eq!(bins[0]["name"].as_str(), Some(case.package));
            assert_eq!(bins[0]["path"].as_str(), Some("src/main.rs"));
            assert_eq!(
                fs::read_to_string(destination.join(case.source_path())).unwrap(),
                "fn main() {\n    println!(\"{} {}\", env!(\"CARGO_PKG_NAME\"), env!(\"CARGO_PKG_VERSION\"));\n}\n"
            );
        }
        _ => unreachable!("Rust-only acceptance received an application preset"),
    }
}

#[cfg(unix)]
fn assert_rust_only_readme_and_config(destination: &Path, case: RustOnlyAcceptanceCase) {
    let readme = fs::read_to_string(destination.join("README.md")).unwrap();
    assert_contains_all(
        &readme,
        &[
            "scripts/jig setup",
            "Setup creates `Cargo.lock`; commit it",
            "`clippy.toml` sets the project-owned",
            "`cognitive_complexity` restriction lint",
            "treats all warnings as failures",
            "publish = false",
        ],
    );
    if case.preset == ScaffoldPreset::RustCli {
        assert_contains_all(&readme, &[&format!("cargo run -p {}", case.package)]);
    }

    let config_text = fs::read_to_string(destination.join(".jig.toml")).unwrap();
    let config = toml::from_str::<toml::Value>(&config_text).unwrap();
    assert_eq!(config["sqlx_enabled"].as_bool(), Some(false));
    assert_eq!(config["schema_dump_enabled"].as_bool(), Some(false));
    assert_eq!(
        config["rust_crate_roots"].as_array().unwrap(),
        &[toml::Value::String("crates".into())]
    );
    assert_eq!(
        config["application_contracts_enabled"].as_bool(),
        Some(false)
    );
    assert!(config["frontend_apps"].as_array().unwrap().is_empty());
    assert!(
        config["frontend_workspace_roots"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    if let Some(package_manager) = case.package_manager {
        assert_eq!(
            config["web_package_manager"].as_str(),
            Some(package_manager)
        );
    }
    assert_contains_none(
        &config_text,
        &[
            "[[dev.apps]]",
            case.preset.as_str(),
            "backend_language =",
            "go_database =",
            "migration_dir =",
            "sqlx_check_command",
        ],
    );

    let components = config["repository"]["components"].as_array().unwrap();
    assert_eq!(
        components
            .iter()
            .map(|component| component["id"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["repo", "workspace"]
    );
    let workspace = components
        .iter()
        .find(|component| component["id"].as_str() == Some("workspace"))
        .unwrap();
    assert_eq!(workspace["root"].as_str(), Some("."));
    assert_eq!(
        workspace["adapters"].as_array().unwrap(),
        &[toml::Value::String("rust".into())]
    );
    assert!(
        config["repository"]["actions"]
            .as_array()
            .unwrap()
            .iter()
            .all(|action| action["target"]["component"].as_str() != Some("api"))
    );
}

#[cfg(unix)]
fn assert_rust_only_contract(destination: &Path, case: RustOnlyAcceptanceCase) {
    let contract_text = fs::read_to_string(destination.join(".agent/jig-contract.json")).unwrap();
    let contract = serde_json::from_str::<serde_json::Value>(&contract_text).unwrap();
    assert_eq!(contract["default_check_profile"], "verify");
    assert_eq!(
        contract["components"]
            .as_array()
            .unwrap()
            .iter()
            .map(|component| component["id"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["repo", "workspace"]
    );
    for (component, action, alias) in [
        ("workspace", "fmt", jig_contract::tool::FMT_CHECK),
        ("workspace", "clippy", jig_contract::tool::CLIPPY),
        ("workspace", "test", jig_contract::tool::TEST),
        ("workspace", "test-locked", jig_contract::tool::TEST_LOCKED),
        ("repo", "file-budget", jig_contract::tool::FILE_BUDGET),
    ] {
        let action = contract["actions"]
            .as_array()
            .unwrap()
            .iter()
            .find(|candidate| {
                candidate["target"]["component"] == component
                    && candidate["target"]["action"] == action
            })
            .unwrap_or_else(|| panic!("missing {component}:{action}"));
        assert!(
            action["legacy_aliases"]
                .as_array()
                .unwrap()
                .iter()
                .any(|candidate| candidate == alias),
            "{component}:{action} is missing {alias}"
        );
    }
    let action_ids = contract["actions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|action| {
            format!(
                "{}:{}",
                action["target"]["component"].as_str().unwrap(),
                action["target"]["action"].as_str().unwrap()
            )
        })
        .collect::<Vec<_>>();
    for absent in [
        "api",
        "backend",
        "dev",
        "go",
        "sqlx",
        "typescript",
        "frontend",
    ] {
        assert!(
            action_ids.iter().all(|action| !action.contains(absent)),
            "unexpected {absent} action in {action_ids:?}"
        );
    }
    assert!(contract["tools"].as_array().unwrap().iter().all(|tool| {
        !tool["name"]
            .as_str()
            .is_some_and(|name| name.contains("sqlx") || name.contains("typescript"))
    }));
    assert_contains_none(&contract_text, &[case.preset.as_str()]);
}

#[cfg(unix)]
fn assert_rust_only_guides_and_workflow(destination: &Path, case: RustOnlyAcceptanceCase) {
    let root_guide = fs::read_to_string(destination.join("AGENTS.md")).unwrap();
    assert_contains_all(
        &root_guide,
        &[
            "before Rust work",
            "## Rust Defaults",
            "For Rust changes",
            "## Crate Guide Conventions",
        ],
    );
    assert_contains_none(
        &root_guide,
        &[
            "Keep transport logic thin",
            "- `scripts/jig dev`",
            "## Backend Defaults",
            "For backend changes",
        ],
    );
    let crate_guide = fs::read_to_string(destination.join(case.crate_path("AGENTS.md"))).unwrap();
    assert_contains_all(
        &crate_guide,
        &[
            "## Purpose",
            "## Key entrypoints",
            "## Edit here for X",
            "## Invariants",
            "## Common commands",
        ],
    );
    assert_contains_all(
        &fs::read_to_string(destination.join("agent-map.md")).unwrap(),
        &[&format!("crates/{}", case.package)],
    );
    let workflow =
        fs::read_to_string(destination.join(".github/workflows/rust-tests.yml")).unwrap();
    assert_contains_count(&workflow, &[("      - \"**\"", 2)]);
    assert_contains_all(
        &workflow,
        &["workspace:fmt", "workspace:clippy", "workspace:test-locked"],
    );
    assert_contains_none(
        &workflow,
        &["SQLX_OFFLINE", "migrations/**", ".sqlx/**", "check-webapps"],
    );
}

#[cfg(unix)]
fn verify_rust_only_repository(
    destination: &Path,
    config: &toml::Value,
    case: RustOnlyAcceptanceCase,
) {
    let bootstrap = config["commands"]["repo_bootstrap_command"]
        .as_str()
        .unwrap();
    assert_rust_only_command(destination, "/bin/sh", &["-c", bootstrap]);
    assert!(destination.join("Cargo.lock").is_file());
    assert_rust_only_command(
        destination,
        "cargo",
        &[
            "metadata",
            "--offline",
            "--locked",
            "--no-deps",
            "--format-version",
            "1",
        ],
    );
    assert_rust_only_command(destination, "cargo", &["fmt", "--all", "--", "--check"]);
    assert_rust_only_command(
        destination,
        "cargo",
        &[
            "clippy",
            "--workspace",
            "--all-targets",
            "--locked",
            "--",
            "-D",
            "warnings",
        ],
    );
    assert_rust_only_command(destination, "cargo", &["test", "--workspace"]);
    assert_rust_only_command(destination, "cargo", &["test", "--workspace", "--locked"]);
    assert_rust_only_command(
        destination,
        "cargo",
        &["doc", "--workspace", "--no-deps", "--locked"],
    );
    if case.preset == ScaffoldPreset::RustCli {
        let binary = Command::new("cargo")
            .current_dir(destination)
            .env("CARGO_NET_OFFLINE", "true")
            .args(["run", "--quiet", "--locked", "-p", case.package])
            .output()
            .unwrap();
        assert!(binary.status.success(), "{}", binary.status);
        assert_eq!(
            binary.stdout,
            format!("{} 0.1.0\n", case.package).as_bytes()
        );
        assert!(binary.stderr.is_empty());
    }
}

#[cfg(unix)]
fn assert_rust_only_update_preserves_project_files(
    destination: &Path,
    case: RustOnlyAcceptanceCase,
) {
    let source_path = case.source_path();
    let project_readme = format!("# {}\n\nProject-owned README.\n", case.destination_name);
    let project_source = match case.preset {
        ScaffoldPreset::RustLibrary => "//! Project-owned library documentation.\n",
        ScaffoldPreset::RustCli => "fn main() {\n    println!(\"project-owned\");\n}\n",
        _ => unreachable!(),
    };
    fs::write(destination.join("README.md"), &project_readme).unwrap();
    fs::write(destination.join(&source_path), project_source).unwrap();
    run_update(UpdateOpts {
        path: destination.to_path_buf(),
        template: None,
        template_mode: None,
        recopy: true,
        launcher_only: false,
        force: false,
        vcs_ref: None,
        defaults: true,
        no_input: true,
    })
    .unwrap();
    assert_eq!(
        fs::read_to_string(destination.join("README.md")).unwrap(),
        project_readme
    );
    assert_eq!(
        fs::read_to_string(destination.join(&source_path)).unwrap(),
        project_source
    );
    let managed = managed_manifest_paths(destination);
    for project_owned in [
        "README.md".to_string(),
        "Cargo.toml".to_string(),
        "clippy.toml".to_string(),
        case.crate_path("AGENTS.md"),
        case.crate_path("Cargo.toml"),
        source_path,
    ] {
        assert!(!managed.contains(&project_owned), "managed {project_owned}");
    }
}

#[cfg(unix)]
fn assert_rust_only_generated_repository(case: RustOnlyAcceptanceCase) {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_git_worktree();
    let destination = temp.path().join(case.destination_name);
    let answers = AnswerOpts {
        web_package_manager: case.package_manager.map(str::to_string),
        ..AnswerOpts::default()
    };

    let report = run_init(InitOpts {
        path: destination.clone(),
        scaffold: ScaffoldOpts {
            preset: Some(case.preset),
            ..ScaffoldOpts::default()
        },
        template: Some(template.path().display().to_string()),
        template_mode: None,
        vcs_ref: None,
        force: false,
        defaults: false,
        no_input: true,
        no_vault: true,
        answers,
    })
    .unwrap();
    let report = serde_json::to_value(report).unwrap();
    assert_rust_only_init_report(&report, case);
    assert_rust_only_layout(&destination, case);
    assert_rust_only_manifests_and_source(&destination, case);
    assert_rust_only_readme_and_config(&destination, case);
    assert_rust_only_contract(&destination, case);
    assert_rust_only_guides_and_workflow(&destination, case);

    let config_text = fs::read_to_string(destination.join(".jig.toml")).unwrap();
    let config = toml::from_str::<toml::Value>(&config_text).unwrap();
    verify_rust_only_repository(&destination, &config, case);
    assert_rust_only_update_preserves_project_files(&destination, case);
}
