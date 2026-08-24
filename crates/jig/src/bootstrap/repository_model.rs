use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Context, Result, bail};
use jig_contract::{
    ActionEffect, ActionId, ActionIntent, ActionRunner, ActionSpec, AdapterRunnerDescriptor,
    ComponentId, ComponentSpec, FieldProvenance, ManifestTool, ProfileId, ProfileSpec, TargetId,
    kind,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::FrontendApp;
use super::answers::RenderAnswers;
use super::source_inputs::FRONTEND_SHARED_INPUTS;

const REPO_COMPONENT: &str = "repo";
const BACKEND_COMPONENT: &str = "api";
const DEFAULT_PROFILE: &str = "verify";
const FRONTEND_CONTRACT_DRIFT_ACTION: &str = "frontend-contract-drift";
const FRONTEND_PUBLIC_BOUNDARY_ACTION: &str = "frontend-public-boundary";
// Explicit component and action inputs are matched before this list. These
// defaults therefore suppress only repository guidance, documentation, and
// hosted-CI metadata that no local action declares as an input. Files such as
// `.gitignore`, `Makefile`, and `justfile` deliberately remain fail-closed.
const DEFAULT_AFFECTED_IGNORE: &[&str] = &[
    ".env",
    ".env.*",
    "**/.env",
    "**/.env.*",
    "README.md",
    "**/README.md",
    "AGENTS.md",
    "**/AGENTS.md",
    "agent-map.md",
    "CHANGELOG.md",
    "CONTRIBUTING.md",
    "CODE_OF_CONDUCT.md",
    "SECURITY.md",
    "docs/**",
    "LICENSE",
    "LICENSE.*",
    ".github/**",
];

#[derive(Clone, Debug, Serialize)]
pub(super) struct RepositoryRenderModel {
    pub(super) affected_ignore: Vec<String>,
    pub(super) components: Vec<ComponentSpec>,
    pub(super) actions: Vec<ActionSpec>,
    pub(super) profiles: Vec<ProfileSpec>,
    pub(super) default_check_profile: ProfileId,
    pub(super) required_commands: Vec<String>,
    pub(super) tools: Vec<ManifestTool>,
    #[serde(skip)]
    commands: BTreeMap<String, String>,
}

#[derive(Serialize)]
struct AuthoredRepository<'a> {
    default_check_profile: &'a ProfileId,
    affected_ignore: &'a [String],
    components: &'a [ComponentSpec],
    actions: &'a [ActionSpec],
    profiles: &'a [ProfileSpec],
}

#[derive(Clone, Debug, Deserialize)]
pub(super) struct AuthoredRepositoryModel {
    pub(super) default_check_profile: ProfileId,
    #[serde(default)]
    pub(super) affected_ignore: Vec<String>,
    #[serde(default)]
    pub(super) components: Vec<ComponentSpec>,
    #[serde(default)]
    pub(super) actions: Vec<ActionSpec>,
    #[serde(default)]
    pub(super) profiles: Vec<ProfileSpec>,
}

#[derive(Serialize)]
struct AuthoredRepositoryDocument<'a> {
    repository: AuthoredRepository<'a>,
}

#[derive(Serialize)]
struct AuthoredCommands<'a> {
    commands: &'a BTreeMap<String, String>,
}

impl RepositoryRenderModel {
    pub(super) fn from_answers(answers: &RenderAnswers) -> Result<Self> {
        if let Some(authored) = answers.authored_repository() {
            return Self::from_authored(authored, answers.authored_repository_commands());
        }
        let mut builder = ModelBuilder::new(answers)?;
        builder.add_repository_component()?;
        builder.add_backend_component()?;
        builder.add_frontend_components()?;
        builder.finish()
    }

