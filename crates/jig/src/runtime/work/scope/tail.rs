fn canonical_cargo_skip(command: &str) -> bool {
    let Some(label) = command
        .strip_prefix("printf '%s\\n' 'No Cargo.toml found; skipping cargo ")
        .and_then(|command| command.strip_suffix(".'"))
    else {
        return false;
    };
    matches!(
        label,
        "bootstrap" | "fmt" | "clippy" | "test" | "test-locked"
    )
}

fn canonical_sqlx_command(ctx: &RepoContext, command: &str) -> bool {
    let Some(tokens) = simple_shell_words(command) else {
        return false;
    };
    let metadata_assignment = format!(
        "SQLX_OFFLINE_DIR={}",
        crate::shell::quote(ctx.rust_sqlx_metadata_dir())
    );
    let mut index = 0;
    let mut seen_cargo = false;
    let mut seen_offline = false;
    let mut seen_metadata = false;
    while let Some(token) = tokens
        .get(index)
        .filter(|token| is_environment_assignment(token))
    {
        match *token {
            "CARGO=cargo" if !seen_cargo => seen_cargo = true,
            "SQLX_OFFLINE=false" if !seen_offline => seen_offline = true,
            token if token == metadata_assignment && !seen_metadata => seen_metadata = true,
            _ => return false,
        }
        index += 1;
    }
    let command_len = if tokens[index..].starts_with(&["sqlx", "prepare"]) {
        2
    } else if tokens[index..].starts_with(&["cargo", "sqlx", "prepare"]) {
        3
    } else {
        return false;
    };
    tokens[index + command_len..].iter().all(|token| {
        matches!(
            *token,
            "--check" | "--workspace" | "--" | "--all-targets" | "--all-features"
        )
    })
}

fn canonical_schema_dump_command(command: &str) -> bool {
    let Some(tokens) = simple_shell_words(command) else {
        return false;
    };
    if !tokens.starts_with(&["cargo", "run"]) {
        return false;
    }
    let mut index = 2;
    while index < tokens.len() {
        match tokens[index] {
            "--locked" | "--release" | "--all-features" | "--no-default-features" => {
                index += 1;
            }
            "--package" | "-p" | "--bin" | "--features" => {
                let Some(value) = tokens.get(index + 1) else {
                    return false;
                };
                if value.is_empty()
                    || !value.bytes().all(|byte| {
                        byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.' | b',')
                    })
                {
                    return false;
                }
                index += 2;
            }
            _ => return false,
        }
    }
    true
}

fn canonical_app_check(ctx: &RepoContext, tool: &str, command: &str) -> bool {
    let Some(tokens) = simple_shell_words(command) else {
        return false;
    };
    let ["scripts/check-webapps.sh", "app-check", app_dir, operation] = tokens.as_slice() else {
        return false;
    };
    let Some(app) = ctx.frontend_apps().iter().find(|app| app.dir == *app_dir) else {
        return false;
    };
    matches!(*operation, "lint" | "typecheck" | "build" | "coverage")
        && tool
            == format!(
                "jig.typescript_{}_{}",
                app.name.to_ascii_lowercase().replace('-', "_"),
                operation
            )
}

fn simple_shell_words(command: &str) -> Option<Vec<&str>> {
    if command.trim().is_empty()
        || command
            .chars()
            .any(|ch| matches!(ch, ';' | '|' | '&' | '<' | '>' | '`' | '\n' | '\r'))
        || command.contains("$(")
    {
        return None;
    }
    Some(command.split_ascii_whitespace().collect())
}

fn is_environment_assignment(token: &str) -> bool {
    let Some((name, _)) = token.split_once('=') else {
        return false;
    };
    !name.is_empty()
        && name.bytes().enumerate().all(|(index, byte)| {
            byte == b'_' || byte.is_ascii_alphabetic() || (index > 0 && byte.is_ascii_digit())
        })
}

