use super::*;

mod common;

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn proven_process_group_quiescence_supersedes_a_stale_signal_error() {
    finish_unix_process_group_termination(Some(io::Error::from_raw_os_error(libc::EPERM)), Ok(()))
        .unwrap();

    let confirmation_only =
        finish_unix_process_group_termination(None, Err(anyhow!("confirmation failed")))
            .unwrap_err()
            .to_string();
    assert_eq!(confirmation_only, "confirmation failed");

    let combined = finish_unix_process_group_termination(
        Some(io::Error::from_raw_os_error(libc::EPERM)),
        Err(anyhow!("confirmation failed")),
    )
    .unwrap_err()
    .to_string();
    assert!(combined.contains("process-group SIGKILL failed"));
    assert!(combined.contains("group confirmation also failed: confirmation failed"));
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn unix_group_signal_refuses_identity_loss_before_any_signal() {
    struct SignalState {
        process_group: Option<libc::pid_t>,
        signal_attempts: usize,
    }

    let started = Instant::now();
    let mut state = SignalState {
        process_group: Some(73),
        signal_attempts: 0,
    };
    let error = signal_pinned_unix_process_group_with(
        &mut state,
        73,
        started + Duration::from_secs(1),
        |state| state.process_group,
        |state| {
            state.process_group = None;
            Err(io::Error::from_raw_os_error(libc::ECHILD))
        },
        || started,
        |state, _| {
            state.signal_attempts += 1;
            Ok(())
        },
    )
    .unwrap_err();

    assert_eq!(error.raw_os_error(), Some(libc::ECHILD));
    assert_eq!(state.process_group, None);
    assert_eq!(state.signal_attempts, 0);
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn unix_group_signal_refuses_when_observation_crosses_deadline() {
    struct SignalState {
        process_group: Option<libc::pid_t>,
        signal_attempts: usize,
    }

    let started = Instant::now();
    let deadline = started + Duration::from_millis(10);
    let clock = std::cell::Cell::new(started);
    let mut state = SignalState {
        process_group: Some(73),
        signal_attempts: 0,
    };
    let error = signal_pinned_unix_process_group_with(
        &mut state,
        73,
        deadline,
        |state| state.process_group,
        |_| {
            clock.set(deadline);
            Ok(LeaderObservation::Exited)
        },
        || clock.get(),
        |state, _| {
            state.signal_attempts += 1;
            Ok(())
        },
    )
    .unwrap_err();

    assert_eq!(error.kind(), io::ErrorKind::TimedOut);
    assert!(error.to_string().contains("cleanup deadline"));
    assert_eq!(state.signal_attempts, 0);
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn unix_group_confirmation_resignals_a_late_member_and_between_linux_proofs() {
    struct ConfirmationState {
        late_member_live: bool,
        signal_attempts: usize,
        proof_attempts: usize,
    }

    let started = Instant::now();
    let clock = std::cell::Cell::new(started);
    let mut state = ConfirmationState {
        // Model a member created after the caller's initial SIGKILL. The
        // confirmation loop must kill it before accepting any proof.
        late_member_live: true,
        signal_attempts: 0,
        proof_attempts: 0,
    };
    confirm_unix_process_group_quiescent_with(
        &mut state,
        73,
        started + Duration::from_secs(1),
        2,
        |state, process_group, received_deadline| {
            assert_eq!(process_group, 73);
            assert_eq!(received_deadline, started + Duration::from_secs(1));
            state.signal_attempts += 1;
            state.late_member_live = false;
            Ok(())
        },
        |state, process_group, _| {
            assert_eq!(process_group, 73);
            state.proof_attempts += 1;
            Ok(!state.late_member_live)
        },
        || clock.get(),
        |duration| clock.set(clock.get() + duration),
    )
    .unwrap();

    assert_eq!(state.signal_attempts, 2);
    assert_eq!(state.proof_attempts, 2);
    assert!(!state.late_member_live);
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn unix_group_confirmation_treats_esrch_and_eperm_as_inconclusive() {
    struct ConfirmationState {
        signal_attempts: usize,
        proof_attempts: usize,
    }

    let started = Instant::now();
    let clock = std::cell::Cell::new(started);
    let mut state = ConfirmationState {
        signal_attempts: 0,
        proof_attempts: 0,
    };
    confirm_unix_process_group_quiescent_with(
        &mut state,
        73,
        started + Duration::from_secs(1),
        1,
        |state, _, _| {
            state.signal_attempts += 1;
            let errno = if state.signal_attempts == 1 {
                libc::ESRCH
            } else {
                libc::EPERM
            };
            Err(io::Error::from_raw_os_error(errno))
        },
        |state, _, _| {
            state.proof_attempts += 1;
            // ESRCH must not finish the first iteration. EPERM may be
            // superseded only by this independent proof on the second.
            Ok(state.proof_attempts == 2)
        },
        || clock.get(),
        |duration| clock.set(clock.get() + duration),
    )
    .unwrap();

    assert_eq!(state.signal_attempts, 2);
    assert_eq!(state.proof_attempts, 2);
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn unix_group_confirmation_preserves_signal_error_before_proof_error() {
    let error = confirm_unix_process_group_quiescent_with(
        &mut (),
        73,
        Instant::now() + Duration::from_secs(1),
        1,
        |(), _, _| Err(io::Error::from_raw_os_error(libc::EPERM)),
        |(), _, _| Err(anyhow!("sole-leader snapshot failed")),
        Instant::now,
        |_| {},
    )
    .unwrap_err();
    let error = format!("{error:#}");

    assert!(error.contains("process-group SIGKILL failed"));
    assert!(error.contains(&io::Error::from_raw_os_error(libc::EPERM).to_string()));
    assert!(error.contains("group confirmation also failed"));
    assert!(error.contains("sole-leader snapshot failed"));
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn unix_group_confirmation_rejects_a_proof_that_crosses_deadline() {
    struct ConfirmationState {
        signal_attempts: usize,
        proof_attempts: usize,
    }

    let started = Instant::now();
    let deadline = started + Duration::from_millis(10);
    let clock = std::cell::Cell::new(started);
    let mut state = ConfirmationState {
        signal_attempts: 0,
        proof_attempts: 0,
    };
    let error = confirm_unix_process_group_quiescent_with(
        &mut state,
        73,
        deadline,
        1,
        |state, _, received_deadline| {
            assert_eq!(received_deadline, deadline);
            state.signal_attempts += 1;
            Ok(())
        },
        |state, _, received_deadline| {
            assert_eq!(received_deadline, deadline);
            state.proof_attempts += 1;
            clock.set(deadline);
            Ok(true)
        },
        || clock.get(),
        |_| panic!("an expired proof must not reach the sleep callback"),
    )
    .unwrap_err()
    .to_string();

    assert!(
        error.contains("cleanup deadline"),
        "unexpected error: {error}"
    );
    assert_eq!(state.signal_attempts, 1);
    assert_eq!(state.proof_attempts, 1);
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn macos_eperm_special_case_preserves_signal_failure_until_proven_quiescent() {
    let eperm = || io::Error::from_raw_os_error(libc::EPERM);
    let eperm_text = eperm().to_string();

    let observation_error = resolve_macos_group_signal_eperm(
        eperm(),
        Err(io::Error::other("leader observation failed")),
        || panic!("confirmation must not run after an observation error"),
    )
    .unwrap_err()
    .to_string();
    assert!(observation_error.contains(&eperm_text));
    assert!(observation_error.contains("failed to observe brokered process leader"));
    assert!(observation_error.contains("leader observation failed"));

    let confirmation_error =
        resolve_macos_group_signal_eperm(eperm(), Ok(LeaderObservation::Exited), || {
            Err(anyhow!("sole-leader snapshot failed"))
        })
        .unwrap_err()
        .to_string();
    assert!(confirmation_error.contains(&eperm_text));
    assert!(confirmation_error.contains("sole-leader snapshot failed"));

    assert!(
        resolve_macos_group_signal_eperm(eperm(), Ok(LeaderObservation::Exited), || Ok(()),)
            .unwrap()
            .is_none()
    );

    let mut confirmation_called = false;
    let fallback_error =
        resolve_macos_group_signal_eperm(eperm(), Ok(LeaderObservation::Running), || {
            confirmation_called = true;
            Ok(())
        })
        .unwrap()
        .expect("a running leader must keep EPERM for the fallback path");
    assert!(!confirmation_called);
    assert_eq!(fallback_error.raw_os_error(), Some(libc::EPERM));
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn unix_process_group_gate_targets_only_the_pinned_identity() {
    let group = PinnedUnixProcessGroup {
        id: jig_owned_process::unix::ProcessGroupId::new(4242).unwrap(),
    };
    let mut target = None;
    with_pinned_unix_process_group(Some(&group), |owned| {
        target = Some(owned.id.as_raw());
        Ok(())
    })
    .unwrap();
    assert_eq!(target, Some(4242));

    target = None;
    let error = with_pinned_unix_process_group(None, |owned| {
        target = Some(owned.id.as_raw());
        Ok(())
    })
    .unwrap_err();
    assert!(error.to_string().contains("refusing to signal"));
    assert_eq!(target, None, "lost identity reached the signal closure");
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn unix_wait_error_clears_identity_only_for_echild() {
    let group = PinnedUnixProcessGroup {
        id: jig_owned_process::unix::ProcessGroupId::new(4242).unwrap(),
    };
    let mut identity = Some(group);
    update_unix_process_group_after_wait_error(
        &mut identity,
        &io::Error::from_raw_os_error(libc::EINVAL),
    );
    assert!(identity.is_some());

    update_unix_process_group_after_wait_error(
        &mut identity,
        &io::Error::from_raw_os_error(libc::ENOSYS),
    );
    assert!(identity.is_some());

    update_unix_process_group_after_wait_error(
        &mut identity,
        &io::Error::from_raw_os_error(libc::ECHILD),
    );
    assert!(identity.is_none());
}

#[cfg(target_os = "macos")]
#[test]
fn macos_group_confirmation_resignals_an_exited_leader_with_a_live_member() {
    let temp = tempfile::tempdir().unwrap();
    let ready = temp.path().join("macos-group-member-ready");
    let release = temp.path().join("macos-group-member-release");
    let leak = temp.path().join("macos-group-member-leaked");
    let mut command = Command::new("sh");
    command
            .args([
                "-c",
                "(printf ready > \"$JIG_VAULT_TEST_READY\"; while [ ! -e \"$JIG_VAULT_TEST_RELEASE\" ]; do sleep 0.01; done; printf leaked > \"$JIG_VAULT_TEST_LEAK\") & while [ ! -e \"$JIG_VAULT_TEST_READY\" ]; do sleep 0.01; done; exit 0",
            ])
            .env("JIG_VAULT_TEST_READY", &ready)
            .env("JIG_VAULT_TEST_RELEASE", &release)
            .env("JIG_VAULT_TEST_LEAK", &leak)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
    let mut process = BrokeredProcess::spawn(&mut command).unwrap();
    wait_for_path(&ready, Duration::from_secs(2));

    let observation_deadline = Instant::now().checked_add(Duration::from_secs(2)).unwrap();
    loop {
        if process.observe_leader().unwrap() == LeaderObservation::Exited {
            break;
        }
        assert!(
            Instant::now() < observation_deadline,
            "brokered test leader did not exit"
        );
        thread::sleep(Duration::from_millis(2));
    }
    let process_group = process.process_group.unwrap().id.as_raw();
    let confirmation_deadline = Instant::now()
        .checked_add(Duration::from_millis(500))
        .unwrap();
    confirm_unix_process_group_quiescent(&mut process, process_group, confirmation_deadline)
        .unwrap();

    let status = process
        .terminate_and_reap(BROKERED_PROCESS_CLEANUP_TIMEOUT)
        .unwrap();
    assert_eq!(status.code(), Some(0));
    fs::write(release, b"release").unwrap();
    assert_path_stays_absent(&leak, Duration::from_millis(300));
}

#[cfg(target_os = "macos")]
#[test]
fn macos_group_confirmation_resignals_a_running_sole_leader_before_proof() {
    let temp = tempfile::tempdir().unwrap();
    let ready = temp.path().join("macos-running-leader-ready");
    let leak = temp.path().join("macos-running-leader-leaked");
    let mut command = Command::new("sh");
    command
            .args([
                "-c",
                "printf ready > \"$JIG_VAULT_TEST_READY\"; kill -STOP $$; printf leaked > \"$JIG_VAULT_TEST_LEAK\"",
            ])
            .env("JIG_VAULT_TEST_READY", &ready)
            .env("JIG_VAULT_TEST_LEAK", &leak)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
    let mut process = BrokeredProcess::spawn(&mut command).unwrap();
    wait_for_path(&ready, Duration::from_secs(2));

    let process_group = process.process_group.unwrap().id.as_raw();
    let confirmation_deadline = Instant::now()
        .checked_add(Duration::from_millis(500))
        .unwrap();
    confirm_unix_process_group_quiescent(&mut process, process_group, confirmation_deadline)
        .unwrap();

    let status = process
        .terminate_and_reap(BROKERED_PROCESS_CLEANUP_TIMEOUT)
        .unwrap();
    assert_eq!(status.signal(), Some(libc::SIGKILL));
    assert_path_stays_absent(&leak, Duration::from_millis(300));
}

#[test]
fn linux_stat_parser_handles_embedded_closing_delimiter_and_process_states() {
    let live = parse_linux_process_stat("41 (worker ) suffix) S 1 73 0".into()).unwrap();
    let zombie = parse_linux_process_stat("42 (zombie) Z 1 73 0".into()).unwrap();
    let dead = parse_linux_process_stat("43 (dead) X 1 73 0".into()).unwrap();

    assert_eq!(live.process_group, 73);
    assert!(live.live);
    assert!(!zombie.live);
    assert!(!dead.live);
}

#[test]
fn linux_group_classifier_ignores_zombies_but_finds_live_members() {
    let only_zombie = linux_process_group_has_live_members_with(
        73,
        [41],
        |_| Ok("41 (zombie) Z 1 73 0".into()),
        |_| unreachable!(),
        || true,
    )
    .unwrap();
    assert!(!only_zombie);

    let live = linux_process_group_has_live_members_with(
        73,
        [41, 42],
        |pid| {
            Ok(format!(
                "{pid} (worker) S 1 {} 0",
                if pid == 42 { 73 } else { 80 }
            ))
        },
        |_| unreachable!(),
        || true,
    )
    .unwrap();
    assert!(live);
}

#[test]
fn linux_group_classifier_fails_closed_for_unreadable_owned_member() {
    let vanished = linux_process_group_has_live_members_with(
        73,
        [41],
        |_| Err(io::Error::new(io::ErrorKind::NotFound, "vanished")),
        |_| Ok(None),
        || true,
    )
    .unwrap();
    assert!(!vanished);

    let unrelated = linux_process_group_has_live_members_with(
        73,
        [42],
        |_| {
            Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "unreadable",
            ))
        },
        |_| Ok(Some(80)),
        || true,
    )
    .unwrap();
    assert!(!unrelated);

    let owned_error = linux_process_group_has_live_members_with(
        73,
        [43],
        |_| {
            Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "unreadable",
            ))
        },
        |_| Ok(Some(73)),
        || true,
    )
    .unwrap_err()
    .to_string();
    assert!(owned_error.contains("belongs to owned process group 73"));
}

#[test]
fn linux_group_classifier_checks_budget_after_stat_read() {
    let within_budget = std::cell::Cell::new(true);
    let membership_checks = std::cell::Cell::new(0usize);
    let error = linux_process_group_has_live_members_with(
        73,
        [41],
        |_| {
            within_budget.set(false);
            Ok("41 (worker) S 1 73 0".into())
        },
        |_| {
            membership_checks.set(membership_checks.get() + 1);
            Ok(Some(73))
        },
        || within_budget.get(),
    )
    .unwrap_err()
    .to_string();
    assert_eq!(membership_checks.get(), 0);
    assert!(error.contains("cleanup scan exceeded its deadline"));
}

#[test]
fn linux_group_enumeration_fails_closed_when_advancing_exhausts_budget() {
    let within_budget = std::cell::Cell::new(true);
    let entries = std::iter::from_fn(|| {
        within_budget.set(false);
        None::<io::Result<i32>>
    });

    let error = collect_linux_process_ids_with(73, entries, Some, || within_budget.get())
        .unwrap_err()
        .to_string();

    assert!(error.contains("cleanup scan exceeded its deadline"));
}

#[test]
fn linux_group_classifier_checks_budget_after_fallback_lookup() {
    let within_budget = std::cell::Cell::new(true);
    let error = linux_process_group_has_live_members_with(
        73,
        [41],
        |_| Err(io::Error::other("injected stat failure")),
        |_| {
            within_budget.set(false);
            Ok(None)
        },
        || within_budget.get(),
    )
    .unwrap_err()
    .to_string();

    assert!(error.contains("cleanup scan exceeded its deadline"));
}

#[test]
fn linux_group_classifier_checks_budget_before_live_and_empty_results() {
    let live_checks = std::cell::Cell::new(0usize);
    let live_error = linux_process_group_has_live_members_with(
        73,
        [41],
        |_| Ok("41 (worker) S 1 73 0".into()),
        |_| unreachable!(),
        || {
            let check = live_checks.get() + 1;
            live_checks.set(check);
            check <= 3
        },
    )
    .unwrap_err()
    .to_string();
    assert_eq!(live_checks.get(), 4);
    assert!(live_error.contains("cleanup scan exceeded its deadline"));

    let empty_checks = std::cell::Cell::new(0usize);
    let empty_error = linux_process_group_has_live_members_with(
        73,
        std::iter::empty(),
        |_| unreachable!(),
        |_| unreachable!(),
        || {
            let check = empty_checks.get() + 1;
            empty_checks.set(check);
            check == 1
        },
    )
    .unwrap_err()
    .to_string();
    assert_eq!(empty_checks.get(), 2);
    assert!(empty_error.contains("cleanup scan exceeded its deadline"));
}

#[cfg(windows)]
#[test]
fn windows_brokered_process_is_created_suspended_in_a_new_group() {
    use windows_sys::Win32::System::Threading::{CREATE_NEW_PROCESS_GROUP, CREATE_SUSPENDED};

    let flags = windows_brokered_process_creation_flags();
    assert_eq!(flags, CREATE_SUSPENDED | CREATE_NEW_PROCESS_GROUP);
    assert_ne!(flags & CREATE_SUSPENDED, 0);
}

#[cfg(windows)]
#[test]
fn windows_brokered_job_is_kill_on_close() {
    use std::ffi::c_void;
    use std::mem::size_of;
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Foundation::HANDLE;
    use windows_sys::Win32::System::JobObjects::{
        JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
        JobObjectExtendedLimitInformation, QueryInformationJobObject,
    };

    let job = create_brokered_process_job().unwrap();
    let mut information = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
    // SAFETY: job and the correctly sized output structure remain live.
    let queried = unsafe {
        QueryInformationJobObject(
            job.as_raw_handle() as HANDLE,
            JobObjectExtendedLimitInformation,
            (&raw mut information).cast::<c_void>(),
            size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            std::ptr::null_mut(),
        )
    };
    assert_ne!(queried, 0);
    assert_ne!(
        information.BasicLimitInformation.LimitFlags & JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
        0
    );
}

#[cfg(windows)]
#[test]
fn windows_brokered_job_terminates_descendant_before_returning_status() {
    let temp = tempfile::tempdir().unwrap();
    let ready = temp.path().join("ready");
    let release = temp.path().join("release");
    let leak = temp.path().join("descendant-survived");
    let output = run_brokered_with_timeout(
        ResolvedBrokeredRun {
            command: vec![
                std::env::current_exe()
                    .unwrap()
                    .into_os_string()
                    .into_string()
                    .unwrap(),
                "--exact".into(),
                WINDOWS_JOB_HELPER_TEST.into(),
                "--nocapture".into(),
            ],
            env: vec![
                test_env_mapping(WINDOWS_JOB_MODE_VAR, "windows_job_mode", b"parent"),
                test_env_mapping(
                    WINDOWS_JOB_READY_VAR,
                    "windows_job_ready",
                    ready.as_os_str().as_encoded_bytes(),
                ),
                test_env_mapping(
                    WINDOWS_JOB_RELEASE_VAR,
                    "windows_job_release",
                    release.as_os_str().as_encoded_bytes(),
                ),
                test_env_mapping(
                    WINDOWS_JOB_LEAK_VAR,
                    "windows_job_leak",
                    leak.as_os_str().as_encoded_bytes(),
                ),
            ],
            files: Vec::new(),
        },
        Duration::from_secs(2),
    )
    .unwrap();

    assert_eq!(output.exit_status, 7);
    assert!(ready.exists());
    fs::write(release, b"release").unwrap();
    assert_path_stays_absent(&leak, Duration::from_secs(1));
}

#[cfg(windows)]
#[test]
fn windows_brokered_job_descendant_helper() {
    let Ok(mode) = std::env::var(WINDOWS_JOB_MODE_VAR) else {
        return;
    };
    let ready = std::path::PathBuf::from(std::env::var_os(WINDOWS_JOB_READY_VAR).unwrap());
    let release = std::path::PathBuf::from(std::env::var_os(WINDOWS_JOB_RELEASE_VAR).unwrap());
    let leak = std::path::PathBuf::from(std::env::var_os(WINDOWS_JOB_LEAK_VAR).unwrap());
    match mode.as_str() {
        "parent" => {
            let child = Command::new(std::env::current_exe().unwrap())
                .args(["--exact", WINDOWS_JOB_HELPER_TEST, "--nocapture"])
                .env(WINDOWS_JOB_MODE_VAR, "descendant")
                .stdin(Stdio::null())
                .stdout(Stdio::inherit())
                .stderr(Stdio::inherit())
                .spawn()
                .unwrap();
            wait_for_path(&ready, Duration::from_secs(2));
            drop(child);
            std::process::exit(7);
        }
        "descendant" => {
            fs::write(ready, b"ready").unwrap();
            let deadline = Instant::now().checked_add(Duration::from_secs(5)).unwrap();
            while !release.exists() && Instant::now() < deadline {
                thread::sleep(Duration::from_millis(10));
            }
            if release.exists() {
                fs::write(leak, b"survived").unwrap();
            }
            std::process::exit(0);
        }
        unexpected => panic!("unexpected Windows Job helper mode {unexpected}"),
    }
}
