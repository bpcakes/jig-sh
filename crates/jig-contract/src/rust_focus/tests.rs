use super::*;
use serde_json::{Value, json};

#[test]
fn minimal_config_is_full_locked_offline_with_separate_optional_profiles() {
    let config: RustNextestConfigV1 =
        serde_json::from_value(json!({"workspace_manifest": "Cargo.toml"})).unwrap();
    assert!(!config.focused);
    assert_eq!(config.context, CargoImpactContextV1::default());
    assert!(config.cargo_profile.is_none());
    assert!(config.nextest_profile.is_none());
    let value = json!({
        "workspace_manifest": "example/Cargo.toml", "focused": true,
        "cargo_profile": "release", "nextest_profile": "ci"
    });
    let config: RustNextestConfigV1 = serde_json::from_value(value).unwrap();
    assert_eq!(config.cargo_profile.as_deref(), Some("release"));
    assert_eq!(config.nextest_profile.as_deref(), Some("ci"));
}

#[test]
fn focus_variants_and_named_targets_round_trip_without_opaque_ids() {
    for value in [
        json!({"kind": "automatic"}),
        json!({"kind": "automatic", "plan_id": "plan_example"}),
        json!({
            "kind": "explicit", "packages": ["example@1.2.3"],
            "targets": [
                {"kind": "lib"}, {"kind": "bin", "name": "example_cli"},
                {"kind": "test", "name": "example_integration"},
                {"kind": "example", "name": "example_usage"},
                {"kind": "bench", "name": "example_bench"}
            ],
            "features": {"features": ["serialization"], "no_default_features": true, "all_features": false},
            "filter": "test(=example)"
        }),
    ] {
        let focus: RustFocusV1 = serde_json::from_value(value.clone()).unwrap();
        let serialized = serde_json::to_value(&focus).unwrap();
        assert_eq!(serialized, value);
        assert_eq!(
            serde_json::from_value::<RustFocusV1>(serialized).unwrap(),
            focus
        );
    }
}

#[test]
fn all_focus_variants_reject_unknown_fields_tags_and_shell_argv_extensions() {
    for value in [
        json!({"kind": "future"}),
        json!({"kind": "automatic", "filter": "all()"}),
        json!({"kind": "explicit", "packages": ["example@1.0.0"], "args": ["--workspace"]}),
        json!({"kind": "explicit", "packages": ["example@1.0.0"], "targets": [{"kind": "future", "name": "example"}]}),
        json!({"kind": "explicit", "packages": ["example@1.0.0"], "targets": [{"kind": "bin", "name": "example", "args": []}]}),
        json!({"kind": "explicit", "packages": ["example@1.0.0"], "targets": [{"kind": "lib", "name": "example"}]}),
        json!({"kind": "explicit", "packages": ["example@1.0.0"], "features": {"unknown": true}}),
        json!({"kind": "explicit", "packages": "example@1.0.0"}),
        json!({"kind": "explicit"}),
    ] {
        assert!(
            serde_json::from_value::<RustFocusV1>(value.clone()).is_err(),
            "accepted {value}"
        );
    }
}

#[test]
fn runner_config_and_nested_metadata_context_are_closed_schemas() {
    for extra in ["args", "command", "shell", "future"] {
        let mut value = json!({"workspace_manifest": "Cargo.toml"});
        value[extra] = json!(true);
        assert!(serde_json::from_value::<RustNextestConfigV1>(value).is_err());
    }
    let mut context = serde_json::to_value(CargoImpactContextV1::default()).unwrap();
    context["future"] = json!(true);
    assert!(
        serde_json::from_value::<RustNextestConfigV1>(json!({
            "workspace_manifest": "Cargo.toml", "context": context
        }))
        .is_err()
    );
}

fn prepared() -> Value {
    json!({
        "schema_version": 1, "disposition": "narrowed", "reasons": [],
        "packages": ["example@1.2.3"], "targets": [{"kind": "lib"}],
        "context": CargoImpactContextV1::default(),
        "comparison_base": "example_baseline", "args": ["nextest", "run", "--package", "example@1.2.3", "--lib"]
    })
}

#[test]
fn prepared_scope_round_trips_and_rejects_unknown_authority_fields() {
    let original = prepared();
    let parsed: PreparedRustInputV1 = serde_json::from_value(original.clone()).unwrap();
    assert_eq!(serde_json::to_value(parsed).unwrap(), original);
    let mut value = prepared();
    value["raw_package_id"] = json!("path+file:///tmp/example#example@1.2.3");
    assert!(serde_json::from_value::<PreparedRustInputV1>(value).is_err());
    let mut value = prepared();
    value["disposition"] = json!("future");
    assert!(serde_json::from_value::<PreparedRustInputV1>(value).is_err());
}
