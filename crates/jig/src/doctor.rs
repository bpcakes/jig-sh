use std::path::Path;

use std::{env, fs};

use anyhow::{Context, Result};

use jig_context::{
    JIG_REPO_ROOT_ENV, RepoContext, find_repo_root_from, find_repo_root_from_or_env,
};

use runtime::{
    contract_migration_check, launcher_repair_cache_check, launcher_repair_seed_stamp_is_present,
    launcher_repair_staging_check, legacy_version_cache_check, runtime_check,
};
use serde_json::{Value, json};

use self::check::{DoctorCheck, check};
use self::context_checks::{doctor_context_checks, doctor_context_checks_with_cancellation};
use self::vault::vault_check;

mod agent;
mod cargo_sqlx;
mod check;
mod context_checks;
mod database_url;
mod environment;
mod programs;
mod proxy;
mod required_tools;
mod run_history;
mod runtime;
mod shell_analysis;
mod sqlx_cli;
mod sqlx_driver;
mod sqlx_driver_probe;
mod toolchains;
mod vault;
mod version_authority;

pub(crate) use self::agent::standalone_codex_support_probe;
pub(crate) use self::programs::program_available_on_path;
pub(crate) use self::proxy::proxy_configured;
pub(crate) use self::version_authority::go_version_selector;

const COMMAND: &str = "doctor";

pub(crate) fn run() -> Result<Value> {
    run_with_optional_cancellation(None)
}

pub(crate) fn run_with_cancellation(cancelled: &dyn Fn() -> bool) -> Result<Value> {
    run_with_optional_cancellation(Some(cancelled))
}

fn run_with_optional_cancellation(cancelled: Option<&dyn Fn() -> bool>) -> Result<Value> {
    let cwd = env::current_dir().context("Failed to resolve current directory")?;
    // Doctor is capability-only so it can diagnose an invalid generated
    // launcher contract. Its explicit JIG_REPO_ROOT target therefore remains
    // authoritative instead of inheriting repository-scoped launcher state.
    let root_result = find_repo_root_from_or_env(&cwd);
    let mut checks = Vec::new();

    let root = match root_result {
        Ok(root) => root,
        Err(error) => {
            checks.push(check(
                "repo",
                "Jig repo",
                true,
                false,
                "missing",
                error.to_string(),
            ).with_fix("Run `scripts/jig adopt . --repo-name <name> --sqlx-enabled false` from the repository root. To create a new repo, choose one prompt-free shape: `scripts/jig init <path> --preset rust-react --db none --frontend web --no-input --no-vault`; `scripts/jig init <path> --preset harness-only --repo-name <name> --sqlx-enabled false --no-input --no-vault`; `scripts/jig init <path> --preset go-react --db none --frontend web --go-module example.com/<name> --no-input --no-vault`; `scripts/jig init <path> --preset rust-library --no-input --no-vault`; or `scripts/jig init <path> --preset rust-cli --no-input --no-vault`. Run `scripts/jig init <path>` interactively to choose the same five shapes."));
            return Ok(output(None, checks));
        }
    };
    if let Some(notice) = doctor_root_override_notice(&cwd, &root) {
        eprintln!("{notice}");
    }

    checks.push(
        check(
            "repo",
            "Jig repo",
            true,
            true,
            "found",
            root.display().to_string(),
        )
        .with_data(json!({ "root": root.display().to_string() })),
    );

    let config_probe = RepoContext::validate_config_file(&root);
    let ctx_result = RepoContext::load_from_root(root.clone());
    let manifest_contract_version = RepoContext::declared_contract_version_from_root(&root).ok();
    let (config_ok, repo_name, config_jig_version) = match &config_probe {
        Ok(probe) => (
            true,
            Some(probe.repo_name.clone()),
            probe.jig_version.clone(),
        ),
        Err(_) => (false, None, None),
    };
    let config_valid_for_launcher_repair =
        config_ok && jig_bootstrap::launcher_only_repair_answers_are_valid(&root);
    checks.push(config_check(&root, &config_probe));
    checks.push(runtime_check(
        &root,
        manifest_contract_version,
        config_jig_version.as_deref(),
        config_valid_for_launcher_repair,
    ));
    if let Some(staging_check) = launcher_repair_staging_check(&root) {
        checks.push(staging_check);
    }
    if let Some(legacy_cache_check) = legacy_version_cache_check(&root) {
        checks.push(legacy_cache_check);
    }
    if let Some(contract_version) = manifest_contract_version
        .filter(|version| launcher_repair_seed_stamp_is_present(&root, *version))
    {
        checks.push(launcher_repair_cache_check(&root, contract_version));
    }
    if let Some(contract_version) = manifest_contract_version.filter(|version| {
        jig_context::is_supported_contract_version(*version)
            && *version < jig_context::CURRENT_CONTRACT_VERSION
    }) {
        checks.push(contract_migration_check(&root, contract_version));
    }

    match &ctx_result {
        Ok(ctx) => {
            checks.push(contract_check(ctx));
            let context_checks = match cancelled {
                Some(cancelled) => doctor_context_checks_with_cancellation(ctx, cancelled),
                None => doctor_context_checks(ctx),
            };
            checks.push(context_checks.required_tools);
            if let Some(rust_runtime) = context_checks.rust_runtime {
                checks.push(rust_runtime);
            }
            if let Some(go_runtime) = context_checks.go_runtime {
                checks.push(go_runtime);
            }
            if let Some(node_runtime) = context_checks.node_runtime {
                checks.push(node_runtime);
            }
            if let Some(sqlx_cli) = context_checks.sqlx_cli {
                checks.push(sqlx_cli);
            }
            checks.push(context_checks.agent);
            checks.push(context_checks.proxy);
        }
        Err(error) => {
            let context_error = if config_ok {
                format!("Repo context failed to load: {error}")
            } else {
                format!("Skipped until .jig.toml is valid: {error}")
            };
            checks.push(
                check(
                    "contract",
                    "Contract",
                    true,
                    false,
                    "blocked",
                    context_error.clone(),
                )
                .with_fix("Run `scripts/jig check contract` after fixing the reported repo configuration issue."),
            );
            checks.push(
                check(
                    "required_tools",
                    "Required tools",
                    true,
                    false,
                    "blocked",
                    format!("Skipped until repo context loads successfully: {context_error}"),
                )
                .with_fix("Run `scripts/jig check contract` first."),
            );
            checks.push(
                check(
                    "agent_skills",
                    "Agent skills",
                    false,
                    false,
                    "blocked",
                    format!("Skipped until repo context loads successfully: {context_error}"),
                )
                .with_fix("Run `scripts/jig doctor` after fixing the contract issue."),
            );
            checks.push(
                check(
                    "proxy",
                    "Dev proxy",
                    false,
                    false,
                    "blocked",
                    format!("Skipped until repo context loads successfully: {context_error}"),
                )
                .with_fix("Run `scripts/jig doctor` after fixing the contract issue."),
            );
        }
    }

    checks.push(vault_check(
        ctx_result
            .as_ref()
            .map_err(std::string::ToString::to_string),
    ));
    if let Some(history) = run_history::check_local_history(&root, cancelled) {
        checks.push(history);
    }

    Ok(output(
        Some(json!({
            "root": root.display().to_string(),
            "name": repo_name,
            "jig_version": config_jig_version,
            "runtime_version": env!("CARGO_PKG_VERSION"),
            "contract_version": manifest_contract_version,
        })),
        checks,
    ))
}