fn hash_field(digest: &mut Sha256, value: &[u8]) {
    digest.update((value.len() as u64).to_be_bytes());
    digest.update(value);
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tempfile::tempdir;

    #[test]
    fn command_scope_safety_accepts_only_proven_command_shapes() {
        let temp = tempdir().unwrap();
        crate::test_env::TestRepoBuilder::new(temp.path())
            .contract_version(crate::context::CURRENT_CONTRACT_VERSION)
            .config(
                r#"
rust_test_command = "cargo test --workspace"
rust_sqlx_metadata_dir = ".sqlx"

[[frontend_apps]]
name = "web"
dir = "apps/web"
coverage_threshold = 80

[[frontend_apps]]
name = "admin"
dir = "apps/admin"
coverage_threshold = 80

[commands]
web_lint_command = "scripts/check-webapps.sh app-check apps/web lint"
"#,
            )
            .required_commands(["rust_test_command", "web_lint_command"])
            .tool(json!({
                "name": "jig.test",
                "kind": "command",
                "description": "Tests.",
                "command": "rust_test_command"
            }))
            .tool(json!({
                "name": "jig.typescript_web_lint",
                "kind": "command",
                "description": "Web lint.",
                "command": "web_lint_command"
            }))
            .write();
        let ctx = RepoContext::load_from(temp.path()).unwrap();
        let rust = WorkCheckGate {
            id: "rust-tests".into(),
            tool: "jig.test".into(),
            required: true,
            paths: Some(vec!["crates/**".into()]),
            paths_ignore: Vec::new(),
            reuse: false,
        };
        let web = WorkCheckGate {
            id: "typescript-web-lint".into(),
            tool: "jig.typescript_web_lint".into(),
            required: true,
            paths: Some(vec!["apps/web/**".into()]),
            paths_ignore: Vec::new(),
            reuse: false,
        };

        assert!(gate_command_scope_is_safe(&ctx, &rust));
        assert!(gate_command_scope_is_safe(&ctx, &web));
        assert!(canonical_cargo_command(
            "if [ -f Cargo.toml ]; then cargo test --workspace; else printf '%s\\n' 'No Cargo.toml found; skipping cargo test.'; fi",
            &["test"]
        ));
        assert!(canonical_sqlx_command(
            &ctx,
            "SQLX_OFFLINE=false SQLX_OFFLINE_DIR=.sqlx cargo sqlx prepare --check"
        ));
        assert!(!canonical_cargo_command("scripts/test.sh", &["test"]));
        assert!(!canonical_cargo_command(
            "cargo test && scripts/extra.sh",
            &["test"]
        ));
        assert!(!canonical_cargo_command(
            "if [ -f Cargo.toml ]; then cargo test; else scripts/fallback.sh; fi",
            &["test"]
        ));
        assert!(!canonical_cargo_command(
            "RUSTC_WRAPPER=tools/wrapper cargo test --workspace",
            &["test"]
        ));
        assert!(!canonical_cargo_command(
            "cargo test --manifest-path tools/fixture/Cargo.toml",
            &["test"]
        ));
        assert!(!canonical_sqlx_command(
            &ctx,
            "CARGO=tools/cargo-wrapper sqlx prepare --check"
        ));
        assert!(!canonical_app_check(
            &ctx,
            "jig.typescript_web_lint",
            "scripts/check-webapps.sh lint"
        ));
        assert!(!canonical_app_check(
            &ctx,
            "jig.typescript_admin_lint",
            "scripts/check-webapps.sh app-check apps/web lint"
        ));
        assert!(canonical_schema_dump_command(
            "cargo run --locked --package schema-tool --bin dump-schema"
        ));
        assert!(!canonical_schema_dump_command("scripts/dump-schema.sh"));
        assert!(!canonical_schema_dump_command(
            "cargo run --manifest-path tools/schema/Cargo.toml"
        ));
    }

    #[test]
    fn native_gate_signatures_include_the_build_identity() {
        let temp = tempdir().unwrap();
        crate::test_env::TestRepoBuilder::new(temp.path())
            .contract_version(crate::context::CURRENT_CONTRACT_VERSION)
            .config("")
            .required_commands(Vec::<String>::new())
            .tool(json!({
                "name": "jig.contract_check",
                "kind": "native",
                "description": "Validate Jig wiring."
            }))
            .write();
        let ctx = RepoContext::load_from(temp.path()).unwrap();
        let gate = WorkCheckGate {
            id: "jig-contract".into(),
            tool: crate::tool_defs::tool::CONTRACT_CHECK.into(),
            required: true,
            paths: None,
            paths_ignore: Vec::new(),
            reuse: false,
        };

        let first = gate_signature_with_native_identity(&ctx, &gate, "build-a").unwrap();
        let second = gate_signature_with_native_identity(&ctx, &gate, "build-b").unwrap();

        assert_ne!(first, second);
    }

    #[test]
    fn gate_signatures_frame_path_lists_unambiguously() {
        let temp = tempdir().unwrap();
        crate::test_env::TestRepoBuilder::new(temp.path())
            .contract_version(crate::context::CURRENT_CONTRACT_VERSION)
            .config("")
            .required_commands(Vec::<String>::new())
            .tool(json!({
                "name": "jig.contract_check",
                "kind": "native",
                "description": "Validate Jig wiring."
            }))
            .write();
        let ctx = RepoContext::load_from(temp.path()).unwrap();
        let left = WorkCheckGate {
            id: "jig-contract".into(),
            tool: crate::tool_defs::tool::CONTRACT_CHECK.into(),
            required: true,
            paths: Some(vec!["a".into(), "paths-ignore".into()]),
            paths_ignore: vec!["b".into()],
            reuse: false,
        };
        let right = WorkCheckGate {
            paths: Some(vec!["a".into()]),
            paths_ignore: vec!["paths-ignore".into(), "b".into()],
            ..left.clone()
        };

        let left = gate_signature_with_native_identity(&ctx, &left, "build").unwrap();
        let right = gate_signature_with_native_identity(&ctx, &right, "build").unwrap();

        assert_ne!(left, right);
    }
}
