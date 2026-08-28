use super::super::*;
use clap::Parser;

#[test]
fn parses_top_level_info_command_and_explain_alias() {
    let cli = Cli::try_parse_from(["jig", "info"]).unwrap();

    match cli.command {
        CommandKind::Info(opts) => assert!(!opts.commands),
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
}
