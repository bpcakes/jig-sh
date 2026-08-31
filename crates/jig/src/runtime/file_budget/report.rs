use super::*;

pub(super) fn engine_error_result(
    context: &FileBudgetEngineContext<'_>,
    error: anyhow::Error,
) -> NativeActionResult {
    let evaluated_at_ms = crate::state::now_ms();
    let (conclusion, code, message) = if let Some(stop) = classify_stop(context, &error) {
        match stop {
            EngineStopV1::Cancelled => (
                RunConclusion::Cancelled,
                "file_budget.cancelled",
                "file-budget evaluation was cancelled".to_owned(),
            ),
            EngineStopV1::TimedOut => (
                RunConclusion::TimedOut,
                "file_budget.timed_out",
                "file-budget evaluation reached its deadline".to_owned(),
            ),
        }
    } else if error
        .downcast_ref::<jig_file_budget::MeasurementErrorV1>()
        .is_some_and(|error| {
            matches!(
                error.kind,
                MeasurementErrorKindV1::PerFileLimit
                    | MeasurementErrorKindV1::TotalLimit
                    | MeasurementErrorKindV1::CounterOverflow
            )
        })
        || format!("{error:#}").contains("output limit")
    {
        (
            RunConclusion::Blocked,
            "file_budget.resource_limit",
            bounded_error_message(context.repository, &error),
        )
    } else {
        (
            RunConclusion::Blocked,
            if format!("{error:#}").contains("changed while") {
                "file_budget.changed_during_read"
            } else {
                "file_budget.scope_incomplete"
            },
            bounded_error_message(context.repository, &error),
        )
    };
    terminal_result(
        conclusion,
        code,
        &message,
        evaluated_at_ms,
        Some(json!({
            "file_budget": {
                "schema": "jig.file_budget/evidence-v1",
                "prepared_input_schema_version": context.prepared_input.schema_version,
                "policy_preparation": context.prepared_input.policy,
                "comparison_preparation": context.prepared_input.comparison,
                "request": context.prepared_input.request,
                "view": context.prepared_input.view,
                "configuration": context.prepared_input.configuration,
                "complete": false,
                "evaluated_at_ms": evaluated_at_ms,
            }
        })),
    )
}

pub(super) fn measurement_error_result(
    context: &FileBudgetEngineContext<'_>,
    prepared: &PreparedNativeInputV1,
    comparison: &ResolvedComparisonV1,
    evaluated_at_ms: u64,
    path: &str,
    error: anyhow::Error,
    progress: MeasurementProgressV1,
) -> NativeActionResult {
    let (conclusion, code) = if let Some(stop) = classify_stop(context, &error) {
        match stop {
            EngineStopV1::Cancelled => (RunConclusion::Cancelled, "file_budget.cancelled"),
            EngineStopV1::TimedOut => (RunConclusion::TimedOut, "file_budget.timed_out"),
        }
    } else if error
        .downcast_ref::<jig_file_budget::MeasurementErrorV1>()
        .is_some_and(|error| {
            matches!(
                error.kind,
                MeasurementErrorKindV1::PerFileLimit
                    | MeasurementErrorKindV1::TotalLimit
                    | MeasurementErrorKindV1::CounterOverflow
            )
        })
        || format!("{error:#}").contains("output limit")
    {
        (RunConclusion::Blocked, "file_budget.resource_limit")
    } else if format!("{error:#}").contains("changed while") {
        (RunConclusion::Blocked, "file_budget.changed_during_read")
    } else {
        (RunConclusion::Blocked, "file_budget.scope_incomplete")
    };
    let message = bounded_error_message(context.repository, &error);
    let mut finding = file_budget_finding(FindingSeverity::Error, code, &message, Some(path));
    finding.location = Some(FindingLocation {
        path: path.to_owned(),
        line: None,
        column: None,
    });
    result_with_findings(
        conclusion,
        vec![finding],
        evaluated_at_ms,
        None,
        Some(json!({
            "file_budget": {
                "schema": "jig.file_budget/evidence-v1",
                "policy_preparation": prepared.policy,
                "comparison_preparation": prepared.comparison,
                "comparison": comparison,
                "request": prepared.request,
                "view": prepared.view,
                "configuration": prepared.configuration,
                "candidate_count": progress.candidate_count,
                "measured_total_bytes": progress.measured_total_bytes,
                "complete": false,
                "evaluated_at_ms": evaluated_at_ms,
            }
        })),
    )
}