    fn from_authored(
        authored: &AuthoredRepositoryModel,
        authored_commands: &BTreeMap<String, String>,
    ) -> Result<Self> {
        let mut commands = BTreeMap::new();
        let mut tools = BTreeMap::new();
        for action in &authored.actions {
            let (kind, command_key) = match &action.runner {
                ActionRunner::Command { command, .. } => {
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
                    commands.insert(command.clone(), value.clone());
                    (kind::COMMAND, Some(command.as_str()))
                }
                ActionRunner::Native { .. } => (kind::NATIVE, None),
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
            required_commands: commands.keys().cloned().collect(),
            tools: tools.into_values().collect(),
            commands,
        })
    }

    pub(super) fn authored_toml(&self) -> Result<String> {
        toml::to_string(&AuthoredRepositoryDocument {
            repository: AuthoredRepository {
                default_check_profile: &self.default_check_profile,
                affected_ignore: &self.affected_ignore,
                components: &self.components,
                actions: &self.actions,
                profiles: &self.profiles,
            },
        })
        .context("Failed to serialize the authored repository model")
    }

    pub(super) fn commands_toml(&self) -> Result<String> {
        toml::to_string(&AuthoredCommands {
            commands: &self.commands,
        })
        .context("Failed to serialize repository commands")
    }

    pub(super) fn frontend_contracts_enabled(&self) -> bool {
        [
            (FRONTEND_CONTRACT_DRIFT_ACTION, "contracts-drift-check"),
            (FRONTEND_PUBLIC_BOUNDARY_ACTION, "contracts-boundary-check"),
        ]
        .into_iter()
        .all(|(action, mode)| {
            self.actions.iter().any(|candidate| {
                if candidate.target.component.as_str() != REPO_COMPONENT
                    || candidate.target.action.as_str() != action
                {
                    return false;
                }
                let ActionRunner::Command { command, .. } = &candidate.runner else {
                    return false;
                };
                self.commands
                    .get(command.as_str())
                    .is_some_and(|value| value.contains(mode))
            })
        })
    }
}

struct ModelBuilder<'a> {
    answers: &'a RenderAnswers,
    components: BTreeMap<ComponentId, ComponentSpec>,
    actions: BTreeMap<TargetId, ActionSpec>,
    compatibility_actions: BTreeSet<TargetId>,
    commands: BTreeMap<String, String>,
    tools: BTreeMap<String, ManifestTool>,
}

impl<'a> ModelBuilder<'a> {
    fn new(answers: &'a RenderAnswers) -> Result<Self> {
        Ok(Self {
            answers,
            components: BTreeMap::new(),
            actions: BTreeMap::new(),
            compatibility_actions: BTreeSet::new(),
            commands: BTreeMap::new(),
            tools: BTreeMap::new(),
        })
    }

    fn add_repository_component(&mut self) -> Result<()> {
        let mut component = ComponentSpec::new(component_id(REPO_COMPONENT)?, ".");
        component.description =
            Some("Repository-wide Jig policy and compatibility actions.".into());
        component.tags = vec!["repository".into()];
        component.adapters = vec!["jig".into()];
        component.provenance = provenance(&[
            ("id", FieldProvenance::Inferred),
            ("root", FieldProvenance::Inferred),
            ("adapters", FieldProvenance::Inherited),
        ]);
        self.insert_component(component)?;
        self.add_adapter_actions(REPO_COMPONENT, "jig", CommandScope::Component, |_| true)?;
        Ok(())
    }

