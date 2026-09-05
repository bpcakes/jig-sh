use crate::dashboard::{GateFinding, GateObservation, GatesObservation, Remediation};

use super::{LimitView, format_timestamp, sanitize_rows, sanitize_text};

#[derive(Clone, Debug)]
pub(crate) struct GateSetView {
    pub(crate) overall: String,
    pub(crate) gates: Vec<GateView>,
    pub(crate) limit: LimitView,
}

impl From<GatesObservation> for GateSetView {
    fn from(gates: GatesObservation) -> Self {
        Self {
            overall: sanitize_text(&gates.overall),
            limit: LimitView::from_rows(&gates.gates),
            gates: gates
                .gates
                .items()
                .iter()
                .cloned()
                .map(Into::into)
                .collect(),
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct GateView {
    pub(crate) id: String,
    pub(crate) required: bool,
    pub(crate) subject: String,
    pub(crate) status: String,
    pub(crate) freshness: String,
    pub(crate) ended_at: String,
    pub(crate) diff_summary: String,
    pub(crate) changed_paths: Vec<String>,
    pub(crate) changed_limit: LimitView,
    pub(crate) matching_paths: Vec<String>,
    pub(crate) matching_limit: LimitView,
    pub(crate) findings: Vec<FindingView>,
    pub(crate) findings_limit: LimitView,
    pub(crate) remediation: Option<RemediationView>,
}

impl From<GateObservation> for GateView {
    fn from(gate: GateObservation) -> Self {
        Self {
            id: sanitize_text(&gate.id),
            required: gate.required,
            subject: gate
                .tool
                .or(gate.skill)
                .as_deref()
                .map(sanitize_text)
                .unwrap_or_else(|| "—".to_string()),
            status: sanitize_text(&gate.status),
            freshness: gate
                .freshness
                .as_deref()
                .map(sanitize_text)
                .unwrap_or_else(|| "—".to_string()),
            ended_at: format_timestamp(gate.ended_at_ms),
            diff_summary: gate
                .diff_summary
                .as_deref()
                .map(sanitize_text)
                .unwrap_or_else(|| "—".to_string()),
            changed_limit: LimitView::from_rows(&gate.changed_paths),
            changed_paths: sanitize_rows(&gate.changed_paths),
            matching_limit: LimitView::from_rows(&gate.matching_paths),
            matching_paths: sanitize_rows(&gate.matching_paths),
            findings_limit: LimitView::from_rows(&gate.findings),
            findings: gate
                .findings
                .items()
                .iter()
                .cloned()
                .map(Into::into)
                .collect(),
            remediation: gate.remediation.map(Into::into),
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct FindingView {
    pub(crate) line: String,
}

impl From<GateFinding> for FindingView {
    fn from(finding: GateFinding) -> Self {
        let location =
            finding
                .path
                .as_deref()
                .map(sanitize_text)
                .map_or_else(String::new, |path| {
                    finding
                        .line
                        .map_or(path.clone(), |line| format!("{path}:{line}"))
                });
        let location = if location.is_empty() {
            String::new()
        } else {
            format!(" ({location})")
        };
        Self {
            line: format!(
                "{} {}{location}",
                sanitize_text(&finding.code),
                sanitize_text(&finding.message)
            ),
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct RemediationView {
    pub(crate) display: String,
    pub(crate) inert_argv: String,
}

impl From<Remediation> for RemediationView {
    fn from(remediation: Remediation) -> Self {
        Self {
            display: sanitize_text(&remediation.display),
            inert_argv: remediation
                .argv
                .iter()
                .map(|arg| quote_argument(&sanitize_text(arg)))
                .collect::<Vec<_>>()
                .join(" "),
        }
    }
}

fn quote_argument(argument: &str) -> String {
    if !argument.is_empty()
        && argument
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || "_@%+=:,./-".contains(character))
    {
        argument.to_string()
    } else {
        format!("'{}'", argument.replace('\'', "'\"'\"'"))
    }
}
