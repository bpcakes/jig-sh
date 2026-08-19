use clap::CommandFactory;

use super::*;

fn rendered_help(path: &[&str]) -> String {
    let mut command = Cli::command();
    let mut current = &mut command;
    for (index, name) in path.iter().enumerate() {
        current = current.find_subcommand_mut(name).unwrap_or_else(|| {
            panic!("missing subcommand {name:?} at index {index} in path {path:?}")
        });
    }
    current.render_help().to_string()
}

fn rendered_long_help(path: &[&str]) -> String {
    let mut command = Cli::command();
    let mut current = &mut command;
    for (index, name) in path.iter().enumerate() {
        current = current.find_subcommand_mut(name).unwrap_or_else(|| {
            panic!("missing subcommand {name:?} at index {index} in path {path:?}")
        });
    }
    current.render_long_help().to_string()
}

fn assert_help_contains(help: &str, expected: &str) {
    assert!(
        help.contains(expected),
        "expected rendered help to contain {expected:?}\n\n{help}"
    );
}

fn assert_help_omits(help: &str, unexpected: &str) {
    assert!(
        !help.contains(unexpected),
        "expected rendered help to omit {unexpected:?}\n\n{help}"
    );
}

#[test]
fn top_level_help_describes_common_commands() {
    let help = Cli::command().render_help().to_string();

    assert_help_contains(&help, "init");
    assert_help_contains(&help, "Create a new repository");
    assert_help_contains(&help, "check");
    assert_help_contains(&help, "Run configured project checks");
    assert_help_contains(&help, "doctor");
    assert_help_contains(&help, "Report repo harness readiness");
    assert_help_contains(&help, "info");
    assert_help_contains(&help, "Summarize repo Jig configuration");
    assert_help_contains(&help, "Manage structured work plans");
    assert_help_contains(&help, "Inspect or bootstrap local agent tooling");
    assert_help_contains(
        &help,
        "Inspect Codex homes, launch Codex, or resume a session from its owning home",
    );
    assert_help_contains(&help, "Manage user, repo, and prompt-pack prompt libraries");
    assert_help_omits(&help, "generate-sqlx-unchecked-queries-todo");
}

#[test]
fn top_level_help_orders_commands_by_user_intent() {
    let help = Cli::command().render_help().to_string();
    let ordered_commands = [
        "  init ",
        "  presets ",
        "  adopt ",
        "  update ",
        "  bootstrap ",
        "  doctor ",
        "  info ",
        "  dev ",
        "  check ",
        "  status ",
        "  ui ",
        "  work ",
        "  loop ",
        "  sqlx ",
        "  vault ",
        "  proxy ",
        "  prompt ",
        "  agent ",
        "  agent-map ",
        "  state ",
        "  mcp ",
    ];

    let mut previous = 0;
    for command in ordered_commands {
        let position = help
            .find(command)
            .unwrap_or_else(|| panic!("missing command marker {command:?}\n\n{help}"));
        assert!(
            position >= previous,
            "expected {command:?} after the preceding command\n\n{help}"
        );
        previous = position;
    }
}

#[test]
fn command_inventory_names_track_the_visible_root_surface() {
    let command = Cli::command();
    let mut visible_commands = command
        .get_subcommands()
        .filter(|command| !command.is_hide_set() && command.get_name() != "help")
        .collect::<Vec<_>>();
    visible_commands.sort_by_key(|command| command.get_display_order());
    let visible_commands = visible_commands
        .into_iter()
        .map(|command| command.get_name())
        .collect::<Vec<_>>();

    let described_commands = crate::root_commands::ALL
        .iter()
        .map(|command| command.name)
        .collect::<Vec<_>>();

    assert_eq!(visible_commands, described_commands);
}

#[test]
fn top_level_help_includes_common_workflows() {
    let help = Cli::command().render_help().to_string();

    assert_help_contains(&help, "Common workflows:");
    assert_help_contains(&help, "jig doctor");
    assert_help_contains(&help, "jig info --commands");
    assert_help_contains(&help, "jig dev");
    assert_help_contains(&help, "jig check test");
    assert_help_contains(&help, "jig work status");
}

