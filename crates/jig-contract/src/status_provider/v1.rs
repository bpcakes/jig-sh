use schemars::{JsonSchema, Schema, SchemaGenerator, generate::SchemaSettings};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet},
    error::Error,
    fmt,
};

use super::V1_SCHEMA_ID;

// agentic-loc-exception: keep the v1 wire DTOs and semantic rules together for contract audits.

/// Root document emitted by a status provider.
#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[non_exhaustive]
pub struct Report {
    /// Protocol discriminator. For this type it must be `jig.status-provider/v1`.
    pub protocol: Protocol,
    /// Stable provider identity and independently versioned implementation.
    pub provider: Provider,
    /// Unix timestamp in milliseconds when the observation completed.
    pub observed_at_ms: u64,
    /// Whether every intended observation completed.
    pub outcome: Outcome,
    /// Source inputs and revisions inspected by the provider.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub inputs: Vec<Input>,
    /// Software-rewrite work packages observed by the provider.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub work_packages: Vec<WorkPackage>,
    /// Provider-level information that does not belong to one work package.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub diagnostics: Vec<Diagnostic>,
    /// Namespaced provider-specific data preserved by generic consumers.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub extensions: BTreeMap<String, Value>,
}

impl Report {
    /// Creates a complete, empty observation for a provider.
    #[must_use]
    pub const fn complete(provider: Provider, observed_at_ms: u64) -> Self {
        Self {
            protocol: Protocol::V1,
            provider,
            observed_at_ms,
            outcome: Outcome::Complete,
            inputs: Vec::new(),
            work_packages: Vec::new(),
            diagnostics: Vec::new(),
            extensions: BTreeMap::new(),
        }
    }

