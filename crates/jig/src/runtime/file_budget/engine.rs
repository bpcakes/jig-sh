use super::report::*;
use super::*;

pub(super) fn execute_ready_file_budget(
    context: &FileBudgetEngineContext<'_>,
) -> Result<NativeActionResult> {
    ensure_active(context)?;
    let prepared = context.prepared_input;
    let (expected_raw_digest, expected_semantic_digest) = match &prepared.policy {
        PolicyPreparationV1::Ready {
            policy_raw_digest,
            policy_semantic_digest,
        } => (policy_raw_digest, policy_semantic_digest),
        PolicyPreparationV1::InvalidPolicy { .. } => {
            bail!("ready file-budget evaluation received invalid policy preparation")
        }
    };
    let comparison = match &prepared.comparison {
        ComparisonPreparationV1::Ready { comparison } => comparison,
        ComparisonPreparationV1::ComparisonUnavailable { .. } => {
            bail!("ready file-budget evaluation received unavailable comparison preparation")
        }
    };

    let evaluated_at_ms = crate::state::now_ms();
    let current_date = policy_date_at_ms(evaluated_at_ms)?;
    let policy_bytes = crate::repository::read_policy_bytes(context.repository, prepared.view)
        .map_err(anyhow::Error::msg)?
        .ok_or_else(|| {
            anyhow::anyhow!("authenticated file-budget policy is missing at execution")
        })?;
    ensure_active(context)?;
    let policy = parse_policy_v1(&policy_bytes, current_date).map_err(|error| {
        anyhow::anyhow!("authenticated file-budget policy became invalid: {error}")
    })?;
    let actual_raw_digest = format!("sha256:{}", policy.identity().raw_sha256());
    let actual_semantic_digest = format!("sha256:{}", policy.identity().semantic_sha256());
    if &actual_raw_digest != expected_raw_digest
        || &actual_semantic_digest != expected_semantic_digest
    {
        bail!(
            "authenticated file-budget policy identity changed between preparation and execution"
        );
    }

    let comparison_policy = read_comparison_policy(context, comparison)?;
    let comparison_policy_ref = match &comparison_policy {
        ComparisonPolicyOwnedV1::Absent => ComparisonPolicyV1::Absent,
        ComparisonPolicyOwnedV1::Present(policy) => ComparisonPolicyV1::Present(policy),
        ComparisonPolicyOwnedV1::Unavailable => ComparisonPolicyV1::Unavailable,
    };

    let mut scope = capture_scope_v1_with_cancellation(
        context.repository.root(),
        comparison,
        prepared.view,
        &|| combined_cancelled(context),
    )?;
    ensure_active(context)?;
    let semantic_policy_changed = matches!(
        &comparison_policy,
        ComparisonPolicyOwnedV1::Present(comparison_policy)
            if comparison_policy.identity().semantic_sha256()
                != policy.identity().semantic_sha256()
    );
    if semantic_policy_changed && matches!(context.mode, FileBudgetEvaluationMode::Check) {
        expand_scope_for_policy_change(context, &mut scope)?;
    }
    shape_scope_for_mode(context, comparison, &mut scope)?;
    if !scope.complete {
        return Ok(scope_incomplete_result(
            prepared,
            comparison,
            evaluated_at_ms,
            &scope,
        ));
    }

    let waiver_paths = waiver_paths(&policy, &comparison_policy);
    let waiver_targets = observe_exact_paths_v1_with_cancellation(
        context.repository.root(),
        prepared.view,
        &waiver_paths,
        &|| combined_cancelled(context),
    )?
    .into_iter()
    .map(|fact| ExactCurrentPathFactV1 {
        path: fact.path,
        state: match fact.state {
            GitExactCurrentPathStateV1::Regular => ExactCurrentPathStateV1::Regular,
            GitExactCurrentPathStateV1::Missing => ExactCurrentPathStateV1::Missing,
            GitExactCurrentPathStateV1::Unsupported { reason } => {
                ExactCurrentPathStateV1::Unsupported(unsupported_file_kind(reason))
            }
        },
    })
    .collect::<Vec<_>>();
    ensure_active(context)?;
    if matches!(context.mode, FileBudgetEvaluationMode::Validate) {
        return validate_current_policy_result(
            prepared,
            comparison,
            evaluated_at_ms,
            &policy,
            &scope,
            &waiver_targets,
        );
    }

    let mut measurement_budget = MeasurementBudgetV1::new(
        prepared.configuration.max_total_bytes,
        prepared.configuration.max_total_bytes,
    );
    let mut files = Vec::new();
    let mut digest_facts = Vec::new();
    let mut candidate_count = 0_u64;
    for entry in &scope.entries {
        ensure_active(context)?;
        let disposition = match policy.classify_path(&entry.current_path) {
            Ok(PathDispositionV1::Outside) => continue,
            Ok(PathDispositionV1::Excluded(exclusion)) => {
                format!("excluded:{:?}:{}", exclusion.kind, exclusion.pattern)
            }
            Ok(PathDispositionV1::LocallyExcluded) => "locally_excluded".to_owned(),
            Ok(PathDispositionV1::Governed(rule)) => format!("governed:{}", rule.id),
            Err(_) => "ambiguous".to_owned(),
        };
        candidate_count = candidate_count.saturating_add(1);
        if candidate_count > prepared.configuration.max_candidates {
            return Ok(resource_limit_result(
                prepared,
                comparison,
                evaluated_at_ms,
                format!(
                    "file-budget scope contains more than the configured {} candidates",
                    prepared.configuration.max_candidates
                ),
                candidate_count,
                measurement_budget.total_bytes_read(),
            ));
        }

        let excluded = matches!(
            policy.classify_path(&entry.current_path),
            Ok(PathDispositionV1::Excluded(_) | PathDispositionV1::LocallyExcluded)
        );
        let current = if excluded {
            MeasuredContentV1 {
                measurement: MeasurementV1::default(),
                digest: digest_bytes(b""),
            }
        } else {
            match measure_current_entry(context, entry, &mut measurement_budget) {
                Ok(measured) => measured,
                Err(error) => {
                    return Ok(measurement_error_result(
                        context,
                        prepared,
                        comparison,
                        evaluated_at_ms,
                        &entry.current_path,
                        error,
                        MeasurementProgressV1 {
                            candidate_count,
                            measured_total_bytes: measurement_budget.total_bytes_read(),
                        },
                    ));
                }
            }
        };
        let resolved_unchanged_baseline = if semantic_policy_changed
            && entry.kind == FileChangeKindV1::Unchanged
            && entry.baseline.is_none()
        {
            comparison
                .baseline_oid()
                .map(|tree_oid| {
                    resolve_tree_path_blob_oid_v1_with_cancellation(
                        context.repository.root(),
                        tree_oid,
                        &entry.current_path,
                        &|| combined_cancelled(context),
                    )
                })
                .transpose()?
                .flatten()
                .map(|blob_oid| BaselineFileV1 {
                    path: entry.current_path.clone(),
                    blob_oid,
                })
        } else {
            None
        };
        let baseline_authority = entry
            .baseline
            .as_ref()
            .or(resolved_unchanged_baseline.as_ref());
        let baseline = if excluded {
            None
        } else if let Some(baseline) = baseline_authority {
            match measure_git_blob(context, &baseline.blob_oid, &mut measurement_budget) {
                Ok(measured) => Some(measured),
                Err(error) => {
                    return Ok(measurement_error_result(
                        context,
                        prepared,
                        comparison,
                        evaluated_at_ms,
                        &baseline.path,
                        error,
                        MeasurementProgressV1 {
                            candidate_count,
                            measured_total_bytes: measurement_budget.total_bytes_read(),
                        },
                    ));
                }
            }
        } else {
            None
        };
        files.push(EvaluateFileV1 {
            current_path: entry.current_path.clone(),
            baseline_path: baseline_authority.map(|baseline| baseline.path.clone()),
            current: CurrentFileStateV1::Regular(current.measurement),
            comparison: baseline.as_ref().map(|baseline| baseline.measurement),
        });
        digest_facts.push(CandidateDigestFactV1 {
            change_kind: change_kind(entry.kind),
            current_path: entry.current_path.clone(),
            baseline_path: baseline_authority.map(|baseline| baseline.path.clone()),
            disposition,
            current_content_digest: (!excluded).then_some(current.digest),
            current: (!excluded).then_some(current.measurement),
            comparison_content_digest: baseline.as_ref().map(|baseline| baseline.digest.clone()),
            comparison: baseline.map(|baseline| baseline.measurement),
        });
    }

    let evaluation = evaluate_v1(EvaluationInputV1 {
        policy: &policy,
        comparison_policy: comparison_policy_ref,
        current_date,
        waiver_targets: &waiver_targets,
        files: &files,
    });
    ensure_active(context)?;
    let mut findings = evaluation
        .diagnostics
        .iter()
        .map(normalize_diagnostic)
        .collect::<Vec<_>>();
    let valid_until_ms = earliest_valid_until_ms(&policy)?;
    if valid_until_ms.is_some_and(|boundary| crate::state::now_ms() >= boundary) {
        findings.push(file_budget_finding(
            FindingSeverity::Error,
            "file_budget.waiver_expired",
            "a file-budget waiver expired while evaluation was running; rerun against current UTC policy authority",
            Some(POLICY_PATH_V1),
        ));
        sort_findings(&mut findings);
    }

    let finding_count = findings.len() as u64;
    let findings_digest = digest_json(b"jig-native-findings-v1\0", &(finding_count, &findings));
    let (notice_count, warning_count, error_count) = finding_severity_counts(&findings);
    let blocked = evaluation.diagnostics.iter().any(|diagnostic| {
        matches!(
            diagnostic.code,
            BudgetDiagnosticCodeV1::ScopeIncomplete
                | BudgetDiagnosticCodeV1::BaselineUnavailable
                | BudgetDiagnosticCodeV1::UnsupportedFile
                | BudgetDiagnosticCodeV1::ChangedDuringRead
                | BudgetDiagnosticCodeV1::ResourceLimit
        )
    });
    let conclusion = if blocked {
        RunConclusion::Blocked
    } else if error_count > 0 {
        RunConclusion::Failure
    } else {
        RunConclusion::Success
    };
    let evaluation_identity = json!({
        "schema_version": 1,
        "policy_schema_version": policy.version(),
        "policy_raw_digest": actual_raw_digest,
        "policy_semantic_digest": actual_semantic_digest,
        "comparison": comparison,
        "request": prepared.request,
        "view": prepared.view,
        "configuration": prepared.configuration,
        "scope_complete": scope.complete,
        "scope_issues": scope.issues.iter().map(scope_issue_json).collect::<Vec<_>>(),
        "candidates": digest_facts,
        "evaluated_at_ms": evaluated_at_ms,
        "valid_until_ms": valid_until_ms,
        "finding_count": finding_count,
        "findings_digest": findings_digest,
        "evaluated_file_count": evaluation.evaluated_files,
        "excluded_file_count": evaluation.excluded_files,
        "waived_file_count": evaluation.waived_files,
    });
    let evaluation_digest = digest_json(b"jig-file-budget-evaluation-v1\0", &evaluation_identity);
    let candidate_details = match context.mode {
        FileBudgetEvaluationMode::Explain { .. } => digest_facts.clone(),
        FileBudgetEvaluationMode::Audit { .. } => {
            let mut facts = digest_facts.clone();
            facts.sort_by(|left, right| {
                right
                    .current
                    .map_or(0, |measurement| measurement.bytes)
                    .cmp(&left.current.map_or(0, |measurement| measurement.bytes))
                    .then_with(|| left.current_path.cmp(&right.current_path))
            });
            facts.truncate(64);
            facts
        }
        FileBudgetEvaluationMode::Check | FileBudgetEvaluationMode::Validate => Vec::new(),
    };
    let preview = findings
        .iter()
        .take(FINDING_PREVIEW_LIMIT_V1)
        .cloned()
        .collect::<Vec<_>>();
    let findings_truncated = preview.len() < findings.len();
    let human_output = human_report(
        prepared,
        comparison,
        &actual_raw_digest,
        &actual_semantic_digest,
        &preview,
        finding_count,
        evaluation.evaluated_files,
        evaluation.excluded_files,
        evaluation.waived_files,
        notice_count,
        warning_count,
        error_count,
        &candidate_details,
    );
    let evidence = json!({
        "file_budget": {
            "schema": "jig.file_budget/evidence-v1",
            "policy_schema_version": policy.version(),
            "prepared_input_schema_version": prepared.schema_version,
            "comparison_schema_version": 1,
            "scope_schema_version": 1,
            "report_schema_version": 1,
            "policy_raw_digest": actual_raw_digest,
            "policy_semantic_digest": actual_semantic_digest,
            "policy_preparation": prepared.policy,
            "comparison_preparation": prepared.comparison,
            "comparison": comparison,
            "request": prepared.request,
            "view": prepared.view,
            "configuration": prepared.configuration,
            "evaluation_digest": evaluation_digest,
            "evaluated_file_count": evaluation.evaluated_files,
            "excluded_file_count": evaluation.excluded_files,
            "waived_file_count": evaluation.waived_files,
            "active_waiver_count": policy.waivers().len(),
            "notice_count": notice_count,
            "warning_count": warning_count,
            "error_count": error_count,
            "candidate_count": candidate_count,
            "measured_total_bytes": measurement_budget.total_bytes_read(),
            "candidate_details": candidate_details,
            "complete": true,
            "finding_count": finding_count,
            "finding_preview_count": preview.len(),
            "findings_truncated": findings_truncated,
            "findings_digest": findings_digest,
            "evaluated_at_ms": evaluated_at_ms,
            "valid_until_ms": valid_until_ms,
        }
    });
    Ok(NativeActionResult {
        conclusion,
        findings: preview,
        finding_count,
        findings_truncated,
        findings_digest,
        human_output,
        evidence: Some(evidence),
        evaluated_at_ms,
        valid_until_ms,
    })
}

