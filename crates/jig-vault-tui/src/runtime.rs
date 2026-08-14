use std::{
    sync::Arc,
    time::{Duration, Instant},
};

use anyhow::{Context, Result};
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers};
use jig_tui::{CooperativeWorker, TerminalSession, is_actionable_key, require_terminal};
use jig_vault::{SecretBytes, VaultReference, VaultSnapshot};

use crate::{
    VaultAction, VaultActionResult, VaultBackend, VaultPresence, VaultUiError, VaultUiErrorKind,
    model::{App, Focus, Screen},
    peek::{PEEK_BEGIN_MARKER, PEEK_END_MARKER, TerminalSafePreviewWriter},
    render,
};

const EVENT_POLL_INTERVAL: Duration = Duration::from_millis(50);
const SPINNER_INTERVAL: Duration = Duration::from_millis(100);
const DEFAULT_IDLE_LOCK_TIMEOUT: Duration = Duration::from_secs(5 * 60);
const PEEK_DISPLAY_TIMEOUT: Duration = Duration::from_secs(10);

pub(crate) fn run(
    backend: impl VaultBackend,
    initial_passphrase: Option<SecretBytes>,
    cancelled: impl Fn() -> bool + Send + Sync + 'static,
) -> Result<()> {
    require_terminal(
        "jig vault tui",
        "use `jig vault field list` for non-interactive metadata",
    )?;
    let mut terminal = TerminalSession::enter_with_bracketed_paste("Vault TUI")?;
    let backend: Arc<dyn VaultBackend> = Arc::new(backend);
    let mut app = App::new(backend.descriptor());
    let external_cancellation: Arc<dyn Fn() -> bool + Send + Sync> = Arc::new(cancelled);
    let mut worker = None;
    if let Some(passphrase) = initial_passphrase {
        if app.descriptor.exists {
            app.begin_loading("Unlocking vault");
            worker = Some(ActionWorker::spawn(
                Arc::clone(&backend),
                BackendRequest::Unlock(passphrase),
            )?);
        }
    }
    let mut dirty = true;
    let mut next_spinner = Instant::now() + SPINNER_INTERVAL;
    let mut idle = IdleTimer::new(Instant::now(), DEFAULT_IDLE_LOCK_TIMEOUT);

    loop {
        if external_cancellation() {
            return finish(&mut terminal, &mut worker, &backend, &mut app);
        }

        if let Some(active) = &mut worker {
            if let Some(completion) = active.try_finish() {
                worker = None;
                apply_completion(&mut app, &backend, completion);
                dirty = true;
            }
        }

        let now = Instant::now();
        if idle.expired(now, app.is_unlocked()) {
            lock_after_inactivity(&mut terminal, &mut worker, &backend, &mut app)?;
            idle.record(now);
            dirty = true;
        }
        if matches!(app.screen, Screen::Loading(_)) && now >= next_spinner {
            app.tick = app.tick.wrapping_add(1);
            next_spinner = now + SPINNER_INTERVAL;
            dirty = true;
        }

        if dirty {
            terminal
                .draw(|frame| render::draw(frame, &app))
                .context("failed to draw the Vault TUI")?;
            dirty = false;
        }

        if event::poll(EVENT_POLL_INTERVAL).context("failed to poll Vault TUI input")? {
            let input = event::read().context("failed to read Vault TUI input")?;
            if matches!(&input, Event::Key(key) if is_actionable_key(*key))
                || matches!(&input, Event::Paste(_))
            {
                idle.record(Instant::now());
            }
            let action = match input {
                Event::Key(key) if is_actionable_key(key) => handle_key(&mut app, key),
                Event::Paste(value) => handle_paste(&mut app, &value),
                Event::Resize(_, _) => RuntimeAction::Redraw,
                _ => RuntimeAction::Ignore,
            };
            match action {
                RuntimeAction::Ignore => {}
                RuntimeAction::Redraw => dirty = true,
                RuntimeAction::Lock => {
                    lock_session(&mut terminal, &mut worker, &backend, &mut app)?;
                    idle.record(Instant::now());
                    dirty = true;
                }
                RuntimeAction::Start(request) => {
                    if worker.is_none() {
                        worker = Some(ActionWorker::spawn(Arc::clone(&backend), request)?);
                        dirty = true;
                    }
                }
                RuntimeAction::Peek(reference) => {
                    if worker.is_none() {
                        terminal
                            .draw(|frame| render::draw(frame, &app))
                            .context("failed to draw the Vault TUI preview state")?;
                        let result = run_controlled_peek(
                            &mut terminal,
                            &backend,
                            &reference,
                            &external_cancellation,
                        )?;
                        apply_peek_result(&mut app, &backend, result);
                        idle.record(Instant::now());
                        dirty = true;
                    }
                }
                RuntimeAction::Quit => {
                    return finish(&mut terminal, &mut worker, &backend, &mut app);
                }
            }
        }
    }
}

fn finish(
    terminal: &mut TerminalSession,
    worker: &mut Option<ActionWorker>,
    backend: &Arc<dyn VaultBackend>,
    app: &mut App,
) -> Result<()> {
    if let Some(worker) = worker {
        app.begin_loading("Finishing current vault operation");
        let draw_result = terminal
            .draw(|frame| render::draw(frame, app))
            .context("failed to draw the Vault TUI shutdown state");
        worker.cancel_and_join();
        backend.lock();
        draw_result?;
    } else {
        backend.lock();
    }
    Ok(())
}

fn lock_after_inactivity(
    terminal: &mut TerminalSession,
    worker: &mut Option<ActionWorker>,
    backend: &Arc<dyn VaultBackend>,
    app: &mut App,
) -> Result<()> {
    if let Some(mut active) = worker.take() {
        app.begin_loading("Finishing current vault operation before idle lock");
        terminal
            .draw(|frame| render::draw(frame, app))
            .context("failed to draw the Vault TUI idle-lock state")?;
        active.cancel_and_join();
    }
    backend.lock();
    app.lock_after_inactivity();
    Ok(())
}

fn lock_session(
    terminal: &mut TerminalSession,
    worker: &mut Option<ActionWorker>,
    backend: &Arc<dyn VaultBackend>,
    app: &mut App,
) -> Result<()> {
    if let Some(mut active) = worker.take() {
        app.begin_loading("Finishing current vault operation before lock");
        terminal
            .draw(|frame| render::draw(frame, app))
            .context("failed to draw the Vault TUI lock state")?;
        active.cancel_and_join();
    }
    backend.lock();
    app.lock();
    Ok(())
}

