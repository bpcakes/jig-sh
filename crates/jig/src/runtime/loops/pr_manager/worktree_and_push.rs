fn prepare_worktree(
    ctx: &RepoContext,
    workflow: &ResolvedWorkflow,
    item: &PrWorkItem,
    worktree_reservation: Option<&OccurrenceWorktreeReservation>,
    observer: &mut dyn ExecutionControl,
) -> std::result::Result<PreparedPrWorktree, PrWorktreePreparationError> {
    let worktree = pr_worktree_path(ctx, workflow, item);
    let existed_before_preflight = match inspect_managed_directory(
        ctx.root(),
        &worktree,
        "PR repair worktree",
    ) {
        Ok(exists) => exists,
        Err(error) => {
            return Err(PrWorktreePreparationError {
                source: PrRepairStepError::failed(anyhow!(error).context(format!(
                    "Failed to inspect PR repair worktree {}",
                    worktree.display()
                ))),
                worktree: Some(PreparedPrWorktree::Retained(worktree)),
            });
        }
    };
    let preflight = (|| -> PrRepairStepResult<()> {
        if !is_git_object_id(&item.head_sha) {
            return Err(PrRepairStepError::failed(anyhow!(
                "GitHub PR snapshot did not include a valid head object ID for PR #{}",
                item.pr_number
            )));
        }
        let parent = worktree
            .parent()
            .ok_or_else(|| anyhow!("Worktree path has no parent: {}", worktree.display()))?;
        ensure_managed_directory(ctx.root(), parent, "PR repair worktree parent")?;

        let head_ref = remote_branch_ref(&item.head_ref);
        git_checked(ctx, ctx.root(), ["fetch", "origin", &head_ref], observer)?;
        let expected_head = format!("{}^{{commit}}", item.head_sha);
        git_checked(
            ctx,
            ctx.root(),
            ["cat-file", "-e", &expected_head],
            observer,
        )?;
        require_remote_head(ctx, ctx.root(), &head_ref, &item.head_sha, observer)
    })();
    if let Err(error) = preflight {
        return Err(PrWorktreePreparationError {
            source: error,
            worktree: existed_before_preflight
                .then_some(PreparedPrWorktree::Retained(worktree)),
        });
    }
    if let Some(reservation) = worktree_reservation
        && let Err(error) = reservation.reserve(&worktree)
    {
        return Err(PrWorktreePreparationError {
            source: PrRepairStepError::failed(
                error.context("Failed to reserve the PR repair worktree in occurrence state"),
            ),
            worktree: existed_before_preflight
                .then_some(PreparedPrWorktree::Retained(worktree)),
        });
    }
    let mut created_by_current_attempt = !existed_before_preflight;
    let result = (|| {
        if existed_before_preflight {
            if !pr_worktree_is_registered(ctx, &worktree, observer)? {
                return Err(PrRepairStepError::failed(anyhow!(
                    "Refusing to reuse untrusted PR repair directory {}: it is not an authenticated registered worktree",
                    worktree.display()
                )));
            }
            // A registered checkout authenticates the path, but it is not a safe cache
            // boundary: ordinary `git clean` intentionally retains ignored files and
            // nested repositories. Recreate it so every worker starts from one tree.
            git_checked(
                ctx,
                ctx.root(),
                vec![
                    OsString::from("worktree"),
                    OsString::from("remove"),
                    OsString::from("--force"),
                    worktree.as_os_str().to_os_string(),
                ],
                observer,
            )?;
            created_by_current_attempt = true;
        }
        git_checked(
            ctx,
            ctx.root(),
            vec![
                OsString::from("worktree"),
                OsString::from("add"),
                OsString::from("--detach"),
                worktree.as_os_str().to_os_string(),
                OsString::from(&item.head_sha),
            ],
            observer,
        )?;
        Ok(())
    })();
    match result {
        Ok(()) => Ok(if created_by_current_attempt {
            PreparedPrWorktree::Created(worktree)
        } else {
            PreparedPrWorktree::Retained(worktree)
        }),
        Err(error) => Err(PrWorktreePreparationError {
            source: error,
            worktree: Some(if created_by_current_attempt {
                PreparedPrWorktree::Created(worktree)
            } else {
                PreparedPrWorktree::Retained(worktree)
            }),
        }),
    }
}

#[derive(Clone, Debug)]
enum PreparedPrWorktree {
    Created(PathBuf),
    Retained(PathBuf),
}

