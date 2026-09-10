use super::*;

#[test]
fn macos_short_lived_output_overflow_retains_its_policy_error() {
    struct NoopObserver;
    impl OwnedProcessObserver for NoopObserver {}
    for iteration in 0..100 {
        let mut command = Command::new("/usr/bin/env");
        command
            .args(["python3", "-c", "print('x'*8192)"])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let result = run_owned_process_tree_with_output_policy_and_observer(
            &mut command,
            Duration::from_secs(5),
            ProcessOutputLimits {
                stdout: 128,
                stderr: 128,
            },
            ProcessOutputOverflowPolicy::Error,
            &mut NoopObserver,
        );
        assert!(
            matches!(
                result,
                Err(OwnedProcessTreeError::OutputLimitExceeded(
                    OwnedProcessOutputStream::Stdout
                ))
            ),
            "iteration {iteration}: {:?}",
            result.err()
        );
    }
}

#[test]
fn macos_eperm_retries_require_quiescence_within_the_original_deadline() {
    for eventually_quiescent in [false, true] {
        let start = Instant::now();
        let deadline = start + Duration::from_millis(30);
        let now = std::cell::Cell::new(start);
        let mut attempts = 0;
        let result = confirm_process_group_quiescent_with(
            &mut attempts,
            73,
            deadline,
            1,
            "injected macOS exit transition",
            |attempts, _, _| {
                signal_owned_process_group_with(
                    attempts,
                    |attempts| {
                        Ok(if eventually_quiescent && *attempts >= 3 {
                            OwnedProcessObservation::Exited
                        } else {
                            OwnedProcessObservation::Running
                        })
                    },
                    |attempts| {
                        *attempts += 1;
                        Err(std::io::Error::from_raw_os_error(libc::EPERM))
                    },
                )
            },
            |attempts, _, _| Ok(eventually_quiescent && *attempts >= 3),
            || now.get(),
            |duration| now.set(now.get() + duration),
        );
        assert_eq!(attempts, 3);
        if eventually_quiescent {
            result.unwrap();
            assert!(now.get() < deadline);
        } else {
            assert_eq!(result.unwrap_err().kind(), std::io::ErrorKind::TimedOut);
            assert_eq!(now.get(), deadline);
        }
    }
}
