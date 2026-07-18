use super::*;

#[test]
fn agent_doctor_reports_configured_codex_marketplace() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    fs::create_dir_all(temp.path().join("bpcakes/jig-skills")).unwrap();
    let codex_home = temp.path().join("codex-home");
    fs::create_dir_all(&codex_home).unwrap();
    fs::write(
        codex_home.join("config.toml"),
        r#"[marketplaces.jig-skills]
source_type = "git"
source = "https://github.com/bpcakes/jig-skills.git"

[plugins."jig-rust@jig-skills"]
enabled = true

[plugins."jig-swift@jig-skills"]
enabled = true

[plugins."jig-typescript@jig-skills"]
enabled = true

[plugins."jig-exec-plans@jig-skills"]
enabled = true
"#,
    )
    .unwrap();
    let codex_path = temp.path().join("codex-stub.sh");
    write_codex_stub(
        &codex_path,
        "#!/bin/sh\nif [ \"$1 $2 $3 $4\" = \"plugin marketplace add --help\" ]; then exit 0; fi\nexit 2\n",
    );

    let _codex_bin = EnvVarGuard::set("JIG_CODEX_BIN", &codex_path);
    let _codex_home = EnvVarGuard::set("CODEX_HOME", &codex_home);
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let output = dispatch(&ctx, CommandKind::Agent(crate::cli::AgentCommand::Doctor)).unwrap();

    assert_eq!(output["ok"], true, "{output:#}");
    assert_eq!(output["codex"]["available"], true);
    assert_eq!(output["marketplaces"][0]["registered"], true);
    assert_eq!(output["marketplaces"][0]["source_matches"], true);
    assert_eq!(output["marketplaces"][0]["plugins_ready"], true);
    assert_eq!(output["marketplaces"][0]["plugins"][0]["enabled"], true);
    assert!(output["next_steps"].as_array().unwrap().is_empty());
}

#[test]
fn agent_doctor_accepts_registered_marketplace_without_plugin_entries() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    let codex_home = temp.path().join("codex-home");
    fs::create_dir_all(&codex_home).unwrap();
    fs::write(
        codex_home.join("config.toml"),
        r#"[marketplaces.jig-skills]
source_type = "git"
source = "https://github.com/bpcakes/jig-skills.git"
"#,
    )
    .unwrap();
    let codex_path = temp.path().join("codex-stub.sh");
    write_codex_stub(
        &codex_path,
        "#!/bin/sh\nif [ \"$1 $2 $3 $4\" = \"plugin marketplace add --help\" ]; then exit 0; fi\nexit 2\n",
    );

    let _codex_bin = EnvVarGuard::set("JIG_CODEX_BIN", &codex_path);
    let _codex_home = EnvVarGuard::set("CODEX_HOME", &codex_home);
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let output = dispatch(&ctx, CommandKind::Agent(crate::cli::AgentCommand::Doctor)).unwrap();

    assert_eq!(output["ok"], true, "{output:#}");
    assert_eq!(output["readiness"]["ok_requires_plugins_enabled"], false);
    assert_eq!(output["marketplaces"][0]["registered"], true);
    assert_eq!(output["marketplaces"][0]["plugins_ready"], false);
    assert_eq!(output["marketplaces"][0]["plugins"][0]["enabled"], false);
}