#[test]
fn top_level_help_groups_every_visible_command_by_user_intent() {
    let help = Cli::command().render_help().to_string();

    for category in crate::root_commands::RootCommandCategory::ALL {
        assert_help_contains(&help, category.label());
    }
    for command in crate::root_commands::ALL {
        let group_line = help
            .lines()
            .find(|line| line.trim_start().starts_with(command.category.label()))
            .unwrap_or_else(|| panic!("missing category for {}\n\n{help}", command.name));
        assert_help_contains(group_line, command.name);
    }
}

#[test]
fn doctor_help_includes_examples() {
    let doctor_help = rendered_help(&["doctor"]);
    assert_help_contains(&doctor_help, "jig doctor");
    assert_help_contains(&doctor_help, "jig doctor --json");
    assert_help_contains(&doctor_help, "Human-readable output is the default");
}

#[test]
fn info_help_includes_examples_and_alias() {
    let info_help = rendered_help(&["info"]);
    assert_help_contains(&info_help, "jig info");
    assert_help_contains(&info_help, "jig info --json");
    assert_help_contains(&info_help, "jig info --commands --json");
    assert_help_contains(&info_help, "--commands");
    assert_help_contains(&info_help, "jig explain --json");
    assert_help_contains(&info_help, "Human-readable output is the default");
}

#[test]
fn status_help_includes_text_json_and_tui_modes() {
    let status_help = rendered_help(&["status"]);
    assert_help_contains(&status_help, "jig status --json");
    assert_help_contains(&status_help, "jig status --tui");
    assert_help_contains(&status_help, "--refresh-seconds");
    assert_help_contains(&status_help, "interactive dashboard");
}

#[test]
fn presets_help_includes_harness_only_automation_example() {
    let presets_help = rendered_help(&["presets"]);

    assert_help_contains(
        &presets_help,
        "jig init ./my-repo --preset harness-only --no-input --no-vault",
    );
}

#[test]
fn nested_help_describes_work_and_agent_commands() {
    let work_help = Cli::command()
        .find_subcommand_mut("work")
        .unwrap()
        .render_help()
        .to_string();
    assert_help_contains(&work_help, "start");
    assert_help_contains(&work_help, "Start a structured work plan");
    assert_help_contains(&work_help, "gates");
    assert_help_contains(&work_help, "Show required gate status");
    assert_help_contains(&work_help, "evidence");
    assert_help_contains(&work_help, "Summarize receipt evidence");

    let agent_help = Cli::command()
        .find_subcommand_mut("agent")
        .unwrap()
        .render_help()
        .to_string();
    assert_help_contains(&agent_help, "doctor");
    assert_help_contains(&agent_help, "Report local Codex marketplace readiness");
    assert_help_contains(&agent_help, "bootstrap");
    assert_help_contains(
        &agent_help,
        "Register the configured Codex skills marketplace",
    );
}

#[test]
fn work_start_help_includes_examples() {
    let work_start_help = rendered_help(&["work", "start"]);
    assert_help_contains(&work_start_help, "jig work start --title \"Add auth\"");
    assert_help_contains(&work_start_help, "--print-plan-id");
    assert_help_contains(&work_start_help, "plan_id=\"$(jig work start");
}

#[test]
fn work_check_help_includes_examples() {
    let work_check_help = rendered_help(&["work", "check"]);
    assert_help_contains(&work_check_help, "jig work check --plan-id plan_abc123");
    assert_help_contains(&work_check_help, "--tool jig.test");
}

#[test]
fn work_evidence_help_includes_examples() {
    let work_evidence_help = rendered_help(&["work", "evidence"]);
    assert_help_contains(&work_evidence_help, "jig work evidence");
    assert_help_contains(&work_evidence_help, "--plan-id plan_abc123");
    assert_help_contains(&work_evidence_help, "changed paths covered");
}

#[test]
fn work_finish_help_includes_examples() {
    let work_finish_help = rendered_help(&["work", "finish"]);
    assert_help_contains(&work_finish_help, "jig work finish --plan-id plan_abc123");
    assert_help_contains(&work_finish_help, "--outcome success");
}

