use super::*;

pub(super) fn waived_policy(
    id: &str,
    path: &str,
    ceiling_lines: u64,
    ceiling_bytes: Option<u64>,
) -> String {
    let byte_ceiling =
        ceiling_bytes.map_or_else(String::new, |value| format!("ceiling_bytes = {value}\n"));
    format!(
        r#"
version = 1
[[rules]]
id = "source"
include = ["**/*"]
max_lines = 10
max_bytes = 100
[[waivers]]
id = "{id}"
rule = "source"
path = "{path}"
ceiling_lines = {ceiling_lines}
{byte_ceiling}reason = "bounded extraction"
expires = 2027-01-01
"#
    )
}

#[test]
fn active_waivers_are_exact_bounded_visible_and_do_not_cover_omitted_metrics() {
    let policy = policy(&waived_policy("legacy", "src/legacy.rs", 20, None));
    let targets = [regular_target("src/legacy.rs")];
    let authorized = evaluate_v1(EvaluationInputV1 {
        policy: &policy,
        comparison_policy: ComparisonPolicyV1::Absent,
        current_date: date(2026, 8, 30),
        waiver_targets: &targets,
        files: &[file("src/legacy.rs", 15, 90, Some((12, 90)))],
    });
    assert!(authorized.passed());
    assert_eq!(authorized.waived_files, 1);
    assert!(codes(&authorized).contains(&BudgetDiagnosticCodeV1::WaiverActive));

    let above_ceiling = evaluate_v1(EvaluationInputV1 {
        policy: &policy,
        comparison_policy: ComparisonPolicyV1::Absent,
        current_date: date(2026, 8, 30),
        waiver_targets: &targets,
        files: &[file("src/legacy.rs", 21, 90, Some((12, 90)))],
    });
    assert!(!above_ceiling.passed());
    assert!(codes(&above_ceiling).contains(&BudgetDiagnosticCodeV1::MaxLines));

    let bytes_not_waived = evaluate_v1(EvaluationInputV1 {
        policy: &policy,
        comparison_policy: ComparisonPolicyV1::Absent,
        current_date: date(2026, 8, 30),
        waiver_targets: &targets,
        files: &[file("src/legacy.rs", 15, 110, Some((12, 100)))],
    });
    assert!(!bytes_not_waived.passed());
    assert!(codes(&bytes_not_waived).contains(&BudgetDiagnosticCodeV1::DebtGrowthBytes));

    let other_path = evaluate_v1(EvaluationInputV1 {
        policy: &policy,
        comparison_policy: ComparisonPolicyV1::Absent,
        current_date: date(2026, 8, 30),
        waiver_targets: &targets,
        files: &[file("src/other.rs", 15, 90, None)],
    });
    assert!(!other_path.passed());
    assert!(codes(&other_path).contains(&BudgetDiagnosticCodeV1::MaxLines));
}

#[test]
fn a_waiver_ceiling_below_the_ordinary_maximum_is_still_an_exact_ceiling() {
    let policy = policy(&waived_policy("legacy", "src/legacy.rs", 8, None));
    let result = evaluate_v1(EvaluationInputV1 {
        policy: &policy,
        comparison_policy: ComparisonPolicyV1::Absent,
        current_date: date(2026, 8, 30),
        waiver_targets: &[regular_target("src/legacy.rs")],
        files: &[file("src/legacy.rs", 9, 80, Some((9, 80)))],
    });
    assert!(!result.passed());
    let diagnostic = result
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == BudgetDiagnosticCodeV1::MaxLines)
        .unwrap();
    assert_eq!(diagnostic.limit, Some(8));
    assert_eq!(diagnostic.waiver_id.as_deref(), Some("legacy"));
}

#[test]
fn removing_one_waiver_metric_cannot_launder_that_debt_coordinate() {
    let historical = parse_comparison_policy_v1(
        waived_policy("legacy", "src/legacy.rs", 20, Some(200)).as_bytes(),
    )
    .unwrap();
    let current = policy(
        r#"version=1
[[rules]]
id="source"
include=["**/*"]
max_lines=10
max_bytes=100
[[waivers]]
id="legacy"
rule="source"
path="src/legacy.rs"
ceiling_bytes=200
reason="line authorization removed"
expires=2027-01-01
"#,
    );
    let result = evaluate_v1(EvaluationInputV1 {
        policy: &current,
        comparison_policy: ComparisonPolicyV1::Present(&historical),
        current_date: date(2026, 8, 30),
        waiver_targets: &[regular_target("src/legacy.rs")],
        files: &[file("src/legacy.rs", 15, 90, Some((15, 90)))],
    });
    assert!(!result.passed());
    assert!(codes(&result).contains(&BudgetDiagnosticCodeV1::WaiverRemovedWithDebt));
    let diagnostic = result
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == BudgetDiagnosticCodeV1::WaiverRemovedWithDebt)
        .unwrap();
    assert_eq!(diagnostic.metric, Some(BudgetMetricV1::Lines));
    assert_eq!(diagnostic.current, Some(15));
    assert_eq!(diagnostic.comparison, Some(15));
    assert_eq!(diagnostic.limit, Some(10));
    assert_eq!(diagnostic.debt, Some(5));
}

