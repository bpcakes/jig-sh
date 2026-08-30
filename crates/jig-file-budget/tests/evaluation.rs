mod common;
#[path = "evaluation/waiver_cases.rs"]
mod waiver_cases;

use jig_file_budget::{
    BudgetDiagnosticCodeV1, BudgetMetricV1, BudgetSeverityV1, ComparisonPolicyV1,
    CurrentFileStateV1, EvaluateFileV1, EvaluationInputV1, ExactCurrentPathFactV1,
    ExactCurrentPathStateV1, MAX_WAIVER_TARGET_FACTS_V1, MeasurementV1, UnsupportedFileKindV1,
    evaluate_v1, parse_comparison_policy_v1, parse_policy_v1,
};

use common::{date, line_policy, policy, regular_target, two_metric_policy};
use waiver_cases::waived_policy;

fn file(path: &str, lines: u64, bytes: u64, comparison: Option<(u64, u64)>) -> EvaluateFileV1 {
    EvaluateFileV1 {
        current_path: path.to_owned(),
        baseline_path: comparison.map(|_| path.to_owned()),
        current: CurrentFileStateV1::Regular(MeasurementV1 { lines, bytes }),
        comparison: comparison.map(|(lines, bytes)| MeasurementV1 { lines, bytes }),
    }
}

fn codes(result: &jig_file_budget::BudgetEvaluationV1) -> Vec<BudgetDiagnosticCodeV1> {
    result
        .diagnostics
        .iter()
        .map(|diagnostic| diagnostic.code)
        .collect()
}

fn missing_target(path: &str) -> ExactCurrentPathFactV1 {
    ExactCurrentPathFactV1 {
        path: path.to_owned(),
        state: ExactCurrentPathStateV1::Missing,
    }
}

#[test]
fn evaluates_new_files_and_thresholds_independently() {
    let policy = two_metric_policy("");
    let compliant = evaluate_v1(EvaluationInputV1 {
        policy: &policy,
        comparison_policy: ComparisonPolicyV1::Absent,
        current_date: date(2026, 8, 30),
        waiver_targets: &[],
        files: &[file("src/new.rs", 4, 40, None)],
    });
    assert!(compliant.passed());
    assert!(compliant.diagnostics.is_empty());

    let result = evaluate_v1(EvaluationInputV1 {
        policy: &policy,
        comparison_policy: ComparisonPolicyV1::Absent,
        current_date: date(2026, 8, 30),
        waiver_targets: &[],
        files: &[
            file("src/near.rs", 7, 61, None),
            file("src/large.rs", 11, 101, None),
        ],
    });
    assert!(!result.passed());
    for code in [
        BudgetDiagnosticCodeV1::NoticeLines,
        BudgetDiagnosticCodeV1::WarningLines,
        BudgetDiagnosticCodeV1::NoticeBytes,
        BudgetDiagnosticCodeV1::WarningBytes,
        BudgetDiagnosticCodeV1::MaxLines,
        BudgetDiagnosticCodeV1::MaxBytes,
    ] {
        assert!(codes(&result).contains(&code), "missing {code:?}");
    }
}

#[test]
fn legacy_debt_may_hold_or_shrink_but_not_grow() {
    let policy = line_policy(10);
    let cases = [
        (15, Some(15), true, BudgetDiagnosticCodeV1::LegacyDebt),
        (14, Some(15), true, BudgetDiagnosticCodeV1::DebtImproved),
        (10, Some(15), true, BudgetDiagnosticCodeV1::DebtImproved),
        (16, Some(15), false, BudgetDiagnosticCodeV1::DebtGrowthLines),
        (11, Some(10), false, BudgetDiagnosticCodeV1::DebtGrowthLines),
        (11, None, false, BudgetDiagnosticCodeV1::MaxLines),
    ];
    for (current, comparison, expected_pass, expected_code) in cases {
        let fact = file(
            "src/lib.rs",
            current,
            current,
            comparison.map(|lines| (lines, lines)),
        );
        let result = evaluate_v1(EvaluationInputV1 {
            policy: &policy,
            comparison_policy: ComparisonPolicyV1::Absent,
            current_date: date(2026, 8, 30),
            waiver_targets: &[],
            files: &[fact],
        });
        assert_eq!(
            result.passed(),
            expected_pass,
            "case {current:?}/{comparison:?}"
        );
        assert!(codes(&result).contains(&expected_code));
    }
}

