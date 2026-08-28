use super::*;

pub(super) struct ExecutionSourceEpoch {
    trusted_fingerprint: std::result::Result<String, String>,
    observed_fingerprint: std::result::Result<String, String>,
    reuse_observation_before_next_target: bool,
    observation_count: usize,
    observation_elapsed: Duration,
}

pub(super) struct ExecutionSourceObservation {
    fingerprint: std::result::Result<String, String>,
    elapsed: Duration,
}

impl ExecutionSourceObservation {
    pub(super) fn collect(ctx: &RepoContext) -> Self {
        Self::collect_with(|| collect_execution_fingerprint(ctx))
    }

    pub(super) fn collect_with(
        collect: impl FnOnce() -> std::result::Result<String, String>,
    ) -> Self {
        let started = Instant::now();
        Self {
            fingerprint: collect(),
            elapsed: started.elapsed(),
        }
    }
}

impl ExecutionSourceEpoch {
    pub(super) fn from_plan(fingerprint: String) -> Self {
        Self {
            trusted_fingerprint: Ok(fingerprint.clone()),
            observed_fingerprint: Ok(fingerprint),
            reuse_observation_before_next_target: false,
            observation_count: 0,
            observation_elapsed: Duration::ZERO,
        }
    }

    pub(super) fn metrics(&self) -> SourceObservationMetrics {
        SourceObservationMetrics {
            count: self.observation_count,
            elapsed_ms: u64::try_from(self.observation_elapsed.as_millis()).unwrap_or(u64::MAX),
        }
    }

    pub(super) fn receipt_fingerprint(&self) -> std::result::Result<String, String> {
        self.observed_fingerprint.clone()
    }

    pub(super) fn discard_reusable_observation(&mut self) {
        self.reuse_observation_before_next_target = false;
    }

    pub(super) fn begin_read_only_layer(&mut self) {
        self.discard_reusable_observation();
    }

    pub(super) fn prepare_target(
        &mut self,
        ctx: &RepoContext,
        planned: &PlannedTarget,
    ) -> std::result::Result<(), String> {
        self.prepare_target_with(planned, || collect_execution_fingerprint(ctx))
    }

    pub(super) fn prepare_target_with(
        &mut self,
        planned: &PlannedTarget,
        collect: impl FnOnce() -> std::result::Result<String, String>,
    ) -> std::result::Result<(), String> {
        let trusted_fingerprint = self.trusted_fingerprint.clone()?;
        // The post-target observation is also the precondition for the next
        // adjacent read-only target. Its own postcondition still rejects any
        // intervening source mutation. A worktree-mutating target must always
        // take a fresh precondition: its postcondition intentionally adopts a
        // new trusted baseline and therefore cannot distinguish an external
        // edit in this gap from its own declared effects.
        let reuse_observation = std::mem::take(&mut self.reuse_observation_before_next_target);
        let current = if reuse_observation && !allows_worktree_mutation(planned) {
            self.observed_fingerprint.clone()
        } else {
            self.observe_with(collect)
        };
        self.observed_fingerprint = current.clone();
        match current {
            Ok(current) if current == trusted_fingerprint => Ok(()),
            Ok(current) => Err(format!(
                "target '{}' could not start because the worktree changed after plan validation or the last declared worktree effect (expected {trusted_fingerprint}, current {current}); plan again",
                planned.target
            )),
            Err(error) => Err(format!(
                "could not establish the worktree effect invariant before target '{}': {error}",
                planned.target
            )),
        }
    }

