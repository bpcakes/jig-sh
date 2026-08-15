use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use super::answers::AnswerInput;
use super::git::init_git_repo_with_validation;
use super::init_transaction::InitMutationTransaction;
use super::initial_copy::{BootstrapCopyRequest, render_and_copy_bootstrap_template};
use super::initial_template::{prepare_initial_template_source, resolve_initial_template_request};
use super::path::bootstrap_invocation_cwd;
use super::template_source::PreparedTemplateSource;
use super::{
    ANSWERS_FILE, AnswerOpts, InitOpts, InitReport, InitialCommand,
    ensure_init_destination_noreplace_supported, initial_next_steps, initial_notes,
    initial_render_report, managed_paths, path, scaffold, template_progress_label,
    validate_init_destination,
};
use crate::progress::CliProgress;

struct PreparedInit {
    destination: PathBuf,
    answers: AnswerOpts,
    answer_input: AnswerInput,
    scaffold_plan: Option<scaffold::InitScaffoldPlan>,
    template: PreparedTemplateSource,
    force: bool,
    use_defaults: bool,
    progress: CliProgress,
}

pub(crate) fn run_init(opts: InitOpts) -> Result<InitReport> {
    execute_init(prepare_init(opts)?)
}

fn prepare_init(mut opts: InitOpts) -> Result<PreparedInit> {
    let invocation_cwd = bootstrap_invocation_cwd()?;
    let destination = path::resolve_init_destination(&opts.path, &invocation_cwd)?;
    // This first validation deliberately precedes answer loading and template
    // resolution so unsafe or non-empty destinations fail without interaction.
    validate_init_destination(&destination, opts.force)?;
    ensure_init_destination_noreplace_supported(&destination)?;
    let progress = CliProgress::new("init");
    progress.header_for_path("render harness into new repo", &destination);
    progress.step("validate destination", "empty directory or --force");
    progress.log_blocked_on_err(validate_init_destination(&destination, opts.force))?;
    progress.step("read init answers", "--answers-file and CLI precedence");
    let answer_input =
        progress.log_blocked_on_err(AnswerInput::from_opts_at(&opts.answers, &invocation_cwd))?;
    let mut answers = progress.log_blocked_on_err(answer_input.effective_opts(&opts.answers))?;
    opts.scaffold.normalize_minimal_harness_shape(&answers);
    progress.log_blocked_on_err(opts.scaffold.validate_init_invariants(&answers))?;
    opts.scaffold.apply_init_answer_defaults(&mut answers);
    let scaffold_plan = progress.log_blocked_on_err(scaffold::InitScaffoldPlan::from_opts(
        &opts.scaffold,
        &answers,
        &destination,
    ))?;
    if let Some(plan) = &scaffold_plan {
        plan.apply_answer_defaults(&mut answers);
    }
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
    Ok(PreparedInit {
        destination,
        answers,
        answer_input,
        scaffold_plan,
        template,
        force: opts.force,
        use_defaults: opts.defaults,
        progress,
    })
}

