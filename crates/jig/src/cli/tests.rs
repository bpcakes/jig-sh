use super::*;

#[test]
fn parses_check_namespace_commands() {
    let fmt = Cli::try_parse_from(["jig", "check", "fmt", "--plan-id", "plan_1"]).unwrap();
    match fmt.command {
        CommandKind::Check(CheckCommand::Fmt(opts)) => {
            assert_eq!(opts.plan_id.as_deref(), Some("plan_1"));
        }
        other => panic!("expected check fmt command, got {other:?}"),
    }

    let rust_file_loc = Cli::try_parse_from(["jig", "check", "rust-file-loc", "--all"]).unwrap();
    match rust_file_loc.command {
        CommandKind::Check(CheckCommand::RustFileLoc(opts)) => {
            assert!(opts.all);
        }
        other => panic!("expected check rust-file-loc command, got {other:?}"),
    }

    let ts_typecheck = Cli::try_parse_from([
        "jig",
        "check",
        "typescript-typecheck",
        "--plan-id",
        "plan_2",
    ])
    .unwrap();
    match ts_typecheck.command {
        CommandKind::Check(CheckCommand::TypeScriptTypecheck(opts)) => {
            assert_eq!(opts.plan_id.as_deref(), Some("plan_2"));
        }
        other => panic!("expected check typescript-typecheck command, got {other:?}"),
    }

    for (command, expected) in [
        ("typescript-lint", "lint"),
        ("typescript-build", "build"),
        ("typescript-coverage", "coverage"),
    ] {
        let parsed = Cli::try_parse_from(["jig", "check", command]).unwrap();
        match (parsed.command, expected) {
            (CommandKind::Check(CheckCommand::TypeScriptLint(_)), "lint") => {}
            (CommandKind::Check(CheckCommand::TypeScriptBuild(_)), "build") => {}
            (CommandKind::Check(CheckCommand::TypeScriptCoverage(_)), "coverage") => {}
            (other, _) => panic!("expected check {command} command, got {other:?}"),
        }
    }
}

#[test]
fn parses_hidden_sqlx_todo_generator_for_compatibility() {
    let cli = Cli::try_parse_from([
        "jig",
        "generate-sqlx-unchecked-queries-todo",
        "sqlx-todo.md",
    ])
    .unwrap();

    match cli.command {
        CommandKind::GenerateSqlxUncheckedQueriesTodo(opts) => {
            assert_eq!(opts.output, Some(PathBuf::from("sqlx-todo.md")));
        }
        other => panic!("expected hidden SQLx TODO generator command, got {other:?}"),
    }
}