pub(super) fn scope_incomplete_result(
    prepared: &PreparedNativeInputV1,
    comparison: &ResolvedComparisonV1,
    evaluated_at_ms: u64,
    scope: &ScopeSnapshotV1,
) -> NativeActionResult {
    let findings = scope
        .issues
        .iter()
        .map(|issue| {
            file_budget_finding(
                FindingSeverity::Error,
                "file_budget.scope_incomplete",
                &issue.message,
                issue.path.as_deref(),
            )
        })
        .collect::<Vec<_>>();
    let issue_count = scope.issues.len();
    let issue_preview = scope
        .issues
        .iter()
        .take(EVIDENCE_ISSUE_PREVIEW_LIMIT_V1)
        .map(scope_issue_json)
        .collect::<Vec<_>>();
    result_with_findings(
        RunConclusion::Blocked,
        findings,
        evaluated_at_ms,
        None,
        Some(json!({
            "file_budget": {
                "schema": "jig.file_budget/evidence-v1",
                "policy_preparation": prepared.policy,
                "comparison_preparation": prepared.comparison,
                "comparison": comparison,
                "request": prepared.request,
                "view": prepared.view,
                "configuration": prepared.configuration,
                "complete": false,
                "scope_issue_count": issue_count,
                "scope_issues_truncated": issue_preview.len() < issue_count,
                "scope_issues": issue_preview,
                "evaluated_at_ms": evaluated_at_ms,
            }
        })),
    )
}

pub(super) fn resource_limit_result(
    prepared: &PreparedNativeInputV1,
    comparison: &ResolvedComparisonV1,
    evaluated_at_ms: u64,
    message: String,
    candidate_count: u64,
    measured_total_bytes: u64,
) -> NativeActionResult {
    let finding = file_budget_finding(
        FindingSeverity::Error,
        "file_budget.resource_limit",
        &message,
        None,
    );
    result_with_findings(
        RunConclusion::Blocked,
        vec![finding],
        evaluated_at_ms,
        None,
        Some(json!({
            "file_budget": {
                "schema": "jig.file_budget/evidence-v1",
                "policy_preparation": prepared.policy,
                "comparison_preparation": prepared.comparison,
                "comparison": comparison,
                "request": prepared.request,
                "view": prepared.view,
                "configuration": prepared.configuration,
                "candidate_count": candidate_count,
                "measured_total_bytes": measured_total_bytes,
                "complete": false,
                "evaluated_at_ms": evaluated_at_ms,
            }
        })),
    )
}

