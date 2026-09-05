use super::*;

#[test]
fn typed_status_contract_round_trips_live_producer_output() {
    let temp = tempdir().unwrap();
    TestRepoBuilder::new(temp.path()).write();
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    crate::state::session_start(&ctx).unwrap();
    let plan = crate::state::plans_open(
        &ctx,
        crate::state::PlanOpenRequest {
            title: "Example status plan".into(),
            body: Some("# Example status plan\n".into()),
            body_file: None,
            base: None,
        },
    )
    .unwrap();
    let plan_id = plan["plan_id"].as_str().unwrap().to_string();
    crate::state::decisions_add(
        &ctx,
        crate::state::DecisionAddRequest {
            title: "Example status decision".into(),
            selected_option: "A".into(),
            rationale: "A preserves the contract.".into(),
            alternatives: vec!["B".into()],
            plan_id: Some(plan_id),
        },
    )
    .unwrap();

    let legacy = snapshot(&ctx).unwrap();
    let typed: jig_ui::dashboard::StatusSnapshot = serde_json::from_value(legacy.clone()).unwrap();

    assert_eq!(serde_json::to_value(typed).unwrap(), legacy);
}

#[test]
fn decoder_preserves_additive_fields_and_builds_normalized_summary() {
    let mut value = report_value("complete", None);
    value["future_report_field"] = json!({"kept": true});
    value["provider"]["future_provider_field"] = json!("also kept");
    let decoded = decode_report(
        &provider(vec!["unused".into()]),
        &serde_json::to_string(&value).unwrap(),
    )
    .unwrap();

    assert_eq!(decoded.raw()["future_report_field"]["kept"], true);
    assert_eq!(
        decoded.raw()["provider"]["future_provider_field"],
        "also kept"
    );
    let summary = serde_json::to_value(ProviderSummary::from_report(decoded.decoded())).unwrap();
    let dashboard_summary = serde_json::to_value(jig_ui::dashboard::ProviderSummary::from_report(
        decoded.decoded(),
    ))
    .unwrap();
    assert_eq!(dashboard_summary, summary);
    assert_eq!(summary["work_packages"], 1);
    assert_eq!(summary["work_packages_with_blockers"], 1);
    assert_eq!(summary["blockers"], 1);
    assert_eq!(summary["implementation"]["active"], 1);
    assert_eq!(summary["verification"]["pending"], 1);
    assert_eq!(summary["acceptance"]["complete"], 1);
}