fn doctor_root_override_notice(cwd: &Path, selected_root: &Path) -> Option<String> {
    if !RepoContext::repo_root_override_is_set() {
        return None;
    }
    let local_root = find_repo_root_from(cwd).ok()?;
    let local_root = fs::canonicalize(local_root).ok()?;
    (local_root != selected_root).then(|| {
        format!(
            "jig doctor is using {JIG_REPO_ROOT_ENV}={} instead of the repository containing its invocation directory {}; unset {JIG_REPO_ROOT_ENV} to diagnose {}",
            selected_root.display(),
            local_root.display(),
            local_root.display(),
        )
    })
}

fn output(repo: Option<Value>, checks: Vec<DoctorCheck>) -> Value {
    let required_ok = checks.iter().all(|check| !check.required || check.ok);
    let next_required_issue = checks.iter().find(|check| check.required && !check.ok);
    let next_optional_issue = required_ok
        .then(|| {
            checks
                .iter()
                .find(|check| !check.required && !check.ok && !check.operator_only)
        })
        .flatten();
    // Operator-only setup needs a human-chosen secret, so it is reported apart
    // from every agent-facing next step.
    let operator_setup = checks
        .iter()
        .find(|check| check.operator_only && !check.ok)
        .and_then(|check| check.fix.clone());
    let next_issue = next_required_issue.or(next_optional_issue);
    let next_step = next_issue.and_then(|check| check.fix.clone());
    let next_required_step = next_required_issue.and_then(|check| check.fix.clone());
    let optional_setup = next_optional_issue.and_then(|check| check.fix.clone());
    let next_issue = next_issue.map(|check| {
        json!({
            "id": &check.id,
            "label": &check.label,
            "required": check.required,
            "status": &check.status,
            "fix": &check.fix,
        })
    });
    let checks = serde_json::to_value(checks).expect("doctor checks serialize");

    json!({
        "ok": required_ok,
        "command": COMMAND,
        "repo": repo,
        "checks": checks,
        "next_issue": next_issue,
        "next_required_step": next_required_step,
        "optional_setup": optional_setup,
        "operator_setup": operator_setup,
        "next_step": next_step,
    })
}

fn config_check(root: &Path, result: &Result<jig_context::RepoConfigProbe>) -> DoctorCheck {
    match result {
        Ok(probe) => check(
            "config",
            ".jig.toml",
            true,
            true,
            "valid",
            match &probe.jig_version {
                Some(version) => format!(
                    "repo_name={}, legacy jig_version={version}",
                    probe.repo_name
                ),
                None => format!("repo_name={}", probe.repo_name),
            },
        )
        .with_data(json!({
            "path": root.join(".jig.toml").display().to_string(),
            "repo_name": probe.repo_name,
            "jig_version": probe.jig_version,
        })),
        Err(error) => check(
            "config",
            ".jig.toml",
            true,
            false,
            "invalid",
            error.to_string(),
        )
        .with_fix("Fix `.jig.toml`, then run `scripts/jig doctor`.")
        .with_data(json!({ "path": root.join(".jig.toml").display().to_string() })),
    }
}

fn contract_check(ctx: &RepoContext) -> DoctorCheck {
    let output = jig_policy::contract_check(ctx);
    if output.exit_status == 0 {
        check(
            "contract",
            "Contract",
            true,
            true,
            "valid",
            output.stdout.trim().to_string(),
        )
        .with_data(json!({ "exit_status": output.exit_status }))
    } else {
        check(
            "contract",
            "Contract",
            true,
            false,
            "invalid",
            output.stderr.trim().to_string(),
        )
        .with_fix("Run `scripts/jig check contract` for the full contract report.")
        .with_data(json!({
            "exit_status": output.exit_status,
            "stdout": output.stdout,
            "stderr": output.stderr,
        }))
    }
}

#[cfg(test)]
mod tests;
