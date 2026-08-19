
#[cfg(unix)]
#[test]
fn required_tools_redacts_every_command_body_and_generic_credential_token() {
    let temp = tempdir().unwrap();
    write_doctor_fixture(temp.path());
    let secret = "generic-required-command-secret";
    let config_path = temp.path().join(".jig.toml");
    let config = fs::read_to_string(&config_path).unwrap().replace(
        "bootstrap_command = \"printf bootstrap\"",
        &format!(
            "bootstrap_command = {:?}",
            format!("postgres://doctor-user:{secret}@localhost/demo --check")
        ),
    );
    fs::write(config_path, config).unwrap();
    let bin = temp.path().join("bin");
    fs::create_dir(&bin).unwrap();
    let ctx = RepoContext::load_from_root(temp.path().to_path_buf()).unwrap();

    let check = required_tools_check_with_environment(&ctx, &doctor_environment(&bin, None));
    assert!(!check.ok);
    assert_eq!(check.status, "missing");
    assert_eq!(
        check.data["tools"][0]["command"],
        "<redacted: bootstrap_command>"
    );
    assert_eq!(check.data["tools"][0]["command_redacted"], true);
    let output = output(None, vec![check]);
    let serialized = serde_json::to_string(&output).unwrap();
    let summary = format_summary(&output);
    for rendered in [&serialized, &summary] {
        assert!(!rendered.contains(secret));
        assert!(!rendered.contains("postgres://doctor-user"));
    }
    assert!(serialized.contains("<redacted: command executable>"));
}

#[test]
fn agent_next_step_prefers_command_shaped_steps() {
    let steps = vec![
        json!("Codex CLI is not available on PATH."),
        json!("Run `scripts/jig agent bootstrap` to register skills."),
    ];

    assert_eq!(
        agent_next_step(&steps),
        Some("Run `scripts/jig agent bootstrap` to register skills.")
    );
}

#[test]
fn summary_surfaces_optional_missing_agent_skills() {
    let output = json!({
        "ok": true,
        "repo": {
            "root": "/tmp/demo",
        },
        "checks": [
            {
                "label": "Agent skills",
                "status": "missing",
                "required": false,
                "ok": false,
            },
        ],
        "next_step": "Run `scripts/jig agent bootstrap`.",
    });

    let summary = format_summary(&output);

    assert!(summary.contains("Jig doctor: ready"));
    assert!(summary.contains("Agent skills: optional setup (missing, optional)"));
    assert!(summary.contains("Next required step: none"));
    assert!(summary.contains("Optional setup: scripts/jig agent bootstrap"));
}

#[test]
fn summary_surfaces_required_tool_missing_detail() {
    let output = json!({
        "ok": false,
        "repo": {
            "root": "/tmp/demo",
        },
        "checks": [
            {
                "label": "Required tools",
                "status": "missing",
                "required": true,
                "ok": false,
                "detail": "Missing command executable(s): schema_dump_command: scripts/dump-schema.sh",
            },
        ],
        "next_step": "Install the missing executable.",
    });

    let summary = format_summary(&output);

    assert!(summary.contains("Required tools: needs setup (missing, required)"));
    assert!(summary.contains(
        "Detail: Missing command executable(s): schema_dump_command: scripts/dump-schema.sh"
    ));
    assert!(summary.contains("Next required step: Install the missing executable."));
    assert!(summary.contains("Optional setup: none"));
}

#[test]
fn doctor_reports_unified_readiness_checks() {
    let _env = lock_env();
    let temp = tempdir().unwrap();
    write_doctor_fixture(temp.path());
    let _cwd = CurrentDirGuard::set(temp.path());

    let output = run().unwrap();

    assert_eq!(output["command"], "doctor");
    assert_eq!(output["repo"]["name"], "demo");
    assert_eq!(output["checks"].as_array().unwrap().len(), 8);
    assert!(check_by_id(&output, "runtime")["ok"].as_bool().unwrap());
    assert!(check_by_id(&output, "config")["ok"].as_bool().unwrap());
    assert!(check_by_id(&output, "contract")["ok"].as_bool().unwrap());
    assert!(
        check_by_id(&output, "required_tools")["ok"]
            .as_bool()
            .unwrap()
    );
    assert!(
        check_by_id(&output, "agent_skills")["ok"]
            .as_bool()
            .unwrap()
    );
    assert_eq!(check_by_id(&output, "agent_skills")["required"], false);
    assert_eq!(check_by_id(&output, "proxy")["status"], "not configured");
    assert!(check_by_id(&output, "proxy")["ok"].as_bool().unwrap());
    assert_eq!(check_by_id(&output, "vault")["required"], false);
}

