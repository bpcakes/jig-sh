use super::*;

#[test]
fn external_check_selectors_accept_execution_flags_after_targets() {
    let request = command::CheckCommand::try_from(CheckOpts {
        tool: ToolOpts::default(),
        profile: None,
        affected: None,
        explain: false,
        fail_fast: false,
        comparison: CheckComparisonOpts::default(),
        command: Some(CheckCommand::Selectors(vec![
            "api:test".into(),
            "web:lint".into(),
            "--no-receipt".into(),
            "--fail-fast".into(),
        ])),
    })
    .unwrap();

    let command::CheckCommand::Repository(request) = request else {
        panic!("expected repository check request");
    };
    assert_eq!(request.selectors, ["api:test", "web:lint"]);
    assert!(request.fail_fast);
    assert_eq!(request.tool.into_parts(), (None, false));
}

#[test]
fn repository_action_selectors_reject_direct_file_budget_mode_flags() {
    for flag in ["--all", "--staged", "--changed-against"] {
        let mut selectors = vec!["repo:file-budget".into(), flag.into()];
        if flag == "--changed-against" {
            selectors.push("origin/main".into());
        }
        let error = command::CheckCommand::try_from(CheckOpts {
            tool: ToolOpts::default(),
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
        tool: ToolOpts::default(),
        profile: None,
        affected: None,
        explain: false,
        fail_fast: false,
        comparison: CheckComparisonOpts::default(),
        command: Some(CheckCommand::Test(CheckTargetOpts {
            tool: ToolOpts::default(),
            selectors: vec!["api:lint".into()],
        })),
    })
    .unwrap();

    let command::CheckCommand::Repository(request) = request else {
        panic!("expected repository check request");
    };
    assert_eq!(request.selectors, ["test", "api:lint"]);
}

#[test]
fn dev_conversion_preserves_default_launch_and_replace() {
    let request: command::DevCommand = DevOpts {
        command: None,
        launch: DevLaunchOpts {
            jig_project: Some("demo@/tmp/demo".into()),
            apps: vec!["web".into(), "api".into()],
            discover_workspace: true,
            no_proxy: false,
            replace: true,
            proxy: ProxyRuntimeOpts {
                state_dir: Some("/tmp/proxy".into()),
                https: true,
                ..Default::default()
            },
        },
    }
    .into();

    match request {
        command::DevCommand::Launch(request) => {
            assert_eq!(request.apps, vec!["web", "api"]);
            assert!(request.discover_workspace);
            assert!(!request.no_proxy);
            assert!(request.replace);
            assert_eq!(request.proxy.state_dir, Some("/tmp/proxy".into()));
            assert!(request.proxy.https);
        }
        other => panic!("expected dev launch request, got {other:?}"),
    }
}

#[test]
fn dev_conversion_preserves_management_action_state_dirs() {
    let status: command::DevCommand = DevOpts {
        command: Some(DevSubcommand::Status(DevStatusOpts {
            state_dir: Some("/tmp/status".into()),
        })),
        launch: DevLaunchOpts::default(),
    }
    .into();
    match status {
        command::DevCommand::Status(request) => {
            assert_eq!(request.state_dir, Some("/tmp/status".into()));
        }
        other => panic!("expected dev status request, got {other:?}"),
    }

    let stop: command::DevCommand = DevOpts {
        command: Some(DevSubcommand::Stop(DevStopOpts {
            state_dir: Some("/tmp/stop".into()),
            forget_ambiguous_orphans: true,
        })),
        launch: DevLaunchOpts::default(),
    }
    .into();
    match stop {
        command::DevCommand::Stop(request) => {
            assert_eq!(request.state_dir, Some("/tmp/stop".into()));
            assert!(request.forget_ambiguous_orphans);
        }
        other => panic!("expected dev stop request, got {other:?}"),
    }
}

#[test]
fn work_receipts_conversion_preserves_filters() {
    let request: command::WorkReceiptsRequest = WorkReceiptsOpts {
        session_id: Some("session_1".to_string()),
        plan_id: Some("plan_1".to_string()),
        tool_name: Some(crate::tool_defs::tool::TEST.to_string()),
        failed_only: true,
        limit: 7,
    }
    .into();

    assert_eq!(request.session_id.as_deref(), Some("session_1"));
    assert_eq!(request.plan_id.as_deref(), Some("plan_1"));
    assert_eq!(
        request.tool_name.as_deref(),
        Some(crate::tool_defs::tool::TEST)
    );
    assert!(request.failed_only);
    assert_eq!(request.limit, 7);
}

#[test]
fn work_evidence_conversion_preserves_plan_id() {
    let request: command::WorkEvidenceRequest = WorkEvidenceOpts {
        plan_id: Some("plan_1".to_string()),
    }
    .into();

    assert_eq!(request.plan_id.as_deref(), Some("plan_1"));
}

#[test]
fn state_archive_conversion_preserves_cutoff_run_scope_and_dry_run() {
    let request: command::StateCommand = StateCommand::Archive(StateArchiveOpts {
        before: "2026-01-01".into(),
        include_runs: true,
        dry_run: true,
    })
    .into();

    match request {
        command::StateCommand::Archive(request) => {
            assert_eq!(request.before, "2026-01-01");
            assert!(request.include_runs);
            assert!(request.dry_run);
        }
        other => panic!("expected state archive request, got {other:?}"),
    }
}

#[test]
fn state_maintenance_conversion_preserves_arguments() {
    let request: command::StateCommand =
        StateCommand::Diagnose(StateDiagnoseOpts { deep: true }).into();
    match request {
        command::StateCommand::Diagnose(request) => assert!(request.deep),
        other => panic!("expected state diagnose request, got {other:?}"),
    }

    let request: command::StateCommand = StateCommand::Compact {
        command: StateCompactCommand::Sessions(StateCompactSessionsOpts { dry_run: true }),
    }
    .into();
    match request {
        command::StateCommand::CompactSessions(request) => {
            assert!(request.dry_run);
        }
        other => {
            panic!("expected state compact sessions request, got {other:?}")
        }
    }

    let backup = std::path::PathBuf::from("backup/manifest.json");
    let request: command::StateCommand = StateCommand::Restore(StateRestoreOpts {
        backup: backup.clone(),
    })
    .into();
    match request {
        command::StateCommand::Restore(request) => {
            assert_eq!(request.backup, backup);
        }
        other => panic!("expected state restore request, got {other:?}"),
    }

    let output = std::path::PathBuf::from("receipts.jsonl.gz");
    let request: command::StateCommand = StateCommand::Export {
        command: StateExportCommand::Receipts(StateExportReceiptsOpts {
            before: "2026-01-01".into(),
            output: output.clone(),
        }),
    }
    .into();
    match request {
        command::StateCommand::ExportReceipts(request) => {
            assert_eq!(request.before, "2026-01-01");
            assert_eq!(request.output, output);
        }
        other => {
            panic!("expected state export receipts request, got {other:?}")
        }
    }
}
