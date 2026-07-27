use clap::Parser;

use super::*;

#[test]
fn parses_top_level_status_command() {
    let cli = Cli::try_parse_from(["jig", "status"]).unwrap();
    assert!(matches!(cli.command, CommandKind::Status));

    let with_json = Cli::try_parse_from(["jig", "status", "--json"]).unwrap();
    assert!(with_json.json);
    assert!(matches!(with_json.command, CommandKind::Status));

    assert!(Cli::try_parse_from(["jig", "status", "--summary"]).is_err());
}
