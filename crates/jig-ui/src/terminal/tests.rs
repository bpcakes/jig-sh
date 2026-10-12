use jig_dashboard::{RecorderRefresh, StatusLocalSnapshot, StatusSnapshot, scenarios};
use ratatui::{Terminal, backend::TestBackend, layout::Rect};

use super::{
    model::{App, Tab},
    render::{self, LayoutTier},
};

mod local;

fn status_local(
    status: StatusSnapshot,
    recorder: &jig_dashboard::RecorderSnapshot,
) -> StatusLocalSnapshot {
    StatusLocalSnapshot {
        epoch_id: recorder.epoch_id,
        observed_at_ms: status.observed_at_ms,
        repository: status.repository,
        loops: status.loops,
        errors: status.errors,
    }
}

fn app_with_snapshot(tab: Tab) -> App {
    let recorder = scenarios::recorder_snapshot();
    let mut app = App::new(tab);
    app.accept_recorder_refresh(RecorderRefresh {
        status_local: status_local(scenarios::status_snapshot(), &recorder),
        recorder,
    });
    app
}

#[test]
fn three_tabs_keep_the_local_contract_order() {
    assert_eq!(
        Tab::ALL.map(Tab::title),
        ["1 Status", "2 Timeline", "3 Health"]
    );
    let mut app = App::default();
    for expected in [Tab::Timeline, Tab::Health, Tab::Status] {
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
fn status_view_surfaces_local_repository_harness_loops_and_errors() {
    let mut app = app_with_snapshot(Tab::Status);
    let rendered = render_text(&app, 120, 36);
    for expected in [
        "Repository",
        "clean",
        "origin/main · ahead 0 · behind 0 · in_sync",
        "Harness",
        "Runtime: 0.3.0 · contract 8",
        "Source: /example/source",
        "Loops",
        "Workflows: 1",
        "Exhausted: 1",
        "Collection",
        "All local observations completed.",
    ] {
        assert!(
            rendered.contains(expected),
            "missing {expected:?}: {rendered}"
        );
    }
    for removed in ["Open plans", "Gate snapshots", "Current session"] {
        assert!(!rendered.contains(removed), "found {removed:?}: {rendered}");
    }

    let mut status = scenarios::status_snapshot();
    status.errors.push(jig_dashboard::StatusCollectionError {
        scope: "loops".to_string(),
        code: "loop_status_unavailable".to_string(),
        message: "example failure".to_string(),
    });
    let recorder = scenarios::recorder_snapshot();
    app.accept_recorder_refresh(RecorderRefresh {
        status_local: status_local(status, &recorder),
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