    fn add_backend_component(&mut self) -> Result<()> {
        let backend_adapter = self.answers.backend_language().as_str();
        let mut adapters = vec![backend_adapter.to_owned()];
        if self.answers.sqlx_enabled() {
            adapters.push("sqlx".into());
        }
        if self.answers.backend_language().is_go() && self.answers.go_database().is_postgres() {
            adapters.push("go-postgres".into());
        }

        let mut component = ComponentSpec::new(component_id(BACKEND_COMPONENT)?, ".");
        component.description = Some("Primary application backend.".into());
        component.tags = vec!["backend".into()];
        component.propagate_affected_to_dependents = true;
        component.adapters = adapters.clone();
        component.provenance = provenance(&[
            ("id", FieldProvenance::Inferred),
            ("root", FieldProvenance::Inferred),
            ("adapters", FieldProvenance::Inferred),
        ]);
        self.insert_component(component)?;

        self.add_adapter_actions(
            BACKEND_COMPONENT,
            backend_adapter,
            CommandScope::Component,
            |_| true,
        )?;
        if adapters.iter().any(|adapter| adapter == "sqlx") {
            let schema_dump_enabled = self.answers.schema_dump_enabled();
            let migration_add_enabled = self.answers.migration_add_enabled();
            self.add_adapter_actions(BACKEND_COMPONENT, "sqlx", CommandScope::Component, |id| {
                (schema_dump_enabled || !matches!(id, "schema" | "schema-dump"))
                    && (migration_add_enabled || id != "migration-add")
            })?;
        }
        if adapters.iter().any(|adapter| adapter == "go-postgres") {
            self.add_adapter_actions(
                BACKEND_COMPONENT,
                "go-postgres",
                CommandScope::Component,
                |_| true,
            )?;
        }
        Ok(())
    }

    fn add_frontend_components(&mut self) -> Result<()> {
        if !self.answers.frontend_harness_enabled() {
            return Ok(());
        }

        if self.answers.scaffolded_frontend_contracts() {
            let first_app = self
                .answers
                .frontend_apps()
                .first()
                .expect("the frontend harness is enabled only with configured apps")
                .clone();
            self.add_frontend_contract_actions(&first_app)?;
        }
        for app in self.answers.frontend_apps() {
            let component = frontend_component(app)?;
            let component_name = component.id.to_string();
            self.insert_component(component)?;
            self.add_typescript_actions(&component_name, app)?;
        }
        self.add_typescript_compatibility_actions()?;
        Ok(())
    }

    fn add_frontend_contract_actions(&mut self, dependency_anchor: &FrontendApp) -> Result<()> {
        let public_boundary_description = if self.answers.backend_language().is_go() {
            // The go-react preset intentionally has no privileged backend/API
            // surface yet. Do not claim that its artifact-only boundary check
            // proves a backend dependency property that the preset does not
            // model.
            "Check public frontend manifests and artifacts for privileged markers."
        } else {
            "Check the repository-wide public/private dependency boundary."
        };
        for (action_id, description, mode) in [
            (
                FRONTEND_CONTRACT_DRIFT_ACTION,
                "Check repository-wide OpenAPI and generated-client drift.",
                "contracts-drift-check",
            ),
            (
                FRONTEND_PUBLIC_BOUNDARY_ACTION,
                public_boundary_description,
                "contracts-boundary-check",
            ),
        ] {
            let command_key = CommandScope::Component.command_key(REPO_COMPONENT, action_id)?;
            let command = format!(
                "scripts/check-webapps.sh {mode} {}",
                crate::shell::quote(&dependency_anchor.dir)
            );
            self.insert_command(&command_key, &command)?;
            let mut action = ActionSpec::new(
                target_id(REPO_COMPONENT, action_id)?,
                ActionIntent::Check,
                ActionRunner::command(command_key),
            );
            action.description = Some(description.into());
            action.effects = vec![ActionEffect::ReadOnly, ActionEffect::Process];
            action.inputs = frontend_contract_inputs(action_id == FRONTEND_PUBLIC_BOUNDARY_ACTION);
            action.provenance = provenance(&[
                ("target", FieldProvenance::Inferred),
                ("intent", FieldProvenance::Inherited),
                ("effects", FieldProvenance::Inherited),
                ("runner", FieldProvenance::Inferred),
                ("inputs", FieldProvenance::Inferred),
            ]);
            self.insert_action(action)?;
        }
        Ok(())
    }

