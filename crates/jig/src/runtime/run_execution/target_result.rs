use super::*;

pub(super) struct TargetFinisher<'a> {
    pub(super) ctx: &'a RepoContext,
    pub(super) catalog: &'a RepositoryCatalog,
    pub(super) run: &'a crate::state::DurableRun,
    pub(super) work_plan_id: Option<&'a str>,
    pub(super) record_receipts: bool,
}

pub(super) struct CompletedTargetCapture {
    pub(super) started_at_ms: Option<u64>,
    pub(super) ended_at_ms: u64,
    pub(super) capture: TargetCapture,
}

impl CompletedTargetCapture {
    pub(super) fn now(started_at_ms: Option<u64>, capture: TargetCapture) -> Self {
        Self {
            started_at_ms,
            ended_at_ms: now_ms(),
            capture,
        }
    }

    pub(super) fn was_started(&self) -> bool {
        self.started_at_ms.is_some()
    }

    pub(super) fn succeeded(&self) -> bool {
        self.capture.conclusion == RunConclusion::Success
    }

    pub(super) fn map_capture(mut self, map: impl FnOnce(TargetCapture) -> TargetCapture) -> Self {
        self.capture = map(self.capture);
        self
    }
}

impl TargetFinisher<'_> {
    pub(super) fn finish(
        &self,
        planned: &PlannedTarget,
        completed: CompletedTargetCapture,
        worktree_fingerprint: std::result::Result<String, String>,
    ) -> Result<(TargetRunResult, Option<Value>)> {
        let CompletedTargetCapture {
            started_at_ms,
            ended_at_ms,
            capture,
        } = completed;
        let tool_name = capture.alias.as_deref().unwrap_or(GENERIC_TARGET_TOOL);
        let input_digest = match &worktree_fingerprint {
            Ok(fingerprint) => target_input_digest(self.catalog, &planned.target, fingerprint)?,
            Err(_) => planned.input_digest.clone(),
        };
        let receipt_id = self
            .record_receipts
            .then(|| {
                record_target_receipt(
                    self.ctx,
                    ReceiptInput {
                        tool_name,
                        args: json!({
                            "run_id": self.run.result.run_id,
                            "target": planned.target,
                        }),
                        invoked_command_key: capture.command_key.clone(),
                        plan_id: self.work_plan_id.map(str::to_owned),
                        started_at_ms: started_at_ms.unwrap_or(ended_at_ms),
                        ended_at_ms,
                        exit_status: capture.receipt_exit_status,
                        stdout: &capture.stdout,
                        stderr: &capture.stderr,
                        evidence: capture.native_evidence.clone(),
                        session_override: None,
                        collect_git_metadata: true,
                        collect_worktree_fingerprint: false,
                        worktree_fingerprint_override: Some(worktree_fingerprint),
                    },
                    TargetReceiptMetadata {
                        run_id: self.run.result.run_id.clone(),
                        target: planned.target.clone(),
                        config_digest: self.run.plan.config_digest.clone(),
                        input_digest: input_digest.clone(),
                        findings: capture.findings.clone(),
                        finding_count: capture.finding_count,
                        findings_truncated: capture.findings_truncated,
                        findings_digest: capture.findings_digest.clone(),
                        evaluated_at_ms: capture.evaluated_at_ms,
                        valid_until_ms: capture.valid_until_ms,
                    },
                )
            })
            .transpose()?;

        let mut result = TargetRunResult::queued(
            planned.target.clone(),
            self.run.plan.config_digest.clone(),
            input_digest,
        );
        result.status = RunStatus::Completed;
        result.conclusion = Some(capture.conclusion);
        result.started_at_ms = started_at_ms;
        result.ended_at_ms = Some(ended_at_ms);
        result.exit_code = capture.exit_code;
        result.receipt_id.clone_from(&receipt_id);
        result.findings.clone_from(&capture.findings);
        result.finding_count = capture.finding_count;
        result.findings_truncated = capture.findings_truncated;
        result.findings_digest.clone_from(&capture.findings_digest);
        result.native_evidence.clone_from(&capture.native_evidence);
        result.evaluated_at_ms = capture.evaluated_at_ms;
        result.valid_until_ms = capture.valid_until_ms;

        let compatibility = started_at_ms.map(|_| {
            let alias = self
                .catalog
                .aliases_for_target(&planned.target)
                .first()
                .cloned();
            json!({
                "target": planned.target,
                "tool": alias,
                "response": {
                    "ok": capture.conclusion == RunConclusion::Success,
                    "tool": alias.as_deref().unwrap_or(GENERIC_TARGET_TOOL),
                    "command_key": capture.command_key,
                    "args": {},
                    "result": {
                        "exit_status": capture.receipt_exit_status,
                        "stdout": capture.stdout,
                        "stderr": capture.stderr,
                        "finding_count": capture.finding_count,
                        "findings_truncated": capture.findings_truncated,
                        "findings_digest": capture.findings_digest,
                        "evaluated_at_ms": capture.evaluated_at_ms,
                        "valid_until_ms": capture.valid_until_ms,
                    },
                    "receipt_id": receipt_id,
                },
            })
        });
        Ok((result, compatibility))
    }
}

