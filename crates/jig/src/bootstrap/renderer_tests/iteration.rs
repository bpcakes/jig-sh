use super::*;
use crate::context::RepoContext;

#[test]
fn generated_iteration_selection_loads_without_weakening_final_requirements() {
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
        Some(8),
    )
    .unwrap();
    let context = RepoContext::load_from_root(destination.path().to_path_buf()).unwrap();
    assert_eq!(
        context.work_iteration_profile().unwrap().as_str(),
        "iteration"
    );
    let source: toml::Value =
        toml::from_str(&fs::read_to_string(destination.path().join(".jig.toml")).unwrap()).unwrap();
    assert_eq!(
        source["work"]["gates"][0]["profile"].as_str(),
        Some("verify")
    );
    let manifest: JsonValue = serde_json::from_slice(
        &fs::read(destination.path().join(".agent/jig-contract.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(
        serde_json::to_value(&source["repository"]["profiles"]).unwrap(),
        manifest["profiles"]
    );
    assert!(
        manifest["actions"]
            .as_array()
            .unwrap()
            .iter()
            .all(|action| {
                action["inputs_policy"] == "whole_repository" && action["source_state"] == "git"
            })
    );

    // An authored model without iteration must not acquire a dangling selector.
    let path = destination.path().join(".jig.toml");
    let mut source = source;
    source["work"]
        .as_table_mut()
        .unwrap()
        .remove("iteration_profile");
    source["repository"]["profiles"]
        .as_array_mut()
        .unwrap()
        .retain(|profile| profile["id"].as_str() != Some("iteration"));
    fs::write(&path, toml::to_string(&source).unwrap()).unwrap();
    let authored = RenderAnswers::from_answers_file(&path).unwrap();
    let recopy = tempfile::tempdir().unwrap();
    render_template_files(
        &template,
        &authored,
        recopy.path(),
        Some(&selected),
        Some(8),
    )
    .unwrap();
    let context = RepoContext::load_from_root(recopy.path().to_path_buf()).unwrap();
    assert!(context.work_iteration_profile().is_none());

    // A coincidentally named project-owned profile is not a generated default.
    source["repository"]["profiles"]
        .as_array_mut()
        .unwrap()
        .push(
            toml::from_str(
                "id = 'iteration'\ntargets = [{ component = 'repo', action = 'bootstrap' }]\n",
            )
            .unwrap(),
        );
    fs::write(&path, toml::to_string(&source).unwrap()).unwrap();
    let authored = RenderAnswers::from_answers_file(&path).unwrap();
    render_template_files(
        &template,
        &authored,
        recopy.path(),
        Some(&selected),
        Some(8),
    )
    .unwrap();
    let context = RepoContext::load_from_root(recopy.path().to_path_buf()).unwrap();
    assert!(context.work_iteration_profile().is_none());
}