#[test]
fn parses_prompt_registry_commands() {
    let get = Cli::try_parse_from([
        "jig",
        "prompt",
        "get",
        "repo:review-loop",
        "--var",
        "base=main",
    ])
    .unwrap();
    match get.command {
        CommandKind::Prompt(PromptCommand::Get(opts)) => {
            assert_eq!(opts.name, "repo:review-loop");
            assert_eq!(opts.vars, vec!["base=main"]);
        }
        other => panic!("expected prompt get command, got {other:?}"),
    }

    let cat = Cli::try_parse_from(["jig", "prompt", "cat", "review-loop"]).unwrap();
    match cat.command {
        CommandKind::Prompt(PromptCommand::Get(opts)) => {
            assert_eq!(opts.name, "review-loop");
        }
        other => panic!("expected prompt get command from cat alias, got {other:?}"),
    }

    let cp = Cli::try_parse_from(["jig", "prompt", "cp", "review-loop"]).unwrap();
    match cp.command {
        CommandKind::Prompt(PromptCommand::Copy(opts)) => {
            assert_eq!(opts.name, "review-loop");
        }
        other => panic!("expected prompt copy command from cp alias, got {other:?}"),
    }

    let add = Cli::try_parse_from([
        "jig",
        "prompt",
        "add",
        "comprehensive-review-loop",
        "body",
        "--description",
        "Review loop",
        "--tag",
        "review",
    ])
    .unwrap();
    match add.command {
        CommandKind::Prompt(PromptCommand::Add(opts)) => {
            assert_eq!(opts.name.as_deref(), Some("comprehensive-review-loop"));
            assert_eq!(opts.body.as_deref(), Some("body"));
            assert!(!opts.no_editor);
            assert_eq!(opts.description.as_deref(), Some("Review loop"));
            assert_eq!(opts.tags, vec!["review"]);
        }
        other => panic!("expected prompt add command, got {other:?}"),
    }

    let new = Cli::try_parse_from(["jig", "prompt", "new", "review-loop", "body"]).unwrap();
    match new.command {
        CommandKind::Prompt(PromptCommand::Add(opts)) => {
            assert_eq!(opts.name.as_deref(), Some("review-loop"));
            assert_eq!(opts.body.as_deref(), Some("body"));
            assert!(!opts.no_editor);
        }
        other => panic!("expected prompt add command from new alias, got {other:?}"),
    }

    let new_no_editor =
        Cli::try_parse_from(["jig", "prompt", "new", "review-loop", "--no-editor"]).unwrap();
    match new_no_editor.command {
        CommandKind::Prompt(PromptCommand::Add(opts)) => {
            assert_eq!(opts.name.as_deref(), Some("review-loop"));
            assert_eq!(opts.body, None);
            assert!(opts.no_editor);
        }
        other => {
            panic!("expected prompt add command from new alias with --no-editor, got {other:?}")
        }
    }

    let edit_no_editor =
        Cli::try_parse_from(["jig", "prompt", "edit", "review-loop", "--no-editor"]).unwrap();
    match edit_no_editor.command {
        CommandKind::Prompt(PromptCommand::Edit(opts)) => {
            assert_eq!(opts.name, "review-loop");
            assert!(opts.no_editor);
        }
        other => panic!("expected prompt edit command with --no-editor, got {other:?}"),
    }

    let interactive_add = Cli::try_parse_from(["jig", "prompt", "add"]).unwrap();
    match interactive_add.command {
        CommandKind::Prompt(PromptCommand::Add(opts)) => {
            assert_eq!(opts.name, None);
            assert_eq!(opts.body, None);
            assert_eq!(opts.file, None);
            assert!(!opts.no_editor);
        }
        other => panic!("expected prompt add command, got {other:?}"),
    }

    let named_interactive_add =
        Cli::try_parse_from(["jig", "prompt", "add", "review-loop"]).unwrap();
    match named_interactive_add.command {
        CommandKind::Prompt(PromptCommand::Add(opts)) => {
            assert_eq!(opts.name.as_deref(), Some("review-loop"));
            assert_eq!(opts.body, None);
            assert_eq!(opts.file, None);
            assert!(!opts.no_editor);
        }
        other => panic!("expected prompt add command, got {other:?}"),
    }

    let list_without_packs = Cli::try_parse_from(["jig", "prompt", "list", "--no-packs"]).unwrap();
    match list_without_packs.command {
        CommandKind::Prompt(PromptCommand::List(opts)) => {
            assert!(opts.no_packs);
        }
        other => panic!("expected prompt list command, got {other:?}"),
    }

    let ls = Cli::try_parse_from(["jig", "prompt", "ls", "--no-packs"]).unwrap();
    match ls.command {
        CommandKind::Prompt(PromptCommand::List(opts)) => {
            assert!(opts.no_packs);
        }
        other => panic!("expected prompt list command from ls alias, got {other:?}"),
    }

    let find = Cli::try_parse_from(["jig", "prompt", "find", "review", "--body"]).unwrap();
    match find.command {
        CommandKind::Prompt(PromptCommand::Search(opts)) => {
            assert_eq!(opts.query, "review");
            assert!(opts.body);
        }
        other => panic!("expected prompt search command from find alias, got {other:?}"),
    }

    let rm = Cli::try_parse_from(["jig", "prompt", "rm", "review-loop"]).unwrap();
    match rm.command {
        CommandKind::Prompt(PromptCommand::Remove(opts)) => {
            assert_eq!(opts.name, "review-loop");
        }
        other => panic!("expected prompt remove command from rm alias, got {other:?}"),
    }
}

#[test]
fn prompt_raw_conflicts_with_template_vars() {
    let error = Cli::try_parse_from([
        "jig",
        "prompt",
        "get",
        "literal",
        "--raw",
        "--var",
        "name=value",
    ])
    .unwrap_err();

    assert_eq!(error.kind(), clap::error::ErrorKind::ArgumentConflict);

    let error = Cli::try_parse_from([
        "jig",
        "prompt",
        "copy",
        "literal",
        "--raw",
        "--var",
        "name=value",
    ])
    .unwrap_err();

    assert_eq!(error.kind(), clap::error::ErrorKind::ArgumentConflict);
}

#[test]
fn parses_prompt_get_with_global_json_for_exact_output_contract() {
    let cli = Cli::try_parse_from(["jig", "--json", "prompt", "get", "review"]).unwrap();
    assert!(cli.json);
    match cli.command {
        CommandKind::Prompt(PromptCommand::Get(opts)) => {
            assert_eq!(opts.name, "review");
        }
        other => panic!("expected prompt get command, got {other:?}"),
    }
}

