use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use anyhow::{Context, Result, bail};
use serde_json::Value;

use crate::command::{
    ProxyAliasRequest, ProxyCertCommand, ProxyCommand, ProxyRuntimeOptions, ProxyServiceCommand,
    ProxyStartRequest,
};
#[cfg(test)]
use crate::command::{
    ProxyCertGenerateRequest, ProxyCertRuntimeRequest, ProxyCertTrustRequest,
    ProxyCertUntrustRequest, ProxyListRequest, ProxyPruneRequest, ProxyRunRequest,
    ProxyServiceInstallRequest, ProxyServiceRuntimeRequest, ProxyStopRequest,
};
use crate::context::{DevAppConfig, RepoContext};
use crate::progress::CliProgress;
use crate::shell::quote as shell_quote;

pub(crate) mod commands {
    pub(crate) use self::dev::dev;
    use super::*;

    mod dev;

    pub(crate) fn proxy(ctx: &RepoContext, command: ProxyCommand) -> Result<Value> {
        match command {
            ProxyCommand::Start(opts) => proxy_start(ctx, opts),
            ProxyCommand::Stop(opts) => proxy_stop(settings(ctx, &opts.proxy)?),
            ProxyCommand::List(opts) => proxy_list(settings(ctx, &opts.proxy)?, opts.raw),
            ProxyCommand::Prune(opts) => proxy_prune(settings(ctx, &opts.proxy)?),
            ProxyCommand::Run(opts) => {
                reject_no_proxy_runtime_flags(opts.no_proxy, &opts.proxy)?;
                let settings = settings(ctx, &opts.proxy)?;
                let dir = opts
                    .dir
                    .as_deref()
                    .map(|dir| repo_dir(ctx.root(), dir, "--dir"))
                    .transpose()?
                    .unwrap_or_else(|| ctx.root().to_path_buf());
                let hostname =
                    jig_dev_proxy::app_hostname(&opts.name, ctx.repo_name(), &settings.tld)?;
                jig_dev_proxy::proxy_run_foreground(jig_dev_proxy::ProxyRunRequest::new(
                    settings,
                    jig_dev_proxy::AppRunSpec::new(
                        opts.name,
                        dir,
                        jig_dev_proxy::CommandSpec::Argv(opts.command),
                        hostname,
                    )
                    .with_kind(
                        opts.kind
                            .as_deref()
                            .map(jig_dev_proxy::AppKind::from_config)
                            .transpose()?
                            .unwrap_or(jig_dev_proxy::AppKind::EnvPort),
                    )
                    .with_explicit_port(opts.port)
                    .with_proxy(!opts.no_proxy),
                ))
            }
            ProxyCommand::Alias(opts) => proxy_alias(ctx, opts),
            ProxyCommand::Cert(command) => proxy_cert(ctx, command),
            ProxyCommand::Service(command) => proxy_service(ctx, command),
        }
    }

    pub(crate) fn can_run_without_context(command: &ProxyCommand) -> bool {
        if matches!(command, ProxyCommand::Start(opts) if opts.foreground) {
            return true;
        }
        matches!(
            command,
            ProxyCommand::Stop(_)
                | ProxyCommand::List(_)
                | ProxyCommand::Prune(_)
                | ProxyCommand::Cert(
                    ProxyCertCommand::Status(_)
                        | ProxyCertCommand::Trust(_)
                        | ProxyCertCommand::Untrust(_)
                )
                | ProxyCommand::Service(
                    ProxyServiceCommand::Uninstall(_) | ProxyServiceCommand::Status(_)
                )
        )
    }

