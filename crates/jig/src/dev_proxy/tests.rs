use std::fs;

use serde_json::json;
use tempfile::tempdir;

use super::*;
use crate::context::DevConfig;
use crate::test_env::{EnvVarGuard, lock_env};
#[cfg(any(target_os = "linux", target_os = "macos"))]
use crate::test_process::{read_test_process_identity, terminate_and_confirm_test_process};

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

fn write_dependency_checker(root: &std::path::Path) {
    fs::create_dir_all(root.join("scripts")).unwrap();
    fs::write(
        root.join("scripts/check-webapps.sh"),
        r#"#!/usr/bin/env bash
set -euo pipefail
if [ "${1:-}" != "dependencies-ready" ] || [ "$#" -ne 2 ]; then
  echo "Usage: scripts/check-webapps.sh dependencies-ready <app-dir>" >&2
  exit 2
fi
[ -f ".agent/tmp/test-ready/$2" ]
"#,
    )
    .unwrap();
}

fn mark_frontend_dependencies_ready(root: &std::path::Path, app_dir: &str) {
    let marker = root.join(".agent/tmp/test-ready").join(app_dir);
    fs::create_dir_all(marker.parent().unwrap()).unwrap();
    fs::write(marker, "ready\n").unwrap();
}

fn preflight_configured_apps(ctx: &RepoContext, selected_apps: &[&str]) -> Result<()> {
    let settings = settings(ctx, &ProxyRuntimeOptions::default())?;
    let apps = configured_apps(ctx, &settings)?;
    let request = jig_dev_proxy::DevRequest::new(
        ctx.repo_name(),
        ctx.root().to_path_buf(),
        ctx.web_package_manager(),
        settings,
    )
    .with_apps(apps)
    .with_selected_apps(
        selected_apps
            .iter()
            .map(|name| (*name).to_string())
            .collect(),
    );
    let request = jig_dev_proxy::resolve_dev_request(request)?;
    ensure_frontend_dependencies(ctx, request.apps(), &|| false)
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
fn dev_interruption_is_distinct_from_json_success() {
    let output = json!({
        "ok": false,
        "interrupted": true,
        "exit_status": 130
    });

    assert!(dev_interrupted(&output));
    assert!(!json_ok(&output));
    assert!(!dev_interrupted(&json!({ "ok": false })));
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
fn frontend_dependency_preflight_directs_fresh_repos_to_bootstrap() {
    let temp = tempdir().unwrap();
    fs::create_dir_all(temp.path().join("web")).unwrap();
    write_config(
        temp.path(),
        r#"bootstrap_command = "scripts/check-webapps.sh bootstrap"

[[frontend_apps]]
name = "web"
dir = "web"
coverage_threshold = 80
"#,
    );
    write_dependency_checker(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let error = preflight_configured_apps(&ctx, &[])
        .unwrap_err()
        .to_string();

    assert!(error.contains("Frontend dependencies are missing or stale for web"));
    assert!(error.contains("scripts/jig bootstrap"));
    assert!(error.contains("does not install packages implicitly"));
}

#[test]
fn frontend_dependency_preflight_requires_checker_receipt_not_install_artifacts() {
    for artifact in ["node_modules", ".pnp.cjs"] {
        let temp = tempdir().unwrap();
        fs::create_dir_all(temp.path().join("web")).unwrap();
        write_config(
            temp.path(),
            r#"[[frontend_apps]]
name = "web"
dir = "web"
coverage_threshold = 80
"#,
        );
        write_dependency_checker(temp.path());
        let artifact_path = temp.path().join(artifact);
        if artifact == "node_modules" {
            fs::create_dir(&artifact_path).unwrap();
        } else {
            fs::write(&artifact_path, "generated pnp loader").unwrap();
        }
        let ctx = RepoContext::load_from(temp.path()).unwrap();

        assert!(preflight_configured_apps(&ctx, &[]).is_err());
        mark_frontend_dependencies_ready(temp.path(), "web");
        preflight_configured_apps(&ctx, &[]).unwrap();
    }

    let temp = tempdir().unwrap();
    fs::create_dir_all(temp.path().join("web/node_modules")).unwrap();
    write_config(
        temp.path(),
        r#"[[frontend_apps]]
name = "web"
dir = "web"
coverage_threshold = 80
        "#,
    );
    write_dependency_checker(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    assert!(preflight_configured_apps(&ctx, &[]).is_err());
    mark_frontend_dependencies_ready(temp.path(), "web");
    preflight_configured_apps(&ctx, &[]).unwrap();
}

#[test]
fn frontend_dependency_preflight_only_checks_selected_frontends() {
    let temp = tempdir().unwrap();
    fs::create_dir_all(temp.path().join("web/node_modules")).unwrap();
    write_config(
        temp.path(),
        r#"[[frontend_apps]]
name = "web"
dir = "web"
coverage_threshold = 80

[[frontend_apps]]
name = "admin"
dir = "admin"
coverage_threshold = 80
        "#,
    );
    write_dependency_checker(temp.path());
    mark_frontend_dependencies_ready(temp.path(), "web");
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    preflight_configured_apps(&ctx, &["web"]).unwrap();
    let error = preflight_configured_apps(&ctx, &["admin"])
        .unwrap_err()
        .to_string();
    assert!(error.contains("admin"));
    assert!(error.contains("must exist"));
    assert!(preflight_configured_apps(&ctx, &[]).is_err());
}

#[test]
fn explicit_selection_ignores_missing_unselected_dev_app_dir() {
    let temp = tempdir().unwrap();
    fs::create_dir_all(temp.path().join("web")).unwrap();
    write_config(
        temp.path(),
        r#"[[dev.apps]]
name = "web"
dir = "web"
command = "cargo run"
proxy = false

[[dev.apps]]
name = "stale"
dir = "missing-stale"
command = "cargo run"
proxy = false
"#,
    );
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    preflight_configured_apps(&ctx, &["web"]).unwrap();
    let selected_error = preflight_configured_apps(&ctx, &["stale"])
        .unwrap_err()
        .to_string();
    assert!(selected_error.contains("missing-stale"));
    assert!(selected_error.contains("must exist"));
    assert!(preflight_configured_apps(&ctx, &[]).is_err());
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn frontend_dependency_preflight_uses_resolved_app_after_config_alias_is_removed() {
    use std::os::unix::fs::symlink;

    let temp = tempdir().unwrap();
    fs::create_dir(temp.path().join("real-web")).unwrap();
    symlink("real-web", temp.path().join("web-alias")).unwrap();
    write_config(
        temp.path(),
        r#"web_package_manager = "bun"

[[frontend_apps]]
name = "web"
dir = "web-alias"
coverage_threshold = 80

[[dev.apps]]
name = "web"
kind = "vite"
dir = "web-alias"
argv = ["bun", "run", "dev"]
"#,
    );
    write_dependency_checker(temp.path());
    mark_frontend_dependencies_ready(temp.path(), "real-web");
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let settings = settings(&ctx, &ProxyRuntimeOptions::default()).unwrap();
    let apps = configured_apps(&ctx, &settings).unwrap();
    let request = jig_dev_proxy::DevRequest::new(
        ctx.repo_name(),
        ctx.root().to_path_buf(),
        ctx.web_package_manager(),
        settings,
    )
    .with_apps(apps)
    .with_selected_apps(vec!["web".into()]);
    let request = jig_dev_proxy::resolve_dev_request(request).unwrap();

    fs::remove_file(temp.path().join("web-alias")).unwrap();
    assert!(temp.path().join("real-web").is_dir());

    ensure_frontend_dependencies(&ctx, request.apps(), &|| false).unwrap();
}

#[test]
fn frontend_dependency_preflight_checks_selected_dev_vite_apps_but_not_env_port_services() {
    let temp = tempdir().unwrap();
    for dir in ["web", "console", "api"] {
        fs::create_dir_all(temp.path().join(dir)).unwrap();
    }
    write_config(
        temp.path(),
        r#"[[frontend_apps]]
name = "web"
dir = "web"
coverage_threshold = 80

[[dev.apps]]
name = "web"
kind = "vite"
dir = "web"
argv = ["bun", "run", "dev"]

[[dev.apps]]
name = "console"
kind = "vite"
dir = "console"
argv = ["bun", "run", "dev"]

[[dev.apps]]
name = "api"
kind = "env-port"
dir = "api"
command = "cargo run"
"#,
    );
    write_dependency_checker(temp.path());
    mark_frontend_dependencies_ready(temp.path(), "web");
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    preflight_configured_apps(&ctx, &["web"]).unwrap();
    preflight_configured_apps(&ctx, &["api"]).unwrap();
    let error = preflight_configured_apps(&ctx, &["console"])
        .unwrap_err()
        .to_string();
    assert!(error.contains("console"));
    assert!(!error.contains("api"));
    assert!(error.contains("no bun-owned package manifest"));
    assert!(!error.contains("dependencies-bootstrap"));
    assert!(!error.contains("scripts/jig bootstrap"));
}

#[test]
fn frontend_dependency_preflight_uses_selected_workspace_discovery_plan() {
    let temp = tempdir().unwrap();
    for dir in ["apps/console", "apps/service"] {
        fs::create_dir_all(temp.path().join(dir)).unwrap();
    }
    write_config(temp.path(), "");
    fs::write(
        temp.path().join("package.json"),
        r#"{"private":true,"workspaces":["apps/*"]}"#,
    )
    .unwrap();
    fs::write(
        temp.path().join("apps/console/package.json"),
        r#"{"name":"console","scripts":{"dev":"vite"}}"#,
    )
    .unwrap();
    fs::write(
        temp.path().join("apps/service/package.json"),
        r#"{"name":"service","scripts":{"dev":"node server.js"}}"#,
    )
    .unwrap();
    write_dependency_checker(temp.path());
    mark_frontend_dependencies_ready(temp.path(), "apps/service");
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let settings = settings(&ctx, &ProxyRuntimeOptions::default()).unwrap();

    let service = jig_dev_proxy::resolve_dev_request(
        jig_dev_proxy::DevRequest::new(
            ctx.repo_name(),
            ctx.root().to_path_buf(),
            ctx.web_package_manager(),
            settings.clone(),
        )
        .with_selected_apps(vec!["service".into()])
        .with_discover_workspace(true),
    )
    .unwrap();
    ensure_frontend_dependencies(&ctx, service.apps(), &|| false).unwrap();

    let console = jig_dev_proxy::resolve_dev_request(
        jig_dev_proxy::DevRequest::new(
            ctx.repo_name(),
            ctx.root().to_path_buf(),
            ctx.web_package_manager(),
            settings,
        )
        .with_selected_apps(vec!["console".into()])
        .with_discover_workspace(true),
    )
    .unwrap();
    let error = ensure_frontend_dependencies(&ctx, console.apps(), &|| false)
        .unwrap_err()
        .to_string();
    assert!(error.contains("console"));
    assert!(!error.contains("service"));
    assert!(error.contains("scripts/check-webapps.sh dependencies-bootstrap apps/console"));
    assert!(!error.contains("scripts/jig bootstrap"));
}

#[test]
fn frontend_dependency_preflight_checks_selected_discovered_astro_app() {
    let temp = tempdir().unwrap();
    fs::create_dir_all(temp.path().join("apps/docs")).unwrap();
    write_config(temp.path(), "");
    fs::write(
        temp.path().join("package.json"),
        r#"{"private":true,"workspaces":["apps/*"]}"#,
    )
    .unwrap();
    fs::write(
        temp.path().join("apps/docs/package.json"),
        r#"{"name":"docs","scripts":{"dev":"astro dev"}}"#,
    )
    .unwrap();
    write_dependency_checker(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let settings = settings(&ctx, &ProxyRuntimeOptions::default()).unwrap();
    let request = jig_dev_proxy::resolve_dev_request(
        jig_dev_proxy::DevRequest::new(
            ctx.repo_name(),
            ctx.root().to_path_buf(),
            ctx.web_package_manager(),
            settings,
        )
        .with_selected_apps(vec!["docs".into()])
        .with_discover_workspace(true),
    )
    .unwrap();

    assert_eq!(request.apps()[0].kind, jig_dev_proxy::AppKind::EnvPort);
    assert!(matches!(
        &request.apps()[0].command,
        jig_dev_proxy::CommandSpec::Argv(argv)
            if argv.as_slice() == [ctx.web_package_manager(), "run", "dev"]
    ));
    let error = ensure_frontend_dependencies(&ctx, request.apps(), &|| false)
        .unwrap_err()
        .to_string();
    assert!(error.contains("docs"));
    assert!(error.contains("scripts/check-webapps.sh dependencies-bootstrap apps/docs"));

    mark_frontend_dependencies_ready(temp.path(), "apps/docs");
    ensure_frontend_dependencies(&ctx, request.apps(), &|| false).unwrap();
}

#[test]
fn pnpm_dev_recovery_recognizes_only_manager_owned_manifest_names() {
    let temp = tempdir().unwrap();
    for manifest in ["package.json", "package.json5", "package.yaml"] {
        let app = temp.path().join(manifest.replace('.', "-"));
        fs::create_dir_all(&app).unwrap();
        fs::write(app.join(manifest), "{}\n").unwrap();
        assert!(package_manager_manifest_exists(&app, "pnpm"));
    }

    let app = temp.path().join("unmanaged");
    fs::create_dir_all(&app).unwrap();
    fs::write(app.join("deno.json"), "{}\n").unwrap();
    assert!(!package_manager_manifest_exists(&app, "pnpm"));
    assert!(!package_manager_manifest_exists(&app, "npm"));
}

#[test]
fn frontend_dependency_preflight_preserves_legacy_repos_without_readiness_mode() {
    let temp = tempdir().unwrap();
    fs::create_dir_all(temp.path().join("web")).unwrap();
    write_config(
        temp.path(),
        r#"[[frontend_apps]]
name = "web"
dir = "web"
coverage_threshold = 80
"#,
    );
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    preflight_configured_apps(&ctx, &[]).unwrap();

    fs::create_dir_all(temp.path().join("scripts")).unwrap();
    fs::write(
        temp.path().join("scripts/check-webapps.sh"),
        "#!/usr/bin/env bash\necho 'Usage: scripts/check-webapps.sh bootstrap|lint' >&2\nexit 2\n",
    )
    .unwrap();
    preflight_configured_apps(&ctx, &[]).unwrap();
}

#[test]
fn dependency_readiness_usage_classifies_only_unadvertised_legacy_mode() {
    for (diagnostic, expected_legacy) in [
        (
            "Usage: scripts/check-webapps.sh lint|typecheck|build|coverage\n",
            true,
        ),
        (
            "Usage: scripts/check-webapps.sh bootstrap|dependencies-ready <app-dir>|lint\n",
            false,
        ),
        ("dependency helper failed\n", false),
        (
            "dependency helper failed\nUsage: scripts/check-webapps.sh lint\ndependencies-ready failed\n",
            false,
        ),
    ] {
        assert_eq!(
            dependency_readiness_usage_is_legacy(diagnostic),
            expected_legacy,
            "{diagnostic:?}"
        );
    }
}

#[test]
fn frontend_dependency_preflight_surfaces_current_checker_usage_failures() {
    let temp = tempdir().unwrap();
    fs::create_dir_all(temp.path().join("web")).unwrap();
    write_config(
        temp.path(),
        r#"[[frontend_apps]]
name = "web"
dir = "web"
coverage_threshold = 80
"#,
    );
    fs::create_dir_all(temp.path().join("scripts")).unwrap();
    fs::write(
        temp.path().join("scripts/check-webapps.sh"),
        "#!/usr/bin/env bash\necho 'Usage: scripts/check-webapps.sh bootstrap|dependencies-ready <app-dir>|lint' >&2\nexit 2\n",
    )
    .unwrap();
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let error = preflight_configured_apps(&ctx, &[])
        .unwrap_err()
        .to_string();

    assert!(error.contains("readiness check failed for web"), "{error}");
    assert!(error.contains("dependencies-ready"), "{error}");
}

#[test]
fn frontend_dependency_preflight_surfaces_checker_failures() {
    let temp = tempdir().unwrap();
    fs::create_dir_all(temp.path().join("web")).unwrap();
    write_config(
        temp.path(),
        r#"[[frontend_apps]]
name = "web"
dir = "web"
coverage_threshold = 80
"#,
    );
    fs::create_dir_all(temp.path().join("scripts")).unwrap();
    fs::write(
        temp.path().join("scripts/check-webapps.sh"),
        "#!/usr/bin/env bash\necho 'fingerprint helper crashed' >&2\nexit 9\n",
    )
    .unwrap();
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let error = preflight_configured_apps(&ctx, &[])
        .unwrap_err()
        .to_string();
    assert!(error.contains("readiness check failed for web"));
    assert!(error.contains("fingerprint helper crashed"));
}

#[cfg(unix)]
#[test]
fn frontend_dependency_preflight_bounds_and_lossily_decodes_diagnostics() {
    let temp = tempdir().unwrap();
    fs::create_dir_all(temp.path().join("scripts")).unwrap();
    let checker = temp.path().join("scripts/check-webapps.sh");
    fs::write(
        &checker,
        "#!/usr/bin/env bash\ni=0\nwhile [ \"$i\" -lt 5000 ]; do printf '0123456789abcdef0123456789abcdef' >&2; i=$((i + 1)); done\nexit 9\n",
    )
    .unwrap();
    let error = frontend_dependency_readiness_with_shell(temp.path(), "web", OsStr::new("bash"))
        .unwrap_err()
        .to_string();
    assert!(error.contains("exceeded the capture limit"));

    fs::write(
        &checker,
        "#!/usr/bin/env bash\ni=0\nwhile [ \"$i\" -lt 5000 ]; do printf '0123456789abcdef0123456789abcdef' >&2; i=$((i + 1)); done\nexit 0\n",
    )
    .unwrap();
    assert_eq!(
        frontend_dependency_readiness_with_shell(temp.path(), "web", OsStr::new("bash")).unwrap(),
        FrontendDependencyReadiness::Ready
    );

    fs::write(
        &checker,
        "#!/usr/bin/env bash\nprintf '\\377invalid diagnostic\\n' >&2\nexit 9\n",
    )
    .unwrap();
    let error = frontend_dependency_readiness_with_shell(temp.path(), "web", OsStr::new("bash"))
        .unwrap_err()
        .to_string();
    assert!(error.contains("invalid diagnostic"));
    assert!(!error.contains("could not be read"));
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn frontend_dependency_preflight_rejects_incomplete_capture_after_success() {
    let _env = lock_env();
    let temp = tempdir().unwrap();
    fs::create_dir_all(temp.path().join("scripts")).unwrap();
    let marker = temp.path().join("escaped-preflight-writer");
    fs::write(
        temp.path().join("scripts/check-webapps.sh"),
        r#"#!/usr/bin/env bash
"$JIG_PREFLIGHT_ESCAPE_TEST_EXE" --exact doctor::tests::owned_process_output_escape_helper --nocapture
exit 0
"#,
    )
    .unwrap();
    let _test_exe = EnvVarGuard::set(
        "JIG_PREFLIGHT_ESCAPE_TEST_EXE",
        std::env::current_exe().unwrap(),
    );
    let _mode = EnvVarGuard::set("JIG_OWNED_OUTPUT_ESCAPE_HELPER", "spawn");
    let _marker = EnvVarGuard::set("JIG_OWNED_OUTPUT_ESCAPE_MARKER", &marker);

    let error = frontend_dependency_readiness_with_shell(temp.path(), "web", OsStr::new("bash"))
        .unwrap_err()
        .to_string();
    let escaped = read_test_process_identity(&marker);
    terminate_and_confirm_test_process(&escaped);

    assert!(error.contains("capture did not complete"), "{error}");
}

#[test]
fn frontend_dependency_preflight_reports_an_actionable_missing_bash_error() {
    let temp = tempdir().unwrap();
    fs::create_dir_all(temp.path().join("web")).unwrap();
    write_dependency_checker(temp.path());

    let missing_shell = temp.path().join("definitely-not-bash");
    let error =
        frontend_dependency_readiness_with_shell(temp.path(), "web", missing_shell.as_os_str())
            .unwrap_err()
            .to_string();

    assert!(error.contains("scripts/check-webapps.sh"));
    assert!(error.contains("Bash is required"));
    assert!(error.contains("ensure `bash` is on PATH"));
}

#[cfg(unix)]
#[test]
fn frontend_dependency_preflight_preserves_non_not_found_start_errors() {
    use std::os::unix::fs::PermissionsExt;

    let temp = tempdir().unwrap();
    fs::create_dir_all(temp.path().join("web")).unwrap();
    write_dependency_checker(temp.path());
    let unusable_shell = temp.path().join("not-executable-bash");
    fs::write(&unusable_shell, "#!/bin/sh\nexit 0\n").unwrap();
    fs::set_permissions(&unusable_shell, fs::Permissions::from_mode(0o644)).unwrap();

    let error =
        frontend_dependency_readiness_with_shell(temp.path(), "web", unusable_shell.as_os_str())
            .unwrap_err()
            .to_string();

    assert!(error.contains("Failed to start dependency readiness check"));
    assert!(error.contains("Permission denied"));
    assert!(!error.contains("Bash is required"));
    assert!(!error.contains("ensure `bash` is on PATH"));
}

#[cfg(unix)]
#[test]
fn frontend_dependency_preflight_sanitizes_bash_control_environment() {
    let temp = tempdir().unwrap();
    fs::create_dir_all(temp.path().join("scripts")).unwrap();
    let checker_marker = temp.path().join("checker-ran");
    let poison_marker = temp.path().join("shell-poison-ran");
    let trace_marker = temp.path().join("shell-trace-poison-ran");
    let startup = temp.path().join("startup-poison.sh");
    fs::write(
        &startup,
        "printf poison > \"$JIG_PREFLIGHT_POISON_MARKER\"\nexit 0\n",
    )
    .unwrap();
    fs::write(
        temp.path().join("scripts/check-webapps.sh"),
        r#"#!/usr/bin/env bash
printf checker > "$JIG_PREFLIGHT_CHECKER_MARKER"
if [ -n "${BASH_ENV+x}" ] || [ -n "${ENV+x}" ] || [ -n "${CDPATH+x}" ] || [ -n "${BASH_XTRACEFD+x}" ] || declare -F jig_preflight_poison >/dev/null; then
  exit 0
fi
case "$-" in *x*|*v*) exit 0 ;; esac
shopt -q extglob && exit 0
case "$PS4" in *JIG_PREFLIGHT_PS4_POISON*) exit 0 ;; esac
[ "$JIG_PREFLIGHT_ORDINARY_ENV" = preserved ] || exit 0
exit 1
"#,
    )
    .unwrap();
    let command_environment = [
        (OsString::from("BASH_ENV"), startup.as_os_str().to_owned()),
        (OsString::from("ENV"), startup.as_os_str().to_owned()),
        (OsString::from("CDPATH"), temp.path().as_os_str().to_owned()),
        (
            OsString::from("BASH_FUNC_jig_preflight_poison%%"),
            OsString::from("() { printf poison > \"$JIG_PREFLIGHT_POISON_MARKER\"; }"),
        ),
        (
            OsString::from("SHELLOPTS"),
            OsString::from("xtrace:verbose"),
        ),
        (OsString::from("BASHOPTS"), OsString::from("extglob")),
        (
            OsString::from("PS4"),
            OsString::from(
                "JIG_PREFLIGHT_PS4_POISON$(printf poison > \"$JIG_PREFLIGHT_TRACE_MARKER\")",
            ),
        ),
        (OsString::from("BASH_XTRACEFD"), OsString::from("2")),
        (
            OsString::from("JIG_PREFLIGHT_CHECKER_MARKER"),
            checker_marker.as_os_str().to_owned(),
        ),
        (
            OsString::from("JIG_PREFLIGHT_POISON_MARKER"),
            poison_marker.as_os_str().to_owned(),
        ),
        (
            OsString::from("JIG_PREFLIGHT_TRACE_MARKER"),
            trace_marker.as_os_str().to_owned(),
        ),
        (
            OsString::from("JIG_PREFLIGHT_ORDINARY_ENV"),
            OsString::from("preserved"),
        ),
    ];

    let readiness = frontend_dependency_readiness_with_shell_timeout_and_environment(
        temp.path(),
        "web",
        OsStr::new("bash"),
        FRONTEND_DEPENDENCY_READINESS_TIMEOUT,
        &|| false,
        &command_environment,
    )
    .unwrap();

    assert_eq!(readiness, FrontendDependencyReadiness::MissingOrStale);
    assert!(
        checker_marker.exists(),
        "ordinary runtime env was not preserved"
    );
    assert!(
        !poison_marker.exists(),
        "Bash control environment executed during readiness preflight"
    );
    assert!(
        !trace_marker.exists(),
        "Bash trace environment executed during readiness preflight"
    );
}

#[cfg(unix)]
#[test]
fn frontend_dependency_preflight_uses_typed_cancellation_marker() {
    let temp = tempdir().unwrap();
    fs::create_dir_all(temp.path().join("scripts")).unwrap();
    fs::write(
        temp.path().join("scripts/check-webapps.sh"),
        "#!/usr/bin/env bash\n/bin/sleep 30\n",
    )
    .unwrap();

    let error = frontend_dependency_readiness_with_shell_and_timeout(
        temp.path(),
        "web",
        OsStr::new("bash"),
        Duration::from_secs(2),
        &|| true,
    )
    .unwrap_err();

    assert!(error.is::<FrontendDependencyPreflightCancelled>());
    assert!(error.to_string().contains("cancelled for web"));
}

#[cfg(unix)]
#[test]
fn frontend_dependency_preflight_cleans_descendants_after_exit_and_timeout() {
    let temp = tempdir().unwrap();
    fs::create_dir_all(temp.path().join("scripts")).unwrap();
    let checker = temp.path().join("scripts/check-webapps.sh");

    let completed_marker = temp.path().join("completed-descendant");
    fs::write(
        &checker,
        format!(
            "#!/usr/bin/env bash\n(/bin/sleep 0.5; printf leaked > '{}') &\nexit 0\n",
            completed_marker.display()
        ),
    )
    .unwrap();
    assert_eq!(
        frontend_dependency_readiness_with_shell_and_timeout(
            temp.path(),
            "web",
            OsStr::new("bash"),
            Duration::from_secs(2),
            &|| false,
        )
        .unwrap(),
        FrontendDependencyReadiness::Ready
    );
    std::thread::sleep(Duration::from_millis(700));
    assert!(!completed_marker.exists());

    let timeout_marker = temp.path().join("timeout-descendant");
    fs::write(
        &checker,
        format!(
            "#!/usr/bin/env bash\n(/bin/sleep 0.5; printf leaked > '{}') &\n/bin/sleep 30\n",
            timeout_marker.display()
        ),
    )
    .unwrap();
    let error = frontend_dependency_readiness_with_shell_and_timeout(
        temp.path(),
        "web",
        OsStr::new("bash"),
        Duration::from_millis(100),
        &|| false,
    )
    .unwrap_err()
    .to_string();
    assert!(error.contains("timed out"));
    std::thread::sleep(Duration::from_millis(700));
    assert!(!timeout_marker.exists());
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

    let error = RepoContext::load_from(temp.path()).unwrap_err().to_string();

    assert!(error.contains("Invalid dev app kind 'vit'"));
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
fn dev_app_dirs_must_be_portable_repository_relative() {
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

    let error = RepoContext::load_from(temp.path()).unwrap_err().to_string();

    assert!(
        error.contains("portable repository-relative"),
        "unexpected absolute dev app dir error: {error}"
    );
}

#[cfg(unix)]
#[test]
fn selected_app_dir_symlink_escape_is_rejected_after_filtering() {
    use std::os::unix::fs::symlink;

    let temp = tempdir().unwrap();
    let outside = tempdir().unwrap();
    symlink(outside.path(), temp.path().join("escaped-app")).unwrap();
    write_config(
        temp.path(),
        r#"[[dev.apps]]
name = "web"
dir = "escaped-app"
command = "cargo run"
proxy = false
"#,
    );
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let error = preflight_configured_apps(&ctx, &["web"])
        .unwrap_err()
        .to_string();

    assert!(error.contains("resolves outside repo root"));
}

#[test]
fn selected_dev_app_dirs_must_exist_after_filtering() {
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
    configured_apps(&ctx, &settings).unwrap();
    let error = preflight_configured_apps(&ctx, &["web"])
        .unwrap_err()
        .to_string();

    assert!(error.contains("development app 'web' directory"));
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
fn configured_zero_proxy_ports_are_rejected_but_explicit_zero_is_ephemeral() {
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

    let ephemeral = settings(
        &ctx,
        &ProxyRuntimeOptions {
            http_port: Some(0),
            ..ProxyRuntimeOptions::default()
        },
    )
    .unwrap();
    assert_eq!(ephemeral.http_port, 0);
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
