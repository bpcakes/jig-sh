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
