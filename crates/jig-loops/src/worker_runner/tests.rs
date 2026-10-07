#[cfg(test)]
mod tests {
    #[cfg(unix)]
    use std::fs;
    #[cfg(unix)]
    use std::process::Command;
    #[cfg(unix)]
    use std::thread;
    #[cfg(unix)]
    use std::time::{Duration, Instant};

    #[cfg(unix)]
    use crate::test_env::{EnvVarGuard, TestRepoBuilder, lock_env};

    use std::path::Path;

    use super::*;

    #[derive(Default)]
    struct RecordingControl {
        output: Vec<u8>,
    }

    impl jig_execution::ExecutionObserver for RecordingControl {
        fn event(&mut self, event: jig_execution::ExecutionEvent<'_>) {
            if let jig_execution::ExecutionEvent::Output { bytes, .. } = event {
                self.output.extend_from_slice(bytes);
            }
        }
    }

    impl jig_execution::ExecutionCancellation for RecordingControl {}

    struct CancelledControl;

    impl jig_execution::ExecutionObserver for CancelledControl {}

    impl jig_execution::ExecutionCancellation for CancelledControl {
        fn cancelled(&self) -> bool {
            true
        }
    }

    #[test]
    fn codex_refine_approval_policy_is_a_top_level_codex_arg() {
        let mut request = CodexExecRequest {
            root: Path::new("/tmp/repo"),
            codex_home: Some(Path::new("/tmp/codex-home")),
            model: Some("gpt-x"),
            approval_policy: Some("never"),
            sandbox: Some("workspace-write"),
            ephemeral: true,
            extra_args: Vec::new(),
            output_schema: None,
            transcript_overflow_policy: ProcessOutputOverflowPolicy::Truncate,
            prompt: "fix this",
            run: WorkerRunLabel {
                purpose: "work_refine",
                workflow_id: None,
                item_key: None,
            },
            phase: Some(WorkerPhase {
                label: "test worker",
                position: PhasePosition::single(),
            }),
        };
        assert_eq!(
            request.transcript_overflow_policy,
            ProcessOutputOverflowPolicy::Truncate,
            "refinement edits are authoritative; its transcript is diagnostic"
        );
        let command = build_codex_command("codex", &request, None, Path::new("/tmp/codex-output"));
        let args = command
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect::<Vec<_>>();

        assert_eq!(
            args,
            [
                "--ask-for-approval",
                "never",
                "exec",
                "--sandbox",
                "workspace-write",
                "--ephemeral",
                "--model",
                "gpt-x",
                "-o",
                "/tmp/codex-output",
                "-",
            ]
        );
        assert!(command.get_envs().any(|(key, value)| {
            key == jig_agents::codex::CODEX_HOME_ENV && value == Some(OsStr::new("/tmp/codex-home"))
        }));

        request.codex_home = None;
        let inherited_command =
            build_codex_command("codex", &request, None, Path::new("/tmp/codex-output"));
        assert!(
            inherited_command
                .get_envs()
                .all(|(key, _)| key != jig_agents::codex::CODEX_HOME_ENV)
        );
    }

    #[test]
    fn codex_timeout_override_uses_the_validated_command_timeout_range() {
        assert_eq!(parse_codex_timeout("1").unwrap().as_secs(), 1);
        assert_eq!(
            parse_codex_timeout(&MAX_COMMAND_TIMEOUT_SECONDS.to_string())
                .unwrap()
                .as_secs(),
            MAX_COMMAND_TIMEOUT_SECONDS
        );
        for value in [
            "0".to_string(),
            (MAX_COMMAND_TIMEOUT_SECONDS + 1).to_string(),
        ] {
            let error = parse_codex_timeout(&value).unwrap_err().to_string();
            assert!(error.contains("must be between 1 and 86400"), "{error}");
        }
    }

