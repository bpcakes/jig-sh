use super::*;

mod info;
mod launcher_only;

#[test]
fn parses_canonical_and_legacy_sqlx_commands() {
    let migration = Cli::try_parse_from([
        "jig",
        "sqlx",
        "migration",
        "add",
        "create_users",
        "--plan-id",
        "plan_1",
    ])
    .unwrap();
    match migration.command {
        CommandKind::Sqlx(SqlxCommand::Migration(SqlxMigrationCommand::Add(opts))) => {
            assert_eq!(opts.name, "create_users");
            assert_eq!(opts.tool.plan_id.as_deref(), Some("plan_1"));
        }
        other => panic!("expected sqlx migration add command, got {other:?}"),
    }

    let schema = Cli::try_parse_from(["jig", "sqlx", "schema", "dump", "--no-receipt"]).unwrap();
    match schema.command {
        CommandKind::Sqlx(SqlxCommand::Schema(SqlxSchemaCommand::Dump(opts))) => {
            assert!(opts.no_receipt);
        }
        other => panic!("expected sqlx schema dump command, got {other:?}"),
    }

    assert!(matches!(
        Cli::try_parse_from(["jig", "migration-add", "create_users"])
            .unwrap()
            .command,
        CommandKind::MigrationAdd(_)
    ));
    assert!(matches!(
        Cli::try_parse_from(["jig", "schema-dump"]).unwrap().command,
        CommandKind::SchemaDump(_)
    ));
}

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
            (CommandKind::Check(CheckCommand::TypeScriptLint(_)), "lint")
            | (CommandKind::Check(CheckCommand::TypeScriptBuild(_)), "build")
            | (CommandKind::Check(CheckCommand::TypeScriptCoverage(_)), "coverage") => {}
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
fn parses_prompt_get_with_global_json_for_structured_output() {
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
fn adopt_parses_sqlx_inventory_remediation_flags() {
    for (schema_dump, extra_args) in [
        (false, Vec::<&str>::new()),
        (true, vec!["--schema-dump-enabled", "true"]),
    ] {
        let mut args = vec![
            "jig",
            "adopt",
            ".",
            "--sqlx-enabled",
            "true",
            "--rust-migration-dir",
            "migrations",
        ];
        args.extend(extra_args);

        let adopt = Cli::try_parse_from(args).unwrap();

        match adopt.command {
            CommandKind::Adopt(opts) => {
                assert_eq!(opts.answers.sqlx_enabled, Some(true));
                assert_eq!(
                    opts.answers.rust_migration_dir.as_deref(),
                    Some("migrations")
                );
                assert_eq!(
                    opts.answers.schema_dump_enabled,
                    schema_dump.then_some(true)
                );
            }
            other => panic!("expected adopt command, got {other:?}"),
        }
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
fn parses_state_maintenance_commands() {
    let cli = Cli::try_parse_from(["jig", "state", "diagnose", "--deep"]).unwrap();
    match cli.command {
        CommandKind::State(StateCommand::Diagnose(opts)) => assert!(opts.deep),
        other => panic!("expected state diagnose command, got {other:?}"),
    }

    let cli = Cli::try_parse_from(["jig", "state", "compact", "sessions", "--dry-run"]).unwrap();
    match cli.command {
        CommandKind::State(StateCommand::Compact {
            command: StateCompactCommand::Sessions(opts),
        }) => assert!(opts.dry_run),
        other => panic!("expected state compact sessions command, got {other:?}"),
    }

    let cli = Cli::try_parse_from([
        "jig",
        "state",
        "restore",
        "--backup",
        ".agent/.cache/state-backups/sessions-1",
    ])
    .unwrap();
    match cli.command {
        CommandKind::State(StateCommand::Restore(opts)) => {
            assert_eq!(
                opts.backup,
                std::path::PathBuf::from(".agent/.cache/state-backups/sessions-1")
            );
        }
        other => panic!("expected state restore command, got {other:?}"),
    }

    let cli = Cli::try_parse_from([
        "jig",
        "state",
        "export",
        "receipts",
        "--before",
        "2026-01-01",
        "--output",
        "receipts.jsonl.gz",
    ])
    .unwrap();
    match cli.command {
        CommandKind::State(StateCommand::Export {
            command: StateExportCommand::Receipts(opts),
        }) => {
            assert_eq!(opts.before, "2026-01-01");
            assert_eq!(opts.output, std::path::PathBuf::from("receipts.jsonl.gz"));
        }
        other => panic!("expected state export receipts command, got {other:?}"),
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
fn parses_top_level_setup_command() {
    let cli = Cli::try_parse_from(["jig", "setup"]).unwrap();
    assert!(matches!(cli.command, CommandKind::Setup));

    let with_json = Cli::try_parse_from(["jig", "setup", "--json"]).unwrap();
    assert!(with_json.json);
    assert!(matches!(with_json.command, CommandKind::Setup));
}

#[test]
fn web_package_manager_cli_rejects_unknown_values_during_parsing() {
    let accepted =
        Cli::try_parse_from(["jig", "init", "demo", "--web-package-manager", "pnpm"]).unwrap();
    let CommandKind::Init(opts) = accepted.command else {
        panic!("expected init command");
    };
    assert_eq!(opts.answers.web_package_manager.as_deref(), Some("pnpm"));

    let rejected = Cli::try_parse_from(["jig", "init", "demo", "--web-package-manager", "cargo"])
        .unwrap_err()
        .to_string();
    assert!(rejected.contains("possible values"));
    assert!(rejected.contains("bun"));
    assert!(rejected.contains("yarn"));
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

    let migrate = Cli::try_parse_from([
        "jig",
        "vault",
        "migrate",
        "--to",
        "2",
        "--home",
        "/tmp/jig-vault",
    ])
    .unwrap();
    match migrate.command {
        CommandKind::Vault(VaultCommand::Migrate(opts)) => {
            assert_eq!(opts.to, 2);
            assert_eq!(opts.vault.home, Some(PathBuf::from("/tmp/jig-vault")));
        }
        other => panic!("expected vault migrate command, got {other:?}"),
    }

    let field_list =
        Cli::try_parse_from(["jig", "vault", "field", "list", "jig://Production"]).unwrap();
    match field_list.command {
        CommandKind::Vault(VaultCommand::Field(VaultFieldCommand::List(opts))) => {
            assert_eq!(
                opts.item.as_ref().map(|item| item.as_str()),
                Some("Production")
            );
        }
        other => panic!("expected vault field list command, got {other:?}"),
    }

    let field_set = Cli::try_parse_from([
        "jig",
        "vault",
        "field",
        "set",
        "jig://Production/RESTIC_COMPRESSION",
        "--text",
        "--value-stdin",
    ])
    .unwrap();
    match field_set.command {
        CommandKind::Vault(VaultCommand::Field(VaultFieldCommand::Set(opts))) => {
            assert_eq!(
                opts.reference.to_string(),
                "jig://Production/RESTIC_COMPRESSION"
            );
            assert!(opts.text);
            assert!(opts.value_stdin);
            assert!(!opts.value_prompt);
        }
        other => panic!("expected vault field set command, got {other:?}"),
    }

    let field_remove = Cli::try_parse_from([
        "jig",
        "vault",
        "field",
        "remove",
        "jig://Production/RESTIC_COMPRESSION",
    ])
    .unwrap();
    match field_remove.command {
        CommandKind::Vault(VaultCommand::Field(VaultFieldCommand::Remove(opts))) => {
            assert_eq!(
                opts.reference.to_string(),
                "jig://Production/RESTIC_COMPRESSION"
            );
        }
        other => panic!("expected vault field remove command, got {other:?}"),
    }

    let read = Cli::try_parse_from([
        "jig",
        "vault",
        "read",
        "jig://Production/RESTIC_PASSWORD",
        "--out-file",
        "/tmp/restic-password",
        "--overwrite",
    ])
    .unwrap();
    match read.command {
        CommandKind::Vault(VaultCommand::Read(opts)) => {
            assert_eq!(
                opts.reference.to_string(),
                "jig://Production/RESTIC_PASSWORD"
            );
            assert_eq!(
                opts.out_file.as_deref(),
                Some(std::path::Path::new("/tmp/restic-password"))
            );
            assert!(opts.overwrite);
            assert!(!opts.reveal);
        }
        other => panic!("expected vault read command, got {other:?}"),
    }

    let inject = Cli::try_parse_from(["jig", "vault", "inject", "--in", "-", "--reveal"]).unwrap();
    match inject.command {
        CommandKind::Vault(VaultCommand::Inject(opts)) => {
            assert_eq!(opts.input, PathBuf::from("-"));
            assert!(opts.reveal);
            assert!(opts.out_file.is_none());
            assert!(!opts.overwrite);
        }
        other => panic!("expected vault inject command, got {other:?}"),
    }

    let exec = Cli::try_parse_from([
        "jig",
        "vault",
        "exec",
        "--env-file",
        ".env.jig",
        "--home",
        "/tmp/jig-vault",
        "--",
        "command",
        "--flag",
    ])
    .unwrap();
    match exec.command {
        CommandKind::Vault(VaultCommand::Exec(opts)) => {
            assert_eq!(opts.env_file, PathBuf::from(".env.jig"));
            assert_eq!(opts.vault.home, Some(PathBuf::from("/tmp/jig-vault")));
            assert_eq!(
                opts.command,
                vec![
                    std::ffi::OsString::from("command"),
                    std::ffi::OsString::from("--flag")
                ]
            );
        }
        other => panic!("expected vault exec command, got {other:?}"),
    }

    let import = Cli::try_parse_from([
        "jig",
        "vault",
        "import",
        "onepassword",
        "--env-file",
        ".env.op",
        "--item",
        "Production",
        "--out-env",
        ".env.jig",
        "--replace",
        "--overwrite",
        "--dry-run",
    ])
    .unwrap();
    match import.command {
        CommandKind::Vault(VaultCommand::Import(VaultImportCommand::OnePassword(opts))) => {
            assert_eq!(opts.env_file, PathBuf::from(".env.op"));
            assert_eq!(opts.item.to_string(), "jig://Production");
            assert_eq!(opts.out_env, PathBuf::from(".env.jig"));
            assert!(opts.replace);
            assert!(opts.overwrite);
            assert!(opts.dry_run);
        }
        other => panic!("expected vault onepassword import command, got {other:?}"),
    }
    assert!(
        Cli::try_parse_from([
            "jig",
            "vault",
            "import",
            "onepassword",
            "--env-file",
            ".env.op",
            "--item",
            "jig://Production",
            "--out-env",
            ".env.jig",
        ])
        .is_err()
    );

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

    let duplicate_field_value_source = Cli::try_parse_from([
        "jig",
        "vault",
        "field",
        "set",
        "jig://Production/RESTIC_PASSWORD",
        "--value-stdin",
        "--value-prompt",
    ])
    .unwrap_err();
    assert!(duplicate_field_value_source.to_string().contains("cannot"));

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
fn rejects_invalid_vault_field_inputs_during_clap_parsing() {
    for args in [
        vec!["jig", "vault", "migrate", "--to", "3"],
        vec!["jig", "vault", "migrate", "--to", "two"],
        vec!["jig", "vault", "field", "list", "jig://Production/extra"],
        vec!["jig", "vault", "field", "set", "jig://Production"],
        vec!["jig", "vault", "read", "jig://Production"],
        vec![
            "jig",
            "vault",
            "field",
            "remove",
            "jig://Production/RESTIC_PASSWORD?query",
        ],
    ] {
        let error = Cli::try_parse_from(args).unwrap_err();
        assert!(
            matches!(
                error.kind(),
                clap::error::ErrorKind::InvalidValue | clap::error::ErrorKind::ValueValidation
            ),
            "unexpected error kind for {error}"
        );
    }
}

#[test]
fn vault_raw_output_options_are_fail_closed_during_clap_parsing() {
    for args in [
        vec![
            "jig",
            "vault",
            "read",
            "jig://Production/PASSWORD",
            "--overwrite",
        ],
        vec![
            "jig",
            "vault",
            "read",
            "jig://Production/PASSWORD",
            "--reveal",
            "--out-file",
            "password.txt",
        ],
        vec!["jig", "vault", "inject"],
        vec!["jig", "vault", "inject", "--in", "template", "--overwrite"],
        vec![
            "jig",
            "vault",
            "inject",
            "--in",
            "template",
            "--reveal",
            "--out-file",
            "rendered",
        ],
    ] {
        let error = Cli::try_parse_from(args).unwrap_err();
        assert!(
            matches!(
                error.kind(),
                clap::error::ErrorKind::ArgumentConflict
                    | clap::error::ErrorKind::MissingRequiredArgument
            ),
            "unexpected error kind for {error}"
        );
    }
}

#[test]
fn vault_exec_requires_an_env_file_separator_and_command() {
    for args in [
        vec!["jig", "vault", "exec", "--", "command"],
        vec!["jig", "vault", "exec", "--env-file", ".env.jig"],
        vec!["jig", "vault", "exec", "--env-file", ".env.jig", "command"],
    ] {
        assert!(Cli::try_parse_from(args).is_err());
    }
}

#[test]
fn invalid_vault_fields_fail_before_passphrase_or_vault_side_effects() {
    use tempfile::tempdir;

    use crate::test_env::{EnvVarGuard, lock_env};

    let _env = lock_env();
    let temp = tempdir().unwrap();
    let vault_home = temp.path().join("vault");
    let _passphrase = EnvVarGuard::set("JIG_VAULT_PASSPHRASE", "test-passphrase");

    let error = Cli::try_parse_from([
        "jig",
        "vault",
        "field",
        "set",
        "jig://Production",
        "--home",
        vault_home.to_str().unwrap(),
    ])
    .unwrap_err();

    assert!(matches!(
        error.kind(),
        clap::error::ErrorKind::InvalidValue | clap::error::ErrorKind::ValueValidation
    ));
    assert!(std::env::var_os("JIG_VAULT_PASSPHRASE").is_some());
    assert!(!vault_home.exists());
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
fn work_start_rejects_multiple_body_sources() {
    let error = Cli::try_parse_from([
        "jig",
        "work",
        "start",
        "--title",
        "DX polish",
        "--body",
        "inline",
        "--body-file",
        "plan.md",
    ])
    .unwrap_err();

    assert_eq!(error.kind(), clap::error::ErrorKind::ArgumentConflict);
}

#[test]
fn work_append_requires_exactly_one_body_source() {
    let missing =
        Cli::try_parse_from(["jig", "work", "append", "--plan-id", "plan_1"]).unwrap_err();
    assert_eq!(
        missing.kind(),
        clap::error::ErrorKind::MissingRequiredArgument
    );

    let conflicting = Cli::try_parse_from([
        "jig",
        "work",
        "append",
        "--plan-id",
        "plan_1",
        "--body",
        "inline",
        "--body-file",
        "plan.md",
    ])
    .unwrap_err();
    assert_eq!(conflicting.kind(), clap::error::ErrorKind::ArgumentConflict);
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