#[test]
fn doctor_reports_all_checks_when_config_is_invalid() {
    let _env = lock_env();
    let temp = tempdir().unwrap();
    fs::write(temp.path().join(".jig.toml"), "repo_name = \n").unwrap();
    fs::create_dir_all(temp.path().join("scripts")).unwrap();
    fs::write(
        temp.path().join("scripts/jig"),
        "#!/bin/sh\n# Runtime selection uses __runtime-compatible.\n",
    )
    .unwrap();
    let _cwd = CurrentDirGuard::set(temp.path());

    let output = run().unwrap();

    assert_eq!(output["command"], "doctor");
    assert_eq!(output["checks"].as_array().unwrap().len(), 8);
    assert_eq!(check_by_id(&output, "config")["status"], "invalid");
    assert_eq!(check_by_id(&output, "contract")["status"], "blocked");
    assert_eq!(check_by_id(&output, "required_tools")["status"], "blocked");
    assert_eq!(check_by_id(&output, "agent_skills")["status"], "blocked");
    assert_eq!(check_by_id(&output, "proxy")["status"], "blocked");
    assert_eq!(check_by_id(&output, "vault")["status"], "blocked");
    for id in ["contract", "required_tools", "agent_skills", "proxy"] {
        assert!(
            check_by_id(&output, id)["detail"]
                .as_str()
                .unwrap()
                .contains(".jig.toml")
        );
    }
    assert!(
        check_by_id(&output, "vault")["detail"]
            .as_str()
            .unwrap()
            .contains("repo context")
    );
    assert!(output["next_step"].as_str().unwrap().contains(".jig.toml"));
    assert!(
        output["next_required_step"]
            .as_str()
            .unwrap()
            .contains(".jig.toml")
    );
    assert!(output["optional_setup"].is_null());
    let summary = format_summary(&output);
    assert!(summary.contains("Next required step: Fix `.jig.toml`"));
    assert!(summary.contains("Optional setup: none"));
}

#[test]
fn doctor_uses_configured_repo_root_before_current_directory() {
    let _env = lock_env();
    let temp = tempdir().unwrap();
    let repo = temp.path().join("repo");
    let other = temp.path().join("other");
    fs::create_dir_all(&repo).unwrap();
    fs::create_dir_all(&other).unwrap();
    write_doctor_fixture(&repo);
    let _repo_root = EnvVarGuard::set("JIG_REPO_ROOT", &repo);
    let _cwd = CurrentDirGuard::set(&other);

    let output = run().unwrap();

    assert_eq!(
        output["repo"]["root"],
        fs::canonicalize(&repo).unwrap().display().to_string()
    );
    assert_eq!(output["repo"]["name"], "demo");
}

#[test]
fn doctor_reports_invalid_configured_repo_root() {
    let _env = lock_env();
    let temp = tempdir().unwrap();
    let missing_config = temp.path().join("missing-config");
    fs::create_dir_all(&missing_config).unwrap();
    let _repo_root = EnvVarGuard::set("JIG_REPO_ROOT", &missing_config);

    let output = run().unwrap();

    assert_eq!(output["ok"], false);
    assert_eq!(check_by_id(&output, "repo")["status"], "missing");
    assert!(
        check_by_id(&output, "repo")["detail"]
            .as_str()
            .unwrap()
            .contains("JIG_REPO_ROOT does not contain .jig.toml")
    );
    assert!(
            check_by_id(&output, "repo")["fix"]
                .as_str()
                .unwrap()
                .contains("init <path> --preset harness-only --repo-name <name> --sqlx-enabled false --no-input --no-vault")
        );
}

