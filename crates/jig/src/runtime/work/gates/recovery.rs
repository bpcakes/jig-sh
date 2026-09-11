use jig_contract::freshness::FreshnessCollectionLimit;
pub(super) use jig_contract::recovery::GateRecovery as Recovery;
use jig_contract::recovery::{RecoveryCommand, TargetRecovery};

use super::*;

fn command(args: &[&str], plan_id: &str, read_only: bool) -> RecoveryCommand {
    let mut argv = vec!["scripts/jig".into()];
    argv.extend(args.iter().map(|arg| (*arg).to_owned()));
    argv.extend(["--plan-id".into(), plan_id.into()]);
    RecoveryCommand { argv, read_only }
}

pub(super) fn from_report(
    report: &GateReport,
    catalog: Option<&RepositoryCatalog>,
) -> Option<Recovery> {
    let catalog = catalog?;
    let mut required = BTreeSet::new();
    let mut passing = BTreeSet::new();
    let mut deadline = None;
    let mut resource = false;
    let mut unavailable = report.current_worktree_fingerprint_error.is_some();
    for gate in &report.gates {
        let GateEvaluation::Evidence(gate) = gate else {
            continue;
        };
        if !gate.required() {
            continue;
        }
        if let Some(stats) = gate.collection_stats() {
            match stats.limit {
                Some(FreshnessCollectionLimit::Deadline) => deadline = Some(stats.timeout_ms),
                Some(FreshnessCollectionLimit::Resource) => resource = true,
                None => {}
            }
        }
        for (target, passed, value) in gate.check_targets() {
            required.insert(target.clone());
            if passed {
                passing.insert(target);
            }
            unavailable |= value["freshness"] == "unsupported"
                || value["freshness_reasons"]
                    .as_array()
                    .is_some_and(|reasons| {
                        reasons.iter().any(|reason| {
                            matches!(
                                reason["code"].as_str(),
                                Some(
                                    "collection_failed"
                                        | "collection_limit"
                                        | "source_raced"
                                        | "unobservable_input"
                                        | "unsupported_authority"
                                        | "unsupported_reference"
                                )
                            )
                        })
                    });
        }
    }
    if required.is_empty() {
        return None;
    }
    let mut recovery = Recovery {
            scope: "required_native_targets".into(),
            inspection: "complete".into(),
            preview_available: false,
            execute: Vec::new(),
            reuse: Vec::new(),
            targets: Vec::new(),
            next_step: None,
            message: String::new(),
            legacy_tool_note: "work check --tool records legacy tool evidence; it cannot satisfy a native target gate. Native refresh commands use --plan-id to retain the plan's comparison authority.".into(),
        };
    if resource || deadline.is_some() || unavailable {
        recovery.inspection = if resource {
            "resource_exhausted"
        } else if deadline.is_some() {
            "deadline_exhausted"
        } else {
            "unavailable"
        }
        .into();
        recovery.message = match (resource, deadline) {
                (true, _) => "Inspection exhausted a resource ceiling; a larger timeout does not raise entry, byte, graph, depth, or record limits. Resolve the collection limit before choosing checks to execute.".into(),
                (false, Some(timeout)) if timeout < RECORDING_TIMEOUT_MS => {
                    let mut command = command(&["work", "gates"], &report.plan_id, true);
                    command.argv.extend(["--freshness-timeout-ms".into(), RECORDING_TIMEOUT_MS.to_string()]);
                    recovery.next_step = Some(command);
                    format!("Inspection exceeded its {timeout} ms budget; this does not establish that evidence is stale. Retry read-only inspection with a larger budget before executing checks.")
                }
                (false, Some(timeout)) => format!("Inspection exceeded its {timeout} ms budget at the maximum timeout. Resolve the observation limit before choosing checks to execute."),
                _ => "Inspection could not establish current target evidence. Resolve the reported observation or compatibility problem before choosing checks to execute.".into(),
            };
        return Some(recovery);
    }
    let scheduled = match super::super::check_schedule::schedule(catalog, &required, &passing) {
        Ok(scheduled) => scheduled,
        Err(_) => {
            recovery.inspection = "unavailable".into();
            recovery.message = "The configured target graph could not be prepared; inspect repository configuration before executing checks.".into();
            return Some(recovery);
        }
    };
    recovery.preview_available = true;
    recovery.execute = scheduled.iter().cloned().collect();
    recovery.reuse = required.difference(&scheduled).cloned().collect();
    recovery.targets = required
        .iter()
        .map(|target| {
            let execute = scheduled.contains(target);
            TargetRecovery {
                target: target.clone(),
                disposition: if execute { "execute" } else { "reuse" }.into(),
                reason: if !execute {
                    "current_pass"
                } else if passing.contains(target) {
                    "dependency_execution"
                } else {
                    "evidence_not_current_and_passing"
                }
                .into(),
                refresh: (execute && report.plan_state == "open")
                    .then(|| command(&["check", &target.to_string()], &report.plan_id, false)),
            }
        })
        .collect();
    if report.plan_state == "open" && !scheduled.is_empty() {
        recovery.next_step = Some(command(&["work", "check"], &report.plan_id, false));
    }
    recovery.message = "Preview covers the required native target phase of work check. Legacy check and review gates are reported separately. Execution revalidates current evidence; source changes can change this preview. A forced target refresh also runs its prerequisites and may invalidate dependent passes.".into();
    Some(recovery)
}
