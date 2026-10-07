use super::*;

#[test]
fn retired_work_settings_are_accepted_without_changing_authority() {
    let configured = toml::from_str::<WorkConfig>(
        r#"
iteration_profile = "iteration"

[[refinements]]
id = "rust-simplify"
skill = "jig-rust:rust-simplify"

[[refinements]]
id = "second-refinement"
"#,
    )
    .unwrap();
    configured.validate().unwrap();

    // Retired values never reach execution authority: the serialized shape
    // matches a configuration that never set them.
    assert_eq!(
        serde_json::to_value(&configured).unwrap(),
        serde_json::to_value(WorkConfig::default()).unwrap()
    );
}
