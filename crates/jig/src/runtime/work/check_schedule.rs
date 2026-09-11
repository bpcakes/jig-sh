use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Result, anyhow};
use jig_contract::TargetId;

use crate::repository::RepositoryCatalog;

/// Shared by read-only recovery previews and actual work-check execution.
pub(super) fn schedule(
    catalog: &RepositoryCatalog,
    required: &BTreeSet<TargetId>,
    passing: &BTreeSet<TargetId>,
) -> Result<BTreeSet<TargetId>> {
    let mut dependencies = BTreeMap::new();
    let mut pending: Vec<_> = required.iter().cloned().collect();
    while let Some(target) = pending.pop() {
        if dependencies.contains_key(&target) {
            continue;
        }
        let action = catalog
            .action(&target)
            .ok_or_else(|| anyhow!("unknown target {target}"))?;
        let prerequisites: BTreeSet<_> = action.depends_on.iter().cloned().collect();
        pending.extend(prerequisites.iter().cloned());
        dependencies.insert(target, prerequisites);
    }
    Ok(schedule_graph(&dependencies, required, passing))
}

fn schedule_graph(
    dependencies: &BTreeMap<TargetId, BTreeSet<TargetId>>,
    required: &BTreeSet<TargetId>,
    passing: &BTreeSet<TargetId>,
) -> BTreeSet<TargetId> {
    let mut scheduled: BTreeSet<_> = required.difference(passing).cloned().collect();
    // The planner executes prerequisites. Re-execute their required dependents
    // too, so a newer dependency cannot leave an older dependent proof behind.
    // Epoch-nine gates select roots, so traverse hidden prerequisites as well.
    loop {
        let mut expanded = scheduled.clone();
        for (target, prerequisites) in dependencies {
            if scheduled.contains(target) {
                expanded.extend(prerequisites.iter().cloned());
            } else if !prerequisites.is_disjoint(&scheduled) {
                expanded.insert(target.clone());
            }
        }
        if expanded == scheduled {
            return scheduled;
        }
        scheduled = expanded;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn targets(names: &[&str]) -> BTreeSet<TargetId> {
        names.iter().map(|name| name.parse().unwrap()).collect()
    }

    #[test]
    fn independent_passes_reuse_and_shared_prerequisites_propagate_transitively() {
        let graph = BTreeMap::from([
            ("api:prepare".parse().unwrap(), targets(&[])),
            ("api:test".parse().unwrap(), targets(&["api:prepare"])),
            ("api:smoke".parse().unwrap(), targets(&["api:test"])),
            ("repo:policy".parse().unwrap(), targets(&[])),
        ]);
        let all = graph.keys().cloned().collect();
        assert!(schedule_graph(&graph, &all, &all).is_empty());
        assert_eq!(
            schedule_graph(
                &graph,
                &all,
                &targets(&["api:prepare", "api:test", "api:smoke"])
            ),
            targets(&["repo:policy"])
        );
        assert_eq!(
            schedule_graph(
                &graph,
                &all,
                &targets(&["api:prepare", "api:smoke", "repo:policy"])
            ),
            targets(&["api:prepare", "api:test", "api:smoke"])
        );
    }

    #[test]
    fn preview_includes_transitive_prerequisites_outside_gate_roots() {
        let graph = BTreeMap::from([
            ("api:prepare".parse().unwrap(), targets(&[])),
            ("api:compile".parse().unwrap(), targets(&["api:prepare"])),
            ("api:test".parse().unwrap(), targets(&["api:compile"])),
            (
                "api:smoke_compile".parse().unwrap(),
                targets(&["api:prepare"]),
            ),
            (
                "api:smoke".parse().unwrap(),
                targets(&["api:smoke_compile"]),
            ),
            ("repo:policy".parse().unwrap(), targets(&[])),
        ]);
        let required = targets(&["api:test", "api:smoke", "repo:policy"]);
        assert!(schedule_graph(&graph, &required, &required).is_empty());
        assert_eq!(
            schedule_graph(&graph, &required, &targets(&["api:smoke", "repo:policy"])),
            targets(&[
                "api:prepare",
                "api:compile",
                "api:test",
                "api:smoke_compile",
                "api:smoke"
            ])
        );
    }
}
