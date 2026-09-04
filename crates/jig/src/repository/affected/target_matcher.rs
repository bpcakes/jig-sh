use super::*;

#[derive(Debug)]
pub(super) struct TargetInputMatcherV1 {
    by_target: BTreeMap<TargetId, Vec<GlobMatcher>>,
    components_with_inputs: BTreeSet<ComponentId>,
}

impl TargetInputMatcherV1 {
    pub(super) fn new(catalog: &RepositoryCatalog) -> Result<Self> {
        let mut by_target = BTreeMap::new();
        let mut components_with_inputs = BTreeSet::new();
        for action in catalog.actions() {
            if action.inputs.is_empty() {
                continue;
            }
            let inputs = action
                .inputs
                .iter()
                .map(|input| compile_input(&action.target, input))
                .collect::<Result<Vec<_>>>()?;
            components_with_inputs.insert(action.target.component.clone());
            by_target.insert(action.target.clone(), inputs);
        }
        Ok(Self {
            by_target,
            components_with_inputs,
        })
    }

    pub(super) fn matches_any(&self, path: &str) -> bool {
        self.by_target
            .values()
            .any(|inputs| inputs.iter().any(|input| input.is_match(path)))
    }

    pub(super) fn matching_targets(&self, path: &str) -> BTreeSet<TargetId> {
        self.by_target
            .iter()
            .filter(|(_, inputs)| inputs.iter().any(|input| input.is_match(path)))
            .map(|(target, _)| target.clone())
            .collect()
    }

    #[cfg(test)]
    pub(super) fn matching_paths(
        &self,
        paths: impl IntoIterator<Item = String>,
    ) -> BTreeMap<TargetId, BTreeSet<String>> {
        let mut matches = BTreeMap::<TargetId, BTreeSet<String>>::new();
        for path in paths {
            for target in self.matching_targets(&path) {
                matches.entry(target).or_default().insert(path.clone());
            }
        }
        matches
    }

    pub(super) fn component_has_inputs(&self, component: &ComponentId) -> bool {
        self.components_with_inputs.contains(component)
    }
}
