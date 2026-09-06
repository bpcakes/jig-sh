use clap::Parser;

use super::run::post_parse_usage_error;
use super::*;

fn parse_ui(args: &[&str]) -> Cli {
    Cli::try_parse_from(args).unwrap()
}

fn ui_opts(cli: &Cli) -> &UiOpts {
    let CommandKind::Ui(opts) = &cli.command else {
        panic!("expected ui command");
    };
    opts
}

#[test]
fn parses_ui_defaults_and_bounded_options() {
    let cli = parse_ui(&["jig", "ui"]);
    let opts = ui_opts(&cli);
    assert!(!cli.json);
    assert_eq!(opts.effective_refresh_seconds(), 10);
    assert_eq!(opts.effective_timeline_limit(), 120);
    assert!(opts.timeline_limit.is_none());
    assert!(opts.plan.is_none());
    assert!(opts.retired_port.is_none());

    for limit in ["1", "1000"] {
        let cli = parse_ui(&[
            "jig",
            "ui",
            "--refresh-seconds",
            "3600",
            "--timeline-limit",
            limit,
            "--plan",
            "plan_example-1",
        ]);
        let opts = ui_opts(&cli);
        assert_eq!(opts.refresh_seconds, Some(3_600));
        assert_eq!(
            opts.timeline_limit
                .map(|value| value.to_string())
                .as_deref(),
            Some(limit)
        );
        assert_eq!(opts.plan.as_deref(), Some("plan_example-1"));
    }
}

#[test]
fn rejects_out_of_range_values_and_invalid_plan_ids() {
    for args in [
        &["jig", "ui", "--refresh-seconds", "0"][..],
        &["jig", "ui", "--refresh-seconds", "3601"][..],
        &["jig", "ui", "--timeline-limit", "0"][..],
        &["jig", "ui", "--timeline-limit", "1001"][..],
        &["jig", "ui", "--plan", "not/a/plan"][..],
    ] {
        assert!(Cli::try_parse_from(args).is_err(), "accepted {args:?}");
    }
}

#[test]
fn json_placement_and_conflicts_are_explicit() {
    for args in [
        &["jig", "--json", "ui"][..],
        &["jig", "ui", "--json"][..],
        &["jig", "ui", "--plan", "plan_example", "--json"][..],
        &["jig", "ui", "--timeline-limit", "1", "--json"][..],
    ] {
        let cli = parse_ui(args);
        assert!(cli.json);
    }
    for args in [
        &["jig", "ui", "--refresh-seconds", "10", "--json"][..],
        &[
            "jig",
            "ui",
            "--plan",
            "plan_example",
            "--timeline-limit",
            "1",
            "--json",
        ][..],
    ] {
        let cli = parse_ui(args);
        let error = post_parse_usage_error(&cli)
            .unwrap_or_else(|| panic!("accepted conflicting arguments {args:?}"));
        assert_eq!(error.exit_code(), 2);
    }
}

#[test]
fn retired_port_parses_hidden_then_becomes_a_usage_error() {
    let cli = parse_ui(&["jig", "ui", "--port", "0"]);
    let opts = ui_opts(&cli);
    assert_eq!(opts.retired_port, Some(0));
    let error = post_parse_usage_error(&cli).unwrap();
    assert_eq!(error.exit_code(), 2);
    let message = error.to_string();
    assert!(message.contains("browser server"));
    assert!(message.contains("jig ui --json"));
    assert!(message.contains("0.4.0"));
}
