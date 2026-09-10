use super::*;
use crate::context::RepoContext;

#[test]
fn freshness_epoch_defaults_conservatively_and_preserves_authored_assertions() {
    let template = live_template_source();
    let answers = rust_render_answers(RepositoryProjectionHint::RustWorkspace);
    let previous = render_context(&template, &answers, Some(8)).unwrap();
    assert!(
        previous["repository"]["actions"]
            .as_array()
            .unwrap()
            .iter()
            .all(|action| action.get("inputs_policy").is_none())
    );
    let current = render_context(&template, &answers, None).unwrap();
    assert_eq!(current["_jig"]["contract_version"], 9);
    let actions = current["repository"]["actions"].as_array().unwrap();
    assert!(!actions.is_empty());
    assert!(
        actions
            .iter()
            .all(|action| action["inputs_policy"] == "whole_repository"
                && action["provenance"]["inputs_policy"] == "inferred")
    );
    let source: toml::Value = toml::from_str(current["repository_toml"].as_str().unwrap()).unwrap();
    assert_eq!(
        serde_json::to_value(&source["repository"]["actions"]).unwrap(),
        current["repository"]["actions"]
    );

    let initial = tempfile::tempdir().unwrap();
    let selected = BTreeSet::from([
        PathBuf::from(".jig.toml"),
        PathBuf::from(".agent/jig-contract.json"),
    ]);
    render_template_files(
        &template,
        &answers,
        initial.path(),
        Some(&selected),
        Some(9),
    )
    .unwrap();
    let path = initial.path().join(".jig.toml");
    let mut source: toml::Value = toml::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    let action = source["repository"]["actions"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|action| {
            action["runner"]["kind"].as_str() == Some("shell")
                && action
                    .get("inputs")
                    .and_then(toml::Value::as_array)
                    .is_some_and(|inputs| !inputs.is_empty())
        })
        .unwrap();
    action["inputs_policy"] = toml::Value::String("exhaustive".into());
    action["provenance"]["inputs_policy"] = toml::Value::String("declared".into());
    let expected = source["repository"]["actions"].clone();
    fs::write(&path, toml::to_string(&source).unwrap()).unwrap();
    let authored = RenderAnswers::from_answers_file(&path).unwrap();
    assert!(render_context(&template, &authored, Some(8)).is_err());
    let recopy = tempfile::tempdir().unwrap();
    render_template_files(
        &template,
        &authored,
        recopy.path(),
        Some(&selected),
        Some(9),
    )
    .unwrap();
    let manifest: JsonValue =
        serde_json::from_slice(&fs::read(recopy.path().join(".agent/jig-contract.json")).unwrap())
            .unwrap();
    assert_eq!(manifest["contract_version"], 9);
    assert_eq!(manifest["actions"], serde_json::to_value(expected).unwrap());
}

#[test]
fn epoch_nine_loader_normalizes_omitted_policy_without_losing_other_authority() {
    let template = live_template_source();
    let answers = rust_render_answers(RepositoryProjectionHint::RustWorkspace);
    let destination = tempfile::tempdir().unwrap();
    let selected = BTreeSet::from([
        PathBuf::from(".jig.toml"),
        PathBuf::from(".agent/jig-contract.json"),
    ]);
    render_template_files(
        &template,
        &answers,
        destination.path(),
        Some(&selected),
        Some(9),
    )
    .unwrap();
    let path = destination.path().join(".jig.toml");
    let mut source: toml::Value = toml::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    source["repository"]["actions"].as_array_mut().unwrap()[0]
        .as_table_mut()
        .unwrap()
        .remove("inputs_policy");
    fs::write(&path, toml::to_string(&source).unwrap()).unwrap();
    let context = RepoContext::load_from_root(destination.path().to_path_buf()).unwrap();
    assert_eq!(context.contract_version(), 9);
    source["repository"]["actions"].as_array_mut().unwrap()[0]
        .as_table_mut()
        .unwrap()
        .insert(
            "inputs_policy".into(),
            toml::Value::String("exhaustive".into()),
        );
    fs::write(&path, toml::to_string(&source).unwrap()).unwrap();
    assert!(RepoContext::load_from_root(destination.path().to_path_buf()).is_err());
}