    /// Validates cross-field and path rules that JSON Schema cannot express.
    ///
    /// Every discovered issue is returned so provider authors can fix one
    /// report in a single pass.
    pub fn validate(&self) -> Result<(), ValidationErrors> {
        let mut errors = Vec::new();
        validate_required(
            &mut errors,
            "/provider/id",
            &self.provider.id,
            "provider id",
        );
        validate_required(
            &mut errors,
            "/provider/adapter_version",
            &self.provider.adapter_version,
            "adapter version",
        );
        validate_optional(
            &mut errors,
            "/provider/display_name",
            self.provider.display_name.as_deref(),
            "provider display name",
        );

        for (index, input) in self.inputs.iter().enumerate() {
            let base = format!("/inputs/{index}");
            validate_required(
                &mut errors,
                &format!("{base}/name"),
                &input.name,
                "input name",
            );
            validate_required(
                &mut errors,
                &format!("{base}/kind"),
                &input.kind,
                "input kind",
            );
            validate_optional(
                &mut errors,
                &format!("{base}/revision"),
                input.revision.as_deref(),
                "input revision",
            );
            validate_optional(
                &mut errors,
                &format!("{base}/digest"),
                input.digest.as_deref(),
                "input digest",
            );
            if let Some(path) = &input.path {
                validate_source_path(&mut errors, &format!("{base}/path"), path);
            }
        }

        let mut package_ids = BTreeSet::new();
        for (index, package) in self.work_packages.iter().enumerate() {
            let base = format!("/work_packages/{index}");
            validate_required(
                &mut errors,
                &format!("{base}/id"),
                &package.id,
                "work-package id",
            );
            if !package.id.trim().is_empty() && !package_ids.insert(package.id.as_str()) {
                push_error(
                    &mut errors,
                    format!("{base}/id"),
                    format!("duplicate work-package id {:?}", package.id),
                );
            }
            validate_optional(
                &mut errors,
                &format!("{base}/title"),
                package.title.as_deref(),
                "work-package title",
            );
            validate_facet(
                &mut errors,
                &format!("{base}/specification"),
                &package.specification,
            );
            validate_facet(
                &mut errors,
                &format!("{base}/implementation"),
                &package.implementation,
            );
            validate_facet(
                &mut errors,
                &format!("{base}/verification"),
                &package.verification,
            );

            let mut dependencies = BTreeSet::new();
            for (dependency_index, dependency) in package.dependencies.iter().enumerate() {
                let path = format!("{base}/dependencies/{dependency_index}");
                validate_required(&mut errors, &path, dependency, "dependency id");
                if !dependency.trim().is_empty() && !dependencies.insert(dependency.as_str()) {
                    push_error(
                        &mut errors,
                        path,
                        format!("duplicate dependency id {dependency:?}"),
                    );
                }
            }

            let mut ordinals = BTreeSet::new();
            for (check_index, check) in package.acceptance_checks.iter().enumerate() {
                let check_base = format!("{base}/acceptance_checks/{check_index}");
                if check.ordinal == 0 {
                    push_error(
                        &mut errors,
                        format!("{check_base}/ordinal"),
                        "acceptance-check ordinal must be one-based",
                    );
                }
                if !ordinals.insert(check.ordinal) {
                    push_error(
                        &mut errors,
                        format!("{check_base}/ordinal"),
                        format!("duplicate acceptance-check ordinal {}", check.ordinal),
                    );
                }
                validate_optional(
                    &mut errors,
                    &format!("{check_base}/id"),
                    check.id.as_deref(),
                    "acceptance-check id",
                );
                validate_required(
                    &mut errors,
                    &format!("{check_base}/state"),
                    &check.state,
                    "acceptance-check state",
                );
                validate_optional(
                    &mut errors,
                    &format!("{check_base}/target"),
                    check.target.as_deref(),
                    "acceptance-check target",
                );
                validate_source(
                    &mut errors,
                    &format!("{check_base}/source"),
                    check.source.as_ref(),
                );
            }

            for (blocker_index, blocker) in package.blockers.iter().enumerate() {
                let blocker_base = format!("{base}/blockers/{blocker_index}");
                validate_required(
                    &mut errors,
                    &format!("{blocker_base}/code"),
                    &blocker.code,
                    "blocker code",
                );
                validate_required(
                    &mut errors,
                    &format!("{blocker_base}/message"),
                    &blocker.message,
                    "blocker message",
                );
                validate_optional(
                    &mut errors,
                    &format!("{blocker_base}/related_work_package"),
                    blocker.related_work_package.as_deref(),
                    "related work-package id",
                );
                validate_source(
                    &mut errors,
                    &format!("{blocker_base}/source"),
                    blocker.source.as_ref(),
                );
            }

            for (evidence_index, evidence) in package.evidence.iter().enumerate() {
                let evidence_base = format!("{base}/evidence/{evidence_index}");
                validate_required(
                    &mut errors,
                    &format!("{evidence_base}/kind"),
                    &evidence.kind,
                    "evidence kind",
                );
                validate_required(
                    &mut errors,
                    &format!("{evidence_base}/reference"),
                    &evidence.reference,
                    "evidence reference",
                );
                validate_optional(
                    &mut errors,
                    &format!("{evidence_base}/digest"),
                    evidence.digest.as_deref(),
                    "evidence digest",
                );
                validate_source(
                    &mut errors,
                    &format!("{evidence_base}/source"),
                    evidence.source.as_ref(),
                );
            }
        }

        for (index, diagnostic) in self.diagnostics.iter().enumerate() {
            let base = format!("/diagnostics/{index}");
            validate_required(
                &mut errors,
                &format!("{base}/code"),
                &diagnostic.code,
                "diagnostic code",
            );
            validate_required(
                &mut errors,
                &format!("{base}/message"),
                &diagnostic.message,
                "diagnostic message",
            );
            validate_optional(
                &mut errors,
                &format!("{base}/work_package"),
                diagnostic.work_package.as_deref(),
                "diagnostic work-package id",
            );
            validate_source(
                &mut errors,
                &format!("{base}/source"),
                diagnostic.source.as_ref(),
            );
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(ValidationErrors { errors })
        }
    }
}

