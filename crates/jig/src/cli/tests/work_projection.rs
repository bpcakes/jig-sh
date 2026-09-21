use super::super::*;
use clap::Parser;

#[test]
fn work_projection_is_explicit_and_scoped_to_supported_inspection_commands() {
    for command in ["check", "gates", "evidence"] {
        for (selection, expected) in [
            ("standard", crate::surface::ResponseSurface::Standard),
            ("agent-v1", crate::surface::ResponseSurface::AgentV1),
        ] {
            let parsed = Cli::try_parse_from([
                "jig",
                "work",
                command,
                "--plan-id",
                "plan_1",
                "--projection",
                selection,
            ])
            .unwrap();
            let CommandKind::Work(command) = parsed.command else {
                panic!("work command")
            };
            let projection = match crate::command::WorkCommand::from(command) {
                crate::command::WorkCommand::Check(request) => request.projection,
                crate::command::WorkCommand::Gates(request) => request.projection,
                crate::command::WorkCommand::Evidence(request) => request.projection,
                _ => panic!("inspection command"),
            };
            assert_eq!(projection, expected);
        }
        assert!(
            Cli::try_parse_from([
                "jig",
                "work",
                command,
                "--plan-id",
                "plan_1",
                "--projection",
                "future",
            ])
            .is_err()
        );
    }
    assert!(
        Cli::try_parse_from([
            "jig",
            "work",
            "finish",
            "--plan-id",
            "plan_1",
            "--projection",
            "agent-v1"
        ])
        .is_err()
    );
}