#[test]
fn adopt_and_init_default_to_official_template() {
    let adopt = Cli::try_parse_from(["jig", "adopt", ".", "--repo-name", "demo"]).unwrap();
    match adopt.command {
        CommandKind::Adopt(bootstrap::AdoptOpts {
            template, minimal, ..
        }) => {
            assert_eq!(template, None);
            assert!(!minimal);
        }
        other => panic!("expected adopt command, got {other:?}"),
    }

    let init = Cli::try_parse_from(["jig", "init", "/tmp/demo", "--repo-name", "demo"]).unwrap();
    match init.command {
        CommandKind::Init(bootstrap::InitOpts { template, .. }) => {
            assert_eq!(template, None);
        }
        other => panic!("expected init command, got {other:?}"),
    }
}

#[test]
fn adopt_parses_minimal_flag() {
    let adopt = Cli::try_parse_from([
        "jig",
        "adopt",
        ".",
        "--minimal",
        "--write",
        "--repo-name",
        "demo",
    ])
    .unwrap();
    match adopt.command {
        CommandKind::Adopt(opts) => {
            assert!(opts.minimal);
            assert!(opts.write);
        }
        other => panic!("expected adopt command, got {other:?}"),
    }
}

#[test]
fn adopt_accepts_json_after_subcommand() {
    let adopt = Cli::try_parse_from(["jig", "adopt", ".", "--json"]).unwrap();

    assert!(adopt.json);
    assert!(matches!(adopt.command, CommandKind::Adopt(_)));
}

#[test]
fn init_accepts_json_after_subcommand() {
    let init = Cli::try_parse_from(["jig", "init", "/tmp/demo", "--json"]).unwrap();

    assert!(init.json);
    assert!(matches!(init.command, CommandKind::Init(_)));
}

#[test]
fn update_accepts_json_after_subcommand() {
    let update = Cli::try_parse_from(["jig", "update", "--json"]).unwrap();

    assert!(update.json);
    assert!(matches!(update.command, CommandKind::Update(_)));
}

#[test]
fn init_and_adopt_parse_no_vault() {
    let init = Cli::try_parse_from(["jig", "init", "/tmp/demo", "--no-vault"]).unwrap();
    match init.command {
        CommandKind::Init(opts) => assert!(opts.no_vault),
        other => panic!("expected init command, got {other:?}"),
    }

    let adopt = Cli::try_parse_from(["jig", "adopt", ".", "--write", "--no-vault"]).unwrap();
    match adopt.command {
        CommandKind::Adopt(opts) => {
            assert!(opts.write);
            assert!(opts.no_vault);
        }
        other => panic!("expected adopt command, got {other:?}"),
    }
}

#[test]
fn parses_init_command_with_repeatable_flags() {
    let cli = Cli::try_parse_from([
        "jig",
        "init",
        "/tmp/demo",
        "--template",
        "/tmp/template",
        "--template-mode",
        "committed",
        "--repo-name",
        "demo",
        "--rust-migration-dir",
        "migrations",
        "--rust-crate-root",
        "crates",
        "--rust-crate-root",
        "libs",
        "--frontend-app",
        "frontend:web:40",
    ])
    .unwrap();

    match cli.command {
        CommandKind::Init(bootstrap::InitOpts {
            template_mode,
            answers,
            ..
        }) => {
            assert_eq!(template_mode, Some(bootstrap::TemplateMode::Committed));
            assert_eq!(answers.rust_crate_roots, vec!["crates", "libs"]);
            assert_eq!(answers.frontend_apps.len(), 1);
        }
        other => panic!("expected init command, got {other:?}"),
    }
}

#[test]
fn parses_init_scaffold_preset_frontends_and_db() {
    let cli = Cli::try_parse_from([
        "jig",
        "init",
        "demo",
        "--preset",
        "rust-react",
        "--db",
        "postgres",
        "--frontends",
        "web,landing,admin",
    ])
    .unwrap();

    match cli.command {
        CommandKind::Init(bootstrap::InitOpts { scaffold, .. }) => {
            assert_eq!(scaffold.preset, Some(bootstrap::ScaffoldPreset::RustReact));
            assert_eq!(scaffold.db, Some(bootstrap::ScaffoldDb::Postgres));
            assert!(scaffold.frontends.is_empty());
            assert_eq!(scaffold.frontend_list.len(), 3);
        }
        other => panic!("expected init command, got {other:?}"),
    }
}