/// Literal protocol discriminator for a v1 report.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
pub enum Protocol {
    /// `jig.status-provider/v1`.
    #[default]
    #[serde(rename = "jig.status-provider/v1")]
    V1,
}

/// Identity of the provider implementation.
#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[non_exhaustive]
pub struct Provider {
    /// Stable, globally recognizable provider id.
    pub id: String,
    /// Provider implementation version, independent of the protocol version.
    pub adapter_version: String,
    /// Optional human-facing name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    /// Namespaced provider-specific identity data.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub extensions: BTreeMap<String, Value>,
}

impl Provider {
    /// Creates a provider identity.
    #[must_use]
    pub fn new(id: impl Into<String>, adapter_version: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            adapter_version: adapter_version.into(),
            display_name: None,
            extensions: BTreeMap::new(),
        }
    }
}

/// Completeness of an otherwise valid provider observation.
#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    /// Every intended observation completed.
    Complete,
    /// The report is trustworthy, but one or more observations were unavailable.
    Partial,
}

/// A source input inspected by the provider.
#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[non_exhaustive]
pub struct Input {
    /// Stable name within this report, such as `target` or `legacy`.
    pub name: String,
    /// Input kind, such as `git`, `document_catalog`, or `database_schema`.
    pub kind: String,
    /// Repository-relative input path when the input is inside the target checkout.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    /// Exact source revision when the input has revision identity.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revision: Option<String>,
    /// Content or configuration digest used to detect stale observations.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub digest: Option<String>,
}

impl Input {
    /// Creates an input observation.
    #[must_use]
    pub fn new(name: impl Into<String>, kind: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            kind: kind.into(),
            path: None,
            revision: None,
            digest: None,
        }
    }
}

/// Status observation for one software-rewrite work package.
#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[non_exhaustive]
pub struct WorkPackage {
    /// Stable work-package id.
    pub id: String,
    /// Optional human-facing title.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Specification readiness observed by the provider.
    pub specification: Facet,
    /// Implementation progress observed by the provider.
    pub implementation: Facet,
    /// Verification progress observed by the provider.
    pub verification: Facet,
    /// Work-package ids that this package depends on.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dependencies: Vec<String>,
    /// Ordered acceptance checks declared for this package.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub acceptance_checks: Vec<AcceptanceCheck>,
    /// Domain blockers discovered by the provider.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub blockers: Vec<Blocker>,
    /// Evidence references supporting the observation.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub evidence: Vec<Evidence>,
    /// Namespaced provider-specific package data.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub extensions: BTreeMap<String, Value>,
}

impl WorkPackage {
    /// Creates a work-package observation with its three independent facets.
    #[must_use]
    pub fn new(
        id: impl Into<String>,
        specification: Facet,
        implementation: Facet,
        verification: Facet,
    ) -> Self {
        Self {
            id: id.into(),
            title: None,
            specification,
            implementation,
            verification,
            dependencies: Vec::new(),
            acceptance_checks: Vec::new(),
            blockers: Vec::new(),
            evidence: Vec::new(),
            extensions: BTreeMap::new(),
        }
    }
}

/// One independent status dimension for a work package.
#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[non_exhaustive]
pub struct Facet {
    /// Provider-native stable state token.
    pub state: String,
    /// Cross-provider normalized category.
    pub category: Category,
    /// Optional human-facing explanation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    /// Source document location supporting the state.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<SourceLocation>,
    /// Specification or progress digest used to detect stale state.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub digest: Option<String>,
}

impl Facet {
    /// Creates a status facet.
    #[must_use]
    pub fn new(state: impl Into<String>, category: Category) -> Self {
        Self {
            state: state.into(),
            category,
            summary: None,
            source: None,
            digest: None,
        }
    }
}

/// Small shared state vocabulary used for generic rendering and aggregation.
#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Category {
    /// The provider could not determine a state.
    Unknown,
    /// Work has not reached a ready or active state.
    Pending,
    /// Work is eligible to advance within this facet.
    Ready,
    /// Work is currently underway.
    Active,
    /// Work cannot advance within this facet.
    Blocked,
    /// Work completed successfully within this facet.
    Complete,
    /// Work completed unsuccessfully or is invalid.
    Failed,
}

