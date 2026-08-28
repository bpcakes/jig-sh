use super::*;

#[derive(Serialize)]
pub(super) struct ProviderSummary {
    work_packages: u64,
    work_packages_with_blockers: u64,
    blockers: u64,
    acceptance_checks: u64,
    diagnostics: DiagnosticCounts,
    specification: CategoryCounts,
    implementation: CategoryCounts,
    verification: CategoryCounts,
    acceptance: CategoryCounts,
}

impl ProviderSummary {
    pub(super) fn from_report(report: &Report) -> Self {
        let mut summary = Self {
            work_packages: report.work_packages.len() as u64,
            work_packages_with_blockers: 0,
            blockers: 0,
            acceptance_checks: 0,
            diagnostics: DiagnosticCounts::default(),
            specification: CategoryCounts::default(),
            implementation: CategoryCounts::default(),
            verification: CategoryCounts::default(),
            acceptance: CategoryCounts::default(),
        };
        for package in &report.work_packages {
            if !package.blockers.is_empty() {
                summary.work_packages_with_blockers += 1;
            }
            summary.blockers += package.blockers.len() as u64;
            summary.acceptance_checks += package.acceptance_checks.len() as u64;
            summary.specification.add(package.specification.category);
            summary.implementation.add(package.implementation.category);
            summary.verification.add(package.verification.category);
            for check in &package.acceptance_checks {
                summary.acceptance.add(check.category);
            }
        }
        for diagnostic in &report.diagnostics {
            summary.diagnostics.total += 1;
            match diagnostic.level {
                DiagnosticLevel::Info => summary.diagnostics.info += 1,
                DiagnosticLevel::Warning => summary.diagnostics.warning += 1,
                DiagnosticLevel::Error => summary.diagnostics.error += 1,
            }
        }
        summary
    }
}

#[derive(Default, Serialize)]
struct DiagnosticCounts {
    total: u64,
    info: u64,
    warning: u64,
    error: u64,
}

#[derive(Default, Serialize)]
struct CategoryCounts {
    unknown: u64,
    pending: u64,
    ready: u64,
    active: u64,
    blocked: u64,
    complete: u64,
    failed: u64,
}

impl CategoryCounts {
    const fn add(&mut self, category: Category) {
        match category {
            Category::Unknown => self.unknown += 1,
            Category::Pending => self.pending += 1,
            Category::Ready => self.ready += 1,
            Category::Active => self.active += 1,
            Category::Blocked => self.blocked += 1,
            Category::Complete => self.complete += 1,
            Category::Failed => self.failed += 1,
        }
    }
}
