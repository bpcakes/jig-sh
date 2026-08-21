use std::env;
use std::ffi::{OsStr, OsString};
use std::path::Path;
use std::process::{Command, Output, Stdio};
use std::time::Duration;

use anyhow::{Context, Result, bail};
use jig_owned_process::{
    OwnedProcessTreeError, ProcessOutputLimits, format_exit_status, require_success,
    run_owned_process_tree_with_output, run_owned_process_tree_with_output_limits_and_observer,
};
use serde_json::{Value as JsonValue, json};

use crate::command::{AgentBootstrapRequest, AgentCommand};
use crate::context::{CodexMarketplaceConfig, RepoContext};
use crate::execution::{ExecutionControl, ExecutionPhase, PhasePosition, ProcessExecutionObserver};
use crate::progress::CliProgress;
use crate::runtime::CodexSupportProbeResult;

const JIG_SKILLS_MARKETPLACE_ENV: &str = "JIG_SKILLS_MARKETPLACE";
const CODEX_SUPPORT_PROBE_TIMEOUT: Duration = Duration::from_secs(5);

pub(super) fn dispatch_with_observer(
    ctx: &RepoContext,
    command: AgentCommand,
    observer: &mut dyn ExecutionControl,
) -> Result<JsonValue> {
    // Agent tooling commands describe or mutate local client setup, not repo
    // work evidence, so they intentionally do not record receipts.
    match command {
        AgentCommand::Doctor => Ok(doctor_with_cancellation(ctx, &|| observer.cancelled())),
        AgentCommand::Bootstrap(opts) => bootstrap(ctx, opts, observer),
    }
}

pub(super) fn doctor(ctx: &RepoContext) -> JsonValue {
    doctor_with_codex_support_probe(ctx, codex_supports_plugin_marketplaces)
}

pub(super) fn doctor_for_inventory(ctx: &RepoContext, human_progress: bool) -> JsonValue {
    let progress = if human_progress && !ctx.codex_marketplaces().is_empty() {
        CliProgress::new("info --commands")
    } else {
        CliProgress::disabled("info --commands")
    };
    doctor_with_progress(
        ctx,
        codex_supports_plugin_marketplaces,
        progress,
        "command inventory probe complete",
    )
}

pub(super) fn doctor_with_codex_support_probe(
    ctx: &RepoContext,
    probe: impl FnMut(&OsStr) -> CodexSupportProbeResult,
) -> JsonValue {
    doctor_with_progress(
        ctx,
        probe,
        CliProgress::new("agent doctor"),
        "agent doctor complete",
    )
}

fn doctor_with_cancellation(ctx: &RepoContext, cancelled: &dyn Fn() -> bool) -> JsonValue {
    doctor_with_progress(
        ctx,
        |codex_bin| {
            codex_supports_plugin_marketplaces_with_timeout_and_cancellation(
                codex_bin,
                CODEX_SUPPORT_PROBE_TIMEOUT,
                cancelled,
            )
        },
        CliProgress::new("agent doctor"),
        "agent doctor complete",
    )
}

