use super::*;

#[test]
fn runtime_pin_update_probe_parses_as_capability_only() {
    let pin_update_probe = Cli::try_parse_from([
        "jig",
        "__runtime-compatible",
        "--require-runtime-pin-update",
        "--contract-version",
        "4",
        "--profile",
        "runtime",
        "/tmp/repo",
    ])
    .unwrap();
    match pin_update_probe.command {
        CommandKind::RuntimeCompatible(opts) => {
            assert!(opts.capability_only);
            assert_eq!(opts.contract_version, Some(4));
        }
        other => panic!("expected runtime compatibility command, got {other:?}"),
    }
}

#[test]
fn version_after_script_writing_options_is_informational() {
    for args in [
        &["jig", "update", "--force", "--version"][..],
        &["jig", "adopt", ".", "--write", "-V"][..],
        &["jig", "init", ".", "--force", "--version"][..],
    ] {
        let error = Cli::try_parse_from(args.iter().copied()).unwrap_err();
        assert_eq!(error.kind(), clap::error::ErrorKind::DisplayVersion);
    }
}