#[test]
fn agent_doctor_reports_source_mismatch_for_registered_marketplace_id() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    let codex_home = temp.path().join("codex-home");
    fs::create_dir_all(&codex_home).unwrap();
    fs::write(
        codex_home.join("config.toml"),
        r#"[marketplaces.jig-skills]
source_type = "git"
source = "https://github.com/someone-else/jig-skills.git"

[plugins."jig-rust@jig-skills"]
enabled = true

[plugins."jig-swift@jig-skills"]
enabled = true

[plugins."jig-typescript@jig-skills"]
enabled = true

[plugins."jig-exec-plans@jig-skills"]
enabled = true
"#,
    )
    .unwrap();
    let codex_path = temp.path().join("codex-stub.sh");
    write_codex_stub(
        &codex_path,
        "#!/bin/sh\nif [ \"$1 $2 $3 $4\" = \"plugin marketplace add --help\" ]; then exit 0; fi\nexit 2\n",
    );

    let _codex_bin = EnvVarGuard::set("JIG_CODEX_BIN", &codex_path);
    let _codex_home = EnvVarGuard::set("CODEX_HOME", &codex_home);
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let output = dispatch(&ctx, CommandKind::Agent(crate::cli::AgentCommand::Doctor)).unwrap();

    assert_eq!(output["ok"], false, "{output:#}");
    assert_eq!(output["marketplaces"][0]["registered"], false);
    assert_eq!(output["marketplaces"][0]["source_matches"], false);
    assert_eq!(
        output["marketplaces"][0]["configured_source"],
        "https://github.com/someone-else/jig-skills.git"
    );
    assert_eq!(
        output["next_steps"][0],
        "Run `scripts/jig agent bootstrap` to register marketplace jig-skills (source: bpcakes/jig-skills)."
    );
}

#[test]
fn agent_doctor_reports_unsupported_codex_when_marketplace_required() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    let codex_home = temp.path().join("codex-home");
    fs::create_dir_all(&codex_home).unwrap();
    fs::write(
        codex_home.join("config.toml"),
        r#"[marketplaces.jig-skills]
source_type = "git"
source = "https://github.com/bpcakes/jig-skills.git"
"#,
    )
    .unwrap();
    let codex_path = temp.path().join("codex-stub.sh");
    write_codex_stub(&codex_path, "#!/bin/sh\nexit 2\n");

    let _codex_bin = EnvVarGuard::set("JIG_CODEX_BIN", &codex_path);
    let _codex_home = EnvVarGuard::set("CODEX_HOME", &codex_home);
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let output = dispatch(&ctx, CommandKind::Agent(crate::cli::AgentCommand::Doctor)).unwrap();

    assert_eq!(output["ok"], false, "{output:#}");
    assert_eq!(output["codex"]["required"], true);
    assert_eq!(output["codex"]["available"], false);
    assert_eq!(output["codex"]["probe_skipped"], false);
    assert_eq!(
        output["readiness"]["ok_requires_marketplaces_registered"],
        true
    );
    assert_eq!(output["marketplaces"][0]["registered"], true);
    assert!(
        output["next_steps"][0]
            .as_str()
            .unwrap()
            .contains("plugin marketplace add --help")
    );
    assert_eq!(output["next_steps"].as_array().unwrap().len(), 1);
}

#[test]
fn agent_doctor_bounds_and_redacts_codex_probe_output() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    let secret = "codex-probe-output-secret";
    let codex_path = temp.path().join("codex-stub.sh");
    write_codex_stub(
        &codex_path,
        &format!(
            "#!/bin/sh\ni=0\nwhile [ \"$i\" -lt 2000 ]; do printf '{}'; printf '{}' >&2; i=$((i + 1)); done\nexit 0\n",
            secret, secret
        ),
    );

    let _codex_bin = EnvVarGuard::set("JIG_CODEX_BIN", &codex_path);
    let _codex_home = EnvVarGuard::set("CODEX_HOME", temp.path().join("codex-home"));
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let output = dispatch(&ctx, CommandKind::Agent(crate::cli::AgentCommand::Doctor)).unwrap();
    let rendered = serde_json::to_string(&output).unwrap();

    assert_eq!(output["ok"], false, "{output:#}");
    assert!(output["codex"]["available"].is_null());
    assert!(
        output["codex"]["probe_error"]
            .as_str()
            .unwrap()
            .contains("capture limit")
    );
    assert!(
        !rendered.contains(secret),
        "probe output leaked into report"
    );
}