#[test]
fn improving_one_coordinate_never_authorizes_growth_in_the_other() {
    let policy = two_metric_policy("");
    let result = evaluate_v1(EvaluationInputV1 {
        policy: &policy,
        comparison_policy: ComparisonPolicyV1::Absent,
        current_date: date(2026, 8, 30),
        waiver_targets: &[],
        files: &[file("src/lib.rs", 15, 160, Some((20, 150)))],
    });
    assert!(!result.passed());
    assert!(codes(&result).contains(&BudgetDiagnosticCodeV1::DebtImproved));
    assert!(codes(&result).contains(&BudgetDiagnosticCodeV1::DebtGrowthBytes));
    let growth = result
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == BudgetDiagnosticCodeV1::DebtGrowthBytes)
        .unwrap();
    assert_eq!(growth.metric, Some(BudgetMetricV1::Bytes));
    assert_eq!(growth.debt_growth, Some(10));
}

#[test]
fn comparison_rules_and_limits_never_override_current_policy() {
    let comparison = parse_comparison_policy_v1(
        b"version=1\n[[rules]]\nid=\"old\"\ninclude=[\"**/*.txt\"]\nmax_lines=100\n",
    )
    .unwrap();
    let current = line_policy(10);
    let result = evaluate_v1(EvaluationInputV1 {
        policy: &current,
        comparison_policy: ComparisonPolicyV1::Present(&comparison),
        current_date: date(2026, 8, 30),
        waiver_targets: &[],
        files: &[file("src/lib.rs", 11, 11, Some((11, 11)))],
    });
    assert!(result.passed());
    assert!(codes(&result).contains(&BudgetDiagnosticCodeV1::LegacyDebt));
    assert!(codes(&result).contains(&BudgetDiagnosticCodeV1::PolicyChanged));
}

#[test]
fn missing_comparison_policy_is_an_explicit_visible_policy_addition() {
    let current = line_policy(10);
    let result = evaluate_v1(EvaluationInputV1 {
        policy: &current,
        comparison_policy: ComparisonPolicyV1::Absent,
        current_date: date(2026, 8, 30),
        waiver_targets: &[],
        files: &[file("src/lib.rs", 8, 8, Some((8, 8)))],
    });
    assert!(result.passed());
    assert_eq!(codes(&result), [BudgetDiagnosticCodeV1::PolicyChanged]);
}

#[test]
fn unavailable_comparison_policy_fails_without_discarding_historical_authority() {
    let current = line_policy(10);
    let result = evaluate_v1(EvaluationInputV1 {
        policy: &current,
        comparison_policy: ComparisonPolicyV1::Unavailable,
        current_date: date(2026, 8, 30),
        waiver_targets: &[],
        files: &[],
    });
    assert!(!result.passed());
    assert_eq!(
        codes(&result),
        [BudgetDiagnosticCodeV1::BaselineUnavailable]
    );
}

#[test]
fn a_same_id_rule_reassignment_is_visible_and_uses_only_the_current_rule() {
    let historical =
        parse_comparison_policy_v1(waived_policy("legacy", "src/legacy.rs", 20, None).as_bytes())
            .unwrap();
    let current = policy(
        r#"version=1
[[rules]]
id="replacement"
include=["**/*"]
max_lines=5
max_bytes=100
[[waivers]]
id="legacy"
rule="replacement"
path="src/legacy.rs"
ceiling_lines=20
reason="rule ownership moved"
expires=2027-01-01
"#,
    );
    let result = evaluate_v1(EvaluationInputV1 {
        policy: &current,
        comparison_policy: ComparisonPolicyV1::Present(&historical),
        current_date: date(2026, 8, 30),
        waiver_targets: &[regular_target("src/legacy.rs")],
        files: &[file("src/legacy.rs", 15, 80, Some((15, 80)))],
    });
    assert!(result.passed(), "diagnostics: {:?}", result.diagnostics);
    assert!(codes(&result).contains(&BudgetDiagnosticCodeV1::WaiverActive));
    assert!(codes(&result).contains(&BudgetDiagnosticCodeV1::PolicyChanged));
    assert!(!codes(&result).contains(&BudgetDiagnosticCodeV1::WaiverRemovedWithDebt));
}

