use super::*;

const VALID_POLICY: &str =
    "version=1\n[[rules]]\nid=\"source\"\ninclude=[\"**/*.rs\"]\nmax_lines=100000\n";
const EXPIRED_POLICY: &str = "version=1\n[[rules]]\nid=\"source\"\ninclude=[\"**/*.rs\"]\nmax_lines=100000\n[[waivers]]\nid=\"legacy\"\nrule=\"source\"\npath=\"src/legacy.rs\"\nceiling_lines=200000\nreason=\"tracked\"\nexpires=2001-01-01\n";

#[test]
fn rendered_launcher_and_seed_policy_keep_exactly_one_terminal_newline() {
    let _guard = lock_env();
    let template = materialize_template_worktree();
    let temp = tempdir().unwrap();
    for (name, source) in [
        ("filesystem", template.path().display().to_string()),
        ("embedded", "embedded:jig-sh".to_owned()),
    ] {
        let destination = temp.path().join(name);
        run_init(InitOpts {
            path: destination.clone(),
            scaffold: ScaffoldOpts {
                preset: Some(ScaffoldPreset::RustLibrary),
                ..ScaffoldOpts::default()
            },
            template: Some(source),
            template_mode: None,
            vcs_ref: None,
            force: false,
            defaults: false,
            no_input: true,
            no_vault: true,
            answers: AnswerOpts {
                repo_name: Some("ExampleProject".into()),
                ..AnswerOpts::default()
            },
        })
        .unwrap();

        for path in ["scripts/jig", ".jig/file-budget.toml"] {
            let rendered = fs::read(destination.join(path)).unwrap();
            assert!(
                rendered.ends_with(b"\n"),
                "{name}: {path} must end with a newline"
            );
            assert!(
                !rendered.ends_with(b"\n\n"),
                "{name}: {path} has an extra terminal newline"
            );
        }
    }
}

#[test]
fn forced_init_guidance_uses_the_policy_preserved_in_the_destination() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();

    for (name, template_has_policy, authored_policy, audit_available) in [
        ("invalid-authored", true, "", false),
        ("valid-authored-without-seed", false, VALID_POLICY, true),
        ("expired-authored", true, EXPIRED_POLICY, false),
    ] {
        let template = materialize_template_worktree();
        if !template_has_policy {
            fs::remove_file(
                template
                    .path()
                    .join("templates/project/.jig/file-budget.toml.jinja"),
            )
            .unwrap();
        }
        let destination = temp.path().join(name);
        fs::create_dir_all(destination.join(".jig")).unwrap();
        fs::write(destination.join(".jig/file-budget.toml"), authored_policy).unwrap();

        let output = run_init(InitOpts {
            path: destination.clone(),
            scaffold: ScaffoldOpts {
                preset: Some(ScaffoldPreset::RustLibrary),
                ..ScaffoldOpts::default()
            },
            template: Some(template.path().display().to_string()),
            template_mode: None,
            vcs_ref: None,
            force: true,
            defaults: false,
            no_input: true,
            no_vault: true,
            answers: AnswerOpts {
                repo_name: Some("ExampleProject".into()),
                ..AnswerOpts::default()
            },
        })
        .unwrap();

        assert_eq!(
            fs::read_to_string(destination.join(".jig/file-budget.toml")).unwrap(),
            authored_policy,
            "{name}: authored policy should be preserved"
        );
        for path in ["AGENTS.md", "README.md"] {
            let content = fs::read_to_string(destination.join(path)).unwrap();
            assert_eq!(
                content.contains("scripts/jig file-budget audit"),
                audit_available,
                "{name}: {path}"
            );
        }
        assert_eq!(
            output["notes"]
                .to_string()
                .contains("scripts/jig file-budget audit"),
            audit_available,
            "{name}: init notes"
        );
    }
}

#[test]
fn authored_policy_at_the_size_limit_is_available_but_oversized_is_not() {
    let temp = tempdir().unwrap();
    let repo = temp.path().join("repo");
    fs::create_dir_all(repo.join(".jig")).unwrap();
    let mut authored = authored_mixed_repository_config();
    authored.as_table_mut().unwrap().insert(
        "repo_name".into(),
        toml::Value::String("ExampleProject".into()),
    );
    fs::write(
        repo.join(".jig.toml"),
        toml::to_string_pretty(&authored).unwrap(),
    )
    .unwrap();
    let answers = RenderAnswers::from_answers_file(&repo.join(".jig.toml")).unwrap();
    let policy_path = repo.join(".jig/file-budget.toml");
    let mut policy = VALID_POLICY.as_bytes().to_vec();
    policy.resize(jig_file_budget::MAX_POLICY_BYTES_V1, b' ');
    fs::write(&policy_path, &policy).unwrap();
    assert!(crate::renderer::file_budget_audit_available(&repo, Some(&repo), &answers).unwrap());

    policy.push(b' ');
    fs::write(&policy_path, &policy).unwrap();
    assert!(!crate::renderer::file_budget_audit_available(&repo, Some(&repo), &answers).unwrap());
}
