mod common;

use jig_file_budget::{
    BudgetDiagnosticCodeV1, MAX_CANDIDATE_PATH_BYTES_V1, MAX_CATEGORY_BYTES_V1, MAX_PATTERNS_V1,
    MAX_POLICY_BYTES_V1, PathDispositionV1, PolicyDateV1, parse_comparison_policy_v1,
    parse_policy_v1,
};

use common::{date, policy};

fn invalid(body: &str) -> Vec<BudgetDiagnosticCodeV1> {
    parse_policy_v1(body.as_bytes(), date(2026, 8, 30))
        .unwrap_err()
        .diagnostics()
        .iter()
        .map(|diagnostic| diagnostic.code)
        .collect()
}

#[test]
fn parses_explicit_empty_and_minimal_policies() {
    let empty = policy("version = 1\nrules = []\n");
    assert!(empty.rules().is_empty());

    let minimal = policy(
        r#"
version = 1
[[rules]]
id = "source_1"
category = "source"
include = ["**/*.rs"]
max_lines = 800
"#,
    );
    assert_eq!(minimal.version(), 1);
    assert_eq!(minimal.rules()[0].max_lines, Some(800));
}

#[test]
fn rejects_unknown_fields_and_unsupported_versions() {
    assert_eq!(
        invalid("version = 1\nrules = []\nsurprise = true\n"),
        [BudgetDiagnosticCodeV1::PolicyInvalid]
    );
    assert_eq!(
        invalid("version = 1\n"),
        [BudgetDiagnosticCodeV1::PolicyInvalid]
    );
    assert_eq!(
        invalid(
            "version=1\n[[rules]]\nid=\"source\"\ninclude=[\"*.rs\"]\nmax_lines=1\nsurprise=true\n"
        ),
        [BudgetDiagnosticCodeV1::PolicyInvalid]
    );
    let error = parse_policy_v1(b"version = 2\nrules = []\n", date(2026, 8, 30)).unwrap_err();
    assert!(error.to_string().contains("supports version 1"));
    assert_eq!(
        invalid("rules = []\n"),
        [BudgetDiagnosticCodeV1::PolicyInvalid]
    );
}

#[test]
fn rejects_rule_cardinality_ids_and_threshold_errors() {
    let cases = [
        r#"version = 1
[[rules]]
id = "source"
include = []
max_lines = 1
"#,
        r#"version = 1
[[rules]]
id = "source"
include = ["**/*.rs"]
"#,
        r#"version = 1
[[rules]]
id = "Source"
include = ["**/*.rs"]
max_lines = 1
"#,
        r#"version = 1
[[rules]]
id = "-source"
include = ["**/*.rs"]
max_lines = 1
"#,
        r#"version = 1
[[rules]]
id = "source"
include = ["**/*.rs"]
notice_lines = 4
warn_lines = 3
max_lines = 5
"#,
        r#"version = 1
[[rules]]
id = "source"
include = ["**/*.rs"]
notice_bytes = 1
max_lines = 5
"#,
        r#"version = 1
[[rules]]
id = "source"
include = ["**/*.rs"]
max_lines = 0
"#,
        r#"version = 1
[[rules]]
id = "source"
include = ["**/*.rs"]
max_lines = 1
[[rules]]
id = "source"
include = ["**/*.ts"]
max_lines = 1
"#,
    ];
    for case in cases {
        assert!(
            invalid(case).contains(&BudgetDiagnosticCodeV1::PolicyInvalid),
            "case unexpectedly valid:\n{case}"
        );
    }
}

#[test]
fn category_has_an_exposed_bound_but_reasons_use_the_document_bound() {
    let category = "x".repeat(MAX_CATEGORY_BYTES_V1 + 1);
    assert!(
        parse_policy_v1(
            format!(
                "version=1\n[[rules]]\nid=\"source\"\ncategory=\"{category}\"\ninclude=[\"*.rs\"]\nmax_lines=1\n"
            )
            .as_bytes(),
            date(2026, 8, 30),
        )
        .is_err()
    );

    let reason = "review context ".repeat(10_000);
    let body = format!(
        "version=1\nrules=[]\n[[exclusions]]\npattern=\"vendor/**\"\nkind=\"vendored\"\nreason=\"{reason}\"\n"
    );
    assert!(parse_policy_v1(body.as_bytes(), date(2026, 8, 30)).is_ok());
}

