use super::*;

#[test]
fn dev_config_defaults_and_apps_are_loaded() {
    let temp = tempdir().unwrap();
    fs::create_dir_all(temp.path().join(".agent")).unwrap();
    fs::write(
        temp.path().join(".jig.toml"),
        r#"_src_path = "/tmp/template"
_commit = "abc123"
repo_name = "demo"
default_branch = "main"
jig_version = "0.2.0-beta.1"
dev_command = "cargo run"
web_package_manager = "pnpm"

[dev]
proxy_port = 1555
https = true
workspace_discovery = true

[[dev.apps]]
name = "api"
kind = "env-port"
command = "cargo run --bin api"
port = 4545

[[dev.apps]]
name = "web"
kind = "vite"
dir = "apps/web"
argv = ["pnpm", "run", "dev"]
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
    assert_eq!(ctx.web_package_manager(), "pnpm");
    assert_eq!(ctx.dev_config().proxy_port, 1555);
    assert!(ctx.dev_config().https);
    assert!(ctx.dev_config().workspace_discovery);
    assert_eq!(ctx.dev_config().apps.len(), 2);
    assert_eq!(ctx.dev_config().apps[0].name, "api");
    assert_eq!(ctx.dev_config().apps[0].port, Some(4545));
    assert_eq!(ctx.dev_config().apps[1].argv, vec!["pnpm", "run", "dev"]);
}

#[test]
fn duplicate_dev_app_names_are_rejected_at_config_load() {
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

[[dev.apps]]
name = "web"
command = "bun run dev"
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

    assert!(error.contains("Duplicate dev app name"));
}

#[test]
fn duplicate_dev_app_env_prefixes_are_rejected_at_config_load() {
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
name = "web-app"
command = "bun run dev"

[[dev.apps]]
name = "web_app"
command = "bun run dev"
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

    assert!(error.contains("share derived dev environment prefix JIG_DEV_WEB_APP"));
}

#[test]
fn matched_frontend_dev_app_requires_same_dir_at_config_load() {
    let config: RepoConfig = toml::from_str(
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
kind = "vite"
argv = ["npm", "run", "dev"]
"#,
    )
    .unwrap();

    let error = validate_config(&config).unwrap_err().to_string();

    assert!(error.contains("[dev.apps] entry 'web' matches [[frontend_apps]]"));
    assert!(error.contains("must set dir = 'apps/web'"));
}

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
fn template_dev_settings_are_rendered_from_answer_authority() {
    let template = include_str!("../../../../templates/project/.jig.toml.jinja");

    for placeholder in [
        "proxy_port = <<[ dev.proxy_port ]>>",
        "https_port = <<[ dev.https_port ]>>",
        "https = <<[ dev.https ]>>",
        "http2 = <<[ dev.http2 ]>>",
        "lan = <<[ dev.lan ]>>",
        "tld = \"<<[ dev.tld | replace",
        "workspace_discovery = <<[ dev.workspace_discovery ]>>",
    ] {
        assert!(template.contains(placeholder), "missing {placeholder}");
    }
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

    let error = format!("{:#}", RepoContext::load_from(temp.path()).unwrap_err());
    assert!(error.contains("unknown field"));
    assert!(error.contains("proxy_por"));
}
