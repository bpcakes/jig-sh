use super::*;

#[test]
fn parses_work_status_command() {
    let cli = Cli::try_parse_from(["jig", "work", "status"]).unwrap();

    match cli.command {
        CommandKind::Work(WorkCommand::Status) => {}
        other => panic!("expected work status command, got {other:?}"),
    }

    let rejected = Cli::try_parse_from(["jig", "work", "status", "--summary"]);
    assert!(rejected.is_err());
}

#[test]
fn parses_work_start_print_plan_id() {
    let cli = Cli::try_parse_from([
        "jig",
        "work",
        "start",
        "--title",
        "DX polish",
        "--body",
        "Improve workflow.",
        "--print-plan-id",
    ])
    .unwrap();

    match cli.command {
        CommandKind::Work(WorkCommand::Start(opts)) => {
            assert_eq!(opts.title, "DX polish");
            assert_eq!(opts.body.as_deref(), Some("Improve workflow."));
            assert!(opts.print_plan_id);
        }
        other => panic!("expected work start command, got {other:?}"),
    }
}

#[test]
fn work_start_rejects_multiple_body_sources() {
    let error = Cli::try_parse_from([
        "jig",
        "work",
        "start",
        "--title",
        "DX polish",
        "--body",
        "inline",
        "--body-file",
        "plan.md",
    ])
    .unwrap_err();

    assert_eq!(error.kind(), clap::error::ErrorKind::ArgumentConflict);
}

#[test]
fn work_append_requires_exactly_one_body_source() {
    let missing =
        Cli::try_parse_from(["jig", "work", "append", "--plan-id", "plan_1"]).unwrap_err();
    assert_eq!(
        missing.kind(),
        clap::error::ErrorKind::MissingRequiredArgument
    );

    let conflicting = Cli::try_parse_from([
        "jig",
        "work",
        "append",
        "--plan-id",
        "plan_1",
        "--body",
        "inline",
        "--body-file",
        "plan.md",
    ])
    .unwrap_err();
    assert_eq!(conflicting.kind(), clap::error::ErrorKind::ArgumentConflict);
}

#[test]
fn parses_work_check_tools() {
    let cli = Cli::try_parse_from([
        "jig",
        "work",
        "check",
        "--plan-id",
        "plan_1",
        "--tool",
        tool::CONTRACT_CHECK,
        "--tool",
        tool::TEST,
    ])
    .unwrap();

    match cli.command {
        CommandKind::Work(WorkCommand::Check(opts)) => {
            assert_eq!(opts.plan_id, "plan_1");
            assert_eq!(opts.tools, vec![tool::CONTRACT_CHECK, tool::TEST]);
        }
        other => panic!("expected work check command, got {other:?}"),
    }
}

#[test]
fn parses_work_gates_command() {
    let cli = Cli::try_parse_from(["jig", "work", "gates", "--plan-id", "plan_1"]).unwrap();

    match cli.command {
        CommandKind::Work(WorkCommand::Gates(opts)) => {
            assert_eq!(opts.plan_id.as_deref(), Some("plan_1"));
        }
        other => panic!("expected work gates command, got {other:?}"),
    }

    let inferred_plan = Cli::try_parse_from(["jig", "work", "gates"]).unwrap();

    match inferred_plan.command {
        CommandKind::Work(WorkCommand::Gates(opts)) => {
            assert_eq!(opts.plan_id, None);
        }
        other => panic!("expected work gates command, got {other:?}"),
    }
}

#[test]
fn parses_work_evidence_command() {
    let cli = Cli::try_parse_from(["jig", "work", "evidence"]).unwrap();

    match cli.command {
        CommandKind::Work(WorkCommand::Evidence(opts)) => {
            assert_eq!(opts.plan_id, None);
        }
        other => panic!("expected work evidence command, got {other:?}"),
    }

    let with_plan =
        Cli::try_parse_from(["jig", "work", "evidence", "--plan-id", "plan_1"]).unwrap();

    match with_plan.command {
        CommandKind::Work(WorkCommand::Evidence(opts)) => {
            assert_eq!(opts.plan_id.as_deref(), Some("plan_1"));
        }
        other => panic!("expected work evidence command, got {other:?}"),
    }
}

#[test]
fn parses_work_review_command() {
    let cli = Cli::try_parse_from([
        "jig",
        "work",
        "review",
        "--plan-id",
        "plan_1",
        "--gate",
        "rust-error-handling",
    ])
    .unwrap();

    match cli.command {
        CommandKind::Work(WorkCommand::Review(opts)) => {
            assert_eq!(opts.plan_id, "plan_1");
            assert_eq!(opts.gates, vec!["rust-error-handling"]);
        }
        other => panic!("expected work review command, got {other:?}"),
    }
}

#[test]
fn parses_work_refine_command() {
    let cli = Cli::try_parse_from([
        "jig",
        "work",
        "refine",
        "--plan-id",
        "plan_1",
        "--gate",
        "rust-error-handling",
        "--max-iterations",
        "2",
    ])
    .unwrap();

    match cli.command {
        CommandKind::Work(WorkCommand::Refine(opts)) => {
            assert_eq!(opts.plan_id, "plan_1");
            assert_eq!(opts.gates, vec!["rust-error-handling"]);
            assert_eq!(opts.max_iterations, 2);
        }
        other => panic!("expected work refine command, got {other:?}"),
    }
}
