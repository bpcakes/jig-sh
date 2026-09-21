use super::*;
use crate::repository::InspectRequest;

fn output_schema(ctx: &RepoContext, name: &str, surface: crate::surface::ResponseSurface) -> Value {
    crate::tool_defs::tool_descriptors_for_surface(
        ctx.contract_version(),
        ctx.tool_specs(),
        surface,
    )
    .into_iter()
    .find(|descriptor| descriptor["name"] == name)
    .unwrap()["outputSchema"]
        .clone()
}

#[test]
fn surface_selection_preserves_inputs_and_unmodified_descriptors() {
    let temp = tempdir().unwrap();
    write_v8_policy_fixture_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let standard = crate::tool_defs::tool_descriptors(ctx.contract_version(), ctx.tool_specs());
    let agent = crate::tool_defs::tool_descriptors_for_surface(
        ctx.contract_version(),
        ctx.tool_specs(),
        crate::surface::ResponseSurface::AgentV1,
    );
    assert_eq!(standard.len(), agent.len());
    for (baseline, selected) in standard.iter().zip(&agent) {
        assert_eq!(baseline["name"], selected["name"]);
        if baseline["name"] == tool::INSPECT {
            assert_eq!(baseline["inputSchema"], selected["inputSchema"]);
            assert_ne!(baseline["outputSchema"], selected["outputSchema"]);
            println!(
                "inspection descriptor bytes: standard={}, agent-v1={}",
                serde_json::to_vec(baseline).unwrap().len(),
                serde_json::to_vec(selected).unwrap().len()
            );
        } else if matches!(
            baseline["name"].as_str(),
            Some("jig.work_check" | "jig.work_gates" | "jig.work_evidence")
        ) {
            assert_eq!(baseline["inputSchema"], selected["inputSchema"]);
            assert!(baseline.get("outputSchema").is_none());
            assert!(selected["outputSchema"].is_object());
        } else {
            assert_eq!(baseline, selected);
        }
    }
}

fn replace_fixture_actions(root: &std::path::Path, manifest: &Value) {
    fs::write(
        root.join(".agent/jig-contract.json"),
        serde_json::to_string_pretty(manifest).unwrap(),
    )
    .unwrap();
    let config_path = root.join(".jig.toml");
    let mut config: toml::Value =
        toml::from_str(&fs::read_to_string(&config_path).unwrap()).unwrap();
    config["repository"]["actions"] = toml::Value::try_from(&manifest["actions"]).unwrap();
    fs::write(config_path, toml::to_string(&config).unwrap()).unwrap();
}

fn write_v8_policy_fixture_repo(root: &std::path::Path) {
    write_v6_evidence_fixture_repo(root, "");
    let manifest_path = root.join(".agent/jig-contract.json");
    let mut manifest: Value =
        serde_json::from_str(&fs::read_to_string(&manifest_path).unwrap()).unwrap();
    manifest["contract_version"] = json!(8);
    let actions = manifest["actions"].as_array_mut().unwrap();
    actions[0]["runner"] =
        json!({"kind": "argv", "program": "printf", "args": ["api tests passed\n"]});
    actions[0]["inputs_policy"] = json!("whole_repository");
    actions[0]["source_state"] = json!("git");
    actions[0]["provenance"] = json!({"inputs_policy": "inferred", "source_state": "inferred"});
    actions[1]["runner"] =
        json!({"kind": "argv", "program": "printf", "args": ["web tests passed\n"]});
    actions[1]["inputs_policy"] = json!("exhaustive");
    actions[1]["source_state"] = json!("worktree");
    actions[1]["provenance"] = json!({"inputs_policy": "declared", "source_state": "declared"});
    let mut mismatched = actions[0].clone();
    mismatched["target"] = json!({"component": "api", "action": "lint"});
    mismatched["inputs_policy"] = json!("exhaustive");
    mismatched["source_state"] = json!("worktree");
    mismatched["provenance"] = json!({"inputs_policy": "inferred", "source_state": "inferred"});
    actions.push(mismatched);
    replace_fixture_actions(root, &manifest);
}

fn catalog_payload(mut cli: Value) -> Value {
    let object = cli.as_object_mut().unwrap();
    object.remove("ok");
    object.remove("command");
    object.remove("schema_version");
    cli
}