fn doctor_with_progress(
    ctx: &RepoContext,
    mut probe: impl FnMut(&OsStr) -> CodexSupportProbeResult,
    progress: CliProgress,
    completion_detail: &'static str,
) -> JsonValue {
    progress.header("inspect local Codex tooling");
    progress.info("repo", ctx.root().display());
    let codex_bin = crate::codex::codex_bin();
    let codex_bin_display = codex_bin.to_string_lossy().into_owned();
    progress.step("resolve codex", &codex_bin_display);
    let configured_marketplaces = ctx.codex_marketplaces();
    progress.step(
        "read requirements",
        marketplace_requirement_message(configured_marketplaces.len()),
    );
    // Empty marketplace config intentionally means this repo has no Codex skill requirement.
    let codex_required = !configured_marketplaces.is_empty();
    let codex_probe = if codex_required {
        // We only probe Codex when this repo declares Codex marketplace requirements.
        progress.step("probe codex", "plugin marketplace support");
        Some(probe(&codex_bin))
    } else {
        None
    };
    let (codex_available, codex_probe_error, codex_ready) = match codex_probe {
        Some(Ok(available)) => {
            progress.info("codex support", codex_probe_message(available));
            (Some(available), None, available)
        }
        Some(Err(error)) => {
            progress.info("codex support", "plugin marketplace probe incomplete");
            (None, Some(error), false)
        }
        None => (None, None, true),
    };
    let config_path = crate::codex::codex_config_path();
    progress.step(
        "read codex config",
        config_path
            .as_ref()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| "not found".into()),
    );
    let config = if codex_required {
        config_path
            .as_ref()
            .and_then(|path| std::fs::read_to_string(path).ok())
            .and_then(|text| toml::from_str::<toml::Value>(&text).ok())
    } else {
        None
    };

    let mut marketplaces = Vec::new();
    let mut unregistered_marketplaces = Vec::new();
    for marketplace in configured_marketplaces.iter() {
        let status = marketplace_status(marketplace, config.as_ref(), ctx.root());
        let registered = status
            .get("registered")
            .and_then(JsonValue::as_bool)
            .unwrap_or(false);
        if !registered {
            unregistered_marketplaces.push((marketplace.id.clone(), marketplace.source.clone()));
        }
        marketplaces.push(status);
    }
    let all_marketplaces_ready = if codex_required {
        marketplaces.iter().all(|marketplace| {
            marketplace
                .get("registered")
                .and_then(JsonValue::as_bool)
                .unwrap_or(false)
        })
    } else {
        true
    };
    progress.step(
        "check marketplaces",
        readiness_message(codex_required, all_marketplaces_ready),
    );
    let next_steps = if codex_probe_error.is_some() {
        vec!["Run `scripts/jig agent doctor` after process supervision is available.".into()]
    } else {
        doctor_next_steps(
            &codex_bin_display,
            codex_required,
            codex_ready,
            configured_marketplaces.len(),
            &unregistered_marketplaces,
        )
    };
    if codex_ready && all_marketplaces_ready {
        progress.done(completion_detail);
    } else {
        progress.blocked("Codex marketplace setup is incomplete");
    }

    json!({
        "ok": codex_ready && all_marketplaces_ready,
        "command": "agent doctor",
        "codex": {
            "bin": codex_bin_display,
            "required": codex_required,
            "available": codex_available,
            "probe_skipped": !codex_required,
            "probe_error": codex_probe_error,
            "config_path": config_path.map(|path| path.display().to_string()),
            "config_read": config.is_some()
        },
        "readiness": {
            "ok_requires_marketplaces_registered": codex_required,
            "ok_requires_plugins_enabled": false
        },
        "marketplaces": marketplaces,
        "next_steps": next_steps
    })
}

fn bootstrap(
    ctx: &RepoContext,
    opts: AgentBootstrapRequest,
    observer: &mut dyn ExecutionControl,
) -> Result<JsonValue> {
    let progress = CliProgress::new("agent bootstrap");
    progress.info("repo", ctx.root().display());
    let codex_bin = crate::codex::codex_bin();
    let codex_bin_display = codex_bin.to_string_lossy().into_owned();
    progress.step("resolve codex", &codex_bin_display);
    let marketplace_source =
        progress.log_blocked_on_err(requested_marketplace_source(ctx, opts.marketplace))?;
    progress.step("resolve marketplace", &marketplace_source);
    progress.step(
        "install marketplace",
        format!("{codex_bin_display} plugin marketplace add"),
    );
    let mut command = Command::new(&codex_bin);
    command
        .args(["plugin", "marketplace", "add", &marketplace_source])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let label = "codex marketplace registration";
    let phase = ExecutionPhase::start(observer, label, PhasePosition::single());
    let command_output = run_owned_process_tree_with_output_limits_and_observer(
        &mut command,
        ctx.command_timeout().duration(),
        ProcessOutputLimits {
            stdout: usize::MAX,
            stderr: usize::MAX,
        },
        &mut ProcessExecutionObserver::new(observer, label),
    )
    .map_err(|error| {
        anyhow::anyhow!(
            "Failed to run {codex_bin_display} plugin marketplace add {marketplace_source} under process supervision (timeout: {}s): {error}",
            ctx.command_timeout().as_secs()
        )
    });
    phase.finish(
        observer,
        command_output
            .as_ref()
            .is_ok_and(|output| output.status.success()),
    );
    let output = progress.log_blocked_on_err(command_output)?;
    let output = Output {
        status: output.status,
        stdout: output.stdout.map_or_else(Vec::new, |output| output.bytes),
        stderr: output.stderr.map_or_else(Vec::new, |output| output.bytes),
    };
    if !output.status.success() {
        progress.blocked(format!(
            "Codex exited with {}",
            format_exit_status(&output.status)
        ));
    }
    require_success(&output, |output| {
        codex_marketplace_add_failed_message(&codex_bin_display, &marketplace_source, output)
    })?;
    progress.done("agent bootstrap complete");

    Ok(json!({
        "ok": true,
        "command": "agent bootstrap",
        "codex_bin": codex_bin_display,
        "marketplace_source": marketplace_source,
        "stdout": String::from_utf8_lossy(&output.stdout),
        "stderr": String::from_utf8_lossy(&output.stderr)
    }))
}

