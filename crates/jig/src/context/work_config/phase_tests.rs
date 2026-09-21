use super::*;

#[test]
fn iteration_profile_is_optional_and_omitted_when_disabled() {
    let default = WorkConfig::default();
    assert!(default.iteration_profile().is_none());
    assert!(
        !toml::to_string(&default)
            .unwrap()
            .contains("iteration_profile")
    );

    let configured = toml::from_str::<WorkConfig>("iteration_profile = \"iteration\"\n").unwrap();
    assert_eq!(
        configured
            .iteration_profile()
            .map(|profile| profile.as_str()),
        Some("iteration")
    );
    assert!(
        toml::to_string(&configured)
            .unwrap()
            .contains("iteration_profile = \"iteration\"")
    );
}
