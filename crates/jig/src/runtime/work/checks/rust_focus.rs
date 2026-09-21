use std::collections::BTreeMap;

use anyhow::{Result, bail};
use jig_contract::{ActionArguments, RustFocusV1, TargetId};

use crate::command::{WorkCheckPhase, WorkCheckRequest};

pub(super) fn prepare_arguments(
    ctx: &crate::context::RepoContext,
    catalog: &crate::repository::RepositoryCatalog,
    opts: &WorkCheckRequest,
) -> Result<(BTreeMap<TargetId, ActionArguments>, Vec<serde_json::Value>)> {
    let mut arguments = arguments(opts)?;
    let mut fallbacks = Vec::new();
    if arguments.is_empty() {
        return Ok((arguments, fallbacks));
    }
    let profile = ctx
        .work_iteration_profile()
        .and_then(|id| catalog.profile(id))
        .ok_or_else(|| anyhow::anyhow!("Rust focus requires a configured iteration profile"))?;
    let mut pending = profile.targets.clone();
    let mut selected = std::collections::BTreeSet::new();
    while let Some(target) = pending.pop() {
        if selected.insert(target.clone())
            && let Some(action) = catalog.action(&target)
        {
            pending.extend(action.depends_on.clone());
        }
    }
    for (target, focus) in &opts.rust_focus {
        if !selected.contains(target) {
            bail!("Rust focus target {target} is not selected by the iteration profile");
        }
        let action = catalog
            .action(target)
            .ok_or_else(|| anyhow::anyhow!("Unknown Rust focus target {target}"))?;
        if !matches!(
            &action.runner,
            jig_contract::ActionRunner::RustNextestV1 { .. }
        ) {
            if matches!(focus, RustFocusV1::Explicit { .. }) {
                bail!(
                    "Explicit Rust focus is unsupported for {target}; run the configured full check without --rust-focus"
                );
            }
            arguments.remove(target);
            fallbacks.push(serde_json::json!({"target":target,"reason":"unsupported_runner","scope":"configured_default"}));
        }
    }
    Ok((arguments, fallbacks))
}

pub(super) fn arguments(opts: &WorkCheckRequest) -> Result<BTreeMap<TargetId, ActionArguments>> {
    anyhow::ensure!(
        opts.rust_focus.len() <= 32,
        "at most 32 Rust focus targets are supported"
    );
    if !opts.rust_focus.is_empty() && opts.phase != Some(WorkCheckPhase::Iteration) {
        bail!("Rust focus requires --phase iteration; full/final work checks cannot be narrowed");
    }
    opts.rust_focus
        .iter()
        .map(|(target, focus)| {
            let mut focus = focus.clone();
            jig_rust::rust_focus::normalize_focus(&mut focus).map_err(anyhow::Error::msg)?;
            if let RustFocusV1::Automatic { plan_id } = &mut focus {
                if plan_id.as_ref().is_some_and(|id| id != &opts.plan_id) {
                    bail!(
                        "Automatic Rust focus for {target} must use the owning work plan {}",
                        opts.plan_id
                    );
                }
                *plan_id = Some(opts.plan_id.clone());
            }
            let encoded = serde_json::to_string(&focus)?;
            anyhow::ensure!(encoded.len() <= 65536, "Rust focus exceeds 65536 bytes");
            Ok((target.clone(), BTreeMap::from([("focus".into(), encoded)])))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;
    use serde_json::json;

    #[derive(Parser)]
    struct WorkCheckCli {
        #[command(flatten)]
        opts: crate::cli::WorkCheckOpts,
    }

    fn cli_request(focus: &str) -> WorkCheckRequest {
        let cli = WorkCheckCli::try_parse_from([
            "work-check",
            "--plan-id",
            "plan_1",
            "--phase",
            "iteration",
            "--rust-focus",
            focus,
            "--explain",
        ])
        .unwrap();
        cli.opts.try_into().unwrap()
    }

    #[test]
    fn cli_and_mcp_focus_have_identical_scope_and_plan_binding() {
        for focus in [
            json!({"kind":"automatic"}),
            json!({"kind":"explicit", "packages":["example-api@0.1.0"],
                "targets":[{"kind":"lib"}], "features":{"features":["other","example","other"]},
                "filter":"test(example)"}),
        ] {
            let cli = cli_request(&format!("api:test={focus}"));
            let mcp: WorkCheckRequest = serde_json::from_value(json!({
                "plan_id":"plan_1", "phase":"iteration", "explain":true,
                "rust_focus":{"api:test":focus},
            }))
            .unwrap();
            assert_eq!(arguments(&cli).unwrap(), arguments(&mcp).unwrap());
            if focus["kind"] == "automatic" {
                let args = arguments(&cli).unwrap();
                let value: serde_json::Value =
                    serde_json::from_str(&args[&"api:test".parse().unwrap()]["focus"]).unwrap();
                assert_eq!(value["plan_id"], "plan_1");
            }
        }
    }

    #[test]
    fn focus_rejects_final_scope_and_foreign_plan_without_planning() {
        let mut request = cli_request(r#"api:test={"kind":"automatic","plan_id":"plan_other"}"#);
        assert!(
            arguments(&request)
                .unwrap_err()
                .to_string()
                .contains("owning work plan")
        );
        request.rust_focus.insert(
            "api:test".parse().unwrap(),
            RustFocusV1::Automatic { plan_id: None },
        );
        for phase in [None, Some(WorkCheckPhase::Final)] {
            request.phase = phase;
            assert!(
                arguments(&request)
                    .unwrap_err()
                    .to_string()
                    .contains("cannot be narrowed")
            );
        }
    }

    #[test]
    fn repeated_cli_target_is_rejected_instead_of_overwriting_scope() {
        let cli = WorkCheckCli::try_parse_from([
            "work-check",
            "--plan-id",
            "plan_1",
            "--phase",
            "iteration",
            "--rust-focus",
            r#"api:test={"kind":"automatic"}"#,
            "--rust-focus",
            r#"api:test={"kind":"explicit","packages":["example-api@0.1.0"]}"#,
        ])
        .unwrap();
        assert!(WorkCheckRequest::try_from(cli.opts).is_err());
    }

    #[test]
    fn typed_mcp_map_has_the_same_target_bound_as_cli() {
        let mut request: WorkCheckRequest = serde_json::from_value(json!({
            "plan_id":"plan_1", "phase":"iteration"
        }))
        .unwrap();
        for index in 0..33 {
            request.rust_focus.insert(
                format!("api:test-{index}").parse().unwrap(),
                RustFocusV1::Automatic { plan_id: None },
            );
        }
        assert!(
            arguments(&request)
                .unwrap_err()
                .to_string()
                .contains("at most 32")
        );
    }
}