    pub(crate) fn proxy_without_context(command: ProxyCommand) -> Result<Value> {
        match command {
            ProxyCommand::Start(opts) if opts.foreground => jig_dev_proxy::proxy_start(
                jig_dev_proxy::ProxyStartRequest::new(settings_without_context(&opts.proxy)?, true),
            ),
            ProxyCommand::Stop(opts) => proxy_stop(settings_without_context(&opts.proxy)?),
            ProxyCommand::List(opts) => {
                proxy_list(settings_without_context(&opts.proxy)?, opts.raw)
            }
            ProxyCommand::Prune(opts) => proxy_prune(settings_without_context(&opts.proxy)?),
            ProxyCommand::Cert(ProxyCertCommand::Status(opts)) => {
                proxy_cert_status(settings_existing_state_dir_without_context(&opts.proxy)?)
            }
            ProxyCommand::Cert(ProxyCertCommand::Trust(opts)) => proxy_cert_trust(
                settings_existing_state_dir_without_context(&opts.proxy)?,
                opts.accept_trust_scope,
            ),
            ProxyCommand::Cert(ProxyCertCommand::Untrust(opts)) => proxy_cert_untrust(
                settings_existing_state_dir_without_context(&opts.proxy)?,
                opts.accept_trust_scope,
            ),
            ProxyCommand::Service(ProxyServiceCommand::Uninstall(opts)) => {
                let progress = CliProgress::new("proxy service");
                progress.header("remove user service");
                progress.step("resolve proxy", "state directory and runtime flags");
                let settings =
                    progress.log_blocked_on_err(settings_without_context(&opts.proxy))?;
                let output = progress.log_blocked_on_err(jig_dev_proxy::proxy_service(
                    jig_dev_proxy::ProxyServiceRequest::Uninstall { settings },
                ))?;
                finish_service_progress(
                    &progress,
                    "service uninstall complete",
                    "service uninstall did not complete",
                    &output,
                );
                Ok(output)
            }
            ProxyCommand::Service(ProxyServiceCommand::Status(opts)) => {
                let progress = CliProgress::new("proxy service");
                progress.header("inspect user service");
                progress.step("resolve proxy", "state directory and runtime flags");
                let settings = progress
                    .log_blocked_on_err(service_status_settings_without_context(&opts.proxy))?;
                let output = progress.log_blocked_on_err(jig_dev_proxy::proxy_service(
                    jig_dev_proxy::ProxyServiceRequest::Status { settings },
                ))?;
                finish_service_progress(
                    &progress,
                    "service status complete",
                    "service is not active",
                    &output,
                );
                Ok(output)
            }
            _ => bail!("This proxy command requires an adopted Jig repo."),
        }
    }

    fn proxy_start(ctx: &RepoContext, opts: ProxyStartRequest) -> Result<Value> {
        jig_dev_proxy::proxy_start(jig_dev_proxy::ProxyStartRequest::new(
            settings(ctx, &opts.proxy)?,
            opts.foreground,
        ))
    }

    fn proxy_stop(settings: jig_dev_proxy::ProxySettings) -> Result<Value> {
        jig_dev_proxy::proxy_stop(jig_dev_proxy::ProxyStopRequest::new(settings))
    }

    fn proxy_list(settings: jig_dev_proxy::ProxySettings, raw: bool) -> Result<Value> {
        jig_dev_proxy::proxy_list(jig_dev_proxy::ProxyListRequest::new(settings, raw))
    }

    fn proxy_prune(settings: jig_dev_proxy::ProxySettings) -> Result<Value> {
        jig_dev_proxy::proxy_prune(jig_dev_proxy::ProxyPruneRequest::new(settings))
    }

    fn proxy_alias(ctx: &RepoContext, opts: ProxyAliasRequest) -> Result<Value> {
        jig_dev_proxy::proxy_alias(
            jig_dev_proxy::ProxyAliasRequest::new(
                settings(ctx, &opts.proxy)?,
                ctx.repo_name(),
                opts.name,
                opts.host,
                opts.port,
            )
            .with_accept_non_loopback_target(opts.accept_non_loopback_target),
        )
    }

    fn proxy_cert(ctx: &RepoContext, command: ProxyCertCommand) -> Result<Value> {
        match command {
            ProxyCertCommand::Generate(opts) => {
                jig_dev_proxy::proxy_cert(jig_dev_proxy::ProxyCertRequest::Generate {
                    settings: settings(ctx, &opts.proxy)?,
                    force: opts.force,
                })
            }
            ProxyCertCommand::Status(opts) => {
                proxy_cert_status(settings_existing_state_dir(ctx, &opts.proxy)?)
            }
            ProxyCertCommand::Trust(opts) => proxy_cert_trust(
                settings_existing_state_dir(ctx, &opts.proxy)?,
                opts.accept_trust_scope,
            ),
            ProxyCertCommand::Untrust(opts) => proxy_cert_untrust(
                settings_existing_state_dir(ctx, &opts.proxy)?,
                opts.accept_trust_scope,
            ),
        }
    }

    fn proxy_cert_status(settings: jig_dev_proxy::ProxySettings) -> Result<Value> {
        jig_dev_proxy::proxy_cert(jig_dev_proxy::ProxyCertRequest::Status { settings })
    }

