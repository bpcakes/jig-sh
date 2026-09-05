use jig_contract::status_provider::v1::{
    Blocker, Category, Diagnostic, DiagnosticLevel, Evidence, Facet, SourceLocation, WorkPackage,
};

use crate::dashboard::{
    InputFreshness, ProviderFailure, ProviderSummary, StatusCollectionError, StatusProvider,
    StatusRepositoryObservation, UpstreamObservation,
};

use super::*;

impl From<StatusRepositoryObservation> for RepositoryView {
    fn from(observation: StatusRepositoryObservation) -> Self {
        Self {
            name: sanitize_text(&observation.name),
            default_branch: sanitize_text(&observation.default_branch),
            head_revision: observation.head_revision.as_deref().map(sanitize_text),
            branch: observation.branch.as_deref().map(sanitize_text),
            detached: observation.detached,
            dirty: observation.dirty,
            upstream: observation.upstream.map(Into::into),
        }
    }
}

impl From<UpstreamObservation> for UpstreamView {
    fn from(observation: UpstreamObservation) -> Self {
        Self {
            reference: sanitize_text(&observation.reference),
            ahead: observation.ahead,
            behind: observation.behind,
            state: sanitize_text(&observation.state),
            basis: sanitize_text(&observation.basis),
        }
    }
}

