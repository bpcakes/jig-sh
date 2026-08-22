use anyhow::{Result, bail};
use jig_contract::{
    ActionEffect, ActionIntent, ActionRunner, ComponentId, ComponentSpec, ProfileId, ProfileSpec,
    ResultParser, TargetId,
};
use schemars::JsonSchema;
use serde::Serialize;
use serde_json::{Value, json};

use crate::context::RepoContext;

use super::RepositoryCatalog;

#[derive(Clone, Debug)]
pub(crate) enum InspectRequest {
    Workspace,
    Components,
    Component(String),
    Targets,
    Target(String),
    Profiles,
    Profile(String),
}

#[derive(Clone, Debug, JsonSchema, Serialize)]
#[serde(untagged)]
pub(crate) enum CatalogInspection {
    Workspace(WorkspaceInspection),
    Components(ComponentsInspection),
    Component(ComponentInspection),
    Targets(TargetsInspection),
    Target(TargetInspection),
    Profiles(ProfilesInspection),
    Profile(ProfileInspection),
}

impl CatalogInspection {
    const fn command(&self) -> &'static str {
        match self {
            Self::Workspace(_) => "info workspace",
            Self::Components(_) => "info components",
            Self::Component(_) => "info component",
            Self::Targets(_) => "info targets",
            Self::Target(_) => "info target",
            Self::Profiles(_) => "info profiles",
            Self::Profile(_) => "info profile",
        }
    }
}

#[derive(Clone, Debug, JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct WorkspaceInspection {
    workspace: WorkspaceSummary,
    components: Vec<ComponentSpec>,
    targets: Vec<TargetSummary>,
    profiles: Vec<ProfileSpec>,
}

#[derive(Clone, Debug, JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ComponentsInspection {
    workspace: WorkspaceSummary,
    components: Vec<ComponentSummary>,
}

#[derive(Clone, Debug, JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ComponentInspection {
    workspace: WorkspaceSummary,
    component: ComponentSpec,
    targets: Vec<TargetSummary>,
}

#[derive(Clone, Debug, JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct TargetsInspection {
    workspace: WorkspaceSummary,
    targets: Vec<TargetSummary>,
}

#[derive(Clone, Debug, JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct TargetInspection {
    workspace: WorkspaceSummary,
    target: TargetSummary,
}

#[derive(Clone, Debug, JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ProfilesInspection {
    workspace: WorkspaceSummary,
    default_check_profile: Option<ProfileId>,
    profiles: Vec<ProfileSpec>,
}

#[derive(Clone, Debug, JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ProfileInspection {
    workspace: WorkspaceSummary,
    profile: ProfileSpec,
    is_default_check_profile: bool,
}

#[derive(Clone, Debug, JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
struct WorkspaceSummary {
    name: String,
    root: String,
    contract_version: u32,
    config_digest: String,
    default_check_profile: Option<ProfileId>,
}

#[derive(Clone, Debug, JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
struct ComponentSummary {
    component: ComponentSpec,
    target_count: usize,
}

#[derive(Clone, Debug, JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
struct TargetSummary {
    id: TargetId,
    description: Option<String>,
    intent: ActionIntent,
    effects: Vec<ActionEffect>,
    runner: ActionRunner,
    inputs: Vec<String>,
    depends_on: Vec<TargetId>,
    timeout_seconds: Option<u64>,
    result_parser: ResultParser,
    legacy_aliases: Vec<String>,
}

pub(crate) fn inspect_repository(ctx: &RepoContext, request: InspectRequest) -> Result<Value> {
    inspection_value(inspect_repository_data(ctx, request)?)
}

fn inspection_value(inspection: CatalogInspection) -> Result<Value> {
    let command = inspection.command();
    let Value::Object(mut result) = serde_json::to_value(inspection)? else {
        unreachable!("catalog inspection serializes as an object");
    };
    result.insert("ok".into(), json!(true));
    result.insert("command".into(), json!(command));
    result.insert("schema_version".into(), json!(1));
    Ok(Value::Object(result))
}

pub(crate) fn inspect_repository_data(
    ctx: &RepoContext,
    request: InspectRequest,
) -> Result<CatalogInspection> {
    let catalog = RepositoryCatalog::from_context(ctx)?;
    inspect_catalog_data(
        &catalog,
        ctx.repo_name(),
        &ctx.root().display().to_string(),
        request,
    )
}

#[cfg(test)]
fn inspect_catalog(
    catalog: &RepositoryCatalog,
    workspace_name: &str,
    workspace_root: &str,
    request: InspectRequest,
) -> Result<Value> {
    inspection_value(inspect_catalog_data(
        catalog,
        workspace_name,
        workspace_root,
        request,
    )?)
}