#[test]
fn waiver_target_facts_fail_closed_without_repository_access() {
    let policy = policy(&waived_policy("legacy", "src/legacy.rs", 20, None));
    let cases = [
        Vec::new(),
        vec![ExactCurrentPathFactV1 {
            path: "src/legacy.rs".to_owned(),
            state: ExactCurrentPathStateV1::Missing,
        }],
        vec![ExactCurrentPathFactV1 {
            path: "src/legacy.rs".to_owned(),
            state: ExactCurrentPathStateV1::Unsupported(UnsupportedFileKindV1::Symlink),
        }],
        vec![
            regular_target("src/legacy.rs"),
            regular_target("src/legacy.rs"),
        ],
    ];
    for targets in cases {
        let result = evaluate_v1(EvaluationInputV1 {
            policy: &policy,
            comparison_policy: ComparisonPolicyV1::Absent,
            current_date: date(2026, 8, 30),
            waiver_targets: &targets,
            files: &[file("src/legacy.rs", 15, 80, Some((15, 80)))],
        });
        assert!(!result.passed());
        assert!(codes(&result).contains(&BudgetDiagnosticCodeV1::WaiverInvalid));
        assert!(!codes(&result).contains(&BudgetDiagnosticCodeV1::WaiverActive));
    }

    let extra = evaluate_v1(EvaluationInputV1 {
        policy: &policy,
        comparison_policy: ComparisonPolicyV1::Absent,
        current_date: date(2026, 8, 30),
        waiver_targets: &[
            regular_target("src/legacy.rs"),
            regular_target("src/not-a-waiver.rs"),
        ],
        files: &[file("src/legacy.rs", 15, 80, Some((15, 80)))],
    });
    assert!(!extra.passed());
    assert!(codes(&extra).contains(&BudgetDiagnosticCodeV1::ScopeIncomplete));
}

#[test]
fn a_prepared_current_waiver_expiring_before_evaluation_fails_deterministically() {
    let policy = parse_policy_v1(
        waived_policy("legacy", "src/legacy.rs", 20, None).as_bytes(),
        date(2026, 8, 30),
    )
    .unwrap();
    let result = evaluate_v1(EvaluationInputV1 {
        policy: &policy,
        comparison_policy: ComparisonPolicyV1::Absent,
        current_date: date(2027, 1, 2),
        waiver_targets: &[regular_target("src/legacy.rs")],
        files: &[file("src/legacy.rs", 15, 80, Some((15, 80)))],
    });
    assert!(!result.passed());
    assert!(codes(&result).contains(&BudgetDiagnosticCodeV1::WaiverExpired));
    assert!(!codes(&result).contains(&BudgetDiagnosticCodeV1::WaiverActive));
}

#[test]
fn comparison_waiver_expiry_does_not_erase_authorization_history() {
    let historical =
        waived_policy("old-id", "src/legacy.rs", 20, None).replace("2027-01-01", "2025-01-01");
    let historical = parse_comparison_policy_v1(historical.as_bytes()).unwrap();
    let current = line_policy(10);
    let result = evaluate_v1(EvaluationInputV1 {
        policy: &current,
        comparison_policy: ComparisonPolicyV1::Present(&historical),
        current_date: date(2026, 8, 30),
        waiver_targets: &[regular_target("src/legacy.rs")],
        files: &[file("src/legacy.rs", 15, 15, Some((15, 15)))],
    });
    assert!(!result.passed());
    assert!(
        codes(&result).contains(&BudgetDiagnosticCodeV1::WaiverRemovedWithDebt),
        "diagnostics: {:?}",
        result.diagnostics
    );
}