    pub(super) fn prepare_queued_read_only_target(
        &mut self,
        planned: &PlannedTarget,
        observation: ExecutionSourceObservation,
    ) -> (
        std::result::Result<(), String>,
        std::result::Result<String, String>,
    ) {
        debug_assert!(!allows_worktree_mutation(planned));
        self.discard_reusable_observation();
        self.observation_count = self.observation_count.saturating_add(1);
        self.observation_elapsed = self.observation_elapsed.saturating_add(observation.elapsed);
        let current = observation.fingerprint;
        self.observed_fingerprint = current.clone();
        let precondition = match (self.trusted_fingerprint.as_deref(), current.as_deref()) {
            (Ok(expected), Ok(actual)) if actual == expected => Ok(()),
            (Ok(expected), Ok(actual)) => Err(format!(
                "target '{}' could not start because the worktree changed after plan validation or the last declared worktree effect (expected {expected}, current {actual}); plan again",
                planned.target
            )),
            (Ok(_), Err(error)) => Err(format!(
                "could not establish the worktree effect invariant before target '{}': {error}",
                planned.target
            )),
            (Err(error), _) => Err(error.to_string()),
        };
        (precondition, current)
    }

    pub(super) fn finish_target(
        &mut self,
        ctx: &RepoContext,
        planned: &PlannedTarget,
        capture: TargetCapture,
    ) -> (TargetCapture, std::result::Result<String, String>) {
        self.finish_target_with(planned, capture, || collect_execution_fingerprint(ctx))
    }

    pub(super) fn finish_completed_target(
        &mut self,
        ctx: &RepoContext,
        planned: &PlannedTarget,
        completed: CompletedTargetCapture,
    ) -> (CompletedTargetCapture, std::result::Result<String, String>) {
        let CompletedTargetCapture {
            started_at_ms,
            ended_at_ms,
            capture,
        } = completed;
        let (capture, fingerprint) = self.finish_target(ctx, planned, capture);
        (
            CompletedTargetCapture {
                started_at_ms,
                ended_at_ms,
                capture,
            },
            fingerprint,
        )
    }

    pub(super) fn prepare_read_only_layer(
        &mut self,
        ctx: &RepoContext,
        target_count: usize,
    ) -> std::result::Result<(), String> {
        let trusted_fingerprint = self.trusted_fingerprint.clone()?;
        let current = self.observe_with(|| collect_execution_fingerprint(ctx));
        self.observed_fingerprint = current.clone();
        match current {
            Ok(current) if current == trusted_fingerprint => Ok(()),
            Ok(current) => Err(format!(
                "parallel read-only layer of {target_count} targets could not start because the worktree changed after plan validation or the last declared worktree effect (expected {trusted_fingerprint}, current {current}); plan again"
            )),
            Err(error) => Err(format!(
                "could not establish the worktree effect invariant before a parallel read-only layer of {target_count} targets: {error}"
            )),
        }
    }

    pub(super) fn observe_read_only_layer_postcondition(
        &mut self,
        ctx: &RepoContext,
    ) -> std::result::Result<String, String> {
        let current = self.observe_with(|| collect_execution_fingerprint(ctx));
        self.observed_fingerprint = current.clone();
        self.discard_reusable_observation();
        current
    }

    pub(super) fn finish_started_read_only_layer_target(
        &self,
        planned: &PlannedTarget,
        current: &std::result::Result<String, String>,
        completed: CompletedTargetCapture,
    ) -> (CompletedTargetCapture, std::result::Result<String, String>) {
        debug_assert!(completed.was_started());
        let completed =
            completed.map_capture(|capture| match self.trusted_fingerprint.as_deref() {
                Ok(expected) => {
                    enforce_read_only_layer_worktree_effect(planned, expected, current, capture)
                }
                Err(error) => block_for_unverifiable_effect_policy(planned, error, capture),
            });
        (completed, current.clone())
    }

    pub(super) fn finish_target_with(
        &mut self,
        planned: &PlannedTarget,
        capture: TargetCapture,
        collect: impl FnOnce() -> std::result::Result<String, String>,
    ) -> (TargetCapture, std::result::Result<String, String>) {
        if allows_worktree_mutation(planned) && !capture.may_have_executed {
            // A declared worktree effect is authority to adopt changes made
            // by the target, not changes made by somebody else while Jig was
            // still validating or spawning it. Preserve the trusted epoch
            // when execution definitely never began.
            self.discard_reusable_observation();
            return (capture, self.receipt_fingerprint());
        }
        let current = self.observe_with(collect);
        self.observed_fingerprint = current.clone();
        // A successful postcondition is a valid precondition for an adjacent
        // target. An observation failure is not stable evidence and must be
        // retried before the next target instead of poisoning it permanently.
        self.reuse_observation_before_next_target = current.is_ok();
        if allows_worktree_mutation(planned) {
            self.trusted_fingerprint = current.clone();
            let capture = match current.as_deref() {
                Ok(_) => capture,
                Err(error) => block_for_unverifiable_effect_policy(planned, error, capture),
            };
            return (capture, current);
        }

        let capture = match self.trusted_fingerprint.as_deref() {
            Ok(expected) => enforce_declared_worktree_effect(planned, expected, &current, capture),
            Err(error) => block_for_unverifiable_effect_policy(planned, error, capture),
        };
        (capture, current)
    }

