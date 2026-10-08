use super::*;

#[derive(Clone, Debug, Deserialize)]
pub struct AuthoredRepositoryModel {
    pub default_check_profile: ProfileId,
    #[serde(default)]
    pub affected_ignore: Vec<String>,
    #[serde(default)]
    pub components: Vec<ComponentSpec>,
    #[serde(default)]
    pub actions: Vec<ActionSpec>,
    #[serde(default)]
    pub profiles: Vec<ProfileSpec>,
}

impl AuthoredRepositoryModel {
    pub fn is_complete(&self) -> bool {
        !self.components.is_empty() && !self.actions.is_empty() && !self.profiles.is_empty()
    }

    pub fn has_adapter(&self, expected: &str) -> bool {
        self.components
            .iter()
            .any(|component| component.adapters.iter().any(|adapter| adapter == expected))
    }

    pub fn scaffold_go_component_roots(&self) -> Vec<String> {
        self.components
            .iter()
            .filter(|component| component.adapters.iter().any(|adapter| adapter == "go"))
            .map(|component| component.root.clone())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect()
    }

    pub fn rust_workspace_guidance_enabled(&self) -> bool {
        rust_workspace_guidance_enabled(&self.components)
    }
}

impl RepositoryRenderModel {
    pub(super) fn from_authored(
        answers: &RenderAnswers,
        authored: &AuthoredRepositoryModel,
        authored_commands: &BTreeMap<String, String>,
    ) -> Result<Self> {
        let mut commands = authored_commands.clone();
        refresh_managed_rust_file_loc_command(
            &authored.actions,
            &mut commands,
            answers.default_branch(),
        );
        let mut required_commands = BTreeSet::new();
        let mut tools = BTreeMap::new();
        for action in &authored.actions {
            let (kind, command_key) = match &action.runner {
                ActionRunner::RustNextestV1 { configuration } => {
                    jig_rust::rust_focus::validate_config(configuration)
                        .map_err(anyhow::Error::msg)?;
                    if !action.legacy_aliases.is_empty() {
                        bail!(
                            "Rust Nextest v1 uses typed target execution, not legacy tool aliases"
                        );
                    }
                    continue;
                }
                ActionRunner::Command { command, .. } | ActionRunner::Shell { command, .. } => {
                    let value = authored_commands.get(command.as_str()).ok_or_else(|| {
                        anyhow::anyhow!(
                            "authored target '{}' references missing command '{}'",
                            action.target,
                            command
                        )
                    })?;
                    if value.trim().is_empty() {
                        bail!(
                            "authored target '{}' references empty command '{}'",
                            action.target,
                            command
                        );
                    }
                    required_commands.insert(command.clone());
                    (kind::COMMAND, Some(command.as_str()))
                }
                ActionRunner::Native { .. } => (kind::NATIVE, None),
                ActionRunner::Argv { .. } => (kind::COMMAND, None),
            };
            for alias in &action.legacy_aliases {
                let mut tool = ManifestTool::new(
                    alias,
                    kind,
                    action
                        .description
                        .as_deref()
                        .unwrap_or("Compatibility alias for a repository target."),
                );
                tool.command = command_key.map(str::to_owned);
                if tools.insert(alias.clone(), tool).is_some() {
                    bail!("authored repository model contains duplicate legacy alias '{alias}'");
                }
            }
        }

        Ok(Self {
            affected_ignore: authored.affected_ignore.clone(),
            components: authored.components.clone(),
            actions: authored.actions.clone(),
            profiles: authored.profiles.clone(),
            default_check_profile: authored.default_check_profile.clone(),
            required_commands: required_commands.into_iter().collect(),
            tools: tools.into_values().collect(),
            commands,
        })
    }
}
