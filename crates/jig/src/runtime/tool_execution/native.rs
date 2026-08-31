use super::*;

pub(in crate::runtime) fn run_native_tool_with_control(
    ctx: &RepoContext,
    tool_name: &str,
    target: Option<&TargetId>,
    args_value: &Value,
    timeout: Duration,
    cancelled: &dyn Fn() -> bool,
) -> Result<NativeToolOutput> {
    let deadline = std::time::Instant::now()
        .checked_add(timeout)
        .ok_or(jig_owned_process::OwnedProcessTreeError::TimedOut)?;
    check_native_control(deadline, cancelled)?;
    let output = match jig_features::native_tool_kind(tool_name)
        .ok_or_else(|| anyhow!("Unsupported native tool: {tool_name}"))?
    {
        NativeToolKind::ContractCheck => Ok(crate::policy::contract_check(ctx)),
        NativeToolKind::MigrationAdd => {
            let name = args_value
                .get(args::NAME)
                .and_then(Value::as_str)
                .ok_or_else(|| anyhow!("{} requires a name argument", tool::MIGRATION_ADD))?;
            crate::policy::migration_add(ctx, name)
        }
        NativeToolKind::SchemaCheck => {
            crate::policy::schema_check_with_control(ctx, target, timeout, cancelled)
        }
        _ => bail!("Unsupported native tool kind for {tool_name}"),
    }?;
    // Once an in-process tool returns, its effects may already be durable.
    // Completion is therefore authoritative; a late timeout observation must
    // not report that a completed mutation did not happen.
    Ok(bound_native_output(output))
}

fn check_native_control(deadline: std::time::Instant, cancelled: &dyn Fn() -> bool) -> Result<()> {
    if cancelled() {
        return Err(jig_owned_process::OwnedProcessTreeError::CancelledBeforeStart.into());
    }
    if std::time::Instant::now() >= deadline {
        return Err(jig_owned_process::OwnedProcessTreeError::TimedOut.into());
    }
    Ok(())
}

enum NativeToolRun {
    Completed(NativeToolOutput),
    CancelledBeforeStart,
    Cancelled,
}

fn run_native_tool(
    ctx: &RepoContext,
    operation: &str,
    target: Option<&TargetId>,
    timeout: Duration,
    args_value: &Value,
    observer: &mut dyn ExecutionControl,
) -> Result<NativeToolRun> {
    let output = match jig_features::native_tool_kind(operation)
        .ok_or_else(|| anyhow!("Unsupported native tool: {operation}"))?
    {
        NativeToolKind::ContractCheck => {
            Ok(NativeToolRun::Completed(crate::policy::contract_check(ctx)))
        }
        NativeToolKind::MigrationAdd => {
            let name = args_value
                .get(args::NAME)
                .and_then(Value::as_str)
                .ok_or_else(|| anyhow!("{} requires a name argument", tool::MIGRATION_ADD))?;
            crate::policy::migration_add(ctx, name).map(NativeToolRun::Completed)
        }
        NativeToolKind::SchemaCheck => {
            match crate::policy::schema_check_with_observer_and_timeout(
                ctx, target, timeout, observer,
            ) {
                Ok(output) => Ok(NativeToolRun::Completed(output)),
                Err(ExecutionCommandError::CancelledBeforeStart) => {
                    Ok(NativeToolRun::CancelledBeforeStart)
                }
                Err(ExecutionCommandError::Cancelled) => Ok(NativeToolRun::Cancelled),
                Err(ExecutionCommandError::Failed(error)) => Err(error),
            }
        }
        _ => bail!("Unsupported native tool kind for {operation}"),
    }?;
    Ok(match output {
        NativeToolRun::Completed(output) => NativeToolRun::Completed(bound_native_output(output)),
        NativeToolRun::CancelledBeforeStart => NativeToolRun::CancelledBeforeStart,
        NativeToolRun::Cancelled => NativeToolRun::Cancelled,
    })
}

pub(super) fn bound_native_output(mut output: NativeToolOutput) -> NativeToolOutput {
    let limits = jig_owned_process::ProcessOutputLimits::default();
    truncate_native_stream(&mut output.stdout, limits.stdout);
    truncate_native_stream(&mut output.stderr, limits.stderr);
    output
}

fn truncate_native_stream(value: &mut String, limit: usize) {
    if value.len() <= limit {
        return;
    }
    let mut end = limit;
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    value.truncate(end);
    value.push_str("\n[output truncated by Jig]\n");
}

pub(super) struct NativeToolInvocation<'a> {
    pub(super) tool_name: &'a str,
    pub(super) operation: &'a str,
    pub(super) target: Option<&'a TargetId>,
    pub(super) timeout_seconds: Option<u64>,
}

