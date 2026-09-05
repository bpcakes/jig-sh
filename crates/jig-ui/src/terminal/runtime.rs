use std::{
    sync::Arc,
    time::{Duration, Instant},
};

use super::{
    DashboardOptions, InitialTab,
    model::{App, Tab},
    render,
};
use crate::dashboard::{
    DashboardSource, RecorderMode, RecorderRefresh, RecorderRequest, SourceError, StatusRefresh,
    StatusRequest, TimelineLimit,
};
use anyhow::{Context, Result, bail};
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers};
use jig_tui::{CooperativeWorker, TerminalSession, is_actionable_key, require_terminal};

const EVENT_POLL_INTERVAL: Duration = Duration::from_millis(100);

pub(crate) fn run(source: impl DashboardSource + 'static, options: DashboardOptions) -> Result<()> {
    validate_refresh_interval(options.refresh_interval)?;
    require_terminal(
        "Jig dashboard",
        "use `jig ui --json` for recorder data or `jig status --json` for provider data when redirecting",
    )?;
    let source: Arc<dyn DashboardSource> = Arc::new(source);
    let mut terminal = TerminalSession::enter("Jig dashboard")?;
    let mut app = App::new(match options.initial_tab {
        InitialTab::Status => Tab::Status,
        InitialTab::Work => Tab::Work,
    });
    let mut worker = Some(RefreshWorker::spawn(Arc::clone(&source), app.tab)?);
    app.domain_mut(app.tab).set_refreshing(true);
    let mut next_refresh = Instant::now() + options.refresh_interval;
    let mut dirty = true;

    loop {
        if dirty {
            terminal
                .draw(|frame| render::draw(frame, &app))
                .context("failed to draw the status TUI")?;
            dirty = false;
        }

        if let Some((tab, result)) = worker.as_mut().and_then(RefreshWorker::try_finish) {
            worker = None;
            apply_refresh_result(&mut app, tab, result);
            next_refresh = Instant::now() + options.refresh_interval;
            dirty = true;
        }

        let requested_tab = worker.is_none().then(|| {
            next_queued_refresh(&app)
                .or_else(|| (Instant::now() >= next_refresh).then_some(app.tab))
        });
        if let Some(Some(requested_tab)) = requested_tab {
            app.domain_mut(requested_tab).set_refresh_queued(false);
            app.domain_mut(requested_tab).set_refreshing(true);
            worker = Some(RefreshWorker::spawn(Arc::clone(&source), requested_tab)?);
            dirty = true;
        }

        if event::poll(EVENT_POLL_INTERVAL).context("failed to poll terminal input")? {
            match handle_event(
                &mut app,
                event::read().context("failed to read terminal input")?,
            ) {
                RuntimeAction::Ignore => {}
                RuntimeAction::Redraw => {
                    dirty = true;
                }
                RuntimeAction::TabChanged => {
                    if !app.domain_has_data(app.tab)
                        && worker
                            .as_ref()
                            .is_none_or(|active| !active.tab.same_domain(app.tab))
                    {
                        if worker.is_some() {
                            app.domain_mut(app.tab).set_refresh_queued(true);
                        } else {
                            app.domain_mut(app.tab).set_refreshing(true);
                            worker = Some(RefreshWorker::spawn(Arc::clone(&source), app.tab)?);
                        }
                    }
                    dirty = true;
                }
                RuntimeAction::Refresh => {
                    if worker.is_some() {
                        app.domain_mut(app.tab).set_refresh_queued(true);
                    } else {
                        app.domain_mut(app.tab).set_refreshing(true);
                        worker = Some(RefreshWorker::spawn(Arc::clone(&source), app.tab)?);
                    }
                    dirty = true;
                }
                RuntimeAction::RefreshAll => {
                    app.domain_mut(Tab::Work).set_refresh_queued(true);
                    app.domain_mut(Tab::Status).set_refresh_queued(true);
                    if worker.is_none() {
                        let requested_tab = next_queued_refresh(&app)
                            .expect("refresh all always queues both dashboard domains");
                        app.domain_mut(requested_tab).set_refresh_queued(false);
                        app.domain_mut(requested_tab).set_refreshing(true);
                        worker = Some(RefreshWorker::spawn(Arc::clone(&source), requested_tab)?);
                    }
                    dirty = true;
                }
                RuntimeAction::Quit => {
                    if let Some(mut active) = worker.take() {
                        active.cancel_and_join();
                    }
                    return Ok(());
                }
            }
        }
    }
}