pub(super) fn aggregate_conclusion(
    conclusions: impl Iterator<Item = RunConclusion>,
) -> RunConclusion {
    conclusions
        .max_by_key(|conclusion| match conclusion {
            RunConclusion::Failure => 5,
            RunConclusion::TimedOut => 4,
            RunConclusion::Blocked => 3,
            RunConclusion::Cancelled => 2,
            RunConclusion::Skipped => 1,
            RunConclusion::Success => 0,
        })
        // An affected plan with no relevant targets is a verified no-op.
        .unwrap_or(RunConclusion::Success)
}

pub(super) struct TargetCapture {
    pub(super) conclusion: RunConclusion,
    pub(super) exit_code: Option<i32>,
    pub(super) receipt_exit_status: i32,
    pub(super) stdout: String,
    pub(super) stderr: String,
    pub(super) findings: Vec<Finding>,
    pub(super) command_key: Option<String>,
    pub(super) alias: Option<String>,
    pub(super) may_have_executed: bool,
    pub(super) finding_count: Option<u64>,
    pub(super) findings_truncated: bool,
    pub(super) findings_digest: Option<String>,
    pub(super) native_evidence: Option<Value>,
    pub(super) evaluated_at_ms: Option<u64>,
    pub(super) valid_until_ms: Option<u64>,
}

impl TargetCapture {
    pub(super) fn from_process(
        exit_status: i32,
        stdout: String,
        stderr: String,
        parser: ResultParser,
    ) -> Self {
        let ParsedFindings {
            mut findings,
            succeeded: findings_parse_succeeded,
        } = parse_findings(parser, &stdout);
        let has_error_finding = findings
            .iter()
            .any(|finding| finding.severity == FindingSeverity::Error);
        let conclusion = if exit_status == 0 && findings_parse_succeeded && !has_error_finding {
            RunConclusion::Success
        } else {
            if exit_status != 0 {
                findings.push(finding(
                    format!("target process exited with status {exit_status}"),
                    "exit_code",
                ));
            }
            RunConclusion::Failure
        };
        let receipt_exit_status = if conclusion == RunConclusion::Success {
            exit_status
        } else {
            exit_status.max(1)
        };
        Self {
            conclusion,
            exit_code: Some(exit_status),
            receipt_exit_status,
            stdout,
            stderr,
            findings,
            command_key: None,
            alias: None,
            may_have_executed: true,
            finding_count: None,
            findings_truncated: false,
            findings_digest: None,
            native_evidence: None,
            evaluated_at_ms: None,
            valid_until_ms: None,
        }
    }

