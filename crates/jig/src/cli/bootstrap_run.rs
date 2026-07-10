use std::io::{self, Write};

use anyhow::{Context, Result};
use serde_json::{Value, json};

use super::output::print_json;
use crate::{bootstrap, context::RepoContext, runtime};

pub(super) fn run_init_command(opts: bootstrap::InitOpts, json_output: bool) -> Result<()> {
    let vault_setup = prepare_bootstrap_vault(!opts.no_vault, opts.no_input, opts.defaults)?;
    let mut output = bootstrap::run_init(opts)?;
    let vault = ensure_bootstrap_vault(&output, vault_setup.requested, !vault_setup.pre_captured)?;
    attach_bootstrap_vault(&mut output, vault, "bootstrap::run_init")?;
    if json_output {
        print_json(&output)
    } else {
        print_human_summary(format_init_human_summary(&output))
    }
}

pub(super) fn run_presets_command(json_output: bool) -> Result<()> {
    let output = bootstrap::scaffold_presets_report();
    if json_output {
        print_json(&output)
    } else {
        print_human_summary(format_presets_human_summary(&output))
    }
}

pub(super) fn run_adopt_command(opts: bootstrap::AdoptOpts, json_output: bool) -> Result<()> {
    let vault_setup =
        prepare_bootstrap_vault(opts.write && !opts.no_vault, opts.no_input, opts.defaults)?;
    let mut output = bootstrap::run_adopt(opts)?;
    let vault = ensure_bootstrap_vault(&output, vault_setup.requested, !vault_setup.pre_captured)?;
    attach_bootstrap_vault(&mut output, vault, "bootstrap::run_adopt")?;
    if json_output {
        print_json(&output)
    } else {
        print_human_summary(format_adopt_human_summary(&output))
    }
}

pub(super) fn run_update_command(opts: bootstrap::UpdateOpts, json_output: bool) -> Result<()> {
    let output = bootstrap::run_update(opts)?;
    if json_output {
        print_json(&output)
    } else {
        print_human_summary(format_update_human_summary(&output))
    }
}

fn attach_bootstrap_vault(output: &mut Value, vault: Value, source: &str) -> Result<()> {
    if output.get("vault").is_some() {
        anyhow::bail!("{source} output unexpectedly included a vault field");
    }
    output["vault"] = vault;
    Ok(())
}

#[derive(Clone, Copy, Debug)]
struct BootstrapVaultSetup {
    requested: bool,
    pre_captured: bool,
}

fn prepare_bootstrap_vault(
    requested: bool,
    no_input: bool,
    defaults: bool,
) -> Result<BootstrapVaultSetup> {
    let env_present = runtime::vault_passphrase_env_present();
    let pre_captured = should_pre_capture_bootstrap_vault(
        requested,
        no_input,
        defaults,
        env_present,
        runtime::vault_passphrase_prompt_available(),
    );
    reject_missing_no_input_vault_passphrase(requested, no_input, env_present)?;
    if pre_captured {
        runtime::capture_new_vault_passphrase()?;
    }
    Ok(BootstrapVaultSetup {
        requested,
        pre_captured,
    })
}

pub(super) fn should_pre_capture_bootstrap_vault(
    requested: bool,
    no_input: bool,
    defaults: bool,
    env_present: bool,
    prompt_available: bool,
) -> bool {
    // `--defaults` is treated as automation intent for vault setup even though
    // it can still leave ordinary answer prompts interactive.
    requested && (no_input || defaults || env_present || !prompt_available)
}

pub(super) fn reject_missing_no_input_vault_passphrase(
    requested: bool,
    no_input: bool,
    env_present: bool,
) -> Result<()> {
    if requested && no_input && !env_present {
        anyhow::bail!(
            "JIG_VAULT_PASSPHRASE is required when --no-input initializes a vault; export JIG_VAULT_PASSPHRASE or pass --no-vault to skip initial vault setup"
        );
    }
    Ok(())
}