#[test]
fn check_help_includes_examples() {
    let check_help = rendered_help(&["check"]);
    assert_help_contains(&check_help, "jig check fmt");
    assert_help_contains(&check_help, "jig check contract");
    assert_help_contains(&check_help, "jig check rust-file-loc --changed-against");
}

#[test]
fn vault_help_includes_quick_start_examples() {
    let vault_help = rendered_help(&["vault"]);
    assert_help_contains(&vault_help, "JIG_VAULT_PASSPHRASE");
    assert_help_contains(&vault_help, "are project-relative:");
    assert_help_contains(&vault_help, "project name is never a reference segment");
    assert_help_contains(&vault_help, "Both concealed and text fields are encrypted");
    assert_help_contains(&vault_help, "jig vault init");
    assert_help_contains(&vault_help, "jig vault tui");
    assert_help_contains(&vault_help, "jig vault migrate --to 2");
    assert_help_contains(
        &vault_help,
        "jig vault field set jig://Production/RESTIC_PASSWORD --value-prompt",
    );
    assert_help_contains(
        &vault_help,
        "jig vault read jig://Production/RESTIC_PASSWORD | command",
    );
    assert_help_contains(
        &vault_help,
        "jig vault inject --in config.template > config",
    );
    assert_help_contains(&vault_help, "jig vault exec --env-file .env.jig -- command");
    assert_help_contains(
        &vault_help,
        "jig vault import onepassword --env-file .env.op --item Production --out-env .env.jig",
    );
    assert_help_contains(&vault_help, "jig vault passphrase change");
    assert_help_contains(
        &vault_help,
        "jig vault backup create --out ./project-vault.backup",
    );
    assert_help_contains(
        &vault_help,
        "jig vault backup restore --in ./project-vault.backup --home ./restored-vault",
    );
    assert_help_contains(&vault_help, "jig vault secret set api_token --value-prompt");

    let vault_init_help = rendered_help(&["vault", "init"]);
    assert_help_contains(&vault_init_help, "prompts twice for a new vault passphrase");
    assert_help_contains(&vault_init_help, "jig vault init");

    let vault_tui_help = rendered_help(&["vault", "tui"]);
    assert_help_contains(&vault_tui_help, "keyboard-first full-screen vault manager");
    assert_help_contains(&vault_tui_help, "five minutes");
    assert_help_contains(&vault_tui_help, "terminal-safe escaped");
    assert_help_contains(&vault_tui_help, "preview after a warning");
    assert_help_contains(&vault_tui_help, "available only on Unix");
    assert_help_contains(&vault_tui_help, "There is no clipboard integration");
    assert_help_contains(&vault_tui_help, "exec, run, and inject");

    let vault_secret_set_help = rendered_help(&["vault", "secret", "set"]);
    assert_help_contains(&vault_secret_set_help, "--value-prompt");
    assert_help_contains(&vault_secret_set_help, "use printf instead");
    assert_help_contains(&vault_secret_set_help, "of echo");
    assert_help_contains(
        &vault_secret_set_help,
        "jig vault secret set api_token --value-stdin",
    );

    let vault_migrate_help = rendered_help(&["vault", "migrate"]);
    assert_help_contains(&vault_migrate_help, "jig vault migrate --to 2");
    assert_help_contains(&vault_migrate_help, "--to <TO>");
    assert_help_contains(&vault_migrate_help, "version 1 vaults");
    assert_help_contains(&vault_migrate_help, "old Jig rejects");

    let vault_field_set_help = rendered_help(&["vault", "field", "set"]);
    assert_help_contains(&vault_field_set_help, "--text");
    assert_help_contains(&vault_field_set_help, "jig://Production/RESTIC_PASSWORD");
    assert_help_contains(&vault_field_set_help, "use printf instead");

    let vault_read_help = rendered_help(&["vault", "read"]);
    assert_help_contains(&vault_read_help, "without an added newline");
    assert_help_contains(&vault_read_help, "--out-file <PATH>");
    assert_help_contains(&vault_read_help, "--reveal");
    assert_help_contains(&vault_read_help, "currently Unix-only");

    let vault_inject_help = rendered_help(&["vault", "inject"]);
    assert_help_contains(&vault_inject_help, "--in <PATH|->");
    assert_help_contains(&vault_inject_help, "{{ jig://ITEM/FIELD }}");
    assert_help_contains(&vault_inject_help, "--overwrite");

    let vault_exec_help = rendered_help(&["vault", "exec"]);
    assert_help_contains(&vault_exec_help, "transparent developer-process wrapper");
    assert_help_contains(&vault_exec_help, "--env-file <FILE>");
    assert_help_contains(&vault_exec_help, "NAME=VALUE");
    assert_help_contains(&vault_exec_help, "Single quotes preserve");
    assert_help_contains(&vault_exec_help, "Within env-file values");
    assert_help_contains(&vault_exec_help, "older vault run command");
    assert_help_contains(&vault_exec_help, "Exec is transparent, not a sandbox");
    assert_help_contains(&vault_exec_help, "must follow --");

    let vault_import_help = rendered_help(&["vault", "import", "onepassword"]);
    assert_help_contains(&vault_import_help, "op read --no-newline");
    assert_help_contains(&vault_import_help, "op://VAULT/ITEM/FIELD");
    assert_help_contains(&vault_import_help, "--item <ITEM>");
    assert_help_contains(&vault_import_help, "--out-env <DESTINATION>");
    assert_help_contains(&vault_import_help, "never invokes `op`");
    assert_help_contains(&vault_import_help, "--replace --overwrite");
    assert_help_contains(&vault_import_help, "does not synchronize with");
    assert_help_contains(&vault_import_help, "currently Unix-only");

    let vault_passphrase_help = rendered_help(&["vault", "passphrase", "change"]);
    assert_help_contains(&vault_passphrase_help, "JIG_VAULT_PASSPHRASE");
    assert_help_contains(&vault_passphrase_help, "JIG_VAULT_NEW_PASSPHRASE");
    assert_help_contains(
        &vault_passphrase_help,
        "never accepted as command-line arguments",
    );

    let vault_backup_create_help = rendered_help(&["vault", "backup", "create"]);
    assert_help_contains(&vault_backup_create_help, "--out <FILE>");
    assert_help_contains(&vault_backup_create_help, "--overwrite");
    assert_help_contains(&vault_backup_create_help, "vault's current");
    assert_help_contains(
        &vault_backup_create_help,
        "passphrase and remains decryptable",
    );
    assert_help_contains(&vault_backup_create_help, "currently Unix-only");

    let vault_backup_restore_help = rendered_help(&["vault", "backup", "restore"]);
    assert_help_contains(&vault_backup_restore_help, "--in <FILE>");
    assert_help_contains(&vault_backup_restore_help, "complete target");
    assert_help_contains(&vault_backup_restore_help, "home must be absent");
    assert_help_contains(&vault_backup_restore_help, "--in -");
    assert_help_contains(&vault_backup_restore_help, "currently Linux-only");

    let vault_field_list_help = rendered_help(&["vault", "field", "list"]);
    assert_help_contains(&vault_field_list_help, "jig://ITEM");

    let vault_run_help = rendered_help(&["vault", "run"]);
    assert_help_contains(&vault_run_help, "--file");
    assert_help_contains(&vault_run_help, "jig vault run --file TOKEN_FILE=api_token");
}

