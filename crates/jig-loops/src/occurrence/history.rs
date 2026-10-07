use std::collections::BTreeSet;

use super::*;

pub(super) fn prune_history(store: &mut ScheduleFile) {
    let workflow_ids = store
        .occurrences
        .values()
        .map(|record| record.workflow_id.clone())
        .collect::<BTreeSet<_>>();
    for workflow_id in workflow_ids {
        let latest_scheduled_id = store
            .occurrences
            .values()
            .filter(|record| {
                record.workflow_id == workflow_id
                    && record.scheduled_at_ms != MANUAL_OCCURRENCE_SCHEDULED_AT_MS
            })
            .max_by_key(|record| record.scheduled_at_ms)
            .map(|record| record.occurrence_id.clone());
        let mut terminal = store
            .occurrences
            .values()
            .filter(|record| {
                record.workflow_id == workflow_id
                    && record.is_prunable_history()
                    && !record.has_retained_worktree()
            })
            .map(|record| {
                (
                    record.finished_at_ms.unwrap_or(record.started_at_ms),
                    record.started_at_ms,
                    record.scheduled_at_ms,
                    record.occurrence_id.clone(),
                )
            })
            .collect::<Vec<_>>();
        terminal.sort_by(|left, right| right.cmp(left));
        let mut retained = BTreeSet::new();
        if let Some(latest_scheduled_id) = latest_scheduled_id
            && terminal
                .iter()
                .any(|(_, _, _, occurrence_id)| occurrence_id == &latest_scheduled_id)
        {
            retained.insert(latest_scheduled_id);
        }
        for (_, _, _, occurrence_id) in &terminal {
            if retained.len() == OCCURRENCE_HISTORY_PER_WORKFLOW {
                break;
            }
            retained.insert(occurrence_id.clone());
        }
        for (_, _, _, occurrence_id) in terminal {
            if !retained.contains(&occurrence_id) {
                store.occurrences.remove(&occurrence_id);
            }
        }
    }
}
