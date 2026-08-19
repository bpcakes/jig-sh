use super::*;

#[test]
fn update_accepts_json_after_subcommand() {
    let update = Cli::try_parse_from(["jig", "update", "--json"]).unwrap();

    assert!(update.json);
    assert!(matches!(update.command, CommandKind::Update(_)));
}

#[test]
fn parses_update_recopy_flag() {
    let cli = Cli::try_parse_from([
        "jig",
        "update",
        "--recopy",
        "--force",
        "--template",
        "/tmp/template",
        "--template-mode",
        "committed",
    ])
    .unwrap();

    match cli.command {
        CommandKind::Update(bootstrap::UpdateOpts {
            recopy,
            force,
            template,
            template_mode,
            ..
        }) => {
            assert!(recopy);
            assert!(force);
            assert_eq!(template.as_deref(), Some("/tmp/template"));
            assert_eq!(template_mode, Some(bootstrap::TemplateMode::Committed));
        }
        other => panic!("expected update command, got {other:?}"),
    }
}

#[test]
fn parses_launcher_only_update_and_rejects_source_overrides() {
    let cli =
        Cli::try_parse_from(["jig", "update", "/tmp/repo", "--launcher-only", "--force"]).unwrap();
    match cli.command {
        CommandKind::Update(bootstrap::UpdateOpts {
            path,
            launcher_only,
            force,
            ..
        }) => {
            assert_eq!(path, PathBuf::from("/tmp/repo"));
            assert!(launcher_only);
            assert!(force);
        }
        other => panic!("expected launcher-only update, got {other:?}"),
    }

    let missing_force =
        Cli::try_parse_from(["jig", "update", "/tmp/repo", "--launcher-only"]).unwrap_err();
    assert_eq!(
        missing_force.kind(),
        clap::error::ErrorKind::MissingRequiredArgument
    );

    for conflicting in ["--recopy", "--template=/tmp/template", "--vcs-ref=main"] {
        let error = Cli::try_parse_from([
            "jig",
            "update",
            "/tmp/repo",
            "--launcher-only",
            "--force",
            conflicting,
        ])
        .unwrap_err();
        assert_eq!(error.kind(), clap::error::ErrorKind::ArgumentConflict);
    }
    for ignored in ["--defaults", "--no-input"] {
        let error = Cli::try_parse_from([
            "jig",
            "update",
            "/tmp/repo",
            "--launcher-only",
            "--force",
            ignored,
        ])
        .unwrap_err();
        assert_eq!(error.kind(), clap::error::ErrorKind::ArgumentConflict);
    }
}