    fn proxy_cert_trust(
        settings: jig_dev_proxy::ProxySettings,
        accept_trust_scope: bool,
    ) -> Result<Value> {
        jig_dev_proxy::proxy_cert(jig_dev_proxy::ProxyCertRequest::Trust {
            settings,
            accept_trust_scope,
        })
    }

    fn proxy_cert_untrust(
        settings: jig_dev_proxy::ProxySettings,
        accept_trust_scope: bool,
    ) -> Result<Value> {
        jig_dev_proxy::proxy_cert(jig_dev_proxy::ProxyCertRequest::Untrust {
            settings,
            accept_trust_scope,
        })
    }

    fn proxy_service(ctx: &RepoContext, command: ProxyServiceCommand) -> Result<Value> {
        let progress = CliProgress::new("proxy service");
        progress.header(service_action(&command));
        progress.info("repo", ctx.root().display());
        progress.step("resolve proxy", "state directory and runtime flags");
        let runtime_detail = service_runtime_detail(&command);
        let failure_message = service_failure_message(&command);
        let request = match command {
            ProxyServiceCommand::Install(opts) => jig_dev_proxy::ProxyServiceRequest::Install {
                settings: progress.log_blocked_on_err(settings(ctx, &opts.proxy))?,
                current_exe: {
                    progress.step("resolve binary", "current jig executable");
                    progress.log_blocked_on_err(jig_dev_proxy::current_exe())?
                },
                repo_root: ctx.root().to_path_buf(),
                accept_service_scope: opts.accept_service_scope,
            },
            ProxyServiceCommand::Uninstall(opts) => jig_dev_proxy::ProxyServiceRequest::Uninstall {
                settings: progress.log_blocked_on_err(settings(ctx, &opts.proxy))?,
            },
            ProxyServiceCommand::Status(opts) => {
                let settings =
                    progress.log_blocked_on_err(service_status_settings(ctx, &opts.proxy))?;
                jig_dev_proxy::ProxyServiceRequest::Status { settings }
            }
        };
        progress.step("run service action", runtime_detail);
        let output = progress.log_blocked_on_err(jig_dev_proxy::proxy_service(request))?;
        finish_service_progress(
            &progress,
            "service command complete",
            failure_message,
            &output,
        );
        Ok(output)
    }
}

fn dev_session_message(configured_app_count: usize, discover_workspace: bool) -> String {
    let configured = match configured_app_count {
        1 => "1 configured app".to_string(),
        count => format!("{count} configured apps"),
    };
    if discover_workspace {
        format!("{configured}; workspace discovery enabled")
    } else {
        configured
    }
}

const FRONTEND_DEPENDENCY_READINESS_TIMEOUT: Duration = Duration::from_secs(30);

fn ensure_frontend_dependencies(
    ctx: &RepoContext,
    apps: &[jig_dev_proxy::AppRunSpec],
    cancelled: &dyn Fn() -> bool,
) -> Result<()> {
    let root = ctx
        .root()
        .canonicalize()
        .with_context(|| format!("Failed to canonicalize repo root {}", ctx.root().display()))?;
    let mut configured_missing = Vec::new();
    let mut recoverable_missing = Vec::new();
    let mut unmanaged_missing = Vec::new();
    for app in apps {
        let matching_frontend = ctx
            .frontend_apps()
            .iter()
            .any(|frontend| frontend.name == app.name);
        let package_manager_dev_app =
            package_manager_manifest_exists(&app.dir, ctx.web_package_manager())
                && matches!(
                    &app.command,
                    jig_dev_proxy::CommandSpec::Argv(argv)
                        if argv.as_slice()
                            == [ctx.web_package_manager(), "run", "dev"]
                );
        if app.kind != jig_dev_proxy::AppKind::Vite
            && !matching_frontend
            && !package_manager_dev_app
        {
            continue;
        }
        let relative_dir = app.dir.strip_prefix(&root).with_context(|| {
            format!(
                "development app '{}' directory {} resolves outside repo root {}",
                app.name,
                app.dir.display(),
                root.display()
            )
        })?;
        let app_dir = if relative_dir.as_os_str().is_empty() {
            ".".to_string()
        } else {
            relative_dir.to_string_lossy().replace('\\', "/")
        };
        match frontend_dependency_readiness(&root, &app_dir, cancelled)? {
            FrontendDependencyReadiness::Ready | FrontendDependencyReadiness::Unsupported => {}
            FrontendDependencyReadiness::MissingOrStale if matching_frontend => {
                configured_missing.push(app.name.as_str());
            }
            FrontendDependencyReadiness::MissingOrStale
                if package_manager_manifest_exists(&app.dir, ctx.web_package_manager()) =>
            {
                recoverable_missing.push((app.name.as_str(), app_dir));
            }
            FrontendDependencyReadiness::MissingOrStale => {
                unmanaged_missing.push(app.name.as_str());
            }
        }
    }
    if configured_missing.is_empty()
        && recoverable_missing.is_empty()
        && unmanaged_missing.is_empty()
    {
        return Ok(());
    }

    let mut diagnostics = Vec::new();
    if !configured_missing.is_empty() {
        diagnostics.push(format!(
            "Frontend dependencies are missing or stale for {}. Run `scripts/jig bootstrap` before `scripts/jig dev`.",
            configured_missing.join(", ")
        ));
    }
    for (name, app_dir) in recoverable_missing {
        diagnostics.push(format!(
            "Frontend dependencies are missing or stale for {name}. Run `scripts/check-webapps.sh dependencies-bootstrap {}` before `scripts/jig dev`.",
            shell_quote(&app_dir)
        ));
    }
    if !unmanaged_missing.is_empty() {
        diagnostics.push(format!(
            "Frontend dependency readiness failed for {}, but the selected directories have no {}-owned package manifest, so Jig cannot offer a dependency bootstrap command.",
            unmanaged_missing.join(", "),
            ctx.web_package_manager()
        ));
    }
    diagnostics.push("dev does not install packages implicitly.".to_string());
    bail!(diagnostics.join(" "))
}

