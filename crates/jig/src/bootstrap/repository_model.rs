use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Context, Result, bail};
use jig_contract::{
    ActionId, ActionIntent, ActionRunner, ActionSpec, AdapterRunnerDescriptor, ComponentId,
    ComponentSpec, FieldProvenance, ManifestTool, ProfileId, ProfileSpec, TargetId, kind,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::FrontendApp;
use super::answers::RenderAnswers;

const REPO_COMPONENT: &str = "repo";
const BACKEND_COMPONENT: &str = "api";
const DEFAULT_PROFILE: &str = "verify";

#[derive(Clone, Debug, Serialize)]
pub(super) struct RepositoryRenderModel {
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
    components: &'a [ComponentSpec],
    actions: &'a [ActionSpec],
    profiles: &'a [ProfileSpec],
}

#[derive(Clone, Debug, Deserialize)]
pub(super) struct AuthoredRepositoryModel {
    pub(super) default_check_profile: ProfileId,
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
            self.add_adapter_actions(BACKEND_COMPONENT, "sqlx", CommandScope::Component, |id| {
                schema_dump_enabled || !matches!(id, "schema" | "schema-dump")
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

        for app in self.answers.frontend_apps() {
            let component = frontend_component(app)?;
            let component_name = component.id.to_string();
            self.insert_component(component)?;
            self.add_typescript_actions(&component_name, app)?;
        }
        self.add_typescript_compatibility_actions()?;
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
            action.inputs = descriptor
                .inputs
                .iter()
                .map(|input| (*input).into())
                .collect();
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
            action.provenance = provenance(&[
                ("target", FieldProvenance::Declared),
                ("intent", FieldProvenance::Inherited),
                ("effects", FieldProvenance::Inherited),
                ("runner", FieldProvenance::Inferred),
                ("inputs", FieldProvenance::Inferred),
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

fn frontend_component_id(name: &str) -> Result<ComponentId> {
    let normalized = name.to_ascii_lowercase();
    if normalized.len() <= 64 {
        return component_id(&normalized);
    }
    let digest = format!("{:x}", Sha256::digest(normalized.as_bytes()));
    component_id(&format!("{}-{}", &normalized[..51], &digest[..12]))
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
        [
            "package.json",
            "bun.lock",
            "bun.lockb",
            "package-lock.json",
            "pnpm-lock.yaml",
            "yarn.lock",
        ]
        .into_iter()
        .map(str::to_owned),
    );
    resolved.sort();
    resolved.dedup();
    resolved
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
mod tests {
    use std::fs;

    use tempfile::TempDir;

    use super::*;

    fn answers(contents: &str) -> RenderAnswers {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("answers.toml");
        fs::write(
            &path,
            format!(
                "repo_name = \"ExampleProject\"\nsqlx_enabled = false\nschema_dump_enabled = false\n{contents}"
            ),
        )
        .unwrap();
        RenderAnswers::from_answers_file(&path).unwrap()
    }

    #[test]
    fn go_and_multiple_frontends_have_distinct_component_targets() {
        let answers = answers(
            r#"
backend_language = "go"
go_database = "postgres"

[[frontend_apps]]
name = "web"
dir = "frontend/web"
coverage_threshold = 80
kind = "vite"
role = "spa"

[[frontend_apps]]
name = "admin"
dir = "frontend/admin"
coverage_threshold = 85
kind = "vite"
role = "admin"
"#,
        );
        let model = RepositoryRenderModel::from_answers(&answers).unwrap();

        assert!(
            model
                .components
                .iter()
                .any(|component| component.id.as_str() == "api"
                    && component.adapters == ["go", "go-postgres"])
        );
        for target in ["api:test", "web:test", "admin:test"] {
            assert!(
                model
                    .actions
                    .iter()
                    .any(|action| action.target.to_string() == target),
                "missing {target}"
            );
        }
        assert!(model.required_commands.contains(&"web_test_command".into()));
        assert!(
            model
                .required_commands
                .contains(&"admin_test_command".into())
        );
        assert!(
            model
                .profiles
                .first()
                .unwrap()
                .targets
                .iter()
                .any(|target| target.to_string() == "web:test")
        );
    }

    #[test]
    fn rust_repository_uses_adapter_actions_without_backend_identity_fields() {
        let model = RepositoryRenderModel::from_answers(&answers("")).unwrap();
        let authored = model.authored_toml().unwrap();
        let commands = model.commands_toml().unwrap();

        assert!(authored.contains("adapters = [\"rust\"]"));
        assert!(!authored.contains("backend_language"));
        assert!(commands.contains("api_test_command"));
        assert!(!commands.contains("rust_test_command"));
    }

    #[test]
    fn adapter_identity_survives_loading_v6_authored_answers() {
        let answers = answers(
            r#"
[repository]
default_check_profile = "verify"

[[repository.components]]
id = "api"
root = "."
adapters = ["go", "go-postgres"]
"#,
        );

        assert!(answers.backend_language().is_go());
        assert!(answers.go_database().is_postgres());
        let model = RepositoryRenderModel::from_answers(&answers).unwrap();
        assert!(
            model
                .actions
                .iter()
                .any(|action| action.target.to_string() == "api:sqlc")
        );
    }

    #[test]
    fn component_command_overrides_survive_v6_answer_reload() {
        let initial = answers("rust_test_command = \"cargo nextest run\"\n");
        let model = RepositoryRenderModel::from_answers(&initial).unwrap();
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("answers.toml");
        fs::write(
            &path,
            format!(
                "repo_name = \"ExampleProject\"\nsqlx_enabled = false\nschema_dump_enabled = false\n{}\n{}",
                model.authored_toml().unwrap(),
                model.commands_toml().unwrap()
            ),
        )
        .unwrap();

        let reloaded = RenderAnswers::from_answers_file(&path).unwrap();

        assert_eq!(
            reloaded.repository_command("rust_test_command"),
            Some("cargo nextest run")
        );
    }

    #[test]
    fn go_and_typescript_command_overrides_survive_v6_answer_reload() {
        let initial = answers(
            r#"
backend_language = "go"
go_database = "postgres"
go_test_command = "go test -race ./..."
typescript_lint_command = "scripts/lint-all.sh"

[[frontend_apps]]
name = "web"
dir = "frontend/web"
coverage_threshold = 80
kind = "vite"
role = "spa"
"#,
        );
        let model = RepositoryRenderModel::from_answers(&initial).unwrap();
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("answers.toml");
        fs::write(
            &path,
            format!(
                "repo_name = \"ExampleProject\"\nsqlx_enabled = false\nschema_dump_enabled = false\n{}\n{}",
                model.authored_toml().unwrap(),
                model.commands_toml().unwrap()
            ),
        )
        .unwrap();

        let reloaded = RenderAnswers::from_answers_file(&path).unwrap();

        assert_eq!(
            reloaded.repository_command("go_test_command"),
            Some("go test -race ./...")
        );
        assert_eq!(
            reloaded.repository_command("typescript_lint_command"),
            Some("scripts/lint-all.sh")
        );
        let rerendered = RepositoryRenderModel::from_answers(&reloaded).unwrap();
        assert_eq!(
            serde_json::to_value(rerendered).unwrap(),
            serde_json::to_value(model).unwrap()
        );
    }

    #[test]
    fn authored_multi_backend_model_survives_v6_recopy_resolution() {
        let api = ComponentSpec {
            adapters: vec!["go".into()],
            ..ComponentSpec::new(component_id("api").unwrap(), "services/api")
        };
        let worker = ComponentSpec {
            adapters: vec!["rust".into()],
            ..ComponentSpec::new(component_id("worker").unwrap(), "services/worker")
        };
        let mut api_test = ActionSpec::new(
            target_id("api", "test").unwrap(),
            ActionIntent::Check,
            ActionRunner::command("api_test_command"),
        );
        api_test.effects = vec![jig_contract::ActionEffect::ReadOnly];
        let mut worker_test = ActionSpec::new(
            target_id("worker", "test").unwrap(),
            ActionIntent::Check,
            ActionRunner::command("worker_test_command"),
        );
        worker_test.effects = vec![jig_contract::ActionEffect::ReadOnly];
        let profile = ProfileSpec::new(
            ProfileId::parse("verify").unwrap(),
            vec![api_test.target.clone(), worker_test.target.clone()],
        );
        let authored = RepositoryRenderModel {
            components: vec![api, worker],
            actions: vec![api_test, worker_test],
            profiles: vec![profile],
            default_check_profile: ProfileId::parse("verify").unwrap(),
            required_commands: vec!["api_test_command".into(), "worker_test_command".into()],
            tools: Vec::new(),
            commands: BTreeMap::from([
                ("api_test_command".into(), "go test ./...".into()),
                (
                    "worker_test_command".into(),
                    "cargo test -p example-worker".into(),
                ),
            ]),
        };
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("answers.toml");
        fs::write(
            &path,
            format!(
                "repo_name = \"ExampleProject\"\nsqlx_enabled = false\nschema_dump_enabled = false\n{}\n{}",
                authored.authored_toml().unwrap(),
                authored.commands_toml().unwrap()
            ),
        )
        .unwrap();

        let answers = RenderAnswers::from_answers_file(&path).unwrap();
        let rerendered = RepositoryRenderModel::from_answers(&answers).unwrap();

        assert_eq!(
            rerendered
                .components
                .iter()
                .map(|component| component.id.as_str())
                .collect::<Vec<_>>(),
            ["api", "worker"]
        );
        assert_eq!(
            rerendered
                .actions
                .iter()
                .map(|action| action.target.to_string())
                .collect::<Vec<_>>(),
            ["api:test", "worker:test"]
        );
        assert_eq!(
            rerendered.commands["worker_test_command"],
            "cargo test -p example-worker"
        );
    }
}