pub(super) fn ensure_bootstrap_vault(
    output: &serde_json::Value,
    requested: bool,
    capture_passphrase: bool,
) -> Result<serde_json::Value> {
    let destination = output["destination"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("bootstrap output did not include destination"))?;
    if !requested {
        return Ok(json!({
            "requested": false,
            "initialized": false,
            "created": false,
            "skipped_reason": "disabled",
        }));
    }

    let ctx =
        RepoContext::load_from_root(std::path::PathBuf::from(destination)).with_context(|| {
            "vault auto-init could not load the rendered repo context after repo files were written; fix the reported .jig.toml or .agent/jig-contract.json issue before rerunning `jig vault init`"
        })?;
    let Some(vault) = runtime::repo_vault_options_for_context(&ctx) else {
        return Ok(json!({
            "requested": true,
            "initialized": false,
            "created": false,
            "skipped_reason": "repo has no [vault] scope",
        }));
    };
    let status = runtime::dispatch_vault(crate::command::VaultCommand::Status(
        crate::command::VaultStatusRequest {
            vault: vault.clone(),
        },
    ))
    .context("vault auto-init status check failed after repo files were written; rerun `jig vault status` from the repo after fixing the reported vault issue")?;
    if status["exists"].as_bool().unwrap_or(false) {
        return Ok(json!({
            "requested": true,
            "initialized": true,
            "created": false,
            "vault_home": status["vault_home"],
            "vault_scope": status["vault_scope"],
            "vault_scope_id": status["vault_scope_id"],
        }));
    }

    if capture_passphrase {
        runtime::capture_new_vault_passphrase().context(
            "vault auto-init passphrase capture failed after repo files were written; rerun `jig vault init` from the repo after fixing the reported vault issue",
        )?;
    }
    let init = runtime::dispatch_vault(crate::command::VaultCommand::Init(
        crate::command::VaultInitRequest { vault },
    ))
    .context("vault auto-init failed after repo files were written; rerun `jig vault init` from the repo after fixing the reported vault issue")?;
    Ok(json!({
        "requested": true,
        "initialized": true,
        "created": true,
        "vault_home": init["vault_home"],
        "vault_scope": init["vault_scope"],
        "vault_scope_id": init["vault_scope_id"],
    }))
}

fn print_human_summary(summary: String) -> Result<()> {
    let mut stdout = io::stdout().lock();
    stdout.write_all(summary.as_bytes())?;
    Ok(())
}

pub(super) fn format_presets_human_summary(output: &serde_json::Value) -> String {
    let mut summary = String::new();
    summary.push_str("available presets\n");
    let Some(presets) = output["presets"].as_array() else {
        summary.push_str("  Preset report did not include a presets list.\n");
        return summary;
    };
    if presets.is_empty() {
        summary.push_str("  No presets are currently registered.\n");
        return summary;
    }
    for (index, preset) in presets.iter().enumerate() {
        if index > 0 {
            summary.push('\n');
        }
        let name = preset["name"].as_str().unwrap_or("<unknown>");
        let summary_text = preset["summary"].as_str().unwrap_or("");
        summary.push_str(&format!("  {name}\n"));
        if !summary_text.is_empty() {
            summary.push_str(&format!("    {summary_text}\n"));
        }
        if let Some(defaults) = preset["defaults"].as_array()
            && !defaults.is_empty()
        {
            summary.push_str("    defaults:\n");
            for default in defaults.iter().filter_map(serde_json::Value::as_str) {
                summary.push_str(&format!("      - {default}\n"));
            }
        }
        if let Some(layout) = preset["layout"].as_array()
            && !layout.is_empty()
        {
            summary.push_str("    generated layout:\n");
            for path in layout.iter().filter_map(serde_json::Value::as_str) {
                summary.push_str(&format!("      - {path}\n"));
            }
        }
        if let Some(frontends) = preset["frontend_shorthands"].as_array()
            && !frontends.is_empty()
        {
            summary.push_str("    frontend shorthands:\n");
            for frontend in frontends {
                let shorthand = frontend["name"].as_str().unwrap_or("<unknown>");
                let expands_to = frontend["expands_to"].as_str().unwrap_or("");
                summary.push_str(&format!("      - {shorthand}: {expands_to}\n"));
            }
        }
        if let Some(examples) = preset["examples"].as_array()
            && !examples.is_empty()
        {
            summary.push_str("    examples:\n");
            for example in examples.iter().filter_map(serde_json::Value::as_str) {
                summary.push_str(&format!("      {example}\n"));
            }
        }
        if let Some(ownership) = preset["ownership"].as_str() {
            summary.push_str("    ownership:\n");
            summary.push_str(&format!("      - {ownership}\n"));
        }
        if let Some(non_goals) = preset["non_goals"].as_array()
            && !non_goals.is_empty()
        {
            summary.push_str("    non-goals:\n");
            for non_goal in non_goals.iter().filter_map(serde_json::Value::as_str) {
                summary.push_str(&format!("      - {non_goal}\n"));
            }
        }
    }
    summary
}

