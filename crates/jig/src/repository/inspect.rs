use anyhow::{Result, bail};
use jig_contract::freshness::{
    InspectedInputsPolicyV1, InspectedSourceStateV1, TARGET_FRESHNESS_CONTRACT_VERSION,
    TargetFreshnessPolicyInspectionV1, TargetFreshnessPolicyModeV1,
};
use jig_contract::{
    ActionEffect, ActionInputsPolicy, ActionIntent, ActionRunner, ActionSourceState, ComponentId,
    ComponentSpec, FieldProvenance, ProfileId, ProfileSpec, ResultParser, TargetId,
};
use schemars::JsonSchema;
use serde::Serialize;
use serde_json::{Value, json};

use crate::context::RepoContext;
use crate::surface::ResponseSurface;

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

#[derive(Clone, Debug, JsonSchema, Serialize)]
#[serde(untagged)]
pub(crate) enum AgentCatalogInspection {
    Workspace(AgentWorkspaceInspection),
    Components(ComponentsInspection),
    Component(AgentComponentInspection),
    Targets(AgentTargetsInspection),
    Target(AgentTargetInspection),
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

impl AgentCatalogInspection {
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
pub(crate) struct AgentWorkspaceInspection {
    workspace: WorkspaceSummary,
    components: Vec<ComponentSpec>,
    targets: Vec<AgentTargetSummary>,
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
pub(crate) struct AgentComponentInspection {
    workspace: WorkspaceSummary,
    component: ComponentSpec,
    targets: Vec<AgentTargetSummary>,
}

#[derive(Clone, Debug, JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct TargetsInspection {
    workspace: WorkspaceSummary,
    targets: Vec<TargetSummary>,
}

#[derive(Clone, Debug, JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct AgentTargetsInspection {
    workspace: WorkspaceSummary,
    targets: Vec<AgentTargetSummary>,
}

#[derive(Clone, Debug, JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct TargetInspection {
    workspace: WorkspaceSummary,
    target: TargetSummary,
}

#[derive(Clone, Debug, JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct AgentTargetInspection {
    workspace: WorkspaceSummary,
    target: AgentTargetSummary,
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
    affected_ignore: Vec<String>,
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
    arguments: std::collections::BTreeMap<String, jig_contract::ActionArgumentSpec>,
    inputs: Vec<String>,
    depends_on: Vec<TargetId>,
    timeout_seconds: Option<u64>,
    result_parser: ResultParser,
    legacy_aliases: Vec<String>,
}

#[derive(Clone, Debug, JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
struct AgentTargetSummary {
    id: TargetId,
    description: Option<String>,
    intent: ActionIntent,
    effects: Vec<ActionEffect>,
    runner: ActionRunner,
    arguments: std::collections::BTreeMap<String, jig_contract::ActionArgumentSpec>,
    inputs: Vec<String>,
    freshness_policy: TargetFreshnessPolicyInspectionV1,
    depends_on: Vec<TargetId>,
    timeout_seconds: Option<u64>,
    result_parser: ResultParser,
    legacy_aliases: Vec<String>,
}