pub(super) fn result_with_findings(
    conclusion: RunConclusion,
    findings: Vec<Finding>,
    evaluated_at_ms: u64,
    valid_until_ms: Option<u64>,
    evidence: Option<Value>,
) -> NativeActionResult {
    let finding_count = findings.len() as u64;
    let findings_digest = digest_json(b"jig-native-findings-v1\0", &(finding_count, &findings));
    let preview = findings
        .iter()
        .take(FINDING_PREVIEW_LIMIT_V1)
        .cloned()
        .collect::<Vec<_>>();
    let findings_truncated = preview.len() < findings.len();
    let mut human_output = preview
        .iter()
        .map(|finding| finding.message.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    if findings_truncated {
        human_output.push_str(&format!(
            "\n... {} finding(s) omitted",
            findings.len() - preview.len()
        ));
    }
    NativeActionResult {
        conclusion,
        findings: preview,
        finding_count,
        findings_truncated,
        findings_digest,
        human_output: bound_utf8(&human_output, HUMAN_OUTPUT_BYTES_V1),
        evidence,
        evaluated_at_ms,
        valid_until_ms,
    }
}

pub(super) fn terminal_result(
    conclusion: RunConclusion,
    code: &str,
    message: &str,
    evaluated_at_ms: u64,
    evidence: Option<Value>,
) -> NativeActionResult {
    result_with_findings(
        conclusion,
        vec![file_budget_finding(
            FindingSeverity::Error,
            code,
            message,
            None,
        )],
        evaluated_at_ms,
        None,
        evidence,
    )
}

pub(super) fn normalize_diagnostic(diagnostic: &BudgetDiagnosticV1) -> Finding {
    file_budget_finding(
        match diagnostic.severity {
            BudgetSeverityV1::Error => FindingSeverity::Error,
            BudgetSeverityV1::Warning => FindingSeverity::Warning,
            BudgetSeverityV1::Notice => FindingSeverity::Notice,
        },
        diagnostic.code.as_str(),
        &diagnostic.message,
        diagnostic.path.as_deref(),
    )
}

pub(super) fn file_budget_finding(
    severity: FindingSeverity,
    code: &str,
    message: &str,
    path: Option<&str>,
) -> Finding {
    Finding {
        severity,
        message: bound_utf8(message, 4 * 1024),
        code: Some(code.to_owned()),
        source: Some(jig_contract::tool::FILE_BUDGET.to_owned()),
        location: path.map(|path| FindingLocation {
            path: path.to_owned(),
            line: None,
            column: None,
        }),
    }
}

pub(super) fn sort_findings(findings: &mut [Finding]) {
    findings.sort_by(|left, right| {
        severity_rank(left.severity)
            .cmp(&severity_rank(right.severity))
            .then_with(|| {
                left.location
                    .as_ref()
                    .map(|location| location.path.as_str())
                    .cmp(
                        &right
                            .location
                            .as_ref()
                            .map(|location| location.path.as_str()),
                    )
            })
            .then_with(|| left.code.cmp(&right.code))
            .then_with(|| left.message.cmp(&right.message))
    });
}

pub(super) const fn severity_rank(severity: FindingSeverity) -> u8 {
    match severity {
        FindingSeverity::Error => 0,
        FindingSeverity::Warning => 1,
        FindingSeverity::Notice => 2,
    }
}

pub(super) fn finding_severity_counts(findings: &[Finding]) -> (u64, u64, u64) {
    findings.iter().fold((0, 0, 0), |mut counts, finding| {
        match finding.severity {
            FindingSeverity::Notice => counts.0 += 1,
            FindingSeverity::Warning => counts.1 += 1,
            FindingSeverity::Error => counts.2 += 1,
        }
        counts
    })
}

#[allow(clippy::too_many_arguments)]
pub(super) fn human_report(
    prepared: &PreparedNativeInputV1,
    comparison: &ResolvedComparisonV1,
    policy_raw_digest: &str,
    policy_semantic_digest: &str,
    findings: &[Finding],
    finding_count: u64,
    evaluated_files: u64,
    excluded_files: u64,
    waived_files: u64,
    notice_count: u64,
    warning_count: u64,
    error_count: u64,
    candidate_details: &[CandidateDigestFactV1],
) -> String {
    let mut output = format!(
        "file budget: view={:?} comparison={} max_candidates={} max_total_bytes={}\npolicy: raw={} semantic={}\n",
        prepared.view,
        comparison_kind(comparison),
        prepared.configuration.max_candidates,
        prepared.configuration.max_total_bytes,
        policy_raw_digest,
        policy_semantic_digest,
    );
    for finding in findings {
        let severity = match finding.severity {
            FindingSeverity::Error => "error",
            FindingSeverity::Warning => "warning",
            FindingSeverity::Notice => "notice",
        };
        let path = finding
            .location
            .as_ref()
            .map_or(String::new(), |location| format!(" {}", location.path));
        let code = finding.code.as_deref().unwrap_or("file_budget");
        output.push_str(&format!("{severity} {code}{path}: {}\n", finding.message));
    }
    for detail in candidate_details {
        let current = detail.current.map_or_else(
            || "current=missing".to_owned(),
            |measurement| {
                format!(
                    "current_lines={} current_bytes={}",
                    measurement.lines, measurement.bytes
                )
            },
        );
        let comparison = detail.comparison.map_or_else(
            || "comparison=missing".to_owned(),
            |measurement| {
                format!(
                    "comparison_lines={} comparison_bytes={}",
                    measurement.lines, measurement.bytes
                )
            },
        );
        output.push_str(&format!(
            "detail {}: change={} disposition={} {current} {comparison}\n",
            detail.current_path, detail.change_kind, detail.disposition
        ));
    }
    let omitted = finding_count.saturating_sub(findings.len() as u64);
    output.push_str(&format!(
        "summary: evaluated={evaluated_files} excluded={excluded_files} waived={waived_files} notices={notice_count} warnings={warning_count} errors={error_count} findings={finding_count} omitted={omitted}\n"
    ));
    bound_utf8(&output, HUMAN_OUTPUT_BYTES_V1)
}

pub(super) fn earliest_valid_until_ms(policy: &PolicyV1) -> Result<Option<u64>> {
    policy
        .waivers()
        .iter()
        .map(|waiver| {
            let month = Month::try_from(waiver.expires.month())
                .map_err(|_| anyhow::anyhow!("invalid waiver expiry month"))?;
            let date = Date::from_calendar_date(
                i32::from(waiver.expires.year()),
                month,
                waiver.expires.day(),
            )
            .map_err(|_| anyhow::anyhow!("invalid waiver expiry date"))?;
            let boundary = date
                .next_day()
                .ok_or_else(|| anyhow::anyhow!("waiver expiry exceeds supported calendar"))?;
            let timestamp = PrimitiveDateTime::new(boundary, Time::MIDNIGHT)
                .assume_utc()
                .unix_timestamp_nanos();
            u64::try_from(timestamp / 1_000_000).map_err(|_| {
                anyhow::anyhow!("waiver validity boundary is outside u64 milliseconds")
            })
        })
        .collect::<Result<Vec<_>>>()
        .map(|boundaries| boundaries.into_iter().min())
}

pub(super) fn policy_date_at_ms(timestamp_ms: u64) -> Result<PolicyDateV1> {
    let seconds = i64::try_from(timestamp_ms / 1_000)
        .context("evaluation timestamp exceeds supported UTC range")?;
    let date = OffsetDateTime::from_unix_timestamp(seconds)
        .context("evaluation timestamp is outside supported UTC range")?
        .date();
    PolicyDateV1::new(date.year() as u16, u8::from(date.month()), date.day())
        .map_err(anyhow::Error::msg)
}

pub(super) fn ensure_active(context: &FileBudgetEngineContext<'_>) -> Result<()> {
    if (context.cancelled)() {
        return Err(EngineStopV1::Cancelled.into());
    }
    if Instant::now() >= context.deadline {
        return Err(EngineStopV1::TimedOut.into());
    }
    Ok(())
}

pub(super) fn combined_cancelled(context: &FileBudgetEngineContext<'_>) -> bool {
    (context.cancelled)() || Instant::now() >= context.deadline
}

pub(super) fn classify_stop(
    context: &FileBudgetEngineContext<'_>,
    error: &anyhow::Error,
) -> Option<EngineStopV1> {
    if let Some(stop) = error.downcast_ref::<EngineStopV1>() {
        return Some(*stop);
    }
    if error
        .downcast_ref::<jig_file_budget::MeasurementErrorV1>()
        .is_some_and(|error| error.kind == MeasurementErrorKindV1::Cancelled)
        || is_git_receipt_collection_cancellation(error)
    {
        return Some(if Instant::now() >= context.deadline {
            EngineStopV1::TimedOut
        } else {
            EngineStopV1::Cancelled
        });
    }
    None
}

impl std::fmt::Display for EngineStopV1 {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Cancelled => "file-budget evaluation was cancelled",
            Self::TimedOut => "file-budget evaluation reached its deadline",
        })
    }
}