impl ProviderView {
    pub(super) fn from_status(provider: StatusProvider) -> Self {
        let report = provider.report.map(|accepted| accepted.decoded().clone());
        let packages = report
            .as_ref()
            .map(|report| {
                report
                    .work_packages
                    .iter()
                    .cloned()
                    .map(PackageView::from)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let blockers = blocker_items(&packages);
        let summary = provider
            .summary
            .or_else(|| report.as_ref().map(ProviderSummary::from_report))
            .map(Into::into)
            .unwrap_or_default();
        let id = provider.id;
        let display_name = report
            .as_ref()
            .and_then(|report| report.provider.display_name.as_deref())
            .map(sanitize_text);
        let adapter_version = report
            .as_ref()
            .and_then(|report| nonempty(report.provider.adapter_version.clone()))
            .map(|value| sanitize_text(&value));
        let diagnostics = report
            .map(|report| report.diagnostics.into_iter().map(Into::into).collect())
            .unwrap_or_default();

        Self {
            display_id: sanitize_text(&id),
            id,
            display_name,
            adapter_version,
            status: sanitize_text(&provider.status),
            duration_ms: provider.duration_ms,
            summary,
            input_freshness: provider
                .input_freshness
                .into_iter()
                .map(Into::into)
                .collect(),
            packages,
            blockers,
            diagnostics,
            error: provider.error.map(Into::into),
        }
    }
}

fn blocker_items(packages: &[PackageView]) -> Vec<BlockerItemView> {
    packages
        .iter()
        .flat_map(|package| {
            let mut occurrences = HashMap::<BlockerAnchor, usize>::new();
            package.blockers.iter().map(move |blocker| BlockerItemView {
                key: {
                    let anchor = BlockerAnchor::new(&package.id, blocker);
                    let occurrence = occurrences.entry(anchor.clone()).or_default();
                    let key = BlockerKey {
                        anchor,
                        occurrence: *occurrence,
                    };
                    *occurrence += 1;
                    key
                },
                display_package_id: package.display_id.clone(),
                package_title: package.title.clone(),
                specification: package.specification.clone(),
                implementation: package.implementation.clone(),
                verification: package.verification.clone(),
                blocker: blocker.clone(),
            })
        })
        .collect()
}

impl From<ProviderSummary> for SummaryView {
    fn from(summary: ProviderSummary) -> Self {
        Self {
            work_packages: summary.work_packages,
            work_packages_with_blockers: summary.work_packages_with_blockers,
            blockers: summary.blockers,
            acceptance_checks: summary.acceptance_checks,
            diagnostics: summary.diagnostics.into(),
            specification: summary.specification.into(),
            implementation: summary.implementation.into(),
            verification: summary.verification.into(),
            acceptance: summary.acceptance.into(),
        }
    }
}

impl From<crate::dashboard::CategoryCounts> for CategoryCounts {
    fn from(counts: crate::dashboard::CategoryCounts) -> Self {
        Self {
            unknown: counts.unknown,
            pending: counts.pending,
            ready: counts.ready,
            active: counts.active,
            blocked: counts.blocked,
            complete: counts.complete,
            failed: counts.failed,
        }
    }
}

impl From<crate::dashboard::DiagnosticCounts> for DiagnosticCounts {
    fn from(counts: crate::dashboard::DiagnosticCounts) -> Self {
        Self {
            total: counts.total,
            info: counts.info,
            warning: counts.warning,
            error: counts.error,
        }
    }
}

impl From<InputFreshness> for InputFreshnessView {
    fn from(input: InputFreshness) -> Self {
        Self {
            name: sanitize_text(&input.name),
            kind: sanitize_text(&input.kind),
            path: input.path.as_deref().map(sanitize_text),
            expected_revision: input.expected_revision.as_deref().map(sanitize_text),
            observed_revision: input.observed_revision.as_deref().map(sanitize_text),
            dirty: input.dirty,
            status: sanitize_text(&input.status),
            reason: input.reason.as_deref().map(sanitize_text),
        }
    }
}

impl From<WorkPackage> for PackageView {
    fn from(package: WorkPackage) -> Self {
        let acceptance_total = u64::try_from(package.acceptance_checks.len()).unwrap_or(u64::MAX);
        let acceptance_complete = u64::try_from(
            package
                .acceptance_checks
                .iter()
                .filter(|check| check.category == Category::Complete)
                .count(),
        )
        .unwrap_or(u64::MAX);
        let id = package.id;
        Self {
            display_id: sanitize_text(&id),
            title: package
                .title
                .as_deref()
                .map(sanitize_text)
                .unwrap_or_else(|| sanitize_text(&id)),
            id,
            specification: package.specification.into(),
            implementation: package.implementation.into(),
            verification: package.verification.into(),
            dependencies: package
                .dependencies
                .iter()
                .map(|value| sanitize_text(value))
                .collect(),
            acceptance_complete,
            acceptance_total,
            acceptance_checks: package
                .acceptance_checks
                .into_iter()
                .map(Into::into)
                .collect(),
            blockers: package.blockers.into_iter().map(Into::into).collect(),
            evidence: package.evidence.into_iter().map(Into::into).collect(),
            extensions: package.extensions,
        }
    }
}

impl From<Facet> for FacetView {
    fn from(facet: Facet) -> Self {
        Self {
            state: sanitize_text(&facet.state),
            category: category_name(facet.category).to_string(),
            summary: facet.summary.as_deref().map(sanitize_text),
            source: facet.source.map(Into::into),
            digest: facet.digest.as_deref().map(sanitize_text),
        }
    }
}

pub(crate) const fn category_name(category: Category) -> &'static str {
    match category {
        Category::Unknown => "unknown",
        Category::Pending => "pending",
        Category::Ready => "ready",
        Category::Active => "active",
        Category::Blocked => "blocked",
        Category::Complete => "complete",
        Category::Failed => "failed",
    }
}

impl From<Blocker> for BlockerView {
    fn from(blocker: Blocker) -> Self {
        let code = blocker.code;
        let related_work_package = blocker.related_work_package;
        Self {
            display_code: sanitize_text(&code),
            code,
            message: sanitize_text(&blocker.message),
            display_related_work_package: related_work_package.as_deref().map(sanitize_text),
            related_work_package,
            source: blocker.source.map(Into::into),
        }
    }
}

impl From<Evidence> for EvidenceView {
    fn from(evidence: Evidence) -> Self {
        Self {
            kind: sanitize_text(&evidence.kind),
            reference: sanitize_text(&evidence.reference),
            source: evidence.source.map(Into::into),
            digest: evidence.digest.as_deref().map(sanitize_text),
        }
    }
}

impl From<Diagnostic> for DiagnosticView {
    fn from(diagnostic: Diagnostic) -> Self {
        Self {
            code: sanitize_text(&diagnostic.code),
            level: match diagnostic.level {
                DiagnosticLevel::Info => "info",
                DiagnosticLevel::Warning => "warning",
                DiagnosticLevel::Error => "error",
            }
            .to_string(),
            message: sanitize_text(&diagnostic.message),
            work_package: diagnostic.work_package.as_deref().map(sanitize_text),
            source: diagnostic.source.map(Into::into),
        }
    }
}

impl From<SourceLocation> for SourceView {
    fn from(source: SourceLocation) -> Self {
        let path = source.path;
        Self {
            display_path: sanitize_text(&path),
            path,
            line: source.line,
            column: source.column,
        }
    }
}

impl From<ProviderFailure> for ProviderErrorView {
    fn from(error: ProviderFailure) -> Self {
        Self {
            code: sanitize_text(&error.code),
            message: sanitize_text(&error.message),
            stderr: error.stderr.as_deref().map(sanitize_text),
            stderr_truncated: error.stderr_truncated,
        }
    }
}

impl From<StatusCollectionError> for CollectionErrorView {
    fn from(error: StatusCollectionError) -> Self {
        Self {
            scope: sanitize_text(&error.scope),
            code: sanitize_text(&error.code),
            message: sanitize_text(&error.message),
        }
    }
}
