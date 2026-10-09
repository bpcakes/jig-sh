use super::*;

#[test]
fn retained_generation_budget_fails_before_a_low_soft_handle_limit() {
    let planned = (0..12)
        .map(|index| PathBuf::from(format!("nested/{index}/generated")))
        .collect::<BTreeSet<_>>();
    let repeated_generation_count = 2;
    let required = retained_generation_handle_requirement(&planned, repeated_generation_count);

    let error = validate_retained_generation_budget(
        &planned,
        repeated_generation_count,
        Some(required + 9),
        10,
    )
    .unwrap_err()
    .to_string();
    assert!(error.contains("soft handle limit"), "{error}");
    validate_retained_generation_budget(
        &planned,
        repeated_generation_count,
        Some(required + 10),
        10,
    )
    .unwrap();
}

#[test]
fn retained_generation_budget_only_reserves_preimages_that_were_snapshotted() {
    let planned = (0..12)
        .map(|index| PathBuf::from(format!("nested/{index}/generated")))
        .collect::<BTreeSet<_>>();
    let repeated_generation_count = 2;
    let pessimistic = retained_generation_handle_requirement(&planned, repeated_generation_count);
    let all_missing = retained_generation_handle_requirement_with_preimages(
        &planned,
        repeated_generation_count,
        0,
    );

    assert_eq!(pessimistic - all_missing, planned.len());
    validate_retained_generation_budget_with_preimages(
        &planned,
        repeated_generation_count,
        0,
        Some(all_missing + 10),
        10,
    )
    .unwrap();
}

#[test]
fn retained_generation_budget_caps_planned_and_repeated_generations_together() {
    let planned = (0..MAX_EXISTING_INIT_RETAINED_GENERATIONS)
        .map(|index| PathBuf::from(format!("generated-{index}")))
        .collect::<BTreeSet<_>>();

    validate_retained_generation_budget(&planned, 0, None, 0).unwrap();
    let error = validate_retained_generation_budget(&planned, 1, None, 0)
        .unwrap_err()
        .to_string();
    assert!(
        error.contains(&format!(
            "plans {} generated file generations",
            MAX_EXISTING_INIT_RETAINED_GENERATIONS + 1
        )),
        "{error}"
    );
}

#[test]
fn retained_generation_model_matches_preimages_first_outputs_and_explicit_repeats() {
    fn snapshot_handle_count(snapshot: &InitPathSnapshot) -> usize {
        usize::from(!matches!(snapshot, InitPathSnapshot::Missing))
    }

    let temp = tempdir().unwrap();
    let root = temp.path().join("repo");
    fs::create_dir_all(root.join("nested")).unwrap();
    let first = Path::new("nested/first");
    let second = Path::new("nested/second");
    fs::write(root.join(first), "first preimage\n").unwrap();
    fs::write(root.join(second), "second preimage\n").unwrap();

    let mut transaction = InitMutationTransaction::create(&root).unwrap();
    publish_existing_transaction_file(&mut transaction, first, b"first Jig generation\n");
    publish_existing_transaction_file(&mut transaction, second, b"second Jig generation\n");
    publish_existing_transaction_file(&mut transaction, first, b"repeated Jig generation\n");

    let retained_file_generations = transaction
        .files
        .values()
        .map(|mutation| {
            snapshot_handle_count(&mutation.before)
                + mutation
                    .expected_jig_states
                    .iter()
                    .map(snapshot_handle_count)
                    .sum::<usize>()
        })
        .sum::<usize>();
    assert_eq!(retained_file_generations, 2 * 2 + 1);

    let planned = BTreeSet::from([first.to_path_buf(), second.to_path_buf()]);
    assert_eq!(
        retained_generation_handle_requirement(&planned, 1),
        retained_file_generations
            + 1 // one retained directory prefix: nested
            + 1 // one private write-staging directory for that parent
            + RETAINED_GENERATION_HANDLE_HEADROOM
    );

    transaction.rollback().unwrap();
    assert_eq!(
        fs::read_to_string(root.join(first)).unwrap(),
        "first preimage\n"
    );
    assert_eq!(
        fs::read_to_string(root.join(second)).unwrap(),
        "second preimage\n"
    );
}

#[cfg(unix)]
const EXISTING_INIT_SOFT_HANDLE_LIMIT_HELPER_ENV: &str =
    "JIG_TEST_EXISTING_INIT_SOFT_HANDLE_LIMIT_HELPER";

#[cfg(unix)]
const EXISTING_INIT_SOFT_HANDLE_LIMIT_HELPER_TEST: &str = "tests::basic::init_safety::generation_budget::existing_empty_default_init_succeeds_with_256_soft_handle_limit_helper";

#[cfg(unix)]
#[test]
fn existing_empty_default_init_succeeds_with_256_soft_handle_limit() {
    let _guard = lock_env();
    let output = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            EXISTING_INIT_SOFT_HANDLE_LIMIT_HELPER_TEST,
            "--nocapture",
        ])
        .env(EXISTING_INIT_SOFT_HANDLE_LIMIT_HELPER_ENV, "1")
        .env_remove(jig_git::GIT_BIN_ENV)
        .env_remove(path::INVOCATION_CWD_ENV)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "soft-limit init helper failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[cfg(unix)]
#[test]
fn existing_empty_default_init_succeeds_with_256_soft_handle_limit_helper() {
    if std::env::var_os(EXISTING_INIT_SOFT_HANDLE_LIMIT_HELPER_ENV).is_none() {
        return;
    }

    let mut limit = libc::rlimit {
        rlim_cur: 0,
        rlim_max: 0,
    };
    // SAFETY: the isolated helper owns this process and `limit` is writable.
    assert_eq!(
        unsafe { libc::getrlimit(libc::RLIMIT_NOFILE, &mut limit) },
        0,
        "failed to read helper descriptor limit: {}",
        std::io::Error::last_os_error()
    );
    let requested: libc::rlim_t = 256;
    assert!(
        limit.rlim_max == libc::RLIM_INFINITY || limit.rlim_max >= requested,
        "helper hard descriptor limit {} is below {requested}",
        limit.rlim_max
    );
    limit.rlim_cur = requested;
    // SAFETY: this limit change is confined to the isolated helper subprocess.
    assert_eq!(
        unsafe { libc::setrlimit(libc::RLIMIT_NOFILE, &limit) },
        0,
        "failed to set helper descriptor limit: {}",
        std::io::Error::last_os_error()
    );
    assert_eq!(process_soft_handle_limit(), Some(256));

    let temp = tempdir().unwrap();
    let destination = temp.path().join("existing-empty");
    fs::create_dir(&destination).unwrap();
    let report = with_test_build_template_pin_policy(BuildTemplatePinPolicy::Unreleased, || {
        run_init(rollback_test_init_opts(destination.clone(), false))
    })
    .unwrap();

    assert_eq!(report["scaffold"]["preset"], "rust-react");
    assert!(destination.join(".jig.toml").is_file());
    assert!(
        destination
            .join("apps/rollback-demo-api/Cargo.toml")
            .is_file()
    );
    assert!(destination.join("apps/web/e2e/app.spec.ts").is_file());
}
