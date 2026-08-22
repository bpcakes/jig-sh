#[test]
fn matched_frontend_and_dev_dirs_use_portable_lexical_identity() {
    let config: RepoConfig = toml::from_str(
        r#"_src_path = "/tmp/template"
_commit = "abc123"
repo_name = "demo"
default_branch = "main"
jig_version = "0.2.0-beta.1"

[[frontend_apps]]
name = "docs"
dir = "./apps//docs/./"
coverage_threshold = 80

[[dev.apps]]
name = "docs"
dir = "apps/docs"
kind = "env-port"
argv = ["npm", "run", "dev"]
"#,
    )
    .unwrap();

    validate_config(&config).unwrap();
    assert_eq!(
        configured_frontend_app_metadata(&config, &config.frontend_apps[0]),
        ResolvedFrontendMetadata {
            kind: "env-port",
            role: "astro"
        }
    );
}

#[test]
fn configured_app_dirs_reject_non_portable_or_escaping_spellings() {
    for dir in ["/apps/web", "C:/apps/web", "apps/../web", r"apps\web", ""] {
        let config: RepoConfig = toml::from_str(&format!(
            r#"_src_path = "/tmp/template"
_commit = "abc123"
repo_name = "demo"
default_branch = "main"
jig_version = "0.2.0-beta.1"

[[frontend_apps]]
name = "web"
dir = {dir:?}
coverage_threshold = 80
"#,
        ))
        .unwrap();

        let error = validate_config(&config).unwrap_err().to_string();
        assert!(
            error.contains("portable repository-relative")
                || error.contains("must not contain '..'")
                || error.contains("portable '/' separators")
                || error.contains("must not be empty"),
            "{dir:?}: {error}"
        );
    }
}

#[test]
fn configured_app_dir_identity_is_case_sensitive_and_does_not_alias_paths() {
    for dev_dir in ["Apps/Web", "web-link"] {
        let config: RepoConfig = toml::from_str(&format!(
            r#"_src_path = "/tmp/template"
_commit = "abc123"
repo_name = "demo"
default_branch = "main"
jig_version = "0.2.0-beta.1"

[[frontend_apps]]
name = "web"
dir = "apps/web"
coverage_threshold = 80

[[dev.apps]]
name = "web"
dir = {dev_dir:?}
kind = "vite"
argv = ["npm", "run", "dev"]
"#,
        ))
        .unwrap();

        let error = validate_config(&config).unwrap_err().to_string();
        assert!(error.contains("uses dir"), "{dev_dir:?}: {error}");
        assert!(error.contains("apps/web"), "{dev_dir:?}: {error}");
    }
}

#[test]
fn frontend_role_defaults_to_spa_for_existing_repositories() {
    let config: RepoConfig = toml::from_str(
        r#"_src_path = "/tmp/template"
_commit = "abc123"
repo_name = "demo"
default_branch = "main"
jig_version = "0.2.0-beta.1"

[[frontend_apps]]
name = "legacy-web"
dir = "web"
coverage_threshold = 80
"#,
    )
    .unwrap();

    validate_config(&config).unwrap();
    assert_eq!(
        configured_frontend_app_metadata(&config, &config.frontend_apps[0]).role,
        "spa"
    );
}

#[test]
fn legacy_frontend_role_uses_known_admin_name_and_matching_dev_kind() {
    let config: RepoConfig = toml::from_str(
        r#"_src_path = "/tmp/template"
_commit = "abc123"
repo_name = "demo"
default_branch = "main"
jig_version = "0.2.0-beta.1"

[[frontend_apps]]
name = "admin-panel"
dir = "admin-panel"
coverage_threshold = 80

[[frontend_apps]]
name = "docs"
dir = "apps/docs"
coverage_threshold = 80

[[frontend_apps]]
name = "marketing"
dir = "apps/marketing"
coverage_threshold = 80
kind = "vite"

[[dev.apps]]
name = "admin-panel"
dir = "admin-panel"
kind = "vite"
argv = ["npm", "run", "dev"]

[[dev.apps]]
name = "docs"
dir = "apps/docs"
kind = "env-port"
argv = ["npm", "run", "dev"]

[[dev.apps]]
name = "marketing"
dir = "apps/marketing"
kind = "env-port"
argv = ["npm", "run", "dev"]
"#,
    )
    .unwrap();

    validate_config(&config).unwrap();
    assert_eq!(
        configured_frontend_app_metadata(&config, &config.frontend_apps[0]).role,
        "admin"
    );
    assert_eq!(
        configured_frontend_app_metadata(&config, &config.frontend_apps[1]).role,
        "astro"
    );
    assert_eq!(
        configured_frontend_app_metadata(&config, &config.frontend_apps[2]),
        ResolvedFrontendMetadata {
            kind: "vite",
            role: "spa"
        }
    );
}

