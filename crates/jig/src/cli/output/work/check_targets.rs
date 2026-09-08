use serde_json::Value;

use super::{value_bool, value_str};

pub(super) struct TargetSummary<'a> {
    pub(super) target: &'a Value,
    pub(super) conclusion: &'a str,
    pub(super) receipt: Option<&'a str>,
}

pub(super) fn target_summaries(value: &Value) -> Vec<TargetSummary<'_>> {
    // Durable target results include blocked and cancelled targets which may
    // never have started and therefore have no compatibility response.
    if let Some(targets) = value["run"]["targets"]
        .as_array()
        .filter(|targets| !targets.is_empty())
    {
        return targets
            .iter()
            .map(|result| TargetSummary {
                target: &result["target"],
                conclusion: value_str(result, "conclusion").unwrap_or("unknown"),
                receipt: value_str(result, "receipt_id"),
            })
            .collect();
    }
    value["results"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|result| {
            let response = &result["response"];
            let conclusion = match (
                value_bool(response, "ok"),
                response["result"]["exit_status"].as_i64(),
            ) {
                (Some(false), _) => "failure",
                (_, Some(0)) => "success",
                (_, Some(_)) => "failure",
                (_, None) => "unknown",
            };
            TargetSummary {
                target: &result["target"],
                conclusion,
                receipt: value_str(response, "receipt_id"),
            }
        })
        .collect()
}

pub(super) fn append_target_summary(
    lines: &mut Vec<String>,
    value: &Value,
    targets: &[TargetSummary<'_>],
) {
    if let Some(run) = value.get("run").filter(|run| run.is_object()) {
        lines.push(format!(
            "  Run: {} ({})",
            value_str(run, "run_id").unwrap_or("<unknown>"),
            value_str(run, "conclusion").unwrap_or("unknown")
        ));
    }
    if !targets.is_empty() {
        lines.push(format!("  Targets: {}", targets.len()));
    }
    for target in targets {
        let component = value_str(target.target, "component").unwrap_or("<unknown>");
        let action = value_str(target.target, "action").unwrap_or("<unknown>");
        lines.push(format!(
            "  - {component}:{action}: {}, receipt {}",
            target.conclusion,
            target.receipt.unwrap_or("none")
        ));
    }
}