fn run_controlled_peek(
    terminal: &mut TerminalSession,
    backend: &Arc<dyn VaultBackend>,
    reference: &VaultReference,
    external_cancellation: &Arc<dyn Fn() -> bool + Send + Sync>,
) -> Result<std::result::Result<usize, VaultUiError>> {
    let direct_result = terminal
        .with_direct_output(|output| {
            write!(
                output,
                "{PEEK_BEGIN_MARKER}\r\nReference: {reference}\r\n\
                 This controlled preview may be captured by terminal scrollback, multiplexers, or screen recording.\r\n\r\n"
            )?;
            let result = {
                let mut writer = TerminalSafePreviewWriter::new(output);
                let result = backend.peek(reference, &mut writer);
                writer.finish()?;
                result
            };
            if result.is_err() {
                write!(
                    output,
                    "\r\n\r\nThe field could not be previewed. Details will be shown after this screen is cleared."
                )?;
            }
            write!(
                output,
                "\r\n\r\n{PEEK_END_MARKER}\r\nPress any key to clear now; otherwise this screen clears in ten seconds."
            )?;
            output.flush()?;
            Ok(result)
        })
        .context("failed to write the controlled Vault preview");

    let wait_result = if direct_result.is_ok() {
        wait_for_peek_dismissal(external_cancellation)
    } else {
        Ok(())
    };
    let clear_result = terminal
        .clear_direct_output()
        .context("failed to clear the controlled Vault preview");
    clear_result?;
    wait_result?;
    direct_result
}

