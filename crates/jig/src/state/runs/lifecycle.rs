//! Authoritative structural validation for append-only run lifecycles.
//!
//! Archiving and diagnostics both feed decoded records through this boundary,
//! so a lifecycle cannot be considered healthy by diagnosis when archival
//! validation would reject it.

use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Result, anyhow, bail};
use jig_contract::{RunStatus, TargetId};

use super::{
    EVENT_CANCEL_REQUESTED, EVENT_COMPLETED, EVENT_QUEUED, EVENT_RUNNING, EVENT_TARGET_COMPLETED,
    EVENT_TARGET_STARTED, RunEventRecord, validate_run_id_for_lease, validate_run_plan_structure,
};

pub(in crate::state) fn is_recognized_run_event(event: &str) -> bool {
    matches!(
        event,
        EVENT_QUEUED
            | EVENT_RUNNING
            | EVENT_TARGET_STARTED
            | EVENT_TARGET_COMPLETED
            | EVENT_COMPLETED
            | EVENT_CANCEL_REQUESTED
    )
}

#[derive(Debug, Default)]
pub(in crate::state) struct RunLifecycleValidator {
    event_count: usize,
    known_event_count: usize,
    queued: bool,
    work_plan_id: Option<String>,
    planned_targets: BTreeSet<TargetId>,
    completed_targets: BTreeSet<TargetId>,
    completed_at_ms: Option<u64>,
}

/// Whole-stream lifecycle validation shared by archive and restore preflight
/// consumers. Callers may feed records incrementally so compressed diagnostic
/// scans retain their aggregate byte bound.
#[derive(Debug, Default)]
pub(in crate::state) struct RunStreamValidator {
    lifecycles: BTreeMap<String, RunLifecycleValidator>,
}

impl RunStreamValidator {
    pub(in crate::state) fn observe(&mut self, event: &RunEventRecord) -> Result<()> {
        self.lifecycles
            .entry(event.run_id.clone())
            .or_default()
            .observe(event)
    }

    pub(in crate::state) fn finish(self) -> Result<BTreeMap<String, RunLifecycleValidator>> {
        for (run_id, lifecycle) in &self.lifecycles {
            if lifecycle.known_event_count() > 0 && !lifecycle.queued() {
                bail!("run '{run_id}' has no queued event");
            }
        }
        Ok(self.lifecycles)
    }
}

impl RunLifecycleValidator {
    pub(in crate::state) fn observe(&mut self, event: &RunEventRecord) -> Result<()> {
        let known = is_recognized_run_event(&event.event);
        if known {
            validate_run_id_for_lease(&event.run_id)?;
        }
        if event.event == EVENT_QUEUED {
            if self.queued || self.known_event_count > 0 {
                bail!(
                    "run '{}' has more than one or a late queued event",
                    event.run_id
                );
            }
            let plan = event
                .plan
                .as_ref()
                .ok_or_else(|| anyhow!("run '{}' queued event has no plan", event.run_id))?;
            if let Err(error) = validate_run_plan_structure(plan) {
                bail!(
                    "run '{}' queued event has an invalid plan: {error:#}",
                    event.run_id
                );
            }
            self.planned_targets = plan
                .targets
                .iter()
                .map(|target| target.target.clone())
                .collect();
            self.queued = true;
            self.work_plan_id.clone_from(&event.work_plan_id);
        } else if known && !self.queued {
            bail!(
                "run '{}' has a {} event before queued",
                event.run_id,
                event.event
            );
        }
        match event.event.as_str() {
            EVENT_TARGET_STARTED => {
                let target = event.target.as_ref().ok_or_else(|| {
                    anyhow!("run '{}' target_started event has no target", event.run_id)
                })?;
                if !self.planned_targets.contains(target) {
                    bail!(
                        "run '{}' references unplanned target '{target}'",
                        event.run_id
                    );
                }
                if self.completed_targets.contains(target) {
                    bail!(
                        "run '{}' target '{target}' started after completion",
                        event.run_id
                    );
                }
            }
            EVENT_TARGET_COMPLETED => {
                let result = event.result.as_ref().ok_or_else(|| {
                    anyhow!(
                        "run '{}' target_completed event has no result",
                        event.run_id
                    )
                })?;
                if event.target.as_ref() != Some(&result.target) {
                    bail!(
                        "run '{}' target_completed identity does not match its result",
                        event.run_id
                    );
                }
                if !self.planned_targets.contains(&result.target) {
                    bail!(
                        "run '{}' references unplanned target '{}'",
                        event.run_id,
                        result.target
                    );
                }
                if result.status != RunStatus::Completed || result.conclusion.is_none() {
                    bail!(
                        "run '{}' target '{}' has a nonterminal result",
                        event.run_id,
                        result.target
                    );
                }
                if !self.completed_targets.insert(result.target.clone()) {
                    bail!(
                        "run '{}' target '{}' completed more than once",
                        event.run_id,
                        result.target
                    );
                }
            }
            EVENT_COMPLETED => {
                if self.completed_targets != self.planned_targets {
                    bail!(
                        "run '{}' completed before every target reached a conclusion",
                        event.run_id
                    );
                }
                if event.conclusion.is_none() {
                    bail!("run '{}' completed event has no conclusion", event.run_id);
                }
                if self.completed_at_ms.replace(event.timestamp_ms).is_some() {
                    bail!("run '{}' has more than one completed event", event.run_id);
                }
                self.planned_targets.clear();
                self.completed_targets.clear();
            }
            _ => {}
        }
        if self.completed_at_ms.is_some()
            && known
            && !matches!(
                event.event.as_str(),
                EVENT_COMPLETED | EVENT_CANCEL_REQUESTED
            )
        {
            bail!(
                "run '{}' has a {} event after completion",
                event.run_id,
                event.event
            );
        }
        if known {
            self.known_event_count = self.known_event_count.saturating_add(1);
        }
        self.event_count = self.event_count.saturating_add(1);
        Ok(())
    }