#[test]
fn agent_doctor_matches_relative_config_to_absolute_codex_source() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let repo_root = temp.path().join("repo");
    let skills_root = temp.path().join("jig-skills");
    fs::create_dir_all(&repo_root).unwrap();
    fs::create_dir_all(&skills_root).unwrap();
    write_fixture_repo(&repo_root);
    fs::write(
        repo_root.join(".jig.toml"),
        r#"_src_path = "/tmp/template"
_commit = "abc123"
repo_name = "demo"
default_branch = "main"
jig_version = "0.2.0-beta.1"

[[agent_tooling.codex.marketplaces]]
id = "local-skills"
source = "../jig-skills"

[[work.gates]]
id = "custom"
kind = "check"
tool = "jig.custom_check"
"#,
    )
    .unwrap();
    let codex_home = temp.path().join("codex-home");
    fs::create_dir_all(&codex_home).unwrap();
    fs::write(
        codex_home.join("config.toml"),
        format!(
            r#"[marketplaces.local-skills]
source_type = "path"
source = "{}"
"#,
            skills_root.display()
        ),
    )
    .unwrap();
    let codex_path = temp.path().join("codex-stub.sh");
    write_codex_stub(
        &codex_path,
        "#!/bin/sh\nif [ \"$1 $2 $3 $4\" = \"plugin marketplace add --help\" ]; then exit 0; fi\nexit 2\n",
    );

    let _codex_bin = EnvVarGuard::set("JIG_CODEX_BIN", &codex_path);
    let _codex_home = EnvVarGuard::set("CODEX_HOME", &codex_home);
    let ctx = RepoContext::load_from(&repo_root).unwrap();
    let output = dispatch(&ctx, CommandKind::Agent(crate::cli::AgentCommand::Doctor)).unwrap();

    assert_eq!(output["ok"], true, "{output:#}");
    assert_eq!(output["marketplaces"][0]["registered"], true);
    assert_eq!(output["marketplaces"][0]["source_matches"], true);
}

#[test]
fn agent_doctor_accepts_empty_marketplace_config_without_codex() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    fs::write(
        temp.path().join(".jig.toml"),
        r#"_src_path = "/tmp/template"
_commit = "abc123"
repo_name = "demo"
default_branch = "main"
jig_version = "0.2.0-beta.1"

[agent_tooling.codex]
marketplaces = []

[[work.gates]]
id = "custom"
kind = "check"
tool = "jig.custom_check"
"#,
    )
    .unwrap();
    let missing_codex = temp.path().join("missing-codex");

    let _codex_bin = EnvVarGuard::set("JIG_CODEX_BIN", &missing_codex);
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let output = dispatch(&ctx, CommandKind::Agent(crate::cli::AgentCommand::Doctor)).unwrap();

    assert_eq!(output["ok"], true, "{output:#}");
    assert_eq!(output["codex"]["probe_skipped"], true);
    assert_eq!(output["codex"]["available"], serde_json::Value::Null);
    assert_eq!(output["codex"]["config_read"], false);
    assert_eq!(
        output["readiness"]["ok_requires_marketplaces_registered"],
        false
    );
    assert!(output["marketplaces"].as_array().unwrap().is_empty());
    assert!(output["next_steps"].as_array().unwrap().is_empty());
}

#[test]
fn agent_bootstrap_invokes_codex_marketplace_add() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    let skills_root = temp.path().join("jig-skills");
    fs::create_dir_all(&skills_root).unwrap();
    let log_path = temp.path().join("codex.log");
    let codex_path = temp.path().join("codex-stub.sh");
    write_codex_stub(
        &codex_path,
        &format!(
            "#!/bin/sh\nprintf '%s\\n' \"$*\" >> \"{}\"\nexit 0\n",
            log_path.display()
        ),
    );

    let _codex_bin = EnvVarGuard::set("JIG_CODEX_BIN", &codex_path);
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let output = dispatch(
        &ctx,
        CommandKind::Agent(crate::cli::AgentCommand::Bootstrap(
            crate::cli::AgentBootstrapOpts {
                marketplace: Some("./jig-skills".into()),
            },
        )),
    )
    .unwrap();

    assert_eq!(output["ok"], true);
    assert_eq!(
        output["marketplace_source"],
        skills_root.canonicalize().unwrap().display().to_string()
    );
    let log = fs::read_to_string(log_path).unwrap();
    assert!(log.contains(&format!(
        "plugin marketplace add {}",
        skills_root.canonicalize().unwrap().display()
    )));
}

