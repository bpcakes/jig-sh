use serde_json::{Value, json};

use super::records::PlanEvent;

/// A confirmed plan close followed by a failed finalization step. This does
/// not cover ambiguous failures while appending the close event itself.
#[derive(Debug)]
pub(crate) struct PlanClosurePartialFailure {
    plan_id: String,
    close_event_id: Option<String>,
    retirement: Value,
    receipt_id: Option<String>,
    stage: FailureStage,
    source: anyhow::Error,
}

#[derive(Debug)]
enum FailureStage {
    ReceiptPublication { may_have_landed: bool },
    SessionTeardown,
}

impl PlanClosurePartialFailure {
    pub(super) fn receipt(event: &PlanEvent, source: anyhow::Error) -> Self {
        Self {
            plan_id: event.plan_id().to_owned(),
            close_event_id: Some(event.id().to_owned()),
            retirement: event
                .retirement()
                .map_or(Value::Null, |value| value.to_value()),
            receipt_id: None,
            stage: FailureStage::ReceiptPublication {
                may_have_landed: super::receipt_append_may_have_landed(&source),
            },
            source,
        }
    }

    pub(crate) fn session(plan_id: &str, plan: &Value, source: anyhow::Error) -> Self {
        Self {
            plan_id: plan_id.to_owned(),
            close_event_id: plan["close_event_id"].as_str().map(str::to_owned),
            retirement: plan["retirement"].clone(),
            receipt_id: plan["receipt_id"].as_str().map(str::to_owned),
            stage: FailureStage::SessionTeardown,
            source,
        }
    }

    pub(crate) fn details(&self) -> Value {
        let (stage, receipt_status, session_status) = match self.stage {
            FailureStage::ReceiptPublication { may_have_landed } => (
                "receipt_publication",
                if may_have_landed {
                    "unknown"
                } else {
                    "not_recorded"
                },
                "not_attempted",
            ),
            FailureStage::SessionTeardown => ("session_teardown", "recorded", "unknown"),
        };
        json!({
            "plan_id": self.plan_id,
            "plan_state": "closed",
            "close_event_id": self.close_event_id,
            "retirement": self.retirement,
            "failed_stage": stage,
            "receipt": { "status": receipt_status, "receipt_id": self.receipt_id },
            "session_teardown": { "status": session_status },
            "retry_safe": false,
            "recovery": {
                "detail": "The plan remains closed. Automatic finalization retry is not supported; repeating finish or retire is rejected. Inspect persisted state before repairing the failed finalization step.",
                "inspect_commands": [
                    "scripts/jig work status",
                    format!("scripts/jig work receipts --plan-id {}", self.plan_id),
                ],
            },
        })
    }
}

impl std::fmt::Display for PlanClosurePartialFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Plan {} is already closed{} (close event {}); ",
            self.plan_id,
            if self.retirement.is_null() {
                ""
            } else {
                " by retirement"
            },
            self.close_event_id.as_deref().unwrap_or("unknown")
        )?;
        match self.stage {
            FailureStage::ReceiptPublication { may_have_landed } => {
                write!(
                    f,
                    "closure receipt publication failed{}. Session teardown was not attempted. ",
                    if may_have_landed {
                        " and the receipt may have been written"
                    } else {
                        " before the receipt was written"
                    }
                )?;
            }
            FailureStage::SessionTeardown => {
                write!(
                    f,
                    "the closure receipt was recorded, but session teardown failed. Session state may already have changed. "
                )?;
            }
        }
        write!(
            f,
            "Repeating finish or retire will be rejected. Inspect `scripts/jig work status` and `scripts/jig work receipts --plan-id {}` before recovery.",
            self.plan_id
        )
    }
}

impl std::error::Error for PlanClosurePartialFailure {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(self.source.as_ref())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ambiguous_receipt_append_is_reported_as_unknown() {
        let event = PlanEvent::close("close_example".into(), "plan_example".into(), 1, None);
        let partial = PlanClosurePartialFailure::receipt(
            &event,
            crate::state::receipt_append_may_have_landed_for_test(),
        );
        assert_eq!(partial.details()["receipt"]["status"], "unknown");
        assert_eq!(
            partial.details()["session_teardown"]["status"],
            "not_attempted"
        );
        assert!(partial.to_string().contains("may have been written"));
        assert!(crate::state::receipt_append_may_have_landed(
            &partial.into()
        ));
    }
}
