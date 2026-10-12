use super::*;

#[test]
fn loop_config_accepts_compiled_in_workflow_kinds() {
    let config: RepoConfig = toml::from_str(
        r#"_src_path = "/tmp/template"
_commit = "abc123"
repo_name = "demo"
default_branch = "main"
jig_version = "0.2.0-beta.1"

[loop]
lease_ttl_seconds = 60
max_attempts = 2

[[loop.workflows]]
id = "status-check"
kind = "noop_status"

[[loop.workflows]]
id = "pr-status"
kind = "github_pr_status"

[[loop.workflows]]
id = "pr-manager"
kind = "pr_manager"
codex_home = "work"
"#,
    )
    .unwrap();

    validate_config(&config).unwrap();
}

#[test]
fn loop_config_rejects_codex_home_for_non_codex_workflow() {
    let config: RepoConfig = toml::from_str(
        r#"_src_path = "/tmp/template"
_commit = "abc123"
repo_name = "demo"
default_branch = "main"
jig_version = "0.2.0-beta.1"

[[loop.workflows]]
id = "pr-status"
kind = "github_pr_status"
codex_home = "work"
"#,
    )
    .unwrap();

    let error = validate_config(&config).unwrap_err().to_string();
    assert!(error.contains("can set codex_home only when kind is 'pr_manager' or 'codex_task'"));
}

#[test]
fn loop_config_rejects_an_empty_codex_home() {
    let config: RepoConfig = toml::from_str(
        r#"_src_path = "/tmp/template"
_commit = "abc123"
repo_name = "demo"
default_branch = "main"
jig_version = "0.2.0-beta.1"

[[loop.workflows]]
id = "pr-manager"
kind = "pr_manager"
codex_home = ""
"#,
    )
    .unwrap();

    let error = validate_config(&config).unwrap_err().to_string();
    assert!(error.contains("codex_home must not be empty"));
}

#[test]
fn loop_config_rejects_unknown_workflow_kinds() {
    let config: RepoConfig = toml::from_str(
        r#"_src_path = "/tmp/template"
_commit = "abc123"
repo_name = "demo"
default_branch = "main"
jig_version = "0.2.0-beta.1"

[[loop.workflows]]
id = "pr-manager"
kind = "github_pr_loop"
"#,
    )
    .unwrap();

    let error = validate_config(&config).unwrap_err().to_string();
    assert!(error.contains("Unsupported loop workflow kind 'github_pr_loop'"));
}

#[test]
fn loop_config_rejects_zero_backoff() {
    let config: RepoConfig = toml::from_str(
        r#"_src_path = "/tmp/template"
_commit = "abc123"
repo_name = "demo"
default_branch = "main"
jig_version = "0.2.0-beta.1"

[loop]
backoff_seconds = 0
"#,
    )
    .unwrap();

    let error = validate_config(&config).unwrap_err().to_string();
    assert!(error.contains("[loop].backoff_seconds must be greater than zero"));
}

#[test]
fn loop_config_rejects_colon_in_workflow_ids() {
    let config: RepoConfig = toml::from_str(
        r#"_src_path = "/tmp/template"
_commit = "abc123"
repo_name = "demo"
default_branch = "main"
jig_version = "0.2.0-beta.1"

[[loop.workflows]]
id = "status:check"
kind = "noop_status"
"#,
    )
    .unwrap();

    let error = validate_config(&config).unwrap_err().to_string();
    assert!(error.contains("Unsupported loop workflow id value 'status:check'"));
}

#[test]
fn loop_config_rejects_invalid_schedule_and_timezone() {
    let invalid_schedule: RepoConfig = toml::from_str(
        r#"_src_path = "/tmp/template"
_commit = "abc123"
repo_name = "demo"
default_branch = "main"
jig_version = "0.2.0-beta.1"

[[loop.workflows]]
id = "nightly"
kind = "codex_task"
schedule = "0 0 2 * * *"
timezone = "UTC"
prompt_file = ".agent/tasks/nightly.md"
"#,
    )
    .unwrap();
    let error = validate_config(&invalid_schedule).unwrap_err().to_string();
    assert!(error.contains("invalid five-field cron schedule"));

    let invalid_timezone: RepoConfig = toml::from_str(
        r#"_src_path = "/tmp/template"
_commit = "abc123"
repo_name = "demo"
default_branch = "main"
jig_version = "0.2.0-beta.1"

[[loop.workflows]]
id = "nightly"
kind = "codex_task"
schedule = "0 2 * * *"
timezone = "Prague"
prompt_file = ".agent/tasks/nightly.md"
"#,
    )
    .unwrap();
    let error = validate_config(&invalid_timezone).unwrap_err().to_string();
    assert!(error.contains("invalid IANA timezone 'Prague'"));
}