impl std::error::Error for EngineStopV1 {}

pub(super) fn bounded_error_message(repository: &RepoContext, error: &anyhow::Error) -> String {
    let message =
        format!("{error:#}").replace(&repository.root().display().to_string(), "<repository>");
    bound_utf8(&message, 4 * 1024)
}

pub(super) fn unsupported_file_kind(kind: ScopeIssueKindV1) -> UnsupportedFileKindV1 {
    match kind {
        ScopeIssueKindV1::Symlink => UnsupportedFileKindV1::Symlink,
        ScopeIssueKindV1::Gitlink | ScopeIssueKindV1::EmbeddedRepository => {
            UnsupportedFileKindV1::Gitlink
        }
        _ => UnsupportedFileKindV1::Special,
    }
}

pub(super) const fn change_kind(kind: FileChangeKindV1) -> &'static str {
    match kind {
        FileChangeKindV1::Added => "added",
        FileChangeKindV1::Modified => "modified",
        FileChangeKindV1::TypeChanged => "type_changed",
        FileChangeKindV1::Renamed => "renamed",
        FileChangeKindV1::Untracked => "untracked",
        FileChangeKindV1::Unchanged => "unchanged",
    }
}

pub(super) const fn comparison_kind(comparison: &ResolvedComparisonV1) -> &'static str {
    match comparison {
        ResolvedComparisonV1::MergeBase { .. } => "merge_base",
        ResolvedComparisonV1::ExactTree { .. } => "exact_tree",
        ResolvedComparisonV1::IndexAgainstHead { .. } => "index_against_head",
        ResolvedComparisonV1::StrictInventory { .. } => "strict_inventory",
    }
}

pub(super) fn scope_issue_json(issue: &crate::git_receipts::ScopeIssueV1) -> Value {
    json!({
        "kind": format!("{:?}", issue.kind).to_ascii_lowercase(),
        "path": issue.path,
        "message": issue.message,
    })
}

pub(super) fn digest_bytes(bytes: &[u8]) -> String {
    format!("sha256:{:x}", Sha256::digest(bytes))
}

pub(super) fn digest_json(domain: &[u8], value: &impl Serialize) -> String {
    let encoded = serde_json::to_vec(value).expect("file-budget digest facts are serializable");
    let mut hasher = Sha256::new();
    hasher.update(domain);
    hasher.update((encoded.len() as u64).to_be_bytes());
    hasher.update(encoded);
    format!("sha256:{:x}", hasher.finalize())
}

pub(super) fn bound_utf8(value: &str, max_bytes: usize) -> String {
    if value.len() <= max_bytes {
        return value.to_owned();
    }
    let mut end = max_bytes;
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    let mut bounded = value[..end].to_owned();
    bounded.push_str("\n[output truncated by Jig]\n");
    bounded
}