fn expand_scope_for_policy_change(
    context: &FileBudgetEngineContext<'_>,
    scope: &mut ScopeSnapshotV1,
) -> Result<()> {
    let mut inventory = capture_all_current_scope_v1_with_cancellation(
        context.repository.root(),
        context.prepared_input.view,
        context.prepared_input.view != jig_contract::CurrentViewV1::Index,
        &|| combined_cancelled(context),
    )?;
    let changed = scope
        .entries
        .iter()
        .cloned()
        .map(|entry| (entry.current_path.clone(), entry))
        .collect::<std::collections::BTreeMap<_, _>>();
    for entry in &mut inventory.entries {
        if let Some(authoritative) = changed.get(&entry.current_path) {
            entry.clone_from(authoritative);
        }
    }
    let inventory_paths = inventory
        .entries
        .iter()
        .map(|entry| entry.current_path.clone())
        .collect::<std::collections::BTreeSet<_>>();
    inventory.entries.extend(
        changed
            .into_values()
            .filter(|entry| !inventory_paths.contains(&entry.current_path)),
    );
    inventory.entries.sort();
    inventory.entries.dedup();
    inventory.issues.extend(scope.issues.iter().cloned());
    inventory.issues.sort();
    inventory.issues.dedup();
    inventory.complete = inventory.issues.is_empty();
    *scope = inventory;
    Ok(())
}

