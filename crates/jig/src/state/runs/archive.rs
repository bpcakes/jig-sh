use super::*;

#[cfg(test)]
use std::cell::Cell;

#[cfg(test)]
thread_local! {
    static CORRUPT_NEXT_RUN_ARCHIVE_AFTER_PUBLISH: Cell<bool> = const { Cell::new(false) };
}

#[cfg(test)]
pub(super) fn corrupt_next_run_archive_after_publish() {
    CORRUPT_NEXT_RUN_ARCHIVE_AFTER_PUBLISH.with(|corrupt| corrupt.set(true));
}

#[derive(Default)]
struct RunArchiveLifecycle {
    event_count: usize,
    known_event_count: usize,
    queued: bool,
    work_plan_id: Option<String>,
    planned_targets: BTreeSet<TargetId>,
    completed_targets: BTreeSet<TargetId>,
    completed_at_ms: Option<u64>,
}

impl RunArchiveLifecycle {
    fn observe(&mut self, event: &RunEventRecord) -> Result<()> {
        let known = matches!(
            event.event.as_str(),
            EVENT_QUEUED
                | EVENT_RUNNING
                | EVENT_TARGET_STARTED
                | EVENT_TARGET_COMPLETED
                | EVENT_COMPLETED
                | EVENT_CANCEL_REQUESTED
        );
        if known {
            validate_run_id_for_lease(&event.run_id)?;
        }
        if event.event == EVENT_QUEUED {
            if self.queued || self.known_event_count > 0 {
                bail!(
                    "run '{}' has more than one or a late queued event",
                    event.run_id
                );
            }
            let plan = event
                .plan
                .as_ref()
                .ok_or_else(|| anyhow!("run '{}' queued event has no plan", event.run_id))?;
            self.planned_targets = plan
                .targets
                .iter()
                .map(|target| target.target.clone())
                .collect();
            if self.planned_targets.len() != plan.targets.len() {
                bail!("run '{}' plan contains duplicate targets", event.run_id);
            }
            self.queued = true;
            self.work_plan_id.clone_from(&event.work_plan_id);
        } else if known && !self.queued {
            bail!(
                "run '{}' has a {} event before queued",
                event.run_id,
                event.event
            );
        }
        match event.event.as_str() {
            EVENT_TARGET_STARTED => {
                let target = event.target.as_ref().ok_or_else(|| {
                    anyhow!("run '{}' target_started event has no target", event.run_id)
                })?;
                if !self.planned_targets.contains(target) {
                    bail!(
                        "run '{}' references unplanned target '{target}'",
                        event.run_id
                    );
                }
                if self.completed_targets.contains(target) {
                    bail!(
                        "run '{}' target '{target}' started after completion",
                        event.run_id
                    );
                }
            }
            EVENT_TARGET_COMPLETED => {
                let result = event.result.as_ref().ok_or_else(|| {
                    anyhow!(
                        "run '{}' target_completed event has no result",
                        event.run_id
                    )
                })?;
                if event.target.as_ref() != Some(&result.target) {
                    bail!(
                        "run '{}' target_completed identity does not match its result",
                        event.run_id
                    );
                }
                if !self.planned_targets.contains(&result.target) {
                    bail!(
                        "run '{}' references unplanned target '{}'",
                        event.run_id,
                        result.target
                    );
                }
                if result.status != RunStatus::Completed || result.conclusion.is_none() {
                    bail!(
                        "run '{}' target '{}' has a nonterminal result",
                        event.run_id,
                        result.target
                    );
                }
                if !self.completed_targets.insert(result.target.clone()) {
                    bail!(
                        "run '{}' target '{}' completed more than once",
                        event.run_id,
                        result.target
                    );
                }
            }
            EVENT_COMPLETED => {
                if self.completed_targets != self.planned_targets {
                    bail!(
                        "run '{}' completed before every target reached a conclusion",
                        event.run_id
                    );
                }
                if event.conclusion.is_none() {
                    bail!("run '{}' completed event has no conclusion", event.run_id);
                }
                if self.completed_at_ms.replace(event.timestamp_ms).is_some() {
                    bail!("run '{}' has more than one completed event", event.run_id);
                }
                self.planned_targets.clear();
                self.completed_targets.clear();
            }
            _ => {}
        }
        if self.completed_at_ms.is_some()
            && known
            && !matches!(
                event.event.as_str(),
                EVENT_COMPLETED | EVENT_CANCEL_REQUESTED
            )
        {
            bail!(
                "run '{}' has a {} event after completion",
                event.run_id,
                event.event
            );
        }
        if known {
            self.known_event_count = self.known_event_count.saturating_add(1);
        }
        self.event_count = self.event_count.saturating_add(1);
        Ok(())
    }
}

