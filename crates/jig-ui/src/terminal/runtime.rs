use std::time::Duration;
#[cfg(test)]
use std::time::Instant;

use super::{
    DashboardOptions, InitialTab,
    model::{App, Tab},
    render,
};
use crate::dashboard::DashboardSource;
#[cfg(test)]
use crate::dashboard::{
    RecorderRefresh, RecorderRequest, SourceError, StatusRefresh, StatusRequest,
};
use anyhow::{Result, bail};
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers};
use jig_tui::{TerminalSession, is_actionable_key, require_terminal};

mod event_loop;
mod scheduler;
mod worker;

const EVENT_POLL_INTERVAL: Duration = Duration::from_millis(100);

pub(crate) fn run(source: impl DashboardSource + 'static, options: DashboardOptions) -> Result<()> {
    run_with_cancellation(source, options, || false)
}

pub(crate) fn run_with_cancellation(
    source: impl DashboardSource + 'static,
    options: DashboardOptions,
    externally_cancelled: impl Fn() -> bool,
) -> Result<()> {
    validate_refresh_interval(options.local_refresh_interval)?;
    validate_refresh_interval(options.status_refresh_interval)?;
    require_terminal(
        "Jig dashboard",
        "use `jig ui --json` for recorder data or `jig status --json` for provider data when redirecting",
    )?;
    let mut terminal = TerminalSession::enter("Jig dashboard")?;
    let mut app = App::new(match options.initial_tab {
        InitialTab::Status => Tab::Status,
        InitialTab::Work => Tab::Work,
    });
    if let Some(plan_id) = options.initial_plan.clone() {
        app.request_initial_plan(plan_id);
    }
    event_loop::run(&mut terminal, source, app, options, externally_cancelled)
}

