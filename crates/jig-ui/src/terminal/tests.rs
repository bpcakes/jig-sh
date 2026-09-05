use ratatui::{Terminal, backend::TestBackend, layout::Rect};

use super::{
    model::{App, Tab},
    render::{self, LayoutTier},
};
use crate::dashboard::{RecorderRefresh, StatusLocalSnapshot, scenarios};

mod local;

fn app_with_snapshot(tab: Tab) -> App {
    let recorder = scenarios::recorder_snapshot();
    let status = scenarios::status_snapshot();
    let mut app = App::new(tab);
    app.accept_recorder_refresh(RecorderRefresh {
        status_local: StatusLocalSnapshot {
            epoch_id: recorder.epoch_id,
            observed_at_ms: status.observed_at_ms,
            repository: status.repository,
            work: status.work,
            loops: status.loops,
            errors: status.errors,
        },
        recorder,
    });
    app
}

#[test]
fn four_tabs_keep_the_local_contract_order() {
    assert_eq!(
        Tab::ALL.map(Tab::title),
        ["1 Status", "2 Work", "3 Timeline", "4 Health"]
    );
    let mut app = App::default();
    for expected in [Tab::Work, Tab::Timeline, Tab::Health, Tab::Status] {
        app.cycle_tab(false);
        assert_eq!(app.tab, expected);
    }
}

#[test]
fn every_view_renders_from_one_recorder_refresh() {
    for tab in Tab::ALL {
        let app = app_with_snapshot(tab);
        let rendered = render_text(&app, 120, 36);
        assert!(!rendered.contains("Loading"), "{tab:?}: {rendered}");
        assert!(rendered.contains("ExampleProject"), "{tab:?}: {rendered}");
    }
}

#[test]
fn status_view_surfaces_local_repository_work_loops_and_errors() {
    let mut app = app_with_snapshot(Tab::Status);
    let rendered = render_text(&app, 120, 36);
    for expected in [
        "Repository",
        "Open plans",
        "Gate snapshots",
        "Loops",
        "Collection",
    ] {
        assert!(
            rendered.contains(expected),
            "missing {expected:?}: {rendered}"
        );
    }

    let mut status = scenarios::status_snapshot();
    status.errors.push(crate::dashboard::StatusCollectionError {
        scope: "loops".to_string(),
        code: "loop_status_unavailable".to_string(),
        message: "example failure".to_string(),
    });
    let recorder = scenarios::recorder_snapshot();
    app.accept_recorder_refresh(RecorderRefresh {
        status_local: StatusLocalSnapshot {
            epoch_id: recorder.epoch_id,
            observed_at_ms: status.observed_at_ms,
            repository: status.repository,
            work: status.work,
            loops: status.loops,
            errors: status.errors,
        },
        recorder,
    });
    let rendered = render_text(&app, 120, 36);
    assert!(rendered.contains("loop_status_unavailable"));
    assert!(rendered.contains("example failure"));
}

#[test]
fn layout_tiers_cover_all_breakpoints() {
    assert_eq!(
        render::layout_tier(Rect::new(0, 0, 0, 0)),
        LayoutTier::Micro
    );
    assert_eq!(
        render::layout_tier(Rect::new(0, 0, 39, 11)),
        LayoutTier::Micro
    );
    assert_eq!(
        render::layout_tier(Rect::new(0, 0, 40, 12)),
        LayoutTier::Compact
    );
    assert_eq!(
        render::layout_tier(Rect::new(0, 0, 72, 20)),
        LayoutTier::Standard
    );
    assert_eq!(
        render::layout_tier(Rect::new(0, 0, 108, 24)),
        LayoutTier::Wide
    );
}

fn normalized(output: &str) -> String {
    output.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn render_text(app: &App, width: u16, height: u16) -> String {
    let backend = TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal.draw(|frame| render::draw(frame, app)).unwrap();
    normalized(&terminal.backend().to_string())
}