    pub(in crate::state) fn event_count(&self) -> usize {
        self.event_count
    }

    pub(in crate::state) fn known_event_count(&self) -> usize {
        self.known_event_count
    }

    pub(in crate::state) fn queued(&self) -> bool {
        self.queued
    }

    pub(in crate::state) fn completed(&self) -> bool {
        self.completed_at_ms.is_some()
    }

    pub(in crate::state) fn completed_at_ms(&self) -> Option<u64> {
        self.completed_at_ms
    }

    pub(super) fn work_plan_id(&self) -> Option<&str> {
        self.work_plan_id.as_deref()
    }
}

#[cfg(test)]
mod tests {
    use jig_contract::{
        ActionIntent, ActionRunner, PlannedTarget, RunPlan, SourceIdentity, TargetId,
    };

    use super::*;

    fn target(id: &str) -> TargetId {
        id.parse().unwrap()
    }

    fn plan() -> RunPlan {
        let first = target("api:prepare");
        let second = target("api:test");
        let mut second_target = PlannedTarget::new(
            second.clone(),
            ActionIntent::Check,
            ActionRunner::command("test"),
            "sha256:test-input",
        );
        second_target.depends_on.push(first.clone());
        RunPlan::new(
            "run-plan_example",
            "sha256:config",
            SourceIdentity::new(Some("abc".into()), "sha256:worktree"),
            vec![
                PlannedTarget::new(
                    first.clone(),
                    ActionIntent::Check,
                    ActionRunner::command("prepare"),
                    "sha256:prepare-input",
                ),
                second_target,
            ],
            vec![vec![first], vec![second]],
        )
    }

    fn queued_event(plan: RunPlan) -> RunEventRecord {
        RunEventRecord {
            id: "run_event_example".into(),
            run_id: "run_01ARZ3NDEKTSV4RRFFQ69G5FAV".into(),
            event: EVENT_QUEUED.into(),
            timestamp_ms: 1,
            work_plan_id: None,
            plan: Some(plan),
            target: None,
            result: None,
            conclusion: None,
        }
    }

    #[test]
    fn queued_lifecycle_uses_the_runtime_plan_structure_contract() {
        type PlanValidationCase = (&'static str, fn(&mut RunPlan), &'static str);

        let cases: [PlanValidationCase; 7] = [
            (
                "duplicate planned target",
                |plan| plan.targets.push(plan.targets[0].clone()),
                "contains duplicate targets",
            ),
            (
                "empty layer",
                |plan| plan.execution_layers.push(Vec::new()),
                "execution layer 2 is empty",
            ),
            (
                "unknown layer target",
                |plan| plan.execution_layers[0].push(target("api:unknown")),
                "reference unknown target 'api:unknown'",
            ),
            (
                "duplicate layer target",
                |plan| plan.execution_layers[1].push(target("api:prepare")),
                "contain duplicate target 'api:prepare'",
            ),
            (
                "omitted target",
                |plan| {
                    plan.execution_layers.pop();
                },
                "omit planned target(s): api:test",
            ),
            (
                "missing dependency",
                |plan| plan.targets[1].depends_on.push(target("api:unknown")),
                "depends on missing target 'api:unknown'",
            ),
            (
                "dependency in a later layer",
                |plan| plan.targets[0].depends_on.push(target("api:test")),
                "must execute after dependency 'api:test'",
            ),
        ];

        for (name, invalidate, expected) in cases {
            let mut invalid = plan();
            invalidate(&mut invalid);
            let error = RunLifecycleValidator::default()
                .observe(&queued_event(invalid))
                .expect_err(name);
            assert!(format!("{error:#}").contains(expected), "{name}: {error:#}");
        }
    }
}