#[test]
fn parses_explicit_harness_only_init_preset() {
    let cli = Cli::try_parse_from([
        "jig",
        "init",
        "demo",
        "--preset",
        "harness-only",
        "--no-input",
    ])
    .unwrap();

    match cli.command {
        CommandKind::Init(bootstrap::InitOpts { scaffold, .. }) => {
            assert_eq!(
                scaffold.preset,
                Some(bootstrap::ScaffoldPreset::HarnessOnly)
            );
            assert!(scaffold.db.is_none());
            assert!(!scaffold.has_frontends());
        }
        other => panic!("expected init command, got {other:?}"),
    }
}

#[test]
fn init_parser_allows_defaults_and_no_input_for_defaults_precedence() {
    let cli = Cli::try_parse_from([
        "jig",
        "init",
        "demo",
        "--defaults",
        "--no-input",
        "--no-vault",
    ])
    .unwrap();

    match cli.command {
        CommandKind::Init(opts) => {
            assert!(opts.defaults);
            assert!(opts.no_input);
        }
        other => panic!("expected init command, got {other:?}"),
    }
}

#[test]
fn rejects_working_tree_template_mode() {
    let error = Cli::try_parse_from([
        "jig",
        "init",
        "/tmp/demo",
        "--template",
        "/tmp/template",
        "--template-mode",
        "working-tree",
    ])
    .unwrap_err()
    .to_string();

    assert!(error.contains("invalid value 'working-tree'"));
    assert!(error.contains("committed"));
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
fn parses_work_receipts_filters() {
    let cli = Cli::try_parse_from([
        "jig",
        "work",
        "receipts",
        "--session-id",
        "session_1",
        "--plan-id",
        "plan_1",
        "--tool-name",
        tool::TEST,
        "--failed-only",
        "--limit",
        "5",
    ])
    .unwrap();

    match cli.command {
        CommandKind::Work(WorkCommand::Receipts(opts)) => {
            assert_eq!(opts.session_id.as_deref(), Some("session_1"));
            assert_eq!(opts.plan_id.as_deref(), Some("plan_1"));
            assert_eq!(opts.tool_name.as_deref(), Some(tool::TEST));
            assert!(opts.failed_only);
            assert_eq!(opts.limit, 5);
        }
        other => panic!("expected work receipts command, got {other:?}"),
    }
}

#[test]
fn parses_state_archive_command() {
    let cli = Cli::try_parse_from([
        "jig",
        "state",
        "archive",
        "--before",
        "2026-01-01",
        "--dry-run",
    ])
    .unwrap();

    match cli.command {
        CommandKind::State(StateCommand::Archive(opts)) => {
            assert_eq!(opts.before, "2026-01-01");
            assert!(opts.dry_run);
        }
        other => panic!("expected state archive command, got {other:?}"),
    }
}

#[test]
fn parses_state_summary_command() {
    let cli = Cli::try_parse_from(["jig", "state", "summary"]).unwrap();

    match cli.command {
        CommandKind::State(StateCommand::Summary) => {}
        other => panic!("expected state summary command, got {other:?}"),
    }
}

#[test]
fn parses_tool_no_receipt_flag() {
    let cli = Cli::try_parse_from(["jig", "check", "contract", "--no-receipt"]).unwrap();

    match cli.command {
        CommandKind::Check(CheckCommand::Contract(opts)) => {
            assert!(opts.no_receipt);
            assert_eq!(opts.plan_id, None);
        }
        other => panic!("expected check contract command, got {other:?}"),
    }

    let error = Cli::try_parse_from([
        "jig",
        "check",
        "contract",
        "--plan-id",
        "plan_1",
        "--no-receipt",
    ])
    .unwrap_err();
    assert_eq!(error.kind(), clap::error::ErrorKind::ArgumentConflict);
}

#[test]
fn parses_work_goal() {
    let cli = Cli::try_parse_from([
        "jig",
        "work",
        "goal",
        "--objective",
        "Migrate the API",
        "--success",
        "all handlers use the new type",
        "--validation",
        "scripts/jig check test",
        "--validation",
        "scripts/jig check clippy",
        "--constraint",
        "do not change public routes",
        "--checkpoint",
        "baseline current tests",
        "--title",
        "API migration",
        "--notes",
        "Keep changes small.",
    ])
    .unwrap();

    match cli.command {
        CommandKind::Work(WorkCommand::Goal(opts)) => {
            assert_eq!(opts.objective, "Migrate the API");
            assert_eq!(opts.success, "all handlers use the new type");
            assert_eq!(
                opts.validations,
                vec!["scripts/jig check test", "scripts/jig check clippy"]
            );
            assert_eq!(opts.constraints, vec!["do not change public routes"]);
            assert_eq!(opts.checkpoints, vec!["baseline current tests"]);
            assert_eq!(opts.title.as_deref(), Some("API migration"));
            assert_eq!(opts.notes.as_deref(), Some("Keep changes small."));
        }
        other => panic!("expected work goal command, got {other:?}"),
    }
}

#[test]
fn parses_agent_doctor_command() {
    let cli = Cli::try_parse_from(["jig", "agent", "doctor"]).unwrap();

    match cli.command {
        CommandKind::Agent(AgentCommand::Doctor) => {}
        other => panic!("expected agent doctor command, got {other:?}"),
    }

    let rejected = Cli::try_parse_from(["jig", "agent", "doctor", "--summary"]);
    assert!(rejected.is_err());
}

#[test]
fn parses_top_level_doctor_command() {
    let cli = Cli::try_parse_from(["jig", "doctor"]).unwrap();

    match cli.command {
        CommandKind::Doctor => {}
        other => panic!("expected doctor command, got {other:?}"),
    }

    let with_json = Cli::try_parse_from(["jig", "doctor", "--json"]).unwrap();
    assert!(with_json.json);
    match with_json.command {
        CommandKind::Doctor => {}
        other => panic!("expected doctor command, got {other:?}"),
    }

    let rejected = Cli::try_parse_from(["jig", "doctor", "--summary"]);
    assert!(rejected.is_err());
}

#[test]
fn parses_top_level_info_command_and_explain_alias() {
    let cli = Cli::try_parse_from(["jig", "info"]).unwrap();

    match cli.command {
        CommandKind::Info => {}
        other => panic!("expected info command, got {other:?}"),
    }

    let with_json = Cli::try_parse_from(["jig", "info", "--json"]).unwrap();
    assert!(with_json.json);
    match with_json.command {
        CommandKind::Info => {}
        other => panic!("expected info command, got {other:?}"),
    }

    let alias = Cli::try_parse_from(["jig", "explain", "--json"]).unwrap();
    assert!(alias.json);
    match alias.command {
        CommandKind::Info => {}
        other => panic!("expected info alias command, got {other:?}"),
    }

    let rejected = Cli::try_parse_from(["jig", "info", "--summary"]);
    assert!(rejected.is_err());
}

#[test]
fn parses_agent_bootstrap_marketplace() {
    let cli = Cli::try_parse_from([
        "jig",
        "agent",
        "bootstrap",
        "--marketplace",
        "../jig-skills",
    ])
    .unwrap();

    match cli.command {
        CommandKind::Agent(AgentCommand::Bootstrap(opts)) => {
            assert_eq!(opts.marketplace.as_deref(), Some("../jig-skills"));
        }
        other => panic!("expected agent bootstrap command, got {other:?}"),
    }
}

#[test]
fn parses_proxy_run_command() {
    let cli = Cli::try_parse_from([
        "jig",
        "proxy",
        "run",
        "web",
        "--kind",
        "vite",
        "--http-port",
        "1555",
        "--",
        "vite",
        "--open",
    ])
    .unwrap();

    match cli.command {
        CommandKind::Proxy(ProxyCommand::Run(opts)) => {
            assert_eq!(opts.name, "web");
            assert_eq!(opts.kind.as_deref(), Some("vite"));
            assert_eq!(opts.proxy.http_port, Some(1555));
            assert!(!opts.no_proxy);
            assert_eq!(opts.command, vec!["vite", "--open"]);
        }
        other => panic!("expected proxy run command, got {other:?}"),
    }
}

#[test]
fn parses_ephemeral_proxy_http_port() {
    let cli = Cli::try_parse_from([
        "jig",
        "proxy",
        "run",
        "web",
        "--http-port",
        "0",
        "--",
        "vite",
    ])
    .unwrap();

    match cli.command {
        CommandKind::Proxy(ProxyCommand::Run(opts)) => {
            assert_eq!(opts.proxy.http_port, Some(0));
        }
        other => panic!("expected proxy run command, got {other:?}"),
    }
}

#[test]
fn proxy_run_requires_separator_before_command() {
    let error = Cli::try_parse_from(["jig", "proxy", "run", "web", "vite"]).unwrap_err();

    assert_eq!(error.kind(), clap::error::ErrorKind::UnknownArgument);
}

#[test]
fn parses_proxy_run_no_proxy() {
    let cli = Cli::try_parse_from([
        "jig",
        "proxy",
        "run",
        "web",
        "--no-proxy",
        "--",
        "cargo",
        "run",
    ])
    .unwrap();

    match cli.command {
        CommandKind::Proxy(ProxyCommand::Run(opts)) => {
            assert!(opts.no_proxy);
            assert_eq!(opts.command, vec!["cargo", "run"]);
        }
        other => panic!("expected proxy run command, got {other:?}"),
    }
}

#[test]
fn parses_vault_commands() {
    let init = Cli::try_parse_from(["jig", "vault", "init", "--home", "/tmp/jig-vault"]).unwrap();
    match init.command {
        CommandKind::Vault(VaultCommand::Init(opts)) => {
            assert_eq!(opts.vault.home, Some(PathBuf::from("/tmp/jig-vault")));
        }
        other => panic!("expected vault init command, got {other:?}"),
    }

    let global_status = Cli::try_parse_from(["jig", "vault", "status", "--global"]).unwrap();
    match global_status.command {
        CommandKind::Vault(VaultCommand::Status(opts)) => {
            assert!(opts.vault.global);
        }
        other => panic!("expected vault status command, got {other:?}"),
    }

    let set = Cli::try_parse_from([
        "jig",
        "vault",
        "secret",
        "set",
        "api_token",
        "--value-stdin",
    ])
    .unwrap();
    match set.command {
        CommandKind::Vault(VaultCommand::Secret(VaultSecretCommand::Set(opts))) => {
            assert_eq!(opts.name, "api_token");
            assert!(opts.value_stdin);
            assert!(!opts.value_prompt);
        }
        other => panic!("expected vault secret set command, got {other:?}"),
    }

    let prompted_set = Cli::try_parse_from([
        "jig",
        "vault",
        "secret",
        "set",
        "api_token",
        "--value-prompt",
    ])
    .unwrap();
    match prompted_set.command {
        CommandKind::Vault(VaultCommand::Secret(VaultSecretCommand::Set(opts))) => {
            assert_eq!(opts.name, "api_token");
            assert!(!opts.value_stdin);
            assert!(opts.value_prompt);
        }
        other => panic!("expected vault secret set command, got {other:?}"),
    }

    let default_prompt_set =
        Cli::try_parse_from(["jig", "vault", "secret", "set", "api_token"]).unwrap();
    match default_prompt_set.command {
        CommandKind::Vault(VaultCommand::Secret(VaultSecretCommand::Set(opts))) => {
            assert_eq!(opts.name, "api_token");
            assert!(!opts.value_stdin);
            assert!(!opts.value_prompt);
        }
        other => panic!("expected vault secret set command, got {other:?}"),
    }

    let duplicate_value_source = Cli::try_parse_from([
        "jig",
        "vault",
        "secret",
        "set",
        "api_token",
        "--value-stdin",
        "--value-prompt",
    ])
    .unwrap_err();
    assert!(duplicate_value_source.to_string().contains("cannot"));

    let audit = Cli::try_parse_from(["jig", "vault", "audit", "verify"]).unwrap();
    match audit.command {
        CommandKind::Vault(VaultCommand::Audit(VaultAuditCommand::Verify(_))) => {}
        other => panic!("expected vault audit verify command, got {other:?}"),
    }

    let run = Cli::try_parse_from([
        "jig",
        "vault",
        "run",
        "--json",
        "--env",
        "TOKEN=api_token",
        "--file",
        "TOKEN_FILE=api_token",
        "--",
        "sh",
        "-c",
        "true",
    ])
    .unwrap();
    assert!(run.json);
    match run.command {
        CommandKind::Vault(VaultCommand::Run(opts)) => {
            assert_eq!(opts.env, vec!["TOKEN=api_token"]);
            assert_eq!(opts.files, vec!["TOKEN_FILE=api_token"]);
            assert_eq!(opts.command, vec!["sh", "-c", "true"]);
        }
        other => panic!("expected vault run command, got {other:?}"),
    }
}

#[test]
fn parses_proxy_state_dir() {
    let cli = Cli::try_parse_from(["jig", "proxy", "list", "--state-dir", "/tmp/jig-proxy-test"])
        .unwrap();

    match cli.command {
        CommandKind::Proxy(ProxyCommand::List(opts)) => {
            assert_eq!(
                opts.proxy.state_dir,
                Some(PathBuf::from("/tmp/jig-proxy-test"))
            );
        }
        other => panic!("expected proxy list command, got {other:?}"),
    }
}

#[test]
fn parses_proxy_alias_port_flag() {
    let cli = Cli::try_parse_from(["jig", "proxy", "alias", "api", "--port", "8080"]).unwrap();

    match cli.command {
        CommandKind::Proxy(ProxyCommand::Alias(opts)) => {
            assert_eq!(opts.name, "api");
            assert_eq!(opts.port, 8080);
        }
        other => panic!("expected proxy alias command, got {other:?}"),
    }
}

#[test]
fn proxy_alias_host_rejects_non_ip_literals_at_parse_time() {
    let error = Cli::try_parse_from([
        "jig",
        "proxy",
        "alias",
        "api",
        "--port",
        "8080",
        "--host",
        "localhost",
    ])
    .unwrap_err();

    assert_eq!(error.kind(), clap::error::ErrorKind::ValueValidation);
}

#[test]
fn proxy_ports_reject_zero_at_parse_time() {
    let alias_error =
        Cli::try_parse_from(["jig", "proxy", "alias", "api", "--port", "0"]).unwrap_err();
    assert_eq!(alias_error.kind(), clap::error::ErrorKind::ValueValidation);

    let run_error =
        Cli::try_parse_from(["jig", "proxy", "run", "web", "--port", "0", "--", "vite"])
            .unwrap_err();
    assert_eq!(run_error.kind(), clap::error::ErrorKind::ValueValidation);
}

#[test]
fn proxy_cert_trust_requires_scope_acknowledgement_at_parse_time() {
    for command in ["trust", "untrust"] {
        let error = Cli::try_parse_from(["jig", "proxy", "cert", command]).unwrap_err();

        assert_eq!(
            error.kind(),
            clap::error::ErrorKind::MissingRequiredArgument
        );
    }
}

#[test]
fn proxy_service_install_requires_scope_acknowledgement_at_parse_time() {
    let error = Cli::try_parse_from(["jig", "proxy", "service", "install"]).unwrap_err();

    assert_eq!(
        error.kind(),
        clap::error::ErrorKind::MissingRequiredArgument
    );
}

#[test]
fn parses_proxy_runtime_flags_on_prune_cert_and_service_commands() {
    let prune =
        Cli::try_parse_from(["jig", "proxy", "prune", "--state-dir", "/tmp/proxy"]).unwrap();
    match prune.command {
        CommandKind::Proxy(ProxyCommand::Prune(opts)) => {
            assert_eq!(opts.proxy.state_dir, Some(PathBuf::from("/tmp/proxy")));
        }
        other => panic!("expected proxy prune command, got {other:?}"),
    }

    let cert = Cli::try_parse_from(["jig", "proxy", "cert", "status", "--tld", "test"]).unwrap();
    match cert.command {
        CommandKind::Proxy(ProxyCommand::Cert(ProxyCertCommand::Status(opts))) => {
            assert_eq!(opts.proxy.tld.as_deref(), Some("test"));
        }
        other => panic!("expected proxy cert status command, got {other:?}"),
    }

    let cert_trust = Cli::try_parse_from([
        "jig",
        "proxy",
        "cert",
        "trust",
        "--accept-trust-scope",
        "--state-dir",
        "/tmp/proxy",
    ])
    .unwrap();
    match cert_trust.command {
        CommandKind::Proxy(ProxyCommand::Cert(ProxyCertCommand::Trust(opts))) => {
            assert!(opts.accept_trust_scope);
            assert_eq!(opts.proxy.state_dir, Some(PathBuf::from("/tmp/proxy")));
        }
        other => panic!("expected proxy cert trust command, got {other:?}"),
    }

    let cert_untrust = Cli::try_parse_from([
        "jig",
        "proxy",
        "cert",
        "untrust",
        "--accept-trust-scope",
        "--state-dir",
        "/tmp/proxy",
    ])
    .unwrap();
    match cert_untrust.command {
        CommandKind::Proxy(ProxyCommand::Cert(ProxyCertCommand::Untrust(opts))) => {
            assert!(opts.accept_trust_scope);
            assert_eq!(opts.proxy.state_dir, Some(PathBuf::from("/tmp/proxy")));
        }
        other => panic!("expected proxy cert untrust command, got {other:?}"),
    }

    let service = Cli::try_parse_from([
        "jig",
        "proxy",
        "service",
        "status",
        "--state-dir",
        "/tmp/proxy",
    ])
    .unwrap();
    match service.command {
        CommandKind::Proxy(ProxyCommand::Service(ProxyServiceCommand::Status(opts))) => {
            assert_eq!(opts.proxy.state_dir, Some(PathBuf::from("/tmp/proxy")));
        }
        other => panic!("expected proxy service status command, got {other:?}"),
    }

    let service_install = Cli::try_parse_from([
        "jig",
        "proxy",
        "service",
        "install",
        "--accept-service-scope",
        "--state-dir",
        "/tmp/proxy",
    ])
    .unwrap();
    match service_install.command {
        CommandKind::Proxy(ProxyCommand::Service(ProxyServiceCommand::Install(opts))) => {
            assert!(opts.accept_service_scope);
            assert_eq!(opts.proxy.state_dir, Some(PathBuf::from("/tmp/proxy")));
        }
        other => panic!("expected proxy service install command, got {other:?}"),
    }
}

#[test]
fn parses_hidden_proxy_no_http2_runtime_flag() {
    let cli = Cli::try_parse_from(["jig", "proxy", "start", "--foreground", "--no-http2"]).unwrap();

    match cli.command {
        CommandKind::Proxy(ProxyCommand::Start(opts)) => {
            assert!(opts.foreground);
            assert!(opts.proxy.no_http2);
        }
        other => panic!("expected proxy start command, got {other:?}"),
    }
}

#[test]
fn parses_work_status_command() {
    let cli = Cli::try_parse_from(["jig", "work", "status"]).unwrap();

    match cli.command {
        CommandKind::Work(WorkCommand::Status) => {}
        other => panic!("expected work status command, got {other:?}"),
    }

    let rejected = Cli::try_parse_from(["jig", "work", "status", "--summary"]);
    assert!(rejected.is_err());
}

#[test]
fn parses_work_start_print_plan_id() {
    let cli = Cli::try_parse_from([
        "jig",
        "work",
        "start",
        "--title",
        "DX polish",
        "--body",
        "Improve workflow.",
        "--print-plan-id",
    ])
    .unwrap();

    match cli.command {
        CommandKind::Work(WorkCommand::Start(opts)) => {
            assert_eq!(opts.title, "DX polish");
            assert_eq!(opts.body.as_deref(), Some("Improve workflow."));
            assert!(opts.print_plan_id);
        }
        other => panic!("expected work start command, got {other:?}"),
    }
}

#[test]
fn parses_work_check_tools() {
    let cli = Cli::try_parse_from([
        "jig",
        "work",
        "check",
        "--plan-id",
        "plan_1",
        "--tool",
        tool::CONTRACT_CHECK,
        "--tool",
        tool::TEST,
    ])
    .unwrap();

    match cli.command {
        CommandKind::Work(WorkCommand::Check(opts)) => {
            assert_eq!(opts.plan_id, "plan_1");
            assert_eq!(opts.tools, vec![tool::CONTRACT_CHECK, tool::TEST]);
        }
        other => panic!("expected work check command, got {other:?}"),
    }
}

#[test]
fn parses_work_gates_command() {
    let cli = Cli::try_parse_from(["jig", "work", "gates", "--plan-id", "plan_1"]).unwrap();

    match cli.command {
        CommandKind::Work(WorkCommand::Gates(opts)) => {
            assert_eq!(opts.plan_id.as_deref(), Some("plan_1"));
        }
        other => panic!("expected work gates command, got {other:?}"),
    }

    let inferred_plan = Cli::try_parse_from(["jig", "work", "gates"]).unwrap();

    match inferred_plan.command {
        CommandKind::Work(WorkCommand::Gates(opts)) => {
            assert_eq!(opts.plan_id, None);
        }
        other => panic!("expected work gates command, got {other:?}"),
    }
}

#[test]
fn parses_work_evidence_command() {
    let cli = Cli::try_parse_from(["jig", "work", "evidence"]).unwrap();

    match cli.command {
        CommandKind::Work(WorkCommand::Evidence(opts)) => {
            assert_eq!(opts.plan_id, None);
        }
        other => panic!("expected work evidence command, got {other:?}"),
    }

    let with_plan =
        Cli::try_parse_from(["jig", "work", "evidence", "--plan-id", "plan_1"]).unwrap();

    match with_plan.command {
        CommandKind::Work(WorkCommand::Evidence(opts)) => {
            assert_eq!(opts.plan_id.as_deref(), Some("plan_1"));
        }
        other => panic!("expected work evidence command, got {other:?}"),
    }
}

#[test]
fn parses_work_review_command() {
    let cli = Cli::try_parse_from([
        "jig",
        "work",
        "review",
        "--plan-id",
        "plan_1",
        "--gate",
        "rust-error-handling",
    ])
    .unwrap();

    match cli.command {
        CommandKind::Work(WorkCommand::Review(opts)) => {
            assert_eq!(opts.plan_id, "plan_1");
            assert_eq!(opts.gates, vec!["rust-error-handling"]);
        }
        other => panic!("expected work review command, got {other:?}"),
    }
}

#[test]
fn parses_work_refine_command() {
    let cli = Cli::try_parse_from([
        "jig",
        "work",
        "refine",
        "--plan-id",
        "plan_1",
        "--gate",
        "rust-error-handling",
        "--max-iterations",
        "2",
    ])
    .unwrap();

    match cli.command {
        CommandKind::Work(WorkCommand::Refine(opts)) => {
            assert_eq!(opts.plan_id, "plan_1");
            assert_eq!(opts.gates, vec!["rust-error-handling"]);
            assert_eq!(opts.max_iterations, 2);
        }
        other => panic!("expected work refine command, got {other:?}"),
    }
}