#[cfg(unix)]
fn cargo_sqlx_program(check: &DoctorCheck) -> &Value {
    check.data["tools"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|tool| tool["programs"].as_array().unwrap())
        .find(|program| program.get("driver_probe").is_some())
        .unwrap()
}

#[cfg(unix)]
fn doctor_environment(bin: &Path, database_url: Option<&str>) -> DoctorEnvironment {
    let bin = fs::canonicalize(bin).unwrap_or_else(|_| bin.to_path_buf());
    DoctorEnvironment {
        search_path: Some(bin.into_os_string()),
        path_extensions: None,
        database_url: database_url.map(OsString::from),
        cargo_alias_sqlx: None,
        cargo_home: None,
        home: None,
        probe_environment: Vec::new(),
        shell_environment_issue: None,
    }
}

#[cfg(unix)]
#[test]
fn rust_runtime_check_rejects_an_active_version_below_the_cargo_authority() {
    let temp = tempdir().unwrap();
    let root = temp.path().join("repo");
    let bin = temp.path().join("bin");
    fs::create_dir_all(&bin).unwrap();
    write_doctor_fixture(&root);
    write_workspace_version_manifest(&root, "1.94", None);
    write_test_executable(
        &bin.join("rustc"),
        "#!/bin/sh\nprintf 'rustc 1.85.1 (fixture 2025-03-18)\\n'\n",
    );
    let ctx = RepoContext::load_from_root(root).unwrap();

    let check = rust_runtime_check(
        &ctx,
        &doctor_environment(&bin, None),
        DoctorProcessControl::allowed_without_signal_session(),
    )
    .unwrap();

    assert!(!check.ok, "{check:?}");
    assert_eq!(check.status, "incompatible", "{check:?}");
    assert_eq!(check.data["required"], "1.94.0");
    assert_eq!(check.data["actual"], "1.85.1");
    assert!(check.fix.as_deref().unwrap().contains("Rust 1.94.0"));
    let summary = format_summary(&output(None, vec![check]));
    assert!(summary.contains("Jig doctor: needs attention"));
    assert!(summary.contains("Rust runtime: needs setup"));
}

#[cfg(unix)]
#[test]
fn rust_runtime_check_accepts_an_active_version_at_or_above_the_cargo_authority() {
    let temp = tempdir().unwrap();
    let root = temp.path().join("repo");
    let bin = temp.path().join("bin");
    fs::create_dir_all(&bin).unwrap();
    write_doctor_fixture(&root);
    write_workspace_version_manifest(&root, "1.94", None);
    write_test_executable(
        &bin.join("rustc"),
        "#!/bin/sh\nprintf 'rustc 1.94.1 (fixture 2026-03-25)\\n'\n",
    );
    let ctx = RepoContext::load_from_root(root).unwrap();

    let check = rust_runtime_check(
        &ctx,
        &doctor_environment(&bin, None),
        DoctorProcessControl::allowed_without_signal_session(),
    )
    .unwrap();

    assert!(check.ok, "{check:?}");
    assert_eq!(check.status, "compatible");
    assert_eq!(check.data["actual"], "1.94.1");
    assert!(check.fix.is_none());
}

#[cfg(unix)]
#[test]
fn sqlx_cli_version_check_requires_the_dependency_minor_line() {
    let temp = tempdir().unwrap();
    let root = temp.path().join("repo");
    let bin = temp.path().join("bin");
    fs::create_dir_all(&bin).unwrap();
    write_sqlx_doctor_fixture_with_command(&root, "sqlx prepare -D sqlite:doctor.db");
    write_workspace_version_manifest(&root, "1.94", Some("0.9"));
    write_test_executable(&bin.join("sqlx"), "#!/bin/sh\nprintf 'sqlx-cli 0.8.6\\n'\n");
    let ctx = RepoContext::load_from_root(root).unwrap();

    let check = sqlx_cli_version_check(
        &ctx,
        &doctor_environment(&bin, None),
        DoctorProcessControl::allowed_without_signal_session(),
    )
    .unwrap();

    assert!(!check.ok);
    assert_eq!(check.status, "incompatible");
    assert_eq!(check.data["required"], "0.9");
    assert_eq!(check.data["actual"], "0.8.6");
    assert!(check.fix.as_deref().unwrap().contains("--version ^0.9"));
    assert!(check.fix.as_deref().unwrap().contains("features sqlite"));
}