fn shape_scope_for_mode(
    context: &FileBudgetEngineContext<'_>,
    comparison: &ResolvedComparisonV1,
    scope: &mut ScopeSnapshotV1,
) -> Result<()> {
    match context.mode {
        FileBudgetEvaluationMode::Check => {}
        FileBudgetEvaluationMode::Audit { tracked_only: true } => {
            scope
                .entries
                .retain(|entry| entry.kind != FileChangeKindV1::Untracked);
        }
        FileBudgetEvaluationMode::Audit {
            tracked_only: false,
        } => {}
        FileBudgetEvaluationMode::Validate => {
            *scope = capture_all_current_scope_v1_with_cancellation(
                context.repository.root(),
                context.prepared_input.view,
                context.prepared_input.view != jig_contract::CurrentViewV1::Index,
                &|| combined_cancelled(context),
            )?;
        }
        FileBudgetEvaluationMode::Explain { path } => {
            if let Some(entry) = scope
                .entries
                .iter()
                .find(|entry| entry.current_path == path)
                .cloned()
            {
                scope.entries = vec![entry];
                return Ok(());
            }
            let observed = observe_exact_paths_v1_with_cancellation(
                context.repository.root(),
                context.prepared_input.view,
                &[path.to_owned()],
                &|| combined_cancelled(context),
            )?;
            let fact = observed
                .first()
                .ok_or_else(|| anyhow::anyhow!("explain path observation returned no fact"))?;
            if fact.state != GitExactCurrentPathStateV1::Regular {
                bail!(
                    "file-budget explain path `{path}` is missing or unsupported in the selected current view"
                );
            }
            let current_source = match context.prepared_input.view {
                jig_contract::CurrentViewV1::Worktree | jig_contract::CurrentViewV1::Inventory => {
                    CurrentSourceV1::WorktreePath
                }
                jig_contract::CurrentViewV1::Index => {
                    let oid = resolve_index_blob_oid_v1_with_cancellation(
                        context.repository.root(),
                        path,
                        &|| combined_cancelled(context),
                    )?
                    .ok_or_else(|| {
                        anyhow::anyhow!(
                            "file-budget explain path `{path}` disappeared from the index"
                        )
                    })?;
                    CurrentSourceV1::IndexBlob { oid }
                }
            };
            let baseline = comparison
                .baseline_oid()
                .map(|tree_oid| {
                    resolve_tree_path_blob_oid_v1_with_cancellation(
                        context.repository.root(),
                        tree_oid,
                        path,
                        &|| combined_cancelled(context),
                    )
                })
                .transpose()?
                .flatten()
                .map(|blob_oid| BaselineFileV1 {
                    path: path.to_owned(),
                    blob_oid,
                });
            scope.entries = vec![ScopeEntryV1 {
                kind: if baseline.is_some() {
                    FileChangeKindV1::Unchanged
                } else {
                    FileChangeKindV1::Added
                },
                current_path: path.to_owned(),
                current_source,
                baseline,
            }];
        }
    }
    Ok(())
}