fn scan_run_archive_lifecycles(
    path: &Path,
    mut scan: impl FnMut(&mut dyn FnMut(RawJsonlRecord<'_>) -> Result<()>) -> Result<bool>,
) -> Result<BTreeMap<String, RunArchiveLifecycle>> {
    let mut lifecycles = BTreeMap::<String, RunArchiveLifecycle>::new();
    let mut observe = |raw: RawJsonlRecord<'_>| {
        let event = parse_run_event(raw, path)?;
        lifecycles
            .entry(event.run_id.clone())
            .or_default()
            .observe(&event)
    };
    if scan(&mut observe)? {
        bail!(
            "Refusing to archive {} because its final JSONL record is not newline-terminated",
            path.display()
        );
    }
    for (run_id, lifecycle) in &lifecycles {
        if lifecycle.known_event_count > 0 && !lifecycle.queued {
            bail!("run '{run_id}' has no queued event");
        }
    }
    Ok(lifecycles)
}

pub(in crate::state) fn validate_run_stream(path: &Path) -> Result<()> {
    scan_run_archive_lifecycles(path, |observe| {
        let report = scan_jsonl_raw(path, &|| false, &mut *observe)?;
        Ok(report.unterminated_final_record)
    })?;
    Ok(())
}

pub(in crate::state) fn ensure_run_stream_replaceable(
    ctx: &RepoContext,
    path: &Path,
    guard: &JsonlWriteGuard,
) -> Result<()> {
    let lifecycles = scan_run_archive_lifecycles(path, |observe| {
        let report = scan_jsonl_raw_locked(guard, path, &|| false, &mut *observe)?;
        Ok(report.unterminated_final_record)
    })?;
    let nonterminal_run_ids = lifecycles
        .iter()
        .filter(|(_, lifecycle)| {
            lifecycle.known_event_count > 0 && lifecycle.completed_at_ms.is_none()
        })
        .map(|(run_id, _)| run_id.as_str())
        .collect::<Vec<_>>();
    if !nonterminal_run_ids.is_empty() {
        let (preview, suffix) = run_id_preview(&nonterminal_run_ids);
        bail!(
            "Refusing to restore run state while {} nonterminal run(s) exist ({preview}{suffix}); wait for them to complete or cancel them before retrying",
            nonterminal_run_ids.len()
        );
    }

    // A restore is also a recovery path for a missing or externally damaged
    // journal. In that case a live worker's stable lease may no longer have a
    // corresponding queued event to discover above, so inspect every owned
    // lease file as well as every lifecycle represented by current state.
    let mut lease_run_ids = lifecycles.keys().cloned().collect::<BTreeSet<_>>();
    let lease_dir = ctx.root().join(RUN_LEASE_DIR);
    match fs::read_dir(&lease_dir) {
        Ok(entries) => {
            for entry in entries {
                let entry = entry.with_context(|| {
                    format!(
                        "Failed to inspect run lease directory {}",
                        lease_dir.display()
                    )
                })?;
                let name = entry.file_name().into_string().map_err(|_| {
                    anyhow!(
                        "Run lease directory {} contains a non-UTF-8 entry",
                        lease_dir.display()
                    )
                })?;
                let Some(run_id) = name.strip_suffix(".lock") else {
                    continue;
                };
                validate_run_id_for_lease(run_id)?;
                lease_run_ids.insert(run_id.to_owned());
            }
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(error).with_context(|| {
                format!(
                    "Failed to inspect run lease directory {}",
                    lease_dir.display()
                )
            });
        }
    }

    let mut active_run_ids = Vec::new();
    for run_id in &lease_run_ids {
        if !run_lease_is_idle(ctx, run_id)? {
            active_run_ids.push(run_id.as_str());
        }
    }
    if !active_run_ids.is_empty() {
        let (preview, suffix) = run_id_preview(&active_run_ids);
        bail!(
            "Refusing to restore run state while {} active worker lease(s) remain ({preview}{suffix}); wait for the run workers to exit before retrying",
            active_run_ids.len()
        );
    }
    Ok(())
}

fn run_id_preview(run_ids: &[&str]) -> (String, String) {
    let preview = run_ids
        .iter()
        .take(10)
        .copied()
        .collect::<Vec<_>>()
        .join(", ");
    let omitted = run_ids.len().saturating_sub(10);
    let suffix = if omitted == 0 {
        String::new()
    } else {
        format!(" (+{omitted} more)")
    };
    (preview, suffix)
}

fn reconcile_abandoned_runs_before_archive(ctx: &RepoContext, path: &Path) -> Result<usize> {
    let lifecycles = scan_run_archive_lifecycles(path, |observe| {
        let report = scan_jsonl_raw(path, &|| false, &mut *observe)?;
        Ok(report.unterminated_final_record)
    })?;
    let nonterminal_run_ids = lifecycles
        .into_iter()
        .filter(|(_, lifecycle)| {
            lifecycle.known_event_count > 0 && lifecycle.completed_at_ms.is_none()
        })
        .map(|(run_id, _)| run_id)
        .collect::<Vec<_>>();
    nonterminal_run_ids
        .into_iter()
        .try_fold(0usize, |count, run_id| {
            let run = reconcile_run_for_inspection(ctx, &run_id)?;
            Ok(count.saturating_add(usize::from(run.result.status == RunStatus::Completed)))
        })
}

fn validate_run_archive_artifact(
    path: &Path,
    artifact: &GzipWriteReport,
    expected_event_count: usize,
) -> Result<()> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let (restored, restored_report) =
        decompress_gzip_to_temp(path, parent, Some(artifact.uncompressed_bytes)).with_context(
            || format!("Failed to verify published run archive {}", path.display()),
        )?;
    let lifecycles = scan_run_archive_lifecycles(restored.path(), |observe| {
        let report = scan_jsonl_raw(restored.path(), &|| false, &mut *observe)?;
        Ok(report.unterminated_final_record)
    })
    .with_context(|| {
        format!(
            "Failed to validate published run archive {}",
            path.display()
        )
    })?;
    let restored_event_count = lifecycles.values().try_fold(0usize, |count, lifecycle| {
        count
            .checked_add(lifecycle.event_count)
            .context("Run archive event count overflow")
    })?;
    if restored_report.uncompressed_bytes != artifact.uncompressed_bytes
        || restored_report.uncompressed_sha256 != artifact.uncompressed_sha256
    {
        bail!(
            "Run archive content verification failed for {}; refusing to rewrite active state",
            path.display()
        );
    }
    if restored_event_count != expected_event_count {
        bail!(
            "Run archive event count mismatch for {}; expected {expected_event_count}, found {restored_event_count}; refusing to rewrite active state",
            path.display()
        );
    }
    Ok(())
}