fn package_manager_manifest_exists(app_dir: &Path, package_manager: &str) -> bool {
    let candidates: &[&str] = if package_manager == "pnpm" {
        &["package.json", "package.json5", "package.yaml"]
    } else {
        &["package.json"]
    };
    candidates
        .iter()
        .any(|candidate| app_dir.join(candidate).is_file())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum FrontendDependencyReadiness {
    Ready,
    MissingOrStale,
    Unsupported,
}

#[derive(Debug)]
struct FrontendDependencyPreflightCancelled {
    app_dir: String,
}

impl std::fmt::Display for FrontendDependencyPreflightCancelled {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "Frontend dependency readiness check was cancelled for {}",
            self.app_dir
        )
    }
}

impl std::error::Error for FrontendDependencyPreflightCancelled {}

#[derive(Debug)]
struct FrontendDependencyPreflightCleanupUnconfirmed {
    app_dir: String,
}

impl std::fmt::Display for FrontendDependencyPreflightCleanupUnconfirmed {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "Frontend dependency readiness check process tree could not be cleaned up safely for {}",
            self.app_dir
        )
    }
}

impl std::error::Error for FrontendDependencyPreflightCleanupUnconfirmed {}

fn frontend_dependency_readiness(
    repo_root: &Path,
    app_dir: &str,
    cancelled: &dyn Fn() -> bool,
) -> Result<FrontendDependencyReadiness> {
    frontend_dependency_readiness_with_shell_and_timeout(
        repo_root,
        app_dir,
        OsStr::new("bash"),
        FRONTEND_DEPENDENCY_READINESS_TIMEOUT,
        cancelled,
    )
}

#[cfg(test)]
fn frontend_dependency_readiness_with_shell(
    repo_root: &Path,
    app_dir: &str,
    shell: &OsStr,
) -> Result<FrontendDependencyReadiness> {
    frontend_dependency_readiness_with_shell_and_timeout(
        repo_root,
        app_dir,
        shell,
        FRONTEND_DEPENDENCY_READINESS_TIMEOUT,
        &|| false,
    )
}

fn frontend_dependency_readiness_with_shell_and_timeout(
    repo_root: &Path,
    app_dir: &str,
    shell: &OsStr,
    timeout: Duration,
    cancelled: &dyn Fn() -> bool,
) -> Result<FrontendDependencyReadiness> {
    frontend_dependency_readiness_with_shell_timeout_and_environment(
        repo_root,
        app_dir,
        shell,
        timeout,
        cancelled,
        &[],
    )
}