fn validate_refresh_interval(interval: Duration) -> Result<()> {
    if interval < Duration::from_secs(1) {
        bail!("the dashboard refresh interval must be at least one second");
    }
    Ok(())
}

fn apply_refresh_result(app: &mut App, tab: Tab, result: Result<RefreshResult, SourceError>) {
    app.domain_mut(tab).set_refreshing(false);
    match result {
        Ok(RefreshResult::Status(refresh)) => app.accept_status_refresh(refresh),
        Ok(RefreshResult::Recorder(refresh)) => app.accept_recorder_refresh(refresh),
        Err(error) => app.accept_error(tab, error.to_string()),
    }
}

fn next_queued_refresh(app: &App) -> Option<Tab> {
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RuntimeAction {
    Ignore,
    Redraw,
    TabChanged,
    Refresh,
    RefreshAll,
    Quit,
}

fn handle_event(app: &mut App, event: Event) -> RuntimeAction {
    match event {
        Event::Key(key) if is_actionable_key(key) => handle_key(app, key),
        Event::Resize(_, _) => RuntimeAction::Redraw,
        _ => RuntimeAction::Ignore,
    }
}

fn handle_key(app: &mut App, key: KeyEvent) -> RuntimeAction {
    if key.code == KeyCode::Char('q')
        || (key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL))
    {
        return RuntimeAction::Quit;
    }
    if app.package_detail_is_open() {
        return match key.code {
            KeyCode::Esc | KeyCode::Enter | KeyCode::Backspace => {
                app.close_package_detail();
                RuntimeAction::Redraw
            }
            KeyCode::Char('r') => RuntimeAction::Refresh,
            KeyCode::Char('R') => RuntimeAction::RefreshAll,
            KeyCode::Up | KeyCode::Char('k') => {
                app.scroll_package_detail(-1);
                RuntimeAction::Redraw
            }
            KeyCode::Down | KeyCode::Char('j') => {
                app.scroll_package_detail(1);
                RuntimeAction::Redraw
            }
            KeyCode::PageUp => {
                app.scroll_package_detail(-8);
                RuntimeAction::Redraw
            }
            KeyCode::PageDown => {
                app.scroll_package_detail(8);
                RuntimeAction::Redraw
            }
            KeyCode::Home => {
                app.move_package_detail_to_edge(false);
                RuntimeAction::Redraw
            }
            KeyCode::End => {
                app.move_package_detail_to_edge(true);
                RuntimeAction::Redraw
            }
            _ => RuntimeAction::Ignore,
        };
    }
    if key.code == KeyCode::Esc {
        return RuntimeAction::Quit;
    }
    match key.code {
        KeyCode::Char('r') => RuntimeAction::Refresh,
        KeyCode::Char('R') => RuntimeAction::RefreshAll,
        KeyCode::Enter => {
            if app.open_package_detail() {
                RuntimeAction::Redraw
            } else {
                RuntimeAction::Ignore
            }
        }
        KeyCode::Tab => {
            app.cycle_tab(false);
            RuntimeAction::TabChanged
        }
        KeyCode::BackTab => {
            app.cycle_tab(true);
            RuntimeAction::TabChanged
        }
        KeyCode::Left => {
            app.cycle_tab(true);
            RuntimeAction::TabChanged
        }
        KeyCode::Right => {
            app.cycle_tab(false);
            RuntimeAction::TabChanged
        }
        KeyCode::Char('1') => {
            app.select_tab(Tab::Status);
            RuntimeAction::TabChanged
        }
        KeyCode::Char('2') => {
            app.select_tab(Tab::Packages);
            RuntimeAction::TabChanged
        }
        KeyCode::Char('3') => {
            app.select_tab(Tab::Blockers);
            RuntimeAction::TabChanged
        }
        KeyCode::Char('4') => {
            app.select_tab(Tab::Work);
            RuntimeAction::TabChanged
        }
        KeyCode::Char('5') => {
            app.select_tab(Tab::Timeline);
            RuntimeAction::TabChanged
        }
        KeyCode::Char('6') => {
            app.select_tab(Tab::Health);
            RuntimeAction::TabChanged
        }
        KeyCode::Char('[') if app.tab.is_status_domain() => {
            app.switch_provider(true);
            RuntimeAction::Redraw
        }
        KeyCode::Char(']') if app.tab.is_status_domain() => {
            app.switch_provider(false);
            RuntimeAction::Redraw
        }
        KeyCode::Up | KeyCode::Char('k') => {
            app.move_selection(-1);
            RuntimeAction::Redraw
        }
        KeyCode::Down | KeyCode::Char('j') => {
            app.move_selection(1);
            RuntimeAction::Redraw
        }
        KeyCode::PageUp => {
            app.move_selection(-10);
            RuntimeAction::Redraw
        }
        KeyCode::PageDown => {
            app.move_selection(10);
            RuntimeAction::Redraw
        }
        KeyCode::Home => {
            app.move_to_edge(false);
            RuntimeAction::Redraw
        }
        KeyCode::End => {
            app.move_to_edge(true);
            RuntimeAction::Redraw
        }
        KeyCode::Char('b') if app.tab == Tab::Packages => {
            app.toggle_blocked_only();
            RuntimeAction::Redraw
        }
        _ => RuntimeAction::Ignore,
    }
}

