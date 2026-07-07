use std::fs;

use serde_json::json;
use tempfile::tempdir;

use super::*;
use crate::context::DevConfig;

fn write_contract(root: &std::path::Path) {
    fs::create_dir_all(root.join(".agent")).unwrap();
    fs::write(
        root.join(".agent/jig-contract.json"),
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
}

fn write_config(root: &std::path::Path, extra: &str) {
    write_contract(root);
    fs::write(
        root.join(".jig.toml"),
        format!(
            r#"_src_path = "/tmp/template"
_commit = "abc123"
repo_name = "demo"
default_branch = "main"
jig_version = "0.2.0-beta.1"
{extra}
"#
        ),
    )
    .unwrap();
}

#[test]
fn dev_config_defaults_match_proxy_settings_defaults() {
    let dev = DevConfig::default();
    let proxy = jig_dev_proxy::ProxySettings::default();

    assert_eq!(dev.proxy_port, proxy.http_port);
    assert_eq!(dev.https_port, proxy.https_port);
    assert_eq!(dev.tld, proxy.tld);
}

#[test]
fn dev_apps_take_precedence_when_matching_frontend_apps_are_also_configured() {
    let temp = tempdir().unwrap();
    write_contract(temp.path());
    fs::create_dir_all(temp.path().join("apps/web")).unwrap();
    fs::write(
        temp.path().join(".jig.toml"),
        r#"_src_path = "/tmp/template"
_commit = "abc123"
repo_name = "demo"
default_branch = "main"
jig_version = "0.2.0-beta.1"
web_package_manager = "bun"

[[frontend_apps]]
name = "web"
dir = "apps/web"
coverage_threshold = 80

[dev]
proxy_port = 1555

[[dev.apps]]
name = "web"
kind = "vite"
dir = "apps/web"
argv = ["bun", "run", "dev"]
"#,
    )
    .unwrap();

    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let settings = settings(&ctx, &ProxyRuntimeOptions::default()).unwrap();
    let apps = configured_apps(&ctx, &settings).unwrap();

    assert_eq!(apps.len(), 1);
    assert_eq!(apps[0].name, "web");
    assert!(matches!(
        &apps[0].command,
        jig_dev_proxy::CommandSpec::Argv(argv)
            if argv == &vec!["bun".to_string(), "run".to_string(), "dev".to_string()]
    ));
}

#[test]
fn dev_apps_reject_unmatched_frontend_apps_when_both_sections_are_configured() {
    let temp = tempdir().unwrap();
    write_contract(temp.path());
    fs::create_dir_all(temp.path().join("apps/legacy-web")).unwrap();
    fs::create_dir_all(temp.path().join("apps/web")).unwrap();
    fs::write(
        temp.path().join(".jig.toml"),
        r#"_src_path = "/tmp/template"
_commit = "abc123"
repo_name = "demo"
default_branch = "main"
jig_version = "0.2.0-beta.1"
web_package_manager = "bun"

[[frontend_apps]]
name = "legacy-web"
dir = "apps/legacy-web"
coverage_threshold = 80

[dev]
proxy_port = 1555

[[dev.apps]]
name = "web"
kind = "vite"
dir = "apps/web"
argv = ["bun", "run", "dev"]
"#,
    )
    .unwrap();

    let error = RepoContext::load_from(temp.path()).unwrap_err().to_string();

    assert!(error.contains("Add a matching [[dev.apps]] entry"));
    assert!(error.contains("legacy-web"));
}

#[test]
fn dev_apps_reject_mismatched_frontend_app_dirs_when_both_sections_are_configured() {
    let temp = tempdir().unwrap();
    write_contract(temp.path());
    fs::create_dir_all(temp.path().join("apps/web")).unwrap();
    fs::create_dir_all(temp.path().join("frontend/web")).unwrap();
    fs::write(
        temp.path().join(".jig.toml"),
        r#"_src_path = "/tmp/template"
_commit = "abc123"
repo_name = "demo"
default_branch = "main"
jig_version = "0.2.0-beta.1"
web_package_manager = "bun"

[[frontend_apps]]
name = "web"
dir = "apps/web"
coverage_threshold = 80

[dev]
proxy_port = 1555

[[dev.apps]]
name = "web"
kind = "vite"
dir = "frontend/web"
argv = ["bun", "run", "dev"]
"#,
    )
    .unwrap();

    let error = RepoContext::load_from(temp.path()).unwrap_err().to_string();

    assert!(error.contains("uses dir 'frontend/web'"));
    assert!(error.contains("matching [[frontend_apps]] uses 'apps/web'"));
}

#[test]
fn legacy_frontend_apps_are_used_when_dev_apps_are_absent() {
    let temp = tempdir().unwrap();
    write_contract(temp.path());
    fs::create_dir_all(temp.path().join("apps/web")).unwrap();
    fs::write(
        temp.path().join(".jig.toml"),
        r#"_src_path = "/tmp/template"
_commit = "abc123"
repo_name = "demo"
default_branch = "main"
jig_version = "0.2.0-beta.1"
web_package_manager = "pnpm"

[[frontend_apps]]
name = "web"
dir = "apps/web"
coverage_threshold = 80

[dev]
proxy_port = 1555
"#,
    )
    .unwrap();

    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let settings = settings(&ctx, &ProxyRuntimeOptions::default()).unwrap();
    let apps = configured_apps(&ctx, &settings).unwrap();

    assert_eq!(apps.len(), 1);
    assert_eq!(apps[0].name, "web");
    assert_eq!(apps[0].kind, jig_dev_proxy::AppKind::Vite);
    assert!(matches!(
        &apps[0].command,
        jig_dev_proxy::CommandSpec::Argv(argv)
            if argv == &vec!["pnpm".to_string(), "run".to_string(), "dev".to_string()]
    ));
}

#[test]
fn unknown_dev_app_kind_is_rejected() {
    let temp = tempdir().unwrap();
    write_contract(temp.path());
    fs::write(
        temp.path().join(".jig.toml"),
        r#"_src_path = "/tmp/template"
_commit = "abc123"
repo_name = "demo"
default_branch = "main"
jig_version = "0.2.0-beta.1"
web_package_manager = "bun"

[dev]
proxy_port = 1555

[[dev.apps]]
name = "web"
kind = "vit"
command = "bun run dev"
"#,
    )
    .unwrap();

    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let settings = settings(&ctx, &ProxyRuntimeOptions::default()).unwrap();
    let error = configured_apps(&ctx, &settings).unwrap_err().to_string();

    assert!(error.contains("Unsupported dev app kind"));
}

#[test]
fn dev_app_host_must_be_ip_literal() {
    let temp = tempdir().unwrap();
    write_contract(temp.path());
    fs::write(
        temp.path().join(".jig.toml"),
        r#"_src_path = "/tmp/template"
_commit = "abc123"
repo_name = "demo"
default_branch = "main"
jig_version = "0.2.0-beta.1"
web_package_manager = "bun"

[dev]
proxy_port = 1555

[[dev.apps]]
name = "web"
host = "api.example.test"
command = "bun run dev"
"#,
    )
    .unwrap();

    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let settings = settings(&ctx, &ProxyRuntimeOptions::default()).unwrap();
    let error = configured_apps(&ctx, &settings).unwrap_err().to_string();

    assert!(error.contains("must be an IP literal"));
}

#[test]
fn proxied_dev_app_host_must_be_loopback() {
    let temp = tempdir().unwrap();
    write_contract(temp.path());
    fs::write(
        temp.path().join(".jig.toml"),
        r#"_src_path = "/tmp/template"
_commit = "abc123"
repo_name = "demo"
default_branch = "main"
jig_version = "0.2.0-beta.1"
web_package_manager = "bun"

[[dev.apps]]
name = "web"
host = "192.0.2.10"
command = "bun run dev"
"#,
    )
    .unwrap();

    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let settings = settings(&ctx, &ProxyRuntimeOptions::default()).unwrap();
    let error = configured_apps(&ctx, &settings).unwrap_err().to_string();

    assert!(error.contains("must target a loopback IP literal"));
}

#[test]
fn non_proxied_dev_app_may_use_non_loopback_direct_host() {
    let temp = tempdir().unwrap();
    write_contract(temp.path());
    fs::write(
        temp.path().join(".jig.toml"),
        r#"_src_path = "/tmp/template"
_commit = "abc123"
repo_name = "demo"
default_branch = "main"
jig_version = "0.2.0-beta.1"
web_package_manager = "bun"

[[dev.apps]]
name = "web"
host = "192.0.2.10"
proxy = false
command = "bun run dev"
"#,
    )
    .unwrap();

    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let settings = settings(&ctx, &ProxyRuntimeOptions::default()).unwrap();
    let apps = configured_apps(&ctx, &settings).unwrap();

    assert_eq!(apps[0].target_host, "192.0.2.10");
    assert!(!apps[0].proxy);
}

#[test]
fn dev_app_name_rejects_surrounding_whitespace() {
    let temp = tempdir().unwrap();
    write_contract(temp.path());
    fs::write(
        temp.path().join(".jig.toml"),
        r#"_src_path = "/tmp/template"
_commit = "abc123"
repo_name = "demo"
default_branch = "main"
jig_version = "0.2.0-beta.1"
web_package_manager = "bun"

[[dev.apps]]
name = " web "
command = "bun run dev"
"#,
    )
    .unwrap();

    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let settings = settings(&ctx, &ProxyRuntimeOptions::default()).unwrap();
    let error = configured_apps(&ctx, &settings).unwrap_err().to_string();

    assert!(error.contains("must not contain leading or trailing whitespace"));
}

#[test]
fn dev_app_dirs_must_stay_under_repo_root() {
    let temp = tempdir().unwrap();
    write_contract(temp.path());
    let outside = tempdir().unwrap();
    fs::write(
        temp.path().join(".jig.toml"),
        format!(
            r#"_src_path = "/tmp/template"
_commit = "abc123"
repo_name = "demo"
default_branch = "main"
jig_version = "0.2.0-beta.1"

[[dev.apps]]
name = "web"
dir = "{}"
command = "bun run dev"
"#,
            outside.path().display()
        ),
    )
    .unwrap();

    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let settings = settings(&ctx, &ProxyRuntimeOptions::default()).unwrap();
    let error = configured_apps(&ctx, &settings).unwrap_err().to_string();

    assert!(error.contains("resolves outside repo root"));
}

#[test]
fn settings_do_not_require_configured_app_dirs_to_exist() {
    let temp = tempdir().unwrap();
    write_contract(temp.path());
    fs::write(
        temp.path().join(".jig.toml"),
        r#"_src_path = "/tmp/template"
_commit = "abc123"
repo_name = "demo"
default_branch = "main"
jig_version = "0.2.0-beta.1"

[[dev.apps]]
name = "web"
dir = "missing-app-dir"
command = "bun run dev"
"#,
    )
    .unwrap();

    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let settings = settings(&ctx, &ProxyRuntimeOptions::default()).unwrap();
    let error = configured_apps(&ctx, &settings).unwrap_err().to_string();

    assert!(error.contains("dev app dir"));
    assert!(error.contains("must exist"));
}

#[test]
fn vite_dev_app_requires_argv() {
    let temp = tempdir().unwrap();
    write_contract(temp.path());
    fs::write(
        temp.path().join(".jig.toml"),
        r#"_src_path = "/tmp/template"
_commit = "abc123"
repo_name = "demo"
default_branch = "main"
jig_version = "0.2.0-beta.1"
web_package_manager = "bun"

[dev]
proxy_port = 1555

[[dev.apps]]
name = "web"
kind = "vite"
command = "bun run dev"
"#,
    )
    .unwrap();

    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let settings = settings(&ctx, &ProxyRuntimeOptions::default()).unwrap();
    let error = configured_apps(&ctx, &settings).unwrap_err().to_string();

    assert!(error.contains("must set argv"));
}

#[test]
fn invalid_dev_tld_is_rejected() {
    let temp = tempdir().unwrap();
    write_contract(temp.path());
    fs::write(
        temp.path().join(".jig.toml"),
        r#"_src_path = "/tmp/template"
_commit = "abc123"
repo_name = "demo"
default_branch = "main"
jig_version = "0.2.0-beta.1"

[dev]
tld = "bad,tld"
"#,
    )
    .unwrap();

    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let error = settings(&ctx, &ProxyRuntimeOptions::default())
        .unwrap_err()
        .to_string();

    assert!(error.contains("invalid hostname"));
}

#[test]
fn public_dev_tld_is_rejected() {
    let temp = tempdir().unwrap();
    write_contract(temp.path());
    fs::write(
        temp.path().join(".jig.toml"),
        r#"_src_path = "/tmp/template"
_commit = "abc123"
repo_name = "demo"
default_branch = "main"
jig_version = "0.2.0-beta.1"

[dev]
tld = "dev"
"#,
    )
    .unwrap();

    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let error = settings(&ctx, &ProxyRuntimeOptions::default())
        .unwrap_err()
        .to_string();

    assert!(error.contains("is not allowed"));
}

#[test]
fn zero_proxy_ports_are_rejected() {
    let temp = tempdir().unwrap();
    write_contract(temp.path());
    fs::write(
        temp.path().join(".jig.toml"),
        r#"_src_path = "/tmp/template"
_commit = "abc123"
repo_name = "demo"
default_branch = "main"
jig_version = "0.2.0-beta.1"

[dev]
proxy_port = 0
"#,
    )
    .unwrap();

    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let error = settings(&ctx, &ProxyRuntimeOptions::default())
        .unwrap_err()
        .to_string();

    assert!(error.contains("proxy HTTP port must be greater than 0"));
}

#[test]
fn explicit_read_only_state_dir_must_exist() {
    let temp = tempdir().unwrap();
    write_contract(temp.path());
    fs::write(
        temp.path().join(".jig.toml"),
        r#"_src_path = "/tmp/template"
_commit = "abc123"
repo_name = "demo"
default_branch = "main"
jig_version = "0.2.0-beta.1"
"#,
    )
    .unwrap();
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let opts = ProxyRuntimeOptions {
        state_dir: Some(temp.path().join("missing-state")),
        ..ProxyRuntimeOptions::default()
    };

    let error = settings_existing_state_dir(&ctx, &opts)
        .unwrap_err()
        .to_string();

    assert!(error.contains("does not exist"));
}

#[cfg(unix)]
#[test]
fn explicit_read_only_state_dir_reports_inspection_errors() {
    let temp = tempdir().unwrap();
    let loop_path = temp.path().join("loop-state");
    std::os::unix::fs::symlink(&loop_path, &loop_path).unwrap();
    let settings = jig_dev_proxy::ProxySettings {
        state_dir: Some(loop_path.clone()),
        ..jig_dev_proxy::ProxySettings::default()
    };

    let error = require_existing_state_dir(settings)
        .unwrap_err()
        .to_string();

    assert!(error.contains("Failed to inspect proxy state dir"));
    assert!(error.contains(&loop_path.display().to_string()));
}

#[test]
fn settings_does_not_create_missing_state_dir() {
    let temp = tempdir().unwrap();
    write_contract(temp.path());
    fs::write(
        temp.path().join(".jig.toml"),
        r#"_src_path = "/tmp/template"
_commit = "abc123"
repo_name = "demo"
default_branch = "main"
jig_version = "0.2.0-beta.1"
"#,
    )
    .unwrap();
    let missing = temp.path().join("missing-state");
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let opts = ProxyRuntimeOptions {
        state_dir: Some(missing.clone()),
        ..ProxyRuntimeOptions::default()
    };

    let settings = settings(&ctx, &opts).unwrap();

    assert_eq!(settings.state_dir.as_deref(), Some(missing.as_path()));
    assert!(!missing.exists());
}

#[test]
fn service_status_settings_allow_missing_state_dir() {
    let temp = tempdir().unwrap();
    write_contract(temp.path());
    fs::write(
        temp.path().join(".jig.toml"),
        r#"_src_path = "/tmp/template"
_commit = "abc123"
repo_name = "demo"
default_branch = "main"
jig_version = "0.2.0-beta.1"
"#,
    )
    .unwrap();
    let missing = temp.path().join("missing-state");
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let opts = ProxyRuntimeOptions {
        state_dir: Some(missing.clone()),
        ..ProxyRuntimeOptions::default()
    };

    let settings = service_status_settings(&ctx, &opts).unwrap();

    assert_eq!(settings.state_dir.as_deref(), Some(missing.as_path()));
    assert!(!missing.exists());
}

#[test]
fn contextless_service_status_settings_allow_missing_state_dir() {
    let temp = tempdir().unwrap();
    let missing = temp.path().join("missing-state");
    let opts = ProxyRuntimeOptions {
        state_dir: Some(missing.clone()),
        ..ProxyRuntimeOptions::default()
    };

    let settings = service_status_settings_without_context(&opts).unwrap();

    assert_eq!(settings.state_dir.as_deref(), Some(missing.as_path()));
    assert!(!missing.exists());
}

#[test]
fn service_blocked_detail_uses_nested_manager_stderr() {
    let output = json!({
        "ok": false,
        "service": {
            "ok": false,
            "show": {
                "ok": false,
                "status": 1,
                "stderr": "Failed to connect to bus: No medium found",
            },
        },
    });

    let detail = service_blocked_detail(&output, "service is not active");

    assert_eq!(detail, "Failed to connect to bus: No medium found");
}

#[test]
fn service_blocked_detail_reports_nested_manager_timeout() {
    let output = json!({
        "ok": false,
        "load": {
            "ok": false,
            "daemon_reload": {
                "ok": false,
                "timed_out": true,
                "stdout": "",
                "stderr": "",
            },
        },
    });

    let detail = service_blocked_detail(&output, "service install did not complete");

    assert_eq!(detail, "service manager command timed out");
}

#[test]
fn service_blocked_detail_ignores_successful_nested_stderr_before_later_failure() {
    let output = json!({
        "ok": false,
        "unload": {
            "ok": false,
            "bootout": {
                "ok": true,
                "status": 5,
                "stderr": "Bootstrap failed: 5: Input/output error\nservice is not loaded",
            },
        },
        "reload": {
            "ok": false,
            "daemon_reload": {
                "ok": false,
                "status": 1,
                "stderr": "Failed to reload service manager",
            },
        },
    });

    let detail = service_blocked_detail(&output, "service uninstall did not complete");

    assert_eq!(detail, "Failed to reload service manager");
}

#[test]
fn no_proxy_rejects_proxy_runtime_flags() {
    let opts = ProxyRuntimeOptions {
        https: true,
        tld: Some("localhost".into()),
        ..ProxyRuntimeOptions::default()
    };

    let error = reject_no_proxy_runtime_flags(true, &opts)
        .unwrap_err()
        .to_string();

    assert!(error.contains("--no-proxy cannot be combined"));
    assert!(error.contains("--https"));
    assert!(error.contains("--tld"));
}

#[test]
fn no_proxy_allows_state_dir_for_other_proxy_commands() {
    let opts = ProxyRuntimeOptions {
        state_dir: Some(PathBuf::from("/tmp/jig-proxy-state")),
        ..ProxyRuntimeOptions::default()
    };

    reject_no_proxy_runtime_flags(true, &opts).unwrap();
}

#[test]
fn contextless_proxy_commands_are_limited_to_host_cleanup_and_status() {
    assert!(commands::can_run_without_context(&ProxyCommand::Stop(
        ProxyStopRequest::default()
    )));
    assert!(commands::can_run_without_context(&ProxyCommand::Service(
        ProxyServiceCommand::Status(ProxyServiceRuntimeRequest::default())
    )));
    assert!(commands::can_run_without_context(&ProxyCommand::Start(
        ProxyStartRequest {
            foreground: true,
            proxy: ProxyRuntimeOptions::default(),
        }
    )));
    assert!(!commands::can_run_without_context(&ProxyCommand::Start(
        ProxyStartRequest {
            foreground: false,
            proxy: ProxyRuntimeOptions::default(),
        }
    )));
    assert!(!commands::can_run_without_context(&ProxyCommand::Cert(
        ProxyCertCommand::Generate(ProxyCertGenerateRequest::default())
    )));
}

#[test]
fn contextless_proxy_allowlist_is_exhaustive() {
    let commands = proxy_command_cases();
    let allowed = commands
        .iter()
        .filter_map(|command| {
            commands::can_run_without_context(command).then_some(proxy_command_case_name(command))
        })
        .collect::<Vec<_>>();

    assert_eq!(
        allowed,
        vec![
            "start:foreground",
            "stop",
            "list",
            "prune",
            "cert:status",
            "cert:trust",
            "cert:untrust",
            "service:uninstall",
            "service:status",
        ]
    );
}

fn proxy_command_cases() -> Vec<ProxyCommand> {
    vec![
        ProxyCommand::Start(ProxyStartRequest {
            foreground: true,
            proxy: ProxyRuntimeOptions::default(),
        }),
        ProxyCommand::Start(ProxyStartRequest {
            foreground: false,
            proxy: ProxyRuntimeOptions::default(),
        }),
        ProxyCommand::Stop(ProxyStopRequest::default()),
        ProxyCommand::List(ProxyListRequest::default()),
        ProxyCommand::Prune(ProxyPruneRequest::default()),
        ProxyCommand::Run(ProxyRunRequest {
            name: "web".into(),
            kind: None,
            dir: None,
            port: Some(3000),
            no_proxy: false,
            proxy: ProxyRuntimeOptions::default(),
            command: vec!["npm".into(), "run".into(), "dev".into()],
        }),
        ProxyCommand::Alias(ProxyAliasRequest {
            name: "web".into(),
            port: 3000,
            host: "127.0.0.1".into(),
            accept_non_loopback_target: false,
            proxy: ProxyRuntimeOptions::default(),
        }),
        ProxyCommand::Cert(ProxyCertCommand::Generate(
            ProxyCertGenerateRequest::default(),
        )),
        ProxyCommand::Cert(ProxyCertCommand::Status(ProxyCertRuntimeRequest::default())),
        ProxyCommand::Cert(ProxyCertCommand::Trust(ProxyCertTrustRequest {
            accept_trust_scope: true,
            proxy: ProxyRuntimeOptions::default(),
        })),
        ProxyCommand::Cert(ProxyCertCommand::Untrust(ProxyCertUntrustRequest {
            accept_trust_scope: true,
            proxy: ProxyRuntimeOptions::default(),
        })),
        ProxyCommand::Service(ProxyServiceCommand::Install(ProxyServiceInstallRequest {
            accept_service_scope: true,
            proxy: ProxyRuntimeOptions::default(),
        })),
        ProxyCommand::Service(ProxyServiceCommand::Uninstall(
            ProxyServiceRuntimeRequest::default(),
        )),
        ProxyCommand::Service(ProxyServiceCommand::Status(
            ProxyServiceRuntimeRequest::default(),
        )),
    ]
}

fn proxy_command_case_name(command: &ProxyCommand) -> &'static str {
    match command {
        ProxyCommand::Start(opts) if opts.foreground => "start:foreground",
        ProxyCommand::Start(_) => "start:background",
        ProxyCommand::Stop(_) => "stop",
        ProxyCommand::List(_) => "list",
        ProxyCommand::Prune(_) => "prune",
        ProxyCommand::Run(_) => "run",
        ProxyCommand::Alias(_) => "alias",
        ProxyCommand::Cert(ProxyCertCommand::Generate(_)) => "cert:generate",
        ProxyCommand::Cert(ProxyCertCommand::Status(_)) => "cert:status",
        ProxyCommand::Cert(ProxyCertCommand::Trust(_)) => "cert:trust",
        ProxyCommand::Cert(ProxyCertCommand::Untrust(_)) => "cert:untrust",
        ProxyCommand::Service(ProxyServiceCommand::Install(_)) => "service:install",
        ProxyCommand::Service(ProxyServiceCommand::Uninstall(_)) => "service:uninstall",
        ProxyCommand::Service(ProxyServiceCommand::Status(_)) => "service:status",
    }
}