fn frontend_dependency_readiness_with_shell_timeout_and_environment(
    repo_root: &Path,
    app_dir: &str,
    shell: &OsStr,
    timeout: Duration,
    cancelled: &dyn Fn() -> bool,
    command_environment: &[(OsString, OsString)],
) -> Result<FrontendDependencyReadiness> {
    let checker = repo_root.join("scripts/check-webapps.sh");
    if !checker.is_file() {
        // Older adopted repositories predate dependency receipts and did not
        // have a dev preflight. Preserve that behavior until their managed
        // harness is refreshed instead of guessing from partial artifacts.
        return Ok(FrontendDependencyReadiness::Unsupported);
    }

    let mut command = Command::new(shell);
    command
        .arg(&checker)
        .arg("dependencies-ready")
        .arg(app_dir)
        .current_dir(repo_root)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    for (key, value) in command_environment {
        command.env(key, value);
    }
    crate::shell::sanitize_bash_environment(&mut command);
    let output = match crate::process::run_owned_process_tree_with_output(
        &mut command,
        timeout,
        cancelled,
    ) {
        Ok(output) => output,
        Err(crate::process::OwnedProcessTreeError::Start(error)) => {
            if error.kind() == std::io::ErrorKind::NotFound {
                return Err(anyhow::anyhow!(
                    "Failed to run dependency readiness check {} with Bash: {error}. {}",
                    checker.display(),
                    bash_requirement_hint()
                ));
            }
            return Err(anyhow::anyhow!(
                "Failed to start dependency readiness check {} with Bash: {error}",
                checker.display(),
            ));
        }
        Err(crate::process::OwnedProcessTreeError::TimedOut) => bail!(
            "Frontend dependency readiness check timed out for {app_dir} after {:.1} seconds",
            timeout.as_secs_f64()
        ),
        Err(crate::process::OwnedProcessTreeError::Cancelled) => {
            return Err(FrontendDependencyPreflightCancelled {
                app_dir: app_dir.to_string(),
            }
            .into());
        }
        Err(crate::process::OwnedProcessTreeError::Await) => {
            bail!("Frontend dependency readiness check could not be awaited for {app_dir}")
        }
        Err(crate::process::OwnedProcessTreeError::Cleanup) => {
            return Err(FrontendDependencyPreflightCleanupUnconfirmed {
                app_dir: app_dir.to_string(),
            }
            .into());
        }
    };
    let stderr = output.stderr.as_ref().ok_or_else(|| {
        anyhow::anyhow!("Frontend dependency readiness diagnostic output was not captured")
    })?;
    if !stderr.complete {
        bail!(
            "Frontend dependency readiness diagnostic output capture did not complete for {app_dir}"
        );
    }
    let status = output.status;
    if status.success() {
        // A successful checker has no diagnostic payload to interpret. Its
        // exit status is authoritative even when advisory stderr exceeded the
        // bounded capture; incomplete capture remains an I/O failure above.
        return Ok(FrontendDependencyReadiness::Ready);
    }
    if stderr.truncated {
        bail!(
            "Frontend dependency readiness diagnostic output exceeded the capture limit for {app_dir}"
        );
    }
    if status.code() == Some(1) {
        return Ok(FrontendDependencyReadiness::MissingOrStale);
    }

    let stderr = stderr.to_string_lossy();
    if status.code() == Some(2) && dependency_readiness_usage_is_legacy(stderr.as_ref()) {
        return Ok(FrontendDependencyReadiness::Unsupported);
    }

    let detail = stderr.trim();
    if detail.is_empty() {
        bail!(
            "Frontend dependency readiness check failed for {app_dir} with status {}",
            status
        );
    }
    bail!(
        "Frontend dependency readiness check failed for {app_dir} with status {}: {detail}",
        status
    )
}

fn dependency_readiness_usage_is_legacy(stderr: &str) -> bool {
    stderr.contains("Usage: scripts/check-webapps.sh") && !stderr.contains("dependencies-ready")
}

#[cfg(windows)]
fn bash_requirement_hint() -> &'static str {
    "Bash is required for generated web-app checks; run Jig from Git Bash or WSL and ensure `bash` is on PATH."
}

#[cfg(not(windows))]
fn bash_requirement_hint() -> &'static str {
    "Bash is required for generated web-app checks; install Bash and ensure `bash` is on PATH."
}

fn service_action(command: &ProxyServiceCommand) -> &'static str {
    match command {
        ProxyServiceCommand::Install(_) => "install user service",
        ProxyServiceCommand::Uninstall(_) => "remove user service",
        ProxyServiceCommand::Status(_) => "inspect user service",
    }
}

fn service_runtime_detail(command: &ProxyServiceCommand) -> &'static str {
    match command {
        ProxyServiceCommand::Install(_) => "write and load service file",
        ProxyServiceCommand::Uninstall(_) => "unload and remove service file",
        ProxyServiceCommand::Status(_) => "query service manager",
    }
}