impl PreparedPrWorktree {
    fn path(&self) -> &Path {
        match self {
            Self::Created(path) | Self::Retained(path) => path,
        }
    }

    const fn created_by_current_attempt(&self) -> bool {
        matches!(self, Self::Created(_))
    }
}

#[derive(Debug)]
struct PrWorktreePreparationError {
    source: PrRepairStepError,
    worktree: Option<PreparedPrWorktree>,
}

enum PrCleanupLease<'a> {
    Guard(&'a mut LeaseGuard),
    #[cfg(test)]
    AssumedHeld,
}

impl PrCleanupLease<'_> {
    fn refresh(&mut self) -> Result<()> {
        match self {
            Self::Guard(guard) => guard.refresh(),
            #[cfg(test)]
            Self::AssumedHeld => Ok(()),
        }
    }

    fn renewal_failed(&self) -> bool {
        match self {
            Self::Guard(guard) => guard.renewal_failed(),
            #[cfg(test)]
            Self::AssumedHeld => false,
        }
    }
}

struct PrWorktreeCleanup<'a> {
    ctx: &'a RepoContext,
    lease: PrCleanupLease<'a>,
    observer: NoopExecutionObserver,
}

impl<'a> PrWorktreeCleanup<'a> {
    fn new(ctx: &'a RepoContext, lease_guard: &'a mut LeaseGuard) -> Self {
        Self {
            ctx,
            lease: PrCleanupLease::Guard(lease_guard),
            observer: NoopExecutionObserver,
        }
    }

    #[cfg(test)]
    fn assuming_lease(ctx: &'a RepoContext) -> Self {
        Self {
            ctx,
            lease: PrCleanupLease::AssumedHeld,
            observer: NoopExecutionObserver,
        }
    }

    fn refresh(&mut self, operation: &str) -> Result<()> {
        self.lease.refresh().with_context(|| {
            format!("Branch lease authority was lost before PR worktree {operation}")
        })
    }

    fn with_control<T>(
        &mut self,
        operation: impl FnOnce(&RepoContext, &mut dyn ExecutionControl) -> Result<T>,
    ) -> Result<T> {
        let lease = &self.lease;
        let cancelled = || lease.renewal_failed();
        let mut control = AdditionalCancellationControl::new(&mut self.observer, &cancelled);
        operation(self.ctx, &mut control)
    }

    fn cleanup_candidate(&mut self, worktree: &Path) -> Result<bool> {
        self.refresh("path inspection")?;
        let path_exists =
            inspect_managed_directory(self.ctx.root(), worktree, "PR repair worktree")?;
        self.refresh("registration inspection")?;
        if self.with_control(|ctx, observer| {
            pr_worktree_is_registered(ctx, worktree, observer)
        })? {
            self.remove(worktree, true)?;
            return Ok(true);
        }
        if path_exists {
            self.refresh("partial-directory removal")?;
            fs::remove_dir(worktree).with_context(|| {
                format!(
                    "Failed to remove partial unregistered PR repair worktree {}",
                    worktree.display()
                )
            })?;
            return Ok(true);
        }
        Ok(false)
    }

    fn failed_worktree_has_evidence(
        &mut self,
        worktree: &Path,
        expected_head: &str,
    ) -> Result<bool> {
        self.refresh("status inspection")?;
        let status = self.with_control(|ctx, observer| {
            git_stdout(ctx, worktree, ["status", "--porcelain"], observer)
                .map_err(pr_step_error)
        })?;
        self.refresh("revision inspection")?;
        let head = self.with_control(|ctx, observer| {
            git_stdout(ctx, worktree, ["rev-parse", "HEAD"], observer).map_err(pr_step_error)
        })?;
        Ok(!status.trim().is_empty() || head.trim() != expected_head)
    }

    fn remove(&mut self, worktree: &Path, force: bool) -> Result<()> {
        self.refresh("removal")?;
        self.with_control(|ctx, observer| remove_pr_worktree(ctx, worktree, force, observer))
    }
}

