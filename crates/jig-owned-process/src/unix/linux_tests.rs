use super::*;

#[test]
fn linux_stat_parser_validates_pid_and_handles_arbitrary_command_bytes() {
    let mut live_stat = b"73 (worker ) suffix-".to_vec();
    live_stat.push(0xff);
    live_stat.extend_from_slice(b") S 1 73 0 0 0");
    assert_eq!(
        parse_linux_process_stat(73, &live_stat).unwrap(),
        LinuxProcessObservation {
            process_group: 73,
            live: true,
        }
    );

    for state in ["Z", "X", "x"] {
        let stat = format!("73 (worker) {state} 1 73 0 0 0");
        assert!(!parse_linux_process_stat(73, stat.as_bytes()).unwrap().live);
    }
    assert!(parse_linux_process_stat(0, b"0 (worker) Z 1 0 0 0 0").is_err());
    assert!(parse_linux_process_stat(73, b"74 (worker) Z 1 73 0 0 0").is_err());
    assert!(parse_linux_process_stat(73, b") Z 1 73 0 0 0").is_err());
}

#[test]
fn linux_process_scan_finds_live_members_and_ignores_dead_or_proven_unrelated_pids() {
    let process_group = ProcessGroupId::new(73).unwrap();
    let live = linux_process_group_has_live_members_with(
        process_group,
        [73, 74, 75, 76],
        |pid| match pid {
            73 => Ok(b"73 (leader) Z 1 73 0 0 0".to_vec()),
            74 => Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "malformed unrelated stat",
            )),
            75 => Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "vanished process",
            )),
            76 => Ok(b"76 (worker) S 73 73 0 0 0".to_vec()),
            _ => unreachable!(),
        },
        |pid| match pid {
            74 => Ok(Some(90)),
            75 => Ok(None),
            _ => panic!("unexpected fallback lookup for PID {pid}"),
        },
        || true,
    )
    .unwrap();
    assert!(live);

    let only_dead = linux_process_group_has_live_members_with(
        process_group,
        [73],
        |_| Ok(b"73 (leader) Z 1 73 0 0 0".to_vec()),
        |_| unreachable!(),
        || true,
    )
    .unwrap();
    assert!(!only_dead);
}

#[test]
fn linux_process_scan_fails_closed_for_unreadable_or_unclassifiable_owned_pids() {
    let process_group = ProcessGroupId::new(73).unwrap();
    let owned_error = linux_process_group_has_live_members_with(
        process_group,
        [74],
        |_| {
            Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "injected stat denial",
            ))
        },
        |_| Ok(Some(73)),
        || true,
    )
    .unwrap_err();
    assert_eq!(owned_error.kind(), std::io::ErrorKind::PermissionDenied);
    assert!(
        owned_error
            .to_string()
            .contains("belongs to Linux process group 73")
    );

    let unknown_error = linux_process_group_has_live_members_with(
        process_group,
        [74],
        |_| Err(std::io::Error::other("injected stat failure")),
        |_| Err(std::io::Error::other("injected getpgid failure")),
        || true,
    )
    .unwrap_err();
    assert!(unknown_error.to_string().contains("or prove"));
    assert!(
        unknown_error
            .to_string()
            .contains("injected getpgid failure")
    );
}

#[test]
fn linux_process_scan_checks_budget_around_every_observation_and_result() {
    let process_group = ProcessGroupId::new(73).unwrap();
    let within_budget = std::cell::Cell::new(true);
    let entries = std::iter::from_fn(|| {
        within_budget.set(false);
        None::<std::io::Result<i32>>
    });
    let enumeration_error =
        collect_linux_process_ids_with(process_group, entries, Some, || within_budget.get())
            .unwrap_err();
    assert_eq!(enumeration_error.kind(), std::io::ErrorKind::TimedOut);

    let within_budget = std::cell::Cell::new(true);
    let fallback_error = linux_process_group_has_live_members_with(
        process_group,
        [74],
        |_| Err(std::io::Error::other("injected stat failure")),
        |_| {
            within_budget.set(false);
            Ok(None)
        },
        || within_budget.get(),
    )
    .unwrap_err();
    assert_eq!(fallback_error.kind(), std::io::ErrorKind::TimedOut);

    let live_checks = std::cell::Cell::new(0usize);
    let live_error = linux_process_group_has_live_members_with(
        process_group,
        [74],
        |_| Ok(b"74 (worker) S 1 73 0 0 0".to_vec()),
        |_| unreachable!(),
        || {
            let check = live_checks.get() + 1;
            live_checks.set(check);
            check <= 3
        },
    )
    .unwrap_err();
    assert_eq!(live_checks.get(), 4);
    assert_eq!(live_error.kind(), std::io::ErrorKind::TimedOut);

    let empty_checks = std::cell::Cell::new(0usize);
    let empty_error = linux_process_group_has_live_members_with(
        process_group,
        std::iter::empty(),
        |_| unreachable!(),
        |_| unreachable!(),
        || {
            let check = empty_checks.get() + 1;
            empty_checks.set(check);
            check == 1
        },
    )
    .unwrap_err();
    assert_eq!(empty_checks.get(), 2);
    assert_eq!(empty_error.kind(), std::io::ErrorKind::TimedOut);
}
