//! One preparation boundary for typed Rust execution; raw Cargo IDs stay private.
use std::{collections::BTreeMap, path::Path};

use anyhow::{Context, Result, bail, ensure};
use jig_contract::{
    ActionRunner, CargoImpactDispositionV1, PreparedRustInputV1, RunPlan, RustFocusV1,
    RustNextestConfigV1, RustScopeDispositionV1, TargetId,
};

use crate::{
    context::RepoContext,
    execution::{ExecutionCancellation, ExecutionObserver},
};

pub(crate) fn parse_cli(values: Vec<String>) -> Result<BTreeMap<TargetId, RustFocusV1>> {
    ensure!(
        values.len() <= 32,
        "at most 32 Rust focus targets are supported"
    );
    let mut result = BTreeMap::new();
    for value in values {
        let (target, value) = value
            .split_once('=')
            .context("--rust-focus requires TARGET=JSON")?;
        ensure!(value.len() <= 65536, "Rust focus exceeds 65536 bytes");
        let target: TargetId = target.parse()?;
        let mut focus: RustFocusV1 =
            serde_json::from_str(value).context("invalid typed Rust focus")?;
        jig_rust::rust_focus::normalize_focus(&mut focus).map_err(anyhow::Error::msg)?;
        ensure!(
            result.insert(target, focus).is_none(),
            "duplicate Rust focus target"
        );
    }
    Ok(result)
}

struct Control<'a>(&'a dyn Fn() -> bool);
impl ExecutionObserver for Control<'_> {}
impl ExecutionCancellation for Control<'_> {
    fn cancelled(&self) -> bool {
        (self.0)()
    }
}

pub(super) fn prepare_plan(
    ctx: &RepoContext,
    plan: &mut RunPlan,
    cancelled: Option<&dyn Fn() -> bool>,
) -> Result<bool> {
    let cancelled = cancelled.unwrap_or(&never_cancelled);
    ensure!(
        plan.targets
            .iter()
            .filter(
                |target| matches!(target.runner, ActionRunner::RustNextestV1 { .. })
                    && target.arguments.contains_key("focus")
            )
            .count()
            <= 32,
        "at most 32 focused Rust metadata acquisitions are supported per plan"
    );
    let mut prepared = false;
    for target in &mut plan.targets {
        if let ActionRunner::RustNextestV1 { configuration } = &target.runner {
            let focus = target
                .arguments
                .get("focus")
                .map(|value| serde_json::from_str(value))
                .transpose()?;
            target.prepared_rust_input = Some(prepare(
                ctx,
                &target.target,
                configuration,
                focus,
                cancelled,
            )?);
            prepared = true;
        }
    }
    Ok(prepared)
}

fn never_cancelled() -> bool {
    false
}

pub(crate) fn full_input(config: &RustNextestConfigV1) -> PreparedRustInputV1 {
    let mut context = config.context.clone();
    context.features.sort();
    context.features.dedup();
    PreparedRustInputV1 {
        schema_version: 1,
        disposition: RustScopeDispositionV1::Full,
        reasons: Vec::new(),
        packages: Vec::new(),
        targets: Vec::new(),
        comparison_base: None,
        args: jig_rust::rust_focus::nextest_args(config, &context, &[], &[], None),
        context,
    }
}