#[test]
fn authorized_relaxations_and_exclusions_change_results_but_remain_visible() {
    let strict = parse_comparison_policy_v1(
        b"version=1\n[[rules]]\nid=\"source\"\ninclude=[\"**/*.rs\",\"*.rs\"]\nmax_lines=10\n",
    )
    .unwrap();
    let relaxed = line_policy(20);
    let result = evaluate_v1(EvaluationInputV1 {
        policy: &relaxed,
        comparison_policy: ComparisonPolicyV1::Present(&strict),
        current_date: date(2026, 8, 30),
        waiver_targets: &[],
        files: &[file("src/lib.rs", 15, 15, Some((15, 15)))],
    });
    assert!(result.passed());
    assert_eq!(codes(&result), [BudgetDiagnosticCodeV1::PolicyChanged]);

    let excluded = policy(
        r#"version=1
[[rules]]
id="source"
include=["**/*.rs"]
max_lines=10
[[exclusions]]
pattern="vendor/**"
kind="vendored"
reason="upstream source"
"#,
    );
    let result = evaluate_v1(EvaluationInputV1 {
        policy: &excluded,
        comparison_policy: ComparisonPolicyV1::Present(&strict),
        current_date: date(2026, 8, 30),
        waiver_targets: &[],
        files: &[file("vendor/lib.rs", 100, 100, Some((100, 100)))],
    });
    assert!(result.passed());
    assert_eq!(result.excluded_files, 1);
    assert!(codes(&result).contains(&BudgetDiagnosticCodeV1::PolicyChanged));
}

#[test]
fn removing_a_waiver_while_excluding_its_target_is_scope_independent() {
    let historical =
        parse_comparison_policy_v1(waived_policy("legacy", "src/legacy.rs", 20, None).as_bytes())
            .unwrap();
    let current = policy(
        r#"version=1
[[rules]]
id="source"
include=["**/*.rs"]
max_lines=10
[[exclusions]]
pattern="src/legacy.rs"
kind="generated"
reason="generated source is governed by its schema"
"#,
    );
    let evaluate = |files: &[EvaluateFileV1]| {
        evaluate_v1(EvaluationInputV1 {
            policy: &current,
            comparison_policy: ComparisonPolicyV1::Present(&historical),
            current_date: date(2026, 8, 30),
            waiver_targets: &[],
            files,
        })
    };
    let unchanged = evaluate(&[]);
    let changed = evaluate(&[file("src/legacy.rs", 15, 15, Some((15, 15)))]);
    assert!(
        unchanged.passed(),
        "diagnostics: {:?}",
        unchanged.diagnostics
    );
    assert!(changed.passed(), "diagnostics: {:?}", changed.diagnostics);
    assert!(
        codes(&unchanged)
            .iter()
            .all(|code| *code == BudgetDiagnosticCodeV1::PolicyChanged)
    );
    assert!(
        codes(&changed)
            .iter()
            .all(|code| *code == BudgetDiagnosticCodeV1::PolicyChanged)
    );
}

#[test]
fn governed_unsupported_files_fail_and_excluded_files_are_not_evaluated() {
    let policy = policy(
        r#"version=1
[[rules]]
id="source"
include=["**/*"]
max_lines=10
[[exclusions]]
pattern="vendor/**"
kind="vendored"
reason="upstream"
"#,
    );
    let files = [
        EvaluateFileV1 {
            current_path: "src/link.rs".to_owned(),
            baseline_path: None,
            current: CurrentFileStateV1::Unsupported(UnsupportedFileKindV1::Symlink),
            comparison: None,
        },
        EvaluateFileV1 {
            current_path: "vendor/link.rs".to_owned(),
            baseline_path: None,
            current: CurrentFileStateV1::Unsupported(UnsupportedFileKindV1::Symlink),
            comparison: None,
        },
    ];
    let result = evaluate_v1(EvaluationInputV1 {
        policy: &policy,
        comparison_policy: ComparisonPolicyV1::Absent,
        current_date: date(2026, 8, 30),
        waiver_targets: &[],
        files: &files,
    });
    assert!(!result.passed());
    assert_eq!(result.evaluated_files, 1);
    assert_eq!(result.excluded_files, 1);
    assert_eq!(codes(&result), [BudgetDiagnosticCodeV1::UnsupportedFile]);
}

