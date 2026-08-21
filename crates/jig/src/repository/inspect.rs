use anyhow::{Result, bail};
use jig_contract::{ComponentId, ProfileId, TargetId};
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

pub(crate) fn inspect_repository(ctx: &RepoContext, request: InspectRequest) -> Result<Value> {
    let catalog = RepositoryCatalog::from_context(ctx)?;
    inspect_catalog(
        &catalog,
        ctx.repo_name(),
        &ctx.root().display().to_string(),
        request,
    )
}

fn inspect_catalog(
    catalog: &RepositoryCatalog,
    workspace_name: &str,
    workspace_root: &str,
    request: InspectRequest,
) -> Result<Value> {
    match request {
        InspectRequest::Workspace => Ok(json!({
            "ok": true,
            "command": "info workspace",
            "schema_version": 1,
            "workspace": workspace_value(catalog, workspace_name, workspace_root),
            "components": catalog.components().collect::<Vec<_>>(),
            "targets": catalog.actions().map(|action| target_value(catalog, action)).collect::<Vec<_>>(),
            "profiles": catalog.profiles().collect::<Vec<_>>(),
        })),
        InspectRequest::Components => Ok(json!({
            "ok": true,
            "command": "info components",
            "schema_version": 1,
            "workspace": workspace_value(catalog, workspace_name, workspace_root),
            "components": catalog.components().map(|component| component_value(catalog, component)).collect::<Vec<_>>(),
        })),
        InspectRequest::Component(id) => {
            let id = ComponentId::parse(id)?;
            let Some(component) = catalog.component(&id) else {
                bail!("unknown component '{id}'");
            };
            Ok(json!({
                "ok": true,
                "command": "info component",
                "schema_version": 1,
                "workspace": workspace_value(catalog, workspace_name, workspace_root),
                "component": component,
                "targets": catalog.actions().filter(|action| action.target.component == id).map(|action| target_value(catalog, action)).collect::<Vec<_>>(),
            }))
        }
        InspectRequest::Targets => Ok(json!({
            "ok": true,
            "command": "info targets",
            "schema_version": 1,
            "workspace": workspace_value(catalog, workspace_name, workspace_root),
            "targets": catalog.actions().map(|action| target_value(catalog, action)).collect::<Vec<_>>(),
        })),
        InspectRequest::Target(target) => {
            let target: TargetId = target.parse()?;
            let Some(action) = catalog.action(&target) else {
                bail!("unknown target '{target}'");
            };
            Ok(json!({
                "ok": true,
                "command": "info target",
                "schema_version": 1,
                "workspace": workspace_value(catalog, workspace_name, workspace_root),
                "target": target_value(catalog, action),
            }))
        }
        InspectRequest::Profiles => Ok(json!({
            "ok": true,
            "command": "info profiles",
            "schema_version": 1,
            "workspace": workspace_value(catalog, workspace_name, workspace_root),
            "default_check_profile": catalog.default_check_profile(),
            "profiles": catalog.profiles().collect::<Vec<_>>(),
        })),
        InspectRequest::Profile(profile) => {
            let profile = ProfileId::parse(profile)?;
            let Some(spec) = catalog.profile(&profile) else {
                bail!("unknown profile '{profile}'");
            };
            Ok(json!({
                "ok": true,
                "command": "info profile",
                "schema_version": 1,
                "workspace": workspace_value(catalog, workspace_name, workspace_root),
                "profile": spec,
                "is_default_check_profile": catalog.default_check_profile() == Some(&profile),
            }))
        }
    }
}

fn workspace_value(
    catalog: &RepositoryCatalog,
    workspace_name: &str,
    workspace_root: &str,
) -> Value {
    json!({
        "name": workspace_name,
        "root": workspace_root,
        "contract_version": catalog.contract_version(),
        "config_digest": catalog.config_digest(),
        "default_check_profile": catalog.default_check_profile(),
    })
}

fn component_value(catalog: &RepositoryCatalog, component: &jig_contract::ComponentSpec) -> Value {
    json!({
        "component": component,
        "target_count": catalog.actions().filter(|action| action.target.component == component.id).count(),
    })
}

fn target_value(catalog: &RepositoryCatalog, action: &jig_contract::ActionSpec) -> Value {
    json!({
        "id": action.target,
        "description": action.description,
        "intent": action.intent,
        "effects": action.effects,
        "runner": action.runner,
        "inputs": action.inputs,
        "depends_on": action.depends_on,
        "timeout_seconds": action.timeout_seconds,
        "result_parser": action.result_parser,
        "legacy_aliases": catalog.aliases_for_target(&action.target),
    })
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
