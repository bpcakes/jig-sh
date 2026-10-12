use super::*;

impl ModelBuilder<'_> {
    pub(super) fn finish(self) -> Result<RepositoryRenderModel> {
        let default_check_profile = ProfileId::parse(DEFAULT_PROFILE)?;
        // Profile membership requires every check to pass without turning policy
        // receipts into execution prerequisites of otherwise independent checks.
        let profile_targets = self
            .actions
            .values()
            .filter(|action| {
                action.intent == ActionIntent::Check
                    && action
                        .effects
                        .contains(&jig_contract::ActionEffect::ReadOnly)
                    && action.target.action.as_str() != "test-locked"
                    && !self.compatibility_actions.contains(&action.target)
            })
            .map(|action| action.target.clone())
            .collect::<Vec<_>>();
        let mut profile = ProfileSpec::new(default_check_profile.clone(), profile_targets);
        profile.description = Some("Default repository verification targets.".into());
        profile.provenance = provenance(&[
            ("id", FieldProvenance::Inferred),
            ("targets", FieldProvenance::Inherited),
        ]);
        Ok(RepositoryRenderModel {
            affected_ignore: DEFAULT_AFFECTED_IGNORE
                .iter()
                .map(ToString::to_string)
                .collect(),
            components: self.components.into_values().collect(),
            actions: self.actions.into_values().collect(),
            profiles: vec![profile],
            default_check_profile,
            required_commands: self.commands.keys().cloned().collect(),
            tools: self.tools.into_values().collect(),
            commands: self.commands,
        })
    }
}

#[derive(Clone, Copy)]
pub(super) enum CommandScope {
    Component,
    Compatibility,
}

impl CommandScope {
    pub(super) fn command_key(self, component: &str, action: &str) -> Result<String> {
        let action = ActionId::parse(action)?;
        let prefix = match self {
            Self::Component => component.to_owned(),
            Self::Compatibility => format!("{component}_compat"),
        };
        let prefix = if prefix
            .as_bytes()
            .first()
            .is_some_and(u8::is_ascii_lowercase)
        {
            prefix
        } else {
            format!("component_{prefix}")
        };
        Ok(format!(
            "{}_{}_command",
            prefix.replace('-', "_"),
            action.as_str().replace('-', "_")
        ))
    }
}