#[test]
fn rule_local_exclusions_are_counted_without_evaluating_content() {
    let policy = policy(
        r#"version=1
[[rules]]
id="source"
include=["**/*.rs"]
exclude=["generated/**"]
max_lines=10
"#,
    );
    let result = evaluate_v1(EvaluationInputV1 {
        policy: &policy,
        comparison_policy: ComparisonPolicyV1::Absent,
        current_date: date(2026, 8, 30),
        waiver_targets: &[],
        files: &[file("generated/huge.rs", 1_000, 1_000, None)],
    });
    assert!(result.passed());
    assert_eq!(result.excluded_files, 1);
    assert_eq!(result.evaluated_files, 0);
}

#[test]
fn incomplete_baseline_facts_fail_closed_instead_of_becoming_new_or_legacy() {
    let policy = line_policy(10);
    let cases = [
        EvaluateFileV1 {
            current_path: "src/lib.rs".to_owned(),
            baseline_path: Some("src/lib.rs".to_owned()),
            current: CurrentFileStateV1::Regular(MeasurementV1 { lines: 5, bytes: 5 }),
            comparison: None,
        },
        EvaluateFileV1 {
            current_path: "src/lib.rs".to_owned(),
            baseline_path: None,
            current: CurrentFileStateV1::Regular(MeasurementV1 { lines: 5, bytes: 5 }),
            comparison: Some(MeasurementV1 { lines: 5, bytes: 5 }),
        },
    ];
    for fact in cases {
        let result = evaluate_v1(EvaluationInputV1 {
            policy: &policy,
            comparison_policy: ComparisonPolicyV1::Absent,
            current_date: date(2026, 8, 30),
            waiver_targets: &[],
            files: &[fact],
        });
        assert!(!result.passed());
        assert!(codes(&result).contains(&BudgetDiagnosticCodeV1::BaselineUnavailable));
    }
}

#[test]
fn diagnostics_are_stable_regardless_of_input_order() {
    let policy = two_metric_policy("");
    let first = file("src/z.rs", 11, 101, None);
    let second = file("src/a.rs", 12, 102, None);
    let evaluate = |files: &[EvaluateFileV1]| {
        evaluate_v1(EvaluationInputV1 {
            policy: &policy,
            comparison_policy: ComparisonPolicyV1::Absent,
            current_date: date(2026, 8, 30),
            waiver_targets: &[],
            files,
        })
        .diagnostics
    };
    let forward = evaluate(&[first.clone(), second.clone()]);
    let reverse = evaluate(&[second, first]);
    assert_eq!(forward, reverse);
    assert_eq!(forward[0].severity, BudgetSeverityV1::Error);
    assert_eq!(forward[0].path.as_deref(), Some("src/a.rs"));
}

#[test]
fn duplicate_file_facts_are_rejected_without_order_dependent_budget_findings() {
    let policy = line_policy(10);
    let small = file("src/lib.rs", 5, 5, None);
    let large = file("src/lib.rs", 50, 50, None);
    let evaluate = |files: &[EvaluateFileV1]| {
        evaluate_v1(EvaluationInputV1 {
            policy: &policy,
            comparison_policy: ComparisonPolicyV1::Absent,
            current_date: date(2026, 8, 30),
            waiver_targets: &[],
            files,
        })
    };
    let forward = evaluate(&[small.clone(), large.clone()]);
    let reverse = evaluate(&[large, small]);
    assert_eq!(forward, reverse);
    assert_eq!(codes(&forward), [BudgetDiagnosticCodeV1::ScopeIncomplete]);
    assert_eq!(forward.evaluated_files, 0);
}

#[test]
fn duplicate_baseline_ancestry_cannot_multiply_legacy_debt() {
    let policy = line_policy(10);
    let first = EvaluateFileV1 {
        current_path: "src/first.rs".to_owned(),
        baseline_path: Some("src/legacy.rs".to_owned()),
        current: CurrentFileStateV1::Regular(MeasurementV1 {
            lines: 15,
            bytes: 15,
        }),
        comparison: Some(MeasurementV1 {
            lines: 15,
            bytes: 15,
        }),
    };
    let mut second = first.clone();
    second.current_path = "src/second.rs".to_owned();
    let evaluate = |files: &[EvaluateFileV1]| {
        evaluate_v1(EvaluationInputV1 {
            policy: &policy,
            comparison_policy: ComparisonPolicyV1::Absent,
            current_date: date(2026, 8, 30),
            waiver_targets: &[],
            files,
        })
    };
    let forward = evaluate(&[first.clone(), second.clone()]);
    let reverse = evaluate(&[second, first]);
    assert_eq!(forward, reverse);
    assert!(!forward.passed());
    assert!(codes(&forward).contains(&BudgetDiagnosticCodeV1::ScopeIncomplete));
    assert!(!codes(&forward).contains(&BudgetDiagnosticCodeV1::LegacyDebt));
    assert_eq!(forward.evaluated_files, 0);
}