#[test]
fn loop_config_rejects_schedule_without_a_calendar_occurrence() {
    let impossible: RepoConfig = toml::from_str(
        r#"_src_path = "/tmp/template"
_commit = "abc123"
repo_name = "demo"
default_branch = "main"
jig_version = "0.2.0-beta.1"

[[loop.workflows]]
id = "impossible"
kind = "codex_task"
schedule = "0 0 31 6 *"
timezone = "UTC"
prompt_file = ".agent/tasks/impossible.md"
"#,
    )
    .unwrap();

    let error = validate_config(&impossible).unwrap_err().to_string();
    assert!(error.contains("has no possible calendar occurrence"));

    let leap_day: RepoConfig = toml::from_str(
        r#"_src_path = "/tmp/template"
_commit = "abc123"
repo_name = "demo"
default_branch = "main"
jig_version = "0.2.0-beta.1"

[[loop.workflows]]
id = "leap-day"
kind = "codex_task"
schedule = "0 0 29 2 *"
timezone = "UTC"
prompt_file = ".agent/tasks/leap-day.md"
"#,
    )
    .unwrap();

    validate_config(&leap_day).unwrap();
}

#[test]
fn loop_config_requires_safe_codex_task_fields() {
    let missing_prompt: RepoConfig = toml::from_str(
        r#"_src_path = "/tmp/template"
_commit = "abc123"
repo_name = "demo"
default_branch = "main"
jig_version = "0.2.0-beta.1"

[[loop.workflows]]
id = "nightly"
kind = "codex_task"
schedule = "0 2 * * *"
"#,
    )
    .unwrap();
    let error = validate_config(&missing_prompt).unwrap_err().to_string();
    assert!(error.contains("requires prompt_file"));

    let escaping_prompt: RepoConfig = toml::from_str(
        r#"_src_path = "/tmp/template"
_commit = "abc123"
repo_name = "demo"
default_branch = "main"
jig_version = "0.2.0-beta.1"

[[loop.workflows]]
id = "nightly"
kind = "codex_task"
schedule = "0 2 * * *"
prompt_file = "../outside.md"
"#,
    )
    .unwrap();
    let error = validate_config(&escaping_prompt).unwrap_err().to_string();
    assert!(error.contains("repository-relative path without '..'"));

    let full_access: RepoConfig = toml::from_str(
        r#"_src_path = "/tmp/template"
_commit = "abc123"
repo_name = "demo"
default_branch = "main"
jig_version = "0.2.0-beta.1"

[[loop.workflows]]
id = "nightly"
kind = "codex_task"
schedule = "0 2 * * *"
prompt_file = ".agent/tasks/nightly.md"
sandbox = "danger-full-access"
"#,
    )
    .unwrap();
    let error = validate_config(&full_access).unwrap_err().to_string();
    assert!(error.contains("sandbox must be 'read-only' or 'workspace-write'"));
}

#[test]
fn loop_config_rejects_task_fields_on_other_workflows() {
    let config: RepoConfig = toml::from_str(
        r#"_src_path = "/tmp/template"
_commit = "abc123"
repo_name = "demo"
default_branch = "main"
jig_version = "0.2.0-beta.1"

[[loop.workflows]]
id = "status"
kind = "noop_status"
prompt_file = ".agent/tasks/status.md"
"#,
    )
    .unwrap();

    let error = validate_config(&config).unwrap_err().to_string();
    assert!(error.contains("can set prompt_file only when kind = 'codex_task'"));
}

#[test]
fn loop_config_validates_isolated_preparation_command() {
    let base = r#"_src_path = "/tmp/template"
_commit = "abc123"
repo_name = "ExampleProject"
default_branch = "main"
jig_version = "0.2.0-beta.1"

[[loop.workflows]]
id = "nightly"
kind = "codex_task"
schedule = "0 2 * * *"
prompt_file = "tasks/nightly.md"
"#;
    let valid: RepoConfig = toml::from_str(&format!(
        "{base}prepare_command = [\"./scripts/prepare.sh\", \"--offline\"]\n"
    ))
    .unwrap();
    validate_config(&valid).unwrap();

    for (fields, expected) in [
        ("prepare_command = []", "non-empty argument array"),
        ("prepare_command = [\"\"]", "non-empty argument array"),
        (
            "checkout = \"repo\"\nprepare_command = [\"./scripts/prepare.sh\"]",
            "requires a worktree checkout",
        ),
    ] {
        let config: RepoConfig = toml::from_str(&format!("{base}{fields}\n")).unwrap();
        assert!(
            validate_config(&config)
                .unwrap_err()
                .to_string()
                .contains(expected)
        );
    }
}