#[cfg(unix)]
#[test]
fn sqlx_cli_version_check_accepts_matching_patch_versions_and_older_dependency_lines() {
    for (dependency, cli) in [("0.9", "0.9.3"), ("0.8", "0.8.6")] {
        let temp = tempdir().unwrap();
        let root = temp.path().join("repo");
        let bin = temp.path().join("bin");
        fs::create_dir_all(&bin).unwrap();
        write_sqlx_doctor_fixture_with_command(&root, "sqlx prepare -D sqlite:doctor.db");
        write_workspace_version_manifest(&root, "1.94", Some(dependency));
        write_test_executable(
            &bin.join("sqlx"),
            &format!("#!/bin/sh\nprintf 'sqlx-cli {cli}\\n'\n"),
        );
        let ctx = RepoContext::load_from_root(root).unwrap();

        let check = sqlx_cli_version_check(
            &ctx,
            &doctor_environment(&bin, None),
            DoctorProcessControl::allowed_without_signal_session(),
        )
        .unwrap();

        assert!(check.ok, "{dependency} should accept {cli}: {check:?}");
        assert_eq!(check.status, "compatible");
        assert_eq!(check.data["actual"], cli);
        assert!(check.fix.is_none());
    }
}

#[cfg(unix)]
#[test]
fn node_runtime_check_rejects_an_active_version_below_the_repo_authority() {
    let temp = tempdir().unwrap();
    let root = temp.path().join("repo");
    let bin = temp.path().join("bin");
    fs::create_dir_all(&bin).unwrap();
    write_frontend_doctor_fixture(&root);
    fs::write(root.join(".node-version"), "24.19.0\n").unwrap();
    write_test_executable(&bin.join("node"), "#!/bin/sh\nprintf 'v22.23.2\\n'\n");
    let ctx = RepoContext::load_from_root(root).unwrap();

    let check = node_runtime_check(
        &ctx,
        &doctor_environment(&bin, None),
        DoctorProcessControl::allowed_without_signal_session(),
    )
    .unwrap();

    assert!(!check.ok);
    assert_eq!(check.status, "incompatible");
    assert_eq!(check.data["required"], "24.19.0");
    assert_eq!(check.data["actual"], "22.23.2");
    assert!(
        check
            .fix
            .as_deref()
            .unwrap()
            .contains("Activate Node 24.19.0")
    );
    let summary = format_summary(&output(None, vec![check]));
    assert!(summary.contains("Jig doctor: needs attention"));
    assert!(summary.contains("Node runtime: needs setup"));
}

#[cfg(unix)]
#[test]
fn node_runtime_check_accepts_an_active_version_at_or_above_the_repo_authority() {
    let temp = tempdir().unwrap();
    let root = temp.path().join("repo");
    let bin = temp.path().join("bin");
    fs::create_dir_all(&bin).unwrap();
    write_frontend_doctor_fixture(&root);
    fs::write(root.join(".node-version"), "24.19.0\n").unwrap();
    write_test_executable(&bin.join("node"), "#!/bin/sh\nprintf 'v24.20.1\\n'\n");
    let ctx = RepoContext::load_from_root(root).unwrap();

    let check = node_runtime_check(
        &ctx,
        &doctor_environment(&bin, None),
        DoctorProcessControl::allowed_without_signal_session(),
    )
    .unwrap();

    assert!(check.ok);
    assert_eq!(check.status, "compatible");
    assert_eq!(check.data["actual"], "24.20.1");
    assert!(check.fix.is_none());
}