fn wait_for_peek_dismissal(
    external_cancellation: &Arc<dyn Fn() -> bool + Send + Sync>,
) -> Result<()> {
    let deadline = Instant::now() + PEEK_DISPLAY_TIMEOUT;
    loop {
        if external_cancellation() || Instant::now() >= deadline {
            return Ok(());
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        if !event::poll(remaining.min(EVENT_POLL_INTERVAL))
            .context("failed to poll controlled Vault preview input")?
        {
            continue;
        }
        match event::read().context("failed to read controlled Vault preview input")? {
            Event::Key(key) if is_actionable_key(key) => return Ok(()),
            Event::Paste(_) => return Ok(()),
            _ => {}
        }
    }
}

fn apply_peek_result(
    app: &mut App,
    backend: &Arc<dyn VaultBackend>,
    result: std::result::Result<usize, VaultUiError>,
) {
    match result {
        Ok(bytes_written) => app.complete_peek(bytes_written),
        Err(error)
            if matches!(
                error.kind(),
                VaultUiErrorKind::Authentication | VaultUiErrorKind::Audit
            ) =>
        {
            backend.lock();
            app.fail_unlock(&error);
        }
        Err(error) => app.fail_action(&error),
    }
}

fn apply_completion(
    app: &mut App,
    backend: &Arc<dyn VaultBackend>,
    completion: Result<BackendCompletion, String>,
) {
    let completion = match completion {
        Ok(completion) => completion,
        Err(error) => {
            backend.lock();
            app.fail_unlock(&VaultUiError::new(VaultUiErrorKind::Other, error));
            return;
        }
    };
    match completion.outcome {
        BackendOutcome::Success(result) => apply_success(app, completion.kind, result),
        BackendOutcome::LifecycleFailure { error, presence } => {
            apply_lifecycle_failure(app, backend, error, presence);
        }
        BackendOutcome::Failure(failure) => {
            apply_failure(app, backend, completion.kind, failure);
        }
    }
}

fn apply_success(app: &mut App, kind: OperationKind, result: VaultActionResult) {
    match result {
        VaultActionResult::Snapshot(snapshot) => {
            app.apply_snapshot(snapshot);
            let message = match kind {
                OperationKind::Unlock => "Vault unlocked.",
                OperationKind::Initialize => "Vault initialized and unlocked.",
                OperationKind::Refresh => "Vault metadata refreshed.",
                OperationKind::Migrate => "Vault migrated to version 2.",
                OperationKind::Mutation => "Vault updated.",
                OperationKind::Import => "1Password import completed.",
                OperationKind::Passphrase => "Vault passphrase changed.",
                OperationKind::Activity
                | OperationKind::VerifyAudit
                | OperationKind::ImportPreview
                | OperationKind::ImportDiscard
                | OperationKind::Backup
                | OperationKind::Restore
                | OperationKind::Export => "Vault operation completed.",
            };
            app.set_info(message);
        }
        VaultActionResult::Activity(records) => app.apply_activity(records),
        VaultActionResult::Audit(verification) => app.apply_audit_result(verification),
        VaultActionResult::ImportPreview(preview) => app.apply_import_preview(preview),
        VaultActionResult::ImportDiscarded => app.finish_import_discard(),
        VaultActionResult::BackupCreated {
            output,
            bytes_written,
            backup_version,
            snapshot,
        } => {
            app.apply_snapshot(snapshot);
            app.set_info(&format!(
                "Encrypted backup v{backup_version} written to {} ({bytes_written} bytes).",
                output.display()
            ));
        }
        VaultActionResult::Restored { .. } => app.apply_restore(),
        VaultActionResult::Exported {
            output,
            bytes_written,
            snapshot,
        } => {
            app.apply_snapshot(snapshot);
            app.set_info(&format!(
                "Exported {bytes_written} bytes to private file {}.",
                output.display()
            ));
        }
    }
}

fn apply_failure(
    app: &mut App,
    backend: &Arc<dyn VaultBackend>,
    kind: OperationKind,
    failure: BackendFailure,
) {
    if matches!(
        failure.error().kind(),
        VaultUiErrorKind::Authentication | VaultUiErrorKind::Audit
    ) {
        backend.lock();
        app.fail_unlock(failure.error());
        return;
    }

    match failure {
        BackendFailure::Refreshed { error, snapshot } => {
            app.apply_recovery_snapshot(snapshot);
            app.set_error(error.message());
        }
        BackendFailure::RefreshFailed {
            error,
            refresh_error,
        } => {
            backend.lock();
            let error = VaultUiError::new(
                refresh_error.kind(),
                format!(
                    "{} Vault metadata recovery also failed: {}",
                    error.message(),
                    refresh_error.message()
                ),
            );
            app.fail_unlock(&error);
        }
        BackendFailure::Primary(error) => apply_primary_failure(app, kind, &error),
    }
}

fn apply_lifecycle_failure(
    app: &mut App,
    backend: &Arc<dyn VaultBackend>,
    error: VaultUiError,
    presence: Result<VaultPresence, VaultUiError>,
) {
    let session_invalid = matches!(
        error.kind(),
        VaultUiErrorKind::Authentication | VaultUiErrorKind::Audit
    );
    match presence {
        Ok(presence) => {
            if session_invalid {
                backend.lock();
            }
            app.fail_lifecycle(&error, presence);
        }
        Err(presence_error) => {
            backend.lock();
            let error = VaultUiError::new(
                presence_error.kind(),
                format!(
                    "{} Vault presence reconciliation also failed: {}",
                    error.message(),
                    presence_error.message()
                ),
            );
            app.fail_lifecycle(&error, VaultPresence::Present);
        }
    }
}

fn apply_primary_failure(app: &mut App, kind: OperationKind, error: &VaultUiError) {
    match kind {
        OperationKind::Unlock => app.fail_unlock(error),
        OperationKind::Initialize | OperationKind::Restore => {
            app.fail_lifecycle(error, VaultPresence::Present);
        }
        OperationKind::Refresh
        | OperationKind::Migrate
        | OperationKind::Mutation
        | OperationKind::Activity
        | OperationKind::VerifyAudit
        | OperationKind::ImportPreview
        | OperationKind::ImportDiscard
        | OperationKind::Import
        | OperationKind::Backup
        | OperationKind::Passphrase
        | OperationKind::Export => app.fail_action(error),
    }
}

#[derive(Debug)]
pub(crate) enum RuntimeAction {
    Ignore,
    Redraw,
    Lock,
    Start(BackendRequest),
    Peek(VaultReference),
    Quit,
}

#[derive(Debug)]
pub(crate) enum BackendRequest {
    Unlock(SecretBytes),
    Initialize(SecretBytes),
    Execute(VaultAction),
}

impl BackendRequest {
    const fn kind(&self) -> OperationKind {
        match self {
            Self::Unlock(_) => OperationKind::Unlock,
            Self::Initialize(_) => OperationKind::Initialize,
            Self::Execute(VaultAction::MigrateToV2) => OperationKind::Migrate,
            Self::Execute(VaultAction::Refresh) => OperationKind::Refresh,
            Self::Execute(VaultAction::Mutate { .. }) => OperationKind::Mutation,
            Self::Execute(VaultAction::Activity { .. }) => OperationKind::Activity,
            Self::Execute(VaultAction::VerifyAudit) => OperationKind::VerifyAudit,
            Self::Execute(VaultAction::PreviewOnePasswordImport { .. }) => {
                OperationKind::ImportPreview
            }
            Self::Execute(VaultAction::CommitOnePasswordImport { .. }) => OperationKind::Import,
            Self::Execute(VaultAction::DiscardOnePasswordImport { .. }) => {
                OperationKind::ImportDiscard
            }
            Self::Execute(VaultAction::CreateBackup { .. }) => OperationKind::Backup,
            Self::Execute(VaultAction::ChangePassphrase { .. }) => OperationKind::Passphrase,
            Self::Execute(VaultAction::RestoreBackup { .. }) => OperationKind::Restore,
            Self::Execute(VaultAction::ExportField { .. }) => OperationKind::Export,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum OperationKind {
    Unlock,
    Initialize,
    Refresh,
    Migrate,
    Mutation,
    Activity,
    VerifyAudit,
    ImportPreview,
    ImportDiscard,
    Import,
    Backup,
    Passphrase,
    Restore,
    Export,
}

impl OperationKind {
    const fn failure_recovery(self, error: &VaultUiError) -> FailureRecovery {
        match self {
            Self::Initialize | Self::Restore => FailureRecovery::ReconcilePresence,
            Self::Migrate
            | Self::Mutation
            | Self::Import
            | Self::Backup
            | Self::Passphrase
            | Self::Export
                if !matches!(
                    error.kind(),
                    VaultUiErrorKind::Authentication | VaultUiErrorKind::Audit
                ) =>
            {
                FailureRecovery::RefreshSnapshot
            }
            Self::Unlock
            | Self::Refresh
            | Self::Migrate
            | Self::Mutation
            | Self::Activity
            | Self::VerifyAudit
            | Self::ImportPreview
            | Self::ImportDiscard
            | Self::Import
            | Self::Backup
            | Self::Passphrase
            | Self::Export => FailureRecovery::Primary,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum FailureRecovery {
    Primary,
    RefreshSnapshot,
    ReconcilePresence,
}

struct BackendCompletion {
    kind: OperationKind,
    outcome: BackendOutcome,
}

impl BackendCompletion {
    fn new(
        kind: OperationKind,
        result: std::result::Result<VaultActionResult, VaultUiError>,
        backend: &dyn VaultBackend,
    ) -> Self {
        let outcome = match result {
            Ok(result) => BackendOutcome::Success(result),
            Err(error) => match kind.failure_recovery(&error) {
                FailureRecovery::Primary => BackendOutcome::Failure(BackendFailure::Primary(error)),
                FailureRecovery::RefreshSnapshot => {
                    let failure = match backend.refresh() {
                        Ok(snapshot) => BackendFailure::Refreshed { error, snapshot },
                        Err(refresh_error) => BackendFailure::RefreshFailed {
                            error,
                            refresh_error,
                        },
                    };
                    BackendOutcome::Failure(failure)
                }
                FailureRecovery::ReconcilePresence => BackendOutcome::LifecycleFailure {
                    error,
                    presence: backend.presence(),
                },
            },
        };
        Self { kind, outcome }
    }
}

enum BackendOutcome {
    Success(VaultActionResult),
    LifecycleFailure {
        error: VaultUiError,
        presence: Result<VaultPresence, VaultUiError>,
    },
    Failure(BackendFailure),
}

enum BackendFailure {
    Primary(VaultUiError),
    Refreshed {
        error: VaultUiError,
        snapshot: VaultSnapshot,
    },
    RefreshFailed {
        error: VaultUiError,
        refresh_error: VaultUiError,
    },
}

impl BackendFailure {
    const fn error(&self) -> &VaultUiError {
        match self {
            Self::Primary(error)
            | Self::Refreshed { error, .. }
            | Self::RefreshFailed { error, .. } => error,
        }
    }
}

struct ActionWorker {
    worker: CooperativeWorker<BackendCompletion>,
}

impl ActionWorker {
    fn spawn(backend: Arc<dyn VaultBackend>, request: BackendRequest) -> Result<Self> {
        let kind = request.kind();
        let worker = CooperativeWorker::spawn("jig-vault-tui-action", move |_cancelled| {
            let result = match request {
                BackendRequest::Unlock(passphrase) => {
                    backend.unlock(passphrase).map(VaultActionResult::Snapshot)
                }
                BackendRequest::Initialize(passphrase) => backend
                    .initialize(passphrase)
                    .map(VaultActionResult::Snapshot),
                BackendRequest::Execute(action) => backend.execute(action),
            };
            BackendCompletion::new(kind, result, backend.as_ref())
        })?;
        Ok(Self { worker })
    }

    fn try_finish(&mut self) -> Option<Result<BackendCompletion, String>> {
        self.worker.try_finish()
    }

    fn cancel_and_join(&mut self) {
        self.worker.cancel_and_join();
    }
}

pub(crate) fn handle_key(app: &mut App, key: KeyEvent) -> RuntimeAction {
    if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
        return RuntimeAction::Quit;
    }

    match &app.screen {
        Screen::Missing => return handle_missing_key(app, key),
        Screen::Locked(_) => return handle_locked_key(app, key),
        Screen::Initialize { .. } => return handle_initialize_key(app, key),
        Screen::Loading(_) => {
            return match key.code {
                KeyCode::Esc | KeyCode::Char('q') => RuntimeAction::Quit,
                _ => RuntimeAction::Ignore,
            };
        }
        Screen::Help => {
            return match key.code {
                KeyCode::Esc | KeyCode::Char('?') | KeyCode::Char('q') => {
                    app.close_overlay();
                    RuntimeAction::Redraw
                }
                _ => RuntimeAction::Ignore,
            };
        }
        Screen::ConfirmMigration => {
            return match key.code {
                KeyCode::Enter => {
                    app.begin_loading("Migrating vault to version 2");
                    RuntimeAction::Start(BackendRequest::Execute(VaultAction::MigrateToV2))
                }
                KeyCode::Esc | KeyCode::Char('q') => {
                    app.close_overlay();
                    RuntimeAction::Redraw
                }
                _ => RuntimeAction::Ignore,
            };
        }
        Screen::Form(_) => return handle_form_key(app, key),
        Screen::ConfirmMutation(_) => {
            return handle_mutation_confirmation_key(app, key);
        }
        Screen::ConfirmDelete(_) => return handle_delete_key(app, key),
        Screen::Tools(_) => return handle_tools_key(app, key),
        Screen::ToolForm(_) => return handle_tool_form_key(app, key),
        Screen::ImportPreview(_) => return handle_import_preview_key(app, key),
        Screen::Activity(_) => return handle_activity_key(app, key),
        Screen::ConfirmPeek(_) => return handle_peek_confirmation_key(app, key),
        Screen::AuditResult(_) => {
            return match key.code {
                KeyCode::Esc | KeyCode::Enter | KeyCode::Char('q') => {
                    app.close_overlay();
                    RuntimeAction::Redraw
                }
                _ => RuntimeAction::Ignore,
            };
        }
        Screen::Browse => {}
    }

    if app.searching {
        return match key.code {
            KeyCode::Esc | KeyCode::Enter => {
                app.searching = false;
                RuntimeAction::Redraw
            }
            KeyCode::Backspace => {
                app.pop_filter();
                RuntimeAction::Redraw
            }
            KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                app.clear_filter();
                RuntimeAction::Redraw
            }
            KeyCode::Char(character)
                if !key
                    .modifiers
                    .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
            {
                let mut encoded = [0; 4];
                app.append_filter(character.encode_utf8(&mut encoded));
                RuntimeAction::Redraw
            }
            KeyCode::Up => {
                app.move_selection(-1);
                RuntimeAction::Redraw
            }
            KeyCode::Down => {
                app.move_selection(1);
                RuntimeAction::Redraw
            }
            _ => RuntimeAction::Ignore,
        };
    }

    match key.code {
        KeyCode::Char('q') => RuntimeAction::Quit,
        KeyCode::Esc if !app.filter.is_empty() => {
            app.clear_filter();
            RuntimeAction::Redraw
        }
        KeyCode::Esc => RuntimeAction::Quit,
        KeyCode::Char('?') => {
            app.show_help();
            RuntimeAction::Redraw
        }
        KeyCode::Char('/') => {
            app.searching = true;
            app.focus = Focus::Items;
            RuntimeAction::Redraw
        }
        KeyCode::Tab => {
            app.cycle_focus(false);
            RuntimeAction::Redraw
        }
        KeyCode::BackTab => {
            app.cycle_focus(true);
            RuntimeAction::Redraw
        }
        KeyCode::Char('h') | KeyCode::Left => {
            app.cycle_focus(true);
            RuntimeAction::Redraw
        }
        KeyCode::Char('l') | KeyCode::Right => {
            app.cycle_focus(false);
            RuntimeAction::Redraw
        }
        KeyCode::Char('j') | KeyCode::Down => {
            app.move_selection(1);
            RuntimeAction::Redraw
        }
        KeyCode::Char('k') | KeyCode::Up => {
            app.move_selection(-1);
            RuntimeAction::Redraw
        }
        KeyCode::PageDown => {
            app.move_selection(10);
            RuntimeAction::Redraw
        }
        KeyCode::PageUp => {
            app.move_selection(-10);
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
        KeyCode::Char('r') => {
            app.begin_loading("Refreshing vault metadata");
            RuntimeAction::Start(BackendRequest::Execute(VaultAction::Refresh))
        }
        KeyCode::Char('a') => {
            app.begin_add();
            RuntimeAction::Redraw
        }
        KeyCode::Char('A') => {
            app.begin_add_legacy();
            RuntimeAction::Redraw
        }
        KeyCode::Char('e') => {
            app.begin_replace();
            RuntimeAction::Redraw
        }
        KeyCode::Char('K') => {
            app.begin_change_kind();
            RuntimeAction::Redraw
        }
        KeyCode::Char('n') => {
            app.begin_rename();
            RuntimeAction::Redraw
        }
        KeyCode::Char('c') => {
            app.begin_convert();
            RuntimeAction::Redraw
        }
        KeyCode::Char('D') => {
            app.begin_delete();
            RuntimeAction::Redraw
        }
        KeyCode::Char('x') => {
            app.begin_export();
            RuntimeAction::Redraw
        }
        KeyCode::Char('p') => {
            app.begin_peek();
            RuntimeAction::Redraw
        }
        KeyCode::Char('m')
            if app
                .snapshot
                .as_ref()
                .is_some_and(|snapshot| snapshot.format_version == 1) =>
        {
            app.confirm_migration();
            RuntimeAction::Redraw
        }
        KeyCode::Char('L') => RuntimeAction::Lock,
        KeyCode::Char(':') => {
            app.open_tools();
            RuntimeAction::Redraw
        }
        _ => RuntimeAction::Ignore,
    }
}

fn handle_missing_key(app: &mut App, key: KeyEvent) -> RuntimeAction {
    match key.code {
        KeyCode::Char('i') => {
            app.begin_initialize_form();
            RuntimeAction::Redraw
        }
        KeyCode::Esc | KeyCode::Char('q') => RuntimeAction::Quit,
        KeyCode::Char(':') => {
            app.open_tools();
            RuntimeAction::Redraw
        }
        _ => RuntimeAction::Ignore,
    }
}

fn handle_locked_key(app: &mut App, key: KeyEvent) -> RuntimeAction {
    match key.code {
        KeyCode::Esc => RuntimeAction::Quit,
        KeyCode::Enter => app
            .begin_unlock()
            .map_or(RuntimeAction::Redraw, |passphrase| {
                RuntimeAction::Start(BackendRequest::Unlock(passphrase))
            }),
        _ => handle_protected_editing_key(app, key),
    }
}

fn handle_initialize_key(app: &mut App, key: KeyEvent) -> RuntimeAction {
    match key.code {
        KeyCode::Esc => {
            app.cancel_initialize();
            RuntimeAction::Redraw
        }
        KeyCode::Tab | KeyCode::BackTab => {
            app.toggle_initialize_focus();
            RuntimeAction::Redraw
        }
        KeyCode::Enter => app
            .begin_initialize()
            .map_or(RuntimeAction::Redraw, |passphrase| {
                RuntimeAction::Start(BackendRequest::Initialize(passphrase))
            }),
        _ => handle_protected_editing_key(app, key),
    }
}

fn handle_protected_editing_key(app: &mut App, key: KeyEvent) -> RuntimeAction {
    match key.code {
        KeyCode::Backspace => {
            if let Some(input) = app.protected_input_mut() {
                input.backspace();
            }
            RuntimeAction::Redraw
        }
        KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            if let Some(input) = app.protected_input_mut() {
                input.clear();
            }
            RuntimeAction::Redraw
        }
        KeyCode::Char(character)
            if !key
                .modifiers
                .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
        {
            if let Some(input) = app.protected_input_mut() {
                if input.push_char(character).is_err() {
                    app.set_error("Protected input exceeds the vault value size limit.");
                }
            }
            RuntimeAction::Redraw
        }
        _ => RuntimeAction::Ignore,
    }
}

fn handle_form_key(app: &mut App, key: KeyEvent) -> RuntimeAction {
    handle_edit_form_key(app, key, App::submit_form)
}

fn handle_tool_form_key(app: &mut App, key: KeyEvent) -> RuntimeAction {
    handle_edit_form_key(app, key, App::submit_tool_form)
}

fn handle_edit_form_key(
    app: &mut App,
    key: KeyEvent,
    submit: fn(&mut App) -> Option<VaultAction>,
) -> RuntimeAction {
    match key.code {
        KeyCode::Esc => {
            app.close_overlay();
            RuntimeAction::Redraw
        }
        KeyCode::Tab => {
            app.cycle_form_focus(false);
            RuntimeAction::Redraw
        }
        KeyCode::BackTab => {
            app.cycle_form_focus(true);
            RuntimeAction::Redraw
        }
        KeyCode::Char(' ') => {
            if app.metadata_input_is_active() || app.protected_input_mut().is_some() {
                push_form_character(app, ' ')
            } else {
                app.toggle_form_choice();
                RuntimeAction::Redraw
            }
        }
        KeyCode::Enter => submit(app).map_or(RuntimeAction::Redraw, |action| {
            RuntimeAction::Start(BackendRequest::Execute(action))
        }),
        KeyCode::Backspace => {
            if let Some(input) = app.protected_input_mut() {
                input.backspace();
            } else {
                app.pop_metadata_input();
            }
            RuntimeAction::Redraw
        }
        KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            if let Some(input) = app.protected_input_mut() {
                input.clear();
            } else {
                app.clear_metadata_input();
            }
            RuntimeAction::Redraw
        }
        KeyCode::Char(character)
            if !key
                .modifiers
                .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
        {
            push_form_character(app, character)
        }
        _ => RuntimeAction::Ignore,
    }
}

fn handle_tools_key(app: &mut App, key: KeyEvent) -> RuntimeAction {
    match key.code {
        KeyCode::Esc | KeyCode::Char('q') => {
            app.close_overlay();
            RuntimeAction::Redraw
        }
        KeyCode::Char('j') | KeyCode::Down => {
            app.move_tool_selection(1);
            RuntimeAction::Redraw
        }
        KeyCode::Char('k') | KeyCode::Up => {
            app.move_tool_selection(-1);
            RuntimeAction::Redraw
        }
        KeyCode::Enter => app.activate_tool().map_or(RuntimeAction::Redraw, |action| {
            RuntimeAction::Start(BackendRequest::Execute(action))
        }),
        _ => RuntimeAction::Ignore,
    }
}

fn handle_activity_key(app: &mut App, key: KeyEvent) -> RuntimeAction {
    match key.code {
        KeyCode::Esc | KeyCode::Char('q') | KeyCode::Enter => {
            app.close_overlay();
            RuntimeAction::Redraw
        }
        KeyCode::Char('j') | KeyCode::Down => {
            app.move_activity_selection(1);
            RuntimeAction::Redraw
        }
        KeyCode::Char('k') | KeyCode::Up => {
            app.move_activity_selection(-1);
            RuntimeAction::Redraw
        }
        KeyCode::PageDown => {
            app.move_activity_selection(10);
            RuntimeAction::Redraw
        }
        KeyCode::PageUp => {
            app.move_activity_selection(-10);
            RuntimeAction::Redraw
        }
        _ => RuntimeAction::Ignore,
    }
}

fn handle_import_preview_key(app: &mut App, key: KeyEvent) -> RuntimeAction {
    match key.code {
        KeyCode::Esc => app
            .discard_import_preview()
            .map_or(RuntimeAction::Redraw, |action| {
                RuntimeAction::Start(BackendRequest::Execute(action))
            }),
        KeyCode::Char('r') => {
            app.toggle_import_replace();
            RuntimeAction::Redraw
        }
        KeyCode::Char('o') => {
            app.toggle_import_overwrite();
            RuntimeAction::Redraw
        }
        KeyCode::Enter => app
            .submit_import_preview()
            .map_or(RuntimeAction::Redraw, |action| {
                RuntimeAction::Start(BackendRequest::Execute(action))
            }),
        KeyCode::Backspace => {
            app.pop_metadata_input();
            RuntimeAction::Redraw
        }
        KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            app.clear_metadata_input();
            RuntimeAction::Redraw
        }
        KeyCode::Char(character)
            if !key
                .modifiers
                .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
        {
            append_metadata_character(app, character);
            RuntimeAction::Redraw
        }
        _ => RuntimeAction::Ignore,
    }
}

fn handle_delete_key(app: &mut App, key: KeyEvent) -> RuntimeAction {
    handle_metadata_confirmation_key(app, key, |app| {
        app.submit_delete().map_or(RuntimeAction::Redraw, |action| {
            RuntimeAction::Start(BackendRequest::Execute(action))
        })
    })
}

fn handle_mutation_confirmation_key(app: &mut App, key: KeyEvent) -> RuntimeAction {
    handle_metadata_confirmation_key(app, key, |app| {
        app.submit_mutation_confirmation()
            .map_or(RuntimeAction::Redraw, |action| {
                RuntimeAction::Start(BackendRequest::Execute(action))
            })
    })
}

fn handle_peek_confirmation_key(app: &mut App, key: KeyEvent) -> RuntimeAction {
    handle_metadata_confirmation_key(app, key, |app| {
        app.submit_peek()
            .map_or(RuntimeAction::Redraw, RuntimeAction::Peek)
    })
}

fn handle_metadata_confirmation_key(
    app: &mut App,
    key: KeyEvent,
    submit: impl FnOnce(&mut App) -> RuntimeAction,
) -> RuntimeAction {
    match key.code {
        KeyCode::Esc => {
            app.close_overlay();
            RuntimeAction::Redraw
        }
        KeyCode::Enter => submit(app),
        KeyCode::Backspace => {
            app.pop_metadata_input();
            RuntimeAction::Redraw
        }
        KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            app.clear_metadata_input();
            RuntimeAction::Redraw
        }
        KeyCode::Char(character)
            if !key
                .modifiers
                .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
        {
            append_metadata_character(app, character);
            RuntimeAction::Redraw
        }
        _ => RuntimeAction::Ignore,
    }
}

fn push_form_character(app: &mut App, character: char) -> RuntimeAction {
    if let Some(input) = app.protected_input_mut() {
        if input.push_char(character).is_err() {
            app.set_error("Protected input exceeds the vault value size limit.");
        }
    } else {
        append_metadata_character(app, character);
    }
    RuntimeAction::Redraw
}

fn append_metadata_character(app: &mut App, character: char) {
    let mut encoded = [0; 4];
    app.handle_metadata_append(character.encode_utf8(&mut encoded));
}

pub(crate) fn handle_paste(app: &mut App, value: &str) -> RuntimeAction {
    if let Some(input) = app.protected_input_mut() {
        if input.paste(value).is_err() {
            app.set_error(
                "Paste rejected: protected input would exceed the vault value size limit.",
            );
        }
        return RuntimeAction::Redraw;
    }
    if app.handle_metadata_append(value) {
        return RuntimeAction::Redraw;
    }
    if app.searching {
        app.append_filter(value);
        return RuntimeAction::Redraw;
    }
    RuntimeAction::Ignore
}

#[derive(Clone, Copy, Debug)]
struct IdleTimer {
    last_input: Instant,
    timeout: Duration,
}

impl IdleTimer {
    const fn new(last_input: Instant, timeout: Duration) -> Self {
        Self {
            last_input,
            timeout,
        }
    }

    fn record(&mut self, now: Instant) {
        self.last_input = now;
    }

    fn expired(self, now: Instant, unlocked: bool) -> bool {
        unlocked && now.saturating_duration_since(self.last_input) >= self.timeout
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{
        Mutex,
        atomic::{AtomicUsize, Ordering},
        mpsc,
    };

    use jig_vault::{FieldKind, SecretBytes, Vault};
    use secrecy::SecretString;

    use super::*;
    use crate::{
        VaultDescriptor,
        model::{EntryIdentity, ItemIdentity},
    };

    struct TrackingBackend {
        locks: AtomicUsize,
        presence_reads: AtomicUsize,
        refreshes: AtomicUsize,
        presence_result: Mutex<Option<std::result::Result<VaultPresence, VaultUiError>>>,
        refresh_result: Mutex<Option<std::result::Result<VaultSnapshot, VaultUiError>>>,
        events: Mutex<Vec<&'static str>>,
        started: Mutex<Option<mpsc::Sender<()>>>,
        release: Mutex<Option<mpsc::Receiver<()>>>,
    }

    impl TrackingBackend {
        fn immediate() -> Self {
            Self {
                locks: AtomicUsize::new(0),
                presence_reads: AtomicUsize::new(0),
                refreshes: AtomicUsize::new(0),
                presence_result: Mutex::new(None),
                refresh_result: Mutex::new(None),
                events: Mutex::new(Vec::new()),
                started: Mutex::new(None),
                release: Mutex::new(None),
            }
        }

        fn with_refresh(result: std::result::Result<VaultSnapshot, VaultUiError>) -> Self {
            let backend = Self::immediate();
            *backend.refresh_result.lock().unwrap() = Some(result);
            backend
        }

        fn with_presence(result: std::result::Result<VaultPresence, VaultUiError>) -> Self {
            let backend = Self::immediate();
            *backend.presence_result.lock().unwrap() = Some(result);
            backend
        }

        fn blocking(started: mpsc::Sender<()>, release: mpsc::Receiver<()>) -> Self {
            Self {
                locks: AtomicUsize::new(0),
                presence_reads: AtomicUsize::new(0),
                refreshes: AtomicUsize::new(0),
                presence_result: Mutex::new(None),
                refresh_result: Mutex::new(None),
                events: Mutex::new(Vec::new()),
                started: Mutex::new(Some(started)),
                release: Mutex::new(Some(release)),
            }
        }
    }

    impl VaultBackend for TrackingBackend {
        fn descriptor(&self) -> VaultDescriptor {
            VaultDescriptor {
                scope: "test".to_owned(),
                scope_id: None,
                repo_name: None,
                home: std::path::PathBuf::from("/tmp/test-vault"),
                exists: true,
            }
        }

        fn presence(&self) -> std::result::Result<VaultPresence, VaultUiError> {
            self.presence_reads.fetch_add(1, Ordering::SeqCst);
            self.presence_result
                .lock()
                .unwrap()
                .take()
                .unwrap_or(Ok(VaultPresence::Present))
        }

        fn unlock(&self, _: SecretBytes) -> std::result::Result<VaultSnapshot, VaultUiError> {
            Err(VaultUiError::new(VaultUiErrorKind::Other, "unused"))
        }

        fn initialize(&self, _: SecretBytes) -> std::result::Result<VaultSnapshot, VaultUiError> {
            Err(VaultUiError::new(VaultUiErrorKind::Other, "unused"))
        }

        fn lock(&self) {
            self.events.lock().unwrap().push("lock");
            self.locks.fetch_add(1, Ordering::SeqCst);
        }

        fn refresh(&self) -> std::result::Result<VaultSnapshot, VaultUiError> {
            self.refreshes.fetch_add(1, Ordering::SeqCst);
            self.refresh_result
                .lock()
                .unwrap()
                .take()
                .unwrap_or_else(|| Err(VaultUiError::new(VaultUiErrorKind::Other, "unused")))
        }

        fn execute(&self, _: VaultAction) -> std::result::Result<VaultActionResult, VaultUiError> {
            self.events.lock().unwrap().push("operation-started");
            if let Some(started) = self.started.lock().unwrap().take() {
                started.send(()).unwrap();
            }
            if let Some(release) = self.release.lock().unwrap().take() {
                release.recv().unwrap();
            }
            self.events.lock().unwrap().push("operation-finished");
            Err(VaultUiError::new(VaultUiErrorKind::Other, "expected"))
        }

        fn peek(
            &self,
            _: &VaultReference,
            _: &mut dyn std::io::Write,
        ) -> std::result::Result<usize, VaultUiError> {
            Err(VaultUiError::new(VaultUiErrorKind::Other, "unused"))
        }
    }

    fn unlocked_app() -> App {
        let temp = tempfile::tempdir().unwrap();
        let vault = Vault::resolve_for_test(Some(temp.path().join("vault"))).unwrap();
        let passphrase = SecretString::from("correct horse battery staple".to_owned());
        vault.init(&passphrase).unwrap();
        vault
            .set_field(
                &passphrase,
                "jig://Production/TOKEN".parse().unwrap(),
                FieldKind::Concealed,
                SecretBytes::new(b"runtime-test-secret".to_vec()),
            )
            .unwrap();
        vault
            .set_field(
                &passphrase,
                "jig://Production/OTHER".parse().unwrap(),
                FieldKind::Concealed,
                SecretBytes::new(b"other-runtime-secret".to_vec()),
            )
            .unwrap();
        let mut app = App::new(VaultDescriptor {
            scope: "test".to_owned(),
            scope_id: None,
            repo_name: None,
            home: temp.path().join("vault"),
            exists: true,
        });
        app.apply_snapshot(vault.snapshot(&passphrase).unwrap());
        app
    }

    #[test]
    fn idle_timer_resets_and_expires_only_while_unlocked() {
        let start = Instant::now();
        let timeout = Duration::from_secs(300);
        let mut timer = IdleTimer::new(start, timeout);
        assert!(!timer.expired(start + timeout, false));
        assert!(timer.expired(start + timeout, true));

        timer.record(start + Duration::from_secs(299));
        assert!(!timer.expired(start + timeout, true));
        assert!(timer.expired(start + Duration::from_secs(599), true));
    }

    #[test]
    fn authentication_and_audit_failures_drop_the_session() {
        for kind in [VaultUiErrorKind::Authentication, VaultUiErrorKind::Audit] {
            let backend = Arc::new(TrackingBackend::immediate());
            let erased: Arc<dyn VaultBackend> = backend.clone();
            let mut app = unlocked_app();
            apply_completion(
                &mut app,
                &erased,
                Ok(BackendCompletion {
                    kind: OperationKind::VerifyAudit,
                    outcome: BackendOutcome::Failure(BackendFailure::Primary(VaultUiError::new(
                        kind,
                        "safe failure",
                    ))),
                }),
            );
            assert_eq!(backend.locks.load(Ordering::SeqCst), 1);
            assert!(app.snapshot.is_none());
            assert!(matches!(app.screen, Screen::Locked(_)));
        }
    }

    #[test]
    fn recovery_policy_follows_operation_semantics() {
        let recoverable = VaultUiError::new(VaultUiErrorKind::Io, "safe primary failure");
        for kind in [
            OperationKind::Migrate,
            OperationKind::Mutation,
            OperationKind::Import,
            OperationKind::Backup,
            OperationKind::Passphrase,
            OperationKind::Export,
        ] {
            assert_eq!(
                kind.failure_recovery(&recoverable),
                FailureRecovery::RefreshSnapshot
            );
        }
        for kind in [
            OperationKind::Unlock,
            OperationKind::Refresh,
            OperationKind::Activity,
            OperationKind::VerifyAudit,
            OperationKind::ImportPreview,
            OperationKind::ImportDiscard,
        ] {
            assert_eq!(
                kind.failure_recovery(&recoverable),
                FailureRecovery::Primary
            );
        }
        for kind in [OperationKind::Initialize, OperationKind::Restore] {
            assert_eq!(
                kind.failure_recovery(&recoverable),
                FailureRecovery::ReconcilePresence
            );
        }
        for fatal_kind in [VaultUiErrorKind::Authentication, VaultUiErrorKind::Audit] {
            let fatal = VaultUiError::new(fatal_kind, "safe fatal failure");
            assert_eq!(
                OperationKind::Import.failure_recovery(&fatal),
                FailureRecovery::Primary
            );
        }
    }

    #[test]
    fn lifecycle_failures_reconcile_authoritative_presence() {
        for kind in [OperationKind::Initialize, OperationKind::Restore] {
            for presence in [VaultPresence::Missing, VaultPresence::Present] {
                let backend = Arc::new(TrackingBackend::with_presence(Ok(presence)));
                let erased: Arc<dyn VaultBackend> = backend.clone();
                let mut app = App::new(VaultDescriptor {
                    scope: "test".to_owned(),
                    scope_id: None,
                    repo_name: None,
                    home: std::path::PathBuf::from("/tmp/test-vault"),
                    exists: !presence.is_present(),
                });
                let completion = BackendCompletion::new(
                    kind,
                    Err(VaultUiError::new(
                        VaultUiErrorKind::Io,
                        "safe lifecycle failure",
                    )),
                    erased.as_ref(),
                );

                apply_completion(&mut app, &erased, Ok(completion));

                assert_eq!(backend.presence_reads.load(Ordering::SeqCst), 1);
                assert_eq!(app.descriptor.exists, presence.is_present());
                match presence {
                    VaultPresence::Missing => assert!(matches!(app.screen, Screen::Missing)),
                    VaultPresence::Present => assert!(matches!(app.screen, Screen::Locked(_))),
                }
                assert_eq!(app.status.as_ref().unwrap().text, "safe lifecycle failure");
            }
        }
    }

    #[test]
    fn failed_lifecycle_presence_check_fails_closed() {
        let backend = Arc::new(TrackingBackend::with_presence(Err(VaultUiError::new(
            VaultUiErrorKind::Io,
            "safe presence failure",
        ))));
        let erased: Arc<dyn VaultBackend> = backend.clone();
        let mut app = App::new(VaultDescriptor {
            scope: "test".to_owned(),
            scope_id: None,
            repo_name: None,
            home: std::path::PathBuf::from("/tmp/test-vault"),
            exists: false,
        });
        let completion = BackendCompletion::new(
            OperationKind::Restore,
            Err(VaultUiError::new(
                VaultUiErrorKind::Audit,
                "safe restore failure",
            )),
            erased.as_ref(),
        );

        apply_completion(&mut app, &erased, Ok(completion));

        assert_eq!(backend.presence_reads.load(Ordering::SeqCst), 1);
        assert_eq!(backend.locks.load(Ordering::SeqCst), 1);
        assert!(app.descriptor.exists);
        assert!(matches!(app.screen, Screen::Locked(_)));
        assert_eq!(
            app.status.unwrap().text,
            "safe restore failure Vault presence reconciliation also failed: safe presence failure"
        );
    }

    #[test]
    fn successful_recovery_refresh_replaces_stale_snapshot_and_retains_primary_error() {
        let mut app = unlocked_app();
        let original: VaultReference = "jig://Production/TOKEN".parse().unwrap();
        app.selected_item = Some(ItemIdentity::Canonical("Production".to_owned()));
        app.selected_entry = Some(EntryIdentity::Field(original.clone()));
        app.focus = Focus::Fields;
        app.begin_rename();
        handle_paste(&mut app, "OTHER");
        assert!(app.submit_form().is_some());
        let refreshed = app.snapshot.clone().unwrap();
        let backend = Arc::new(TrackingBackend::with_refresh(Ok(refreshed.clone())));
        let erased: Arc<dyn VaultBackend> = backend.clone();
        let completion = BackendCompletion::new(
            OperationKind::Mutation,
            Err(VaultUiError::new(
                VaultUiErrorKind::Conflict,
                "safe mutation failure",
            )),
            erased.as_ref(),
        );

        apply_completion(&mut app, &erased, Ok(completion));

        assert_eq!(backend.refreshes.load(Ordering::SeqCst), 1);
        assert_eq!(backend.locks.load(Ordering::SeqCst), 0);
        assert_eq!(app.snapshot, Some(refreshed));
        assert!(matches!(app.screen, Screen::Browse));
        assert_eq!(
            app.selected_entry,
            Some(EntryIdentity::Field(original)),
            "failed mutation must preserve the operator's prior selection"
        );
        assert_eq!(app.status.unwrap().text, "safe mutation failure");
    }

    #[test]
    fn failed_recovery_refresh_drops_the_unverified_session() {
        for refresh_kind in [
            VaultUiErrorKind::Authentication,
            VaultUiErrorKind::Audit,
            VaultUiErrorKind::Other,
        ] {
            let backend = Arc::new(TrackingBackend::with_refresh(Err(VaultUiError::new(
                refresh_kind,
                "safe recovery failure",
            ))));
            let erased: Arc<dyn VaultBackend> = backend.clone();
            let mut app = unlocked_app();
            let completion = BackendCompletion::new(
                OperationKind::Import,
                Err(VaultUiError::new(
                    VaultUiErrorKind::Io,
                    "safe import failure",
                )),
                erased.as_ref(),
            );

            apply_completion(&mut app, &erased, Ok(completion));

            assert_eq!(backend.refreshes.load(Ordering::SeqCst), 1);
            assert_eq!(backend.locks.load(Ordering::SeqCst), 1);
            assert!(app.snapshot.is_none());
            assert!(matches!(app.screen, Screen::Locked(_)));
            assert_eq!(
                app.status.unwrap().text,
                "safe import failure Vault metadata recovery also failed: safe recovery failure"
            );
        }
    }

    #[test]
    fn worker_join_finishes_the_operation_before_backend_lock() {
        let (started_sender, started_receiver) = mpsc::channel();
        let (release_sender, release_receiver) = mpsc::channel();
        let backend = Arc::new(TrackingBackend::blocking(started_sender, release_receiver));
        let erased: Arc<dyn VaultBackend> = backend.clone();
        let mut worker = ActionWorker::spawn(
            Arc::clone(&erased),
            BackendRequest::Execute(VaultAction::Refresh),
        )
        .unwrap();
        started_receiver
            .recv_timeout(Duration::from_secs(1))
            .unwrap();
        release_sender.send(()).unwrap();
        worker.cancel_and_join();
        erased.lock();

        assert_eq!(
            *backend.events.lock().unwrap(),
            ["operation-started", "operation-finished", "lock"]
        );
    }
}