fn service_failure_message(command: &ProxyServiceCommand) -> &'static str {
    match command {
        ProxyServiceCommand::Install(_) => "service install did not complete",
        ProxyServiceCommand::Uninstall(_) => "service uninstall did not complete",
        ProxyServiceCommand::Status(_) => "service is not active",
    }
}

fn finish_service_progress(
    progress: &CliProgress,
    success_message: &str,
    failure_message: &str,
    output: &Value,
) {
    if json_ok(output) {
        progress.done(success_message);
    } else {
        progress.blocked(service_blocked_detail(output, failure_message));
    }
}

fn json_ok(output: &Value) -> bool {
    output.get("ok").and_then(Value::as_bool).unwrap_or(false)
}

fn dev_interrupted(output: &Value) -> bool {
    output
        .get("interrupted")
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

fn service_blocked_detail(output: &Value, fallback: &str) -> String {
    service_failure_detail(output).unwrap_or_else(|| fallback.to_string())
}

fn service_failure_detail(output: &Value) -> Option<String> {
    service_value_failure_detail(output).or_else(|| {
        ["service", "load", "unload", "reload"]
            .into_iter()
            .filter_map(|key| output.get(key))
            .find_map(service_nested_failure_detail)
    })
}

fn service_nested_failure_detail(value: &Value) -> Option<String> {
    service_nested_value_failure_detail(value).or_else(|| {
        value
            .as_object()?
            .values()
            .filter(|value| value.is_object())
            .find_map(service_nested_failure_detail)
    })
}

fn service_nested_value_failure_detail(value: &Value) -> Option<String> {
    if service_nested_value_is_failed_or_uncertain(value) {
        service_value_failure_detail(value)
    } else {
        None
    }
}

fn service_nested_value_is_failed_or_uncertain(value: &Value) -> bool {
    value.get("ok").and_then(Value::as_bool) == Some(false)
        || value
            .get("timed_out")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        || ["error", "output_error", "kill_error"]
            .into_iter()
            .any(|key| {
                value
                    .get(key)
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .is_some_and(|detail| !detail.is_empty())
            })
}

fn service_value_failure_detail(value: &Value) -> Option<String> {
    for key in ["error", "stderr", "output_error", "kill_error"] {
        if let Some(detail) = value
            .get(key)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|detail| !detail.is_empty())
        {
            return Some(detail.to_string());
        }
    }
    if value
        .get("timed_out")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        return Some("service manager command timed out".to_string());
    }
    if let Some(status) = value.get("status").and_then(Value::as_i64) {
        if status != 0 {
            return Some(format!(
                "service manager command exited with status {status}"
            ));
        }
    }
    None
}

fn workspace_discovery_enabled(ctx: &RepoContext, cli_requested: bool) -> Result<bool> {
    if cli_requested {
        return Ok(true);
    }
    if !ctx.dev_config().workspace_discovery {
        return Ok(false);
    }
    if std::env::var_os("JIG_DEV_ALLOW_WORKSPACE_DISCOVERY").is_some() {
        return Ok(true);
    }
    bail!(
        "[dev].workspace_discovery requires JIG_DEV_ALLOW_WORKSPACE_DISCOVERY=1 for automatic package script execution, or pass --discover-workspace for this invocation."
    )
}

fn configured_apps(
    ctx: &RepoContext,
    settings: &jig_dev_proxy::ProxySettings,
) -> Result<Vec<jig_dev_proxy::AppRunSpec>> {
    let mut apps = Vec::new();
    for app in &ctx.dev_config().apps {
        apps.push(app_from_dev_config(ctx, settings, app)?);
    }
    if apps.is_empty() {
        if ctx
            .frontend_apps()
            .iter()
            .any(|frontend| frontend.coverage_threshold != 0)
        {
            eprintln!(
                "Legacy [[frontend_apps]] coverage_threshold is ignored by dev proxy; move active dev-server settings into [[dev.apps]]."
            );
        }
        for frontend in ctx.frontend_apps() {
            let configured_kind = ctx.frontend_app_kind(frontend);
            let kind = jig_dev_proxy::AppKind::from_config(configured_kind)?;
            eprintln!(
                "Legacy [[frontend_apps]] entry '{}' is being launched as a proxied {} dev app; move it to [[dev.apps]] to make this explicit.",
                frontend.name, configured_kind
            );
            let dir = unresolved_repo_dir(ctx.root(), Path::new(&frontend.dir));
            let hostname =
                jig_dev_proxy::app_hostname(&frontend.name, ctx.repo_name(), &settings.tld)?;
            apps.push(
                jig_dev_proxy::AppRunSpec::new(
                    frontend.name.clone(),
                    dir,
                    jig_dev_proxy::CommandSpec::Argv(vec![
                        ctx.web_package_manager().into(),
                        "run".into(),
                        "dev".into(),
                    ]),
                    hostname,
                )
                .with_kind(kind),
            );
        }
    }
    Ok(apps)
}

