use super::*;

pub(super) struct TargetExecutionControl<'a> {
    started: Instant,
    timeout: Duration,
    run_control: &'a mut dyn RepositoryRunControl,
    poll_failure: Mutex<Option<String>>,
}

impl<'a> TargetExecutionControl<'a> {
    pub(super) fn new(
        ctx: &RepoContext,
        planned: &PlannedTarget,
        run_control: &'a mut dyn RepositoryRunControl,
    ) -> Self {
        let timeout = planned
            .timeout_seconds
            .map(Duration::from_secs)
            .unwrap_or_else(|| ctx.command_timeout().duration());
        Self {
            started: Instant::now(),
            timeout,
            run_control,
            poll_failure: Mutex::new(None),
        }
    }

    pub(super) fn remaining(&self) -> std::result::Result<Duration, TargetStop> {
        match self.poll_cancelled() {
            Ok(true) => return Err(TargetStop::Cancelled),
            Ok(false) => {}
            Err(message) => return Err(TargetStop::Blocked(message)),
        }
        let remaining = self.timeout.saturating_sub(self.started.elapsed());
        if remaining.is_zero() {
            Err(TargetStop::TimedOut)
        } else {
            Ok(remaining)
        }
    }

    pub(super) fn is_cancelled(&self) -> bool {
        self.poll_cancelled().unwrap_or(true)
    }

    pub(super) fn poll_cancelled(&self) -> std::result::Result<bool, String> {
        if let Some(message) = self.poll_failure() {
            return Err(message);
        }
        match self.run_control.cancelled() {
            Ok(cancelled) => Ok(cancelled),
            Err(error) => {
                let message = format!("cancellation state could not be inspected: {error:#}");
                *self
                    .poll_failure
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(message.clone());
                Err(message)
            }
        }
    }

    pub(super) fn poll_failure(&self) -> Option<String> {
        self.poll_failure
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    pub(super) fn enforce_poll_health(&self, mut capture: TargetCapture) -> TargetCapture {
        let Some(message) = self.poll_failure() else {
            return capture;
        };
        capture.stderr.push_str(&format!("{message}\n"));
        capture.findings.push(finding(message, "cancellation"));
        if matches!(
            capture.conclusion,
            RunConclusion::Success | RunConclusion::Cancelled
        ) {
            capture.conclusion = RunConclusion::Blocked;
            capture.receipt_exit_status = capture.receipt_exit_status.max(1);
        }
        capture
    }
}

impl ExecutionObserver for TargetExecutionControl<'_> {
    fn event(&mut self, event: ExecutionEvent<'_>) {
        self.run_control.event(event);
    }

    fn flush(&mut self) -> Result<()> {
        self.run_control.flush()
    }
}

impl ExecutionCancellation for TargetExecutionControl<'_> {
    fn cancelled(&self) -> bool {
        self.is_cancelled()
    }
}

pub(super) enum TargetStop {
    Cancelled,
    TimedOut,
    Blocked(String),
}

pub(super) fn stopped_before_start(planned: &PlannedTarget, stop: TargetStop) -> TargetCapture {
    match stop {
        TargetStop::Cancelled => TargetCapture::not_started(
            RunConclusion::Cancelled,
            format!("target '{}' was cancelled", planned.target),
        ),
        TargetStop::TimedOut => TargetCapture::not_started(
            RunConclusion::TimedOut,
            format!("target '{}' timed out", planned.target),
        ),
        TargetStop::Blocked(message) => TargetCapture::blocked(format!(
            "target '{}' could not start because {message}",
            planned.target
        )),
    }
}

pub(super) fn run_command_target(
    ctx: &RepoContext,
    planned: &PlannedTarget,
    command_key: &str,
    working_directory: Option<&str>,
    environment: &BTreeMap<String, String>,
    control: &mut TargetExecutionControl<'_>,
) -> TargetCapture {
    let command_text = match ctx.command_for_key(command_key) {
        Ok(command) => command,
        Err(error) => {
            return TargetCapture::blocked(format!(
                "command runner '{command_key}' for target '{}' is unavailable: {error:#}",
                planned.target
            ))
            .with_command_key(command_key);
        }
    };
    let working_directory =
        match resolve_repository_working_directory(ctx.root(), working_directory) {
            Ok(path) => path,
            Err(error) => {
                return TargetCapture::blocked(format!(
                    "target '{}' has an invalid working directory: {error:#}",
                    planned.target
                ))
                .with_command_key(command_key);
            }
        };
    if let Err(error) = validate_runner_environment(environment) {
        return TargetCapture::blocked(format!(
            "target '{}' has an invalid runner environment: {error:#}",
            planned.target
        ))
        .with_command_key(command_key);
    }

    // Runner commands and their environment are checked-in execution
    // authority, equivalent in trust to a repository shell script. Preserve
    // the caller's ordinary environment and allow reviewed overrides such as
    // PATH; Jig-owned native probes use narrower scrubbed environments.
    let mut command = Command::new("bash");
    command
        .current_dir(working_directory)
        .arg("-c")
        .arg(command_text)
        .envs(environment);
    let timeout = match control.remaining() {
        Ok(timeout) => timeout,
        Err(conclusion) => {
            return stopped_before_start(planned, conclusion).with_command_key(command_key);
        }
    };
    let label = format!(
        "Command runner '{command_key}' for target '{}'",
        planned.target
    );
    match run_supervised_execution_command(
        &mut command,
        timeout,
        ctx.command_output_limit(),
        &label,
        control,
    ) {
        Ok(output) => TargetCapture::from_process(
            output.status.code().unwrap_or(1),
            String::from_utf8_lossy(&output.stdout).into_owned(),
            String::from_utf8_lossy(&output.stderr).into_owned(),
            planned.result_parser,
        )
        .with_command_key(command_key),
        Err(SupervisedExecutionError::TimedOut) => TargetCapture::stopped_after_start(
            RunConclusion::TimedOut,
            format!(
                "target '{}' exceeded its {timeout:?} timeout",
                planned.target
            ),
        )
        .with_command_key(command_key),
        Err(SupervisedExecutionError::CancelledBeforeStart) => TargetCapture::not_started(
            RunConclusion::Cancelled,
            format!("target '{}' was cancelled", planned.target),
        )
        .with_command_key(command_key),
        Err(SupervisedExecutionError::Cancelled) => TargetCapture::stopped_after_start(
            RunConclusion::Cancelled,
            format!("target '{}' was cancelled", planned.target),
        )
        .with_command_key(command_key),
        Err(SupervisedExecutionError::OutputLimitExceeded {
            stream,
            stdout,
            stderr,
        }) => TargetCapture::failed_with_output(
            format!(
                "command runner '{command_key}' for target '{}' exceeded the {} byte {stream} capture limit",
                planned.target,
                ctx.command_output_limit().bytes()
            ),
            "execution_policy",
            stdout,
            stderr,
        )
        .with_command_key(command_key),
        Err(SupervisedExecutionError::Failed {
            error,
            process_started,
        }) => {
            TargetCapture::blocked(format!(
                "command runner '{command_key}' for target '{}' failed: {error:#}",
                planned.target
            ))
            .with_maybe_executed(process_started)
            .with_command_key(command_key)
        }
    }
}
