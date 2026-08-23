use super::*;

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn brokered_run_injects_and_redacts_env_secret() {
    let output = run_brokered(ResolvedBrokeredRun {
        command: vec![
            "sh".into(),
            "-c".into(),
            "printf '%s' \"$TOKEN\"; printf '%s' \"$TOKEN\" >&2".into(),
        ],
        env: vec![ResolvedBrokeredEnv {
            var: EnvVarName::parse("TOKEN").unwrap(),
            secret_name: SecretName::parse("api_token").unwrap(),
            value: SecretBytes::new(b"secret-value".to_vec()),
        }],
        files: Vec::new(),
    })
    .unwrap();
    assert_eq!(output.exit_status, 0);
    assert_eq!(output.exit_signal, None);
    assert_eq!(output.stdout, "[REDACTED]");
    assert_eq!(output.stderr, "[REDACTED]");
}

#[test]
fn brokered_run_rejects_non_utf8_env_secret() {
    let error = run_brokered(ResolvedBrokeredRun {
        command: vec!["true".into()],
        env: vec![ResolvedBrokeredEnv {
            var: EnvVarName::parse("TOKEN").unwrap(),
            secret_name: SecretName::parse("binary_token").unwrap(),
            value: SecretBytes::new(vec![0xff, 0xfe, 0xfd, 0xfc]),
        }],
        files: Vec::new(),
    })
    .unwrap_err()
    .to_string();

    assert!(error.contains("not valid UTF-8"));
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn brokered_run_rejects_oversized_stdout() {
    let error = run_brokered(ResolvedBrokeredRun {
        command: vec![
            "sh".into(),
            "-c".into(),
            format!("head -c {} /dev/zero", MAX_CAPTURED_STREAM_BYTES + 1),
        ],
        env: Vec::new(),
        files: Vec::new(),
    })
    .unwrap_err()
    .to_string();

    assert!(error.contains("capture limit"), "unexpected error: {error}");
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn brokered_run_accepts_exact_capture_limit() {
    let output = run_brokered(ResolvedBrokeredRun {
        command: vec![
            "sh".into(),
            "-c".into(),
            format!("head -c {MAX_CAPTURED_STREAM_BYTES} /dev/zero"),
        ],
        env: Vec::new(),
        files: Vec::new(),
    })
    .unwrap();

    assert_eq!(output.exit_status, 0);
    assert_eq!(output.stdout.len(), MAX_CAPTURED_STREAM_BYTES);
    assert!(output.stderr.is_empty());
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn brokered_run_terminates_other_stream_after_stdout_overflow() {
    let temp = tempfile::tempdir().unwrap();
    let marker = temp.path().join("overflow-descendant-ran");
    let ready = temp.path().join("overflow-descendant-ready");
    let release = temp.path().join("overflow-descendant-release");
    let started = Instant::now();
    let error = run_brokered_with_timeout(
            ResolvedBrokeredRun {
                command: vec![
                    "sh".into(),
                    "-c".into(),
                    format!(
                        "(printf ready > \"$JIG_VAULT_TEST_READY\"; while [ ! -e \"$JIG_VAULT_TEST_RELEASE\" ]; do sleep 0.01; done; printf leaked > \"$JIG_VAULT_TEST_MARKER\") >&2 & while [ ! -e \"$JIG_VAULT_TEST_READY\" ]; do sleep 0.01; done; head -c {} /dev/zero",
                        MAX_CAPTURED_STREAM_BYTES + 1
                    ),
                ],
                env: vec![
                    test_env_mapping(
                        "JIG_VAULT_TEST_MARKER",
                        "overflow_marker",
                        marker.as_os_str().as_encoded_bytes(),
                    ),
                    test_env_mapping(
                        "JIG_VAULT_TEST_READY",
                        "overflow_ready",
                        ready.as_os_str().as_encoded_bytes(),
                    ),
                    test_env_mapping(
                        "JIG_VAULT_TEST_RELEASE",
                        "overflow_release",
                        release.as_os_str().as_encoded_bytes(),
                    ),
                ],
                files: Vec::new(),
            },
            Duration::from_secs(2),
        )
        .unwrap_err()
        .to_string();

    assert!(error.contains("capture limit"), "unexpected error: {error}");
    assert!(started.elapsed() < Duration::from_secs(2));
    assert!(ready.exists());
    fs::write(release, b"release").unwrap();
    assert_path_stays_absent(&marker, Duration::from_millis(500));
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn brokered_run_times_out() {
    let error = run_brokered_with_timeout(
        ResolvedBrokeredRun {
            command: vec!["sh".into(), "-c".into(), "sleep 2".into()],
            env: Vec::new(),
            files: Vec::new(),
        },
        Duration::from_millis(20),
    )
    .unwrap_err()
    .to_string();

    assert!(error.contains("run timeout"));
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn brokered_run_times_out_after_child_closes_both_pipes() {
    let started = Instant::now();
    let error = run_brokered_with_timeout(
        ResolvedBrokeredRun {
            command: vec!["sh".into(), "-c".into(), "exec 1>&- 2>&-; sleep 5".into()],
            env: Vec::new(),
            files: Vec::new(),
        },
        Duration::from_millis(30),
    )
    .unwrap_err()
    .to_string();

    assert!(error.contains("run timeout"));
    assert!(started.elapsed() < Duration::from_secs(3));
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn zero_run_timeout_wins_even_if_child_can_exit_immediately() {
    let error = run_brokered_with_timeout(
        ResolvedBrokeredRun {
            command: vec!["sh".into(), "-c".into(), "exit 0".into()],
            env: Vec::new(),
            files: Vec::new(),
        },
        Duration::ZERO,
    )
    .unwrap_err()
    .to_string();

    assert!(error.contains("run timeout"));
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn brokered_run_preserves_nonzero_exit_status() {
    let output = run_brokered(ResolvedBrokeredRun {
        command: vec!["sh".into(), "-c".into(), "printf ok; exit 7".into()],
        env: Vec::new(),
        files: Vec::new(),
    })
    .unwrap();

    assert_eq!(output.exit_status, 7);
    assert_eq!(output.exit_signal, None);
    assert_eq!(output.stdout, "ok");
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn brokered_run_reports_unix_signal_exit_status() {
    let output = run_brokered(ResolvedBrokeredRun {
        command: vec!["sh".into(), "-c".into(), "kill -TERM $$".into()],
        env: Vec::new(),
        files: Vec::new(),
    })
    .unwrap();
    assert_eq!(output.exit_status, 143);
    assert_eq!(output.exit_signal, Some(15));
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn brokered_run_kills_same_group_descendant_before_returning_status() {
    let temp = tempfile::tempdir().unwrap();
    let marker = temp.path().join("same-group-descendant-ran");
    let ready = temp.path().join("same-group-descendant-ready");
    let release = temp.path().join("same-group-descendant-release");
    let started = Instant::now();
    let output = run_brokered(ResolvedBrokeredRun {
            command: vec![
                "sh".into(),
                "-c".into(),
                "(printf ready > \"$JIG_VAULT_TEST_READY\"; while [ ! -e \"$JIG_VAULT_TEST_RELEASE\" ]; do sleep 0.01; done; printf leaked > \"$JIG_VAULT_TEST_MARKER\") & while [ ! -e \"$JIG_VAULT_TEST_READY\" ]; do sleep 0.01; done; printf leader; exit 7".into(),
            ],
            env: vec![
                test_env_mapping(
                    "JIG_VAULT_TEST_MARKER",
                    "descendant_marker",
                    marker.as_os_str().as_encoded_bytes(),
                ),
                test_env_mapping(
                    "JIG_VAULT_TEST_READY",
                    "descendant_ready",
                    ready.as_os_str().as_encoded_bytes(),
                ),
                test_env_mapping(
                    "JIG_VAULT_TEST_RELEASE",
                    "descendant_release",
                    release.as_os_str().as_encoded_bytes(),
                ),
            ],
            files: Vec::new(),
        })
        .unwrap();

    assert_eq!(output.exit_status, 7);
    assert_eq!(output.stdout, "leader");
    assert!(started.elapsed() < Duration::from_secs(2));
    assert!(ready.exists());
    fs::write(release, b"release").unwrap();
    assert_path_stays_absent(&marker, Duration::from_millis(500));
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn brokered_run_repeatedly_cleans_an_immediate_background_wrapper() {
    for iteration in 0..8 {
        let output = run_brokered(ResolvedBrokeredRun {
            command: vec![
                "sh".into(),
                "-c".into(),
                "sleep 5 & printf leader; exit 0".into(),
            ],
            env: Vec::new(),
            files: Vec::new(),
        })
        .unwrap_or_else(|error| panic!("iteration {iteration}: {error:#}"));

        assert_eq!(output.exit_status, 0, "iteration {iteration}");
        assert_eq!(output.stdout, "leader", "iteration {iteration}");
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn brokered_run_bounds_escaped_pipe_holder_and_allows_cooperative_teardown() {
    let temp = tempfile::tempdir().unwrap();
    let marker = temp.path().join("escaped.pid");
    let release = temp.path().join("release");
    let done = temp.path().join("done");
    let started = Instant::now();
    let error = run_brokered_with_timeout(
        ResolvedBrokeredRun {
            command: vec![
                std::env::current_exe()
                    .unwrap()
                    .into_os_string()
                    .into_string()
                    .unwrap(),
                "--exact".into(),
                PIPE_ESCAPE_HELPER_TEST.into(),
                "--nocapture".into(),
            ],
            env: vec![
                test_env_mapping(PIPE_ESCAPE_MODE_VAR, "escape_mode", b"spawn"),
                test_env_mapping(
                    PIPE_ESCAPE_MARKER_VAR,
                    "escape_marker",
                    marker.as_os_str().as_encoded_bytes(),
                ),
                test_env_mapping(
                    PIPE_ESCAPE_RELEASE_VAR,
                    "escape_release",
                    release.as_os_str().as_encoded_bytes(),
                ),
                test_env_mapping(
                    PIPE_ESCAPE_DONE_VAR,
                    "escape_done",
                    done.as_os_str().as_encoded_bytes(),
                ),
            ],
            files: Vec::new(),
        },
        Duration::from_secs(2),
    )
    .unwrap_err()
    .to_string();

    assert!(error.contains("output drain"), "unexpected error: {error}");
    assert!(started.elapsed() < Duration::from_secs(3));
    assert!(marker.exists(), "escaped helper never established itself");
    fs::write(&release, b"release").unwrap();
    wait_for_path(&done, Duration::from_secs(3));
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn brokered_run_pipe_escape_helper() {
    let Ok(mode) = std::env::var(PIPE_ESCAPE_MODE_VAR) else {
        return;
    };
    let marker = std::path::PathBuf::from(std::env::var_os(PIPE_ESCAPE_MARKER_VAR).unwrap());
    let release = std::path::PathBuf::from(std::env::var_os(PIPE_ESCAPE_RELEASE_VAR).unwrap());
    let done = std::path::PathBuf::from(std::env::var_os(PIPE_ESCAPE_DONE_VAR).unwrap());
    match mode.as_str() {
        "spawn" => {
            let child = Command::new(std::env::current_exe().unwrap())
                .args(["--exact", PIPE_ESCAPE_HELPER_TEST, "--nocapture"])
                .env(PIPE_ESCAPE_MODE_VAR, "escaped")
                .stdin(Stdio::null())
                .stdout(Stdio::inherit())
                .stderr(Stdio::inherit())
                .spawn()
                .unwrap();
            wait_for_path(&marker, Duration::from_secs(2));
            drop(child);
            std::process::exit(0);
        }
        "escaped" => {
            // SAFETY: this helper is a non-leader descendant of the
            // brokered setsid leader, so it may deliberately escape into
            // a new session to exercise the bounded-drain boundary.
            assert_ne!(unsafe { libc::setsid() }, -1);
            fs::write(&marker, std::process::id().to_string()).unwrap();
            let deadline = Instant::now().checked_add(Duration::from_secs(5)).unwrap();
            while !release.exists() && Instant::now() < deadline {
                thread::sleep(Duration::from_millis(10));
            }
            fs::write(done, b"done").unwrap();
            std::process::exit(0);
        }
        unexpected => panic!("unexpected pipe escape helper mode {unexpected}"),
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn brokered_run_delivers_and_redacts_secret_file() {
    let output = run_brokered(ResolvedBrokeredRun {
        command: vec![
            "sh".into(),
            "-c".into(),
            "test -f \"$TOKEN_FILE\" && cat \"$TOKEN_FILE\"".into(),
        ],
        env: Vec::new(),
        files: vec![ResolvedBrokeredFile {
            var: EnvVarName::parse("TOKEN_FILE").unwrap(),
            secret_name: SecretName::parse("api_token").unwrap(),
            value: SecretBytes::new(b"secret-value".to_vec()),
        }],
    })
    .unwrap();

    assert_eq!(output.exit_status, 0);
    assert_eq!(output.exit_signal, None);
    assert_eq!(output.stdout, "[REDACTED]");
    assert_eq!(output.stderr, "");
}

#[cfg(unix)]
#[test]
fn brokered_secret_files_create_owner_only_paths() {
    let files = [ResolvedBrokeredFile {
        var: EnvVarName::parse("TOKEN_FILE").unwrap(),
        secret_name: SecretName::parse("api_token").unwrap(),
        value: SecretBytes::new(b"secret-value".to_vec()),
    }];

    let secret_files = BrokeredSecretFiles::create(&files).unwrap().unwrap();
    let file_path = std::path::PathBuf::from(secret_files.env()[0].1.clone());
    let dir_path = file_path.parent().unwrap();

    assert_eq!(
        fs::metadata(dir_path).unwrap().permissions().mode() & 0o777,
        0o700
    );
    assert_eq!(
        fs::metadata(file_path).unwrap().permissions().mode() & 0o777,
        0o600
    );
}

#[cfg(unix)]
#[test]
fn wipe_secret_file_overwrites_contents_before_cleanup() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("secret");
    fs::write(&path, b"secret-value").unwrap();
    let mut file = fs::OpenOptions::new().write(true).open(&path).unwrap();

    wipe_secret_file(&mut file, &path).unwrap();

    assert_eq!(fs::read(&path).unwrap(), vec![0_u8; "secret-value".len()]);
}