#[test]
fn rejects_unsafe_patterns_and_protected_authority_exclusions() {
    for pattern in [
        "/tmp/*.rs",
        "C:/tmp/*.rs",
        "nested/C:/tmp/*.rs",
        "src//*.rs",
        "../*.rs",
        "src/../*.rs",
        ".agent/**",
        ".git/**",
    ] {
        let body =
            format!("version=1\n[[rules]]\nid=\"source\"\ninclude=[\"{pattern}\"]\nmax_lines=1\n");
        assert!(
            parse_policy_v1(body.as_bytes(), date(2026, 8, 30)).is_err(),
            "unsafe pattern passed: {pattern}"
        );
    }
    for pattern in ["[.]agent/**", "[.]git/**", "{.agent,src}/**", "?agent/**"] {
        let body =
            format!("version=1\n[[rules]]\nid=\"source\"\ninclude=[\"{pattern}\"]\nmax_lines=1\n");
        assert!(
            parse_policy_v1(body.as_bytes(), date(2026, 8, 30)).is_err(),
            "equivalent protected pattern passed: {pattern}"
        );
    }
    for pattern in ["**", ".jig/**", ".jig.toml"] {
        let body = format!(
            "version=1\nrules=[]\n[[exclusions]]\npattern=\"{pattern}\"\nkind=\"policy\"\nreason=\"reviewed\"\n"
        );
        assert!(
            parse_policy_v1(body.as_bytes(), date(2026, 8, 30)).is_err(),
            "authority exclusion passed: {pattern}"
        );
    }
}

#[test]
fn top_level_and_local_exclusions_precede_ambiguity() {
    let policy = policy(
        r#"
version = 1
[[rules]]
id = "rust"
include = ["**/*.rs"]
exclude = ["generated/**/*.rs"]
max_lines = 10
[[rules]]
id = "all-source"
include = ["src/**", "generated/**"]
max_lines = 20
[[exclusions]]
pattern = "vendor/**"
kind = "vendored"
reason = "upstream bytes"
"#,
    );
    assert!(matches!(
        policy.classify_path("vendor/lib.rs").unwrap(),
        PathDispositionV1::Excluded(_)
    ));
    assert!(matches!(
        policy.classify_path("generated/lib.rs").unwrap(),
        PathDispositionV1::Governed(rule) if rule.id == "all-source"
    ));
    let diagnostic = policy.classify_path("src/lib.rs").unwrap_err();
    assert_eq!(diagnostic.code, BudgetDiagnosticCodeV1::RuleAmbiguous);
    assert!(matches!(
        policy.classify_path("README.md").unwrap(),
        PathDispositionV1::Outside
    ));

    let local_only = common::policy(
        r#"version=1
[[rules]]
id="source"
include=["**/*.rs"]
exclude=["generated/**"]
max_lines=10
"#,
    );
    assert!(matches!(
        local_only.classify_path("generated/lib.rs").unwrap(),
        PathDispositionV1::LocallyExcluded
    ));
    assert!(matches!(
        policy.classify_path(".agent/AGENTS.md").unwrap(),
        PathDispositionV1::Outside
    ));
}

#[test]
fn invalid_candidate_paths_are_scope_diagnostics() {
    let broad = policy("version=1\n[[rules]]\nid=\"source\"\ninclude=[\"**\"]\nmax_lines=10\n");
    let diagnostic = broad.classify_path("src//lib.rs").unwrap_err();
    assert_eq!(diagnostic.code, BudgetDiagnosticCodeV1::ScopeIncomplete);
    for path in [".agent/../src/lib.rs", ".agent//state", ".git/../config"] {
        assert_eq!(
            broad.classify_path(path).unwrap_err().code,
            BudgetDiagnosticCodeV1::ScopeIncomplete,
            "path {path}"
        );
    }
    assert!(matches!(
        broad.classify_path(".agent/state/receipts.jsonl").unwrap(),
        PathDispositionV1::Outside
    ));
}

