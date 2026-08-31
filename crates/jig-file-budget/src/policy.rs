use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use globset::GlobSet;
use serde::{Deserialize, Serialize};

use crate::diagnostic::{
    BudgetDiagnosticCodeV1, BudgetDiagnosticV1, BudgetSeverityV1, sort_diagnostics,
};

use self::identity::{canonical_semantic_input, is_valid_date, sha256_hex};
use self::validation::{
    compile_patterns, convert_waiver, is_outside_candidate_universe, validate_candidate_path_shape,
    validate_exclusion, validate_rule, validate_waiver,
};

mod identity;
mod validation;

pub const MAX_POLICY_BYTES_V1: usize = 1024 * 1024;
pub const MAX_RULES_V1: usize = 256;
pub const MAX_PATTERNS_V1: usize = 4096;
pub const MAX_WAIVERS_V1: usize = 4096;
pub const MAX_PATTERN_BYTES_V1: usize = 1024;
pub const MAX_CANDIDATE_PATH_BYTES_V1: usize = 4096;
pub const MAX_CATEGORY_BYTES_V1: usize = 64;
const MAX_IDENTIFIER_BYTES_V1: usize = 64;
const POLICY_PATH: &str = ".jig/file-budget.toml";
const CONTRACT_PATH: &str = ".jig.toml";

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ExclusionKindV1 {
    Generated,
    Vendored,
    Policy,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ExclusionV1 {
    pub pattern: String,
    pub kind: ExclusionKindV1,
    pub reason: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RuleV1 {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    pub include: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub exclude: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notice_lines: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub warn_lines: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_lines: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notice_bytes: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub warn_bytes: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_bytes: Option<u64>,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct PolicyDateV1 {
    year: u16,
    month: u8,
    day: u8,
}

impl<'de> Deserialize<'de> for PolicyDateV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::parse_iso(&value).map_err(serde::de::Error::custom)
    }
}

impl Serialize for PolicyDateV1 {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

impl PolicyDateV1 {
    pub fn new(year: u16, month: u8, day: u8) -> Result<Self, String> {
        if is_valid_date(year, month, day) {
            Ok(Self { year, month, day })
        } else {
            Err(format!(
                "invalid calendar date {year:04}-{month:02}-{day:02}"
            ))
        }
    }

    fn parse_iso(value: &str) -> Result<Self, String> {
        let bytes = value.as_bytes();
        if bytes.len() != 10
            || bytes[4] != b'-'
            || bytes[7] != b'-'
            || !bytes
                .iter()
                .enumerate()
                .all(|(index, byte)| matches!(index, 4 | 7) || byte.is_ascii_digit())
        {
            return Err(format!("invalid ISO calendar date `{value}`"));
        }
        let year = value[0..4]
            .parse::<u16>()
            .map_err(|_| format!("invalid ISO calendar date `{value}`"))?;
        let month = value[5..7]
            .parse::<u8>()
            .map_err(|_| format!("invalid ISO calendar date `{value}`"))?;
        let day = value[8..10]
            .parse::<u8>()
            .map_err(|_| format!("invalid ISO calendar date `{value}`"))?;
        Self::new(year, month, day)
    }

    #[must_use]
    pub const fn year(self) -> u16 {
        self.year
    }

    #[must_use]
    pub const fn month(self) -> u8 {
        self.month
    }

    #[must_use]
    pub const fn day(self) -> u8 {
        self.day
    }
}

impl fmt::Display for PolicyDateV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{:04}-{:02}-{:02}",
            self.year, self.month, self.day
        )
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WaiverV1 {
    pub id: String,
    pub rule: String,
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ceiling_lines: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ceiling_bytes: Option<u64>,
    pub reason: String,
    pub expires: PolicyDateV1,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PolicyIdentityV1 {
    raw_sha256: String,
    semantic_sha256: String,
    semantic_input: Vec<u8>,
}

impl PolicyIdentityV1 {
    #[must_use]
    pub fn raw_sha256(&self) -> &str {
        &self.raw_sha256
    }

    #[must_use]
    pub fn semantic_sha256(&self) -> &str {
        &self.semantic_sha256
    }

    /// Canonical JSON over the normalized V1 model, suitable as a semantic
    /// identity input. It is not a public policy interchange format.
    #[must_use]
    pub fn semantic_input(&self) -> &[u8] {
        &self.semantic_input
    }
}

#[derive(Clone, Debug)]
pub struct PolicyV1 {
    rules: Vec<RuleV1>,
    exclusions: Vec<ExclusionV1>,
    waivers: Vec<WaiverV1>,
    identity: PolicyIdentityV1,
    rule_matchers: Vec<RuleMatchersV1>,
    exclusion_matchers: GlobSet,
    waiver_by_id: BTreeMap<String, usize>,
    waiver_by_target: BTreeMap<String, BTreeMap<String, usize>>,
}

#[derive(Clone, Debug)]
struct RuleMatchersV1 {
    include: GlobSet,
    exclude: GlobSet,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PathDispositionV1<'a> {
    Outside,
    Excluded(&'a ExclusionV1),
    LocallyExcluded,
    Governed(&'a RuleV1),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InvalidPolicyV1 {
    raw_sha256: String,
    diagnostics: Vec<BudgetDiagnosticV1>,
}

impl InvalidPolicyV1 {
    #[must_use]
    pub fn raw_sha256(&self) -> &str {
        &self.raw_sha256
    }

    #[must_use]
    pub fn diagnostics(&self) -> &[BudgetDiagnosticV1] {
        &self.diagnostics
    }
}

impl fmt::Display for InvalidPolicyV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = self
            .diagnostics
            .first()
            .map_or("invalid file-budget policy", |diagnostic| {
                diagnostic.message.as_str()
            });
        formatter.write_str(message)
    }
}

impl std::error::Error for InvalidPolicyV1 {}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PolicyDtoV1 {
    version: u32,
    rules: Vec<RuleV1>,
    #[serde(default)]
    exclusions: Vec<ExclusionV1>,
    #[serde(default)]
    waivers: Vec<WaiverDtoV1>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct WaiverDtoV1 {
    id: String,
    rule: String,
    path: String,
    #[serde(default)]
    ceiling_lines: Option<u64>,
    #[serde(default)]
    ceiling_bytes: Option<u64>,
    reason: String,
    expires: toml::value::Datetime,
}

/// Parse and validate current policy authority at the supplied UTC calendar
/// date. An expiry equal to `current_date` remains active.
pub fn parse_policy_v1(
    bytes: &[u8],
    current_date: PolicyDateV1,
) -> Result<PolicyV1, InvalidPolicyV1> {
    parse_policy(bytes, Some(current_date))
}

/// Parse comparison-side policy only for historical waiver authorization.
/// Calendar expiry is deliberately not applied to historical authority.
pub fn parse_comparison_policy_v1(bytes: &[u8]) -> Result<PolicyV1, InvalidPolicyV1> {
    parse_policy(bytes, None)
}

fn parse_policy(
    bytes: &[u8],
    current_date: Option<PolicyDateV1>,
) -> Result<PolicyV1, InvalidPolicyV1> {
    let raw_sha256 = sha256_hex(bytes);
    if bytes.len() > MAX_POLICY_BYTES_V1 {
        return Err(invalid_policy(
            raw_sha256,
            vec![policy_diagnostic(format!(
                "policy is {} bytes; version 1 permits at most {MAX_POLICY_BYTES_V1}",
                bytes.len()
            ))],
        ));
    }
    let text = std::str::from_utf8(bytes).map_err(|_| {
        invalid_policy(
            raw_sha256.clone(),
            vec![policy_diagnostic("policy must be valid UTF-8 TOML")],
        )
    })?;
    let generic: toml::Value = toml::from_str(text).map_err(|error| {
        invalid_policy(
            raw_sha256.clone(),
            vec![policy_diagnostic(format!(
                "policy is not valid TOML: {error}"
            ))],
        )
    })?;
    match generic.get("version").and_then(toml::Value::as_integer) {
        Some(1) => {}
        Some(version) => {
            return Err(invalid_policy(
                raw_sha256,
                vec![policy_diagnostic(format!(
                    "unsupported file-budget policy version {version}; this build supports version 1"
                ))],
            ));
        }
        None => {
            return Err(invalid_policy(
                raw_sha256,
                vec![policy_diagnostic(
                    "policy must contain integer field `version = 1`",
                )],
            ));
        }
    }

    let dto: PolicyDtoV1 = toml::from_str(text).map_err(|error| {
        invalid_policy(
            raw_sha256.clone(),
            vec![policy_diagnostic(format!(
                "policy does not satisfy the strict version 1 schema: {error}"
            ))],
        )
    })?;
    debug_assert_eq!(dto.version, 1);
    build_policy(dto, raw_sha256, current_date)
}

fn build_policy(
    dto: PolicyDtoV1,
    raw_sha256: String,
    current_date: Option<PolicyDateV1>,
) -> Result<PolicyV1, InvalidPolicyV1> {
    let mut diagnostics = Vec::new();
    if dto.rules.len() > MAX_RULES_V1 {
        diagnostics.push(policy_diagnostic(format!(
            "policy contains {} rules; version 1 permits at most {MAX_RULES_V1}",
            dto.rules.len()
        )));
    }
    if dto.waivers.len() > MAX_WAIVERS_V1 {
        diagnostics.push(policy_diagnostic(format!(
            "policy contains {} waivers; version 1 permits at most {MAX_WAIVERS_V1}",
            dto.waivers.len()
        )));
    }
    let pattern_count = dto.exclusions.len()
        + dto
            .rules
            .iter()
            .map(|rule| rule.include.len() + rule.exclude.len())
            .sum::<usize>();
    if pattern_count > MAX_PATTERNS_V1 {
        diagnostics.push(policy_diagnostic(format!(
            "policy contains {pattern_count} patterns; version 1 permits at most {MAX_PATTERNS_V1}"
        )));
    }
    if !diagnostics.is_empty() {
        return Err(invalid_policy(raw_sha256, diagnostics));
    }

    let mut rule_ids = BTreeSet::new();
    for rule in &dto.rules {
        validate_rule(rule, &mut diagnostics);
        if !rule_ids.insert(rule.id.as_str()) {
            diagnostics.push(policy_diagnostic(format!(
                "duplicate rule id `{}`",
                rule.id
            )));
        }
    }
    for exclusion in &dto.exclusions {
        validate_exclusion(exclusion, &mut diagnostics);
    }

    let mut waivers = Vec::with_capacity(dto.waivers.len());
    for waiver in dto.waivers {
        match convert_waiver(waiver) {
            Ok(waiver) => waivers.push(waiver),
            Err(diagnostic) => diagnostics.push(*diagnostic),
        }
    }
    let mut waiver_ids = BTreeSet::new();
    let mut waiver_targets = BTreeSet::new();
    for waiver in &waivers {
        validate_waiver(
            waiver,
            &dto.rules,
            &rule_ids,
            current_date,
            &mut diagnostics,
        );
        if !waiver_ids.insert(waiver.id.as_str()) {
            diagnostics.push(waiver_diagnostic(
                waiver,
                format!("duplicate waiver id `{}`", waiver.id),
            ));
        }
        if !waiver_targets.insert((waiver.rule.as_str(), waiver.path.as_str())) {
            diagnostics.push(waiver_diagnostic(
                waiver,
                format!(
                    "duplicate waiver target for rule `{}` and path `{}`",
                    waiver.rule, waiver.path
                ),
            ));
        }
    }
    if !diagnostics.is_empty() {
        return Err(invalid_policy(raw_sha256, diagnostics));
    }

    let mut compile_diagnostics = Vec::new();
    let mut rule_matchers = Vec::with_capacity(dto.rules.len());
    for rule in &dto.rules {
        match (
            compile_patterns(&rule.include),
            compile_patterns(&rule.exclude),
        ) {
            (Ok(include), Ok(exclude)) => {
                rule_matchers.push(RuleMatchersV1 { include, exclude });
            }
            (include, exclude) => {
                let error = include.err().or_else(|| exclude.err()).expect("one failed");
                compile_diagnostics.push(policy_diagnostic(format!(
                    "rule `{}` patterns could not be compiled as one bounded matcher: {error}",
                    rule.id
                )));
            }
        }
    }
    if !compile_diagnostics.is_empty() {
        return Err(invalid_policy(raw_sha256, compile_diagnostics));
    }
    let exclusion_patterns = dto
        .exclusions
        .iter()
        .map(|exclusion| exclusion.pattern.clone())
        .collect::<Vec<_>>();
    let exclusion_matchers = compile_patterns(&exclusion_patterns).map_err(|error| {
        invalid_policy(
            raw_sha256.clone(),
            vec![policy_diagnostic(format!(
                "top-level exclusion patterns could not be compiled as one bounded matcher: {error}"
            ))],
        )
    })?;

    let semantic_input = canonical_semantic_input(&dto.rules, &dto.exclusions, &waivers);
    let identity = PolicyIdentityV1 {
        raw_sha256,
        semantic_sha256: sha256_hex(&semantic_input),
        semantic_input,
    };
    let waiver_by_id = waivers
        .iter()
        .enumerate()
        .map(|(index, waiver)| (waiver.id.clone(), index))
        .collect();
    let mut waiver_by_target = BTreeMap::<String, BTreeMap<String, usize>>::new();
    for (index, waiver) in waivers.iter().enumerate() {
        waiver_by_target
            .entry(waiver.rule.clone())
            .or_default()
            .insert(waiver.path.clone(), index);
    }
    let policy = PolicyV1 {
        rules: dto.rules,
        exclusions: dto.exclusions,
        waivers,
        identity,
        rule_matchers,
        exclusion_matchers,
        waiver_by_id,
        waiver_by_target,
    };
    let mut match_diagnostics = Vec::new();
    for waiver in &policy.waivers {
        match policy.classify_path(&waiver.path) {
            Ok(PathDispositionV1::Governed(rule)) if rule.id == waiver.rule => {}
            Ok(PathDispositionV1::Governed(rule)) => match_diagnostics.push(waiver_diagnostic(
                waiver,
                format!(
                    "waiver path `{}` matches rule `{}`, not named rule `{}`",
                    waiver.path, rule.id, waiver.rule
                ),
            )),
            Ok(
                PathDispositionV1::Outside
                | PathDispositionV1::Excluded(_)
                | PathDispositionV1::LocallyExcluded,
            ) => {
                match_diagnostics.push(waiver_diagnostic(
                    waiver,
                    format!(
                        "waiver path `{}` is not governed by named rule `{}`",
                        waiver.path, waiver.rule
                    ),
                ));
            }
            Err(diagnostic) => match_diagnostics.push(
                waiver_diagnostic(
                    waiver,
                    format!(
                        "waiver path `{}` does not match exactly one effective rule",
                        waiver.path
                    ),
                )
                .for_rule(waiver.rule.clone())
                .at_path(waiver.path.clone())
                .with_related_message(diagnostic.message.clone()),
            ),
        }
    }
    if match_diagnostics.is_empty() {
        Ok(policy)
    } else {
        Err(invalid_policy(
            policy.identity.raw_sha256,
            match_diagnostics,
        ))
    }
}

impl BudgetDiagnosticV1 {
    fn with_related_message(mut self, related: String) -> Self {
        self.message.push_str(": ");
        self.message.push_str(&related);
        self
    }
}

impl PolicyV1 {
    #[must_use]
    pub const fn version(&self) -> u32 {
        1
    }

    #[must_use]
    pub fn rules(&self) -> &[RuleV1] {
        &self.rules
    }

    #[must_use]
    pub fn exclusions(&self) -> &[ExclusionV1] {
        &self.exclusions
    }

    #[must_use]
    pub fn waivers(&self) -> &[WaiverV1] {
        &self.waivers
    }

    #[must_use]
    pub fn identity(&self) -> &PolicyIdentityV1 {
        &self.identity
    }

    pub fn classify_path(
        &self,
        path: &str,
    ) -> Result<PathDispositionV1<'_>, Box<BudgetDiagnosticV1>> {
        if let Err(message) = validate_candidate_path_shape(path) {
            return Err(Box::new(
                BudgetDiagnosticV1::new(
                    BudgetSeverityV1::Error,
                    BudgetDiagnosticCodeV1::ScopeIncomplete,
                    message,
                )
                .at_path(path),
            ));
        }
        if is_outside_candidate_universe(path) {
            return Ok(PathDispositionV1::Outside);
        }
        if let Some(index) =
            self.exclusion_matchers
                .matches(path)
                .into_iter()
                .min_by(|left, right| {
                    let left = &self.exclusions[*left];
                    let right = &self.exclusions[*right];
                    (&left.pattern, left.kind, &left.reason).cmp(&(
                        &right.pattern,
                        right.kind,
                        &right.reason,
                    ))
                })
        {
            return Ok(PathDispositionV1::Excluded(&self.exclusions[index]));
        }
        let mut locally_excluded = false;
        let mut matches = Vec::new();
        for (rule, matchers) in self.rules.iter().zip(&self.rule_matchers) {
            if !matchers.include.is_match(path) {
                continue;
            }
            if matchers.exclude.is_match(path) {
                locally_excluded = true;
            } else {
                matches.push(rule);
            }
        }
        match matches.as_slice() {
            [] if locally_excluded => Ok(PathDispositionV1::LocallyExcluded),
            [] => Ok(PathDispositionV1::Outside),
            [rule] => Ok(PathDispositionV1::Governed(rule)),
            _ => {
                let mut ids = matches
                    .iter()
                    .map(|rule| rule.id.as_str())
                    .collect::<Vec<_>>();
                ids.sort_unstable();
                let ids = ids.join(", ");
                Err(Box::new(
                    BudgetDiagnosticV1::new(
                        BudgetSeverityV1::Error,
                        BudgetDiagnosticCodeV1::RuleAmbiguous,
                        format!("path `{path}` matches multiple effective rules: {ids}"),
                    )
                    .at_path(path),
                ))
            }
        }
    }

    #[must_use]
    pub fn waiver(&self, id: &str) -> Option<&WaiverV1> {
        self.waiver_by_id.get(id).map(|index| &self.waivers[*index])
    }

    #[must_use]
    pub fn waiver_for(&self, rule: &str, path: &str) -> Option<&WaiverV1> {
        self.waiver_by_target
            .get(rule)
            .and_then(|by_path| by_path.get(path))
            .map(|index| &self.waivers[*index])
    }
}

fn invalid_policy(raw_sha256: String, mut diagnostics: Vec<BudgetDiagnosticV1>) -> InvalidPolicyV1 {
    sort_diagnostics(&mut diagnostics);
    InvalidPolicyV1 {
        raw_sha256,
        diagnostics,
    }
}

fn policy_diagnostic(message: impl Into<String>) -> BudgetDiagnosticV1 {
    BudgetDiagnosticV1::new(
        BudgetSeverityV1::Error,
        BudgetDiagnosticCodeV1::PolicyInvalid,
        message,
    )
}

fn waiver_diagnostic(waiver: &WaiverV1, message: impl Into<String>) -> BudgetDiagnosticV1 {
    BudgetDiagnosticV1::new(
        BudgetSeverityV1::Error,
        BudgetDiagnosticCodeV1::WaiverInvalid,
        message,
    )
    .at_path(waiver.path.clone())
    .for_rule(waiver.rule.clone())
    .for_waiver(waiver.id.clone())
}
