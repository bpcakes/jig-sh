use super::*;
use crate::context::RepoContext;
use crate::repository::RepositoryCatalog;
use anyhow::{Result, ensure};

#[derive(Debug, Default)]
pub(crate) struct Request {
    pub(crate) targets: Vec<TargetId>,
    pub(crate) assert_worktree: bool,
    pub(crate) assert_exhaustive: bool,
    pub(crate) inputs: Vec<String>,
    pub(crate) patch: bool,
}

#[cfg(test)]
pub(crate) fn report(ctx: &RepoContext) -> Result<serde_json::Value> {
    preview(ctx, &Request::default())
}

pub(crate) fn preview(ctx: &RepoContext, request: &Request) -> Result<serde_json::Value> {
    let assertion = request.assert_worktree || request.assert_exhaustive;
    ensure!(
        !assertion || !request.targets.is_empty(),
        "ownership assertions require at least one explicit --target component:action"
    );
    ensure!(
        request.inputs.is_empty() || request.assert_exhaustive,
        "--input requires --assert-exhaustive"
    );
    if assertion || request.patch {
        ensure!(
            ctx.contract_version() >= jig_contract::freshness::WORKTREE_FRESHNESS_CONTRACT_VERSION,
            "freshness adoption requires contract version 8 or later; update the harness first"
        );
    }
    let catalog = RepositoryCatalog::from_context(ctx)?;
    for target in &request.targets {
        ensure!(
            catalog.action(target).is_some(),
            "unknown target '{target}'; use info targets to list exact component:action addresses"
        );
    }
    let mut recommendations = Vec::new();
    let mut changes = Vec::new();
    for resolved in catalog.actions() {
        if !request.targets.is_empty() && !request.targets.contains(&resolved.target) {
            continue;
        }
        let action = ctx
            .authored_action_specs()
            .and_then(|actions| {
                actions
                    .iter()
                    .find(|source| source.target == resolved.target)
            })
            .unwrap_or(resolved);
        let command = match &action.runner {
            ActionRunner::Command { command, .. } | ActionRunner::Shell { command, .. } => {
                Some(ctx.command_for_key(command)?)
            }
            _ => None,
        };
        let mut recommendation = recommend(ctx.contract_version(), action, command);
        let mut proposed = action.clone();
        if recommendation.current.source_state != recommendation.proposed.source_state {
            proposed.source_state = Some(recommendation.proposed.source_state);
            proposed
                .provenance
                .insert("source_state".into(), FieldProvenance::Declared);
        }
        if assertion {
            ensure!(
                is_read_only_check(action) && !matches!(action.runner, ActionRunner::Native { .. }),
                "target '{}' does not support owner freshness assertions: select a read-only command check; native actions retain their comparison authority",
                action.target
            );
            recommendation.reason = Reason::OwnerAssertion;
        }
        if request.assert_worktree {
            proposed.source_state = Some(ActionSourceState::Worktree);
            proposed
                .provenance
                .insert("source_state".into(), FieldProvenance::Declared);
        }
        if request.assert_exhaustive {
            proposed.inputs_policy = Some(ActionInputsPolicy::Exhaustive);
            proposed
                .provenance
                .insert("inputs_policy".into(), FieldProvenance::Declared);
            proposed
                .provenance
                .insert("inputs".into(), FieldProvenance::Declared);
            for input in &request.inputs {
                if !proposed.inputs.contains(input) {
                    proposed.inputs.push(input.clone());
                }
            }
        }
        super::super::validate_inputs_policy(ctx.contract_version(), &proposed)?;
        for input in &proposed.inputs {
            crate::repository::affected::compile_input(&proposed.target, input)?;
        }
        recommendation.proposed = Policy::of(&proposed);
        recommendation.proposed_inputs = proposed.inputs.clone();
        recommendation.exhaustive_requires_owner_assertion =
            proposed.inputs_policy != Some(ActionInputsPolicy::Exhaustive);
        if proposed != *action {
            changes.push((action.clone(), proposed));
        }
        recommendations.push(recommendation);
    }
    // Also checks that the context and files still describe one authority snapshot.
    let patch = super::patch::prepare(ctx, &changes, request.patch)?;
    let mut output = serde_json::json!({
        "ok": true, "command": "info freshness", "schema_version": 1,
        "contract_version": ctx.contract_version(), "targets": recommendations,
        "changed_targets": changes.iter().map(|(_, action)| &action.target).collect::<Vec<_>>(),
        "input_ownership": "Exhaustive inputs require owner review of every repository file the action reads, including nonstandard source paths, configuration, fixtures and toolchain pins. Command recognition alone does not establish completeness.",
        "source_ownership": "Worktree freshness asserts that staging, commits and branch placement cannot change the command result. Explicit assertions replace selected policies; they do not attest installed tools, ambient environment or live services.",
        "next_step": "Preview only. Use --patch to print a paired patch, review it, then apply with git apply. Regenerate the preview after concurrent edits."
    });
    if request.patch {
        output["patch"] = serde_json::json!(patch);
    }
    Ok(output)
}

pub(crate) fn format_report(value: &serde_json::Value) -> String {
    let mut lines = vec![format!(
        "Jig freshness (contract v{})",
        value["contract_version"]
    )];
    if let Some(targets) = value["targets"].as_array() {
        for target in targets {
            lines.push(format!(
                "  {}:{}: {} / {} -> {} / {} ({})",
                target["target"]["component"].as_str().unwrap_or("?"),
                target["target"]["action"].as_str().unwrap_or("?"),
                target["current"]["source_state"].as_str().unwrap_or("?"),
                target["current"]["inputs_policy"].as_str().unwrap_or("?"),
                target["proposed"]["source_state"].as_str().unwrap_or("?"),
                target["proposed"]["inputs_policy"].as_str().unwrap_or("?"),
                target["reason"].as_str().unwrap_or("?")
            ));
            if matches!(
                target["reason"].as_str(),
                Some("known_formatter" | "owner_assertion")
            ) {
                let inputs = target["proposed_inputs"]
                    .as_array()
                    .map(|values| {
                        values
                            .iter()
                            .filter_map(serde_json::Value::as_str)
                            .collect::<Vec<_>>()
                            .join(", ")
                    })
                    .unwrap_or_default();
                lines.push(format!("    Proposed inputs: {inputs}"));
            }
        }
    }
    lines.push(value["input_ownership"].as_str().unwrap_or_default().into());
    lines.push(
        value["source_ownership"]
            .as_str()
            .unwrap_or_default()
            .into(),
    );
    lines.push(value["next_step"].as_str().unwrap_or_default().into());
    lines.join("\n")
}
