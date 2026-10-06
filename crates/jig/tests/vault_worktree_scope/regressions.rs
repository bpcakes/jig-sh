use super::*;

fn assert_scope_refusal(worktree: &Path, vault_base: &Path, expected: &str) {
    std::fs::write(worktree.join("refs.env"), "TOKEN=jig://Example/TOKEN\n").unwrap();
    std::fs::write(worktree.join("input.txt"), "{{ jig://Example/TOKEN }}").unwrap();
    for args in [
        vec!["vault", "init"],
        vec!["vault", "field", "list"],
        vec!["vault", "field", "set", REFERENCE, "--value-stdin"],
        vec!["vault", "secret", "list"],
        vec!["vault", "read", REFERENCE],
        vec!["vault", "inject", "--in", "input.txt"],
        vec!["vault", "exec", "--env-file", "refs.env", "--", "true"],
        vec![
            "vault",
            "run",
            "--env",
            "TOKEN=jig://Example/TOKEN",
            "--",
            "true",
        ],
        vec!["vault", "audit", "verify"],
        vec!["vault", "migrate", "--to", "2"],
    ] {
        let output = jig(worktree, vault_base, &args)
            .stdin(std::process::Stdio::null())
            .output()
            .unwrap();
        assert!(!output.status.success(), "{args:?}");
        let error = String::from_utf8_lossy(&output.stderr);
        assert!(error.contains(expected), "{args:?}: {error}");
        assert!(!error.contains("cannot prompt"), "{args:?}: {error}");
        assert_value_free("scope refusal", &output);
    }
}

#[test]
fn worktree_local_vault_keeps_working_through_the_cli() {
    let temp = tempfile::tempdir().unwrap();
    let main = temp.path().join("ExampleProject");
    let worktree = temp.path().join("worktree");
    let vault_base = temp.path().join("ExampleVault");
    write_main_checkout(&main);
    git(
        &main,
        &[
            "worktree",
            "add",
            "-q",
            "--detach",
            worktree.to_str().unwrap(),
        ],
    );

    // Query the old checkout namespace without reimplementing its digest. A
    // `.git` directory ceiling keeps that probe's walk-up inside the fixture
    // even when the temporary directory itself lies in a linked worktree.
    std::fs::create_dir(temp.path().join(".git")).unwrap();
    std::fs::rename(worktree.join(".git"), worktree.join("git-pointer")).unwrap();
    let local = json(
        "checkout-local scope",
        &jig(&worktree, &vault_base, &["--json", "vault", "status"])
            .output()
            .unwrap(),
    );
    std::fs::rename(worktree.join("git-pointer"), worktree.join(".git")).unwrap();
    let local_home = PathBuf::from(local["vault_home"].as_str().unwrap());
    initialize_vault(local_home.clone());

    let status = json(
        "worktree-local status",
        &jig(&worktree, &vault_base, &["--json", "vault", "status"])
            .output()
            .unwrap(),
    );
    assert_eq!(
        PathBuf::from(status["vault_home"].as_str().unwrap()),
        local_home
    );
    assert_eq!(status["vault_worktree_local"], true);
    assert_eq!(status["vault_main_checkout_root"], serde_json::Value::Null);
    let guidance = status["vault_worktree_local_guidance"].as_str().unwrap();
    assert!(
        guidance.contains("keeps its own repo-scoped vault"),
        "{guidance}"
    );

    let listed = jig(
        &worktree,
        &vault_base,
        &["--json", "vault", "field", "list"],
    )
    .env("JIG_VAULT_PASSPHRASE", PASSPHRASE)
    .output()
    .unwrap();
    assert!(
        json("worktree-local field list", &listed)
            .to_string()
            .contains("TOKEN")
    );
    assert_value_free("worktree-local field list", &listed);
    let info = json(
        "worktree-local info",
        &jig(&worktree, &vault_base, &["--json", "info"])
            .output()
            .unwrap(),
    );
    assert_eq!(info["capabilities"]["vault_worktree_local"], true);
    assert_eq!(
        info["capabilities"]["vault_main_checkout_root"],
        serde_json::Value::Null
    );
    // Using the kept vault never creates the shared namespace. Test builds
    // keep the rollback witness beside the vault home, which is not a
    // namespace.
    assert_eq!(
        std::fs::read_dir(vault_base.join("scopes"))
            .unwrap()
            .filter(|entry| entry.as_ref().unwrap().file_name() != ".jig-vault-witness")
            .count(),
        1
    );
}

