use std::collections::BTreeSet;

use super::*;

pub(super) struct ReadyQueue<'a> {
    pub(super) targets: Vec<(&'a PlannedTarget, PhasePosition)>,
    pub(super) ready: BTreeSet<usize>,
    pub(super) failed_dependency: Vec<bool>,
    pub(super) unfinished: usize,
    dependents: Vec<Vec<usize>>,
    remaining_dependencies: Vec<usize>,
}

impl<'a> ReadyQueue<'a> {
    pub(super) fn new(plan: &'a RunPlan) -> Result<Self> {
        let targets = plan
            .execution_layers
            .iter()
            .flatten()
            .enumerate()
            .map(|(index, target)| {
                Ok((
                    planned_target(plan, target)?,
                    PhasePosition::new(index + 1, plan.targets.len())
                        .expect("planned position is valid"),
                ))
            })
            .collect::<Result<Vec<_>>>()?;
        let indices = targets
            .iter()
            .enumerate()
            .map(|(index, (planned, _))| (&planned.target, index))
            .collect::<BTreeMap<_, _>>();
        let mut dependents = vec![Vec::new(); targets.len()];
        let mut remaining_dependencies = vec![0; targets.len()];
        let mut ready = BTreeSet::new();
        for (index, (planned, _)) in targets.iter().enumerate() {
            for dependency in &planned.depends_on {
                let parent = indices.get(dependency).ok_or_else(|| {
                    anyhow::anyhow!("ready target has an unplanned dependency '{dependency}'")
                })?;
                dependents[*parent].push(index);
                remaining_dependencies[index] += 1;
            }
            if remaining_dependencies[index] == 0 {
                ready.insert(index);
            }
        }
        Ok(Self {
            failed_dependency: vec![false; targets.len()],
            unfinished: targets.len(),
            targets,
            ready,
            dependents,
            remaining_dependencies,
        })
    }

    pub(super) fn publish(
        &mut self,
        index: usize,
        result: TargetRunResult,
        compatibility: Option<Value>,
        publish: &mut Publish<'_>,
    ) -> Result<()> {
        let conclusion = result
            .conclusion
            .expect("completed target has a conclusion");
        publish(&self.targets[index].0.target, result, compatibility)?;
        self.ready.remove(&index);
        self.unfinished -= 1;
        for &child in &self.dependents[index] {
            self.failed_dependency[child] |= conclusion != RunConclusion::Success;
            self.remaining_dependencies[child] -= 1;
            if self.remaining_dependencies[child] == 0 {
                self.ready.insert(child);
            }
        }
        Ok(())
    }

    pub(super) fn finish_unstarted(
        &mut self,
        index: usize,
        finisher: &TargetFinisher<'_>,
        conclusion: RunConclusion,
        reason: String,
        publish: &mut Publish<'_>,
    ) -> Result<()> {
        let planned = self.targets[index].0;
        let capture = TargetCapture::not_started(conclusion, reason).with_alias(
            finisher
                .catalog
                .aliases_for_target(&planned.target)
                .first()
                .cloned(),
        );
        let (result, compatibility) = finisher.finish(
            planned,
            CompletedTargetCapture::now(None, capture),
            Err(format!("target '{}' did not start", planned.target)),
        )?;
        self.publish(index, result, compatibility, publish)
    }
}