#[test]
fn waiver_target_fact_cardinality_limit_fails_closed() {
    let policy = line_policy(10);
    let targets = (0..=MAX_WAIVER_TARGET_FACTS_V1)
        .map(|index| regular_target(&format!("src/{index}.rs")))
        .collect::<Vec<_>>();
    let result = evaluate_v1(EvaluationInputV1 {
        policy: &policy,
        comparison_policy: ComparisonPolicyV1::Absent,
        current_date: date(2026, 8, 30),
        waiver_targets: &targets,
        files: &[],
    });
    assert!(!result.passed());
    assert!(codes(&result).contains(&BudgetDiagnosticCodeV1::ResourceLimit));

    let boundary = &targets[..MAX_WAIVER_TARGET_FACTS_V1];
    let result = evaluate_v1(EvaluationInputV1 {
        policy: &policy,
        comparison_policy: ComparisonPolicyV1::Absent,
        current_date: date(2026, 8, 30),
        waiver_targets: boundary,
        files: &[],
    });
    assert!(!codes(&result).contains(&BudgetDiagnosticCodeV1::ResourceLimit));
}

#[test]
fn cross_language_paths_have_identical_semantics_without_toolchains() {
    let policy = policy(
        r#"version=1
[[rules]]
id="source"
include=["**/*"]
max_lines=10
max_bytes=100
"#,
    );
    let paths = [
        "src/lib.rs",
        "src/app.tsx",
        "src/app.js",
        "src/app.py",
        "src/app.go",
        "src/App.java",
        "src/lib.cpp",
        "src/App.cs",
        "src/app.rb",
        "src/app.php",
        "src/App.swift",
    ];
    let mut expected = None;
    for path in paths {
        let result = evaluate_v1(EvaluationInputV1 {
            policy: &policy,
            comparison_policy: ComparisonPolicyV1::Absent,
            current_date: date(2026, 8, 30),
            waiver_targets: &[],
            files: &[file(path, 12, 120, Some((11, 110)))],
        });
        let signature = result
            .diagnostics
            .iter()
            .map(|diagnostic| {
                (
                    diagnostic.severity,
                    diagnostic.code,
                    diagnostic.metric,
                    diagnostic.current,
                    diagnostic.comparison,
                    diagnostic.limit,
                    diagnostic.debt_growth,
                )
            })
            .collect::<Vec<_>>();
        if let Some(expected) = &expected {
            assert_eq!(&signature, expected, "path {path}");
        } else {
            expected = Some(signature);
        }
    }
}

#[test]
fn property_style_debt_monotonicity_is_syntax_neutral() {
    let extensions = [
        "rs", "ts", "py", "go", "java", "cpp", "cs", "rb", "php", "swift",
    ];
    for maximum in [1_u64, 5, 10] {
        let policy = policy(&format!(
            "version=1\n[[rules]]\nid=\"source\"\ninclude=[\"**/*\"]\nmax_lines={maximum}\n"
        ));
        for extension in extensions {
            let path = format!("src/example.{extension}");
            for comparison in 0..=15 {
                let mut failure_seen = false;
                for current in 0..=15 {
                    let result = evaluate_v1(EvaluationInputV1 {
                        policy: &policy,
                        comparison_policy: ComparisonPolicyV1::Absent,
                        current_date: date(2026, 8, 30),
                        waiver_targets: &[],
                        files: &[file(
                            &path,
                            current,
                            current,
                            Some((comparison, comparison)),
                        )],
                    });
                    let expected_pass =
                        current.saturating_sub(maximum) <= comparison.saturating_sub(maximum);
                    assert_eq!(
                        result.passed(),
                        expected_pass,
                        "{path}: max={maximum}, base={comparison}, current={current}"
                    );
                    if failure_seen {
                        assert!(
                            !result.passed(),
                            "a larger measurement recovered after failure"
                        );
                    }
                    failure_seen |= !result.passed();
                }
            }
        }
    }
}
