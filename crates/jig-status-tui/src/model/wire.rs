use serde::Deserialize;
use serde_json::Value;
use std::collections::BTreeMap;

use super::{CategoryCounts, DiagnosticCounts};

#[derive(Deserialize)]
pub(super) struct AggregateWire {
    pub(super) schema_version: u64,
    #[serde(default)]
    pub(super) observed_at_ms: u64,
    #[serde(default)]
    pub(super) outcome: String,
    #[serde(default)]
    pub(super) repository: RepositoryWire,
    #[serde(default)]
    pub(super) work: Value,
    #[serde(default)]
    pub(super) loops: Value,
    #[serde(default)]
    pub(super) providers: Vec<ProviderWire>,
    #[serde(default)]
    pub(super) errors: Vec<CollectionErrorWire>,
}

#[derive(Default, Deserialize)]
#[serde(default)]
pub(super) struct RepositoryWire {
    pub(super) name: String,
    pub(super) default_branch: String,
    pub(super) head_revision: Option<String>,
    pub(super) branch: Option<String>,
    pub(super) detached: bool,
    pub(super) dirty: Option<bool>,
    pub(super) upstream: Option<UpstreamWire>,
}

#[derive(Default, Deserialize)]
#[serde(default)]
pub(super) struct UpstreamWire {
    pub(super) reference: String,
    pub(super) ahead: u64,
    pub(super) behind: u64,
    pub(super) state: String,
    pub(super) basis: String,
}

#[derive(Default, Deserialize)]
#[serde(default)]
pub(super) struct ProviderWire {
    pub(super) id: String,
    pub(super) status: String,
    pub(super) duration_ms: u64,
    pub(super) summary: Option<SummaryWire>,
    pub(super) input_freshness: Vec<InputFreshnessWire>,
    pub(super) report: Option<ReportWire>,
    pub(super) error: Option<ProviderErrorWire>,
}

#[derive(Default, Deserialize)]
#[serde(default)]
pub(super) struct SummaryWire {
    pub(super) work_packages: u64,
    pub(super) work_packages_with_blockers: u64,
    pub(super) blockers: u64,
    pub(super) acceptance_checks: u64,
    pub(super) diagnostics: DiagnosticCounts,
    pub(super) specification: CategoryCounts,
    pub(super) implementation: CategoryCounts,
    pub(super) verification: CategoryCounts,
    pub(super) acceptance: CategoryCounts,
}

#[derive(Default, Deserialize)]
#[serde(default)]
pub(super) struct InputFreshnessWire {
    pub(super) name: String,
    pub(super) kind: String,
    pub(super) path: Option<String>,
    pub(super) expected_revision: Option<String>,
    pub(super) observed_revision: Option<String>,
    pub(super) dirty: Option<bool>,
    pub(super) status: String,
    pub(super) reason: Option<String>,
}

#[derive(Default, Deserialize)]
#[serde(default)]
pub(super) struct ReportWire {
    pub(super) provider: ProviderIdentityWire,
    pub(super) work_packages: Vec<PackageWire>,
    pub(super) diagnostics: Vec<DiagnosticWire>,
}

#[derive(Default, Deserialize)]
#[serde(default)]
pub(super) struct ProviderIdentityWire {
    pub(super) adapter_version: String,
    pub(super) display_name: Option<String>,
}

#[derive(Default, Deserialize)]
#[serde(default)]
pub(super) struct PackageWire {
    pub(super) id: String,
    pub(super) title: Option<String>,
    pub(super) specification: FacetWire,
    pub(super) implementation: FacetWire,
    pub(super) verification: FacetWire,
    pub(super) dependencies: Vec<String>,
    pub(super) acceptance_checks: Vec<AcceptanceCheckWire>,
    pub(super) blockers: Vec<BlockerWire>,
    pub(super) evidence: Vec<EvidenceWire>,
    pub(super) extensions: BTreeMap<String, Value>,
}

#[derive(Default, Deserialize)]
#[serde(default)]
pub(super) struct FacetWire {
    pub(super) state: String,
    pub(super) category: String,
    pub(super) summary: Option<String>,
    pub(super) source: Option<SourceWire>,
    pub(super) digest: Option<String>,
}

#[derive(Default, Deserialize)]
#[serde(default)]
pub(super) struct AcceptanceCheckWire {
    pub(super) ordinal: u64,
    pub(super) id: Option<String>,
    pub(super) state: String,
    pub(super) category: String,
    pub(super) target: Option<String>,
    pub(super) source: Option<SourceWire>,
}

#[derive(Default, Deserialize)]
#[serde(default)]
pub(super) struct BlockerWire {
    pub(super) code: String,
    pub(super) message: String,
    pub(super) related_work_package: Option<String>,
    pub(super) source: Option<SourceWire>,
}

#[derive(Default, Deserialize)]
#[serde(default)]
pub(super) struct EvidenceWire {
    pub(super) kind: String,
    pub(super) reference: String,
    pub(super) source: Option<SourceWire>,
    pub(super) digest: Option<String>,
}

#[derive(Default, Deserialize)]
#[serde(default)]
pub(super) struct DiagnosticWire {
    pub(super) code: String,
    pub(super) level: String,
    pub(super) message: String,
    pub(super) work_package: Option<String>,
    pub(super) source: Option<SourceWire>,
}

#[derive(Default, Deserialize)]
#[serde(default)]
pub(super) struct SourceWire {
    pub(super) path: String,
    pub(super) line: Option<u64>,
    pub(super) column: Option<u64>,
}

#[derive(Default, Deserialize)]
#[serde(default)]
pub(super) struct ProviderErrorWire {
    pub(super) code: String,
    pub(super) message: String,
    pub(super) stderr: Option<String>,
    pub(super) stderr_truncated: bool,
}

#[derive(Default, Deserialize)]
#[serde(default)]
pub(super) struct CollectionErrorWire {
    pub(super) scope: String,
    pub(super) code: String,
    pub(super) message: String,
}
