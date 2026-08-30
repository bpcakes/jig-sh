#[cfg(unix)]
fn assert_rust_library_command(repo: &Path, program: &str, args: &[&str]) {
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
#[test]
fn rust_library_init_generates_exact_buildable_neutral_workspace() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_git_worktree();
    let destination = temp.path().join("ExampleLibrary");

    let report = run_init(InitOpts {
        path: destination.clone(),
        scaffold: ScaffoldOpts {
            preset: Some(ScaffoldPreset::RustLibrary),
            ..ScaffoldOpts::default()
        },
        template: Some(template.path().display().to_string()),
        template_mode: None,
        vcs_ref: None,
        force: false,
        defaults: false,
        no_input: true,
        no_vault: true,
        answers: AnswerOpts::default(),
    })
    .unwrap();
    let report = serde_json::to_value(report).unwrap();
    let scaffold = &report["scaffold"];

    assert_eq!(scaffold["preset"], "rust-library");
    assert_eq!(scaffold["repo_name"], "examplelibrary");
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
            "crates/examplelibrary/Cargo.toml",
            "crates/examplelibrary/AGENTS.md",
            "crates/examplelibrary/src/lib.rs"
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
    assert!(report["next_steps"]
        .as_array()
        .unwrap()
        .iter()
        .all(|step| step.as_str() != Some("scripts/jig dev")));

    for path in [
        "Cargo.toml",
        "README.md",
        "crates/examplelibrary/AGENTS.md",
        "crates/examplelibrary/Cargo.toml",
        "crates/examplelibrary/src/lib.rs",
        ".jig.toml",
        ".agent/jig-contract.json",
        ".github/workflows/rust-tests.yml",
        "AGENTS.md",
        "agent-map.md",
        "scripts/jig",
    ] {
        assert!(destination.join(path).is_file(), "missing {path}");
    }
    for path in [
        "Cargo.lock",
        "LICENSE",
        "LICENSE.md",
        ".env.example",
        "migrations",
        ".sqlx",
        "apps",
        "openapi",
        "package.json",
        "bun.lock",
        "bun.lockb",
        "package-lock.json",
        "pnpm-lock.yaml",
        "yarn.lock",
        "scripts/contracts.mjs",
        ".github/workflows/webapp-checks.yml",
        ".github/workflows/release.yml",
        "crates/examplelibrary-db",
        "crates/examplelibrary/src/main.rs",
    ] {
        assert!(!destination.join(path).exists(), "unexpected {path}");
    }

    let root_manifest =
        toml::from_str::<toml::Value>(&fs::read_to_string(destination.join("Cargo.toml")).unwrap())
            .unwrap();
    assert_eq!(root_manifest["workspace"]["resolver"].as_str(), Some("3"));
    assert_eq!(
        root_manifest["workspace"]["members"].as_array().unwrap(),
        &[toml::Value::String("crates/examplelibrary".into())]
    );
    assert_eq!(
        root_manifest["workspace"]["package"]["edition"].as_str(),
        Some("2024")
    );
    assert_eq!(
        root_manifest["workspace"]["package"]["rust-version"].as_str(),
        Some(env!("CARGO_PKG_RUST_VERSION"))
    );
    assert!(root_manifest.get("package").is_none());
    let crate_manifest = toml::from_str::<toml::Value>(
        &fs::read_to_string(destination.join("crates/examplelibrary/Cargo.toml")).unwrap(),
    )
    .unwrap();
    assert_eq!(crate_manifest["package"]["publish"].as_bool(), Some(false));
    assert!(crate_manifest["package"].get("license").is_none());
    assert!(crate_manifest["package"].get("license-file").is_none());
    assert!(crate_manifest.get("dependencies").is_none());
    assert!(crate_manifest.get("bin").is_none());
    let library_source =
        fs::read_to_string(destination.join("crates/examplelibrary/src/lib.rs")).unwrap();
    assert!(library_source.starts_with("//! Library entry point"));
    assert!(!library_source.contains("pub "));
    let readme = fs::read_to_string(destination.join("README.md")).unwrap();
    assert!(readme.contains("scripts/jig setup"));
    assert!(readme.contains("Setup creates `Cargo.lock`; commit it"));
    assert!(readme.contains("publish = false"));

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
    assert!(!config_text.contains("[[dev.apps]]"));
    assert!(!config_text.contains("rust-library"));
    assert!(!config_text.contains("backend_language ="));
    assert!(!config_text.contains("go_database ="));
    assert!(!config_text.contains("migration_dir ="));
    assert!(!config_text.contains("sqlx_check_command"));

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

    let contract_text =
        fs::read_to_string(destination.join(".agent/jig-contract.json")).unwrap();
    let contract = serde_json::from_str::<serde_json::Value>(&contract_text).unwrap();
    assert_eq!(
        contract["components"]
            .as_array()
            .unwrap()
            .iter()
            .map(|component| component["id"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["repo", "workspace"]
    );
    for (component, action) in [
        ("workspace", "fmt"),
        ("workspace", "clippy"),
        ("workspace", "test"),
        ("workspace", "test-locked"),
        ("repo", "rust-file-loc"),
    ] {
        assert!(contract["actions"].as_array().unwrap().iter().any(|candidate| {
            candidate["target"]["component"] == component
                && candidate["target"]["action"] == action
        }));
    }
    assert!(contract["tools"].as_array().unwrap().iter().all(|tool| {
        !tool["name"]
            .as_str()
            .is_some_and(|name| name.contains("sqlx") || name.contains("typescript"))
    }));
    assert!(!contract_text.contains("rust-library"));

    let root_guide = fs::read_to_string(destination.join("AGENTS.md")).unwrap();
    for expected in [
        "before Rust work",
        "## Rust Defaults",
        "For Rust changes",
        "## Crate Guide Conventions",
    ] {
        assert!(root_guide.contains(expected), "missing {expected}");
    }
    for absent in [
        "Keep transport logic thin",
        "- `scripts/jig dev`",
        "## Backend Defaults",
        "For backend changes",
    ] {
        assert!(!root_guide.contains(absent), "unexpected {absent}");
    }
    let agent_map = fs::read_to_string(destination.join("agent-map.md")).unwrap();
    assert!(agent_map.contains("crates/examplelibrary"));

    let bootstrap = config["commands"]["repo_bootstrap_command"]
        .as_str()
        .unwrap();
    assert_rust_library_command(&destination, "/bin/sh", &["-c", bootstrap]);
    assert!(destination.join("Cargo.lock").is_file());
    assert_rust_library_command(&destination, "cargo", &["fmt", "--all", "--", "--check"]);
    assert_rust_library_command(
        &destination,
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
    assert_rust_library_command(&destination, "cargo", &["test", "--workspace"]);
    assert_rust_library_command(
        &destination,
        "cargo",
        &["test", "--workspace", "--locked"],
    );
    assert_rust_library_command(
        &destination,
        "cargo",
        &["doc", "--workspace", "--no-deps", "--locked"],
    );

    fs::write(
        destination.join("README.md"),
        "# ExampleLibrary\n\nProject-owned README.\n",
    )
    .unwrap();
    fs::write(
        destination.join("crates/examplelibrary/src/lib.rs"),
        "//! Project-owned library documentation.\n",
    )
    .unwrap();
    run_update(UpdateOpts {
        path: destination.clone(),
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
        "# ExampleLibrary\n\nProject-owned README.\n"
    );
    assert_eq!(
        fs::read_to_string(destination.join("crates/examplelibrary/src/lib.rs")).unwrap(),
        "//! Project-owned library documentation.\n"
    );
    let managed = managed_manifest_paths(&destination);
    assert!(!managed.iter().any(|path| matches!(
        path.as_str(),
        "README.md"
            | "Cargo.toml"
            | "crates/examplelibrary/AGENTS.md"
            | "crates/examplelibrary/Cargo.toml"
            | "crates/examplelibrary/src/lib.rs"
    )));
}
