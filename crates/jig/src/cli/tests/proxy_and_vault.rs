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
    let tui = Cli::try_parse_from(["jig", "vault", "tui", "--home", "/tmp/jig-vault"]).unwrap();
    match tui.command {
        CommandKind::Vault(VaultCommand::Tui(opts)) => {
            assert_eq!(opts.vault.home, Some(PathBuf::from("/tmp/jig-vault")));
        }
        other => panic!("expected vault tui command, got {other:?}"),
    }

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