    fn add_adapter_actions(
        &mut self,
        component: &str,
        adapter_id: &str,
        scope: CommandScope,
        include: impl Fn(&str) -> bool,
    ) -> Result<()> {
        let adapter = jig_features::repository_adapter(adapter_id)
            .ok_or_else(|| anyhow::anyhow!("unknown repository adapter '{adapter_id}'"))?;
        for descriptor in adapter.actions.iter().filter(|action| include(action.id)) {
            let command_key = match descriptor.runner {
                AdapterRunnerDescriptor::Command(legacy_key) => {
                    let command = self.answers.repository_command(legacy_key).ok_or_else(|| {
                        anyhow::anyhow!(
                            "adapter '{adapter_id}' action '{}' has no configured command value for '{legacy_key}'",
                            descriptor.id
                        )
                    })?;
                    let command_key = scope.command_key(component, descriptor.id)?;
                    self.insert_command(&command_key, command)?;
                    Some(command_key)
                }
                AdapterRunnerDescriptor::Native(_) => None,
                _ => bail!(
                    "adapter '{adapter_id}' action '{}' uses an unsupported runner",
                    descriptor.id
                ),
            };
            let runner = match descriptor.runner {
                AdapterRunnerDescriptor::Command(_) => ActionRunner::command(
                    command_key
                        .as_deref()
                        .expect("command descriptors always produce a command key"),
                ),
                AdapterRunnerDescriptor::Native(operation) => ActionRunner::native(operation),
                _ => bail!(
                    "adapter '{adapter_id}' action '{}' uses an unsupported runner",
                    descriptor.id
                ),
            };
            let target = target_id(component, descriptor.id)?;
            let mut action = ActionSpec::new(target.clone(), descriptor.intent, runner);
            action.description = Some(descriptor.description.into());
            action.effects = descriptor.effects.to_vec();
            action.inputs = self.adapter_inputs(descriptor);
            if let Some(alias) = descriptor.legacy_alias {
                action.legacy_aliases.push(alias.into());
                self.insert_tool(alias, descriptor.description, command_key.as_deref())?;
            }
            action.provenance = provenance(&[
                ("target", FieldProvenance::Inherited),
                ("intent", FieldProvenance::Inherited),
                ("effects", FieldProvenance::Inherited),
                ("runner", FieldProvenance::Overridden),
                ("inputs", FieldProvenance::Inherited),
                ("legacy_aliases", FieldProvenance::Inherited),
            ]);
            self.insert_action(action)?;
        }
        Ok(())
    }

    fn adapter_inputs(&self, descriptor: &jig_contract::AdapterActionDescriptor) -> Vec<String> {
        if matches!(
            descriptor.runner,
            AdapterRunnerDescriptor::Native(jig_contract::tool::MIGRATION_ADD)
        ) && let Some(migration_dir) = self.answers.migration_dir()
        {
            return vec![format!("{}/**", migration_dir.trim_end_matches('/'))];
        }

        descriptor
            .inputs
            .iter()
            .map(|input| (*input).into())
            .collect()
    }