#[test]
fn agent_help_includes_examples() {
    let agent_help = rendered_help(&["agent"]);
    assert_help_contains(&agent_help, "jig agent doctor");
    assert_help_contains(&agent_help, "jig agent bootstrap");
}

#[test]
fn codex_help_distinguishes_homes_from_configuration_profiles() {
    let codex_help = rendered_help(&["codex"]);
    assert_help_contains(&codex_help, "jig codex homes --usage");
    assert_help_contains(&codex_help, "Codex configuration profiles");

    let launch_help = rendered_help(&["codex", "launch"]);
    assert_help_contains(&launch_help, "forwarded to Codex without shell parsing");
    assert_help_contains(&launch_help, "searchable terminal picker immediately");
    assert_help_contains(&launch_help, "details load in the background");
    assert_help_contains(&launch_help, "+ marks the best projected outcome");
    assert!(!launch_help.contains('★'), "{launch_help}");
    assert_help_contains(&launch_help, "work resolves as ~/.codex-work");
    assert_help_contains(&launch_help, "use ./work to select a relative directory");
    assert_help_contains(&launch_help, "codex and default both select ~/.codex");
    assert_help_contains(
        &launch_help,
        "jig codex launch codex-1 --dry-run -- --search",
    );

    let resume_help = rendered_help(&["codex", "resume"]);
    assert_help_contains(&resume_help, "queries every discovered Codex home");
    assert_help_contains(&resume_help, "--home");
    assert_help_contains(&resume_help, "show lookup progress");
    assert_help_contains(&resume_help, "forwarded to `codex resume`");
    assert_help_contains(
        &resume_help,
        "jig codex resume 019fe6e4-972f-7392-aaf3-58cb652a4e20 --dry-run -- --search",
    );
}