/// Status of one numbered work-package acceptance check.
#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[non_exhaustive]
pub struct AcceptanceCheck {
    /// One-based ordinal from the work-package specification.
    pub ordinal: u32,
    /// Optional stable check id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// Provider-native stable state token.
    pub state: String,
    /// Cross-provider normalized category.
    pub category: Category,
    /// Executable test target or other evidence target.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
    /// Source document location declaring the check.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<SourceLocation>,
}

impl AcceptanceCheck {
    /// Creates an acceptance-check observation.
    #[must_use]
    pub fn new(ordinal: u32, state: impl Into<String>, category: Category) -> Self {
        Self {
            ordinal,
            id: None,
            state: state.into(),
            category,
            target: None,
            source: None,
        }
    }
}

/// A domain condition that prevents a work package from advancing.
#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[non_exhaustive]
pub struct Blocker {
    /// Stable machine-readable blocker code.
    pub code: String,
    /// Human-facing explanation.
    pub message: String,
    /// Related work-package id when another package causes the blocker.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub related_work_package: Option<String>,
    /// Source location supporting the blocker.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<SourceLocation>,
}

impl Blocker {
    /// Creates a blocker.
    #[must_use]
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            related_work_package: None,
            source: None,
        }
    }
}

/// Reference to evidence without embedding large or sensitive contents.
#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[non_exhaustive]
pub struct Evidence {
    /// Evidence kind, such as `test`, `receipt`, or `legacy_source`.
    pub kind: String,
    /// Stable test target, receipt id, or other reference.
    pub reference: String,
    /// Optional digest of the referenced evidence.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub digest: Option<String>,
    /// Source location associated with the evidence.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<SourceLocation>,
}

impl Evidence {
    /// Creates an evidence reference.
    #[must_use]
    pub fn new(kind: impl Into<String>, reference: impl Into<String>) -> Self {
        Self {
            kind: kind.into(),
            reference: reference.into(),
            digest: None,
            source: None,
        }
    }
}

/// Provider-level information, warning, or error.
#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[non_exhaustive]
pub struct Diagnostic {
    /// Stable machine-readable diagnostic code.
    pub code: String,
    /// Diagnostic importance.
    pub level: DiagnosticLevel,
    /// Human-facing explanation.
    pub message: String,
    /// Related work-package id when applicable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub work_package: Option<String>,
    /// Source location associated with the diagnostic.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<SourceLocation>,
}

impl Diagnostic {
    /// Creates a provider diagnostic.
    #[must_use]
    pub fn new(
        code: impl Into<String>,
        level: DiagnosticLevel,
        message: impl Into<String>,
    ) -> Self {
        Self {
            code: code.into(),
            level,
            message: message.into(),
            work_package: None,
            source: None,
        }
    }
}

/// Importance of a provider-level diagnostic.
#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticLevel {
    /// Informational detail.
    Info,
    /// Degraded or suspicious state that does not invalidate the report.
    Warning,
    /// An observation failed; a partial report may still be trustworthy.
    Error,
}

/// Repository-relative source location supporting an observation.
#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[non_exhaustive]
pub struct SourceLocation {
    /// Forward-slash repository-relative path.
    pub path: String,
    /// Optional one-based line number.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line: Option<u64>,
    /// Optional one-based column number.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub column: Option<u64>,
}

impl SourceLocation {
    /// Creates a source location without a line or column.
    #[must_use]
    pub fn new(path: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            line: None,
            column: None,
        }
    }
}

/// One semantic validation issue with a JSON Pointer-like path.
#[derive(Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub struct ValidationError {
    /// Field path locating the invalid value.
    pub path: String,
    /// Human-facing explanation of the violated rule.
    pub message: String,
}

/// All semantic validation issues found in one report.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ValidationErrors {
    errors: Vec<ValidationError>,
}