#[cfg(unix)]
#[test]
fn node_runtime_check_rejects_a_non_regular_version_authority() {
    let temp = tempdir().unwrap();
    let root = temp.path().join("repo");
    let bin = temp.path().join("bin");
    fs::create_dir_all(&bin).unwrap();
    write_frontend_doctor_fixture(&root);
    fs::create_dir(root.join(".node-version")).unwrap();
    let ctx = RepoContext::load_from_root(root).unwrap();

    let check = node_runtime_check(
        &ctx,
        &doctor_environment(&bin, None),
        DoctorProcessControl::allowed_without_signal_session(),
    )
    .unwrap();

    assert!(!check.ok);
    assert_eq!(check.status, "invalid authority");
    assert!(check.detail.contains("real regular file"));
}

#[cfg(unix)]
fn write_test_executable(path: &Path, body: &str) {
    use std::os::unix::fs::PermissionsExt;

    fs::write(path, body).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

#[cfg(unix)]
fn write_frontend_doctor_fixture(root: &Path) {
    write_doctor_fixture(root);
    let config_path = root.join(".jig.toml");
    let config = format!(
        "{}\n[[frontend_apps]]\nname = \"web\"\ndir = \"web\"\ncoverage_threshold = 80\nkind = \"vite\"\nrole = \"spa\"\n\n[dev]\n\n[[dev.apps]]\nname = \"web\"\ndir = \"web\"\nkind = \"vite\"\nargv = [\"bun\", \"run\", \"dev\"]\n",
        fs::read_to_string(&config_path).unwrap()
    );
    fs::write(config_path, config).unwrap();
    fs::create_dir(root.join("web")).unwrap();
}

#[cfg(unix)]
fn write_workspace_version_manifest(root: &Path, rust: &str, sqlx: Option<&str>) {
    let sqlx = sqlx
        .map(|version| format!("\n[workspace.dependencies]\nsqlx = {{ version = {version:?} }}\n"))
        .unwrap_or_default();
    fs::write(
        root.join("Cargo.toml"),
        format!(
            "[workspace]\nmembers = []\n\n[workspace.package]\nrust-version = {rust:?}\n{sqlx}"
        ),
    )
    .unwrap();
}

fn write_sqlx_doctor_fixture_with_command(root: &Path, command: &str) {
    write_doctor_fixture(root);
    let config_path = root.join(".jig.toml");
    let sqlx_config = format!(
        "sqlx_enabled = true\nrust_crate_roots = [\"crates\"]\nrust_migration_dir = \"migrations\"\nrust_sqlx_metadata_dir = \".sqlx\"\nschema_dump_enabled = false\nsqlx_check_command = {command:?}\n\n[agent_tooling.codex]"
    );
    let config = fs::read_to_string(&config_path)
        .unwrap()
        .replace("[agent_tooling.codex]", &sqlx_config);
    fs::write(config_path, config).unwrap();
    fs::create_dir(root.join("migrations")).unwrap();

    let contract_path = root.join(".agent/jig-contract.json");
    let mut contract: Value =
        serde_json::from_str(&fs::read_to_string(&contract_path).unwrap()).unwrap();
    contract["required_commands"]
        .as_array_mut()
        .unwrap()
        .push(json!("sqlx_check_command"));
    let tools = contract["tools"].as_array_mut().unwrap();
    tools.push(json!({
        "name": tool::SQLX_CHECK,
        "kind": "command",
        "description": "Run the configured SQLx check command.",
        "command": "sqlx_check_command",
    }));
    tools.push(json!({
        "name": tool::MIGRATION_ADD,
        "kind": "native",
        "description": "Add timestamped SQL migration stubs.",
    }));
    fs::write(
        contract_path,
        serde_json::to_string_pretty(&contract).unwrap(),
    )
    .unwrap();
}

fn write_doctor_fixture_with_bootstrap_command(root: &Path, command: &str) {
    write_doctor_fixture(root);
    let config_path = root.join(".jig.toml");
    let config = fs::read_to_string(&config_path).unwrap().replace(
        "bootstrap_command = \"printf bootstrap\"",
        &format!("bootstrap_command = {command:?}"),
    );
    fs::write(config_path, config).unwrap();
}

fn check_by_id<'a>(output: &'a Value, id: &str) -> &'a Value {
    output["checks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|check| check["id"] == id)
        .unwrap()
}