#[test]
fn prompt_help_includes_registry_examples() {
    let prompt_help = rendered_help(&["prompt"]);
    assert_help_contains(&prompt_help, "get");
    assert_help_contains(
        &prompt_help,
        "Print a rendered prompt, using a command envelope with --json",
    );

    let prompt_get_help = rendered_help(&["prompt", "get"]);
    assert_help_contains(&prompt_get_help, "jig prompt get comprehensive-review-loop");
    assert_help_contains(&prompt_get_help, "--var");
    assert_help_contains(&prompt_get_help, "--json requests a command envelope");

    let prompt_export_help = rendered_help(&["prompt", "export"]);
    assert_help_contains(&prompt_export_help, "--output");

    let prompt_list_help = rendered_help(&["prompt", "list"]);
    assert_help_contains(&prompt_list_help, "--no-packs");
}

#[test]
fn agent_bootstrap_help_includes_examples() {
    let agent_bootstrap_help = rendered_help(&["agent", "bootstrap"]);
    assert_help_contains(&agent_bootstrap_help, "GitHub owner/repo skill marketplace");
    assert_help_contains(
        &agent_bootstrap_help,
        "jig agent bootstrap --marketplace owner/skills-repo",
    );
}

#[test]
fn update_help_explains_modes() {
    let update_help = rendered_help(&["update"]);
    assert_help_contains(&update_help, "jig update --recopy");
    assert_help_contains(&update_help, "changed template-managed files");
}

#[test]
fn init_help_explains_defaults_strict_input_and_harness_only() {
    let init_help = rendered_long_help(&["init"]);

    assert_help_contains(
        &init_help,
        "--defaults uses rust-react, database none, and frontend web",
    );
    assert_help_contains(
        &init_help,
        "--no-input and non-terminal execution require the project shape to be fully specified",
    );
    assert_help_contains(&init_help, "--preset harness-only --no-input --no-vault");
    assert_help_contains(&init_help, "--sqlx-enabled false --no-input --no-vault");
    assert_help_contains(
        &init_help,
        "--template-mode committed --repo-name new-repo --sqlx-enabled false --no-input --no-vault",
    );
    assert_help_contains(
        &init_help,
        "resolve omitted project-shape choices to --preset rust-react, --db none, and --frontend web",
    );
    assert_help_contains(
        &init_help,
        "effective frontend_apps from --answers-file prevent the default web scaffold",
    );
    assert_help_contains(
        &init_help,
        "Application presets require an explicit --db choice",
    );
    assert_help_contains(&init_help, "go-react also requires --go-module");
    assert_help_contains(
        &init_help,
        "Non-terminal execution without --defaults follows this strict behavior",
    );
}

#[test]
fn template_error_hint_uses_prompt_free_harness_only_init() {
    assert!(TEMPLATE_ERROR_HINT.contains(
        "jig init /path/to/new-repo --preset harness-only --repo-name new-repo --sqlx-enabled false --no-input --no-vault"
    ));
}