#[test]
fn candidate_paths_have_a_separate_practical_git_path_bound() {
    let broad = policy("version=1\n[[rules]]\nid=\"source\"\ninclude=[\"**\"]\nmax_lines=10\n");
    let at_limit = format!("src/{}", "x".repeat(MAX_CANDIDATE_PATH_BYTES_V1 - 4));
    assert!(matches!(
        broad.classify_path(&at_limit).unwrap(),
        PathDispositionV1::Governed(_)
    ));
    let over_limit = format!("{at_limit}x");
    assert_eq!(
        broad.classify_path(&over_limit).unwrap_err().code,
        BudgetDiagnosticCodeV1::ScopeIncomplete
    );
}

#[test]
fn ordinary_dot_git_prefixes_and_literal_glob_characters_in_candidates_are_supported() {
    let policy = policy(
        r#"
version = 1
[[rules]]
id = "source"
include = ["**/*.rs", ".github/**", ".gitignore"]
max_lines = 10
"#,
    );
    for path in [
        "src/[generated].rs",
        "src/*literal.rs",
        ".github/check.rs",
        ".gitignore",
    ] {
        assert!(
            matches!(
                policy.classify_path(path).unwrap(),
                PathDispositionV1::Governed(_)
            ),
            "path {path}"
        );
    }
}

#[test]
fn nested_authority_names_and_unusual_utf8_git_paths_are_classified_normally() {
    let broad = policy(
        r#"
version = 1
[[rules]]
id = "source"
include = ["**/*", "*"]
max_lines = 10
"#,
    );
    for path in [
        "templates/project/.agent/PLANS.md.jinja",
        "src/line\nbreak.rs",
        "src/carriage\rreturn.rs",
        " leading-and-trailing-space.rs ",
    ] {
        assert!(
            matches!(
                broad.classify_path(path).unwrap(),
                PathDispositionV1::Governed(_)
            ),
            "path {path:?}"
        );
    }
    assert!(matches!(
        broad.classify_path("src\\literal.rs").unwrap(),
        PathDispositionV1::Governed(_)
    ));
    assert!(matches!(
        broad.classify_path(".agent/PLANS.md").unwrap(),
        PathDispositionV1::Outside
    ));

    let rust_only =
        policy("version=1\n[[rules]]\nid=\"rust\"\ninclude=[\"**/*.rs\"]\nmax_lines=10\n");
    assert!(matches!(
        rust_only.classify_path("notes/line\nbreak.txt").unwrap(),
        PathDispositionV1::Outside
    ));
}

