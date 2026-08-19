use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use serde_json::{Value, json};

use super::launcher_repair_cache::{FullRefreshRuntimePolicy, finish_full_refresh};
use super::{
    ANSWERS_FILE, ApplyRenderConflictPolicy, ApplyRenderOptions, CliProgress,
    EMBEDDED_TEMPLATE_SOURCE, HarnessFootprint, InitMutationTransaction,
    LAUNCHER_ONLY_MANAGED_PATHS, RenderAnswers, RenderStageRequest, RepoContext, UpdateOpts,
    absolute_path_from, apply_staged_render, bootstrap_invocation_cwd, managed_paths,
    prepare_template_source_from_base, prepare_update_template_source, read_stored_template_state,
    reject_newer_declared_contract, seed_launcher_repair_runtime, stage_render,
    stage_selected_render, validate_update_destination,
};

struct PreparedUpdate {
    invocation_cwd: PathBuf,
    destination: PathBuf,
    progress: CliProgress,
    mode: &'static str,
    prior_managed_paths: BTreeSet<PathBuf>,
    legacy_manifest_missing: bool,
    answers_path: PathBuf,
}

pub fn run_update(opts: UpdateOpts) -> Result<Value> {
    let prepared = prepare_update(&opts)?;
    if opts.launcher_only {
        run_launcher_only_update(prepared)
    } else {
        run_full_update(&opts, prepared)
    }
}

fn prepare_update(opts: &UpdateOpts) -> Result<PreparedUpdate> {
    if opts.launcher_only && !opts.force {
        bail!("--launcher-only requires --force");
    }
    let invocation_cwd = bootstrap_invocation_cwd()?;
    let destination = absolute_path_from(&opts.path, &invocation_cwd)?;
    let progress = CliProgress::new("update");
    let mode = if opts.launcher_only {
        "launcher-only"
    } else if opts.recopy {
        "recopy"
    } else {
        "update"
    };
    progress.header_for_path(format!("refresh harness ({mode})"), &destination);
    progress.step("validate destination", "adopted repository directory");
    progress.log_blocked_on_err(validate_update_destination(&destination))?;
    progress.log_blocked_on_err(reject_newer_declared_contract(&destination))?;
    let (prior_managed_paths, legacy_manifest_missing) = match progress
        .log_blocked_on_err(managed_paths::load_manifest(&destination))?
    {
        Some(paths) => (paths, false),
        None if opts.launcher_only => {
            progress.info(
                "ownership",
                "managed-path manifest is missing; validating legacy generated launcher signatures",
            );
            (
                progress.log_blocked_on_err(legacy_launcher_only_paths(&destination))?,
                true,
            )
        }
        None => {
            bail!(
                "Cannot update this repository because {} is missing. Run `jig adopt . --write` with the current harness footprint to establish exact managed-path ownership, then retry `jig update`.",
                managed_paths::MANIFEST_PATH
            );
        }
    };
    let answers_path = destination.join(ANSWERS_FILE);
    Ok(PreparedUpdate {
        invocation_cwd,
        destination,
        progress,
        mode,
        prior_managed_paths,
        legacy_manifest_missing,
        answers_path,
    })
}