fn execute_init(prepared: PreparedInit) -> Result<InitReport> {
    let PreparedInit {
        destination,
        answers,
        answer_input,
        scaffold_plan,
        template,
        force,
        use_defaults,
        progress,
    } = prepared;
    let mut transaction = InitMutationTransaction::create(&destination)?;
    let work_destination = transaction.work_destination().to_path_buf();
    let init_result = (|| -> Result<InitReport> {
        // Revalidate after creation: another process may have populated a path
        // between the initial preflight and our atomic create_dir calls.
        progress.log_blocked_on_err(validate_init_destination(&destination, force))?;
        if let Some(plan) = &scaffold_plan {
            progress.step("preflight project scaffold", plan.summary());
            progress.log_blocked_on_err(plan.preflight(&work_destination, force))?;
            progress.log_blocked_on_err(path::validate_repository_regular_file_leaf(
                &work_destination,
                Path::new(managed_paths::AGENT_MAP_PATH),
            ))?;
        }

        let copy_result = render_and_copy_bootstrap_template(BootstrapCopyRequest {
            destination: &work_destination,
            template: &template,
            answers: &answers,
            answer_input: Some(answer_input),
            use_defaults,
            force,
            dry_run: false,
            backup_root: None,
            seed_repo_path: None,
            prior_harness_footprint: None,
            prior_managed_paths: None,
            reconcile_runtime_config: false,
            allow_answers_overwrite: false,
            allow_contract_overwrite: false,
            reserved_output_paths: scaffold_plan
                .as_ref()
                .map(scaffold::InitScaffoldPlan::output_paths)
                .unwrap_or_default(),
            init_transaction: Some(&mut transaction),
            progress,
        })?;
        let scaffold_report = if let Some(plan) = &scaffold_plan {
            progress.step("scaffold project", plan.summary());
            if let Some(note) = plan.sanitized_repo_name_note() {
                progress.info("scaffold note", note);
            }
            let files = progress.log_blocked_on_err(plan.render_files())?;
            progress.log_blocked_on_err(transaction.plan_scaffold_files(&files))?;
            let report = progress.log_blocked_on_err(plan.write_rendered_with_transaction(
                &work_destination,
                files,
                force,
                Some(&mut transaction),
            ))?;
            progress.step("refresh agent map", "include scaffold crate guides");
            let agent_map_path = Path::new(managed_paths::AGENT_MAP_PATH);
            let agent_map = progress.log_blocked_on_err(crate::policy::render_agent_map(
                &work_destination,
                agent_map_path,
            ))?;
            progress.log_blocked_on_err(
                transaction.plan_regular_file_bytes(agent_map_path, &agent_map),
            )?;
            progress.log_blocked_on_err(transaction.prepare_file_publication(agent_map_path))?;
            let agent_map_commit = if transaction.is_privately_staged() {
                let expected_leaf = progress.log_blocked_on_err(
                    path::validate_repository_regular_file_leaf(&work_destination, agent_map_path),
                )?;
                progress.log_blocked_on_err(path::write_repository_file_atomic_staged(
                    &work_destination,
                    agent_map_path,
                    &agent_map,
                    expected_leaf,
                    || transaction.verify_destination_identity(),
                ))?
            } else {
                let desired_permissions = transaction.publication_permissions(agent_map_path)?;
                let temporary_directory = transaction
                    .write_staging_path(agent_map_path)
                    .context("Existing-destination init write staging is unavailable")?
                    .to_path_buf();
                progress.log_blocked_on_err(path::write_repository_file_atomic_guarded(
                    &work_destination,
                    agent_map_path,
                    &agent_map,
                    desired_permissions,
                    &temporary_directory,
                    || transaction.verify_destination_identity(),
                ))?
            };
            progress.log_blocked_on_err(
                transaction.record_regular_commit(agent_map_path, agent_map_commit),
            )?;
            Some(report)
        } else {
            None
        };
        let default_branch = copy_result
            .default_branch
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Missing default_branch in staged {ANSWERS_FILE}"))?;
        progress.step("initialize git", format!("default branch {default_branch}"));
        let git_initialized =
            init_git_repo_with_validation(&work_destination, default_branch, || {
                transaction.verify_destination_identity()
            })?;

        Ok(InitReport {
            ok: true,
            command: "init".to_string(),
            render_mode: "copy".to_string(),
            template: template.source().to_string(),
            destination: destination.display().to_string(),
            answers_file: ANSWERS_FILE.to_string(),
            git_initialized,
            scaffold: scaffold_report,
            render_report: initial_render_report(&copy_result),
            next_steps: initial_next_steps(
                InitialCommand::Init,
                &destination,
                &copy_result,
                scaffold_plan
                    .as_ref()
                    .is_some_and(scaffold::InitScaffoldPlan::database_enabled),
            ),
            notes: initial_notes(
                copy_result.notes,
                copy_result.frontend_apps_configured,
                scaffold_plan.as_ref(),
                false,
            ),
            vault: None,
        })
    })();

    match init_result {
        Ok(report) => {
            transaction.commit()?;
            progress.done("init complete");
            Ok(report)
        }
        Err(primary) => Err(transaction.finish_failed_init(primary)),
    }
}