    #[test]
    fn worker_last_message_file_is_size_bounded() {
        let output = NamedTempFile::new().unwrap();
        output
            .as_file()
            .set_len((EXECUTION_OUTPUT_CAPTURE_LIMIT + 1) as u64)
            .unwrap();

        let error = read_worker_output_file(output.path())
            .unwrap_err()
            .to_string();

        assert!(
            error.contains(&format!(
                "exceeded the {EXECUTION_OUTPUT_CAPTURE_LIMIT} byte capture limit"
            )),
            "{error}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn worker_supervision_delivers_stdin_and_observes_output() {
        let mut command = Command::new("/bin/sh");
        command.args(["-c", "cat"]);
        let mut control = RecordingControl::default();

        let output = run_worker_command(
            &mut command,
            Some("prompt through a file"),
            CommandTimeout::from_seconds(1).unwrap(),
            "test worker",
            ProcessOutputOverflowPolicy::Error,
            None,
            &mut control,
        )
        .unwrap();

        assert!(output.output.status.success());
        assert_eq!(output.output.stdout, b"prompt through a file");
        assert_eq!(control.output, b"prompt through a file");
    }

    #[cfg(unix)]
    #[test]
    fn worker_supervision_preserves_cancellation_with_result_monitor() {
        let mut command = Command::new("/bin/sh");
        command.args(["-c", "sleep 30"]);
        let output_file = NamedTempFile::new().unwrap();

        let error = run_worker_command(
            &mut command,
            None,
            CommandTimeout::from_seconds(5).unwrap(),
            "test worker",
            ProcessOutputOverflowPolicy::Error,
            Some(output_file.path()),
            &mut CancelledControl,
        )
        .unwrap_err();

        assert!(matches!(error, ExecutionCommandError::CancelledBeforeStart));
    }

    #[cfg(unix)]
    #[test]
    fn worker_result_file_inspection_obeys_its_schedule() {
        let temp = tempfile::tempdir().unwrap();
        let output_path = temp.path().join("authoritative-output");
        fs::write(&output_path, b"result").unwrap();
        let mut control = jig_execution::NoopExecutionObserver;
        let mut observer = WorkerProcessObserver::new(
            ProcessExecutionObserver::new(&mut control, "test worker"),
            Some(&output_path),
        );

        assert!(!observer.cancelled(), "the first inspection is immediate");
        fs::remove_file(&output_path).unwrap();
        observer.last_result_file_inspection = Some(Instant::now());
        assert!(
            !observer.cancelled(),
            "metadata must not be inspected again before the interval"
        );

        observer.last_result_file_inspection = Some(
            Instant::now()
                .checked_sub(WORKER_RESULT_FILE_INSPECTION_INTERVAL)
                .unwrap(),
        );
        assert!(
            observer.cancelled(),
            "the missing file is detected once due"
        );
        assert!(matches!(
            observer.take_result_file_failure(),
            Some(WorkerResultFileFailure::Inspection(message))
                if message.contains("Failed to inspect Codex output file")
        ));
    }

    #[cfg(unix)]
    #[test]
    fn worker_supervision_rejects_output_beyond_the_capture_limit() {
        let mut command = Command::new("yes");

        let error = run_worker_command(
            &mut command,
            None,
            CommandTimeout::from_seconds(5).unwrap(),
            "test worker",
            ProcessOutputOverflowPolicy::Error,
            None,
            &mut jig_execution::NoopExecutionObserver,
        )
        .unwrap_err()
        .to_string();

        assert!(
            error.contains(&format!(
                "exceeded the {EXECUTION_OUTPUT_CAPTURE_LIMIT} byte capture limit"
            )),
            "{error}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn diagnostic_worker_allows_truncated_provider_transcript() {
        let mut command = Command::new("/bin/sh");
        command.args([
            "-c",
            &format!("head -c {} /dev/zero", EXECUTION_OUTPUT_CAPTURE_LIMIT + 1),
        ]);

        let output = run_worker_command(
            &mut command,
            None,
            CommandTimeout::from_seconds(5).unwrap(),
            "test worker",
            ProcessOutputOverflowPolicy::Truncate,
            None,
            &mut jig_execution::NoopExecutionObserver,
        )
        .unwrap();

        assert!(output.output.status.success());
        assert_eq!(output.output.stdout.len(), EXECUTION_OUTPUT_CAPTURE_LIMIT);
        assert!(output.provider_stdout_truncated);
        assert!(!output.provider_stderr_truncated);
    }

    #[cfg(unix)]
    #[test]
    fn worker_result_file_limit_terminates_process_group_while_running() {
        let temp = tempfile::tempdir().unwrap();
        let marker = temp.path().join("escaped-result-writer");
        let output_file = NamedTempFile::new().unwrap();
        let script = format!(
            "(sleep 4; printf leaked > \"$1\") & head -c {} /dev/zero > \"$2\"; wait",
            EXECUTION_OUTPUT_CAPTURE_LIMIT + 1
        );
        let mut command = Command::new("/bin/sh");
        command
            .arg("-c")
            .arg(script)
            .arg("worker")
            .arg(&marker)
            .arg(output_file.path());

        let started = Instant::now();
        let error = run_worker_command(
            &mut command,
            None,
            CommandTimeout::from_seconds(5).unwrap(),
            "test worker",
            ProcessOutputOverflowPolicy::Truncate,
            Some(output_file.path()),
            &mut jig_execution::NoopExecutionObserver,
        )
        .unwrap_err()
        .to_string();

        assert!(
            error.contains(&format!(
                "Codex last-message output exceeded the {EXECUTION_OUTPUT_CAPTURE_LIMIT} byte capture limit"
            )),
            "{error}"
        );
        assert!(
            started.elapsed() < Duration::from_secs(3),
            "result-file overflow was checked only after the worker exited"
        );
        let leak_deadline = Duration::from_millis(4250);
        if let Some(remaining) = leak_deadline.checked_sub(started.elapsed()) {
            thread::sleep(remaining);
        }
        assert!(
            !marker.exists(),
            "result-file overflow killed the child process but left its process group running"
        );
    }

    #[cfg(unix)]
    #[test]
    fn schema_less_worker_uses_last_message_file_as_authoritative_output() {
        use std::os::unix::fs::PermissionsExt;

        let _guard = lock_env();
        let temp = tempfile::tempdir().unwrap();
        TestRepoBuilder::new(temp.path())
            .required_commands(Vec::<String>::new())
            .write();
        let script = temp.path().join("codex-stub.sh");
        fs::write(
            &script,
            r#"#!/bin/sh
out=""
prev=""
for arg in "$@"; do
  if [ "$prev" = "-o" ]; then
    out="$arg"
  fi
  prev="$arg"
done
printf 'diagnostic transcript\n'
printf 'authoritative result\n' > "$out"
"#,
        )
        .unwrap();
        let mut permissions = fs::metadata(&script).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&script, permissions).unwrap();
        let _codex = EnvVarGuard::set("JIG_CODEX_BIN", &script);
        let ctx = RepoContext::load_from(temp.path()).unwrap();

        let outcome = run_codex_exec(
            &ctx,
            CodexExecRequest {
                root: temp.path(),
                codex_home: None,
                model: None,
                approval_policy: Some("never"),
                sandbox: Some("workspace-write"),
                ephemeral: true,
                extra_args: Vec::new(),
                output_schema: None,
                transcript_overflow_policy: ProcessOutputOverflowPolicy::Truncate,
                prompt: "example prompt",
                run: WorkerRunLabel {
                    purpose: "test",
                    workflow_id: None,
                    item_key: None,
                },
                phase: None,
            },
            &mut jig_execution::NoopExecutionObserver,
        )
        .unwrap();
        let CodexExecOutcome::Completed(output) = outcome else {
            panic!("worker unexpectedly cancelled");
        };

        assert_eq!(output.authoritative_stdout(), b"authoritative result\n");
        assert_eq!(output.provider_stdout(), "diagnostic transcript\n");
        let evidence = output.evidence();
        assert_eq!(evidence["kind"], "worker_run");
        assert_eq!(evidence["status"], "passed");
        assert_eq!(evidence["exit_status"], 0);
        assert_eq!(evidence["provider_stdout_preview"], "diagnostic transcript\n");
        assert!(!temp.path().join(".agent/state/receipts.jsonl").exists());
    }

    #[cfg(unix)]
    #[test]
    fn worker_timeout_kills_process_group() {
        use std::os::unix::fs::PermissionsExt;

        let _guard = lock_env();
        let temp = tempfile::tempdir().unwrap();
        let marker = temp.path().join("escaped-grandchild");
        let script = temp.path().join("worker.sh");
        fs::write(
            &script,
            r#"#!/bin/sh
marker="$1"
(sh -c 'sleep 3; printf leaked > "$1"' sh "$marker") &
wait
"#,
        )
        .unwrap();
        let mut permissions = fs::metadata(&script).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&script, permissions).unwrap();
        let _timeout = EnvVarGuard::set(CODEX_TIMEOUT_ENV, "1");

        let mut command = Command::new(&script);
        command.arg(&marker);
        let error = run_worker_command(
            &mut command,
            None,
            CommandTimeout::from_seconds(1).unwrap(),
            "test worker",
            ProcessOutputOverflowPolicy::Error,
            None,
            &mut jig_execution::NoopExecutionObserver,
        )
        .unwrap_err()
        .to_string();

        assert!(error.contains("Worker process timed out after 1 seconds"));
        thread::sleep(Duration::from_millis(3500));
        assert!(
            !marker.exists(),
            "worker timeout killed the child process but left its process group running"
        );
    }
}