#[test]
fn state_archive_help_explains_cutoff() {
    let archive_help = rendered_help(&["state", "archive"]);
    assert_help_contains(&archive_help, "--before");
    assert_help_contains(&archive_help, "YYYY-MM-DD");
    assert_help_contains(&archive_help, "--dry-run");
}

#[test]
fn json_output_flag_is_discoverable() {
    let root_help = Cli::command().render_help().to_string();
    assert_help_contains(&root_help, "--json");
    assert_help_contains(
        &root_help,
        "Print structured JSON results and errors; does not disable interactive prompts",
    );

    let work_receipts_help = rendered_help(&["work", "receipts"]);
    assert_help_contains(&work_receipts_help, "work receipts --failed-only");
    assert_help_contains(&work_receipts_help, "--json");

    let work_evidence_help = rendered_help(&["work", "evidence"]);
    assert_help_contains(&work_evidence_help, "jig work evidence --json");

    let vault_run_help = rendered_help(&["vault", "run"]);
    assert_help_contains(&vault_run_help, "--json");
    assert_help_contains(&vault_run_help, "--file");

    assert_help_omits(&rendered_help(&["doctor"]), "--summary");
    assert_help_omits(&rendered_help(&["work", "status"]), "--summary");
    assert_help_omits(&rendered_help(&["agent", "doctor"]), "--summary");
}

#[test]
fn proxy_run_help_includes_launcher_context_and_examples() {
    let proxy_run_help = rendered_help(&["proxy", "run"]);
    assert_help_contains(&proxy_run_help, "The app command must come after --");
    assert_help_contains(&proxy_run_help, "[[dev.apps]].host");
    assert_help_contains(&proxy_run_help, "jig proxy run web -- npm run dev");
    assert_help_contains(&proxy_run_help, "jig proxy run web -- vite --open");
    assert_help_contains(
        &proxy_run_help,
        "jig proxy run api --port 3000 -- cargo run",
    );
    assert_help_contains(
        &proxy_run_help,
        "jig proxy run web --no-proxy -- npm run dev",
    );
}

#[test]
fn dev_help_describes_launch_and_session_management() {
    let dev_help = rendered_help(&["dev"]);
    assert_help_contains(
        &dev_help,
        "Run and manage configured development app sessions",
    );
    assert_help_contains(&dev_help, "status");
    assert_help_contains(&dev_help, "stop");
    assert_help_contains(&dev_help, "--replace");
    assert_help_contains(&dev_help, "jig dev --replace");
    assert_help_contains(&dev_help, "jig dev status");
    assert_help_contains(&dev_help, "jig dev stop");
    assert_help_omits(&dev_help, "--jig-project");

    let status_help = rendered_help(&["dev", "status"]);
    assert_help_contains(&status_help, "--state-dir");
    assert_help_omits(&status_help, "--replace");
    assert_help_omits(&status_help, "--app");

    let stop_help = rendered_help(&["dev", "stop"]);
    assert_help_contains(&stop_help, "--state-dir");
    assert_help_omits(&stop_help, "--replace");
    assert_help_omits(&stop_help, "--app");
}

#[test]
fn migration_help_includes_examples() {
    let migration_help = rendered_help(&["sqlx", "migration", "add"]);
    assert_help_contains(&migration_help, "open structured work plan");
    assert_help_contains(&migration_help, "jig sqlx migration add create_users");
    assert_help_contains(&migration_help, "--plan-id plan_abc123");

    let sqlx_help = rendered_help(&["sqlx"]);
    assert_help_contains(&sqlx_help, "migration");
    assert_help_contains(&sqlx_help, "schema");
    assert_help_contains(&sqlx_help, "jig check sqlx");

    let root_help = rendered_help(&[]);
    assert_help_omits(&root_help, "  migration-add ");
    assert_help_omits(&root_help, "  schema-dump ");

    let legacy_help = rendered_help(&["migration-add"]);
    assert_help_contains(&legacy_help, "remains accepted for compatibility");
}
