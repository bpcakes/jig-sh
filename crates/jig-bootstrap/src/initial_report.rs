//! The report and next steps that `jig init` and `jig adopt` print.

use std::path::Path;

use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[derive(Debug, Deserialize, Serialize)]
pub struct InitReport {
    pub(super) ok: bool,
    pub(super) command: String,
    pub(super) render_mode: String,
    pub(super) template: String,
    pub(super) destination: String,
    pub(super) answers_file: String,
    pub(super) git_initialized: bool,
    pub(super) scaffold: Option<Value>,
    pub(super) render_report: Value,
    pub(super) next_steps: Vec<String>,
    pub(super) notes: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) vault: Option<BootstrapVaultReport>,
    // Keep legacy JSON-style bootstrap assertions working without carrying a
    // second representation in production reports.
    #[cfg(test)]
    #[serde(skip)]
    pub(super) serialized: std::sync::OnceLock<Value>,
}

impl InitReport {
    pub fn destination(&self) -> &str {
        &self.destination
    }

    pub fn template(&self) -> &str {
        &self.template
    }

    pub const fn git_initialized(&self) -> bool {
        self.git_initialized
    }

    pub fn scaffold(&self) -> Option<&Value> {
        self.scaffold.as_ref()
    }

    pub const fn render_report(&self) -> &Value {
        &self.render_report
    }

    pub fn next_steps(&self) -> &[String] {
        &self.next_steps
    }

    pub fn notes(&self) -> &[String] {
        &self.notes
    }

    pub fn vault(&self) -> Option<&BootstrapVaultReport> {
        self.vault.as_ref()
    }

    pub fn attach_vault(&mut self, vault: BootstrapVaultReport) -> Result<()> {
        if self.vault.is_some() {
            bail!("bootstrap::run_init output unexpectedly included a vault field");
        }
        self.vault = Some(vault);
        #[cfg(test)]
        {
            self.serialized = std::sync::OnceLock::new();
        }
        Ok(())
    }
}

#[cfg(test)]
impl std::ops::Deref for InitReport {
    type Target = Value;

    fn deref(&self) -> &Self::Target {
        self.serialized.get_or_init(|| {
            serde_json::to_value(self).expect("typed init report should serialize for legacy tests")
        })
    }
}

#[derive(Debug, Deserialize, Serialize)]
pub struct BootstrapVaultReport {
    requested: bool,
    initialized: bool,
    created: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    skipped_reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    vault_home: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    vault_scope: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    vault_scope_id: Option<Value>,
}

impl BootstrapVaultReport {
    pub fn disabled() -> Self {
        Self::skipped(false, "disabled")
    }

    pub fn missing_scope() -> Self {
        Self::skipped(true, "repo has no [vault] scope")
    }

    fn skipped(requested: bool, reason: &str) -> Self {
        Self {
            requested,
            initialized: false,
            created: false,
            skipped_reason: Some(reason.to_string()),
            vault_home: None,
            vault_scope: None,
            vault_scope_id: None,
        }
    }

    pub fn initialized(created: bool, runtime_report: &Value) -> Self {
        Self {
            requested: true,
            initialized: true,
            created,
            skipped_reason: None,
            vault_home: Some(runtime_report["vault_home"].clone()),
            vault_scope: Some(runtime_report["vault_scope"].clone()),
            vault_scope_id: Some(runtime_report["vault_scope_id"].clone()),
        }
    }

    pub const fn requested(&self) -> bool {
        self.requested
    }

    pub const fn initialized_status(&self) -> bool {
        self.initialized
    }

    pub const fn created(&self) -> bool {
        self.created
    }

    pub fn skipped_reason(&self) -> Option<&str> {
        self.skipped_reason.as_deref()
    }

    pub fn vault_scope(&self) -> Option<&str> {
        self.vault_scope.as_ref().and_then(Value::as_str)
    }
}