    pub(super) fn from_native_action(result: jig_contract::NativeActionResult) -> Self {
        let receipt_exit_status = if result.conclusion == RunConclusion::Success {
            0
        } else {
            1
        };
        Self {
            conclusion: result.conclusion,
            exit_code: None,
            receipt_exit_status,
            stdout: result.human_output,
            stderr: String::new(),
            findings: result.findings,
            command_key: None,
            alias: None,
            may_have_executed: true,
            finding_count: Some(result.finding_count),
            findings_truncated: result.findings_truncated,
            findings_digest: Some(result.findings_digest),
            native_evidence: result.evidence,
            evaluated_at_ms: Some(result.evaluated_at_ms),
            valid_until_ms: result.valid_until_ms,
        }
    }

    pub(super) fn not_started(conclusion: RunConclusion, message: impl Into<String>) -> Self {
        let message = message.into();
        Self {
            conclusion,
            exit_code: None,
            receipt_exit_status: 1,
            stdout: String::new(),
            stderr: message.clone(),
            findings: vec![finding(message, "jig")],
            command_key: None,
            alias: None,
            may_have_executed: false,
            finding_count: None,
            findings_truncated: false,
            findings_digest: None,
            native_evidence: None,
            evaluated_at_ms: None,
            valid_until_ms: None,
        }
    }

    pub(super) fn stopped_after_start(
        conclusion: RunConclusion,
        message: impl Into<String>,
    ) -> Self {
        Self::not_started(conclusion, message).with_maybe_executed(true)
    }

    pub(super) fn blocked(message: impl Into<String>) -> Self {
        Self::not_started(RunConclusion::Blocked, message)
    }

    pub(super) fn failed_with_output(
        message: impl Into<String>,
        source: &str,
        stdout: Vec<u8>,
        stderr: Vec<u8>,
    ) -> Self {
        let message = message.into();
        let mut stderr = String::from_utf8_lossy(&stderr).into_owned();
        if !stderr.is_empty() && !stderr.ends_with('\n') {
            stderr.push('\n');
        }
        stderr.push_str(&message);
        Self {
            conclusion: RunConclusion::Failure,
            exit_code: None,
            receipt_exit_status: 1,
            stdout: String::from_utf8_lossy(&stdout).into_owned(),
            stderr,
            findings: vec![finding(message, source)],
            command_key: None,
            alias: None,
            may_have_executed: true,
            finding_count: None,
            findings_truncated: false,
            findings_digest: None,
            native_evidence: None,
            evaluated_at_ms: None,
            valid_until_ms: None,
        }
    }

    pub(super) fn with_maybe_executed(mut self, may_have_executed: bool) -> Self {
        self.may_have_executed = may_have_executed;
        self
    }

    pub(super) fn with_command_key(mut self, command_key: impl Into<String>) -> Self {
        self.command_key = Some(command_key.into());
        self
    }

    pub(super) fn with_alias(mut self, alias: Option<String>) -> Self {
        self.alias = alias;
        self
    }
}

pub(super) struct ParsedFindings {
    pub(super) findings: Vec<Finding>,
    pub(super) succeeded: bool,
}

pub(super) fn parse_findings(parser: ResultParser, stdout: &str) -> ParsedFindings {
    if parser == ResultParser::ExitCode {
        return ParsedFindings {
            findings: Vec::new(),
            succeeded: true,
        };
    }
    let mut findings = Vec::new();
    let mut succeeded = true;
    for line in stdout.lines().filter(|line| !line.trim().is_empty()) {
        match serde_json::from_str::<Finding>(line) {
            Ok(parsed) => findings.push(parsed),
            Err(error) => {
                succeeded = false;
                findings.push(finding(
                    format!("result parser rejected JSON line: {error}"),
                    "result_parser",
                ));
            }
        }
    }
    ParsedFindings {
        findings,
        succeeded,
    }
}

pub(super) fn finding(message: impl Into<String>, source: &str) -> Finding {
    let mut finding = Finding::new(FindingSeverity::Error, message);
    finding.source = Some(source.into());
    finding
}
