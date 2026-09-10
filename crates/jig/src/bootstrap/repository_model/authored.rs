use super::*;

#[derive(Clone, Debug, Deserialize)]
pub(in crate::bootstrap) struct AuthoredRepositoryModel {
    pub(in crate::bootstrap) default_check_profile: ProfileId,
    #[serde(default)]
    pub(in crate::bootstrap) affected_ignore: Vec<String>,
    #[serde(default)]
    pub(in crate::bootstrap) components: Vec<ComponentSpec>,
    #[serde(default)]
    pub(in crate::bootstrap) actions: Vec<ActionSpec>,
    #[serde(default)]
    pub(in crate::bootstrap) profiles: Vec<ProfileSpec>,
}

impl AuthoredRepositoryModel {
    pub(in crate::bootstrap) fn is_complete(&self) -> bool {
        !self.components.is_empty() && !self.actions.is_empty() && !self.profiles.is_empty()
    }

    pub(in crate::bootstrap) fn has_adapter(&self, expected: &str) -> bool {
        self.components
            .iter()
            .any(|component| component.adapters.iter().any(|adapter| adapter == expected))
    }

    pub(in crate::bootstrap) fn scaffold_go_component_roots(&self) -> Vec<String> {
        self.components
            .iter()
            .filter(|component| component.adapters.iter().any(|adapter| adapter == "go"))
            .map(|component| component.root.clone())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect()
    }

    pub(in crate::bootstrap) fn rust_workspace_guidance_enabled(&self) -> bool {
        rust_workspace_guidance_enabled(&self.components)
    }
}
