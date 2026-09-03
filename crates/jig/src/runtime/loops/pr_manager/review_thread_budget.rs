const REVIEW_THREAD_COMMENT_PAGE_LIMIT: usize = 100;
const REVIEW_THREAD_UPDATE_REQUESTS_PER_INTENT: usize = REVIEW_THREAD_COMMENT_PAGE_LIMIT * 3 + 3;
const REVIEW_THREAD_UPDATE_TIMEOUT: Duration = Duration::from_secs(10 * 60);
const MUTATION_RECONCILIATION_TIMEOUT: Duration = Duration::from_secs(30);

struct ReviewThreadUpdateBudget {
    started_at: Instant,
    timeout: Duration,
    request_count: usize,
    request_limit: usize,
    intent_started_at: Instant,
    intent_timeout: Duration,
    intent_request_count: usize,
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
            intent_started_at: started_at,
            intent_timeout: command_timeout.duration().min(timeout),
            intent_request_count: 0,
        }
    }

    fn begin_intent(&mut self, command_timeout: CommandTimeout) {
        self.intent_started_at = Instant::now();
        self.intent_timeout = command_timeout
            .duration()
            .min(self.timeout.saturating_sub(self.started_at.elapsed()));
        self.intent_request_count = 0;
    }

    fn reserve_request(
        &mut self,
        requested_timeout: Duration,
    ) -> std::result::Result<Duration, ExecutionCommandError> {
        if self.intent_request_count >= REVIEW_THREAD_UPDATE_REQUESTS_PER_INTENT {
            return Err(ExecutionCommandError::failed(anyhow!(
                "GitHub review thread intent exceeded its {REVIEW_THREAD_UPDATE_REQUESTS_PER_INTENT}-request budget"
            )));
        }
        if self.request_count >= self.request_limit {
            return Err(ExecutionCommandError::failed(anyhow!(
                "GitHub review thread updates exceeded their {}-request budget",
                self.request_limit
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
        self.intent_request_count += 1;
        Ok(timeout)
    }
}