fn prepare(
    ctx: &RepoContext,
    target: &TargetId,
    config: &RustNextestConfigV1,
    mut focus: Option<RustFocusV1>,
    cancelled: &dyn Fn() -> bool,
) -> Result<PreparedRustInputV1> {
    jig_rust::rust_focus::validate_config(config).map_err(anyhow::Error::msg)?;
    if let Some(focus) = &mut focus {
        jig_rust::rust_focus::normalize_focus(focus).map_err(anyhow::Error::msg)?;
        ensure!(
            serde_json::to_vec(focus)?.len() <= 65536,
            "Rust focus exceeds 65536 bytes"
        );
    }
    ensure!(!cancelled(), "Rust focus planning was cancelled");
    ensure!(
        focus.is_none() || config.focused,
        "full Rust actions cannot be narrowed"
    );
    let mut prepared = full_input(config);
    let mut filter = None;
    if let Some(RustFocusV1::Explicit {
        features: Some(features),
        ..
    }) = &focus
    {
        prepared.context.features = features.features.clone();
        prepared.context.no_default_features = features.no_default_features;
        prepared.context.all_features = features.all_features;
    }
    prepared.context.features.sort();
    prepared.context.features.dedup();
    if let Some(focus) = &focus {
        let acquisition = super::cargo_discovery::acquire_cargo_metadata_in_context(
            ctx.root(),
            Path::new(&config.workspace_manifest),
            &prepared.context,
            &mut Control(cancelled),
        );
        match acquisition {
            Err(error) if error.is_cancellation() => {
                bail!("Rust focus metadata discovery was cancelled")
            }
            Err(error) => {
                if matches!(focus, RustFocusV1::Explicit { .. }) {
                    bail!(
                        "Rust focus package authority unavailable ({:?}); run the configured full check",
                        error.public_reason()
                    );
                }
                prepared.disposition = RustScopeDispositionV1::BroadFallback;
                prepared
                    .reasons
                    .push(format!("metadata_unavailable:{:?}", error.public_reason()));
            }
            Ok(acquired) => match focus {
                RustFocusV1::Explicit {
                    packages,
                    targets,
                    filter: requested_filter,
                    ..
                } => {
                    for package in packages {
                        ensure!(
                            acquired.graph.verifies_workspace_selector(package),
                            "Rust focus package selector is not an exact unambiguous workspace member; run the configured full check"
                        );
                    }
                    for selected in targets {
                        ensure!(
                            acquired
                                .graph
                                .packages()
                                .iter()
                                .filter(|p| packages.contains(&p.selector))
                                .flat_map(|p| &p.targets)
                                .any(|t| jig_rust::rust_focus::target_matches(
                                    selected, &t.name, &t.kinds
                                )),
                            "Rust focus target does not match a declared target in the selected packages"
                        );
                    }
                    prepared.disposition = RustScopeDispositionV1::Narrowed;
                    prepared.packages = packages.clone();
                    prepared.targets = targets.clone();
                    filter = requested_filter.as_deref();
                }
                RustFocusV1::Automatic { plan_id } => {
                    let comparison = automatic_paths(ctx, plan_id.as_deref(), cancelled)?;
                    match comparison {
                        Some((base, paths)) => {
                            prepared.comparison_base = Some(base);
                            let impact = jig_rust::select_cargo_impact_v1(
                                &acquired.graph,
                                target.component.clone(),
                                &config.workspace_manifest,
                                &paths,
                                prepared.context.clone(),
                            );
                            if impact.disposition == CargoImpactDispositionV1::Narrowed
                                && !impact.build_packages.is_empty()
                                && impact.build_packages.iter().all(|p| {
                                    acquired.graph.verifies_workspace_selector(&p.selector)
                                })
                            {
                                prepared.disposition = RustScopeDispositionV1::Narrowed;
                                prepared.packages = impact
                                    .build_packages
                                    .into_iter()
                                    .map(|p| p.selector)
                                    .collect();
                                // Metadata resolves workspace features before impact narrows
                                // packages. Retain that scope unless every named feature
                                // is explicitly owned by a package still selected. Bare
                                // feature names have workspace-dependent meaning which
                                // the normalized graph does not prove for a subset.
                                if !prepared.context.features.iter().all(|feature| {
                                    feature.split_once('/').is_some_and(|(owner, _)| {
                                        prepared.packages.iter().any(|package| {
                                            package
                                                .split_once('@')
                                                .is_some_and(|(name, _)| name == owner)
                                        })
                                    })
                                }) {
                                    prepared.disposition = RustScopeDispositionV1::BroadFallback;
                                    prepared.packages.clear();
                                    prepared
                                        .reasons
                                        .push("feature_context_requires_workspace".into());
                                }
                            } else {
                                prepared.disposition = RustScopeDispositionV1::BroadFallback;
                                prepared
                                    .reasons
                                    .push(format!("cargo_impact:{:?}", impact.disposition));
                                prepared.reasons.extend(
                                    impact.reasons.iter().map(|reason| format!("{reason:?}")),
                                );
                            }
                        }
                        None => {
                            prepared.disposition = RustScopeDispositionV1::BroadFallback;
                            prepared.reasons.push("comparison_unavailable".into());
                        }
                    }
                }
            },
        }
    }
    ensure!(!cancelled(), "Rust focus planning was cancelled");
    prepared.args = jig_rust::rust_focus::nextest_args(
        config,
        &prepared.context,
        &prepared.packages,
        &prepared.targets,
        filter,
    );
    Ok(prepared)
}

fn automatic_paths(
    ctx: &RepoContext,
    plan_id: Option<&str>,
    cancelled: &dyn Fn() -> bool,
) -> Result<Option<(String, Vec<String>)>> {
    let Some(plan_id) = plan_id else {
        return Ok(None);
    };
    crate::state::ensure_plan_is_open(ctx, plan_id)?;
    let baseline = crate::state::plan_baseline_with_cancellation(ctx, plan_id, cancelled)?;
    let Some(baseline) = baseline else {
        return Ok(None);
    };
    let comparison = if let Some(oid) = baseline.commit_oid {
        crate::git_receipts::plan_change_snapshot_with_cancellation(ctx.root(), &oid, cancelled)
            .map(|snapshot| (oid, snapshot.all_changed_paths()))
    } else if let Some(oid) = baseline.empty_tree_oid {
        crate::git_receipts::plan_change_snapshot_from_empty_tree_with_cancellation(
            ctx.root(),
            &oid,
            cancelled,
        )
        .map(|snapshot| (oid, snapshot.all_changed_paths()))
    } else {
        return Ok(None);
    };
    ensure!(!cancelled(), "Rust focus comparison was cancelled");
    Ok(comparison.ok())
}

#[cfg(test)]
mod feature_tests;
#[cfg(all(test, any(target_os = "linux", target_os = "macos")))]
mod platform_tests;
#[cfg(test)]
mod tests;