#[test]
fn agent_doctor_reports_marketplace_specific_bootstrap_commands() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    fs::write(
        temp.path().join(".jig.toml"),
        r#"_src_path = "/tmp/template"
_commit = "abc123"
repo_name = "demo"
default_branch = "main"
jig_version = "0.2.0-beta.1"

[[agent_tooling.codex.marketplaces]]
id = "first-skills"
source = "bpcakes/jig-skills"

[[agent_tooling.codex.marketplaces]]
id = "local-skills"
source = "./team's-skills"
"#,
    )
    .unwrap();
    let codex_path = temp.path().join("codex-stub.sh");
    write_codex_stub(
        &codex_path,
        "#!/bin/sh\nif [ \"$1 $2 $3 $4\" = \"plugin marketplace add --help\" ]; then exit 0; fi\nexit 2\n",
    );

    let _codex_bin = EnvVarGuard::set("JIG_CODEX_BIN", &codex_path);
    let _codex_home = EnvVarGuard::set("CODEX_HOME", temp.path().join("missing-codex-home"));
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let output = dispatch(&ctx, CommandKind::Agent(crate::cli::AgentCommand::Doctor)).unwrap();

    assert_eq!(output["ok"], false, "{output:#}");
    let steps = output["next_steps"].as_array().unwrap();
    assert_eq!(steps.len(), 2);
    assert_eq!(
        steps[0],
        "Run `scripts/jig agent bootstrap --marketplace 'bpcakes/jig-skills'` to register marketplace first-skills (source: bpcakes/jig-skills)."
    );
    assert_eq!(
        steps[1],
        "Run `scripts/jig agent bootstrap --marketplace './team'\\''s-skills'` to register marketplace local-skills (source: ./team's-skills)."
    );
}

#[test]
fn agent_bootstrap_then_doctor_passes_with_marketplace_registration() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    fs::create_dir_all(temp.path().join("bpcakes/jig-skills")).unwrap();
    let codex_home = temp.path().join("codex-home");
    let codex_path = temp.path().join("codex-stub.sh");
    write_codex_stub(
        &codex_path,
        "#!/bin/sh\nif [ \"$1 $2 $3 $4\" = \"plugin marketplace add --help\" ]; then exit 0; fi\nif [ \"$1 $2 $3\" = \"plugin marketplace add\" ]; then\n  mkdir -p \"$CODEX_HOME\"\n  cat > \"$CODEX_HOME/config.toml\" <<'EOF'\n[marketplaces.jig-skills]\nsource_type = \"git\"\nsource = \"https://github.com/bpcakes/jig-skills.git\"\nEOF\n  exit 0\nfi\nexit 2\n",
    );

    let _codex_bin = EnvVarGuard::set("JIG_CODEX_BIN", &codex_path);
    let _codex_home = EnvVarGuard::set("CODEX_HOME", &codex_home);
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let bootstrap_output = dispatch(
        &ctx,
        CommandKind::Agent(crate::cli::AgentCommand::Bootstrap(
            crate::cli::AgentBootstrapOpts { marketplace: None },
        )),
    )
    .unwrap();
    let doctor_output =
        dispatch(&ctx, CommandKind::Agent(crate::cli::AgentCommand::Doctor)).unwrap();

    assert_eq!(bootstrap_output["ok"], true);
    assert_eq!(bootstrap_output["marketplace_source"], "bpcakes/jig-skills");
    assert_eq!(doctor_output["ok"], true, "{doctor_output:#}");
    assert_eq!(
        doctor_output["readiness"]["ok_requires_plugins_enabled"],
        false
    );
    assert_eq!(doctor_output["marketplaces"][0]["registered"], true);
    assert_eq!(doctor_output["marketplaces"][0]["plugins_ready"], false);
    assert_eq!(
        doctor_output["marketplaces"][0]["plugins"][0]["enabled"],
        false
    );
}