#[test]
fn contextless_proxy_settings_use_runtime_flags() {
    let temp = tempdir().unwrap();
    let settings = settings_without_context(&ProxyRuntimeOptions {
        state_dir: Some(temp.path().to_path_buf()),
        http_port: Some(1555),
        https_port: Some(1556),
        https: true,
        no_https: false,
        http2: false,
        no_http2: true,
        lan: true,
        no_lan: false,
        tld: Some("Test".into()),
    })
    .unwrap();

    assert_eq!(settings.state_dir, Some(temp.path().to_path_buf()));
    assert_eq!(settings.http_port, 1555);
    assert_eq!(settings.https_port, Some(1556));
    assert!(settings.https);
    assert!(!settings.http2);
    assert!(settings.lan);
    assert_eq!(settings.tld, "test");
    assert!(settings.additional_dns_names.is_empty());
}

#[test]
fn proxy_runtime_flags_can_disable_configured_https_and_lan() {
    let temp = tempdir().unwrap();
    write_config(
        temp.path(),
        r#"
[dev]
https = true
lan = true
"#,
    );
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let settings = settings(
        &ctx,
        &ProxyRuntimeOptions {
            no_https: true,
            no_lan: true,
            ..ProxyRuntimeOptions::default()
        },
    )
    .unwrap();

    assert!(!settings.https);
    assert!(!settings.lan);
}

#[test]
fn proxy_http_and_https_ports_must_differ() {
    let temp = tempdir().unwrap();
    write_contract(temp.path());
    fs::write(
        temp.path().join(".jig.toml"),
        r#"_src_path = "/tmp/template"
_commit = "abc123"
repo_name = "demo"
default_branch = "main"
jig_version = "0.2.0-beta.1"

[dev]
proxy_port = 1555
https_port = 1555
"#,
    )
    .unwrap();

    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let error = settings(&ctx, &ProxyRuntimeOptions::default())
        .unwrap_err()
        .to_string();

    assert!(error.contains("must be different"));
}