fn is_git_object_id(value: &str) -> bool {
    matches!(value.len(), 40 | 64) && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn pr_worktree_root(ctx: &RepoContext, workflow_id: &str) -> PathBuf {
    let digest = Sha256::digest(workflow_id.as_bytes());
    let workflow_key = digest
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    ctx.root()
        .join(LOOP_RUNTIME_DIR)
        .join("worktrees/prs")
        .join(workflow_key)
}

fn pr_worktree_path(
    ctx: &RepoContext,
    workflow: &ResolvedWorkflow,
    item: &PrWorkItem,
) -> PathBuf {
    pr_worktree_root(ctx, &workflow.id).join(format!(
        "pr-{}-{}",
        item.pr_number,
        bounded_path_component(&item.head_ref)
    ))
}

fn remove_pr_worktree(
    ctx: &RepoContext,
    worktree: &Path,
    force: bool,
    observer: &mut dyn ExecutionControl,
) -> Result<()> {
    let mut args = vec![OsString::from("worktree"), OsString::from("remove")];
    if force {
        args.push(OsString::from("--force"));
    }
    args.push(worktree.as_os_str().to_os_string());
    match git_output(ctx, ctx.root(), args, observer) {
        Ok(output) if output.status.success() => Ok(()),
        Ok(output) => Err(git_error("Failed to remove PR repair worktree", output)),
        Err(PrRepairStepError::Cancelled(detail)) => Err(anyhow!(detail)),
        Err(PrRepairStepError::Failed(error)) => Err(error),
    }
}

fn start_base_merge(
    ctx: &RepoContext,
    worktree: &Path,
    base_ref: &str,
    observer: &mut dyn ExecutionControl,
) -> PrRepairStepResult<Value> {
    let base_ref = remote_branch_ref(base_ref);
    let fetch = git_output(ctx, worktree, ["fetch", "origin", &base_ref], observer)?;
    if !fetch.status.success() {
        return Err(PrRepairStepError::failed(git_error(
            "git fetch base branch failed",
            fetch,
        )));
    }
    let merge = git_output(
        ctx,
        worktree,
        git_with_pr_manager_identity(["merge", "--no-edit", "FETCH_HEAD"]),
        observer,
    )?;
    Ok(json!({
        "exit_status": merge.status.code().unwrap_or(1),
        "stdout": String::from_utf8_lossy(&merge.stdout),
        "stderr": String::from_utf8_lossy(&merge.stderr),
        "conflicts": !merge.status.success(),
    }))
}

fn validation_tree_after_base_merge(
    ctx: &RepoContext,
    worktree: &Path,
    merge: Option<&Value>,
    observer: &mut dyn ExecutionControl,
) -> PrRepairStepResult<String> {
    if merge
        .and_then(|value| value.get("conflicts"))
        .and_then(Value::as_bool)
        == Some(true)
    {
        // The ort strategy records its conflicted working-tree snapshot here.
        // Comparing the worker result with this tree validates only the repair,
        // rather than revalidating every incoming base-branch line.
        git_stdout(
            ctx,
            worktree,
            ["rev-parse", "--verify", "AUTO_MERGE^{tree}"],
            observer,
        )
    } else {
        git_stdout(ctx, worktree, ["rev-parse", "HEAD"], observer)
    }
}

fn remote_branch_ref(branch: &str) -> String {
    format!("refs/heads/{branch}")
}

fn require_remote_head(
    ctx: &RepoContext,
    cwd: &Path,
    remote_ref: &str,
    expected_head: &str,
    observer: &mut dyn ExecutionControl,
) -> PrRepairStepResult<()> {
    let stdout = git_stdout(
        ctx,
        cwd,
        ["ls-remote", "--exit-code", "origin", remote_ref],
        observer,
    )?;
    let observed = remote_head_from_ls_remote(stdout.as_bytes(), remote_ref)
        .ok_or_else(|| PrRepairStepError::failed(anyhow!("Remote ref {remote_ref} was not found")))?;
    if observed != expected_head {
        return Err(PrRepairStepError::failed(anyhow!(
            "Remote ref {remote_ref} changed after the GitHub snapshot: expected {expected_head}, found {observed}"
        )));
    }
    Ok(())
}

fn commit_and_push(
    ctx: &RepoContext,
    worktree: &Path,
    head_ref: &str,
    base_head: &str,
    validation_tree: &str,
    observer: &mut dyn ExecutionControl,
) -> PrPushResult<Value> {
    let dirty_before_commit = git_stdout(ctx, worktree, ["status", "--porcelain"], observer)?;
    if !dirty_before_commit.is_empty() {
        // Git metadata is deliberately outside the workspace-write worker's
        // authority. The parent owns staging as well as validation and commit.
        git_checked(ctx, worktree, ["add", "-A"], observer)?;
        let unmerged = git_stdout(ctx, worktree, ["ls-files", "--unmerged"], observer)?;
        if !unmerged.is_empty() {
            return Err(PrRepairStepError::failed(anyhow!(
                "PR manager parent staging left unresolved merge entries in the Git index"
            ))
            .into());
        }
        git_checked(
            ctx,
            worktree,
            ["diff", "--check", validation_tree.trim(), "--"],
            observer,
        )?;
        // AUTO_MERGE deliberately includes the original conflict markers, so its
        // worker-only diff cannot prove that the resolution removed them. Compare
        // with the observed PR head independently while disabling whitespace rules;
        // incoming base-branch whitespace remains outside the worker's authority.
        require_no_added_conflict_markers(ctx, worktree, base_head, None, observer)?;
        git_checked(
            ctx,
            worktree,
            git_with_pr_manager_identity([
                "commit",
                "-m",
                &format!("chore: update PR via Jig PR manager ({head_ref})"),
            ]),
            observer,
        )?;
    }
    let final_head = git_stdout(ctx, worktree, ["rev-parse", "HEAD"], observer)?;
    let changed = final_head != base_head.trim();
    if !changed {
        return Ok(json!({
            "status": "no_changes",
            "pushed": false,
            "base_head": base_head.trim(),
            "final_head": final_head.trim(),
        }));
    }
    git_checked(
        ctx,
        worktree,
        [
            "diff",
            "--check",
            validation_tree.trim(),
            &final_head,
            "--",
        ],
        observer,
    )?;
    require_no_added_conflict_markers(
        ctx,
        worktree,
        base_head,
        Some(&final_head),
        observer,
    )?;

    let ancestry = git_output(
        ctx,
        worktree,
        [
            "merge-base",
            "--is-ancestor",
            base_head.trim(),
            final_head.as_str(),
        ],
        observer,
    )?;
    if !ancestry.status.success() {
        return Err(PrRepairStepError::failed(git_error(
            "PR repair head does not descend from the observed head",
            ancestry,
        ))
        .into());
    }

    let remote_ref = remote_branch_ref(head_ref);
    let expected_remote_head = base_head.trim();
    let lease = format!("--force-with-lease={remote_ref}:{expected_remote_head}");
    let push_ref = format!("HEAD:{remote_ref}");
    let push_args = ["push", &lease, "origin", &push_ref];
    let push_result = git_execution_output(worktree, push_args, ctx.command_timeout(), observer);
    let push_error = match push_result {
        Ok(push) if push.status.success() => None,
        Ok(push) => Some(PrPushError::Ambiguous {
            error: git_error("git push with expected-head lease failed", push),
            final_head: final_head.clone(),
        }),
        Err(error) => Some(pr_push_execution_error(error, &final_head)),
    };
    if let Some(push_error) = push_error {
        let reconciliation = reconcile_remote_push(ctx, worktree, head_ref, &final_head);
        if reconciliation.confirmed {
            return Ok(push_result_value(
                base_head,
                &final_head,
                Some(reconciliation.detail),
            ));
        }
        return Err(match push_error {
            PrPushError::Step(PrRepairStepError::Cancelled(detail)) => {
                PrPushError::Step(PrRepairStepError::Cancelled(format!(
                    "{detail}; push outcome was not confirmed: {}",
                    reconciliation.detail
                )))
            }
            PrPushError::Step(PrRepairStepError::Failed(error)) => {
                PrPushError::Step(PrRepairStepError::Failed(error.context(format!(
                    "push outcome was not confirmed: {}",
                    reconciliation.detail
                ))))
            }
            PrPushError::Ambiguous { error, final_head } => {
                PrPushError::Ambiguous {
                    error: error.context(format!(
                        "push outcome was not confirmed: {}",
                        reconciliation.detail
                    )),
                    final_head,
                }
            }
        });
    }

    Ok(push_result_value(base_head, &final_head, None))
}

fn pr_push_execution_error(error: ExecutionCommandError, final_head: &str) -> PrPushError {
    match error {
        ExecutionCommandError::CancelledBeforeStart => {
            PrPushError::Step(PrRepairStepError::Cancelled(
                "git push was cancelled before it started".into(),
            ))
        }
        ExecutionCommandError::Cancelled => PrPushError::Ambiguous {
            error: anyhow!("git push was cancelled while it was running"),
            final_head: final_head.to_string(),
        },
        ExecutionCommandError::Failed {
            error,
            process_started: true,
        } => PrPushError::Ambiguous {
            error,
            final_head: final_head.to_string(),
        },
        ExecutionCommandError::Failed {
            error,
            process_started: false,
        } => PrPushError::Step(PrRepairStepError::Failed(error)),
    }
}

fn push_result_value(base_head: &str, final_head: &str, reconciliation: Option<String>) -> Value {
    let mut value = json!({
        "status": "pushed",
        "pushed": true,
        "base_head": base_head.trim(),
        "final_head": final_head.trim(),
        "force": true,
        "force_with_lease": true,
        "expected_remote_head": base_head.trim(),
    });
    if let Some(reconciliation) = reconciliation {
        value["reconciliation"] = Value::String(reconciliation);
    }
    value
}

struct PushReconciliation {
    confirmed: bool,
    detail: String,
}

fn reconcile_remote_push(
    ctx: &RepoContext,
    worktree: &Path,
    head_ref: &str,
    final_head: &str,
) -> PushReconciliation {
    let remote_ref = format!("refs/heads/{head_ref}");
    let mut observer = NoopExecutionObserver;
    let timeout_seconds = ctx.command_timeout().as_secs().min(30);
    let timeout = CommandTimeout::from_seconds(timeout_seconds)
        .expect("the reconciliation timeout is nonzero and within the command timeout range");
    let output = git_execution_output(
        worktree,
        ["ls-remote", "--exit-code", "origin", &remote_ref],
        timeout,
        &mut observer,
    );
    match output {
        Ok(output) if output.status.success() => {
            let observed = remote_head_from_ls_remote(&output.stdout, &remote_ref);
            PushReconciliation {
                confirmed: observed == Some(final_head),
                detail: match observed {
                    Some(observed) if observed == final_head => {
                        format!("remote {remote_ref} confirmed at {observed}")
                    }
                    Some(observed) => {
                        format!("remote {remote_ref} resolved to {observed}; expected {final_head}")
                    }
                    None => format!("remote {remote_ref} returned no matching head"),
                },
            }
        }
        Ok(output) => PushReconciliation {
            confirmed: false,
            detail: format!(
                "remote {remote_ref} reconciliation exited with status {}",
                output.status.code().unwrap_or(1)
            ),
        },
        Err(error) => PushReconciliation {
            confirmed: false,
            detail: format!("remote {remote_ref} reconciliation failed: {error}"),
        },
    }
}

fn remote_head_from_ls_remote<'a>(stdout: &'a [u8], remote_ref: &str) -> Option<&'a str> {
    std::str::from_utf8(stdout)
        .ok()?
        .lines()
        .filter_map(|line| line.split_once(char::is_whitespace))
        .find_map(|(head, reference)| (reference.trim() == remote_ref).then_some(head.trim()))
}