#[test]
fn frontend_role_accepts_known_values_and_rejects_unknown_values() {
    for role in ["spa", "admin", "astro"] {
        let config: RepoConfig = toml::from_str(&format!(
            r#"_src_path = "/tmp/template"
_commit = "abc123"
repo_name = "demo"
default_branch = "main"
jig_version = "0.2.0-beta.1"

[[frontend_apps]]
name = "frontend"
dir = "frontend"
coverage_threshold = 80
role = "{role}"
"#
        ))
        .unwrap();
        validate_config(&config).unwrap();
    }

    let invalid: RepoConfig = toml::from_str(
        r#"_src_path = "/tmp/template"
_commit = "abc123"
repo_name = "demo"
default_branch = "main"
jig_version = "0.2.0-beta.1"

[[frontend_apps]]
name = "frontend"
dir = "frontend"
coverage_threshold = 80
role = "dashboard"
"#,
    )
    .unwrap();
    let error = validate_config(&invalid).unwrap_err().to_string();
    assert!(error.contains("Invalid frontend app role 'dashboard'"));
    assert!(error.contains("spa",));
    assert!(error.contains("admin"));
    assert!(error.contains("astro"));
}

#[test]
fn invalid_app_kinds_are_attributed_to_the_section_that_declares_them() {
    let explicit_frontend: RepoConfig = toml::from_str(
        r#"_src_path = "/tmp/template"
_commit = "abc123"
repo_name = "demo"
default_branch = "main"
jig_version = "0.2.0-beta.1"

[[frontend_apps]]
name = "web"
dir = "apps/web"
coverage_threshold = 80
kind = "webpack"
"#,
    )
    .unwrap();
    let error = validate_config(&explicit_frontend).unwrap_err().to_string();
    assert!(error.contains("Invalid frontend app kind 'webpack' for 'web'"));
    assert!(!error.contains("[[dev.apps]]"));

    let inherited_from_dev: RepoConfig = toml::from_str(
        r#"_src_path = "/tmp/template"
_commit = "abc123"
repo_name = "demo"
default_branch = "main"
jig_version = "0.2.0-beta.1"

[[frontend_apps]]
name = "web"
dir = "apps/web"
coverage_threshold = 80

[[dev.apps]]
name = "web"
dir = "apps/web"
kind = "webpack"
argv = ["npm", "run", "dev"]
"#,
    )
    .unwrap();
    let error = validate_config(&inherited_from_dev)
        .unwrap_err()
        .to_string();
    assert!(error.contains("Invalid dev app kind 'webpack' for 'web' in [[dev.apps]]"));
    assert!(!error.contains("Invalid frontend app kind"));
}

#[test]
fn unsupported_web_package_manager_is_rejected() {
    let temp = tempdir().unwrap();
    fs::create_dir_all(temp.path().join(".agent")).unwrap();
    fs::write(
        temp.path().join(".jig.toml"),
        r#"_src_path = "/tmp/template"
_commit = "abc123"
repo_name = "demo"
default_branch = "main"
jig_version = "0.2.0-beta.1"
web_package_manager = "/tmp/run-anything"
"#,
    )
    .unwrap();
    fs::write(
        temp.path().join(".agent/jig-contract.json"),
        serde_json::to_string_pretty(&json!({
            "contract_version": 3,
            "tool_namespace": "jig",
            "jig_version": "0.2.0-beta.1",
            "required_commands": ["contract_check_command"],
            "tools": [],
        }))
        .unwrap(),
    )
    .unwrap();

    let error = RepoContext::load_from(temp.path()).unwrap_err().to_string();

    assert!(error.contains("Unsupported web_package_manager"));
}