fn validate_published_run_archive(
    path: &Path,
    artifact: GzipWriteReport,
    expected_event_count: usize,
) -> Result<GzipWriteReport> {
    #[cfg(test)]
    CORRUPT_NEXT_RUN_ARCHIVE_AFTER_PUBLISH.with(|corrupt| {
        if corrupt.replace(false) {
            fs::write(path, b"corrupt published run archive")?;
        }
        Ok::<_, io::Error>(())
    })?;

    match validate_run_archive_artifact(path, &artifact, expected_event_count) {
        Ok(()) => Ok(artifact),
        Err(error) => {
            remove_invalid_gzip(path).with_context(|| {
                format!(
                    "{error:#}; additionally failed to remove invalid run archive {}",
                    path.display()
                )
            })?;
            Err(error)
        }
    }
}

pub(crate) fn runs_archive(ctx: &RepoContext, before: &str, dry_run: bool) -> Result<Value> {
    ensure_state_layout(ctx)?;
    let before_ms = crate::state::receipts::parse_archive_before_ms(before)?;
    let runs_path = ctx.state_file(RUNS_FILE);
    // Apply mode is already a mutating operation, so recover runs whose stable
    // worker lease proves that their process exited before writing a terminal
    // event. Preview remains strictly read-only and reports such runs instead.
    let abandoned_runs_reconciled = if dry_run {
        0
    } else {
        reconcile_abandoned_runs_before_archive(ctx, &runs_path)?
    };
    let open_plan_ids = crate::state::open_plan_summaries(ctx)?
        .into_iter()
        .filter_map(|plan| plan["plan_id"].as_str().map(str::to_owned))
        .collect::<BTreeSet<_>>();
    let mut recovery_hint = None;
    let mut archive_hint = None;
    let result = with_jsonl_write_lock(&runs_path, |guard| {
        let lifecycles = scan_run_archive_lifecycles(&runs_path, |observe| {
            let report = scan_jsonl_raw_locked(guard, &runs_path, &|| false, &mut *observe)?;
            Ok(report.unterminated_final_record)
        })?;
        let nonterminal_run_ids = lifecycles
            .iter()
            .filter(|(_, lifecycle)| {
                lifecycle.known_event_count > 0 && lifecycle.completed_at_ms.is_none()
            })
            .map(|(run_id, _)| run_id.as_str())
            .collect::<Vec<_>>();
        let nonterminal_runs = nonterminal_run_ids.len();
        if nonterminal_runs > 0 {
            let (preview, suffix) = run_id_preview(&nonterminal_run_ids);
            bail!(
                "Refusing to archive runs while {nonterminal_runs} nonterminal run(s) exist ({preview}{suffix}); wait for them to complete or cancel them before retrying"
            );
        }
        let mut archived_run_ids = BTreeSet::new();
        let mut protected_runs_retained = 0usize;
        let mut active_run_leases_retained = 0usize;
        let mut run_events_archived = 0usize;
        for (run_id, lifecycle) in &lifecycles {
            if lifecycle
                .completed_at_ms
                .is_some_and(|ended| ended < before_ms)
            {
                if lifecycle
                    .work_plan_id
                    .as_ref()
                    .is_some_and(|plan_id| open_plan_ids.contains(plan_id))
                {
                    protected_runs_retained = protected_runs_retained.saturating_add(1);
                } else if !run_lease_is_idle(ctx, run_id)? {
                    // A worker may append its terminal event just before
                    // releasing the lease. Retain that run until a later
                    // archive so every supported host keeps one stable inode
                    // for each process that can still hold the lease.
                    active_run_leases_retained = active_run_leases_retained.saturating_add(1);
                } else {
                    archived_run_ids.insert(run_id.clone());
                    run_events_archived = run_events_archived.saturating_add(lifecycle.event_count);
                }
            }
        }

        let runs_archived = archived_run_ids.len();
        let runs_retained = lifecycles.len().saturating_sub(runs_archived);
        let run_event_count_before = lifecycles.values().fold(0usize, |count, lifecycle| {
            count.saturating_add(lifecycle.event_count)
        });
        // Lease files are non-authoritative cache state. Once a terminal run's
        // lease is idle, no execution or inspection path opens it again. Remove
        // it before rewriting the durable stream so a cleanup failure remains
        // retryable without a partially completed archive.
        let run_leases_pruned = if dry_run {
            0
        } else {
            archived_run_ids.iter().try_fold(0usize, |count, run_id| {
                Ok::<_, anyhow::Error>(
                    count.saturating_add(usize::from(remove_run_lease(ctx, run_id)?)),
                )
            })?
        };
        let recovery_backup_path = if runs_archived > 0 && !dry_run {
            Some(
                crate::state::maintenance::create_runs_backup(
                    ctx,
                    &runs_path,
                    "runs-archive-recovery",
                    None,
                )?
                .0,
            )
        } else {
            None
        };
        recovery_hint = recovery_backup_path.clone();
        let archive_path = (runs_archived > 0 && !dry_run).then(|| {
            ctx.root()
                .join(".agent/.cache/state-archives")
                .join(format!("runs-before-{before_ms}-{}.jsonl.gz", Ulid::new()))
        });
        archive_hint = archive_path.clone();
        let artifact = match &archive_path {
            Some(path) => {
                let artifact = write_gzip_atomic(path, |writer| {
                    let report = scan_jsonl_raw_locked(guard, &runs_path, &|| false, |raw| {
                        let identity = parse_run_event_identity(raw, &runs_path)?;
                        if archived_run_ids.contains(&identity.run_id) {
                            writer.write_all(raw.bytes)?;
                            writer.write_all(b"\n")?;
                        }
                        Ok(())
                    })?;
                    if report.unterminated_final_record {
                        bail!("Run state changed to an unterminated stream during archive");
                    }
                    Ok(())
                })?;
                Some(validate_published_run_archive(
                    path,
                    artifact,
                    run_events_archived,
                )?)
            }
            None => None,
        };

        if !dry_run && runs_archived > 0 {
            let rewrite = rewrite_jsonl_raw_locked(
                guard,
                &runs_path,
                &|| false,
                |raw| {
                    let identity = parse_run_event_identity(raw, &runs_path)?;
                    Ok(if archived_run_ids.contains(&identity.run_id) {
                        RawJsonlRewrite::Drop
                    } else {
                        RawJsonlRewrite::Keep
                    })
                },
                validate_run_stream,
            )?;
            if rewrite.dropped_records as usize != run_events_archived {
                bail!(
                    "Run archive selected {run_events_archived} events but rewrote {}",
                    rewrite.dropped_records
                );
            }
        }

        Ok(json!({
            "runs_source_path": ".agent/state/runs.jsonl",
            "runs_archive_path": archive_path.map(|path| path.display().to_string()),
            "runs_recovery_backup_path": recovery_backup_path
                .map(|path| path.display().to_string()),
            "run_event_count_before": run_event_count_before,
            "run_events_archived": run_events_archived,
            "runs_archived": runs_archived,
            "runs_retained": runs_retained,
            "protected_runs_retained": protected_runs_retained,
            "active_run_leases_retained": active_run_leases_retained,
            "abandoned_runs_reconciled": abandoned_runs_reconciled,
            "run_leases_pruned": run_leases_pruned,
            "runs_uncompressed_bytes": artifact.as_ref().map(|artifact| artifact.uncompressed_bytes),
            "runs_compressed_bytes": artifact.as_ref().map(|artifact| artifact.compressed_bytes),
            "runs_content_sha256": artifact.as_ref().map(|artifact| artifact.uncompressed_sha256.as_str()),
            "writer_coordination_note": MAINTENANCE_WRITER_COORDINATION_NOTE,
        }))
    });
    result.map_err(|error| {
        let recovery = recovery_hint.as_ref().map_or_else(
            || "no exact runs recovery backup was completed".into(),
            |path| format!("exact runs recovery backup: {}", path.display()),
        );
        let archive = archive_hint.as_ref().map_or_else(
            || "no run-event archive was completed".into(),
            |path| format!("run-event archive: {}", path.display()),
        );
        anyhow!("{error:#}\nRun archive recovery context: {recovery}; {archive}")
    })
}