fn validate_current_policy_result(
    prepared: &PreparedNativeInputV1,
    comparison: &ResolvedComparisonV1,
    evaluated_at_ms: u64,
    policy: &PolicyV1,
    scope: &ScopeSnapshotV1,
    waiver_targets: &[ExactCurrentPathFactV1],
) -> Result<NativeActionResult> {
    let mut diagnostics = scope
        .entries
        .iter()
        .filter_map(|entry| {
            policy
                .classify_path(&entry.current_path)
                .err()
                .map(|value| *value)
        })
        .collect::<Vec<_>>();
    let waiver_validation = evaluate_v1(EvaluationInputV1 {
        policy,
        comparison_policy: ComparisonPolicyV1::Absent,
        current_date: policy_date_at_ms(evaluated_at_ms)?,
        waiver_targets,
        files: &[],
    });
    diagnostics.extend(waiver_validation.diagnostics);
    let mut findings = diagnostics
        .iter()
        .map(normalize_diagnostic)
        .collect::<Vec<_>>();
    sort_findings(&mut findings);
    let conclusion = if findings
        .iter()
        .any(|finding| finding.severity == FindingSeverity::Error)
    {
        RunConclusion::Failure
    } else {
        RunConclusion::Success
    };
    let valid_until_ms = earliest_valid_until_ms(policy)?;
    Ok(result_with_findings(
        conclusion,
        findings,
        evaluated_at_ms,
        valid_until_ms,
        Some(json!({
            "file_budget": {
                "schema": "jig.file_budget/report-v1",
                "operation": "validate",
                "policy_schema_version": policy.version(),
                "policy_raw_digest": format!("sha256:{}", policy.identity().raw_sha256()),
                "policy_semantic_digest": format!("sha256:{}", policy.identity().semantic_sha256()),
                "policy_preparation": prepared.policy,
                "comparison_preparation": prepared.comparison,
                "comparison": comparison,
                "request": prepared.request,
                "view": prepared.view,
                "configuration": prepared.configuration,
                "candidate_count": scope.entries.len(),
                "active_waiver_count": policy.waivers().len(),
                "complete": true,
                "evaluated_at_ms": evaluated_at_ms,
                "valid_until_ms": valid_until_ms,
            }
        })),
    ))
}