fn pr_worker_prompt(
    ctx: &RepoContext,
    item: &PrWorkItem,
    pull_request: &Value,
    merge: Option<&Value>,
) -> String {
    let worker_snapshot = worker_pull_request_snapshot(pull_request);
    format!(
        "You are Jig's PR manager worker for repository `{}`.\n\
         Work only on PR #{} on branch `{}`. Reasons: {}.\n\
         Resolve the reported PR issues in this isolated worktree. If merge conflicts are present, resolve them completely. \
         If CI is failing, inspect the failing checks and fix the underlying code. \
         If unresolved review threads are present, address the actionable feedback with code changes when possible. \
         Do not use `gh`, `curl`, or network access to reply to or resolve review threads. \
         Instead, return review-thread reply intents in the required structured output. \
         Include a reply intent only when a concise comment or resolution is needed after your code changes; set `resolve` only when the feedback is fully addressed.\n\
         Run relevant local tests when available. Do not merge the PR. Do not force-push. Keep changes minimal. \
         Do not stage or commit changes; Jig owns Git metadata and will stage, validate, and commit after you exit. \
         Always write structured output with `summary` and `review_thread_replies`.\n\n\
         Merge preparation result:\n{}\n\n\
         Normalized PR snapshot:\n{}\n",
        ctx.repo_name(),
        item.pr_number,
        item.head_ref,
        item.reasons.join(", "),
        merge
            .map(|value| serde_json::to_string_pretty(value).unwrap_or_else(|_| value.to_string()))
            .unwrap_or_else(|| "none".into()),
        serde_json::to_string_pretty(&worker_snapshot)
            .unwrap_or_else(|_| worker_snapshot.to_string()),
    )
}