fn marketplace_requirement_message(count: usize) -> String {
    match count {
        0 => "no Codex marketplaces required".into(),
        1 => "1 Codex marketplace required".into(),
        count => format!("{count} Codex marketplaces required"),
    }
}

const fn codex_probe_message(codex_available: bool) -> &'static str {
    match codex_available {
        true => "plugin marketplace support available",
        false => "plugin marketplace support unavailable",
    }
}

const fn readiness_message(codex_required: bool, ready: bool) -> &'static str {
    match (codex_required, ready) {
        (false, _) => "not required",
        (true, true) => "registered",
        (true, false) => "missing registration",
    }
}

fn doctor_next_steps(
    codex_bin: &str,
    codex_required: bool,
    codex_ready: bool,
    marketplace_count: usize,
    unregistered_marketplaces: &[(String, String)],
) -> Vec<String> {
    if !codex_required {
        return Vec::new();
    }

    let mut steps = Vec::new();
    if !codex_ready {
        // Codex marketplace registration depends on this command family, so
        // report the prerequisite first and defer registration steps until the
        // binary can run them.
        let marketplace_help_command = format!(
            "{} plugin marketplace add --help",
            shell_single_quote(codex_bin)
        );
        steps.push(format!(
            "Install or update Codex so `{marketplace_help_command}` succeeds, or set JIG_CODEX_BIN to a compatible Codex binary."
        ));
        return steps;
    }

    for (id, source) in unregistered_marketplaces {
        steps.push(format!(
            "Run `{}` to register marketplace {} (source: {}).",
            agent_bootstrap_command(marketplace_count, source),
            id,
            source
        ));
    }

    steps
}

fn agent_bootstrap_command(marketplace_count: usize, source: &str) -> String {
    // A single configured marketplace can be selected by `agent bootstrap`
    // without a flag; multiple configured marketplaces require an explicit
    // source even if only one is currently unregistered.
    if marketplace_count == 1 {
        "scripts/jig agent bootstrap".into()
    } else {
        format!(
            "scripts/jig agent bootstrap --marketplace {}",
            shell_single_quote(source)
        )
    }
}

fn shell_single_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn requested_marketplace_source(ctx: &RepoContext, explicit: Option<String>) -> Result<String> {
    if let Some(source) = explicit.or_else(|| env::var(JIG_SKILLS_MARKETPLACE_ENV).ok()) {
        return marketplace_source_for_codex(&source, ctx.root());
    }

    match ctx.codex_marketplaces() {
        [] => bail!(
            "No Codex marketplaces are configured in agent_tooling.codex.marketplaces; pass --marketplace <source> to install one explicitly"
        ),
        [marketplace] => marketplace_source_for_codex(&marketplace.source, ctx.root()),
        marketplaces => bail!(
            "Multiple Codex marketplaces are configured ({}); pass --marketplace <source> to choose one explicitly",
            marketplaces
                .iter()
                .map(|marketplace| format!("{}={}", marketplace.id, marketplace.source))
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}

fn marketplace_source_for_codex(source: &str, repo_root: &Path) -> Result<String> {
    let trimmed = source.trim();
    if trimmed.is_empty() {
        bail!("Codex marketplace source cannot be empty");
    }
    let path = Path::new(trimmed);
    let repo_relative_path = repo_root.join(path);
    if path.is_absolute() || trimmed.starts_with('.') {
        let resolved = if path.is_absolute() {
            path.to_path_buf()
        } else {
            repo_relative_path
        };
        return resolved
            .canonicalize()
            .with_context(|| {
                format!(
                    "Configured Codex marketplace path {} does not exist from repo root {}",
                    source,
                    repo_root.display()
                )
            })
            .map(|path| path.display().to_string());
    }

    if !valid_remote_marketplace_source(trimmed) {
        bail!(
            "Codex marketplace source '{source}' must be a local path, GitHub owner/repo shorthand, git@ URL, or https:// URL"
        );
    }

    Ok(trimmed.to_string())
}

fn valid_remote_marketplace_source(source: &str) -> bool {
    if source
        .chars()
        .any(|ch| ch.is_whitespace() || ch.is_control())
    {
        return false;
    }
    valid_https_source(source) || valid_git_ssh_source(source) || valid_github_shorthand(source)
}

fn valid_https_source(source: &str) -> bool {
    source
        .strip_prefix("https://")
        .and_then(|rest| rest.split_once('/'))
        .is_some_and(|(host, path)| !host.is_empty() && !path.is_empty())
}

fn valid_git_ssh_source(source: &str) -> bool {
    source
        .strip_prefix("git@")
        .and_then(|rest| rest.split_once(':'))
        .is_some_and(|(host, path)| !host.is_empty() && !path.is_empty())
}

fn valid_github_shorthand(source: &str) -> bool {
    let mut parts = source.split('/');
    let Some(owner) = parts.next() else {
        return false;
    };
    let Some(repo) = parts.next() else {
        return false;
    };
    parts.next().is_none()
        && valid_github_component(owner)
        && valid_github_component(repo.trim_end_matches(".git"))
}

fn valid_github_component(value: &str) -> bool {
    !value.is_empty()
        && value
            .bytes()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, b'-' | b'_' | b'.'))
}