pub(super) fn template_progress_label(template: Option<&str>) -> String {
    template.unwrap_or("default jig-sh template").to_string()
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum InitialCommand {
    Init,
    Adopt,
}

pub(super) fn initial_next_steps(
    command: InitialCommand,
    destination: &Path,
    result: &super::initial_copy::BootstrapCopyResult,
    database_config_required: bool,
) -> Vec<String> {
    let destination_for_cd = destination
        .canonicalize()
        .unwrap_or_else(|_| destination.to_path_buf());
    let mut steps = vec![format!(
        "cd {}",
        jig_repository::shell::quote(&destination_for_cd.display().to_string())
    )];
    if command == InitialCommand::Adopt && result.apply_report.dry_run {
        steps.push("Review the adoption preview and managed-file diff.".into());
        if result.minimal_footprint {
            if result.full_to_minimal_transition {
                steps.push("jig adopt . --minimal --write --force".into());
            } else {
                steps.push(
                    "Re-run jig adopt . --minimal --write after reviewing the summary.".into(),
                );
            }
        } else {
            steps.push("Re-run jig adopt . --write after reviewing the summary.".into());
        }
        steps.push("No files were changed by this preview.".into());
        return steps;
    }
    if result.minimal_footprint {
        steps.push(
            "Add [[loop.workflows]] entries to .jig.toml, then run jig loop tick / jig loop run."
                .into(),
        );
        steps.push(
            "Re-run jig adopt . --write (without --minimal) when you want the full harness.".into(),
        );
        if command == InitialCommand::Adopt {
            steps.push("Commit the adoption diff after reviewing .jig.toml and .agent/.".into());
        }
        return steps;
    }
    steps.push("scripts/jig setup".into());
    if database_config_required {
        steps.push(
            "Export DATABASE_URL, or copy the backend .env.example to .env and configure it before database setup."
                .into(),
        );
        steps.push("bash scripts/setup-database.sh".into());
    }
    steps.push("scripts/jig info targets".into());
    if result.dev_apps_configured {
        steps.push("scripts/jig dev".into());
    }
    if result.sqlx_enabled {
        steps.push(
            "For SQL query validation, use scripts/jig check sqlx after database access is configured; doctor flags missing cargo-sqlx or a build that lacks the configured database driver."
                .into(),
        );
    }
    if result.schema_dump_enabled {
        steps.push("Provide scripts/dump-schema.sh, then run scripts/jig sqlx schema dump.".into());
    }
    if command == InitialCommand::Adopt {
        steps.push(
            "Commit the adoption diff after reviewing it and validating the affected behavior."
                .into(),
        );
    }
    steps
}

pub(super) fn initial_notes(
    extra_notes: Vec<String>,
    frontend_apps_configured: bool,
    scaffold_plan: Option<&super::scaffold::InitScaffoldPlan>,
    minimal_footprint: bool,
    file_budget_audit_available: bool,
) -> Vec<String> {
    let mut notes = if minimal_footprint {
        vec![
            "The minimal footprint allows .jig.toml, .agent/ scaffolding, and root .gitignore and .gitattributes when supplied by the template; it omits scripts/, workflows, AGENTS.md, agent-map.md.".into(),
            "harness_footprint = \"minimal\" is stored in .jig.toml so jig update keeps the same footprint until you re-adopt without --minimal.".into(),
            "Invoke the installed jig binary directly for loop commands; there is no scripts/jig launcher yet.".into(),
        ]
    } else {
        vec![
            "The first scripts/jig command may install or compile a compatible Jig runtime into this repo's contract/profile cache.".into(),
            "To pin a published runtime independently of template updates, commit .jig/runtime-version containing an exact stable release such as 0.5.0; generated CI caches that executable.".into(),
            "Review generated .jig.toml, AGENTS.md, agent-map.md, and check commands before relying on the harness.".into(),
            "Re-run scripts/jig doctor after setup changes to confirm readiness.".into(),
            "Choose checks for the affected behavior with scripts/jig check COMPONENT:ACTION.".into(),
        ]
    };
    if file_budget_audit_available && !minimal_footprint {
        notes.push(
            "Use scripts/jig file-budget audit for standalone source-size diagnostics without creating runs.".into(),
        );
    }
    if scaffold_plan.is_some() {
        notes.insert(
            0,
            "Scaffolded project code is project-owned after creation. jig update keeps the Jig harness current and does not rewrite project code."
                .into(),
        );
    }
    if frontend_apps_configured && !minimal_footprint {
        notes.push(
            "Frontend checks expect package scripts for lint, typecheck, build:bundle, and test:coverage plus a package-manager lockfile; generated preset apps include them."
                .into(),
        );
        notes.push(
            "Frontend checks are available as scripts/jig check typescript-lint, typescript-typecheck, typescript-build, and typescript-coverage; select those relevant to the change."
                .into(),
        );
    }
    if !minimal_footprint {
        notes.push(
            "Use scripts/jig check contract for harness wiring changes and scripts/jig check agent-guides for ownership guidance changes."
                .into(),
        );
    }
    if let Some(note) =
        scaffold_plan.and_then(super::scaffold::InitScaffoldPlan::sanitized_repo_name_note)
    {
        notes.push(note);
    }
    notes.extend(extra_notes);
    notes
}

pub(super) fn initial_render_report(result: &super::initial_copy::BootstrapCopyResult) -> Value {
    json!({
        "dry_run": result.apply_report.dry_run,
        "active_managed_paths": &result.apply_report.active_managed_paths,
        "retired_managed_paths": &result.apply_report.retired_managed_paths,
        "files_created": &result.apply_report.files_created,
        "files_modified": &result.apply_report.files_modified,
        "files_removed": &result.apply_report.files_removed,
        "files_unchanged": &result.apply_report.files_unchanged,
        "managed_blocks_inserted": &result.apply_report.managed_blocks_inserted,
        "managed_blocks_rendered": &result.apply_report.managed_blocks_rendered,
        "backups": &result.apply_report.backups,
        "conflicts": &result.apply_report.conflicts,
        "commands_detected_or_skipped": initial_command_report(result),
        "todos": initial_todos(result),
        "suggested_jig_toml_edits": initial_suggested_jig_toml_edits(result),
    })
}

pub(super) fn initial_command_report(
    result: &super::initial_copy::BootstrapCopyResult,
) -> Vec<String> {
    let launcher = super::gate_preview::jig_launcher(result.minimal_footprint);
    let mut commands = Vec::new();
    if result.bootstrap_command_configured {
        commands.push(format!(
            "bootstrap_command configured; run {launcher} bootstrap before checks"
        ));
    } else {
        commands.push(format!(
            "bootstrap_command not configured; skip {launcher} bootstrap"
        ));
    }
    commands.push(format!(
        "contract check available through {launcher} check contract"
    ));
    if result.dev_apps_configured {
        commands.push(format!("[[dev.apps]] configured; run {launcher} dev"));
    } else {
        commands.push(format!(
            "no [[dev.apps]] configured; {launcher} dev has no app to launch"
        ));
    }
    if result.frontend_apps_configured && !result.minimal_footprint {
        commands.push(format!(
            "frontend app checks available through {launcher} check typescript-*"
        ));
    }
    commands
}

fn initial_todos(result: &super::initial_copy::BootstrapCopyResult) -> Vec<String> {
    let mut todos = vec![
        "Review generated command strings in .jig.toml against this repo's actual setup.".into(),
        "Add or update crate-level AGENTS.md files for repo-owned business rules.".into(),
    ];
    if result.sqlx_enabled {
        todos.push("Confirm SQLx database access and committed metadata workflow.".into());
    }
    if result.schema_dump_enabled {
        todos.push("Provide the project-owned scripts/dump-schema.sh implementation.".into());
    }
    if result.frontend_apps_configured && !result.minimal_footprint {
        todos.push(
            "Confirm each frontend app has package scripts and starts on the injected PORT/HOST."
                .into(),
        );
    }
    todos
}

fn initial_suggested_jig_toml_edits(
    result: &super::initial_copy::BootstrapCopyResult,
) -> Vec<String> {
    let mut edits = vec![
        "Replace generated fallback Cargo commands if this repo uses nested workspaces or non-Cargo checks.".into(),
    ];
    if result.dev_apps_configured {
        edits.push("Tune [dev] ports, tld, HTTPS, LAN, and each [[dev.apps]] kind/argv if defaults do not match local development.".into());
    }
    if result.sqlx_enabled {
        edits.push("Set rust_migration_dir, rust_sqlx_metadata_dir, and sqlx_check_command to the repo-owned SQLx layout.".into());
    }
    edits
}