pub(super) fn format_init_human_summary(output: &serde_json::Value) -> String {
    let mut summary = String::new();
    summary.push_str("init summary\n");
    push_summary_field(&mut summary, "target", output["destination"].as_str());
    push_summary_field(&mut summary, "template", output["template"].as_str());

    let report = &output["render_report"];
    let created = array_len(&report["files_created"]);
    let modified = array_len(&report["files_modified"]);
    let removed = array_len(&report["files_removed"]);
    summary.push_str(&format!(
        "  managed files: {created} created, {modified} modified, {removed} removed\n"
    ));

    if let Some(scaffold) = output.get("scaffold").filter(|value| !value.is_null()) {
        let preset = scaffold["preset"].as_str().unwrap_or("<unknown>");
        let db = scaffold["db"].as_str().unwrap_or("<unknown>");
        summary.push_str(&format!("  scaffold: {preset}"));
        if let Some(repo_name) = scaffold["repo_name"].as_str() {
            summary.push_str(&format!(" for {repo_name}"));
        }
        summary.push_str(&format!(" (db: {db})\n"));
        let scaffold_created = array_len(&scaffold["files_created"]);
        let scaffold_modified = array_len(&scaffold["files_modified"]);
        let scaffold_unchanged = array_len(&scaffold["files_unchanged"]);
        summary.push_str(&format!(
            "  scaffold files: {scaffold_created} created, {scaffold_modified} modified, {scaffold_unchanged} unchanged\n"
        ));

        if let Some(frontends) = scaffold["frontends"].as_array()
            && !frontends.is_empty()
        {
            let names = frontends
                .iter()
                .filter_map(|frontend| frontend["name"].as_str())
                .collect::<Vec<_>>();
            if !names.is_empty() {
                summary.push_str(&format!("  frontends: {}\n", names.join(", ")));
            }
        }
    }

    if let Some(git_initialized) = output["git_initialized"].as_bool() {
        summary.push_str(&format!(
            "  git: {}\n",
            if git_initialized {
                "initialized"
            } else {
                "already present"
            }
        ));
    }

    push_vault_summary(&mut summary, &output["vault"]);

    if let Some(notes) = output["notes"].as_array()
        && !notes.is_empty()
    {
        summary.push_str("  notes:\n");
        for note in notes.iter().take(5).filter_map(serde_json::Value::as_str) {
            summary.push_str(&format!("    - {note}\n"));
        }
        if notes.len() > 5 {
            summary.push_str(&format!("    - and {} more\n", notes.len() - 5));
        }
    }

    if let Some(steps) = output["next_steps"].as_array()
        && !steps.is_empty()
    {
        summary.push_str("  next steps:\n");
        for step in steps.iter().filter_map(serde_json::Value::as_str) {
            summary.push_str(&format!("    - {step}\n"));
        }
    }

    summary.push_str("  full report: rerun with --json\n");
    summary
}

pub(super) fn format_update_human_summary(output: &serde_json::Value) -> String {
    let mut summary = String::new();
    summary.push_str("update summary\n");
    push_summary_field(&mut summary, "mode", output["render_mode"].as_str());
    push_summary_field(&mut summary, "target", output["destination"].as_str());
    push_summary_field(&mut summary, "answers", output["answers_file"].as_str());

    let report = &output["render_report"];
    let created = array_len(&report["files_created"]);
    let modified = array_len(&report["files_modified"]);
    let removed = array_len(&report["files_removed"]);
    let unchanged = array_len(&report["files_unchanged"]);
    summary.push_str(&format!(
        "  managed files: {created} created, {modified} modified, {removed} removed, {unchanged} unchanged\n"
    ));

    if let Some(conflicts) = report["conflicts"].as_array()
        && !conflicts.is_empty()
    {
        push_conflict_summary(&mut summary, "conflicts accepted", conflicts);
    }

    summary.push_str("  full report: rerun with --json\n");
    summary
}

