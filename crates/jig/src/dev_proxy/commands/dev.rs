use anyhow::Result;
use serde_json::Value;

use crate::command::{DevCommand, DevRequest, DevStatusRequest, DevStopRequest};
use crate::context::RepoContext;
use crate::progress::CliProgress;

use super::super::{
    FrontendDependencyPreflightCancelled, FrontendDependencyPreflightCleanupUnconfirmed,
    configured_apps, dev_interrupted, dev_session_message, ensure_frontend_dependencies, json_ok,
    reject_no_proxy_runtime_flags, settings, workspace_discovery_enabled,
};

pub(crate) fn dev(ctx: &RepoContext, command: DevCommand) -> Result<Value> {
    match command {
        DevCommand::Launch(opts) => dev_launch(ctx, opts),
        DevCommand::Status(opts) => dev_status(ctx, opts),
        DevCommand::Stop(opts) => dev_stop(ctx, opts),
    }
}

fn dev_launch(ctx: &RepoContext, opts: DevRequest) -> Result<Value> {
    let progress = CliProgress::new("dev");
    progress.header("launch configured development apps");
    progress.info("repo", ctx.root().display());
    progress.step("validate flags", "proxy and workspace discovery options");
    progress.log_blocked_on_err(reject_no_proxy_runtime_flags(opts.no_proxy, &opts.proxy))?;
    let discover_workspace =
        progress.log_blocked_on_err(workspace_discovery_enabled(ctx, opts.discover_workspace))?;
    progress.step("resolve proxy", "ports, TLS, LAN, and state directory");
    let settings = progress.log_blocked_on_err(settings(ctx, &opts.proxy))?;
    progress.step("collect apps", "configured frontend and [dev] entries");
    let apps = progress.log_blocked_on_err(configured_apps(ctx, &settings))?;
    let request = jig_dev_proxy::DevRequest::new(
        ctx.repo_name(),
        ctx.root().to_path_buf(),
        ctx.web_package_manager(),
        settings,
    )
    .with_apps(apps)
    .with_selected_apps(opts.apps)
    .with_discover_workspace(discover_workspace)
    .with_no_proxy(opts.no_proxy)
    .with_replace(opts.replace);
    let request = progress.log_blocked_on_err(jig_dev_proxy::resolve_dev_request(request))?;
    progress.step("check dependencies", "selected frontend bootstrap state");
    let output = progress.log_blocked_on_err(jig_dev_proxy::dev_resolved_with_preflight(
        request,
        |apps, cancelled| {
            if let Err(error) = ensure_frontend_dependencies(ctx, apps, cancelled) {
                if error.is::<FrontendDependencyPreflightCancelled>() {
                    return Err(jig_dev_proxy::DevPreflightError::cancelled());
                }
                if error.is::<FrontendDependencyPreflightCleanupUnconfirmed>() {
                    return Err(jig_dev_proxy::DevPreflightError::cleanup_unconfirmed(error));
                }
                return Err(jig_dev_proxy::DevPreflightError::failed(error));
            }
            progress.step(
                "start session",
                dev_session_message(apps.len(), discover_workspace),
            );
            Ok(())
        },
    ))?;
    if dev_interrupted(&output) || output.get("stopped").and_then(Value::as_bool) == Some(true) {
        progress.done("dev session stopped");
    } else if json_ok(&output) {
        progress.done("dev session complete");
    } else {
        progress.blocked("dev session ended with ok=false");
    }
    Ok(output)
}

fn dev_status(ctx: &RepoContext, opts: DevStatusRequest) -> Result<Value> {
    jig_dev_proxy::dev_status(jig_dev_proxy::DevStatusRequest::new(
        ctx.repo_name(),
        ctx.root().to_path_buf(),
        opts.state_dir,
    ))
}

fn dev_stop(ctx: &RepoContext, opts: DevStopRequest) -> Result<Value> {
    jig_dev_proxy::dev_stop(jig_dev_proxy::DevStopRequest::new(
        ctx.repo_name(),
        ctx.root().to_path_buf(),
        opts.state_dir,
    ))
}