enum ComparisonPolicyOwnedV1 {
    Absent,
    Present(Box<PolicyV1>),
    Unavailable,
}

fn read_comparison_policy(
    context: &FileBudgetEngineContext<'_>,
    comparison: &ResolvedComparisonV1,
) -> Result<ComparisonPolicyOwnedV1> {
    let Some(tree_oid) = comparison.baseline_oid() else {
        return Ok(ComparisonPolicyOwnedV1::Absent);
    };
    let bytes = read_tree_path_blob_v1_with_cancellation(
        context.repository.root(),
        tree_oid,
        POLICY_PATH_V1,
        MAX_POLICY_BYTES_V1 + 1,
        &|| combined_cancelled(context),
    )?;
    ensure_active(context)?;
    let Some(bytes) = bytes else {
        return Ok(ComparisonPolicyOwnedV1::Absent);
    };
    if bytes.len() > MAX_POLICY_BYTES_V1 {
        return Ok(ComparisonPolicyOwnedV1::Unavailable);
    }
    Ok(match parse_comparison_policy_v1(&bytes) {
        Ok(policy) => ComparisonPolicyOwnedV1::Present(Box::new(policy)),
        Err(_) => ComparisonPolicyOwnedV1::Unavailable,
    })
}

fn waiver_paths(policy: &PolicyV1, comparison: &ComparisonPolicyOwnedV1) -> Vec<String> {
    let mut paths = policy
        .waivers()
        .iter()
        .map(|waiver| waiver.path.clone())
        .collect::<std::collections::BTreeSet<_>>();
    if let ComparisonPolicyOwnedV1::Present(policy) = comparison {
        paths.extend(policy.waivers().iter().map(|waiver| waiver.path.clone()));
    }
    paths.into_iter().collect()
}