pub(super) fn format_adopt_human_summary(output: &serde_json::Value) -> String {
    let mut summary = String::new();
    summary.push_str("adopt summary\n");
    push_summary_field(&mut summary, "mode", output["render_mode"].as_str());
    push_summary_field(
        &mut summary,
        "footprint",
        output["harness_footprint"].as_str(),
    );
    push_summary_field(&mut summary, "target", output["destination"].as_str());

    let report = &output["render_report"];
    let created = array_len(&report["files_created"]);
    let modified = array_len(&report["files_modified"]);
    let removed = array_len(&report["files_removed"]);
    summary.push_str(&format!(
        "  managed files: {created} created, {modified} modified, {removed} removed\n"
    ));

    push_vault_summary(&mut summary, &output["vault"]);

    if let Some(review) = output["adoption_review"].as_array()
        && !review.is_empty()
    {
        summary.push_str("  review:\n");
        for item in review.iter().filter_map(serde_json::Value::as_str) {
            summary.push_str(&format!("    - {item}\n"));
        }
    }

    if let Some(notes) = output["notes"].as_array()
        && !notes.is_empty()
    {
        summary.push_str("  notes:\n");
        for note in notes.iter().take(8).filter_map(serde_json::Value::as_str) {
            summary.push_str(&format!("    - {note}\n"));
        }
        if notes.len() > 8 {
            summary.push_str(&format!("    - and {} more\n", notes.len() - 8));
        }
    }

    if let Some(conflicts) = report["conflicts"].as_array()
        && !conflicts.is_empty()
    {
        push_conflict_summary(&mut summary, "conflicts", conflicts);
    }

    if let Some(warnings) = output["detection_report"]["warnings"].as_array()
        && !warnings.is_empty()
    {
        summary.push_str(&format!("  warnings: {}\n", warnings.len()));
        for warning in warnings
            .iter()
            .take(5)
            .filter_map(serde_json::Value::as_str)
        {
            summary.push_str(&format!("    - {warning}\n"));
        }
        if warnings.len() > 5 {
            summary.push_str(&format!("    - and {} more\n", warnings.len() - 5));
        }
    }

    if let Some(steps) = output["next_steps"].as_array()
        && !steps.is_empty()
    {
        summary.push_str("  next steps:\n");
        for step in steps.iter().filter_map(serde_json::Value::as_str) {
            summary.push_str(&format!("    - {step}\n"));
        }
    }
    summary
}

fn push_conflict_summary(summary: &mut String, label: &str, conflicts: &[serde_json::Value]) {
    summary.push_str(&format!("  {label}: {}\n", conflicts.len()));
    for conflict in conflicts.iter().take(10) {
        let Some(path) = conflict["path"].as_str() else {
            continue;
        };
        if let Some(detail) = conflict["detail"].as_str() {
            summary.push_str(&format!("    - {path}: {detail}\n"));
        } else {
            summary.push_str(&format!("    - {path}\n"));
        }
    }
    if conflicts.len() > 10 {
        summary.push_str(&format!("    - and {} more\n", conflicts.len() - 10));
    }
}

fn push_vault_summary(summary: &mut String, vault: &serde_json::Value) {
    if vault.is_null() {
        return;
    }
    let requested = vault["requested"].as_bool().unwrap_or(false);
    if !requested {
        summary.push_str("  vault: skipped\n");
        return;
    }
    if let Some(reason) = vault["skipped_reason"].as_str() {
        summary.push_str(&format!("  vault: skipped ({reason})\n"));
        return;
    }
    let status = if vault["created"].as_bool().unwrap_or(false) {
        "created"
    } else if vault["initialized"].as_bool().unwrap_or(false) {
        "already initialized"
    } else {
        "not initialized"
    };
    summary.push_str(&format!("  vault: {status}"));
    if let Some(scope) = vault["vault_scope"].as_str() {
        summary.push_str(&format!(" ({scope})"));
    }
    summary.push('\n');
}

fn push_summary_field(summary: &mut String, label: &str, value: Option<&str>) {
    if let Some(value) = value {
        summary.push_str(&format!("  {label}: {value}\n"));
    }
}

fn array_len(value: &serde_json::Value) -> usize {
    value.as_array().map(Vec::len).unwrap_or(0)
}

#[cfg(test)]
#[path = "bootstrap_run_tests.rs"]
mod tests;