#[test]
fn id_replacement_cannot_launder_historical_waiver_debt() {
    let historical =
        parse_comparison_policy_v1(waived_policy("old-id", "src/legacy.rs", 20, None).as_bytes())
            .unwrap();
    let current = policy(&waived_policy("new-id", "src/legacy.rs", 20, None));
    let result = evaluate_v1(EvaluationInputV1 {
        policy: &current,
        comparison_policy: ComparisonPolicyV1::Present(&historical),
        current_date: date(2026, 8, 30),
        waiver_targets: &[regular_target("src/legacy.rs")],
        files: &[file("src/legacy.rs", 15, 80, Some((15, 80)))],
    });
    assert!(!result.passed());
    assert!(codes(&result).contains(&BudgetDiagnosticCodeV1::WaiverActive));
    assert!(codes(&result).contains(&BudgetDiagnosticCodeV1::WaiverRemovedWithDebt));
}

#[test]
fn same_id_path_transfer_requires_real_ancestry_and_is_visible() {
    let historical =
        parse_comparison_policy_v1(waived_policy("legacy", "src/old.rs", 20, None).as_bytes())
            .unwrap();
    let current = policy(&waived_policy("legacy", "src/new.rs", 20, None));
    let renamed = EvaluateFileV1 {
        current_path: "src/new.rs".to_owned(),
        baseline_path: Some("src/old.rs".to_owned()),
        current: CurrentFileStateV1::Regular(MeasurementV1 {
            lines: 15,
            bytes: 80,
        }),
        comparison: Some(MeasurementV1 {
            lines: 15,
            bytes: 80,
        }),
    };
    let result = evaluate_v1(EvaluationInputV1 {
        policy: &current,
        comparison_policy: ComparisonPolicyV1::Present(&historical),
        current_date: date(2026, 8, 30),
        waiver_targets: &[regular_target("src/new.rs")],
        files: &[renamed],
    });
    assert!(result.passed(), "diagnostics: {:?}", result.diagnostics);
    assert!(codes(&result).contains(&BudgetDiagnosticCodeV1::WaiverActive));
    assert!(codes(&result).contains(&BudgetDiagnosticCodeV1::PolicyChanged));
    assert!(!codes(&result).contains(&BudgetDiagnosticCodeV1::WaiverRemovedWithDebt));

    let old_still_exists = evaluate_v1(EvaluationInputV1 {
        policy: &current,
        comparison_policy: ComparisonPolicyV1::Present(&historical),
        current_date: date(2026, 8, 30),
        waiver_targets: &[regular_target("src/new.rs"), regular_target("src/old.rs")],
        files: &[
            file("src/old.rs", 15, 80, Some((15, 80))),
            file("src/new.rs", 1, 1, None),
        ],
    });
    assert!(!old_still_exists.passed());
    assert!(codes(&old_still_exists).contains(&BudgetDiagnosticCodeV1::WaiverRemovedWithDebt));
}

#[test]
fn removed_waiver_targets_require_explicit_current_view_and_measurement_facts() {
    let historical =
        parse_comparison_policy_v1(waived_policy("legacy", "src/legacy.rs", 20, None).as_bytes())
            .unwrap();
    let current = line_policy(10);
    let evaluate = |targets: &[ExactCurrentPathFactV1], files: &[EvaluateFileV1]| {
        evaluate_v1(EvaluationInputV1 {
            policy: &current,
            comparison_policy: ComparisonPolicyV1::Present(&historical),
            current_date: date(2026, 8, 30),
            waiver_targets: targets,
            files,
        })
    };

    let absent_observation = evaluate(&[], &[]);
    assert!(!absent_observation.passed());
    assert!(codes(&absent_observation).contains(&BudgetDiagnosticCodeV1::WaiverInvalid));

    let omitted_measurement = evaluate(&[regular_target("src/legacy.rs")], &[]);
    assert!(!omitted_measurement.passed());
    assert!(codes(&omitted_measurement).contains(&BudgetDiagnosticCodeV1::ScopeIncomplete));

    let removed_file = evaluate(&[missing_target("src/legacy.rs")], &[]);
    assert!(
        removed_file.passed(),
        "diagnostics: {:?}",
        removed_file.diagnostics
    );
    assert!(
        codes(&removed_file)
            .iter()
            .all(|code| *code == BudgetDiagnosticCodeV1::PolicyChanged)
    );

    let unchanged_debt = evaluate(
        &[regular_target("src/legacy.rs")],
        &[file("src/legacy.rs", 15, 15, Some((15, 15)))],
    );
    assert!(!unchanged_debt.passed());
    assert!(codes(&unchanged_debt).contains(&BudgetDiagnosticCodeV1::WaiverRemovedWithDebt));
}