#[test]
fn policy_dates_round_trip_as_iso_text() {
    let date = PolicyDateV1::new(2026, 8, 30).unwrap();
    let encoded = serde_json::to_string(&date).unwrap();
    assert_eq!(encoded, r#""2026-08-30""#);
    assert_eq!(
        serde_json::from_str::<PolicyDateV1>(&encoded).unwrap(),
        date
    );
    assert!(serde_json::from_str::<PolicyDateV1>(r#""2026-02-30""#).is_err());
    assert!(PolicyDateV1::new(9999, 12, 31).is_ok());
    assert!(PolicyDateV1::new(10_000, 1, 1).is_err());
    assert!(PolicyDateV1::new(2024, 2, 29).is_ok());
    assert!(PolicyDateV1::new(1900, 2, 29).is_err());
    assert!(PolicyDateV1::new(2000, 2, 29).is_ok());
}

#[test]
fn validates_exact_stable_waivers_and_calendar_expiry() {
    let invalid_cases = [
        r#"version=1
[[rules]]
id="source"
include=["**/*.rs"]
max_lines=10
[[waivers]]
id="legacy"
rule="missing"
path="src/legacy.rs"
ceiling_lines=20
reason="tracked"
expires=2026-08-30
"#,
        r#"version=1
[[rules]]
id="source"
include=["**/*.rs"]
max_lines=10
[[waivers]]
id="legacy"
rule="source"
path="src/*.rs"
ceiling_lines=20
reason="tracked"
expires=2026-08-30
"#,
        r#"version=1
[[rules]]
id="source"
include=["**/*.rs"]
max_lines=10
[[waivers]]
id="legacy"
rule="source"
path="docs/legacy.md"
ceiling_lines=20
reason="tracked"
expires=2026-08-30
"#,
        r#"version=1
[[rules]]
id="source"
include=["**/*.rs"]
max_lines=10
[[waivers]]
id="legacy"
rule="source"
path="src/legacy.rs"
reason="tracked"
expires=2026-08-30
"#,
        r#"version=1
[[rules]]
id="source"
include=["**/*.rs"]
max_lines=10
[[waivers]]
id="legacy"
rule="source"
path="src/legacy.rs"
ceiling_lines=20
reason="   "
expires=2026-08-30
"#,
        r#"version=1
[[rules]]
id="source"
include=["**/*.rs"]
max_lines=10
[[waivers]]
id="legacy"
rule="source"
path="src/legacy.rs"
ceiling_bytes=20
reason="tracked"
expires=2026-08-30
"#,
        r#"version=1
[[rules]]
id="source"
include=["**/*"]
max_lines=10
[[waivers]]
id="legacy"
rule="source"
path="nested/C:/legacy.rs"
ceiling_lines=20
reason="tracked"
expires=2026-08-30
"#,
    ];
    for case in invalid_cases {
        assert!(
            invalid(case).contains(&BudgetDiagnosticCodeV1::WaiverInvalid),
            "invalid waiver passed or used wrong code:\n{case}"
        );
    }

    let active_through_date = r#"version=1
[[rules]]
id="source"
include=["**/*.rs"]
max_lines=10
[[waivers]]
id="legacy"
rule="source"
path="src/legacy.rs"
ceiling_lines=20
reason="tracked"
expires=2026-08-30
"#;
    assert!(parse_policy_v1(active_through_date.as_bytes(), date(2026, 8, 30)).is_ok());
    let error = parse_policy_v1(active_through_date.as_bytes(), date(2026, 8, 31)).unwrap_err();
    assert_eq!(
        error.diagnostics()[0].code,
        BudgetDiagnosticCodeV1::WaiverExpired
    );
    assert!(parse_comparison_policy_v1(active_through_date.as_bytes()).is_ok());
    let timestamp = active_through_date.replace("2026-08-30", "2026-08-30T00:00:00Z");
    assert!(invalid(&timestamp).contains(&BudgetDiagnosticCodeV1::WaiverInvalid));
}

#[test]
fn rejects_duplicate_waiver_ids_and_targets() {
    let common = r#"version=1
[[rules]]
id="source"
include=["**/*.rs"]
max_lines=10
"#;
    let duplicate_id = format!(
        r#"{common}
[[waivers]]
id="legacy"
rule="source"
path="src/a.rs"
ceiling_lines=20
reason="a"
expires=2027-01-01
[[waivers]]
id="legacy"
rule="source"
path="src/b.rs"
ceiling_lines=20
reason="b"
expires=2027-01-01
"#
    );
    assert!(invalid(&duplicate_id).contains(&BudgetDiagnosticCodeV1::WaiverInvalid));
    let duplicate_target = duplicate_id.replace(
        "id=\"legacy\"\nrule=\"source\"\npath=\"src/b.rs\"",
        "id=\"other\"\nrule=\"source\"\npath=\"src/a.rs\"",
    );
    assert!(invalid(&duplicate_target).contains(&BudgetDiagnosticCodeV1::WaiverInvalid));
}

#[test]
fn pattern_and_numeric_bounds_fail_closed() {
    let includes = (0..=MAX_PATTERNS_V1)
        .map(|index| format!("\"src/{index}.rs\""))
        .collect::<Vec<_>>()
        .join(",");
    let too_many =
        format!("version=1\n[[rules]]\nid=\"source\"\ninclude=[{includes}]\nmax_lines=1\n");
    assert!(parse_policy_v1(too_many.as_bytes(), date(2026, 8, 30)).is_err());
    assert!(
        parse_policy_v1(
            b"version=1\n[[rules]]\nid=\"source\"\ninclude=[\"*.rs\"]\nmax_lines=18446744073709551616\n",
            date(2026, 8, 30)
        )
        .is_err()
    );
    let long_pattern = "x".repeat(1025);
    let long_pattern =
        format!("version=1\n[[rules]]\nid=\"source\"\ninclude=[\"{long_pattern}\"]\nmax_lines=1\n");
    assert!(parse_policy_v1(long_pattern.as_bytes(), date(2026, 8, 30)).is_err());

    let oversized = vec![b' '; MAX_POLICY_BYTES_V1 + 1];
    let error = parse_policy_v1(&oversized, date(2026, 8, 30)).unwrap_err();
    assert_eq!(error.raw_sha256().len(), 64);
}

#[test]
fn pattern_cardinality_fails_before_individual_glob_compilation() {
    let includes = std::iter::repeat_n(r#""[""#, MAX_PATTERNS_V1 + 1)
        .collect::<Vec<_>>()
        .join(",");
    let body = format!("version=1\n[[rules]]\nid=\"source\"\ninclude=[{includes}]\nmax_lines=1\n");
    let error = parse_policy_v1(body.as_bytes(), date(2026, 8, 30)).unwrap_err();
    assert_eq!(error.diagnostics().len(), 1);
    assert!(error.diagnostics()[0].message.contains("patterns"));
    assert!(error.diagnostics()[0].message.contains("at most"));
}

#[test]
fn rule_and_waiver_cardinality_limits_are_checked_before_compilation() {
    let mut rules = String::from("version=1\n");
    for index in 0..257 {
        rules.push_str(&format!(
            "[[rules]]\nid=\"rule-{index}\"\ninclude=[\"src/{index}.rs\"]\nmax_lines=1\n"
        ));
    }
    assert!(parse_policy_v1(rules.as_bytes(), date(2026, 8, 30)).is_err());

    let mut waivers =
        String::from("version=1\n[[rules]]\nid=\"source\"\ninclude=[\"**/*.rs\"]\nmax_lines=1\n");
    for index in 0..4097 {
        waivers.push_str(&format!(
            "[[waivers]]\nid=\"waiver-{index}\"\nrule=\"source\"\npath=\"src/{index}.rs\"\nceiling_lines=2\nreason=\"tracked\"\nexpires=2027-01-01\n"
        ));
    }
    assert!(parse_policy_v1(waivers.as_bytes(), date(2026, 8, 30)).is_err());
}

#[test]
fn raw_and_semantic_identities_distinguish_formatting_from_meaning() {
    let first = policy(
        "version=1\n[[rules]]\nid=\"source\"\ninclude=[\"*.rs\",\"src/**\"]\nmax_lines=10\n",
    );
    let formatted = policy(
        "version = 1\n\n[[rules]]\nmax_lines = 10\ninclude = [\"src/**\", \"*.rs\"]\nid = \"source\"\n",
    );
    assert_ne!(
        first.identity().raw_sha256(),
        formatted.identity().raw_sha256()
    );
    assert_eq!(
        first.identity().semantic_sha256(),
        formatted.identity().semantic_sha256()
    );
    assert_eq!(
        first.identity().semantic_input(),
        formatted.identity().semantic_input()
    );
    let duplicate_pattern = policy(
        "version=1\n[[rules]]\nid=\"source\"\ninclude=[\"src/**\",\"*.rs\",\"src/**\"]\nmax_lines=10\n",
    );
    assert_eq!(
        first.identity().semantic_sha256(),
        duplicate_pattern.identity().semantic_sha256()
    );
    let changed = policy(
        "version=1\n[[rules]]\nid=\"source\"\ninclude=[\"*.rs\",\"src/**\"]\nmax_lines=11\n",
    );
    assert_ne!(
        first.identity().semantic_sha256(),
        changed.identity().semantic_sha256()
    );
}