#[test]
fn agent_bootstrap_uses_single_configured_marketplace_source() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    let skills_root = temp.path().join("jig-skills");
    fs::create_dir_all(&skills_root).unwrap();
    fs::write(
        temp.path().join(".jig.toml"),
        r#"_src_path = "/tmp/template"
_commit = "abc123"
repo_name = "demo"
default_branch = "main"
jig_version = "0.2.0-beta.1"

[[agent_tooling.codex.marketplaces]]
id = "local-skills"
source = "./jig-skills"
plugins = ["local-rust@local-skills"]

[[work.gates]]
id = "custom"
kind = "check"
tool = "jig.custom_check"
"#,
    )
    .unwrap();
    let log_path = temp.path().join("codex.log");
    let codex_path = temp.path().join("codex-stub.sh");
    write_codex_stub(
        &codex_path,
        &format!(
            "#!/bin/sh\nprintf '%s\\n' \"$*\" >> \"{}\"\nexit 0\n",
            log_path.display()
        ),
    );

    let _codex_bin = EnvVarGuard::set("JIG_CODEX_BIN", &codex_path);
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let output = dispatch(
        &ctx,
        CommandKind::Agent(crate::cli::AgentCommand::Bootstrap(
            crate::cli::AgentBootstrapOpts { marketplace: None },
        )),
    )
    .unwrap();

    assert_eq!(output["ok"], true);
    assert_eq!(
        output["marketplace_source"],
        skills_root.canonicalize().unwrap().display().to_string()
    );
    let log = fs::read_to_string(log_path).unwrap();
    assert!(log.contains(&format!(
        "plugin marketplace add {}",
        skills_root.canonicalize().unwrap().display()
    )));
}

#[test]
fn agent_bootstrap_uses_marketplace_env_override() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    let skills_root = temp.path().join("env-skills");
    fs::create_dir_all(&skills_root).unwrap();
    let log_path = temp.path().join("codex.log");
    let codex_path = temp.path().join("codex-stub.sh");
    write_codex_stub(
        &codex_path,
        &format!(
            "#!/bin/sh\nprintf '%s\\n' \"$*\" >> \"{}\"\nexit 0\n",
            log_path.display()
        ),
    );

    let _codex_bin = EnvVarGuard::set("JIG_CODEX_BIN", &codex_path);
    let _marketplace = EnvVarGuard::set("JIG_SKILLS_MARKETPLACE", "./env-skills");
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let output = dispatch(
        &ctx,
        CommandKind::Agent(crate::cli::AgentCommand::Bootstrap(
            crate::cli::AgentBootstrapOpts { marketplace: None },
        )),
    )
    .unwrap();

    assert_eq!(output["ok"], true);
    assert_eq!(
        output["marketplace_source"],
        skills_root.canonicalize().unwrap().display().to_string()
    );
    let log = fs::read_to_string(log_path).unwrap();
    assert!(log.contains(&format!(
        "plugin marketplace add {}",
        skills_root.canonicalize().unwrap().display()
    )));
}

#[test]
fn agent_bootstrap_rejects_missing_relative_marketplace_path() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());

    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let error = dispatch(
        &ctx,
        CommandKind::Agent(crate::cli::AgentCommand::Bootstrap(
            crate::cli::AgentBootstrapOpts {
                marketplace: Some("./missing-skills".into()),
            },
        )),
    )
    .unwrap_err()
    .to_string();

    assert!(error.contains("Configured Codex marketplace path ./missing-skills does not exist"));
    assert!(error.contains(&temp.path().display().to_string()));
}

#[test]
fn agent_bootstrap_rejects_malformed_marketplace_source() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());

    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let error = dispatch(
        &ctx,
        CommandKind::Agent(crate::cli::AgentCommand::Bootstrap(
            crate::cli::AgentBootstrapOpts {
                marketplace: Some("not a marketplace".into()),
            },
        )),
    )
    .unwrap_err()
    .to_string();

    assert!(error.contains("must be a local path"));
}

