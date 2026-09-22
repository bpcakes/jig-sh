use super::*;

#[test]
fn parses_dev_command_with_selected_apps() {
    let cli = Cli::try_parse_from([
        "jig",
        "dev",
        "--app",
        "web",
        "--app",
        "api",
        "--https",
        "--lan",
        "--replace",
    ])
    .unwrap();

    match cli.command {
        CommandKind::Dev(opts) => {
            assert!(opts.command.is_none());
            assert_eq!(opts.launch.apps, vec!["web", "api"]);
            assert!(opts.launch.proxy.https);
            assert!(opts.launch.proxy.lan);
            assert!(opts.launch.replace);
        }
        other => panic!("expected dev command, got {other:?}"),
    }
}

#[test]
fn parses_bare_dev_as_default_launch() {
    let cli = Cli::try_parse_from(["jig", "dev"]).unwrap();

    match cli.command {
        CommandKind::Dev(opts) => {
            assert!(opts.command.is_none());
            assert!(opts.launch.apps.is_empty());
            assert!(!opts.launch.replace);
        }
        other => panic!("expected default dev launch, got {other:?}"),
    }
}

#[test]
fn parses_dev_status_and_stop_commands() {
    let status = Cli::try_parse_from([
        "jig",
        "dev",
        "status",
        "--state-dir",
        "/tmp/jig-proxy",
        "--json",
    ])
    .unwrap();
    assert!(status.json);
    match status.command {
        CommandKind::Dev(DevOpts {
            command: Some(DevSubcommand::Status(opts)),
            ..
        }) => {
            assert_eq!(opts.state_dir, Some(PathBuf::from("/tmp/jig-proxy")));
        }
        other => panic!("expected dev status command, got {other:?}"),
    }

    let stop = Cli::try_parse_from([
        "jig",
        "--json",
        "dev",
        "stop",
        "--state-dir",
        "/tmp/jig-proxy",
        "--forget-ambiguous-orphans",
    ])
    .unwrap();
    assert!(stop.json);
    match stop.command {
        CommandKind::Dev(DevOpts {
            command: Some(DevSubcommand::Stop(opts)),
            ..
        }) => {
            assert_eq!(opts.state_dir, Some(PathBuf::from("/tmp/jig-proxy")));
            assert!(opts.forget_ambiguous_orphans);
        }
        other => panic!("expected dev stop command, got {other:?}"),
    }
}

#[test]
fn parses_contextless_exact_session_commands_and_rejects_selector_overlap() {
    let all = Cli::try_parse_from([
        "jig",
        "dev",
        "status",
        "--all",
        "--state-dir",
        "/tmp/ExampleProject-proxy-state",
    ])
    .unwrap();
    let CommandKind::Dev(all) = all.command else {
        panic!("expected dev command")
    };
    assert!(all.is_contextless());

    let exact =
        Cli::try_parse_from(["jig", "dev", "status", "--session", "dev_example_target"]).unwrap();
    let CommandKind::Dev(exact) = exact.command else {
        panic!("expected dev command")
    };
    assert!(exact.is_contextless());

    let recover =
        Cli::try_parse_from(["jig", "dev", "recover", "--session", "dev_example_target"]).unwrap();
    let CommandKind::Dev(recover) = recover.command else {
        panic!("expected dev command")
    };
    assert!(recover.is_contextless());

    let stop = Cli::try_parse_from([
        "jig",
        "dev",
        "stop",
        "--session",
        "dev_example_target",
        "--forget-ambiguous-orphans",
    ])
    .unwrap();
    let CommandKind::Dev(stop) = stop.command else {
        panic!("expected dev command")
    };
    assert!(stop.is_contextless());

    assert!(
        Cli::try_parse_from([
            "jig",
            "dev",
            "status",
            "--all",
            "--session",
            "dev_example_target"
        ])
        .is_err()
    );
    assert!(Cli::try_parse_from(["jig", "dev", "recover"]).is_err());
    assert!(Cli::try_parse_from(["jig", "dev", "status", "--all", "--replace"]).is_err());
}

#[test]
fn dev_management_commands_reject_launch_options() {
    for args in [
        &["jig", "dev", "status", "--replace"][..],
        &["jig", "dev", "stop", "--app", "web"][..],
        &["jig", "dev", "status", "--https"][..],
        &["jig", "dev", "status", "--jig-project=demo@/tmp/demo"][..],
        &["jig", "dev", "--app", "web", "status"][..],
        &["jig", "dev", "--replace", "stop"][..],
    ] {
        assert!(
            Cli::try_parse_from(args).is_err(),
            "expected arguments to be rejected: {args:?}"
        );
    }
}

#[test]
fn parses_hidden_dev_process_identity() {
    let cli =
        Cli::try_parse_from(["jig", "dev", "--jig-project=demo@/tmp/demo", "--no-proxy"]).unwrap();

    match cli.command {
        CommandKind::Dev(opts) => {
            assert_eq!(
                opts.launch.jig_project.as_deref(),
                Some(std::ffi::OsStr::new("demo@/tmp/demo"))
            );
            assert!(opts.launch.no_proxy);
        }
        other => panic!("expected dev command, got {other:?}"),
    }
}
