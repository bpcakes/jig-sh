use std::collections::HashSet;

use anyhow::{Result, bail};
use serde::Deserialize;

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct WorkConfig {
    #[serde(default)]
    checks: Vec<String>,
    #[serde(default)]
    gates: Vec<WorkGateConfig>,
    #[allow(dead_code)]
    #[serde(default)]
    refinements: Vec<WorkRefinementConfig>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct WorkGateConfig {
    id: String,
    kind: String,
    #[serde(default)]
    tool: Option<String>,
    #[serde(default)]
    skill: Option<String>,
    #[serde(default)]
    fail_on: Option<String>,
    #[serde(default)]
    severity: Option<String>,
    #[serde(default)]
    scope: Option<String>,
    #[serde(default)]
    model: Option<String>,
    #[serde(default = "default_required")]
    required: bool,
}

#[derive(Clone, Debug)]
pub(crate) enum WorkGate {
    Check(WorkCheckGate),
    CodexReview(WorkReviewGate),
    Unsupported(UnsupportedWorkGate),
}

impl WorkGate {
    pub(crate) fn id(&self) -> &str {
        match self {
            Self::Check(gate) => &gate.id,
            Self::CodexReview(gate) => &gate.id,
            Self::Unsupported(gate) => &gate.id,
        }
    }

    pub(crate) const fn required(&self) -> bool {
        match self {
            Self::Check(gate) => gate.required,
            Self::CodexReview(gate) => gate.required,
            Self::Unsupported(gate) => gate.required,
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct WorkCheckGate {
    pub(crate) id: String,
    pub(crate) tool: String,
    pub(crate) required: bool,
}

#[derive(Clone, Debug)]
pub(crate) struct WorkReviewGate {
    pub(crate) id: String,
    pub(crate) skill: String,
    pub(crate) threshold: &'static str,
    pub(crate) scope: String,
    pub(crate) model: Option<String>,
    pub(crate) required: bool,
}

#[derive(Clone, Debug)]
pub(crate) struct UnsupportedWorkGate {
    pub(crate) id: String,
    pub(crate) kind: String,
    pub(crate) required: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ReviewScopeArg<'a> {
    Uncommitted,
    Base(&'a str),
    Commit(&'a str),
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct WorkRefinementConfig {
    pub(crate) id: String,
    #[serde(default)]
    pub(crate) skill: Option<String>,
    #[serde(default)]
    pub(crate) mode: Option<String>,
    #[serde(default)]
    pub(crate) model: Option<String>,
}

impl WorkConfig {
    pub(crate) fn gates(&self) -> Vec<WorkGate> {
        let mut gates = self.gates.clone();
        let mut existing_ids = gates
            .iter()
            .map(|gate| gate.id.clone())
            .collect::<HashSet<_>>();

        for tool in &self.checks {
            if gates
                .iter()
                .any(|gate| gate.kind == "check" && gate.tool.as_ref() == Some(tool))
            {
                continue;
            }

            let id = unique_gate_id(gate_id_from_tool_name(tool), &mut existing_ids);
            gates.push(WorkGateConfig {
                id,
                kind: "check".into(),
                tool: Some(tool.clone()),
                skill: None,
                fail_on: None,
                severity: None,
                scope: None,
                model: None,
                required: true,
            });
        }

        gates.into_iter().map(resolve_work_gate).collect()
    }

    pub(crate) fn check_tools(&self) -> Vec<String> {
        self.gates()
            .into_iter()
            .filter_map(|gate| match gate {
                WorkGate::Check(gate) => Some(gate.tool),
                _ => None,
            })
            .collect()
    }

    pub(crate) fn refinements(&self) -> &[WorkRefinementConfig] {
        &self.refinements
    }

    pub(crate) fn validate(&self) -> Result<()> {
        let mut gate_ids = HashSet::new();
        for gate in &self.gates {
            if !gate_ids.insert(gate.id.as_str()) {
                bail!("Duplicate work gate id '{}' in [[work.gates]]", gate.id);
            }
            validate_prompt_token("work gate id", &gate.id)?;
            match gate.kind.as_str() {
                "check" => {
                    if gate.tool.is_none() {
                        bail!("work gate '{}' with kind 'check' requires tool", gate.id);
                    }
                    if gate.skill.is_some()
                        || gate.fail_on.is_some()
                        || gate.severity.is_some()
                        || gate.scope.is_some()
                        || gate.model.is_some()
                    {
                        bail!(
                            "work gate '{}' with kind 'check' only supports tool and required; review-only fields belong on kind 'codex_review'",
                            gate.id
                        );
                    }
                }
                "codex_review" => {
                    if gate.tool.is_some() {
                        bail!(
                            "work gate '{}' with kind 'codex_review' uses skill, not tool",
                            gate.id
                        );
                    }
                    if gate.skill.is_none() {
                        bail!(
                            "work gate '{}' with kind 'codex_review' requires skill",
                            gate.id
                        );
                    }
                    if let Some(skill) = gate.skill.as_deref() {
                        validate_prompt_token("codex_review skill", skill)?;
                    }
                    for threshold in [gate.fail_on.as_deref(), gate.severity.as_deref()]
                        .into_iter()
                        .flatten()
                    {
                        validate_review_severity_threshold(threshold)?;
                    }
                    if let Some(scope) = gate.scope.as_deref() {
                        validate_review_scope(scope)?;
                    }
                    if let Some(model) = gate.model.as_deref() {
                        validate_codex_arg_value("model", model)?;
                    }
                }
                other => {
                    bail!(
                        "Unsupported work gate kind '{other}' for gate '{}'. Expected 'check' or 'codex_review'.",
                        gate.id
                    );
                }
            }
        }

        if self.refinements.len() > 1 {
            bail!(
                "Only one [[work.refinements]] entry is supported until refinement selection is implemented"
            );
        }
        let mut refinement_ids = HashSet::new();
        for refinement in &self.refinements {
            if !refinement_ids.insert(refinement.id.as_str()) {
                bail!(
                    "Duplicate work refinement id '{}' in [[work.refinements]]",
                    refinement.id
                );
            }
            validate_prompt_token("work refinement id", &refinement.id)?;
            if let Some(skill) = refinement.skill.as_deref() {
                validate_prompt_token("work refinement skill", skill)?;
            }
            if let Some(mode) = refinement.mode.as_deref() {
                validate_prompt_token("work refinement mode", mode)?;
            }
            if let Some(model) = refinement.model.as_deref() {
                validate_codex_arg_value("refinement model", model)?;
            }
        }

        Ok(())
    }
}

fn resolve_work_gate(gate: WorkGateConfig) -> WorkGate {
    let WorkGateConfig {
        id,
        kind,
        tool,
        skill,
        fail_on,
        severity,
        scope,
        model,
        required,
    } = gate;

    match kind.as_str() {
        "check" => {
            let Some(tool) = tool else {
                return unsupported_work_gate(id, kind, required);
            };
            WorkGate::Check(WorkCheckGate { id, tool, required })
        }
        "codex_review" => {
            let Some(skill) = skill else {
                return unsupported_work_gate(id, kind, required);
            };
            WorkGate::CodexReview(WorkReviewGate {
                id,
                skill,
                threshold: resolved_review_threshold(fail_on.as_deref(), severity.as_deref()),
                scope: resolved_review_scope(scope),
                model,
                required,
            })
        }
        _ => unsupported_work_gate(id, kind, required),
    }
}

const fn unsupported_work_gate(id: String, kind: String, required: bool) -> WorkGate {
    WorkGate::Unsupported(UnsupportedWorkGate { id, kind, required })
}

fn resolved_review_threshold(fail_on: Option<&str>, severity: Option<&str>) -> &'static str {
    fail_on
        .or(severity)
        .map(normalize_review_threshold)
        .unwrap_or("critical")
}

fn normalize_review_threshold(value: &str) -> &'static str {
    match value {
        "high" | "critical" => "critical",
        "medium" | "warning" => "warning",
        "low" | "suggestion" => "suggestion",
        _ => "critical",
    }
}

fn resolved_review_scope(scope: Option<String>) -> String {
    let Some(scope) = scope else {
        return "uncommitted".into();
    };
    match parse_review_scope_arg(&scope) {
        Ok(ReviewScopeArg::Uncommitted) => "uncommitted".into(),
        Ok(ReviewScopeArg::Base(value)) => format!("base:{value}"),
        Ok(ReviewScopeArg::Commit(value)) => format!("commit:{value}"),
        Err(_) => scope,
    }
}

fn validate_review_scope(value: &str) -> Result<()> {
    parse_review_scope_arg(value).map(|_| ())
}

pub(crate) fn parse_review_scope_arg(value: &str) -> Result<ReviewScopeArg<'_>> {
    if value == "uncommitted" {
        return Ok(ReviewScopeArg::Uncommitted);
    }

    for (prefix, scope) in [
        ("base:", "base"),
        ("base=", "base"),
        ("commit:", "commit"),
        ("commit=", "commit"),
    ] {
        let Some(scoped_ref) = value.strip_prefix(prefix).map(str::trim) else {
            continue;
        };
        if scoped_ref.is_empty() || scoped_ref.starts_with('-') {
            break;
        }
        return Ok(match scope {
            "base" => ReviewScopeArg::Base(scoped_ref),
            "commit" => ReviewScopeArg::Commit(scoped_ref),
            _ => unreachable!("review scope parser only uses known scope names"),
        });
    }

    bail!(
        "Unsupported codex_review scope '{value}'. Use uncommitted, base:<ref>, or commit:<sha>."
    );
}

fn validate_review_severity_threshold(value: &str) -> Result<()> {
    match value {
        "critical" | "warning" | "suggestion" | "high" | "medium" | "low" => Ok(()),
        _ => bail!(
            "Unsupported review severity threshold '{value}'. Expected one of: critical, warning, suggestion, high, medium, low."
        ),
    }
}

fn validate_codex_arg_value(label: &str, value: &str) -> Result<()> {
    if value.trim().is_empty() || value.starts_with('-') {
        bail!("Unsupported codex_review {label} value '{value}'");
    }
    Ok(())
}

fn validate_prompt_token(label: &str, value: &str) -> Result<()> {
    if value.is_empty()
        || value.starts_with('-')
        || !value.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b':' | b'.' | b'_' | b'-' | b'/' | b'@')
        })
    {
        bail!(
            "Unsupported {label} value '{value}'. Use only ASCII letters, numbers, ':', '.', '_', '-', '/', or '@'."
        );
    }
    Ok(())
}

fn gate_id_from_tool_name(tool: &str) -> String {
    tool.strip_prefix("jig.")
        .unwrap_or(tool)
        .replace(['_', '.'], "-")
}

fn unique_gate_id(base: String, existing_ids: &mut HashSet<String>) -> String {
    if existing_ids.insert(base.clone()) {
        return base;
    }

    for index in 2.. {
        let candidate = format!("{base}-{index}");
        if existing_ids.insert(candidate.clone()) {
            return candidate;
        }
    }

    unreachable!("unbounded gate id search should always find an unused suffix")
}

const fn default_required() -> bool {
    true
}