enum RefreshResult {
    Status(StatusRefresh),
    Recorder(RecorderRefresh),
}

struct RefreshWorker {
    tab: Tab,
    worker: CooperativeWorker<Result<RefreshResult, SourceError>>,
}

impl RefreshWorker {
    fn spawn(source: Arc<dyn DashboardSource>, tab: Tab) -> Result<Self> {
        CooperativeWorker::spawn("jig-dashboard-refresh", move |cancelled| {
            let is_cancelled = || cancelled.is_cancelled();
            if matches!(tab, Tab::Status | Tab::Packages | Tab::Blockers) {
                source
                    .status(
                        StatusRequest {
                            timeline_limit: TimelineLimit::DEFAULT,
                        },
                        &|_| {},
                        &is_cancelled,
                    )
                    .map(RefreshResult::Status)
            } else {
                source
                    .recorder(
                        RecorderRequest {
                            mode: RecorderMode::Refresh,
                            timeline_limit: TimelineLimit::DEFAULT,
                        },
                        &is_cancelled,
                    )
                    .map(RefreshResult::Recorder)
            }
        })
        .map(|worker| Self { tab, worker })
    }

    fn try_finish(&mut self) -> Option<(Tab, Result<RefreshResult, SourceError>)> {
        self.worker
            .try_finish()
            .map(|result| match result {
                Ok(value) => value,
                Err(message) => Err(SourceError::InternalContract { message }),
            })
            .map(|result| (self.tab, result))
    }

    fn cancel_and_join(&mut self) {
        self.worker.cancel_and_join();
    }
}

#[cfg(test)]
mod tests {
    use jig_tui::require_terminal_with_state;

    use super::*;

    #[test]
    fn terminal_requirement_explains_each_redirect_case() {
        let require = |stdin, stdout| {
            require_terminal_with_state(
                "Jig dashboard",
                "use `jig ui --json` for recorder data or `jig status --json` for provider data when redirecting",
                stdin,
                stdout,
            )
        };
        assert!(require(true, true).is_ok());
        assert!(
            require(false, true)
                .unwrap_err()
                .to_string()
                .contains("terminal input")
        );
        assert!(
            require(true, false)
                .unwrap_err()
                .to_string()
                .contains("terminal output")
        );
        assert!(
            require(false, false)
                .unwrap_err()
                .to_string()
                .contains("terminal input and output")
        );
    }