fn measure_current_entry(
    context: &FileBudgetEngineContext<'_>,
    entry: &ScopeEntryV1,
    budget: &mut MeasurementBudgetV1,
) -> Result<MeasuredContentV1> {
    match &entry.current_source {
        CurrentSourceV1::WorktreePath => {
            measure_worktree_path(context, &entry.current_path, budget)
        }
        CurrentSourceV1::IndexBlob { oid } => measure_git_blob(context, oid, budget),
    }
}

fn measure_git_blob(
    context: &FileBudgetEngineContext<'_>,
    oid: &str,
    budget: &mut MeasurementBudgetV1,
) -> Result<MeasuredContentV1> {
    ensure_active(context)?;
    let remaining = budget
        .max_total_bytes()
        .saturating_sub(budget.total_bytes_read());
    let limit = usize::try_from(remaining.saturating_add(1)).unwrap_or(usize::MAX);
    let bytes = read_git_blob_v1_with_cancellation(context.repository.root(), oid, limit, &|| {
        combined_cancelled(context)
    })?;
    ensure_active(context)?;
    let digest = digest_bytes(&bytes);
    let measurement = measure_stream_v1(&mut Cursor::new(bytes), budget, || {
        combined_cancelled(context)
    })
    .map_err(anyhow::Error::new)?;
    Ok(MeasuredContentV1 {
        measurement,
        digest,
    })
}

fn measure_worktree_path(
    context: &FileBudgetEngineContext<'_>,
    path: &str,
    budget: &mut MeasurementBudgetV1,
) -> Result<MeasuredContentV1> {
    ensure_real_parent_chain(context.repository.root(), path)?;
    let full_path = context.repository.root().join(path);
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW);
    }
    let mut file = options
        .open(&full_path)
        .with_context(|| format!("file-budget path `{path}` could not be opened safely"))?;
    let before = file
        .metadata()
        .with_context(|| format!("file-budget path `{path}` metadata could not be read"))?;
    if !before.is_file() {
        bail!("file-budget path `{path}` is not a regular file");
    }
    let mut reader = DigestingReader::new(&mut file);
    let measurement = measure_stream_v1(&mut reader, budget, || combined_cancelled(context))
        .map_err(anyhow::Error::new)?;
    let digest = reader.finish();
    let after = file
        .metadata()
        .with_context(|| format!("file-budget path `{path}` identity could not be rechecked"))?;
    if !same_file_identity(&before, &after) || after.len() != measurement.bytes {
        bail!("file-budget path `{path}` changed while it was being measured");
    }
    ensure_active(context)?;
    Ok(MeasuredContentV1 {
        measurement,
        digest,
    })
}

fn ensure_real_parent_chain(root: &Path, path: &str) -> Result<()> {
    let mut current = PathBuf::from(root);
    let components = path.split('/').collect::<Vec<_>>();
    for component in components.iter().take(components.len().saturating_sub(1)) {
        current.push(component);
        let metadata = std::fs::symlink_metadata(&current).with_context(|| {
            format!("file-budget parent `{}` is unavailable", current.display())
        })?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            bail!("file-budget path `{path}` traverses a non-directory or symlink parent");
        }
    }
    Ok(())
}

#[cfg(unix)]
fn same_file_identity(left: &std::fs::Metadata, right: &std::fs::Metadata) -> bool {
    use std::os::unix::fs::MetadataExt;
    left.dev() == right.dev()
        && left.ino() == right.ino()
        && left.mode() == right.mode()
        && left.len() == right.len()
        && left.mtime() == right.mtime()
        && left.mtime_nsec() == right.mtime_nsec()
}

#[cfg(not(unix))]
fn same_file_identity(left: &std::fs::Metadata, right: &std::fs::Metadata) -> bool {
    left.len() == right.len() && left.modified().ok() == right.modified().ok()
}

struct DigestingReader<'a> {
    inner: &'a mut File,
    hasher: Sha256,
}

impl<'a> DigestingReader<'a> {
    fn new(inner: &'a mut File) -> Self {
        Self {
            inner,
            hasher: Sha256::new(),
        }
    }

    fn finish(self) -> String {
        format!("sha256:{:x}", self.hasher.finalize())
    }
}

impl Read for DigestingReader<'_> {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        let read = self.inner.read(buffer)?;
        self.hasher.update(&buffer[..read]);
        Ok(read)
    }
}