pub(crate) fn inspect_repository(
    ctx: &RepoContext,
    request: InspectRequest,
    surface: ResponseSurface,
) -> Result<Value> {
    match surface {
        ResponseSurface::Standard => inspection_value(inspect_repository_data(ctx, request)?),
        ResponseSurface::AgentV1 => {
            agent_inspection_value(inspect_repository_data_agent_v1(ctx, request)?)
        }
    }
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

fn agent_inspection_value(inspection: AgentCatalogInspection) -> Result<Value> {
    let command = inspection.command();
    let Value::Object(mut result) = serde_json::to_value(inspection)? else {
        unreachable!("agent catalog inspection serializes as an object");
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

pub(crate) fn inspect_repository_data_agent_v1(
    ctx: &RepoContext,
    request: InspectRequest,
) -> Result<AgentCatalogInspection> {
    let catalog = RepositoryCatalog::from_context(ctx)?;
    inspect_catalog_data_agent_v1(
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

#[cfg(test)]
fn inspect_catalog_agent_v1(
    catalog: &RepositoryCatalog,
    workspace_name: &str,
    workspace_root: &str,
    request: InspectRequest,
) -> Result<Value> {
    agent_inspection_value(inspect_catalog_data_agent_v1(
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

fn inspect_catalog_data_agent_v1(
    catalog: &RepositoryCatalog,
    workspace_name: &str,
    workspace_root: &str,
    request: InspectRequest,
) -> Result<AgentCatalogInspection> {
    match request {
        InspectRequest::Workspace => Ok(AgentCatalogInspection::Workspace(
            AgentWorkspaceInspection {
                workspace: workspace_value(catalog, workspace_name, workspace_root),
                components: catalog.components().cloned().collect(),
                targets: catalog
                    .actions()
                    .map(|action| agent_target_value(catalog, action))
                    .collect(),
                profiles: catalog.profiles().cloned().collect(),
            },
        )),
        InspectRequest::Components => {
            Ok(AgentCatalogInspection::Components(ComponentsInspection {
                workspace: workspace_value(catalog, workspace_name, workspace_root),
                components: catalog
                    .components()
                    .map(|component| component_value(catalog, component))
                    .collect(),
            }))
        }
        InspectRequest::Component(id) => {
            let id = ComponentId::parse(id)?;
            let Some(component) = catalog.component(&id) else {
                bail!("unknown component '{id}'");
            };
            Ok(AgentCatalogInspection::Component(
                AgentComponentInspection {
                    workspace: workspace_value(catalog, workspace_name, workspace_root),
                    component: component.clone(),
                    targets: catalog
                        .actions()
                        .filter(|action| action.target.component == id)
                        .map(|action| agent_target_value(catalog, action))
                        .collect(),
                },
            ))
        }
        InspectRequest::Targets => Ok(AgentCatalogInspection::Targets(AgentTargetsInspection {
            workspace: workspace_value(catalog, workspace_name, workspace_root),
            targets: catalog
                .actions()
                .map(|action| agent_target_value(catalog, action))
                .collect(),
        })),
        InspectRequest::Target(target) => {
            let target: TargetId = target.parse()?;
            let Some(action) = catalog.action(&target) else {
                bail!("unknown target '{target}'");
            };
            Ok(AgentCatalogInspection::Target(AgentTargetInspection {
                workspace: workspace_value(catalog, workspace_name, workspace_root),
                target: agent_target_value(catalog, action),
            }))
        }
        InspectRequest::Profiles => Ok(AgentCatalogInspection::Profiles(ProfilesInspection {
            workspace: workspace_value(catalog, workspace_name, workspace_root),
            default_check_profile: catalog.default_check_profile().cloned(),
            profiles: catalog.profiles().cloned().collect(),
        })),
        InspectRequest::Profile(profile) => {
            let profile = ProfileId::parse(profile)?;
            let Some(spec) = catalog.profile(&profile) else {
                bail!("unknown profile '{profile}'");
            };
            Ok(AgentCatalogInspection::Profile(ProfileInspection {
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
        affected_ignore: catalog.affected_ignore().to_vec(),
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
        arguments: action.arguments.clone(),
        inputs: action.inputs.clone(),
        depends_on: action.depends_on.clone(),
        timeout_seconds: action.timeout_seconds,
        result_parser: action.result_parser,
        legacy_aliases: catalog.aliases_for_target(&action.target).to_vec(),
    }
}

fn agent_target_value(
    catalog: &RepositoryCatalog,
    action: &jig_contract::ActionSpec,
) -> AgentTargetSummary {
    AgentTargetSummary {
        id: action.target.clone(),
        description: action.description.clone(),
        intent: action.intent,
        effects: action.effects.clone(),
        runner: action.runner.clone(),
        arguments: action.arguments.clone(),
        inputs: action.inputs.clone(),
        freshness_policy: freshness_policy_value(catalog.contract_version(), action),
        depends_on: action.depends_on.clone(),
        timeout_seconds: action.timeout_seconds,
        result_parser: action.result_parser,
        legacy_aliases: catalog.aliases_for_target(&action.target).to_vec(),
    }
}

fn freshness_policy_value(
    contract_epoch: u32,
    action: &jig_contract::ActionSpec,
) -> TargetFreshnessPolicyInspectionV1 {
    if contract_epoch < TARGET_FRESHNESS_CONTRACT_VERSION {
        return TargetFreshnessPolicyInspectionV1 {
            contract_epoch,
            mode: TargetFreshnessPolicyModeV1::LegacyGlobal,
            inputs_policy: InspectedInputsPolicyV1 {
                effective: ActionInputsPolicy::WholeRepository,
                defaulted: true,
                provenance: None,
            },
            source_state: InspectedSourceStateV1 {
                effective: ActionSourceState::Git,
                defaulted: true,
                provenance: None,
            },
        };
    }

    let inputs_provenance = action.provenance.get("inputs_policy").copied();
    let source_provenance = action.provenance.get("source_state").copied();
    let inputs_policy = action.inputs_policy.unwrap_or_default();
    let source_state = action.source_state.unwrap_or_default();
    TargetFreshnessPolicyInspectionV1 {
        contract_epoch,
        mode: TargetFreshnessPolicyModeV1::TargetFreshnessV1,
        inputs_policy: InspectedInputsPolicyV1 {
            effective: inputs_policy,
            defaulted: action.inputs_policy.is_none()
                || (inputs_policy == ActionInputsPolicy::WholeRepository
                    && inputs_provenance == Some(FieldProvenance::Inferred)),
            provenance: inputs_provenance,
        },
        source_state: InspectedSourceStateV1 {
            effective: source_state,
            defaulted: action.source_state.is_none()
                || (source_state == ActionSourceState::Git
                    && source_provenance == Some(FieldProvenance::Inferred)),
            provenance: source_provenance,
        },
    }
}

#[cfg(test)]
mod tests {
    use jig_contract::{
        ActionEffect, ActionInputsPolicy, ActionIntent, ActionRunner, ActionSourceState,
        ActionSpec, ComponentId, ComponentSpec, FieldProvenance, ProfileId, ProfileSpec, TargetId,
    };

    use super::{InspectRequest, inspect_catalog, inspect_catalog_agent_v1};
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

    fn freshness_fixture() -> RepositoryCatalog {
        let component = ComponentSpec::new(ComponentId::parse("api").unwrap(), "api");
        let defaulted_target: TargetId = "api:check".parse().unwrap();
        let explicit_target: TargetId = "api:test".parse().unwrap();
        let mut defaulted = ActionSpec::new(
            defaulted_target.clone(),
            ActionIntent::Check,
            ActionRunner::Argv {
                program: "check-command".into(),
                args: Vec::new(),
                working_directory: None,
                environment: Default::default(),
            },
        );
        defaulted.effects.push(ActionEffect::ReadOnly);
        defaulted.inputs_policy = Some(ActionInputsPolicy::WholeRepository);
        defaulted.source_state = Some(ActionSourceState::Git);
        defaulted
            .provenance
            .insert("inputs_policy".into(), FieldProvenance::Inferred);
        defaulted
            .provenance
            .insert("source_state".into(), FieldProvenance::Inferred);

        let mut explicit = ActionSpec::new(
            explicit_target.clone(),
            ActionIntent::Check,
            ActionRunner::Argv {
                program: "test-command".into(),
                args: Vec::new(),
                working_directory: None,
                environment: Default::default(),
            },
        );
        explicit.effects.push(ActionEffect::ReadOnly);
        explicit.inputs.push("api/**".into());
        explicit.inputs_policy = Some(ActionInputsPolicy::Exhaustive);
        explicit.source_state = Some(ActionSourceState::Worktree);
        explicit
            .provenance
            .insert("inputs_policy".into(), FieldProvenance::Declared);
        explicit
            .provenance
            .insert("source_state".into(), FieldProvenance::Declared);

        let profile_id = ProfileId::parse("verify").unwrap();
        let profile = ProfileSpec::new(profile_id.clone(), vec![defaulted_target, explicit_target]);
        RepositoryCatalog::from_native(
            8,
            "sha256:config",
            &[component],
            &[defaulted, explicit],
            &[profile],
            Some(&profile_id),
        )
        .unwrap()
    }

    #[test]
    fn omitted_policy_fields_follow_runtime_defaults() {
        let action = jig_contract::ActionSpec::new(
            "api:test".parse().unwrap(),
            ActionIntent::Check,
            ActionRunner::command("api_test_command"),
        );
        let policy = super::freshness_policy_value(8, &action);
        assert_eq!(
            policy.inputs_policy.effective,
            ActionInputsPolicy::default()
        );
        assert_eq!(policy.source_state.effective, ActionSourceState::default());
        assert!(policy.inputs_policy.defaulted && policy.source_state.defaulted);
        assert!(policy.inputs_policy.provenance.is_none());
        assert!(policy.source_state.provenance.is_none());
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

    #[test]
    fn standard_projection_preserves_the_existing_target_shape() {
        let target = inspect_catalog(
            &freshness_fixture(),
            "ExampleProject",
            "/repo",
            InspectRequest::Target("api:test".into()),
        )
        .unwrap();

        assert!(target["target"].get("freshness_policy").is_none());
    }

    #[test]
    fn agent_projection_exposes_effective_policy_and_authored_defaulting() {
        let targets = inspect_catalog_agent_v1(
            &freshness_fixture(),
            "ExampleProject",
            "/repo",
            InspectRequest::Targets,
        )
        .unwrap();

        let defaulted = &targets["targets"][0]["freshness_policy"];
        assert_eq!(defaulted["contract_epoch"], 8);
        assert_eq!(defaulted["mode"], "target_freshness_v1");
        assert_eq!(defaulted["inputs_policy"]["effective"], "whole_repository");
        assert_eq!(defaulted["inputs_policy"]["defaulted"], true);
        assert_eq!(defaulted["inputs_policy"]["provenance"], "inferred");
        assert_eq!(defaulted["source_state"]["effective"], "git");
        assert_eq!(defaulted["source_state"]["defaulted"], true);
        assert_eq!(defaulted["source_state"]["provenance"], "inferred");

        let explicit = &targets["targets"][1]["freshness_policy"];
        assert_eq!(explicit["inputs_policy"]["effective"], "exhaustive");
        assert_eq!(explicit["inputs_policy"]["defaulted"], false);
        assert_eq!(explicit["inputs_policy"]["provenance"], "declared");
        assert_eq!(explicit["source_state"]["effective"], "worktree");
        assert_eq!(explicit["source_state"]["defaulted"], false);
        assert_eq!(explicit["source_state"]["provenance"], "declared");
        assert!(explicit.get("freshness").is_none());
        assert!(explicit.get("status").is_none());
    }

    #[test]
    fn agent_projection_marks_pre_freshness_contracts_as_legacy_global() {
        for request in [
            InspectRequest::Workspace,
            InspectRequest::Component("api".into()),
            InspectRequest::Targets,
            InspectRequest::Target("api:test".into()),
        ] {
            let output =
                inspect_catalog_agent_v1(&fixture(), "ExampleProject", "/repo", request).unwrap();
            let policy = if let Some(target) = output.get("target") {
                &target["freshness_policy"]
            } else {
                &output["targets"][0]["freshness_policy"]
            };
            assert_eq!(policy["contract_epoch"], 6);
            assert_eq!(policy["mode"], "legacy_global");
            assert_eq!(policy["inputs_policy"]["effective"], "whole_repository");
            assert_eq!(policy["inputs_policy"]["defaulted"], true);
            assert!(policy["inputs_policy"]["provenance"].is_null());
            assert_eq!(policy["source_state"]["effective"], "git");
            assert_eq!(policy["source_state"]["defaulted"], true);
            assert!(policy["source_state"]["provenance"].is_null());
        }
    }
}