#[test]
fn template_dev_defaults_match_runtime_defaults() {
    let template = include_str!("../../../../../templates/project/.jig.toml.jinja");
    let defaults = DevConfig::default();

    assert!(template.contains(&format!("proxy_port = {}", defaults.proxy_port)));
    assert!(template.contains(&format!("https_port = {}", defaults.https_port.unwrap())));
    assert!(template.contains(&format!("https = {}", defaults.https)));
    assert!(template.contains(&format!("http2 = {}", defaults.http2)));
    assert!(template.contains(&format!("lan = {}", defaults.lan)));
    assert!(template.contains(&format!(r#"tld = "{}""#, defaults.tld)));
    assert!(template.contains(&format!(
        "workspace_discovery = {}",
        defaults.workspace_discovery
    )));
}

#[test]
fn unknown_dev_config_fields_are_rejected() {
    let temp = tempdir().unwrap();
    fs::create_dir_all(temp.path().join(".agent")).unwrap();
    fs::write(
        temp.path().join(".jig.toml"),
        r#"_src_path = "/tmp/template"
_commit = "abc123"
repo_name = "demo"
default_branch = "main"
jig_version = "0.2.0-beta.1"

[dev]
proxy_port = 1555
proxy_por = 1556
"#,
    )
    .unwrap();
    fs::write(
        temp.path().join(".agent/jig-contract.json"),
        serde_json::to_string_pretty(&json!({
            "contract_version": 3,
            "tool_namespace": "jig",
            "jig_version": "0.2.0-beta.1",
            "required_commands": ["contract_check_command"],
            "tools": [],
        }))
        .unwrap(),
    )
    .unwrap();

    let error = RepoContext::load_from(temp.path()).unwrap_err().to_string();
    assert!(error.contains("unknown field"));
    assert!(error.contains("proxy_por"));
}

#[test]
fn unknown_dev_app_config_fields_are_rejected() {
    let temp = tempdir().unwrap();
    fs::create_dir_all(temp.path().join(".agent")).unwrap();
    fs::write(
        temp.path().join(".jig.toml"),
        r#"_src_path = "/tmp/template"
_commit = "abc123"
repo_name = "demo"
default_branch = "main"
jig_version = "0.2.0-beta.1"

[[dev.apps]]
name = "web"
command = "bun run dev"
commmand = "typo"
"#,
    )
    .unwrap();
    fs::write(
        temp.path().join(".agent/jig-contract.json"),
        serde_json::to_string_pretty(&json!({
            "contract_version": 3,
            "tool_namespace": "jig",
            "jig_version": "0.2.0-beta.1",
            "required_commands": ["contract_check_command"],
            "tools": [],
        }))
        .unwrap(),
    )
    .unwrap();

    let error = RepoContext::load_from(temp.path()).unwrap_err().to_string();
    assert!(error.contains("unknown field"));
    assert!(error.contains("commmand"));
}

#[test]
fn unknown_top_level_config_fields_are_rejected() {
    let temp = tempdir().unwrap();
    fs::create_dir_all(temp.path().join(".agent")).unwrap();
    fs::write(
        temp.path().join(".jig.toml"),
        r#"_src_path = "/tmp/template"
_commit = "abc123"
repo_name = "demo"
default_branch = "main"
jig_version = "0.2.0-beta.1"
proxy_porrt = 1355
"#,
    )
    .unwrap();
    fs::write(
        temp.path().join(".agent/jig-contract.json"),
        serde_json::to_string_pretty(&json!({
            "contract_version": 3,
            "tool_namespace": "jig",
            "jig_version": "0.2.0-beta.1",
            "required_commands": ["contract_check_command"],
            "tools": [],
        }))
        .unwrap(),
    )
    .unwrap();

    let error = RepoContext::load_from(temp.path()).unwrap_err().to_string();
    assert!(error.contains("unknown field"));
    assert!(error.contains("proxy_porrt"));
}

#[test]
fn legacy_work_checks_are_merged_with_explicit_gates() {
    let temp = tempdir().unwrap();
    fs::create_dir_all(temp.path().join(".agent")).unwrap();
    fs::write(
        temp.path().join(".jig.toml"),
        r#"_src_path = "/tmp/template"
_commit = "abc123"
repo_name = "demo"
default_branch = "main"
jig_version = "0.2.0-beta.1"

[work]
checks = ["jig.contract_check", "jig.test"]

[[work.gates]]
id = "contract"
kind = "check"
tool = "jig.contract_check"
required = false
"#,
    )
    .unwrap();
    fs::write(
        temp.path().join(".agent/jig-contract.json"),
        serde_json::to_string_pretty(&json!({
            "contract_version": 3,
            "tool_namespace": "jig",
            "jig_version": "0.2.0-beta.1",
            "required_commands": ["contract_check_command", "rust_test_command"],
            "tools": [
                {
                    "name": "jig.contract_check",
                    "kind": "command",
                    "description": "Run contract check.",
                    "command": "contract_check_command"
                },
                {
                    "name": "jig.test",
                    "kind": "command",
                    "description": "Run tests.",
                    "command": "rust_test_command"
                }
            ],
        }))
        .unwrap(),
    )
    .unwrap();

    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let gates = ctx.work_gates();
    assert_eq!(gates.len(), 2);
    let WorkGate::Check(gate) = &gates[0] else {
        panic!("expected check gate");
    };
    assert_eq!(gate.id, "contract");
    assert_eq!(gate.tool, "jig.contract_check");
    assert!(!gate.required);
    let WorkGate::Check(gate) = &gates[1] else {
        panic!("expected check gate");
    };
    assert_eq!(gate.id, "test");
    assert_eq!(gate.tool, "jig.test");
    assert!(gate.required);
}

#[test]
fn work_refinements_are_loaded_for_refinement_execution() {
    let temp = tempdir().unwrap();
    fs::create_dir_all(temp.path().join(".agent")).unwrap();
    fs::write(
        temp.path().join(".jig.toml"),
        r#"_src_path = "/tmp/template"
_commit = "abc123"
repo_name = "demo"
default_branch = "main"
jig_version = "0.2.0-beta.1"

[[work.refinements]]
id = "rust-simplify"
skill = "jig-rust:rust-simplify"
"#,
    )
    .unwrap();
    fs::write(
        temp.path().join(".agent/jig-contract.json"),
        serde_json::to_string_pretty(&json!({
            "contract_version": 3,
            "tool_namespace": "jig",
            "jig_version": "0.2.0-beta.1",
            "required_commands": ["contract_check_command"],
            "tools": [],
        }))
        .unwrap(),
    )
    .unwrap();

    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let refinements = ctx.work_refinements();
    assert_eq!(refinements.len(), 1);
    assert_eq!(refinements[0].id, "rust-simplify");
    assert_eq!(
        refinements[0].skill.as_deref(),
        Some("jig-rust:rust-simplify")
    );
}

#[test]
fn multiple_work_refinements_are_rejected_until_selection_exists() {
    let temp = tempdir().unwrap();
    fs::create_dir_all(temp.path().join(".agent")).unwrap();
    fs::write(
        temp.path().join(".jig.toml"),
        r#"_src_path = "/tmp/template"
_commit = "abc123"
repo_name = "demo"
default_branch = "main"
jig_version = "0.2.0-beta.1"

[[work.refinements]]
id = "rust-simplify"

[[work.refinements]]
id = "rust-security"
"#,
    )
    .unwrap();
    fs::write(
        temp.path().join(".agent/jig-contract.json"),
        serde_json::to_string_pretty(&json!({
            "contract_version": 3,
            "tool_namespace": "jig",
            "jig_version": "0.2.0-beta.1",
            "required_commands": [],
            "tools": [],
        }))
        .unwrap(),
    )
    .unwrap();

    let error = RepoContext::load_from(temp.path()).unwrap_err().to_string();

    assert!(error.contains("Only one [[work.refinements]] entry is supported"));
}