fn inspect_catalog_data(
    catalog: &RepositoryCatalog,
    workspace_name: &str,
    workspace_root: &str,
    request: InspectRequest,
) -> Result<CatalogInspection> {
    match request {
        InspectRequest::Workspace => Ok(CatalogInspection::Workspace(WorkspaceInspection {
            workspace: workspace_value(catalog, workspace_name, workspace_root),
            components: catalog.components().cloned().collect(),
            targets: catalog
                .actions()
                .map(|action| target_value(catalog, action))
                .collect(),
            profiles: catalog.profiles().cloned().collect(),
        })),
        InspectRequest::Components => Ok(CatalogInspection::Components(ComponentsInspection {
            workspace: workspace_value(catalog, workspace_name, workspace_root),
            components: catalog
                .components()
                .map(|component| component_value(catalog, component))
                .collect(),
        })),
        InspectRequest::Component(id) => {
            let id = ComponentId::parse(id)?;
            let Some(component) = catalog.component(&id) else {
                bail!("unknown component '{id}'");
            };
            Ok(CatalogInspection::Component(ComponentInspection {
                workspace: workspace_value(catalog, workspace_name, workspace_root),
                component: component.clone(),
                targets: catalog
                    .actions()
                    .filter(|action| action.target.component == id)
                    .map(|action| target_value(catalog, action))
                    .collect(),
            }))
        }
        InspectRequest::Targets => Ok(CatalogInspection::Targets(TargetsInspection {
            workspace: workspace_value(catalog, workspace_name, workspace_root),
            targets: catalog
                .actions()
                .map(|action| target_value(catalog, action))
                .collect(),
        })),
        InspectRequest::Target(target) => {
            let target: TargetId = target.parse()?;
            let Some(action) = catalog.action(&target) else {
                bail!("unknown target '{target}'");
            };
            Ok(CatalogInspection::Target(TargetInspection {
                workspace: workspace_value(catalog, workspace_name, workspace_root),
                target: target_value(catalog, action),
            }))
        }
        InspectRequest::Profiles => Ok(CatalogInspection::Profiles(ProfilesInspection {
            workspace: workspace_value(catalog, workspace_name, workspace_root),
            default_check_profile: catalog.default_check_profile().cloned(),
            profiles: catalog.profiles().cloned().collect(),
        })),
        InspectRequest::Profile(profile) => {
            let profile = ProfileId::parse(profile)?;
            let Some(spec) = catalog.profile(&profile) else {
                bail!("unknown profile '{profile}'");
            };
            Ok(CatalogInspection::Profile(ProfileInspection {
                workspace: workspace_value(catalog, workspace_name, workspace_root),
                profile: spec.clone(),
                is_default_check_profile: catalog.default_check_profile() == Some(&profile),
            }))
        }
    }
}

fn workspace_value(
    catalog: &RepositoryCatalog,
    workspace_name: &str,
    workspace_root: &str,
) -> WorkspaceSummary {
    WorkspaceSummary {
        name: workspace_name.to_owned(),
        root: workspace_root.to_owned(),
        contract_version: catalog.contract_version(),
        config_digest: catalog.config_digest().to_owned(),
        default_check_profile: catalog.default_check_profile().cloned(),
    }
}

fn component_value(catalog: &RepositoryCatalog, component: &ComponentSpec) -> ComponentSummary {
    ComponentSummary {
        component: component.clone(),
        target_count: catalog
            .actions()
            .filter(|action| action.target.component == component.id)
            .count(),
    }
}

fn target_value(catalog: &RepositoryCatalog, action: &jig_contract::ActionSpec) -> TargetSummary {
    TargetSummary {
        id: action.target.clone(),
        description: action.description.clone(),
        intent: action.intent,
        effects: action.effects.clone(),
        runner: action.runner.clone(),
        inputs: action.inputs.clone(),
        depends_on: action.depends_on.clone(),
        timeout_seconds: action.timeout_seconds,
        result_parser: action.result_parser,
        legacy_aliases: catalog.aliases_for_target(&action.target).to_vec(),
    }
}

#[cfg(test)]
mod tests {
    use jig_contract::{
        ActionEffect, ActionIntent, ActionRunner, ActionSpec, ComponentId, ComponentSpec,
        ProfileId, ProfileSpec, TargetId,
    };

    use super::{InspectRequest, inspect_catalog};
    use crate::repository::RepositoryCatalog;

    fn fixture() -> RepositoryCatalog {
        let components = [
            ComponentSpec::new(ComponentId::parse("api").unwrap(), "api"),
            ComponentSpec::new(ComponentId::parse("web").unwrap(), "web"),
        ];
        let api_target: TargetId = "api:test".parse().unwrap();
        let web_target: TargetId = "web:test".parse().unwrap();
        let mut api = ActionSpec::new(
            api_target.clone(),
            ActionIntent::Check,
            ActionRunner::command("go_test_command"),
        );
        api.effects.push(ActionEffect::ReadOnly);
        let mut web = ActionSpec::new(
            web_target.clone(),
            ActionIntent::Check,
            ActionRunner::command("typescript_test_command"),
        );
        web.effects.push(ActionEffect::ReadOnly);
        let profile_id = ProfileId::parse("verify").unwrap();
        let profile = ProfileSpec::new(profile_id.clone(), vec![api_target, web_target]);
        RepositoryCatalog::from_native(
            6,
            "sha256:config",
            &components,
            &[api, web],
            &[profile],
            Some(&profile_id),
        )
        .unwrap()
    }

    #[test]
    fn component_and_target_views_keep_target_identity_structured() {
        let component = inspect_catalog(
            &fixture(),
            "ExampleProject",
            "/repo",
            InspectRequest::Component("api".into()),
        )
        .unwrap();
        assert_eq!(component["component"]["id"], "api");
        assert_eq!(component["targets"][0]["id"]["component"], "api");
        assert_eq!(component["targets"][0]["id"]["action"], "test");

        let targets = inspect_catalog(
            &fixture(),
            "ExampleProject",
            "/repo",
            InspectRequest::Targets,
        )
        .unwrap();
        assert_eq!(targets["targets"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn unknown_inspection_addresses_fail_explicitly() {
        let error = inspect_catalog(
            &fixture(),
            "ExampleProject",
            "/repo",
            InspectRequest::Target("worker:test".into()),
        )
        .unwrap_err()
        .to_string();
        assert_eq!(error, "unknown target 'worker:test'");
    }
}
