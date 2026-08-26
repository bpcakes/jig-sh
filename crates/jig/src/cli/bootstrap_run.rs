use std::fmt::Write as _;
use std::io::{self, Write};

use anyhow::{Context, Result};
use serde_json::Value;

use super::init_wizard::{preflight_init_package_manager, prepare_init_interaction};
use super::output::print_json;
use crate::{bootstrap, context::RepoContext, runtime};

pub(super) fn run_init_command(mut opts: bootstrap::InitOpts, json_output: bool) -> Result<()> {
    bootstrap::preflight_init_destination(&opts)?;
    prepare_init_interaction(&mut opts)?;
    preflight_init_package_manager(&opts)?;
    let vault_setup = prepare_bootstrap_vault(
        BootstrapVaultIntent::from_requested(!opts.no_vault),
        BootstrapInputMode::from_flags(opts.no_input, opts.defaults),
        BootstrapVaultCommand::Init,
    )?;
    let mut output = bootstrap::run_init(opts)?;
    let vault = ensure_bootstrap_vault(output.destination(), vault_setup)?;
    output.attach_vault(vault)?;
    if json_output {
        print_json(&serde_json::to_value(&output)?)
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
    let vault_setup = prepare_bootstrap_vault(
        BootstrapVaultIntent::from_requested(opts.write && !opts.no_vault),
        BootstrapInputMode::from_flags(opts.no_input, opts.defaults),
        BootstrapVaultCommand::Adopt,
    )?;
    let mut output = bootstrap::run_adopt(opts)?;
    let destination = output["destination"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("bootstrap output did not include destination"))?;
    let vault = ensure_bootstrap_vault(destination, vault_setup)?;
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

fn attach_bootstrap_vault(
    output: &mut Value,
    vault: bootstrap::BootstrapVaultReport,
    source: &str,
) -> Result<()> {
    if output.get("vault").is_some() {
        anyhow::bail!("{source} output unexpectedly included a vault field");
    }
    output["vault"] = serde_json::to_value(vault)?;
    Ok(())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum BootstrapVaultIntent {
    Disabled,
    Initialize,
}

impl BootstrapVaultIntent {
    const fn from_requested(requested: bool) -> Self {
        if requested {
            Self::Initialize
        } else {
            Self::Disabled
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum BootstrapInputMode {
    Interactive,
    Defaults,
    NoInput,
}

impl BootstrapInputMode {
    const fn from_flags(no_input: bool, defaults: bool) -> Self {
        if no_input {
            Self::NoInput
        } else if defaults {
            Self::Defaults
        } else {
            Self::Interactive
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum BootstrapPassphraseAvailability {
    Environment,
    Prompt,
    Unavailable,
}

impl BootstrapPassphraseAvailability {
    const fn resolve(env_present: bool, prompt_available: bool) -> Self {
        if env_present {
            Self::Environment
        } else if prompt_available {
            Self::Prompt
        } else {
            Self::Unavailable
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum BootstrapVaultPlan {
    Disabled,
    PreCaptured,
    CaptureAfterRender,
}

#[derive(Clone, Copy, Debug)]
pub(super) enum BootstrapVaultCommand {
    Init,
    Adopt,
}

impl BootstrapVaultCommand {
    const fn invocation(self) -> &'static str {
        match self {
            Self::Init => "jig init",
            Self::Adopt => "jig adopt --write",
        }
    }
}

fn prepare_bootstrap_vault(
    intent: BootstrapVaultIntent,
    input_mode: BootstrapInputMode,
    command: BootstrapVaultCommand,
) -> Result<BootstrapVaultPlan> {
    let availability = BootstrapPassphraseAvailability::resolve(
        runtime::vault_passphrase_env_present(),
        runtime::vault_passphrase_prompt_available(),
    );
    let plan = BootstrapVaultPlan::resolve(intent, input_mode, availability, command)?;
    if plan == BootstrapVaultPlan::PreCaptured {
        runtime::capture_new_vault_passphrase()?;
    }
    Ok(plan)
}

impl BootstrapVaultPlan {
    fn resolve(
        intent: BootstrapVaultIntent,
        input_mode: BootstrapInputMode,
        availability: BootstrapPassphraseAvailability,
        command: BootstrapVaultCommand,
    ) -> Result<Self> {
        if intent == BootstrapVaultIntent::Disabled {
            return Ok(Self::Disabled);
        }
        if availability == BootstrapPassphraseAvailability::Unavailable
            || (input_mode == BootstrapInputMode::NoInput
                && availability != BootstrapPassphraseAvailability::Environment)
        {
            anyhow::bail!(
                "JIG_VAULT_PASSPHRASE is required because `{}` cannot prompt for an initial vault passphrase in non-interactive mode; pass --no-vault to skip initial vault setup, or export JIG_VAULT_PASSPHRASE",
                command.invocation()
            );
        }
        if input_mode == BootstrapInputMode::Interactive
            && availability == BootstrapPassphraseAvailability::Prompt
        {
            return Ok(Self::CaptureAfterRender);
        }
        // `--defaults` is treated as automation intent for vault setup even though
        // it can still leave ordinary answer prompts interactive.
        Ok(Self::PreCaptured)
    }
}

fn ensure_bootstrap_vault(
    destination: &str,
    plan: BootstrapVaultPlan,
) -> Result<bootstrap::BootstrapVaultReport> {
    if plan == BootstrapVaultPlan::Disabled {
        return Ok(bootstrap::BootstrapVaultReport::disabled());
    }

    let ctx =
        RepoContext::load_from_root(std::path::PathBuf::from(destination)).with_context(|| {
            "vault auto-init could not load the rendered repo context after repo files were written; fix the reported .jig.toml or .agent/jig-contract.json issue before rerunning `jig vault init`"
        })?;
    let Some(vault) = runtime::repo_vault_options_for_context(&ctx) else {
        return Ok(bootstrap::BootstrapVaultReport::missing_scope());
    };
    let status = runtime::dispatch_vault(crate::command::VaultCommand::Status(
        crate::command::VaultStatusRequest {
            vault: vault.clone(),
        },
    ))
    .context("vault auto-init status check failed after repo files were written; rerun `jig vault status` from the repo after fixing the reported vault issue")?;
    if status["exists"].as_bool().unwrap_or(false) {
        return Ok(bootstrap::BootstrapVaultReport::initialized(false, &status));
    }

    if plan == BootstrapVaultPlan::CaptureAfterRender {
        runtime::capture_new_vault_passphrase().context(
            "vault auto-init passphrase capture failed after repo files were written; rerun `jig vault init` from the repo after fixing the reported vault issue",
        )?;
    }
    let init = runtime::dispatch_vault(crate::command::VaultCommand::Init(
        crate::command::VaultInitRequest { vault },
    ))
    .context("vault auto-init failed after repo files were written; rerun `jig vault init` from the repo after fixing the reported vault issue")?;
    Ok(bootstrap::BootstrapVaultReport::initialized(true, &init))
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
        let _ = writeln!(summary, "  {name}");
        if !summary_text.is_empty() {
            let _ = writeln!(summary, "    {summary_text}");
        }
        if let Some(defaults) = preset["defaults"].as_array()
            && !defaults.is_empty()
        {
            summary.push_str("    defaults:\n");
            for default in defaults.iter().filter_map(serde_json::Value::as_str) {
                let _ = writeln!(summary, "      - {default}");
            }
        }
        if let Some(layout) = preset["layout"].as_array()
            && !layout.is_empty()
        {
            summary.push_str("    generated layout:\n");
            for path in layout.iter().filter_map(serde_json::Value::as_str) {
                let _ = writeln!(summary, "      - {path}");
            }
        }
        if let Some(frontends) = preset["frontend_shorthands"].as_array()
            && !frontends.is_empty()
        {
            summary.push_str("    frontend shorthands:\n");
            for frontend in frontends {
                let shorthand = frontend["name"].as_str().unwrap_or("<unknown>");
                let expands_to = frontend["expands_to"].as_str().unwrap_or("");
                let _ = writeln!(summary, "      - {shorthand}: {expands_to}");
            }
        }
        if let Some(examples) = preset["examples"].as_array()
            && !examples.is_empty()
        {
            summary.push_str("    examples:\n");
            for example in examples.iter().filter_map(serde_json::Value::as_str) {
                let _ = writeln!(summary, "      {example}");
            }
        }
        if let Some(ownership) = preset["ownership"].as_str() {
            summary.push_str("    ownership:\n");
            let _ = writeln!(summary, "      - {ownership}");
        }
        if let Some(non_goals) = preset["non_goals"].as_array()
            && !non_goals.is_empty()
        {
            summary.push_str("    non-goals:\n");
            for non_goal in non_goals.iter().filter_map(serde_json::Value::as_str) {
                let _ = writeln!(summary, "      - {non_goal}");
            }
        }
    }
    summary
}

pub(super) fn format_init_human_summary(output: &bootstrap::InitReport) -> String {
    let mut summary = String::new();
    summary.push_str("init summary\n");
    push_summary_field(&mut summary, "target", Some(output.destination()));
    push_summary_field(&mut summary, "template", Some(output.template()));

    let report = output.render_report();
    let created = array_len(&report["files_created"]);
    let modified = array_len(&report["files_modified"]);
    let removed = array_len(&report["files_removed"]);
    let _ = writeln!(
        summary,
        "  managed files: {created} created, {modified} modified, {removed} removed"
    );

    if let Some(scaffold) = output.scaffold() {
        let preset = scaffold["preset"].as_str().unwrap_or("<unknown>");
        let db = scaffold["db"].as_str().unwrap_or("<unknown>");
        let _ = write!(summary, "  scaffold: {preset}");
        if let Some(repo_name) = scaffold["repo_name"].as_str() {
            let _ = write!(summary, " for {repo_name}");
        }
        let _ = writeln!(summary, " (db: {db})");
        let scaffold_created = array_len(&scaffold["files_created"]);
        let scaffold_modified = array_len(&scaffold["files_modified"]);
        let scaffold_unchanged = array_len(&scaffold["files_unchanged"]);
        let _ = writeln!(
            summary,
            "  scaffold files: {scaffold_created} created, {scaffold_modified} modified, {scaffold_unchanged} unchanged"
        );

        if let Some(frontends) = scaffold["frontends"].as_array()
            && !frontends.is_empty()
        {
            let names = frontends
                .iter()
                .filter_map(|frontend| frontend["name"].as_str())
                .collect::<Vec<_>>();
            if !names.is_empty() {
                let _ = writeln!(summary, "  frontends: {}", names.join(", "));
            }
        }
        if let Some(notices) = scaffold["frontend_notices"].as_array()
            && !notices.is_empty()
        {
            summary.push_str("  frontend notes:\n");
            for notice in notices.iter().filter_map(serde_json::Value::as_str) {
                let _ = writeln!(summary, "    - {notice}");
            }
        }
    }

    let _ = writeln!(
        summary,
        "  git: {}",
        if output.git_initialized() {
            "initialized"
        } else {
            "already present"
        }
    );

    if let Some(vault) = output.vault() {
        push_bootstrap_vault_summary(&mut summary, vault);
    }

    let notes = output.notes();
    if !notes.is_empty() {
        summary.push_str("  notes:\n");
        for note in notes.iter().take(5) {
            let _ = writeln!(summary, "    - {note}");
        }
        if notes.len() > 5 {
            let _ = writeln!(summary, "    - and {} more", notes.len() - 5);
        }
    }

    let steps = output.next_steps();
    if !steps.is_empty() {
        summary.push_str("  next steps:\n");
        for step in steps {
            let _ = writeln!(summary, "    - {step}");
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
    let _ = writeln!(
        summary,
        "  managed files: {created} created, {modified} modified, {removed} removed, {unchanged} unchanged"
    );

    if let Some(conflicts) = report["conflicts"].as_array()
        && !conflicts.is_empty()
    {
        push_conflict_summary(&mut summary, "conflicts accepted", conflicts);
    }

    if let Some(warnings) = output["warnings"].as_array()
        && !warnings.is_empty()
    {
        summary.push_str("  warnings:\n");
        for warning in warnings.iter().filter_map(serde_json::Value::as_str) {
            let _ = writeln!(summary, "    - {warning}");
        }
    }

    if let Some(steps) = output["next_steps"].as_array()
        && !steps.is_empty()
    {
        summary.push_str("  next steps:\n");
        for step in steps.iter().filter_map(serde_json::Value::as_str) {
            let _ = writeln!(summary, "    - {step}");
        }
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
    let _ = writeln!(
        summary,
        "  managed files: {created} created, {modified} modified, {removed} removed"
    );

    push_vault_summary(&mut summary, &output["vault"]);

    if let Some(review) = output["adoption_review"].as_array()
        && !review.is_empty()
    {
        summary.push_str("  review:\n");
        for item in review.iter().filter_map(serde_json::Value::as_str) {
            let _ = writeln!(summary, "    - {item}");
        }
    }

    if let Some(notes) = output["notes"].as_array()
        && !notes.is_empty()
    {
        summary.push_str("  notes:\n");
        for note in notes.iter().take(8).filter_map(serde_json::Value::as_str) {
            let _ = writeln!(summary, "    - {note}");
        }
        if notes.len() > 8 {
            let _ = writeln!(summary, "    - and {} more", notes.len() - 8);
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
        let _ = writeln!(summary, "  warnings: {}", warnings.len());
        for warning in warnings
            .iter()
            .take(5)
            .filter_map(serde_json::Value::as_str)
        {
            let _ = writeln!(summary, "    - {warning}");
        }
        if warnings.len() > 5 {
            let _ = writeln!(summary, "    - and {} more", warnings.len() - 5);
        }
    }

    if let Some(steps) = output["next_steps"].as_array()
        && !steps.is_empty()
    {
        summary.push_str("  next steps:\n");
        for step in steps.iter().filter_map(serde_json::Value::as_str) {
            let _ = writeln!(summary, "    - {step}");
        }
    }
    summary
}

fn push_conflict_summary(summary: &mut String, label: &str, conflicts: &[serde_json::Value]) {
    let _ = writeln!(summary, "  {label}: {}", conflicts.len());
    for conflict in conflicts.iter().take(10) {
        let Some(path) = conflict["path"].as_str() else {
            continue;
        };
        if let Some(detail) = conflict["detail"].as_str() {
            let _ = writeln!(summary, "    - {path}: {detail}");
        } else {
            let _ = writeln!(summary, "    - {path}");
        }
    }
    if conflicts.len() > 10 {
        let _ = writeln!(summary, "    - and {} more", conflicts.len() - 10);
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
        let _ = writeln!(summary, "  vault: skipped ({reason})");
        return;
    }
    let status = if vault["created"].as_bool().unwrap_or(false) {
        "created"
    } else if vault["initialized"].as_bool().unwrap_or(false) {
        "already initialized"
    } else {
        "not initialized"
    };
    let _ = write!(summary, "  vault: {status}");
    if let Some(scope) = vault["vault_scope"].as_str() {
        let _ = write!(summary, " ({scope})");
    }
    summary.push('\n');
}

fn push_bootstrap_vault_summary(summary: &mut String, vault: &bootstrap::BootstrapVaultReport) {
    if !vault.requested() {
        summary.push_str("  vault: skipped\n");
        return;
    }
    if let Some(reason) = vault.skipped_reason() {
        let _ = writeln!(summary, "  vault: skipped ({reason})");
        return;
    }
    let status = if vault.created() {
        "created"
    } else if vault.initialized_status() {
        "already initialized"
    } else {
        "not initialized"
    };
    let _ = write!(summary, "  vault: {status}");
    if let Some(scope) = vault.vault_scope() {
        let _ = write!(summary, " ({scope})");
    }
    summary.push('\n');
}

fn push_summary_field(summary: &mut String, label: &str, value: Option<&str>) {
    if let Some(value) = value {
        let _ = writeln!(summary, "  {label}: {value}");
    }
}

fn array_len(value: &serde_json::Value) -> usize {
    value.as_array().map(Vec::len).unwrap_or(0)
}

#[cfg(test)]
#[path = "bootstrap_run_tests.rs"]
mod tests;