fn codex_marketplace_add_failed_message(
    codex_bin: &str,
    marketplace_source: &str,
    output: &Output,
) -> String {
    format!(
        "{} plugin marketplace add {} failed with {}\nstdout:\n{}\nstderr:\n{}",
        codex_bin,
        marketplace_source,
        format_exit_status(&output.status),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

fn marketplace_status(
    marketplace: &CodexMarketplaceConfig,
    config: Option<&toml::Value>,
    repo_root: &Path,
) -> JsonValue {
    // Current Codex plugin marketplace config is stored as
    // [marketplaces.<id>].source, with optional plugin diagnostics under
    // [plugins."<plugin id>"].enabled.
    let configured_marketplace = config
        .and_then(|config| config.get("marketplaces"))
        .and_then(|marketplaces| marketplaces.get(&marketplace.id));
    let configured_source = configured_marketplace
        .and_then(|marketplace| marketplace.get("source"))
        .and_then(toml::Value::as_str);
    let configured_source_type = configured_marketplace
        .and_then(|marketplace| marketplace.get("source_type"))
        .and_then(toml::Value::as_str);
    let source_matches = configured_source.is_some_and(|configured_source| {
        marketplace_source_matches(&marketplace.source, configured_source, repo_root)
    });
    let registered = configured_source.is_some() && source_matches;
    let plugins: Vec<JsonValue> = marketplace
        .plugins
        .iter()
        .map(|plugin| {
            let enabled = config
                .and_then(|config| config.get("plugins"))
                .and_then(|plugins| plugins.get(plugin))
                .and_then(|plugin| plugin.get("enabled"))
                .and_then(toml::Value::as_bool)
                .unwrap_or(false);
            json!({
                "id": plugin,
                "enabled": enabled
            })
        })
        .collect();
    let plugins_ready = plugins.iter().all(|plugin| {
        plugin
            .get("enabled")
            .and_then(JsonValue::as_bool)
            .unwrap_or(false)
    });

    json!({
        "id": marketplace.id,
        "source": marketplace.source,
        "configured_source": configured_source,
        "configured_source_type": configured_source_type,
        "registered": registered,
        "source_matches": source_matches,
        "plugins_ready": plugins_ready,
        "plugins": plugins
    })
}

fn marketplace_source_matches(expected: &str, configured: &str, repo_root: &Path) -> bool {
    normalized_marketplace_source(expected, repo_root)
        == normalized_marketplace_source(configured, repo_root)
}

fn normalized_marketplace_source(source: &str, repo_root: &Path) -> String {
    let trimmed = source.trim().trim_end_matches('/');
    let path = Path::new(trimmed);
    let resolved = if path.is_absolute() {
        path.to_path_buf()
    } else {
        repo_root.join(path)
    };
    if path.is_absolute() || trimmed.starts_with('.') {
        return resolved
            .canonicalize()
            .unwrap_or(resolved)
            .display()
            .to_string();
    }

    if let Some(github_source) = normalized_github_marketplace(trimmed) {
        return github_source;
    }

    // Keep diagnostics non-fatal: if a local path is currently missing, compare
    // against the repo-root-resolved display path and report source_matches.
    resolved
        .canonicalize()
        .unwrap_or(resolved)
        .display()
        .to_string()
}

fn normalized_github_marketplace(source: &str) -> Option<String> {
    let source = source
        .strip_prefix("https://github.com/")
        .or_else(|| source.strip_prefix("http://github.com/"))
        .or_else(|| source.strip_prefix("git@github.com:"))
        .unwrap_or(source)
        .trim_end_matches(".git")
        .trim_end_matches('/');
    let mut parts = source.split('/');
    let owner = parts.next()?;
    let repo = parts.next()?;
    if owner.is_empty() || repo.is_empty() || parts.next().is_some() {
        return None;
    }
    Some(format!("github:{owner}/{repo}"))
}

fn codex_supports_plugin_marketplaces(codex_bin: &OsStr) -> CodexSupportProbeResult {
    // Codex does not expose a machine-readable feature probe for plugin
    // marketplaces, so doctor checks the concrete subcommand it later needs.
    crate::doctor::standalone_codex_support_probe(codex_bin, CODEX_SUPPORT_PROBE_TIMEOUT)
}

pub(super) fn codex_supports_plugin_marketplaces_with_timeout_and_cancellation(
    codex_bin: &OsStr,
    timeout: Duration,
    cancelled: impl FnMut() -> bool,
) -> CodexSupportProbeResult {
    codex_supports_plugin_marketplaces_with_environment_and_cancellation(
        codex_bin,
        timeout,
        &[],
        cancelled,
    )
}

fn codex_supports_plugin_marketplaces_with_environment_and_cancellation(
    codex_bin: &OsStr,
    timeout: Duration,
    environment: &[(OsString, OsString)],
    cancelled: impl FnMut() -> bool,
) -> CodexSupportProbeResult {
    let mut command = Command::new(codex_bin);
    command
        .args(["plugin", "marketplace", "add", "--help"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .envs(environment.iter().map(|(key, value)| (key, value)));
    crate::shell::sanitize_bash_environment(&mut command);
    let output = match run_owned_process_tree_with_output(&mut command, timeout, cancelled) {
        Ok(output) => output,
        Err(OwnedProcessTreeError::Start(_)) => return Ok(false),
        Err(error) => return Err(format!("Codex marketplace support probe {error}")),
    };
    let Some(stdout) = output.stdout else {
        return Err("Codex marketplace support probe stdout was not captured".into());
    };
    let Some(stderr) = output.stderr else {
        return Err("Codex marketplace support probe stderr was not captured".into());
    };
    if !stdout.complete || !stderr.complete {
        return Err("Codex marketplace support probe output capture did not complete".into());
    }
    if stdout.truncated || stderr.truncated {
        return Err(
            "Codex marketplace support probe output exceeded the diagnostic capture limit".into(),
        );
    }
    Ok(output.status.success())
}

#[cfg(test)]
mod tests {
    use super::shell_single_quote;
    #[cfg(unix)]
    use super::{
        codex_supports_plugin_marketplaces_with_environment_and_cancellation,
        codex_supports_plugin_marketplaces_with_timeout_and_cancellation,
    };
    #[cfg(unix)]
    use std::fs;
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;
    #[cfg(unix)]
    use std::time::Duration;

    #[test]
    fn shell_single_quote_handles_edge_cases() {
        assert_eq!(shell_single_quote(""), "''");
        assert_eq!(shell_single_quote("'"), "''\\'''");
        assert_eq!(shell_single_quote("''"), "''\\'''\\'''");
        assert_eq!(shell_single_quote("path with space"), "'path with space'");
        assert_eq!(
            shell_single_quote("./team's-skills"),
            "'./team'\\''s-skills'"
        );
    }

    #[cfg(unix)]
    #[test]
    fn codex_support_probe_has_a_finite_owned_timeout() {
        let temp = tempfile::tempdir().unwrap();
        let codex = temp.path().join("codex");
        fs::write(&codex, "#!/bin/sh\nwhile :; do :; done\n").unwrap();
        fs::set_permissions(&codex, fs::Permissions::from_mode(0o755)).unwrap();

        let error = codex_supports_plugin_marketplaces_with_timeout_and_cancellation(
            codex.as_os_str(),
            Duration::from_millis(20),
            || false,
        )
        .unwrap_err();

        assert!(error.contains("timed out"), "{error}");
    }

    #[cfg(unix)]
    #[test]
    fn codex_support_probe_sanitizes_bash_controls_and_preserves_ordinary_environment() {
        let temp = tempfile::tempdir().unwrap();
        let codex = temp.path().join("codex");
        let startup = temp.path().join("startup-poison.sh");
        let startup_marker = temp.path().join("startup-poison-ran");
        let trace_marker = temp.path().join("trace-poison-ran");
        fs::write(
            &startup,
            "printf poison > \"$JIG_CODEX_PROBE_STARTUP_MARKER\"\nexit 91\n",
        )
        .unwrap();
        fs::write(
            &codex,
            r#"#!/usr/bin/env bash
if [ -n "${BASH_ENV+x}" ] || [ -n "${ENV+x}" ] || [ -n "${CDPATH+x}" ] || [ -n "${BASH_XTRACEFD+x}" ]; then
  exit 70
fi
if declare -F jig_codex_probe_poison >/dev/null; then
  exit 71
fi
case "$-" in *x*|*v*) exit 72 ;; esac
shopt -q extglob && exit 73
case "$PS4" in *JIG_CODEX_PROBE_PS4_POISON*) exit 74 ;; esac
[ "$JIG_CODEX_PROBE_ORDINARY" = preserved ] || exit 75
[ "$*" = "plugin marketplace add --help" ] || exit 76
"#,
        )
        .unwrap();
        fs::set_permissions(&codex, fs::Permissions::from_mode(0o755)).unwrap();

        let environment = vec![
            ("BASH_ENV".into(), startup.clone().into_os_string()),
            ("ENV".into(), startup.into_os_string()),
            ("CDPATH".into(), temp.path().as_os_str().to_owned()),
            ("SHELLOPTS".into(), "xtrace:verbose".into()),
            ("BASHOPTS".into(), "extglob".into()),
            (
                "PS4".into(),
                "JIG_CODEX_PROBE_PS4_POISON$(printf poison > \"$JIG_CODEX_PROBE_TRACE_MARKER\")"
                    .into(),
            ),
            ("BASH_XTRACEFD".into(), "2".into()),
            (
                "BASH_FUNC_jig_codex_probe_poison%%".into(),
                "() { printf poison > \"$JIG_CODEX_PROBE_STARTUP_MARKER\"; }".into(),
            ),
            (
                "JIG_CODEX_PROBE_STARTUP_MARKER".into(),
                startup_marker.as_os_str().to_owned(),
            ),
            (
                "JIG_CODEX_PROBE_TRACE_MARKER".into(),
                trace_marker.as_os_str().to_owned(),
            ),
            ("JIG_CODEX_PROBE_ORDINARY".into(), "preserved".into()),
        ];

        let available = codex_supports_plugin_marketplaces_with_environment_and_cancellation(
            codex.as_os_str(),
            Duration::from_secs(2),
            &environment,
            || false,
        )
        .unwrap();

        assert!(available);
        assert!(!startup_marker.exists(), "Bash startup poison executed");
        assert!(!trace_marker.exists(), "Bash trace poison executed");
    }

    // macOS rejects invalid UTF-8 pathnames at the filesystem boundary, so the
    // executable-path integration case is exercised on Unix hosts that permit
    // such names. The byte-preserving path helpers have platform-neutral tests.
    #[cfg(target_os = "linux")]
    #[test]
    fn codex_support_probe_accepts_a_non_utf8_executable_path() {
        use std::os::unix::ffi::OsStringExt;

        let temp = tempfile::tempdir().unwrap();
        let codex = temp
            .path()
            .join(std::ffi::OsString::from_vec(b"codex-\xff".to_vec()));
        fs::write(&codex, "#!/bin/sh\nexit 0\n").unwrap();
        fs::set_permissions(&codex, fs::Permissions::from_mode(0o755)).unwrap();

        let available = codex_supports_plugin_marketplaces_with_timeout_and_cancellation(
            codex.as_os_str(),
            Duration::from_secs(1),
            || false,
        )
        .unwrap();

        assert!(available);
    }
}