fn validate_refresh_interval(interval: Duration) -> Result<()> {
    if interval < Duration::from_secs(1) {
        bail!("the dashboard refresh interval must be at least one second");
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RuntimeAction {
    Ignore,
    Redraw,
    TabChanged,
    Refresh,
    RefreshDetail,
    RefreshAll,
    DetailRequested,
    GrowTimeline,
    ShrinkTimeline,
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
    if app.detail_is_open() {
        return match key.code {
            KeyCode::Esc | KeyCode::Backspace => {
                app.close_detail();
                RuntimeAction::Redraw
            }
            KeyCode::Enter => {
                app.open_detail_leaf_or_close();
                RuntimeAction::Redraw
            }
            KeyCode::Tab => {
                app.cycle_detail_section(false);
                RuntimeAction::Redraw
            }
            KeyCode::BackTab => {
                app.cycle_detail_section(true);
                RuntimeAction::Redraw
            }
            KeyCode::Up | KeyCode::Char('k') => {
                app.move_detail_selection(-1);
                RuntimeAction::Redraw
            }
            KeyCode::Down | KeyCode::Char('j') => {
                app.move_detail_selection(1);
                RuntimeAction::Redraw
            }
            KeyCode::Left | KeyCode::Char('h') => {
                app.scroll_detail_horizontal(-4);
                RuntimeAction::Redraw
            }
            KeyCode::Right | KeyCode::Char('l') => {
                app.scroll_detail_horizontal(4);
                RuntimeAction::Redraw
            }
            KeyCode::PageUp => {
                app.move_detail_selection(-8);
                RuntimeAction::Redraw
            }
            KeyCode::PageDown => {
                app.move_detail_selection(8);
                RuntimeAction::Redraw
            }
            KeyCode::Home => {
                app.move_detail_to_edge(false);
                RuntimeAction::Redraw
            }
            KeyCode::End => {
                app.move_detail_to_edge(true);
                RuntimeAction::Redraw
            }
            KeyCode::Char('r') => {
                if app.detail.target_plan_id.is_some() {
                    RuntimeAction::RefreshDetail
                } else {
                    RuntimeAction::Refresh
                }
            }
            KeyCode::Char('R') => RuntimeAction::RefreshAll,
            _ => RuntimeAction::Ignore,
        };
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
            } else if app.open_selected_detail() {
                RuntimeAction::DetailRequested
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
        KeyCode::Char('f') if app.tab == Tab::Timeline => {
            app.cycle_timeline_filter(false);
            RuntimeAction::Redraw
        }
        KeyCode::Char('F') if app.tab == Tab::Timeline => {
            app.cycle_timeline_filter(true);
            RuntimeAction::Redraw
        }
        KeyCode::Char('+') if app.tab == Tab::Timeline => RuntimeAction::GrowTimeline,
        KeyCode::Char('-') if app.tab == Tab::Timeline => RuntimeAction::ShrinkTimeline,
        _ => RuntimeAction::Ignore,
    }
}

#[cfg(test)]
mod tests {
    static PTY_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    static PTY_PROVIDER_STARTED: std::sync::atomic::AtomicBool =
        std::sync::atomic::AtomicBool::new(false);

    use jig_tui::require_terminal_with_state;

    use super::*;
    use super::{
        scheduler::{ScheduledRequest, WorkKind},
        worker::apply_refresh_result,
    };

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
    fn local_keys_filter_open_and_navigate_plan_detail() {
        let mut app = App::new(Tab::Timeline);
        app.recorder.data = Some(crate::dashboard::scenarios::recorder_snapshot().into());
        assert_eq!(
            handle_key(
                &mut app,
                KeyEvent::new(KeyCode::Char('f'), KeyModifiers::NONE)
            ),
            RuntimeAction::Redraw
        );
        assert_eq!(
            app.timeline_filter,
            crate::terminal::model::TimelineFilter::Receipts
        );
        assert_eq!(
            handle_key(&mut app, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
            RuntimeAction::DetailRequested
        );
        let (basis, plan_id) = app.take_plan_request().unwrap();
        app.accept_plan_result(
            basis,
            &plan_id,
            crate::dashboard::PlanSnapshotResult::Found(Box::new(
                crate::dashboard::scenarios::plan_snapshot(),
            )),
        );
        assert_eq!(
            handle_key(&mut app, KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE)),
            RuntimeAction::Redraw
        );
        assert_eq!(
            app.detail.section,
            crate::terminal::model::PlanSection::Body
        );
        assert_eq!(
            handle_key(&mut app, KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)),
            RuntimeAction::Redraw
        );
        assert!(!app.detail_is_open());
    }

    #[test]
    fn refresh_retries_a_failed_plan_target() {
        let mut app = App::default();
        app.detail.request_plan("plan_example".to_string());
        app.accept_plan_error("plan_example", "detail collection failed".to_string());

        assert_eq!(
            handle_key(
                &mut app,
                KeyEvent::new(KeyCode::Char('r'), KeyModifiers::NONE)
            ),
            RuntimeAction::RefreshDetail
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
    fn worker_errors_are_attributed_to_the_completed_domain() {
        let mut app = App::new(Tab::Work);
        app.domain_mut(Tab::Status).set_refreshing(true);
        apply_refresh_result(
            &mut app,
            &ScheduledRequest {
                generation: 1,
                sequence: 1,
                kind: WorkKind::Status(StatusRequest {
                    timeline_limit: crate::dashboard::TimelineLimit::DEFAULT,
                }),
            },
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
            cancelled: &dyn Fn() -> bool,
        ) -> Result<StatusRefresh, SourceError> {
            phase_changed(crate::dashboard::StatusPhase::Providers);
            if std::env::var_os("JIG_UI_PTY_BLOCK_PROVIDER").is_some() {
                PTY_PROVIDER_STARTED.store(true, std::sync::atomic::Ordering::SeqCst);
                while !cancelled() {
                    std::thread::sleep(Duration::from_millis(5));
                }
                eprintln!("JIG_UI_WORKER_CLEANED");
                return Err(SourceError::Cancelled);
            }
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
        let options = DashboardOptions::new(InitialTab::Status, Duration::from_secs(3_600));
        if std::env::var_os("JIG_UI_PTY_EXTERNAL_CANCEL").is_some() {
            run_with_cancellation(PtySource, options, || {
                PTY_PROVIDER_STARTED.load(std::sync::atomic::Ordering::SeqCst)
            })
            .unwrap();
        } else {
            run(PtySource, options).unwrap();
        }
    }

    #[cfg(unix)]
    fn pty_dashboard_output(
        block_provider: bool,
        external_cancel: bool,
        quit_input: Option<&[u8]>,
    ) -> String {
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
        let mut command = Command::new(std::env::current_exe().unwrap());
        command
            .args([
                "--exact",
                "terminal::runtime::tests::pty_child_runs_dashboard",
                "--nocapture",
            ])
            .env("JIG_UI_PTY_CHILD", "1")
            .stdin(stdin)
            .stdout(stdout)
            .stderr(stderr);
        if block_provider {
            command.env("JIG_UI_PTY_BLOCK_PROVIDER", "1");
        }
        if external_cancel {
            command.env("JIG_UI_PTY_EXTERNAL_CANCEL", "1");
        }
        let mut child = command.spawn().unwrap();
        // `Command` retains its configured `Stdio` handles after spawning. Drop
        // those parent-side slave descriptors so the controller observes EOF
        // as soon as the child exits.
        drop(command);
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
            if let Some(quit_input) = quit_input.filter(|_| Instant::now() >= next_quit) {
                match controller.write_all(quit_input) {
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
        String::from_utf8_lossy(&output).into_owned()
    }

    #[test]
    #[cfg(unix)]
    fn pty_entrypoint_enters_draws_and_restores_after_quit() {
        let _guard = PTY_TEST_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let output = pty_dashboard_output(false, false, Some(b"q"));
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

    #[test]
    #[cfg(unix)]
    fn ctrl_c_restores_the_terminal() {
        let _guard = PTY_TEST_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let output = pty_dashboard_output(false, false, Some(b"\x03"));
        assert!(output.contains("\u{1b}[?1049l"));
    }

    #[test]
    #[cfg(unix)]
    fn quit_joins_provider_worker_before_restoring_the_terminal() {
        let _guard = PTY_TEST_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let output = pty_dashboard_output(true, false, Some(b"q"));
        let cleaned = output
            .find("JIG_UI_WORKER_CLEANED")
            .expect("provider fixture did not observe cancellation");
        let restored = output
            .rfind("\u{1b}[?1049l")
            .expect("alternate screen was not restored");
        assert!(
            cleaned < restored,
            "worker cleanup happened after restoration"
        );
    }

    #[test]
    #[cfg(unix)]
    fn external_cancellation_joins_worker_before_terminal_restoration() {
        let _guard = PTY_TEST_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let output = pty_dashboard_output(true, true, None);
        let cleaned = output.find("JIG_UI_WORKER_CLEANED").unwrap();
        let restored = output.rfind("\u{1b}[?1049l").unwrap();
        assert!(cleaned < restored);
    }
}