fn run_launcher_only_update(prepared: PreparedUpdate) -> Result<Value> {
    let PreparedUpdate {
        invocation_cwd,
        destination,
        progress,
        mode,
        prior_managed_paths,
        legacy_manifest_missing,
        answers_path,
    } = prepared;
    let destination_contract_version = progress.log_blocked_on_err(
        RepoContext::supported_contract_version_from_root(&destination),
    )?;
    progress.step("read answers", answers_path.display());
    let stored_template = progress.log_blocked_on_err(read_stored_template_state(&answers_path))?;
    let answers = progress.log_blocked_on_err(RenderAnswers::from_answers_file(&answers_path))?;
    if answers.harness_footprint() == HarnessFootprint::Minimal {
        bail!(
            "Cannot run launcher-only repair because .jig.toml declares harness_footprint = \"minimal\"; minimal harnesses do not manage scripts/jig or scripts/install-jig.sh. Restore the repository's full-footprint answers before repairing those generated scripts, or invoke the external Jig binary directly."
        );
    }

    let launcher_paths = LAUNCHER_ONLY_MANAGED_PATHS
        .map(PathBuf::from)
        .into_iter()
        .collect::<BTreeSet<_>>();
    let unowned_paths = launcher_paths
        .difference(&prior_managed_paths)
        .map(|path| path.display().to_string())
        .collect::<Vec<_>>();
    if !unowned_paths.is_empty() {
        bail!(
            "Cannot run launcher-only repair because {} does not own these required managed paths: {}",
            managed_paths::MANIFEST_PATH,
            unowned_paths.join(", ")
        );
    }

    if !stored_template.has_source_path() {
        bail!(
            "Cannot run launcher-only repair because .jig.toml does not define a non-empty _src_path. Restore the repository's recorded Jig template source before repairing the generated launcher scripts."
        );
    }
    let configured_source = if answers.template_source_url().is_empty()
        || answers.template_source_url() == stored_template.source_path()
    {
        format!("_src_path {:?}", stored_template.source_path())
    } else {
        format!(
            "_src_path {:?} and template_source_url {:?}",
            stored_template.source_path(),
            answers.template_source_url()
        )
    };
    let warnings = if stored_template.source_path() != EMBEDDED_TEMPLATE_SOURCE
        || (!answers.template_source_url().is_empty()
            && answers.template_source_url() != EMBEDDED_TEMPLATE_SOURCE)
    {
        let warning = format!(
            "Launcher-only repair renders scripts/jig and scripts/install-jig.sh from this Jig binary's embedded templates, while .jig.toml records {configured_source}; source-specific launcher customizations will be replaced until the next full update."
        );
        progress.info("warning", &warning);
        vec![warning]
    } else {
        Vec::new()
    };
    progress.step("resolve template", "embedded launcher templates");
    let update_template = progress.log_blocked_on_err(prepare_template_source_from_base(
        EMBEDDED_TEMPLATE_SOURCE,
        None,
        None,
        &invocation_cwd,
    ))?;
    let contract_version = destination_contract_version;
    let staged = stage_selected_render(
        &update_template,
        &answers,
        &launcher_paths,
        contract_version,
        progress,
    )?;

    let mut transaction = InitMutationTransaction::create(&destination)?;
    transaction.plan_staged_render(&staged, &[])?;
    let repair_result = (|| -> Result<_> {
        let render_report = apply_staged_render(
            &staged,
            &destination,
            ApplyRenderOptions {
                // Launcher-only repair is an explicit forced replacement:
                // Clap and run_update both reject this mode without
                // --force, and staged paths are limited to the two owned
                // runtime scripts above.
                conflict_policy: ApplyRenderConflictPolicy::Accept,
                dry_run: false,
                allow_answers_overwrite: false,
                allow_contract_overwrite: false,
                allow_manifest_overwrite: false,
                backup_root: None,
                progress,
                init_transaction: Some(&mut transaction),
            },
        )?;
        progress.step("seed repair runtime", "managed launcher cache");
        let cache_publication = progress
            .log_blocked_on_err(seed_launcher_repair_runtime(&destination, contract_version))?;
        Ok((render_report, cache_publication))
    })();
    let render_report = match repair_result {
        Ok((render_report, cache_publication)) => match transaction.commit() {
            Ok(()) => {
                cache_publication.commit();
                render_report
            }
            Err(primary) if transaction.needs_rollback() => {
                let primary = cache_publication.finish_failed(primary);
                return Err(
                    transaction.finish_failed_mutation(primary, "launcher-only repair changes")
                );
            }
            Err(primary) => {
                // Commit can report incomplete retained-preimage cleanup
                // after the rendered scripts are already durable. Keep the
                // corresponding cache publication in that case.
                cache_publication.commit();
                return Err(primary);
            }
        },
        Err(primary) => {
            return Err(transaction.finish_failed_mutation(primary, "launcher-only repair changes"));
        }
    };
    progress.done("launcher-only repair complete");

    let next_steps = if legacy_manifest_missing {
        vec![format!(
            "Because {} was missing, review the repository's current harness footprint and answer overrides, then run `cd {} && scripts/jig adopt . --write --force` to establish exact managed-path ownership before a full update.",
            managed_paths::MANIFEST_PATH,
            crate::shell::quote(&destination.to_string_lossy()),
        )]
    } else {
        Vec::new()
    };

    Ok(json!({
        "ok": true,
        "command": "update",
        "render_mode": mode,
        "destination": destination.display().to_string(),
        "answers_file": ANSWERS_FILE,
        "git_initialized": false,
        "render_report": render_report,
        "warnings": warnings,
        "next_steps": next_steps,
    }))
}