pub(super) fn execute_native_tool(
    ctx: &RepoContext,
    invocation: NativeToolInvocation<'_>,
    args: Value,
    plan_id: Option<String>,
    options: ManifestToolExecutionOptions,
    position: PhasePosition,
    observer: &mut dyn ExecutionControl,
) -> Result<ManifestToolExecutionOutcome> {
    let NativeToolInvocation {
        tool_name,
        operation,
        target,
        timeout_seconds,
    } = invocation;
    let started = now_ms();
    if observer.cancelled() {
        return cancelled_native_tool_outcome(
            ctx,
            CancelledNativeToolRequest {
                tool_name,
                args,
                plan_id,
                options,
                started,
                before_start: true,
            },
            observer,
        );
    }
    let phase = ExecutionPhase::start(observer, tool_name, position);
    let timeout = timeout_seconds
        .map(Duration::from_secs)
        .unwrap_or_else(|| ctx.command_timeout().duration());
    let output = run_native_tool(ctx, operation, target, timeout, &args, observer);
    phase.finish(
        observer,
        output.as_ref().is_ok_and(
            |output| matches!(output, NativeToolRun::Completed(output) if output.exit_status == 0),
        ),
    );
    let output = match output? {
        NativeToolRun::Completed(output) => output,
        NativeToolRun::CancelledBeforeStart => {
            return cancelled_native_tool_outcome(
                ctx,
                CancelledNativeToolRequest {
                    tool_name,
                    args,
                    plan_id,
                    options,
                    started,
                    before_start: true,
                },
                observer,
            );
        }
        NativeToolRun::Cancelled => {
            return cancelled_native_tool_outcome(
                ctx,
                CancelledNativeToolRequest {
                    tool_name,
                    args,
                    plan_id,
                    options,
                    started,
                    before_start: false,
                },
                observer,
            );
        }
    };
    let ended = now_ms();

    let receipt_result = maybe_record_receipt(
        ctx,
        options.record_receipt,
        ReceiptInput {
            tool_name,
            args: args.clone(),
            invoked_command_key: None,
            plan_id,
            started_at_ms: started,
            ended_at_ms: ended,
            exit_status: output.exit_status,
            stdout: &output.stdout,
            stderr: &output.stderr,
            evidence: None,
            session_override: None,
            collect_git_metadata: options.collect_git_metadata,
            collect_worktree_fingerprint: options.collect_worktree_fingerprint,
            worktree_fingerprint_override: None,
        },
        &|| observer.cancelled(),
    );

    let tool_failure = tool_failure_message(
        tool_name,
        None,
        output.exit_status,
        &output.stdout,
        &output.stderr,
    );
    let receipt_id =
        receipt_id_for_failure_mode(options.failure_mode, tool_failure, receipt_result)?;

    tool_response_value(ToolExecutionResponse {
        ok: true,
        tool: tool_name,
        command_key: None,
        args,
        result: ToolProcessResult {
            exit_status: output.exit_status,
            stdout: output.stdout,
            stderr: output.stderr,
        },
        receipt_id,
    })
    .map(ManifestToolExecutionOutcome::Completed)
}

struct CancelledNativeToolRequest<'a> {
    pub(super) tool_name: &'a str,
    args: Value,
    plan_id: Option<String>,
    options: ManifestToolExecutionOptions,
    started: u64,
    before_start: bool,
}

fn cancelled_native_tool_outcome(
    ctx: &RepoContext,
    request: CancelledNativeToolRequest<'_>,
    observer: &mut dyn ExecutionControl,
) -> Result<ManifestToolExecutionOutcome> {
    let CancelledNativeToolRequest {
        tool_name,
        args,
        plan_id,
        options,
        started,
        before_start,
    } = request;
    let message = if before_start {
        format!("Native tool {tool_name} was cancelled before it started")
    } else {
        format!("Native tool {tool_name} was cancelled")
    };
    let receipt_id = if before_start {
        None
    } else {
        let evidence = serde_json::json!({
            "kind": "supervised_command",
            "schema_version": 1,
            "status": "cancelled",
            "error": message,
        });
        maybe_record_receipt(
            ctx,
            options.record_receipt,
            ReceiptInput {
                tool_name,
                args: args.clone(),
                invoked_command_key: None,
                plan_id,
                started_at_ms: started,
                ended_at_ms: now_ms(),
                exit_status: 1,
                stdout: "",
                stderr: &message,
                evidence: Some(evidence),
                session_override: None,
                collect_git_metadata: options.collect_git_metadata,
                collect_worktree_fingerprint: options.collect_worktree_fingerprint,
                worktree_fingerprint_override: None,
            },
            &|| observer.cancelled(),
        )?
    };
    let response = tool_response_value(ToolExecutionResponse {
        ok: true,
        tool: tool_name,
        command_key: None,
        args,
        result: ToolProcessResult {
            exit_status: 1,
            stdout: String::new(),
            stderr: message,
        },
        receipt_id,
    })?;
    Ok(ManifestToolExecutionOutcome::Cancelled(response))
}

fn receipt_id_or_preserve_tool_error(
    tool_failure: Option<String>,
    receipt_result: Result<Option<String>>,
) -> Result<Option<String>> {
    if let Some(tool_failure) = tool_failure {
        match receipt_result {
            Ok(Some(receipt_id)) => bail!("{tool_failure}\nreceipt: {receipt_id}"),
            Ok(None) => bail!("{tool_failure}"),
            Err(receipt_error) => {
                bail!("{tool_failure}\nreceipt recording also failed:\n{receipt_error:#}")
            }
        }
    } else {
        receipt_result
    }
}

fn receipt_id_or_preserve_receipt_recording_context(
    tool_failure: Option<String>,
    receipt_result: Result<Option<String>>,
) -> Result<Option<String>> {
    match (tool_failure, receipt_result) {
        (Some(tool_failure), Err(receipt_error)) => {
            bail!("{tool_failure}\nreceipt recording also failed:\n{receipt_error:#}")
        }
        (_, receipt_result) => receipt_result,
    }
}

pub(super) fn receipt_id_for_failure_mode(
    failure_mode: ToolFailureMode,
    tool_failure: Option<String>,
    receipt_result: Result<Option<String>>,
) -> Result<Option<String>> {
    match failure_mode {
        ToolFailureMode::FailFast => {
            receipt_id_or_preserve_tool_error(tool_failure, receipt_result)
        }
        ToolFailureMode::CollectResult => {
            receipt_id_or_preserve_receipt_recording_context(tool_failure, receipt_result)
        }
    }
}