#[test]
fn agent_surface_advertises_and_returns_its_strict_inspection_projection() {
    let temp = tempdir().unwrap();
    write_v6_evidence_fixture_repo(temp.path(), "");
    let manifest_path = temp.path().join(".agent/jig-contract.json");
    let mut manifest: Value =
        serde_json::from_str(&fs::read_to_string(&manifest_path).unwrap()).unwrap();
    for action in manifest["actions"].as_array_mut().unwrap() {
        action["provenance"] = json!({"inputs_policy": "declared", "source_state": "overridden"});
    }
    replace_fixture_actions(temp.path(), &manifest);
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let standard = call_tool(&ctx, tool::INSPECT, json!({"kind": "workspace"})).unwrap();
    let agent = call_tool_on_surface(
        &ctx,
        tool::INSPECT,
        json!({"kind": "workspace"}),
        crate::surface::ResponseSurface::AgentV1,
    )
    .unwrap();
    assert!(
        standard["result"]["targets"][0]
            .get("freshness_policy")
            .is_none()
    );
    assert_eq!(
        agent["result"]["targets"][0]["freshness_policy"]["mode"],
        "legacy_global"
    );
    assert_eq!(
        agent["result"]["targets"][0]["freshness_policy"]["inputs_policy"]["effective"],
        "whole_repository"
    );
    assert_eq!(
        agent["result"]["targets"][0]["freshness_policy"]["source_state"]["effective"],
        "git"
    );
    assert!(
        agent["result"]["targets"][0]["freshness_policy"]["inputs_policy"]["provenance"].is_null()
    );
    assert!(
        agent["result"]["targets"][0]["freshness_policy"]["source_state"]["provenance"].is_null()
    );
    let cli_agent = crate::repository::inspect_repository(
        &ctx,
        crate::repository::InspectRequest::Workspace,
        crate::surface::ResponseSurface::AgentV1,
    )
    .unwrap();
    assert_eq!(cli_agent["targets"], agent["result"]["targets"]);

    let standard_schema = output_schema(
        &ctx,
        tool::INSPECT,
        crate::surface::ResponseSurface::Standard,
    );
    let agent_schema = output_schema(
        &ctx,
        tool::INSPECT,
        crate::surface::ResponseSurface::AgentV1,
    );
    assert!(
        jsonschema::validator_for(&standard_schema)
            .unwrap()
            .is_valid(&standard)
    );
    assert!(
        jsonschema::validator_for(&agent_schema)
            .unwrap()
            .is_valid(&agent)
    );
    assert!(
        !jsonschema::validator_for(&standard_schema)
            .unwrap()
            .is_valid(&agent)
    );
    assert!(
        !jsonschema::validator_for(&agent_schema)
            .unwrap()
            .is_valid(&standard)
    );

    let standard_plan =
        crate::tool_defs::tool_descriptors(ctx.contract_version(), ctx.tool_specs())
            .into_iter()
            .find(|descriptor| descriptor["name"] == tool::PLAN_RUN)
            .unwrap();
    let agent_plan = crate::tool_defs::tool_descriptors_for_surface(
        ctx.contract_version(),
        ctx.tool_specs(),
        crate::surface::ResponseSurface::AgentV1,
    )
    .into_iter()
    .find(|descriptor| descriptor["name"] == tool::PLAN_RUN)
    .unwrap();
    assert_eq!(agent_plan, standard_plan);
}

fn assert_target_views_match_cli_and_schemas(ctx: &RepoContext) {
    let standard_schema = output_schema(
        ctx,
        tool::INSPECT,
        crate::surface::ResponseSurface::Standard,
    );
    let agent_schema = output_schema(ctx, tool::INSPECT, crate::surface::ResponseSurface::AgentV1);

    for (arguments, request) in [
        (json!({"kind": "workspace"}), InspectRequest::Workspace),
        (
            json!({"kind": "component", "id": "api"}),
            InspectRequest::Component("api".into()),
        ),
        (json!({"kind": "targets"}), InspectRequest::Targets),
        (
            json!({"kind": "target", "id": "api:test"}),
            InspectRequest::Target("api:test".into()),
        ),
    ] {
        let standard = call_tool(ctx, tool::INSPECT, arguments.clone()).unwrap();
        let agent = call_tool_on_surface(
            ctx,
            tool::INSPECT,
            arguments,
            crate::surface::ResponseSurface::AgentV1,
        )
        .unwrap();
        let cli = crate::repository::inspect_repository(
            ctx,
            request,
            crate::surface::ResponseSurface::AgentV1,
        )
        .unwrap();

        assert!(
            jsonschema::validator_for(&standard_schema)
                .unwrap()
                .is_valid(&standard),
            "{standard:#}"
        );
        assert!(
            jsonschema::validator_for(&agent_schema)
                .unwrap()
                .is_valid(&agent),
            "{agent:#}"
        );
        assert_eq!(agent["result"], catalog_payload(cli));
    }
}

fn assert_epoch_8_policies(ctx: &RepoContext) {
    let agent = call_tool_on_surface(
        ctx,
        tool::INSPECT,
        json!({"kind": "workspace"}),
        crate::surface::ResponseSurface::AgentV1,
    )
    .unwrap();
    let targets = agent["result"]["targets"].as_array().unwrap();
    let policy = |component, action| {
        &targets
            .iter()
            .find(|target| {
                target["id"]["component"] == component && target["id"]["action"] == action
            })
            .unwrap()["freshness_policy"]
    };
    let defaulted = policy("api", "test");
    assert_eq!(defaulted["inputs_policy"]["defaulted"], true);
    assert_eq!(defaulted["inputs_policy"]["provenance"], "inferred");
    assert_eq!(defaulted["source_state"]["defaulted"], true);
    assert_eq!(defaulted["source_state"]["provenance"], "inferred");
    let declared = policy("web", "test");
    assert_eq!(declared["inputs_policy"]["effective"], "exhaustive");
    assert_eq!(declared["inputs_policy"]["defaulted"], false);
    assert_eq!(declared["inputs_policy"]["provenance"], "declared");
    assert_eq!(declared["source_state"]["effective"], "worktree");
    assert_eq!(declared["source_state"]["defaulted"], false);
    assert_eq!(declared["source_state"]["provenance"], "declared");
    let mismatched = policy("api", "lint");
    assert_eq!(mismatched["inputs_policy"]["effective"], "exhaustive");
    assert_eq!(mismatched["inputs_policy"]["defaulted"], false);
    assert_eq!(mismatched["inputs_policy"]["provenance"], "inferred");
    assert_eq!(mismatched["source_state"]["effective"], "worktree");
    assert_eq!(mismatched["source_state"]["defaulted"], false);
    assert_eq!(mismatched["source_state"]["provenance"], "inferred");
    assert_eq!(defaulted["mode"], "target_freshness_v1");
    assert!(defaulted.get("freshness").is_none());
}

#[test]
fn epoch_8_target_views_match_cli_and_their_advertised_schemas() {
    let temp = tempdir().unwrap();
    write_v8_policy_fixture_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    assert_target_views_match_cli_and_schemas(&ctx);
    assert_epoch_8_policies(&ctx);
}