fn run_full_update(opts: &UpdateOpts, prepared: PreparedUpdate) -> Result<Value> {
    let PreparedUpdate {
        invocation_cwd,
        destination,
        progress,
        mode,
        prior_managed_paths,
        answers_path,
        ..
    } = prepared;
    progress.step("read answers", answers_path.display());
    let stored = progress.log_blocked_on_err(read_stored_template_state(&answers_path))?;
    progress.step("resolve template", "stored source metadata");
    let update_template = progress.log_blocked_on_err(prepare_update_template_source(
        opts,
        &stored,
        &invocation_cwd,
    ))?;
    let Some(update_template) = update_template else {
        progress.blocked("stored template source metadata is missing");
        bail!(
            "Missing template source metadata in {ANSWERS_FILE}. Re-adopt the repo before running jig update."
        );
    };
    let answers = progress.log_blocked_on_err(RenderAnswers::from_answers_file(&answers_path))?;
    let runtime_policy =
        FullRefreshRuntimePolicy::for_render(answers.harness_footprint(), update_template.source());
    let reconcile_runtime_config =
        crate::context::RepoContext::validate_config_file(&destination).is_ok();
    let staged = stage_render(RenderStageRequest {
        template: &update_template,
        answers: &answers,
        seed_repo_path: Some(&destination),
        prior_managed_paths: Some(&prior_managed_paths),
        reconcile_runtime_config,
        // A full update adopts the contract epoch declared by the current
        // template. Only the narrow launcher-only repair preserves a legacy
        // destination epoch while leaving its manifest and answers untouched.
        contract_version: None,
        progress,
    })?;
    let render_report = apply_staged_render(
        &staged,
        &destination,
        ApplyRenderOptions {
            conflict_policy: if opts.force {
                ApplyRenderConflictPolicy::Accept
            } else {
                ApplyRenderConflictPolicy::Reject(
                    "Update would overwrite or remove template-managed paths. No files were changed. Re-run with --force to accept the rendered output:",
                )
            },
            dry_run: false,
            allow_answers_overwrite: true,
            allow_contract_overwrite: false,
            allow_manifest_overwrite: true,
            backup_root: None,
            progress,
            init_transaction: None,
        },
    )?;
    let warnings = finish_full_refresh(&destination, runtime_policy, progress, "update complete");

    Ok(json!({
        "ok": true,
        "command": "update",
        "render_mode": mode,
        "destination": destination.display().to_string(),
        "answers_file": ANSWERS_FILE,
        "git_initialized": false,
        "render_report": render_report,
        "warnings": warnings,
    }))
}

pub(super) fn legacy_launcher_only_paths(destination: &Path) -> Result<BTreeSet<PathBuf>> {
    let launcher_path = destination.join("scripts/jig");
    let installer_path = destination.join("scripts/install-jig.sh");
    for path in [&launcher_path, &installer_path] {
        let metadata = fs::symlink_metadata(path).with_context(|| {
            format!("Failed to inspect legacy launcher path {}", path.display())
        })?;
        if !metadata.file_type().is_file() {
            bail!(
                "Cannot run launcher-only repair without {}: expected a regular generated file at {}",
                managed_paths::MANIFEST_PATH,
                path.display()
            );
        }
    }

    let launcher = fs::read_to_string(&launcher_path)
        .with_context(|| format!("Failed to read {}", launcher_path.display()))?;
    let installer = fs::read_to_string(&installer_path)
        .with_context(|| format!("Failed to read {}", installer_path.display()))?;
    let launcher_is_generated = recognizable_generated_launcher(&launcher);
    let installer_is_generated = recognizable_generated_installer(&installer);
    if !launcher_is_generated || !installer_is_generated {
        bail!(
            "Cannot run launcher-only repair because {} is missing and the existing scripts are not a recognizable generated Jig launcher/installer pair",
            managed_paths::MANIFEST_PATH
        );
    }

    Ok(LAUNCHER_ONLY_MANAGED_PATHS
        .map(PathBuf::from)
        .into_iter()
        .collect())
}

pub(crate) fn launcher_only_repair_scripts_are_recognizable(destination: &Path) -> bool {
    legacy_launcher_only_paths(destination).is_ok()
}

pub(crate) fn launcher_only_repair_answers_are_valid(destination: &Path) -> bool {
    RenderAnswers::from_answers_file(&destination.join(ANSWERS_FILE)).is_ok()
}

pub(super) fn recognizable_generated_launcher(text: &str) -> bool {
    crate::runtime_artifacts::inspect_launcher(text).is_generated()
}

#[cfg(test)]
pub(crate) fn recognizable_contract_launcher(text: &str) -> bool {
    crate::runtime_artifacts::inspect_launcher(text).uses_repository_scope_protocol()
}

pub(super) fn recognizable_generated_installer(text: &str) -> bool {
    crate::runtime_artifacts::inspect_installer(text).is_generated()
}

#[cfg(test)]
pub(crate) fn recognizable_contract_installer(text: &str) -> bool {
    crate::runtime_artifacts::inspect_installer(text).uses_repository_scope_protocol()
}
