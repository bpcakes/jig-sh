#[cfg(test)]
use std::io;
#[cfg(all(test, target_os = "macos"))]
use std::os::unix::process::ExitStatusExt;
use std::process::{Command, Stdio};
#[cfg(test)]
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result as AnyResult, anyhow, bail};
use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

use crate::SecretBytes;
use crate::env_policy::is_preserved_env_var_name;
use crate::redact::Redactor;
use crate::types::{EnvVarName, SecretName};

mod output;
mod process;
#[cfg(any(target_os = "linux", test))]
mod process_linux;
#[cfg(any(target_os = "linux", target_os = "macos", test))]
mod process_unix;
#[cfg(windows)]
mod process_windows;
mod secret_files;

use process::{BrokeredProcess, wait_for_capped_output};
use secret_files::BrokeredSecretFiles;
#[cfg(all(test, unix))]
use secret_files::wipe_secret_file;

// Keep this cap aligned with redaction cost: redaction scans the captured text
// once per raw/encoded secret needle.
pub const MAX_CAPTURED_STREAM_BYTES: usize = 1024 * 1024;
const BROKERED_RUN_TIMEOUT: Duration = Duration::from_secs(30 * 60);
const BROKERED_PROCESS_POLL_INTERVAL: Duration = Duration::from_millis(10);
const BROKERED_PROCESS_CLEANUP_TIMEOUT: Duration = Duration::from_secs(2);
const BROKERED_OUTPUT_DRAIN_TIMEOUT: Duration = Duration::from_millis(250);
const MAX_STREAM_READS_PER_POLL: usize = 16;

fn checked_deadline(label: &str, timeout: Duration) -> AnyResult<Instant> {
    Instant::now()
        .checked_add(timeout)
        .ok_or_else(|| anyhow!("{label} deadline overflowed"))
}

#[derive(Debug)]
pub(crate) struct ResolvedBrokeredEnv {
    pub(crate) var: EnvVarName,
    pub(crate) secret_name: SecretName,
    pub(crate) value: SecretBytes,
}

#[derive(Debug)]
pub(crate) struct ResolvedBrokeredFile {
    pub(crate) var: EnvVarName,
    pub(crate) secret_name: SecretName,
    pub(crate) value: SecretBytes,
}

#[derive(Debug)]
pub(crate) struct ResolvedBrokeredRun {
    pub(crate) command: Vec<String>,
    pub(crate) env: Vec<ResolvedBrokeredEnv>,
    pub(crate) files: Vec<ResolvedBrokeredFile>,
}

#[non_exhaustive]
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct RunOutput {
    pub exit_status: i32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exit_signal: Option<i32>,
    pub stdout: String,
    pub stderr: String,
}

pub(crate) fn run_brokered(request: ResolvedBrokeredRun) -> AnyResult<RunOutput> {
    run_brokered_with_timeout(request, BROKERED_RUN_TIMEOUT)
}

fn run_brokered_with_timeout(
    request: ResolvedBrokeredRun,
    timeout: Duration,
) -> AnyResult<RunOutput> {
    // Keep this guard for direct crate callers; clap enforces it for the CLI.
    if request.command.is_empty() {
        bail!("vault run requires a command after --");
    }
    let redactor = Redactor::from_secret_slices(
        request
            .env
            .iter()
            .map(|mapping| mapping.value.as_slice())
            .chain(request.files.iter().map(|mapping| mapping.value.as_slice())),
    );
    let file_env = BrokeredSecretFiles::create(&request.files)?;
    let mut env_values = Vec::<(String, Zeroizing<String>)>::new();
    for mapping in request.env {
        let env_value = match mapping.value.into_zeroizing_string() {
            Ok(value) => value,
            Err(_value) => {
                bail!(
                    "vault secret '{}' cannot be injected as env var {} because it is not valid UTF-8",
                    mapping.secret_name.as_str(),
                    mapping.var.as_str()
                );
            }
        };
        env_values.push((mapping.var.as_str().to_string(), env_value));
    }

    let mut command = Command::new(&request.command[0]);
    command.args(&request.command[1..]).env_clear();
    preserve_minimal_environment(&mut command);
    for (name, value) in &env_values {
        // std::process::Command copies env values into OsString storage; keep
        // our source copy zeroized, but the std-owned copy is dropped normally.
        command.env(name, value.as_str());
    }
    if let Some(file_env) = &file_env {
        for (name, path) in file_env.env() {
            command.env(name, path);
        }
    }
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let process = BrokeredProcess::spawn(&mut command)
        .with_context(|| format!("failed to run brokered command '{}'", request.command[0]))?;
    let (status, stdout, stderr) = wait_for_capped_output(process, &request.command[0], timeout)?;
    Ok(RunOutput {
        exit_status: status.exit_status,
        exit_signal: status.exit_signal,
        stdout: redactor.redact_bytes_lossy(stdout.as_slice()),
        stderr: redactor.redact_bytes_lossy(stderr.as_slice()),
    })
}