    pub(super) fn observe_with(
        &mut self,
        collect: impl FnOnce() -> std::result::Result<String, String>,
    ) -> std::result::Result<String, String> {
        let started = Instant::now();
        let observation = collect();
        self.observation_count = self.observation_count.saturating_add(1);
        self.observation_elapsed = self.observation_elapsed.saturating_add(started.elapsed());
        observation
    }
}

pub(super) fn collect_execution_fingerprint(
    ctx: &RepoContext,
) -> std::result::Result<String, String> {
    crate::git_receipts::repository_source_snapshot(ctx.root())
        .map(|snapshot| snapshot.worktree_fingerprint)
        .map_err(|error| format!("{error:#}"))
}

pub(super) fn block_for_unverifiable_effect_policy(
    planned: &PlannedTarget,
    error: &str,
    mut capture: TargetCapture,
) -> TargetCapture {
    let message = format!(
        "could not verify the worktree effect invariant for target '{}': {error}",
        planned.target
    );
    capture.stderr.push_str(&format!("{message}\n"));
    capture.findings.push(finding(message, "effect_policy"));
    if capture.conclusion == RunConclusion::Success {
        capture.conclusion = RunConclusion::Blocked;
        capture.receipt_exit_status = capture.receipt_exit_status.max(1);
    }
    capture
}

pub(super) fn allows_worktree_mutation(planned: &PlannedTarget) -> bool {
    planned
        .effects
        .contains(&jig_contract::ActionEffect::Worktree)
}

pub(super) fn enforce_declared_worktree_effect(
    planned: &PlannedTarget,
    expected: &str,
    current: &std::result::Result<String, String>,
    mut capture: TargetCapture,
) -> TargetCapture {
    debug_assert!(!allows_worktree_mutation(planned));

    match current.as_deref() {
        Ok(actual) if actual == expected => capture,
        Ok(actual) => {
            let message = format!(
                "the worktree fingerprint changed while target '{}' was running without a declared worktree effect (before {expected}, after {actual})",
                planned.target
            );
            capture.stderr.push_str(&format!("{message}\n"));
            capture.findings.push(finding(message, "effect_policy"));
            if capture.conclusion == RunConclusion::Success {
                capture.conclusion = RunConclusion::Failure;
                capture.receipt_exit_status = capture.receipt_exit_status.max(1);
            }
            capture
        }
        Err(error) => block_for_unverifiable_effect_policy(planned, error, capture),
    }
}

fn enforce_read_only_layer_worktree_effect(
    planned: &PlannedTarget,
    expected: &str,
    current: &std::result::Result<String, String>,
    mut capture: TargetCapture,
) -> TargetCapture {
    debug_assert!(!allows_worktree_mutation(planned));

    match current.as_deref() {
        Ok(actual) if actual == expected => capture,
        Ok(actual) => {
            let message = format!(
                "the worktree fingerprint changed while a parallel read-only layer was running without a declared worktree effect (before {expected}, after {actual})"
            );
            capture.stderr.push_str(&format!("{message}\n"));
            capture.findings.push(finding(message, "effect_policy"));
            if capture.conclusion == RunConclusion::Success {
                capture.conclusion = RunConclusion::Failure;
                capture.receipt_exit_status = capture.receipt_exit_status.max(1);
            }
            capture
        }
        Err(error) => block_for_unverifiable_effect_policy(planned, error, capture),
    }
}