#[test]
fn agent_bootstrap_rejects_remote_marketplace_source_with_whitespace() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());

    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let error = dispatch(
        &ctx,
        CommandKind::Agent(crate::cli::AgentCommand::Bootstrap(
            crate::cli::AgentBootstrapOpts {
                marketplace: Some("https://github.com/bpcakes/jig skills".into()),
            },
        )),
    )
    .unwrap_err()
    .to_string();

    assert!(error.contains("must be a local path"));
}

#[test]
fn agent_bootstrap_rejects_remote_marketplace_source_with_unicode_whitespace() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());

    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let error = dispatch(
        &ctx,
        CommandKind::Agent(crate::cli::AgentCommand::Bootstrap(
            crate::cli::AgentBootstrapOpts {
                marketplace: Some("https://github.com/bpcakes/jig\u{00a0}skills".into()),
            },
        )),
    )
    .unwrap_err()
    .to_string();

    assert!(error.contains("must be a local path"));
}

#[test]
fn agent_bootstrap_rejects_incomplete_remote_marketplace_sources() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());

    let ctx = RepoContext::load_from(temp.path()).unwrap();
    for marketplace in [
        "https://github.com",
        "https://github.com/",
        "git@github.com:",
        "git@:bpcakes/jig-skills",
    ] {
        let error = dispatch(
            &ctx,
            CommandKind::Agent(crate::cli::AgentCommand::Bootstrap(
                crate::cli::AgentBootstrapOpts {
                    marketplace: Some(marketplace.into()),
                },
            )),
        )
        .unwrap_err()
        .to_string();

        assert!(error.contains("must be a local path"));
    }
}

#[test]
fn agent_bootstrap_rejects_ambiguous_configured_marketplaces() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    fs::write(
        temp.path().join(".jig.toml"),
        r#"_src_path = "/tmp/template"
_commit = "abc123"
repo_name = "demo"
default_branch = "main"
jig_version = "0.2.0-beta.1"

[[agent_tooling.codex.marketplaces]]
id = "first-skills"
source = "../first-skills"

[[agent_tooling.codex.marketplaces]]
id = "second-skills"
source = "../second-skills"

[[work.gates]]
id = "custom"
kind = "check"
tool = "jig.custom_check"
"#,
    )
    .unwrap();

    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let error = dispatch(
        &ctx,
        CommandKind::Agent(crate::cli::AgentCommand::Bootstrap(
            crate::cli::AgentBootstrapOpts { marketplace: None },
        )),
    )
    .unwrap_err()
    .to_string();

    assert!(error.contains("Multiple Codex marketplaces are configured"));
    assert!(error.contains("first-skills=../first-skills"));
    assert!(error.contains("pass --marketplace <source>"));
}

#[test]
fn agent_bootstrap_fails_when_codex_marketplace_add_fails() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    let codex_path = temp.path().join("codex-stub.sh");
    write_codex_stub(
        &codex_path,
        "#!/bin/sh\nprintf 'bad source\\n' >&2\nexit 9\n",
    );

    let _codex_bin = EnvVarGuard::set("JIG_CODEX_BIN", &codex_path);
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let error = dispatch(
        &ctx,
        CommandKind::Agent(crate::cli::AgentCommand::Bootstrap(
            crate::cli::AgentBootstrapOpts { marketplace: None },
        )),
    )
    .unwrap_err()
    .to_string();

    assert!(error.contains("plugin marketplace add bpcakes/jig-skills failed"));
    assert!(error.contains("exit status 9"));
    assert!(error.contains("bad source"));
}

#[test]
fn agent_bootstrap_fails_when_codex_cannot_be_started() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    let missing_codex = temp.path().join("missing-codex");

    let _codex_bin = EnvVarGuard::set("JIG_CODEX_BIN", &missing_codex);
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let error = dispatch(
        &ctx,
        CommandKind::Agent(crate::cli::AgentCommand::Bootstrap(
            crate::cli::AgentBootstrapOpts { marketplace: None },
        )),
    )
    .unwrap_err()
    .to_string();

    assert!(error.contains("Failed to run"));
    assert!(error.contains("plugin marketplace add bpcakes/jig-skills"));
}