fn app_from_dev_config(
    ctx: &RepoContext,
    settings: &jig_dev_proxy::ProxySettings,
    app: &DevAppConfig,
) -> Result<jig_dev_proxy::AppRunSpec> {
    let name = app.name.trim();
    if name.is_empty() {
        bail!("dev app name cannot be empty");
    }
    if name != app.name.as_str() {
        bail!(
            "dev app name '{}' must not contain leading or trailing whitespace",
            app.name
        );
    }
    let hostname = jig_dev_proxy::app_hostname(name, ctx.repo_name(), &settings.tld)?;
    let dir = app
        .dir
        .as_deref()
        .map(|dir| unresolved_repo_dir(ctx.root(), Path::new(dir)))
        .unwrap_or_else(|| ctx.root().to_path_buf());
    let kind = jig_dev_proxy::AppKind::from_config(&app.kind)?;
    let command = if !app.argv.is_empty() {
        jig_dev_proxy::CommandSpec::Argv(app.argv.clone())
    } else {
        if kind == jig_dev_proxy::AppKind::Vite {
            bail!(
                "dev app '{}' uses kind = \"vite\" and must set argv instead of shell-form command",
                name
            );
        }
        let command = app
            .command
            .clone()
            .with_context(|| format!("dev app '{name}' requires command or argv"))?;
        jig_dev_proxy::CommandSpec::Shell(command)
    };
    let target_host = app.host.clone().unwrap_or_else(|| "127.0.0.1".into());
    let target_ip = jig_dev_proxy::parse_ip_literal(&target_host).with_context(|| {
        format!(
            "dev app '{}' host '{}' must be an IP literal",
            name, target_host
        )
    })?;
    if app.proxy && !jig_dev_proxy::ip_is_loopback(target_ip) {
        bail!(
            "dev app '{}' uses proxying and must target a loopback IP literal",
            name
        );
    }
    Ok(jig_dev_proxy::AppRunSpec::new(name, dir, command, hostname)
        .with_kind(kind)
        .with_target_host(target_host)
        .with_explicit_port(app.port)
        .with_proxy(app.proxy))
}

fn settings(ctx: &RepoContext, opts: &ProxyRuntimeOptions) -> Result<jig_dev_proxy::ProxySettings> {
    let config = ctx.dev_config();
    build_settings(
        opts,
        SettingsDefaults {
            http_port: config.proxy_port,
            https_port: config.https_port,
            https: config.https,
            http2: config.http2,
            lan: config.lan,
            tld: config.tld.clone(),
        },
        |tld| repo_certificate_names(ctx, tld),
    )
}

fn settings_existing_state_dir(
    ctx: &RepoContext,
    opts: &ProxyRuntimeOptions,
) -> Result<jig_dev_proxy::ProxySettings> {
    require_existing_state_dir(settings(ctx, opts)?)
}

fn service_status_settings(
    ctx: &RepoContext,
    opts: &ProxyRuntimeOptions,
) -> Result<jig_dev_proxy::ProxySettings> {
    settings(ctx, opts)
}

fn settings_without_context(opts: &ProxyRuntimeOptions) -> Result<jig_dev_proxy::ProxySettings> {
    let defaults = jig_dev_proxy::ProxySettings::default();
    build_settings(
        opts,
        SettingsDefaults {
            http_port: defaults.http_port,
            https_port: defaults.https_port,
            https: defaults.https,
            http2: defaults.http2,
            lan: defaults.lan,
            tld: defaults.tld,
        },
        |_| Ok(Vec::new()),
    )
}

fn settings_existing_state_dir_without_context(
    opts: &ProxyRuntimeOptions,
) -> Result<jig_dev_proxy::ProxySettings> {
    require_existing_state_dir(settings_without_context(opts)?)
}

fn service_status_settings_without_context(
    opts: &ProxyRuntimeOptions,
) -> Result<jig_dev_proxy::ProxySettings> {
    settings_without_context(opts)
}