    fn add_typescript_actions(&mut self, component: &str, app: &FrontendApp) -> Result<()> {
        let adapter = jig_features::repository_adapter("typescript")
            .expect("the TypeScript repository adapter must be registered");
        for descriptor in adapter.actions {
            let script = match descriptor.id {
                "lint" => "lint",
                "typecheck" => "typecheck",
                "build" => "build:bundle",
                "test" => "test:coverage",
                other => bail!("unsupported generated TypeScript action '{other}'"),
            };
            let command_key = CommandScope::Component.command_key(component, descriptor.id)?;
            let command = format!(
                "scripts/check-webapps.sh check-one {} {} {}",
                crate::shell::quote(&app.dir),
                app.coverage_threshold,
                crate::shell::quote(script)
            );
            self.insert_command(&command_key, &command)?;
            let target = target_id(component, descriptor.id)?;
            let mut action = ActionSpec::new(
                target,
                descriptor.intent,
                ActionRunner::command(command_key),
            );
            action.description = Some(descriptor.description.into());
            action.effects = descriptor.effects.to_vec();
            action.inputs = frontend_inputs(&app.dir, descriptor.inputs);
            action.depends_on = match (self.answers.scaffolded_frontend_contracts(), descriptor.id)
            {
                (true, "typecheck") => vec![
                    target_id(REPO_COMPONENT, FRONTEND_CONTRACT_DRIFT_ACTION)?,
                    target_id(REPO_COMPONENT, FRONTEND_PUBLIC_BOUNDARY_ACTION)?,
                ],
                (true, "build") => {
                    vec![target_id(REPO_COMPONENT, FRONTEND_PUBLIC_BOUNDARY_ACTION)?]
                }
                _ => Vec::new(),
            };
            action.provenance = provenance(&[
                ("target", FieldProvenance::Declared),
                ("intent", FieldProvenance::Inherited),
                ("effects", FieldProvenance::Inherited),
                ("runner", FieldProvenance::Inferred),
                ("inputs", FieldProvenance::Inferred),
                ("depends_on", FieldProvenance::Inferred),
            ]);
            self.insert_action(action)?;
        }
        Ok(())
    }

    fn add_typescript_compatibility_actions(&mut self) -> Result<()> {
        let adapter = jig_features::repository_adapter("typescript")
            .expect("the TypeScript repository adapter must be registered");
        for descriptor in adapter.actions {
            let AdapterRunnerDescriptor::Command(legacy_key) = descriptor.runner else {
                continue;
            };
            let suffix = if descriptor.id == "test" {
                "coverage"
            } else {
                descriptor.id
            };
            let action_id = format!("typescript-{suffix}");
            let command_key =
                CommandScope::Compatibility.command_key(REPO_COMPONENT, &action_id)?;
            let command = self
                .answers
                .repository_command(legacy_key)
                .expect("TypeScript adapter command values are resolved with the answers");
            self.insert_command(&command_key, command)?;
            let target = target_id(REPO_COMPONENT, &action_id)?;
            let description = format!(
                "Compatibility aggregate for {} across all TypeScript components.",
                descriptor.id
            );
            let mut action = ActionSpec::new(
                target.clone(),
                ActionIntent::Check,
                ActionRunner::command(command_key.clone()),
            );
            action.description = Some(description.clone());
            action.effects = descriptor.effects.to_vec();
            action.legacy_aliases = descriptor
                .legacy_alias
                .into_iter()
                .map(str::to_owned)
                .collect();
            action.provenance = provenance(&[
                ("target", FieldProvenance::Inferred),
                ("runner", FieldProvenance::Inherited),
                ("legacy_aliases", FieldProvenance::Inherited),
            ]);
            if let Some(alias) = descriptor.legacy_alias {
                self.insert_tool(alias, &description, Some(&command_key))?;
            }
            self.compatibility_actions.insert(target);
            self.insert_action(action)?;
        }
        Ok(())
    }

    fn insert_component(&mut self, component: ComponentSpec) -> Result<()> {
        if self
            .components
            .insert(component.id.clone(), component)
            .is_some()
        {
            bail!("generated repository model contains duplicate component ids");
        }
        Ok(())
    }

    fn insert_action(&mut self, action: ActionSpec) -> Result<()> {
        if self.actions.insert(action.target.clone(), action).is_some() {
            bail!("generated repository model contains duplicate targets");
        }
        Ok(())
    }

    fn insert_command(&mut self, key: &str, value: &str) -> Result<()> {
        if value.trim().is_empty() {
            bail!("generated command key '{key}' has an empty value");
        }
        if let Some(existing) = self.commands.insert(key.into(), value.into())
            && existing != value
        {
            bail!("generated command key '{key}' has conflicting values");
        }
        Ok(())
    }