fn preserve_minimal_environment(command: &mut Command) {
    // Env forwarding is allowlist-only. Loader/interpreter hooks such as
    // LD_PRELOAD, DYLD_*, PYTHONPATH, NODE_OPTIONS, SSH_AUTH_SOCK, XDG_*,
    // and TZ stay out unless deliberately added to the exact list below.
    for (name, value) in std::env::vars() {
        if is_preserved_env_var_name(&name) {
            command.env(name, value);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::process::*;
    #[cfg(any(target_os = "linux", test))]
    use super::process_linux::*;
    #[cfg(any(target_os = "linux", target_os = "macos", test))]
    use super::process_unix::*;
    #[cfg(windows)]
    use super::process_windows::*;
    use super::*;
    use std::fs;
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    const PIPE_ESCAPE_MODE_VAR: &str = "JIG_VAULT_PIPE_ESCAPE_MODE";
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    const PIPE_ESCAPE_MARKER_VAR: &str = "JIG_VAULT_PIPE_ESCAPE_MARKER";
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    const PIPE_ESCAPE_RELEASE_VAR: &str = "JIG_VAULT_PIPE_ESCAPE_RELEASE";
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    const PIPE_ESCAPE_DONE_VAR: &str = "JIG_VAULT_PIPE_ESCAPE_DONE";
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    const PIPE_ESCAPE_HELPER_TEST: &str = "run::tests::brokered_run_pipe_escape_helper";

    #[cfg(windows)]
    const WINDOWS_JOB_MODE_VAR: &str = "JIG_VAULT_WINDOWS_JOB_MODE";
    #[cfg(windows)]
    const WINDOWS_JOB_READY_VAR: &str = "JIG_VAULT_WINDOWS_JOB_READY";
    #[cfg(windows)]
    const WINDOWS_JOB_RELEASE_VAR: &str = "JIG_VAULT_WINDOWS_JOB_RELEASE";
    #[cfg(windows)]
    const WINDOWS_JOB_LEAK_VAR: &str = "JIG_VAULT_WINDOWS_JOB_LEAK";
    #[cfg(windows)]
    const WINDOWS_JOB_HELPER_TEST: &str = "run::tests::windows_brokered_job_descendant_helper";

    #[cfg(any(target_os = "linux", target_os = "macos", windows))]
    fn test_env_mapping(var: &str, secret_name: &str, value: &[u8]) -> ResolvedBrokeredEnv {
        ResolvedBrokeredEnv {
            var: EnvVarName::parse(var).unwrap(),
            secret_name: SecretName::parse(secret_name).unwrap(),
            value: SecretBytes::new(value.to_vec()),
        }
    }

    #[cfg(any(target_os = "linux", target_os = "macos", windows))]
    fn wait_for_path(path: &std::path::Path, timeout: Duration) {
        let deadline = Instant::now().checked_add(timeout).unwrap();
        while !path.exists() {
            assert!(
                Instant::now() < deadline,
                "timed out waiting for test marker {}",
                path.display()
            );
            thread::sleep(Duration::from_millis(10));
        }
    }

    #[cfg(any(target_os = "linux", target_os = "macos", windows))]
    fn assert_path_stays_absent(path: &std::path::Path, duration: Duration) {
        let deadline = Instant::now().checked_add(duration).unwrap();
        while Instant::now() < deadline {
            assert!(
                !path.exists(),
                "terminated descendant wrote unexpected marker {}",
                path.display()
            );
            thread::sleep(Duration::from_millis(10));
        }
    }

    #[test]
    fn cleanup_deadline_is_fixed_by_the_first_attempt() {
        let mut deadline = None;
        let (first, first_attempt) =
            fixed_process_cleanup_deadline(&mut deadline, Duration::from_secs(1)).unwrap();
        let (second, second_attempt) =
            fixed_process_cleanup_deadline(&mut deadline, Duration::from_secs(30)).unwrap();

        assert!(first_attempt);
        assert!(!second_attempt);
        assert_eq!(first, second);
    }

    #[test]
    fn secondary_cleanup_error_does_not_replace_primary_error() {
        let error = append_secondary_error(
            anyhow!("capture failed first"),
            "process cleanup also failed",
            Some(anyhow!("cleanup failed second")),
        )
        .to_string();

        assert!(error.starts_with("capture failed first"));
        assert!(error.contains("process cleanup also failed: cleanup failed second"));
    }

    #[test]
    fn completed_poll_results_take_precedence_over_a_newly_expired_deadline() {
        let drain_error =
            preserve_poll_result_before_timeout(Err(anyhow!("capture failed first")), None, || {
                anyhow!("timeout second")
            })
            .unwrap_err()
            .to_string();
        assert_eq!(drain_error, "capture failed first");

        assert_eq!(
            preserve_leader_poll_result_before_timeout(
                Ok(LeaderObservation::Exited),
                None,
                || anyhow!("timeout second"),
            )
            .unwrap(),
            LeaderObservation::Exited,
        );
        let observation_error = preserve_leader_poll_result_before_timeout(
            Err(anyhow!("observation failed first")),
            None,
            || anyhow!("timeout second"),
        )
        .unwrap_err()
        .to_string();
        assert_eq!(observation_error, "observation failed first");
        let timeout = preserve_leader_poll_result_before_timeout(
            Ok(LeaderObservation::Running),
            None,
            || anyhow!("timeout after running observation"),
        )
        .unwrap_err()
        .to_string();
        assert_eq!(timeout, "timeout after running observation");
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn proven_process_group_quiescence_supersedes_a_stale_signal_error() {
        finish_unix_process_group_termination(
            Some(io::Error::from_raw_os_error(libc::EPERM)),
            Ok(()),
        )
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

        assert!(error.contains("capture limit"));
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

        assert!(error.contains("capture limit"));
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
}