impl ValidationErrors {
    /// Returns every validation issue in discovery order.
    #[must_use]
    pub fn errors(&self) -> &[ValidationError] {
        &self.errors
    }

    /// Consumes the aggregate and returns its validation issues.
    #[must_use]
    pub fn into_errors(self) -> Vec<ValidationError> {
        self.errors
    }
}

impl fmt::Display for ValidationErrors {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "status-provider report has {} validation error(s)",
            self.errors.len()
        )?;
        for error in &self.errors {
            write!(formatter, "; {}: {}", error.path, error.message)?;
        }
        Ok(())
    }
}

impl Error for ValidationErrors {}

/// Generates the normative JSON Schema Draft 2020-12 document for v1.
#[must_use]
pub fn schema() -> Schema {
    let mut schema =
        SchemaGenerator::new(SchemaSettings::draft2020_12()).into_root_schema_for::<Report>();
    schema.insert("$id".to_string(), Value::String(V1_SCHEMA_ID.to_string()));
    schema.insert(
        "title".to_string(),
        Value::String("Jig status-provider report v1".to_string()),
    );
    schema
}

fn validate_facet(errors: &mut Vec<ValidationError>, base: &str, facet: &Facet) {
    validate_required(
        errors,
        &format!("{base}/state"),
        &facet.state,
        "facet state",
    );
    validate_optional(
        errors,
        &format!("{base}/summary"),
        facet.summary.as_deref(),
        "facet summary",
    );
    validate_optional(
        errors,
        &format!("{base}/digest"),
        facet.digest.as_deref(),
        "facet digest",
    );
    validate_source(errors, &format!("{base}/source"), facet.source.as_ref());
}

fn validate_source(errors: &mut Vec<ValidationError>, path: &str, source: Option<&SourceLocation>) {
    let Some(source) = source else {
        return;
    };
    validate_source_path(errors, &format!("{path}/path"), &source.path);
    if source.line == Some(0) {
        push_error(errors, format!("{path}/line"), "line must be one-based");
    }
    if source.column == Some(0) {
        push_error(errors, format!("{path}/column"), "column must be one-based");
    }
}

fn validate_source_path(errors: &mut Vec<ValidationError>, field: &str, value: &str) {
    if value.is_empty() {
        push_error(errors, field, "repository-relative path must not be empty");
        return;
    }
    if value.contains('\0') {
        push_error(
            errors,
            field,
            "repository-relative path must not contain NUL",
        );
    }
    if value.starts_with('/') || value.starts_with('\\') {
        push_error(errors, field, "path must be repository-relative");
    }
    if value.contains('\\') {
        push_error(errors, field, "path must use forward slashes");
    }
    if value.as_bytes().get(1).is_some_and(|byte| *byte == b':')
        && value
            .as_bytes()
            .first()
            .is_some_and(u8::is_ascii_alphabetic)
    {
        push_error(errors, field, "path must not contain a drive prefix");
    }
    for component in value.split('/') {
        if component.is_empty() {
            push_error(errors, field, "path must not contain empty components");
        } else if component == "." || component == ".." {
            push_error(
                errors,
                field,
                "path must not contain `.` or `..` components",
            );
        }
    }
}

fn validate_required(errors: &mut Vec<ValidationError>, path: &str, value: &str, label: &str) {
    if value.trim().is_empty() {
        push_error(errors, path, format!("{label} must not be blank"));
    }
}

fn validate_optional(
    errors: &mut Vec<ValidationError>,
    path: &str,
    value: Option<&str>,
    label: &str,
) {
    if value.is_some_and(|value| value.trim().is_empty()) {
        push_error(
            errors,
            path,
            format!("{label} must not be blank when present"),
        );
    }
}

fn push_error(
    errors: &mut Vec<ValidationError>,
    path: impl Into<String>,
    message: impl Into<String>,
) {
    errors.push(ValidationError {
        path: path.into(),
        message: message.into(),
    });
}
