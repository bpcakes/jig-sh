//! `jig adopt`: render the harness into an existing repository and record the receipt.

use std::io::{IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::{fs, io};

use anyhow::{Context, Result, bail};
use jig_execution::progress::CliProgress;
use jig_repository::path::{
    absolute_path_from, bootstrap_invocation_cwd, validate_repository_relative_ancestors,
};
use serde_json::{Value, json};
use tempfile::Builder as TempFileBuilder;
use time::OffsetDateTime;
use ulid::Ulid;

use super::ANSWERS_FILE;
use super::answers::{AnswerInput, HarnessFootprint, RenderAnswers};
use super::destination::{reject_newer_declared_contract, validate_adopt_destination};
use super::initial_copy::{BootstrapCopyRequest, render_and_copy_bootstrap_template};
use super::initial_report::{
    InitialCommand, initial_next_steps, initial_notes, initial_render_report,
    template_progress_label,
};
use super::initial_template::{prepare_initial_template_source, resolve_initial_template_request};
use super::launcher_repair_cache::{FullRefreshRuntimePolicy, finish_full_refresh};
use super::opts::AdoptOpts;

pub(super) const ADOPT_RECEIPT_PATH: &str = ".agent/.cache/adopt/adopt-last.json";

pub(super) const LEGACY_ADOPT_RECEIPT_PATH: &str = ".agent/state/adopt-last.json";

pub(super) const ADOPT_RECEIPT_PATHS: [&str; 2] = [ADOPT_RECEIPT_PATH, LEGACY_ADOPT_RECEIPT_PATH];

pub fn run_adopt(opts: AdoptOpts) -> Result<Value> {
    let invocation_cwd = bootstrap_invocation_cwd()?;
    let destination = absolute_path_from(&opts.path, &invocation_cwd)?;
    let progress = CliProgress::new("adopt");
    progress.header_for_path("render harness into existing repo", &destination);
    progress.step("validate destination", "existing repository directory");
    progress.log_blocked_on_err(validate_adopt_destination(&destination))?;
    progress.log_blocked_on_err(reject_newer_declared_contract(&destination))?;
    let prior_managed_paths =
        progress.log_blocked_on_err(super::managed_paths::load_manifest(&destination))?;
    progress.step(
        "resolve template",
        template_progress_label(opts.template.as_deref()),
    );
    let template_request = progress.log_blocked_on_err(resolve_initial_template_request(
        opts.template.as_deref(),
        &opts.vcs_ref,
    ))?;
    let template = progress.log_blocked_on_err(prepare_initial_template_source(
        &template_request,
        opts.template_mode,
        &invocation_cwd,
    ))?;
    progress.step("infer answers", "scan existing repository");
    let mut inference = super::adopt_infer::infer_adopt_answers(&destination);
    let prior_answers = recognized_prior_answers(&destination);
    let requested_harness_footprint = if opts.minimal {
        HarnessFootprint::Minimal
    } else {
        HarnessFootprint::Full
    };
    let expands_minimal_harness = prior_answers.as_ref().is_some_and(|prior| {
        prior.harness_footprint() == HarnessFootprint::Minimal
            && requested_harness_footprint == HarnessFootprint::Full
    });
    let establishes_manifest = prior_managed_paths.is_none() && prior_answers.is_some();
    if prior_managed_paths.is_none()
        && prior_answers.as_ref().is_some_and(|prior| {
            prior.harness_footprint() == HarnessFootprint::Full
                && requested_harness_footprint == HarnessFootprint::Minimal
        })
    {
        bail!(
            "Cannot switch this adopted repository from the full harness to --minimal because {} is missing. First run `jig adopt . --write` without --minimal to establish exact managed-path ownership, then retry the minimal adoption.",
            super::managed_paths::MANIFEST_PATH
        );
    }
    let mut answers = opts.answers.clone();
    answers.harness_footprint = Some(requested_harness_footprint);
    let answer_input = progress.log_blocked_on_err(
        if prior_answers.is_some() && answers.answers_file.is_none() {
            AnswerInput::from_file(&destination.join(ANSWERS_FILE))
        } else {
            AnswerInput::from_opts_at(&answers, &invocation_cwd)
        },
    )?;
    let mut answer_input = answer_input;
    answer_input.prepare_adoption(&mut inference, &destination, &answers, &opts.components)?;
    let answer_shape = answer_input.shape().clone();
    progress.info("detected", inference.summary());
    progress.info("detected stack", inference.detected_stack_label());
    if opts.minimal {
        progress.info(
            "footprint",
            "minimal (.jig.toml + .agent/ scaffolding; no scripts/workflows/context files)",
        );
    }
    for warning in inference.warnings() {
        progress.info("warning", warning);
    }
    inference.apply_to_answers(&mut answers, &answer_shape);
    inference.apply_component_decisions(&mut answers);
    let effective_sqlx = answer_input.effective_sqlx(&answers, opts.defaults);
    let review = inference.adoption_review(&answers, &opts.answers, &answer_shape, &effective_sqlx);
    for item in &review.items {
        progress.info("review", item);
    }
    progress.log_blocked_on_err(inference.require_sqlx_migration_answer(&effective_sqlx))?;
    let mut runtime_warnings = Vec::new();
    if opts.write {
        confirm_adopt_write(&opts)?;
    } else {
        progress.info(
            "mode",
            "preview only; re-run with --write to apply managed files",
        );
    }
    let backup_root = opts.write.then(|| adopt_backup_root(&destination));
    if opts.write {
        progress.log_blocked_on_err(validate_adopt_output_ancestors(
            &destination,
            backup_root.as_deref(),
        ))?;
    }

    let copy_result = render_and_copy_bootstrap_template(BootstrapCopyRequest {
        destination: &destination,
        template: &template,
        answers: &answers,
        answer_input: Some(answer_input),
        use_defaults: opts.defaults,
        force: opts.force,
        dry_run: !opts.write,
        backup_root: backup_root.clone(),
        seed_repo_path: Some(&destination),
        prior_harness_footprint: prior_answers.as_ref().map(RenderAnswers::harness_footprint),
        prior_managed_paths: prior_managed_paths.as_ref(),
        reconcile_runtime_config: prior_answers.is_some(),
        allow_answers_overwrite: expands_minimal_harness || establishes_manifest,
        allow_contract_overwrite: expands_minimal_harness,
        reserved_output_paths: Vec::new(),
        scaffolded_frontend_contracts: false,
        scaffolded_go_postgres_integration: false,
        init_transaction: None,
        use_update_transaction: opts.write,
        progress,
    })?;
    if opts.write {
        if let Err(error) =
            write_adopt_last_receipt(&destination, backup_root.as_deref(), &copy_result)
        {
            progress.info(
                "warning",
                format!("adopt write completed but undo receipt could not be recorded: {error:#}"),
            );
        }
        let footprint = if copy_result.minimal_footprint {
            HarnessFootprint::Minimal
        } else {
            HarnessFootprint::Full
        };
        let runtime_policy = FullRefreshRuntimePolicy::for_render(footprint, template.source());
        runtime_warnings =
            finish_full_refresh(&destination, runtime_policy, progress, "adopt complete");
    } else {
        progress.done("adopt preview complete");
    }

    Ok(json!({
        "ok": true,
        "command": "adopt",
        "render_mode": if opts.write { "copy" } else { "preview" },
        "harness_footprint": if copy_result.minimal_footprint {
            "minimal"
        } else {
            "full"
        },
        "template": template.source(),
        "destination": destination.display().to_string(),
        "answers_file": ANSWERS_FILE,
        "git_initialized": false,
        "write": opts.write,
        "warnings": runtime_warnings,
        "detection_report": inference.report(),
        "adoption_profile": inference.adoption_profile_report(
            &copy_result.render_preview.generated_gates,
            &copy_result.render_preview.managed_files,
            &copy_result.render_preview.retired_managed_files,
            &copy_result.render_preview.file_budget,
            &opts.answers,
            &answer_shape,
        ),
        "adoption_review": review.items,
        "render_report": initial_render_report(&copy_result),
        "next_steps": initial_next_steps(
            InitialCommand::Adopt,
            &destination,
            &copy_result,
            false,
        ),
        "notes": initial_notes(
            copy_result.notes,
            copy_result.frontend_apps_configured,
            None,
            copy_result.minimal_footprint,
            copy_result.file_budget_audit_available,
        ),
    }))
}

fn recognized_prior_answers(destination: &Path) -> Option<RenderAnswers> {
    let answers = RenderAnswers::from_answers_file(&destination.join(ANSWERS_FILE)).ok()?;
    jig_context::RepoContext::validate_config_file(destination).ok()?;
    Some(answers)
}

fn adopt_backup_root(destination: &Path) -> PathBuf {
    destination
        .join(".agent/.cache/adopt/backups")
        .join(Ulid::new().to_string())
}

fn validate_adopt_output_ancestors(destination: &Path, backup_root: Option<&Path>) -> Result<()> {
    validate_adopt_receipt_paths(destination)?;
    if let Some(backup_root) = backup_root {
        let backup_relative = backup_root.strip_prefix(destination).with_context(|| {
            format!(
                "Backup destination {} must be contained by repository root {}",
                backup_root.display(),
                destination.display()
            )
        })?;
        validate_repository_relative_ancestors(destination, &backup_relative.join("preflight"))?;
    }
    Ok(())
}

fn validate_adopt_receipt_paths(destination: &Path) -> Result<()> {
    for relative in ADOPT_RECEIPT_PATHS.map(Path::new) {
        validate_repository_relative_ancestors(destination, relative)?;
        let receipt_path = destination.join(relative);
        match fs::symlink_metadata(&receipt_path) {
            Ok(metadata) if metadata.file_type().is_file() => {}
            Ok(_) => {
                bail!(
                    "Adopt receipt path must be missing or a regular file, not a symlink, directory, or other file type: {}",
                    receipt_path.display()
                );
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(error)
                    .with_context(|| format!("Failed to stat {}", receipt_path.display()));
            }
        }
    }
    Ok(())
}

fn confirm_adopt_write(opts: &AdoptOpts) -> Result<()> {
    if opts.defaults || opts.no_input {
        return Ok(());
    }
    let stdin = io::stdin();
    let mut stderr = io::stderr();
    if !stdin.is_terminal() || !stderr.is_terminal() {
        bail!(
            "Adopt write needs confirmation but stdin or stderr is not a terminal. Re-run interactively, or pass --defaults or --no-input for noninteractive execution."
        );
    }

    write!(stderr, "Proceed with adopt --write? [y/N] ")
        .context("Failed to write adopt confirmation prompt")?;
    stderr
        .flush()
        .context("Failed to flush adopt confirmation prompt")?;
    let mut answer = String::new();
    stdin
        .read_line(&mut answer)
        .context("Failed to read adopt confirmation")?;
    if matches!(answer.trim(), "y" | "Y" | "yes" | "YES" | "Yes") {
        return Ok(());
    }
    bail!("Adopt write cancelled; re-run with --defaults or --no-input to skip confirmation.");
}

fn write_adopt_last_receipt(
    destination: &Path,
    backup_root: Option<&Path>,
    result: &super::initial_copy::BootstrapCopyResult,
) -> Result<()> {
    validate_adopt_output_ancestors(destination, backup_root)?;
    let receipt = json!({
        "command": "adopt",
        "created_at_unix": OffsetDateTime::now_utc().unix_timestamp(),
        "destination": destination.display().to_string(),
        "backup_root": backup_root.map(|path| path.display().to_string()),
        "canonical_receipt_path": ADOPT_RECEIPT_PATH,
        "legacy_receipt_path": LEGACY_ADOPT_RECEIPT_PATH,
        "legacy_receipt_deprecated": true,
        "apply_report": &result.apply_report,
        "undo_hint": "Use apply_report.backups to restore modified or removed files, then delete paths listed in apply_report.files_created if you want to undo this adopt write. Delete backup_root when those backups are no longer needed.",
    });
    let text =
        serde_json::to_string_pretty(&receipt).context("Failed to serialize adopt receipt")?;
    let bytes = format!("{text}\n");
    write_adopt_receipt_atomic(destination, Path::new(ADOPT_RECEIPT_PATH), bytes.as_bytes())?;
    // TODO(jig-0.4): remove the legacy receipt copy after adopted repos have
    // had a release window to migrate readers to the canonical cache path.
    write_adopt_receipt_atomic(
        destination,
        Path::new(LEGACY_ADOPT_RECEIPT_PATH),
        bytes.as_bytes(),
    )?;
    Ok(())
}

fn write_adopt_receipt_atomic(destination: &Path, relative: &Path, bytes: &[u8]) -> Result<()> {
    validate_adopt_receipt_paths(destination)?;
    let receipt_path = destination.join(relative);
    let parent = receipt_path.parent().with_context(|| {
        format!(
            "Adopt receipt path has no parent: {}",
            receipt_path.display()
        )
    })?;
    fs::create_dir_all(parent).with_context(|| format!("Failed to create {}", parent.display()))?;
    validate_adopt_receipt_paths(destination)?;

    let existing_permissions = match fs::symlink_metadata(&receipt_path) {
        Ok(metadata) => Some(metadata.permissions()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => None,
        Err(error) => {
            return Err(error)
                .with_context(|| format!("Failed to stat {}", receipt_path.display()));
        }
    };
    #[cfg(unix)]
    let temp_builder = {
        use std::os::unix::fs::PermissionsExt;

        let mut builder = TempFileBuilder::new();
        if existing_permissions.is_none() {
            builder.permissions(fs::Permissions::from_mode(0o666));
        }
        builder
    };
    #[cfg(not(unix))]
    let temp_builder = TempFileBuilder::new();
    let mut temp = temp_builder.tempfile_in(parent).with_context(|| {
        format!(
            "Failed to create temporary adopt receipt in {}",
            parent.display()
        )
    })?;
    if let Some(permissions) = existing_permissions {
        temp.as_file()
            .set_permissions(permissions)
            .with_context(|| {
                format!(
                    "Failed to preserve permissions for {}",
                    receipt_path.display()
                )
            })?;
    }
    temp.write_all(bytes).with_context(|| {
        format!(
            "Failed to write temporary adopt receipt for {}",
            receipt_path.display()
        )
    })?;
    temp.as_file().sync_all().with_context(|| {
        format!(
            "Failed to sync temporary adopt receipt for {}",
            receipt_path.display()
        )
    })?;

    validate_adopt_receipt_paths(destination)?;
    temp.persist(&receipt_path)
        .map(|_| ())
        .map_err(|error| error.error)
        .with_context(|| format!("Failed to write {}", receipt_path.display()))
}
