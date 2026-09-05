use super::{App, Tab, WorkerRequest};

pub(super) fn next_request(app: &mut App, periodic_due: bool) -> Option<WorkerRequest> {
    next_queued_refresh(app)
        .map(WorkerRequest::Domain)
        .or_else(|| {
            app.take_plan_request()
                .map(|(basis, plan_id)| WorkerRequest::Plan { basis, plan_id })
        })
        .or_else(|| periodic_due.then_some(WorkerRequest::Domain(app.tab)))
}

pub(super) fn next_queued_refresh(app: &App) -> Option<Tab> {
    let active = app.tab;
    if app.domain(active).refresh_queued {
        return Some(active);
    }
    let other = if active.is_status_domain() {
        Tab::Work
    } else {
        Tab::Status
    };
    app.domain(other).refresh_queued.then_some(other)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn queued_domain_work_precedes_plan_detail_and_periodic_work() {
        let mut app = App::new(Tab::Work);
        app.recorder.data = Some(crate::dashboard::scenarios::recorder_snapshot().into());
        assert!(app.open_selected_detail());
        app.status.refresh_queued = true;

        assert!(matches!(
            next_request(&mut app, true),
            Some(WorkerRequest::Domain(Tab::Status))
        ));
        app.status.refresh_queued = false;
        let plan = next_request(&mut app, true).unwrap();
        assert!(matches!(plan, WorkerRequest::Plan { .. }));
        assert!(!plan.resets_refresh_timer());
        assert!(matches!(
            next_request(&mut app, true),
            Some(WorkerRequest::Domain(Tab::Work))
        ));
        assert!(WorkerRequest::Domain(Tab::Work).resets_refresh_timer());
    }
}