struct SettingsDefaults {
    http_port: u16,
    https_port: Option<u16>,
    https: bool,
    http2: bool,
    lan: bool,
    tld: String,
}

fn build_settings(
    opts: &ProxyRuntimeOptions,
    defaults: SettingsDefaults,
    additional_dns_names: impl FnOnce(&str) -> Result<Vec<String>>,
) -> Result<jig_dev_proxy::ProxySettings> {
    let tld = opts
        .tld
        .clone()
        .unwrap_or(defaults.tld)
        .to_ascii_lowercase();
    jig_dev_proxy::validate_tld(&tld)?;
    let http_port = opts.http_port.unwrap_or(defaults.http_port);
    if http_port == 0 && opts.http_port.is_none() {
        bail!("proxy HTTP port must be greater than 0");
    }
    let https_port = opts.https_port.or(defaults.https_port);
    if https_port == Some(0) {
        bail!("proxy HTTPS port must be greater than 0");
    }
    if https_port == Some(http_port) {
        bail!("proxy HTTP and HTTPS ports must be different");
    }
    let additional_dns_names = additional_dns_names(&tld)?;
    Ok(jig_dev_proxy::ProxySettings {
        state_dir: Some(jig_dev_proxy::resolve_state_dir(opts.state_dir.clone())?),
        http_port,
        https_port,
        https: flag_override(defaults.https, opts.https, opts.no_https),
        http2: flag_override(defaults.http2, opts.http2, opts.no_http2),
        lan: flag_override(defaults.lan, opts.lan, opts.no_lan),
        tld,
        additional_dns_names,
    })
}

fn flag_override(default: bool, enable: bool, disable: bool) -> bool {
    match (enable, disable) {
        (true, false) => true,
        (false, true) => false,
        _ => default,
    }
}

fn require_existing_state_dir(
    settings: jig_dev_proxy::ProxySettings,
) -> Result<jig_dev_proxy::ProxySettings> {
    if let Some(path) = &settings.state_dir {
        if !path
            .try_exists()
            .with_context(|| format!("Failed to inspect proxy state dir {}", path.display()))?
        {
            bail!("proxy state dir {} does not exist", path.display());
        }
    }
    Ok(settings)
}

fn reject_no_proxy_runtime_flags(no_proxy: bool, opts: &ProxyRuntimeOptions) -> Result<()> {
    if !no_proxy {
        return Ok(());
    }
    let mut flags = Vec::new();
    if opts.http_port.is_some() {
        flags.push("--http-port");
    }
    if opts.https_port.is_some() {
        flags.push("--https-port");
    }
    if opts.https {
        flags.push("--https");
    }
    if opts.no_https {
        flags.push("--no-https");
    }
    if opts.http2 {
        flags.push("--http2");
    }
    if opts.no_http2 {
        flags.push("--no-http2");
    }
    if opts.lan {
        flags.push("--lan");
    }
    if opts.no_lan {
        flags.push("--no-lan");
    }
    if opts.tld.is_some() {
        flags.push("--tld");
    }
    // `--state-dir` remains allowed so no-proxy runs can still target the same
    // state root for compatible status, cert, or follow-up proxy commands.
    if !flags.is_empty() {
        bail!(
            "--no-proxy cannot be combined with proxy runtime options: {}",
            flags.join(", ")
        );
    }
    Ok(())
}

fn repo_dir(root: &Path, input: &Path, label: &str) -> Result<PathBuf> {
    let root = root
        .canonicalize()
        .with_context(|| format!("Failed to canonicalize repo root {}", root.display()))?;
    let candidate = if input.is_absolute() {
        input.to_path_buf()
    } else {
        root.join(input)
    };
    let canonical = candidate
        .canonicalize()
        .with_context(|| format!("{label} {} must exist", candidate.display()))?;
    if !canonical.starts_with(&root) {
        bail!(
            "{label} {} resolves outside repo root {}",
            candidate.display(),
            root.display()
        );
    }
    Ok(canonical)
}

fn unresolved_repo_dir(root: &Path, input: &Path) -> PathBuf {
    if input.is_absolute() {
        input.to_path_buf()
    } else {
        root.join(input)
    }
}

fn repo_certificate_names(ctx: &RepoContext, tld: &str) -> Result<Vec<String>> {
    let repo = jig_dev_proxy::dns_label(ctx.repo_name())?;
    Ok(vec![format!("*.{repo}.{tld}"), format!("{repo}.{tld}")])
}

#[cfg(test)]
mod tests;
