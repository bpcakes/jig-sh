use jig_contract::status_provider::{V1_PROTOCOL, V1_SCHEMA_ID, v1};
use serde_json::{Value, json};

const COMMITTED_SCHEMA: &str = include_str!("../contracts/status-provider/v1.schema.json");
const CONFORMING_EXAMPLE: &str = include_str!("../contracts/status-provider/v1.example.json");

fn parsed_schema() -> Value {
    serde_json::from_str(COMMITTED_SCHEMA).expect("committed v1 schema must be JSON")
}

fn parsed_example() -> Value {
    serde_json::from_str(CONFORMING_EXAMPLE).expect("committed v1 example must be JSON")
}

#[test]
fn generated_schema_matches_committed_contract() {
    let generated = v1::schema().to_value();
    let committed = parsed_schema();

    assert_eq!(
        generated, committed,
        "status-provider schema drifted; inspect compatibility, then regenerate with \
         `cargo run -q -p jig-contract --example status_provider_schema`"
    );
    assert_eq!(committed["$id"], V1_SCHEMA_ID);
    assert_eq!(
        committed["$schema"],
        "https://json-schema.org/draft/2020-12/schema"
    );
}

#[test]
fn committed_example_conforms_to_schema_and_semantic_rules() {
    let schema = parsed_schema();
    let example = parsed_example();
    let validator = jsonschema::validator_for(&schema).expect("v1 schema must compile");

    validator
        .validate(&example)
        .expect("committed example must satisfy the v1 JSON Schema");
    let report: v1::Report =
        serde_json::from_value(example).expect("committed example must deserialize");
    report
        .validate()
        .expect("committed example must satisfy semantic rules");

    assert_eq!(report.work_packages.len(), 1);
    assert_eq!(report.work_packages[0].id, "WP-0042");
}

#[test]
fn v1_accepts_compatible_unknown_fields() {
    let schema = parsed_schema();
    let validator = jsonschema::validator_for(&schema).expect("v1 schema must compile");
    let mut example = parsed_example();

    example["future_report_field"] = json!({"added": "later"});
    example["provider"]["future_provider_field"] = json!(true);
    example["work_packages"][0]["future_package_field"] = json!(["compatible"]);

    validator
        .validate(&example)
        .expect("v1 must tolerate additive unknown fields");
    let report: v1::Report =
        serde_json::from_value(example).expect("Rust consumer must ignore unknown fields");
    report
        .validate()
        .expect("unknown fields must not alter semantic validation");
}

#[test]
fn v1_rejects_a_different_protocol_major() {
    let schema = parsed_schema();
    let validator = jsonschema::validator_for(&schema).expect("v1 schema must compile");
    let mut example = parsed_example();
    example["protocol"] = json!("jig.status-provider/v2");

    assert!(!validator.is_valid(&example));
    let error = serde_json::from_value::<v1::Report>(example)
        .expect_err("v1 Rust type must reject a v2 protocol marker");
    assert!(error.to_string().contains(V1_PROTOCOL));
}

#[test]
fn constructors_emit_the_normative_protocol_marker() {
    let report = v1::Report::complete(v1::Provider::new("example.provider", "1.2.3"), 1);
    let value = serde_json::to_value(report).expect("constructed report must serialize");

    assert_eq!(value["protocol"], V1_PROTOCOL);
    assert_eq!(value["outcome"], "complete");
}

#[test]
fn semantic_validation_reports_all_cross_field_errors() {
    let mut report: v1::Report =
        serde_json::from_value(parsed_example()).expect("example must deserialize");
    report.provider.id = " ".to_string();
    report.inputs[1].path = Some("../legacy".to_string());
    report.work_packages[0].acceptance_checks[0].ordinal = 0;
    let duplicate_check = report.work_packages[0].acceptance_checks[0].clone();
    report.work_packages[0]
        .acceptance_checks
        .push(duplicate_check);
    report.work_packages[0]
        .specification
        .source
        .as_mut()
        .expect("example specification has a source")
        .line = Some(0);
    report.work_packages.push(report.work_packages[0].clone());

    let errors = report
        .validate()
        .expect_err("mutated report must fail semantic validation");
    let paths = errors
        .errors()
        .iter()
        .map(|error| error.path.as_str())
        .collect::<Vec<_>>();

    assert!(paths.contains(&"/provider/id"));
    assert!(paths.contains(&"/inputs/1/path"));
    assert!(paths.contains(&"/work_packages/0/acceptance_checks/0/ordinal"));
    assert!(paths.contains(&"/work_packages/0/acceptance_checks/1/ordinal"));
    assert!(paths.contains(&"/work_packages/0/specification/source/line"));
    assert!(paths.contains(&"/work_packages/1/id"));
    assert!(errors.errors().len() >= 6);
}

#[test]
fn semantic_validation_rejects_unsafe_source_path_forms() {
    for unsafe_path in [
        "/absolute/path",
        "../parent",
        "docs/./package.yml",
        "docs//package.yml",
        r"docs\package.yml",
    ] {
        let mut report: v1::Report =
            serde_json::from_value(parsed_example()).expect("example must deserialize");
        report.work_packages[0]
            .specification
            .source
            .as_mut()
            .expect("example specification has a source")
            .path = unsafe_path.to_string();

        let errors = report
            .validate()
            .expect_err("unsafe path must fail semantic validation")
            .into_errors();
        assert!(
            errors
                .iter()
                .any(|error| error.path == "/work_packages/0/specification/source/path"),
            "path {unsafe_path:?} should fail, got {errors:?}"
        );
    }
}