fn worker_pull_request_snapshot(pull_request: &Value) -> Value {
    let checks = pull_request
        .pointer("/checks/runs")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|check| {
            json!({
                "name": check.get("name").cloned().unwrap_or(Value::Null),
                "workflow": check.get("workflow").cloned().unwrap_or(Value::Null),
                "state": check.get("state").cloned().unwrap_or(Value::Null),
                "bucket": check.get("bucket").cloned().unwrap_or(Value::Null),
                "event": check.get("event").cloned().unwrap_or(Value::Null),
                "link": check.get("link").cloned().unwrap_or(Value::Null),
                "started_at": check.get("started_at").cloned().unwrap_or(Value::Null),
                "completed_at": check.get("completed_at").cloned().unwrap_or(Value::Null),
            })
        })
        .collect::<Vec<_>>();
    let trusted_threads = actionable_review_threads(pull_request)
        .map(|thread| {
            let comments = thread
                .pointer("/comments/nodes")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter(|comment| {
                    comment
                        .pointer("/author/trusted")
                        .and_then(Value::as_bool)
                        .unwrap_or(false)
                })
                .cloned()
                .collect::<Vec<_>>();
            json!({
                "id": thread.get("id").cloned().unwrap_or(Value::Null),
                "is_resolved": thread.get("is_resolved").cloned().unwrap_or(Value::Null),
                "is_outdated": thread.get("is_outdated").cloned().unwrap_or(Value::Null),
                "path": thread.get("path").cloned().unwrap_or(Value::Null),
                "line": thread.get("line").cloned().unwrap_or(Value::Null),
                "start_line": thread.get("start_line").cloned().unwrap_or(Value::Null),
                "subject_type": thread.get("subject_type").cloned().unwrap_or(Value::Null),
                "diff_side": thread.get("diff_side").cloned().unwrap_or(Value::Null),
                "viewer_can_reply": thread.get("viewer_can_reply").cloned().unwrap_or(Value::Null),
                "viewer_can_resolve": thread.get("viewer_can_resolve").cloned().unwrap_or(Value::Null),
                "comments": { "nodes": comments },
            })
        })
        .collect::<Vec<_>>();
    json!({
        "number": pull_request.get("number").cloned().unwrap_or(Value::Null),
        "state": pull_request.get("state").cloned().unwrap_or(Value::Null),
        "base": pull_request.get("base").cloned().unwrap_or(Value::Null),
        "head": pull_request.get("head").cloned().unwrap_or(Value::Null),
        "stack": pull_request.get("stack").cloned().unwrap_or(Value::Null),
        "mergeability": pull_request.get("mergeability").cloned().unwrap_or(Value::Null),
        "checks": {
            "summary": pull_request.pointer("/checks/summary").cloned().unwrap_or(Value::Null),
            "runs": checks,
        },
        "review_threads": {
            "summary": pull_request.pointer("/review_threads/summary").cloned().unwrap_or(Value::Null),
            "nodes": trusted_threads,
        },
    })
}

fn with_attempt(mut action: Value, attempt: AttemptRecord) -> Value {
    if let Some(object) = action.as_object_mut() {
        object.insert("attempt".into(), json!(attempt));
    }
    action
}

fn with_branch_lease_result(mut action: Value, release_error: Option<&anyhow::Error>) -> Value {
    if let Some(release_error) = release_error {
        if action.get("lease_error").is_some() {
            action["lease_release_error"] = json!(format!("{release_error:#}"));
            return action;
        }
        let completed_error = action["error"].as_str().map(str::to_string);
        action["completed_status"] = action["status"].clone();
        if let Some(completed_error) = completed_error.as_deref() {
            action["completed_error"] = json!(completed_error);
        }
        action["status"] = json!("needs_attention");
        action["attention_kind"] = json!("branch_lease_lost_after_start");
        action["lease_error"] = json!(format!("{release_error:#}"));
        action["error"] = json!(match completed_error {
            Some(completed_error) => format!(
                "Branch repair completed, but lease renewal or release failed: {release_error:#}; completed action: {completed_error}"
            ),
            None => format!(
                "Branch repair completed, but lease renewal or release failed: {release_error:#}"
            ),
        });
    }
    action
}