#[test]
fn unverified_worktree_refusal_precedes_capture_but_explicit_scopes_bypass_it() {
    let temp = tempfile::tempdir().unwrap();
    let main = temp.path().join("ExampleProject");
    let worktree = temp.path().join("worktree");
    let vault_base = temp.path().join("ExampleVault");
    write_main_checkout(&main);
    git(
        &main,
        &[
            "worktree",
            "add",
            "-q",
            "--detach",
            worktree.to_str().unwrap(),
        ],
    );
    let pointer = std::fs::read_to_string(worktree.join(".git")).unwrap();
    let admin = PathBuf::from(pointer.trim().strip_prefix("gitdir: ").unwrap());
    std::fs::write(admin.join("gitdir"), main.join(".git").to_str().unwrap()).unwrap();

    assert_scope_refusal(
        &worktree,
        &vault_base,
        "could not verify it as a linked worktree",
    );
    assert!(
        !vault_base.exists(),
        "scope preflight created vault storage"
    );

    let config = worktree.join(".jig.toml");
    std::fs::write(
        &config,
        std::fs::read_to_string(&config)
            .unwrap()
            .replace("allow_global = false", "allow_global = true"),
    )
    .unwrap();
    for options in [vec!["--home", "explicit-vault"], vec!["--global"]] {
        let output = jig(&worktree, &vault_base, &["vault", "field", "list"])
            .args(&options)
            .output()
            .unwrap();
        let error = String::from_utf8_lossy(&output.stderr);
        assert!(!output.status.success());
        assert!(error.contains("cannot prompt"), "{options:?}: {error}");
        assert!(!error.contains("could not verify"), "{options:?}: {error}");
    }
    assert!(!vault_base.exists());
    assert!(!worktree.join("explicit-vault").exists());
}

#[test]
fn direct_vault_task_failure_redacts_output_without_recording_it() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("ExampleProject");
    let vault_base = temp.path().join("ExampleVault");
    write_main_checkout(&repo);
    let status = json(
        "repo scope",
        &jig(&repo, &vault_base, &["--json", "vault", "status"])
            .output()
            .unwrap(),
    );
    let home = PathBuf::from(status["vault_home"].as_str().unwrap());
    initialize_vault(home.clone());
    std::fs::write(repo.join("refs.env"), "TOKEN=jig://Example/TOKEN\n").unwrap();
    let history = repo.join(".agent/state/runs.jsonl");
    std::fs::create_dir_all(history.parent().unwrap()).unwrap();
    let before = b"{\"fixture\":\"existing-run-history\"}\n";
    std::fs::write(&history, before).unwrap();

    let output = jig(
        &repo,
        &vault_base,
        &[
            "vault",
            "exec",
            "--env-file",
            "refs.env",
            "--",
            "sh",
            "-c",
            "printf '%s' \"$TOKEN\"; printf '%s' \"$TOKEN\" >&2; exit 7",
        ],
    )
    .env("JIG_VAULT_PASSPHRASE", PASSPHRASE)
    .output()
    .unwrap();

    assert_eq!(output.status.code(), Some(7));
    assert_eq!(output.stdout, b"[REDACTED]");
    assert_eq!(output.stderr, b"[REDACTED]");
    assert_value_free("direct task failure", &output);
    assert_eq!(std::fs::read(&history).unwrap(), before);
    assert!(
        !std::fs::read_to_string(home.join("audit.jsonl"))
            .unwrap()
            .contains(FIELD_VALUE)
    );
}
