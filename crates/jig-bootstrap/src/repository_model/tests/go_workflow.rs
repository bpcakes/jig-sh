use super::*;

#[test]
fn authored_go_workflow_renders_exact_targets_from_its_capability_aliases() {
    for (action_ids, add_aliases, read_only, add_foreign_fmt, expected) in [
        (
            ["format", "vet", "verify"],
            true,
            true,
            false,
            [Some("api:format"), Some("api:vet"), Some("api:verify")],
        ),
        (
            ["fmt", "lint", "test-locked"],
            true,
            true,
            true,
            [Some("api:fmt"), Some("api:lint"), Some("api:test-locked")],
        ),
        (
            ["fmt", "lint", "test-locked"],
            false,
            true,
            false,
            [None, None, None],
        ),
        (
            ["fmt", "lint", "test-locked"],
            true,
            false,
            false,
            [None, None, None],
        ),
    ] {
        let component = ComponentSpec {
            adapters: vec!["go".into()],
            ..ComponentSpec::new(component_id("api").unwrap(), "services/api")
        };
        let mut actions = action_ids
            .into_iter()
            .zip([
                jig_contract::tool::FMT_CHECK,
                jig_contract::tool::LINT,
                jig_contract::tool::TEST_LOCKED,
            ])
            .map(|(action_id, alias)| {
                let mut action = ActionSpec::new(
                    target_id("api", action_id).unwrap(),
                    ActionIntent::Check,
                    ActionRunner::command(format!("go_{action_id}_command")),
                );
                action.effects = if read_only {
                    vec![
                        jig_contract::ActionEffect::ReadOnly,
                        jig_contract::ActionEffect::Process,
                    ]
                } else {
                    vec![jig_contract::ActionEffect::Worktree]
                };
                if add_aliases {
                    action.legacy_aliases.push(alias.into());
                }
                action.inputs = vec!["shared/proto/**".into()];
                action
            })
            .collect::<Vec<_>>();
        let mut components = vec![component];
        if add_foreign_fmt {
            components.push(ComponentSpec {
                adapters: vec!["rust".into()],
                ..ComponentSpec::new(component_id("worker").unwrap(), "services/worker")
            });
            let mut action = ActionSpec::new(
                target_id("worker", "fmt").unwrap(),
                ActionIntent::Check,
                ActionRunner::command("rust_fmt_command"),
            );
            action.effects = vec![
                jig_contract::ActionEffect::ReadOnly,
                jig_contract::ActionEffect::Process,
            ];
            actions.push(action);
        }
        let targets = actions
            .iter()
            .map(|action| action.target.clone())
            .collect::<Vec<_>>();
        let commands = actions
            .iter()
            .map(|action| {
                let ActionRunner::Command { command, .. } = &action.runner else {
                    unreachable!()
                };
                (command.to_string(), "true".to_string())
            })
            .collect::<BTreeMap<_, _>>();
        let authored = RepositoryRenderModel {
            affected_ignore: Vec::new(),
            components,
            profiles: vec![ProfileSpec::new(ProfileId::parse("ci").unwrap(), targets)],
            default_check_profile: ProfileId::parse("ci").unwrap(),
            required_commands: commands.keys().cloned().collect(),
            tools: Vec::new(),
            commands,
            actions: std::mem::take(&mut actions),
        };
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("answers.toml");
        fs::write(
            &path,
            format!(
                "repo_name = \"ExampleProject\"\nsqlx_enabled = false\nschema_dump_enabled = false\n{}\n{}",
                authored.authored_toml().unwrap(),
                authored.commands_toml().unwrap()
            ),
        )
        .unwrap();

        let answers = RenderAnswers::from_answers_file(&path).unwrap();
        let model = RepositoryRenderModel::from_answers(&answers).unwrap();
        assert!(
            model
                .go_ci_input_paths()
                .contains(&"services/api/**".into())
        );
        assert_eq!(
            model
                .go_ci_input_paths()
                .contains(&"shared/proto/**".into()),
            add_aliases
        );
        assert_eq!(
            [
                answers.go_fmt_ci_target(),
                answers.go_lint_ci_target(),
                answers.go_test_locked_ci_target(),
            ],
            expected.map(|target| target.map(str::to_owned))
        );
        assert_eq!(
            answers.go_ci_workflow_enabled(),
            expected.iter().all(Option::is_some)
        );
    }
}
