use clap::Parser;

use super::*;

#[test]
fn parses_top_level_status_command() {
    let cli = Cli::try_parse_from(["jig", "status"]).unwrap();
    assert!(matches!(
        cli.command,
        CommandKind::Status(StatusOpts {
            tui: false,
            refresh_seconds: None
        })
    ));

    let with_json = Cli::try_parse_from(["jig", "status", "--json"]).unwrap();
    assert!(with_json.json);
    assert!(matches!(
        with_json.command,
        CommandKind::Status(StatusOpts { tui: false, .. })
    ));

    let tui = Cli::try_parse_from(["jig", "status", "--tui", "--refresh-seconds", "45"]).unwrap();
    match tui.command {
        CommandKind::Status(opts) => {
            assert!(opts.tui);
            assert_eq!(opts.effective_refresh_seconds(), 45);
        }
        other => panic!("expected status command, got {other:?}"),
    }

    assert!(Cli::try_parse_from(["jig", "status", "--summary"]).is_err());
    let conflict = Cli::try_parse_from(["jig", "status", "--tui", "--json"]).unwrap();
    match conflict.command {
        CommandKind::Status(opts) => {
            assert!(opts.validate_output_mode(conflict.json).is_err());
        }
        other => panic!("expected status command, got {other:?}"),
    }
    assert!(Cli::try_parse_from(["jig", "status", "--refresh-seconds", "10"]).is_err());
    assert!(Cli::try_parse_from(["jig", "status", "--tui", "--refresh-seconds", "0"]).is_err());
}