    fn insert_tool(
        &mut self,
        name: &str,
        description: &str,
        command_key: Option<&str>,
    ) -> Result<()> {
        let mut tool = ManifestTool::new(
            name,
            if command_key.is_some() {
                kind::COMMAND
            } else {
                kind::NATIVE
            },
            description,
        );
        if let Some(command_key) = command_key {
            tool.command = Some(command_key.into());
        }
        if self.tools.insert(name.into(), tool).is_some() {
            bail!("generated repository model contains duplicate legacy tool '{name}'");
        }
        Ok(())
    }

    fn finish(self) -> Result<RepositoryRenderModel> {
        let default_check_profile = ProfileId::parse(DEFAULT_PROFILE)?;
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
enum CommandScope {
    Component,
    Compatibility,
}

impl CommandScope {
    fn command_key(self, component: &str, action: &str) -> Result<String> {
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

fn frontend_component(app: &FrontendApp) -> Result<ComponentSpec> {
    let id = frontend_component_id(&app.name)?;
    let mut component = ComponentSpec::new(id, &app.dir);
    component.description = Some(format!("Frontend application '{}'.", app.name));
    component.tags = vec!["frontend".into(), app.role.clone()];
    component.depends_on = vec![component_id(BACKEND_COMPONENT)?];
    component.adapters = vec!["typescript".into()];
    component.provenance = provenance(&[
        ("id", FieldProvenance::Inferred),
        ("root", FieldProvenance::Declared),
        ("depends_on", FieldProvenance::Inferred),
        ("adapters", FieldProvenance::Inferred),
    ]);
    Ok(component)
}

pub(super) fn frontend_component_id(name: &str) -> Result<ComponentId> {
    let normalized = name.to_ascii_lowercase();
    if matches!(normalized.as_str(), REPO_COMPONENT | BACKEND_COMPONENT) {
        bail!(
            "Frontend app name '{name}' resolves to reserved repository component id '{normalized}'; choose a different frontend name"
        );
    }
    let value = if normalized.len() <= 64 {
        normalized
    } else {
        let digest = format!("{:x}", Sha256::digest(normalized.as_bytes()));
        let mut end = 51;
        while !normalized.is_char_boundary(end) {
            end -= 1;
        }
        format!(
            "{}-{}",
            normalized[..end].trim_end_matches('-'),
            &digest[..12]
        )
    };
    component_id(&value)
        .with_context(|| format!("Invalid frontend app name '{name}' for repository identity"))
}

fn frontend_inputs(root: &str, inputs: &[&str]) -> Vec<String> {
    let mut resolved = inputs
        .iter()
        .map(|input| {
            if root == "." {
                (*input).to_owned()
            } else {
                format!("{root}/{input}")
            }
        })
        .collect::<Vec<_>>();
    resolved.extend(
        FRONTEND_SHARED_INPUTS
            .iter()
            .map(|input| (*input).to_owned()),
    );
    resolved.sort();
    resolved.dedup();
    resolved
}

fn frontend_contract_inputs(include_public_artifacts: bool) -> Vec<String> {
    let mut inputs = FRONTEND_SHARED_INPUTS
        .iter()
        .copied()
        .chain([
            "Cargo.toml",
            "**/Cargo.toml",
            "**/*.rs",
            "go.mod",
            "**/go.mod",
            "**/*.go",
        ])
        .map(str::to_owned)
        .collect::<Vec<_>>();
    if include_public_artifacts {
        inputs.extend(["docs/public/**".into(), "public-docs/**".into()]);
    }
    inputs.sort();
    inputs.dedup();
    inputs
}

fn provenance(entries: &[(&str, FieldProvenance)]) -> BTreeMap<String, FieldProvenance> {
    entries
        .iter()
        .map(|(field, source)| ((*field).into(), *source))
        .collect()
}

fn component_id(value: &str) -> Result<ComponentId> {
    ComponentId::parse(value).map_err(Into::into)
}

fn target_id(component: &str, action: &str) -> Result<TargetId> {
    Ok(TargetId::new(
        component_id(component)?,
        ActionId::parse(action)?,
    ))
}

#[cfg(test)]
mod tests;
