use super::super::*;
use clap::Parser;

#[test]
fn parses_top_level_info_command_and_explain_alias() {
    let cli = Cli::try_parse_from(["jig", "info"]).unwrap();

    match cli.command {
        CommandKind::Info(opts) => {
            assert!(!opts.commands);
            assert_eq!(opts.projection, crate::surface::ResponseSurface::Standard);
        }
        other => panic!("expected info command, got {other:?}"),
    }

    let with_json = Cli::try_parse_from(["jig", "info", "--json"]).unwrap();
    assert!(with_json.json);
    match with_json.command {
        CommandKind::Info(opts) => assert!(!opts.commands),
        other => panic!("expected info command, got {other:?}"),
    }

    let alias = Cli::try_parse_from(["jig", "explain", "--json"]).unwrap();
    assert!(alias.json);
    match alias.command {
        CommandKind::Info(opts) => assert!(!opts.commands),
        other => panic!("expected info alias command, got {other:?}"),
    }

    let commands = Cli::try_parse_from(["jig", "info", "--commands"]).unwrap();
    match commands.command {
        CommandKind::Info(opts) => assert!(opts.commands),
        other => panic!("expected info commands view, got {other:?}"),
    }

    let go_version = Cli::try_parse_from(["jig", "info", "go-version"]).unwrap();
    assert!(matches!(
        go_version.command,
        CommandKind::Info(InfoOpts {
            subject: Some(InfoCommand::GoVersion),
            ..
        })
    ));

    let rejected = Cli::try_parse_from(["jig", "info", "--summary"]);
    assert!(rejected.is_err());

    let freshness = Cli::try_parse_from(["jig", "info", "freshness", "--json"]).unwrap();
    assert!(freshness.json);
    assert!(matches!(
        freshness.command,
        CommandKind::Info(InfoOpts {
            subject: Some(InfoCommand::Freshness(_)),
            ..
        })
    ));
}

#[test]
fn freshness_assertions_require_explicit_targets_and_input_ownership() {
    let cli = Cli::try_parse_from([
        "jig",
        "info",
        "freshness",
        "--target",
        "workspace:fmt",
        "--assert-worktree",
        "--assert-exhaustive",
        "--input",
        "assets/**",
        "--patch",
        "--json",
    ])
    .unwrap();
    assert!(cli.json);
    let CommandKind::Info(InfoOpts {
        subject: Some(InfoCommand::Freshness(opts)),
        ..
    }) = cli.command
    else {
        panic!("expected freshness preview");
    };
    assert_eq!(opts.targets[0].to_string(), "workspace:fmt");
    assert!(opts.assert_worktree && opts.assert_exhaustive && opts.patch);
    assert_eq!(opts.inputs, ["assets/**"]);
    for args in [
        vec!["--assert-worktree"],
        vec!["--assert-exhaustive"],
        vec!["--target", "workspace:fmt", "--input", "assets/**"],
        vec!["--target", "workspace:*"],
    ] {
        assert!(Cli::try_parse_from(["jig", "info", "freshness"].into_iter().chain(args)).is_err());
    }
    assert!(Cli::try_parse_from(["jig", "info", "freshness", "--patch"]).is_ok());
}

#[test]
fn parses_explicit_agent_surfaces_and_rejects_unknown_values() {
    let info = Cli::try_parse_from([
        "jig",
        "info",
        "target",
        "api:test",
        "--projection",
        "agent-v1",
    ])
    .unwrap();
    let CommandKind::Info(opts) = info.command else {
        panic!("expected info command")
    };
    assert_eq!(opts.projection, crate::surface::ResponseSurface::AgentV1);
    opts.validate_projection().unwrap();

    let mcp = Cli::try_parse_from(["jig", "mcp", "--surface", "agent-v1"]).unwrap();
    assert!(matches!(
        mcp.command,
        CommandKind::Mcp(McpOpts {
            surface: crate::surface::ResponseSurface::AgentV1
        })
    ));

    assert!(
        Cli::try_parse_from(["jig", "info", "--projection", "future"])
            .unwrap_err()
            .to_string()
            .contains("invalid value 'future'")
    );
    assert!(
        Cli::try_parse_from(["jig", "mcp", "--surface", "future"])
            .unwrap_err()
            .to_string()
            .contains("invalid value 'future'")
    );
}

#[test]
fn agent_projection_rejects_info_views_without_target_records() {
    for argv in [
        vec!["jig", "info", "--projection", "agent-v1"],
        vec!["jig", "info", "--commands", "--projection", "agent-v1"],
        vec!["jig", "info", "components", "--projection", "agent-v1"],
        vec!["jig", "info", "profiles", "--projection", "agent-v1"],
        vec![
            "jig",
            "info",
            "profile",
            "verify",
            "--projection",
            "agent-v1",
        ],
        vec!["jig", "info", "go-version", "--projection", "agent-v1"],
        vec!["jig", "info", "freshness", "--projection", "agent-v1"],
    ] {
        let cli = Cli::try_parse_from(argv).unwrap();
        let CommandKind::Info(opts) = cli.command else {
            panic!("expected info command")
        };
        assert_eq!(
            opts.validate_projection().unwrap_err().to_string(),
            "--projection agent-v1 requires a target-bearing info subject: workspace, component, targets, or target"
        );
    }
}
