const REVIEW_THREAD_COMMENT_PAGE_LIMIT: usize = 100;
const REVIEW_THREAD_PRIMARY_REQUESTS_PER_INTENT: usize = REVIEW_THREAD_COMMENT_PAGE_LIMIT * 3 + 2;
const REVIEW_THREAD_RECONCILIATION_REQUESTS_PER_INTENT: usize =
    REVIEW_THREAD_COMMENT_PAGE_LIMIT + 1;
const REVIEW_THREAD_UPDATE_REQUESTS_PER_INTENT: usize =
    REVIEW_THREAD_PRIMARY_REQUESTS_PER_INTENT
        + REVIEW_THREAD_RECONCILIATION_REQUESTS_PER_INTENT;
const REVIEW_THREAD_UPDATE_TIMEOUT: Duration = Duration::from_secs(10 * 60);
const MUTATION_RECONCILIATION_TIMEOUT: Duration = Duration::from_secs(30);

struct ReviewThreadUpdateBudget {
    started_at: Instant,
    timeout: Duration,
    request_count: usize,
    request_limit: usize,
    primary_request_count: usize,
    primary_request_limit: usize,
    intent_started_at: Instant,
    intent_timeout: Duration,
    intent_request_count: usize,
    intent_primary_request_count: usize,
    reconciliation_started_at: Option<Instant>,
}

impl ReviewThreadUpdateBudget {
    fn new(command_timeout: CommandTimeout, actionable_intent_count: usize) -> Self {
        let started_at = Instant::now();
        let intent_multiplier = u32::try_from(actionable_intent_count.max(1)).unwrap_or(u32::MAX);
        let timeout = command_timeout
            .duration()
            .saturating_mul(intent_multiplier)
            .min(REVIEW_THREAD_UPDATE_TIMEOUT);
        Self {
            started_at,
            timeout,
            request_count: 0,
            request_limit: REVIEW_THREAD_UPDATE_REQUESTS_PER_INTENT
                .saturating_mul(actionable_intent_count.max(1)),
            primary_request_count: 0,
            primary_request_limit: REVIEW_THREAD_PRIMARY_REQUESTS_PER_INTENT
                .saturating_mul(actionable_intent_count.max(1)),
            intent_started_at: started_at,
            intent_timeout: command_timeout.duration().min(timeout),
            intent_request_count: 0,
            intent_primary_request_count: 0,
            reconciliation_started_at: None,
        }
    }

    fn begin_intent(&mut self, command_timeout: CommandTimeout) {
        self.intent_started_at = Instant::now();
        self.intent_timeout = command_timeout
            .duration()
            .min(self.timeout.saturating_sub(self.started_at.elapsed()));
        self.intent_request_count = 0;
        self.intent_primary_request_count = 0;
        self.reconciliation_started_at = None;
    }

    fn begin_reconciliation(&mut self) {
        self.reconciliation_started_at = Some(Instant::now());
    }

    fn reserve_request(
        &mut self,
        requested_timeout: Duration,
    ) -> std::result::Result<Duration, ExecutionCommandError> {
        if self.intent_primary_request_count >= REVIEW_THREAD_PRIMARY_REQUESTS_PER_INTENT {
            return Err(ExecutionCommandError::failed(anyhow!(
                "GitHub review thread intent exceeded its {REVIEW_THREAD_PRIMARY_REQUESTS_PER_INTENT}-request primary-operation budget"
            )));
        }
        if self.primary_request_count >= self.primary_request_limit {
            return Err(ExecutionCommandError::failed(anyhow!(
                "GitHub review thread updates exceeded their {}-request primary-operation budget",
                self.primary_request_limit
            )));
        }
        let batch_remaining = self
            .timeout
            .checked_sub(self.started_at.elapsed())
            .filter(|remaining| !remaining.is_zero())
            .ok_or_else(|| {
                ExecutionCommandError::failed(anyhow!(
                    "GitHub review thread updates exceeded their {}-second deadline",
                    self.timeout.as_secs()
                ))
            })?;
        let intent_remaining = self
            .intent_timeout
            .checked_sub(self.intent_started_at.elapsed())
            .filter(|remaining| !remaining.is_zero())
            .ok_or_else(|| {
                ExecutionCommandError::failed(anyhow!(
                    "GitHub review thread intent exceeded its {}-second deadline",
                    self.intent_timeout.as_secs()
                ))
            })?;
        let timeout = batch_remaining.min(intent_remaining).min(requested_timeout);
        if timeout.is_zero() {
            return Err(ExecutionCommandError::failed(anyhow!(
                "GitHub review thread updates exceeded their {}-second deadline",
                self.timeout.as_secs()
            )));
        }
        self.request_count += 1;
        self.primary_request_count += 1;
        self.intent_request_count += 1;
        self.intent_primary_request_count += 1;
        Ok(timeout)
    }

    fn reserve_reconciliation_request(
        &mut self,
        requested_timeout: Duration,
    ) -> std::result::Result<Duration, ExecutionCommandError> {
        if self.intent_request_count >= REVIEW_THREAD_UPDATE_REQUESTS_PER_INTENT {
            return Err(ExecutionCommandError::failed(anyhow!(
                "GitHub review thread intent exceeded its {REVIEW_THREAD_UPDATE_REQUESTS_PER_INTENT}-request total budget"
            )));
        }
        if self.request_count >= self.request_limit {
            return Err(ExecutionCommandError::failed(anyhow!(
                "GitHub review thread updates exceeded their {}-request total budget",
                self.request_limit
            )));
        }
        let reconciliation_started_at = self.reconciliation_started_at.ok_or_else(|| {
            ExecutionCommandError::failed(anyhow!(
                "GitHub mutation reconciliation was not initialized"
            ))
        })?;
        let remaining = MUTATION_RECONCILIATION_TIMEOUT
            .checked_sub(reconciliation_started_at.elapsed())
            .filter(|remaining| !remaining.is_zero())
            .ok_or_else(|| {
                ExecutionCommandError::failed(anyhow!(
                    "GitHub mutation reconciliation exceeded its {}-second deadline",
                    MUTATION_RECONCILIATION_TIMEOUT.as_secs()
                ))
            })?;
        let timeout = remaining.min(requested_timeout);
        if timeout.is_zero() {
            return Err(ExecutionCommandError::failed(anyhow!(
                "GitHub mutation reconciliation exceeded its {}-second deadline",
                MUTATION_RECONCILIATION_TIMEOUT.as_secs()
            )));
        }
        self.request_count += 1;
        self.intent_request_count += 1;
        Ok(timeout)
    }
}
