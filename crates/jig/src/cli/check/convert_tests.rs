use super::*;
use crate::cli::NamedCheckCommand;
use crate::cli::check::CheckComparisonOpts;

#[test]
fn external_check_selectors_accept_execution_flags_after_targets() {
    let request = command::CheckCommand::try_from(CheckOpts {
        profile: None,
        affected: None,
        explain: false,
        fail_fast: false,
        comparison: CheckComparisonOpts::default(),
        command: Some(CheckCommand::Selectors(vec![
            "api:test".into(),
            "web:lint".into(),
            "--fail-fast".into(),
        ])),
    })
    .unwrap();

    let command::CheckCommand::Repository(request) = request else {
        panic!("expected repository check request");
    };
    assert_eq!(request.selectors, ["api:test", "web:lint"]);
    assert!(request.fail_fast);
}

#[test]
fn external_check_selectors_reject_the_retired_receipt_flag() {
    let error = command::CheckCommand::try_from(CheckOpts {
        profile: None,
        affected: None,
        explain: false,
        fail_fast: false,
        comparison: CheckComparisonOpts::default(),
        command: Some(CheckCommand::Selectors(vec![
            "api:test".into(),
            "--no-receipt".into(),
        ])),
    })
    .unwrap_err();

    assert!(
        error
            .to_string()
            .contains("unknown check option '--no-receipt'"),
        "{error:#}"
    );
}

#[test]
fn repository_action_selectors_reject_direct_file_budget_mode_flags() {
    for flag in ["--all", "--staged", "--changed-against"] {
        let mut selectors = vec!["repo:file-budget".into(), flag.into()];
        if flag == "--changed-against" {
            selectors.push("origin/main".into());
        }
        let error = command::CheckCommand::try_from(CheckOpts {
            profile: None,
            affected: None,
            explain: false,
            fail_fast: false,
            comparison: CheckComparisonOpts::default(),
            command: Some(CheckCommand::Selectors(selectors)),
        })
        .unwrap_err()
        .to_string();

        assert!(
            error.contains(&format!("unknown check option '{flag}'")),
            "{error}"
        );
    }
}

#[test]
fn repository_action_selectors_preserve_exact_push_before_authority() {
    let oid = "a".repeat(40);
    let request = command::CheckCommand::try_from(CheckOpts {
        command: Some(CheckCommand::Selectors(vec![
            "repo:file-budget".into(),
            "--comparison-exact-tree".into(),
            oid.clone(),
            "--comparison-provenance=push_before".into(),
        ])),
        ..CheckOpts::default()
    })
    .unwrap();

    let command::CheckCommand::Repository(request) = request else {
        panic!("expected repository check request");
    };
    assert_eq!(request.selectors, ["repo:file-budget"]);
    assert_eq!(
        request.comparison,
        Some(jig_contract::ComparisonRequestV1::ExactTree {
            requested_oid: oid,
            provenance: jig_contract::ExactTreeProvenanceV1::PushBefore,
        })
    );
}

#[test]
fn repository_action_comparison_selector_grammar_is_closed() {
    for selectors in [
        vec!["repo:file-budget", "--comparison-exact-tree", "abcd"],
        vec!["repo:file-budget", "--comparison-provenance", "explicit"],
        vec![
            "repo:file-budget",
            "--comparison-staged",
            "--comparison-base",
            "main",
        ],
    ] {
        let error = command::CheckCommand::try_from(CheckOpts {
            command: Some(CheckCommand::Selectors(
                selectors.into_iter().map(str::to_owned).collect(),
            )),
            ..CheckOpts::default()
        })
        .unwrap_err()
        .to_string();
        assert!(
            error.contains("must be supplied together") || error.contains("mutually exclusive"),
            "{error}"
        );
    }
}

#[test]
fn built_in_action_names_compose_with_additional_selectors() {
    let request = command::CheckCommand::try_from(CheckOpts {
        profile: None,
        affected: None,
        explain: false,
        fail_fast: false,
        comparison: CheckComparisonOpts::default(),
        command: Some(CheckCommand::Named(NamedCheckCommand::Test(
            CheckTargetOpts {
                selectors: vec!["api:lint".into()],
            },
        ))),
    })
    .unwrap();

    let command::CheckCommand::Repository(request) = request else {
        panic!("expected repository check request");
    };
    assert_eq!(request.selectors, ["test", "api:lint"]);
}