    #[test]
    fn keys_cover_tabs_navigation_refresh_and_quit() {
        let mut app = App::default();
        assert_eq!(
            handle_key(&mut app, KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE)),
            RuntimeAction::TabChanged
        );
        assert_eq!(app.tab, Tab::Packages);
        for (key, tab) in [
            ('1', Tab::Status),
            ('2', Tab::Packages),
            ('3', Tab::Blockers),
            ('4', Tab::Work),
            ('5', Tab::Timeline),
            ('6', Tab::Health),
        ] {
            assert_eq!(
                handle_key(
                    &mut app,
                    KeyEvent::new(KeyCode::Char(key), KeyModifiers::NONE)
                ),
                RuntimeAction::TabChanged
            );
            assert_eq!(app.tab, tab);
        }
        assert_eq!(
            handle_key(
                &mut app,
                KeyEvent::new(KeyCode::Char('r'), KeyModifiers::NONE)
            ),
            RuntimeAction::Refresh
        );
        assert_eq!(
            handle_key(
                &mut app,
                KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL)
            ),
            RuntimeAction::Quit
        );
    }

    #[test]
    fn ignored_and_release_keys_do_not_request_a_redraw() {
        assert_eq!(
            handle_key(
                &mut App::default(),
                KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE)
            ),
            RuntimeAction::Ignore
        );
        assert_eq!(
            handle_event(
                &mut App::default(),
                Event::Key(KeyEvent::new_with_kind(
                    KeyCode::Char('1'),
                    KeyModifiers::NONE,
                    crossterm::event::KeyEventKind::Release,
                ))
            ),
            RuntimeAction::Ignore
        );
    }

    #[test]
    fn resize_events_request_an_immediate_redraw() {
        let mut app = App::default();

        assert_eq!(
            handle_event(&mut app, Event::Resize(120, 40)),
            RuntimeAction::Redraw
        );
    }

    #[test]
    fn only_tab_changes_request_lazy_domain_loading() {
        let mut app = App::default();
        assert_eq!(
            handle_event(&mut app, Event::Resize(80, 24)),
            RuntimeAction::Redraw
        );
        assert_eq!(
            handle_key(
                &mut app,
                KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE)
            ),
            RuntimeAction::Redraw
        );
        assert_eq!(
            handle_key(
                &mut app,
                KeyEvent::new(KeyCode::Char('4'), KeyModifiers::NONE)
            ),
            RuntimeAction::TabChanged
        );
    }

    #[test]
    fn refresh_interval_enforces_the_documented_one_second_floor() {
        assert!(validate_refresh_interval(Duration::from_secs(1)).is_ok());
        let error = validate_refresh_interval(Duration::from_millis(999)).unwrap_err();
        assert!(error.to_string().contains("at least one second"));
    }

    #[test]
    fn queued_refresh_prefers_the_visible_domain() {
        let mut app = App::default();
        app.domain_mut(Tab::Status).set_refresh_queued(true);
        app.domain_mut(Tab::Work).set_refresh_queued(true);
        assert_eq!(next_queued_refresh(&app), Some(Tab::Status));

        app.select_tab(Tab::Work);
        assert_eq!(next_queued_refresh(&app), Some(Tab::Work));
    }

    #[test]
    fn worker_errors_are_attributed_to_the_completed_domain() {
        let mut app = App::new(Tab::Work);
        app.domain_mut(Tab::Status).set_refreshing(true);
        apply_refresh_result(
            &mut app,
            Tab::Status,
            Err(SourceError::InternalContract {
                message: "status collection failed".to_string(),
            }),
        );

        assert_eq!(
            app.status.error.as_deref(),
            Some("dashboard contract failed: status collection failed")
        );
        assert!(app.recorder.error.is_none());
        assert!(!app.status.refreshing);
    }

    #[test]
    fn enter_opens_package_detail_and_escape_returns_before_quitting() {
        let mut app = App::default();
        app.accept_snapshot(serde_json::json!({
            "schema_version": 1,
            "repository": {},
            "providers": [{
                "id": "provider",
                "report": {
                    "work_packages": [{
                        "id": "WP-001",
                        "specification": {},
                        "implementation": {},
                        "verification": {}
                    }]
                }
            }]
        }));
        app.select_tab(Tab::Packages);

        assert_eq!(
            handle_key(&mut app, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
            RuntimeAction::Redraw
        );
        assert!(app.package_detail_is_open());
        assert_eq!(
            handle_key(&mut app, KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)),
            RuntimeAction::Redraw
        );
        assert!(!app.package_detail_is_open());
        assert_eq!(
            handle_key(&mut app, KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)),
            RuntimeAction::Quit
        );
    }

    struct PtySource;

    impl DashboardSource for PtySource {
        fn recorder(
            &self,
            _request: RecorderRequest,
            _cancelled: &dyn Fn() -> bool,
        ) -> Result<RecorderRefresh, SourceError> {
            Err(SourceError::InternalContract {
                message: "unexpected recorder request in status PTY fixture".to_string(),
            })
        }

        fn status(
            &self,
            _request: StatusRequest,
            phase_changed: &dyn Fn(crate::dashboard::StatusPhase),
            _cancelled: &dyn Fn() -> bool,
        ) -> Result<StatusRefresh, SourceError> {
            phase_changed(crate::dashboard::StatusPhase::Providers);
            phase_changed(crate::dashboard::StatusPhase::LocalEpoch);
            Ok(StatusRefresh {
                status: crate::dashboard::scenarios::status_snapshot(),
                recorder: crate::dashboard::scenarios::recorder_snapshot(),
            })
        }

        fn plan(
            &self,
            _basis: crate::dashboard::PlanBasis,
            _plan_id: String,
            _cancelled: &dyn Fn() -> bool,
        ) -> Result<crate::dashboard::PlanSnapshotResult, SourceError> {
            Ok(crate::dashboard::PlanSnapshotResult::NotFound)
        }
    }

    #[test]
    #[cfg(unix)]
    fn pty_child_runs_dashboard() {
        if std::env::var_os("JIG_UI_PTY_CHILD").is_none() {
            return;
        }
        run(
            PtySource,
            DashboardOptions::new(InitialTab::Status, Duration::from_secs(3_600)),
        )
        .unwrap();
    }

    #[test]
    #[cfg(unix)]
    fn pty_entrypoint_enters_draws_and_restores_after_quit() {
        use std::{
            fs::File,
            io::{Read, Write},
            os::fd::FromRawFd,
            process::{Command, Stdio},
            thread,
        };

        let mut controller_fd = -1;
        let mut terminal_fd = -1;
        let window = libc::winsize {
            ws_row: 24,
            ws_col: 80,
            ws_xpixel: 0,
            ws_ypixel: 0,
        };
        // SAFETY: openpty initializes both owned descriptors on success. Each
        // descriptor is wrapped exactly once below and closed by File/Stdio.
        let result = unsafe {
            libc::openpty(
                &mut controller_fd,
                &mut terminal_fd,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                &raw const window,
            )
        };
        assert_eq!(
            result,
            0,
            "openpty failed: {}",
            std::io::Error::last_os_error()
        );
        // SAFETY: successful openpty returned distinct owned descriptors.
        let mut controller = unsafe { File::from_raw_fd(controller_fd) };
        // SAFETY: successful openpty returned the owned slave descriptor.
        let terminal = unsafe { File::from_raw_fd(terminal_fd) };
        let stdin = Stdio::from(terminal.try_clone().unwrap());
        let stdout = Stdio::from(terminal.try_clone().unwrap());
        let stderr = Stdio::from(terminal);
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "terminal::runtime::tests::pty_child_runs_dashboard",
                "--nocapture",
            ])
            .env("JIG_UI_PTY_CHILD", "1")
            .stdin(stdin)
            .stdout(stdout)
            .stderr(stderr)
            .spawn()
            .unwrap();
        let mut reader = controller.try_clone().unwrap();
        let output_reader = thread::spawn(move || {
            let mut output = Vec::new();
            let mut buffer = [0_u8; 1_024];
            loop {
                match reader.read(&mut buffer) {
                    Ok(0) => break,
                    Ok(count) => output.extend_from_slice(&buffer[..count]),
                    Err(error) if error.raw_os_error() == Some(libc::EIO) => break,
                    Err(error) => panic!("failed reading PTY output: {error}"),
                }
            }
            output
        });

        let deadline = Instant::now() + Duration::from_secs(5);
        let mut next_quit = Instant::now();
        let status = loop {
            if let Some(status) = child.try_wait().unwrap() {
                break status;
            }
            if Instant::now() >= deadline {
                child.kill().unwrap();
                let _ = child.wait();
                panic!("terminal dashboard did not exit after q");
            }
            if Instant::now() >= next_quit {
                match controller.write_all(b"q") {
                    Ok(()) => {}
                    Err(error)
                        if matches!(
                            error.kind(),
                            std::io::ErrorKind::BrokenPipe | std::io::ErrorKind::WouldBlock
                        ) || error.raw_os_error() == Some(libc::EIO) => {}
                    Err(error) => panic!("failed writing PTY input: {error}"),
                }
                next_quit = Instant::now() + Duration::from_millis(100);
            }
            thread::sleep(Duration::from_millis(10));
        };
        assert!(status.success(), "PTY child failed with {status}");
        let output = output_reader.join().unwrap();
        let output = String::from_utf8_lossy(&output);
        assert!(
            output.contains("\u{1b}[?1049h"),
            "dashboard did not enter the alternate screen"
        );
        assert!(
            output.contains("\u{1b}[?1049l"),
            "alternate screen was not restored"
        );
        assert!(
            output.contains("Jig"),
            "dashboard did not render into the PTY"
        );
    }
}
