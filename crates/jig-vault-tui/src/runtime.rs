use std::{
    sync::Arc,
    time::{Duration, Instant},
};

use anyhow::{Context, Result};
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers};
use jig_tui::{CooperativeWorker, TerminalSession, is_actionable_key, require_terminal};
use jig_vault::{SecretBytes, VaultReference, VaultSnapshot};

use crate::{
    VaultAction, VaultActionResult, VaultBackend, VaultCommittedAction, VaultHomeState,
    VaultUiError, VaultUiErrorKind,
    commands::{CommandOutcome, CommandPaletteScope, UiCommand},
    line_editor::LineEdit,
    model::{App, Focus, Screen},
    peek::{PEEK_BEGIN_MARKER, PEEK_END_MARKER, TerminalSafePreviewWriter},
    render,
    viewport::ViewportSize,
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
        if app.descriptor.home_state.is_initialized() {
            app.begin_loading("Unlocking vault");
            worker = Some(ActionWorker::spawn(
                Arc::clone(&backend),
                BackendRequest::Unlock(passphrase),
            )?);
        }
    }
    let mut dirty = true;
    let mut viewport = ViewportSize::new(0, 0);
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
                .draw(|frame| {
                    let area = frame.area();
                    viewport = ViewportSize::new(area.width, area.height);
                    render::draw(frame, &app);
                })
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
            let action = dispatch_event(&mut app, viewport, input);
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

pub(crate) fn dispatch_event(app: &mut App, viewport: ViewportSize, input: Event) -> RuntimeAction {
    match input {
        Event::Resize(_, _) => RuntimeAction::Redraw,
        Event::Key(key) if is_actionable_key(key) => {
            if viewport.supports_full_ui() {
                handle_key(app, key)
            } else {
                handle_undersized_key(key)
            }
        }
        Event::Paste(value) if viewport.supports_full_ui() => handle_paste(app, &value),
        _ => RuntimeAction::Ignore,
    }
}

fn handle_undersized_key(key: KeyEvent) -> RuntimeAction {
    if key.code == KeyCode::Char('q') && key.modifiers.is_empty()
        || key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL)
    {
        RuntimeAction::Quit
    } else {
        RuntimeAction::Ignore
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
        Err(error) => match home_state_for_not_found(&error, backend.as_ref()) {
            Some(Ok(home_state @ (VaultHomeState::Absent | VaultHomeState::Uninitialized))) => {
                apply_lifecycle_failure(app, backend, error, Ok(home_state));
            }
            Some(Err(home_state_error)) => {
                apply_lifecycle_failure(app, backend, error, Err(home_state_error));
            }
            Some(Ok(VaultHomeState::Initialized)) | None => app.fail_action(&error),
        },
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
        BackendOutcome::LifecycleFailure { error, home_state } => {
            apply_lifecycle_failure(app, backend, error, home_state);
        }
        BackendOutcome::CommittedWithoutSnapshot(committed) => {
            apply_committed_without_snapshot(app, backend, committed);
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
        VaultActionResult::Committed { .. } => {
            unreachable!("committed results are normalized by BackendCompletion")
        }
    }
}

fn apply_committed_without_snapshot(
    app: &mut App,
    backend: &Arc<dyn VaultBackend>,
    committed: CommittedWithoutSnapshot,
) {
    let message = committed.action.completion_message();
    let error = match committed.retry_error {
        Some(retry_error) => VaultUiError::new(
            retry_error.kind(),
            format!(
                "{message}, but its metadata refresh failed: {} The automatic retry also failed: {}",
                committed.refresh_error.message(),
                retry_error.message()
            ),
        ),
        None => VaultUiError::new(
            committed.refresh_error.kind(),
            format!(
                "{message}, but its metadata refresh failed: {}",
                committed.refresh_error.message()
            ),
        ),
    };
    apply_lifecycle_failure(app, backend, error, committed.home_state);
}

impl VaultCommittedAction {
    fn completion_message(&self) -> String {
        match self {
            Self::Initialized => "Vault initialization completed".to_owned(),
            Self::Migrated => "Vault migration to version 2 completed".to_owned(),
            Self::Mutated => "Vault update completed".to_owned(),
            Self::Imported => "1Password import completed".to_owned(),
            Self::PassphraseChanged => "Vault passphrase change completed".to_owned(),
            Self::BackupCreated {
                output,
                bytes_written,
                backup_version,
            } => format!(
                "Encrypted backup v{backup_version} was written to {} ({bytes_written} bytes)",
                output.display()
            ),
            Self::Exported {
                output,
                bytes_written,
            } => format!(
                "Private export wrote {bytes_written} bytes to {}",
                output.display()
            ),
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
            home_state,
        } => {
            let error = VaultUiError::new(
                refresh_error.kind(),
                format!(
                    "{} Vault metadata recovery also failed: {}",
                    error.message(),
                    refresh_error.message()
                ),
            );
            apply_lifecycle_failure(app, backend, error, home_state);
        }
        BackendFailure::Primary(error) => apply_primary_failure(app, kind, &error),
    }
}

fn apply_lifecycle_failure(
    app: &mut App,
    backend: &Arc<dyn VaultBackend>,
    error: VaultUiError,
    home_state: Result<VaultHomeState, VaultUiError>,
) {
    backend.lock();
    match home_state {
        Ok(home_state) => {
            app.fail_lifecycle(&error, home_state);
        }
        Err(home_state_error) => {
            let error = VaultUiError::new(
                home_state_error.kind(),
                format!(
                    "{} Vault presence reconciliation also failed: {}",
                    error.message(),
                    home_state_error.message()
                ),
            );
            app.fail_lifecycle(&error, VaultHomeState::Initialized);
        }
    }
}

fn apply_primary_failure(app: &mut App, kind: OperationKind, error: &VaultUiError) {
    match kind {
        OperationKind::Unlock => app.fail_unlock(error),
        OperationKind::Initialize | OperationKind::Restore => {
            app.fail_lifecycle(error, VaultHomeState::Initialized);
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
            Self::Initialize | Self::Restore => FailureRecovery::ReconcileHomeState,
            Self::Unlock | Self::Refresh if matches!(error.kind(), VaultUiErrorKind::NotFound) => {
                FailureRecovery::ReconcileHomeState
            }
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
    ReconcileHomeState,
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
            Ok(VaultActionResult::Committed {
                action,
                refresh_error,
            }) => {
                let retry = if matches!(
                    refresh_error.kind(),
                    VaultUiErrorKind::Authentication | VaultUiErrorKind::Audit
                ) {
                    None
                } else {
                    Some(backend.refresh())
                };
                match retry {
                    Some(Ok(snapshot)) => BackendOutcome::Success(action.with_snapshot(snapshot)),
                    Some(Err(retry_error)) => {
                        BackendOutcome::CommittedWithoutSnapshot(CommittedWithoutSnapshot {
                            action,
                            refresh_error,
                            retry_error: Some(retry_error),
                            home_state: backend.home_state(),
                        })
                    }
                    None => BackendOutcome::CommittedWithoutSnapshot(CommittedWithoutSnapshot {
                        action,
                        refresh_error,
                        retry_error: None,
                        home_state: backend.home_state(),
                    }),
                }
            }
            Ok(result) => BackendOutcome::Success(result),
            Err(error) => Self::failure_outcome(kind, error, backend),
        };
        Self { kind, outcome }
    }

    fn failure_outcome(
        kind: OperationKind,
        error: VaultUiError,
        backend: &dyn VaultBackend,
    ) -> BackendOutcome {
        let not_found_home_state = home_state_for_not_found(&error, backend);
        match not_found_home_state {
            Some(Ok(home_state @ (VaultHomeState::Absent | VaultHomeState::Uninitialized))) => {
                BackendOutcome::LifecycleFailure {
                    error,
                    home_state: Ok(home_state),
                }
            }
            Some(Err(home_state_error)) => BackendOutcome::LifecycleFailure {
                error,
                home_state: Err(home_state_error),
            },
            known_home_state => match kind.failure_recovery(&error) {
                FailureRecovery::Primary => BackendOutcome::Failure(BackendFailure::Primary(error)),
                FailureRecovery::RefreshSnapshot => {
                    let failure = match backend.refresh() {
                        Ok(snapshot) => BackendFailure::Refreshed { error, snapshot },
                        Err(refresh_error) => BackendFailure::RefreshFailed {
                            error,
                            refresh_error,
                            home_state: backend.home_state(),
                        },
                    };
                    BackendOutcome::Failure(failure)
                }
                FailureRecovery::ReconcileHomeState => BackendOutcome::LifecycleFailure {
                    error,
                    home_state: known_home_state.unwrap_or_else(|| backend.home_state()),
                },
            },
        }
    }
}

fn home_state_for_not_found(
    error: &VaultUiError,
    backend: &dyn VaultBackend,
) -> Option<Result<VaultHomeState, VaultUiError>> {
    (error.kind() == VaultUiErrorKind::NotFound).then(|| backend.home_state())
}

enum BackendOutcome {
    Success(VaultActionResult),
    CommittedWithoutSnapshot(CommittedWithoutSnapshot),
    LifecycleFailure {
        error: VaultUiError,
        home_state: Result<VaultHomeState, VaultUiError>,
    },
    Failure(BackendFailure),
}

struct CommittedWithoutSnapshot {
    action: VaultCommittedAction,
    refresh_error: VaultUiError,
    retry_error: Option<VaultUiError>,
    home_state: Result<VaultHomeState, VaultUiError>,
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
        home_state: Result<VaultHomeState, VaultUiError>,
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
                BackendRequest::Initialize(passphrase) => {
                    backend.initialize_with_completion(passphrase)
                }
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
        Screen::Commands(_) => return handle_command_palette_key(app, key),
        Screen::QuickAccess(_) => return handle_quick_access_key(app, key),
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

    if key.code == KeyCode::Char('p') && key.modifiers == KeyModifiers::CONTROL {
        app.open_quick_access();
        return RuntimeAction::Redraw;
    }

    if app.searching {
        let action = match key.code {
            KeyCode::Esc | KeyCode::Enter => {
                app.searching = false;
                Some(RuntimeAction::Redraw)
            }
            KeyCode::Up => {
                app.move_selection(-1);
                Some(RuntimeAction::Redraw)
            }
            KeyCode::Down => {
                app.move_selection(1);
                Some(RuntimeAction::Redraw)
            }
            _ => None,
        };
        if let Some(action) = action {
            return action;
        }
        if let Some(edit) = line_edit_from_key(key) {
            app.edit_filter(edit);
            return RuntimeAction::Redraw;
        }
        return match key.code {
            KeyCode::Char(character)
                if !key
                    .modifiers
                    .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
            {
                let mut encoded = [0; 4];
                app.append_filter(character.encode_utf8(&mut encoded));
                RuntimeAction::Redraw
            }
            _ => RuntimeAction::Ignore,
        };
    }

    if let Some(command) = UiCommand::from_key(key) {
        return command_outcome(app.activate_direct_command(command));
    }

    match key.code {
        KeyCode::Char('q') => RuntimeAction::Quit,
        KeyCode::Esc if !app.filter().is_empty() => {
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
        KeyCode::Char(':') => {
            app.open_command_palette(CommandPaletteScope::Universal);
            RuntimeAction::Redraw
        }
        KeyCode::Enter => {
            app.open_command_palette(CommandPaletteScope::Context);
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
            app.open_command_palette(CommandPaletteScope::Universal);
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
    if let Some(edit) = line_edit_from_key(key) {
        if let Some(input) = app.protected_input_mut() {
            match edit {
                LineEdit::Backspace => input.backspace(),
                LineEdit::Clear => input.clear(),
                LineEdit::Delete
                | LineEdit::Left
                | LineEdit::Right
                | LineEdit::Home
                | LineEdit::End
                | LineEdit::WordLeft
                | LineEdit::WordRight
                | LineEdit::DeleteWordLeft => return RuntimeAction::Ignore,
            }
            return RuntimeAction::Redraw;
        }
        if app.edit_metadata_input(edit) {
            return RuntimeAction::Redraw;
        }
    }
    if matches!(key.code, KeyCode::Char(_))
        && key
            .modifiers
            .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
    {
        return RuntimeAction::Ignore;
    }
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
        KeyCode::Char(character) => push_form_character(app, character),
        _ => RuntimeAction::Ignore,
    }
}

fn handle_command_palette_key(app: &mut App, key: KeyEvent) -> RuntimeAction {
    let action = match key.code {
        KeyCode::Esc => {
            app.close_overlay();
            Some(RuntimeAction::Redraw)
        }
        KeyCode::Down => {
            app.move_command_selection(1);
            Some(RuntimeAction::Redraw)
        }
        KeyCode::Up => {
            app.move_command_selection(-1);
            Some(RuntimeAction::Redraw)
        }
        KeyCode::PageDown => {
            app.move_command_selection(10);
            Some(RuntimeAction::Redraw)
        }
        KeyCode::PageUp => {
            app.move_command_selection(-10);
            Some(RuntimeAction::Redraw)
        }
        KeyCode::Enter => Some(command_outcome(app.activate_selected_command())),
        _ => None,
    };
    if let Some(action) = action {
        return action;
    }
    if let Some(edit) = line_edit_from_key(key) {
        app.edit_command_filter(edit);
        return RuntimeAction::Redraw;
    }
    match key.code {
        KeyCode::Char(character)
            if !key
                .modifiers
                .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
        {
            let mut encoded = [0; 4];
            app.append_command_filter(character.encode_utf8(&mut encoded));
            RuntimeAction::Redraw
        }
        _ => RuntimeAction::Ignore,
    }
}

fn command_outcome(outcome: CommandOutcome) -> RuntimeAction {
    match outcome {
        CommandOutcome::Redraw => RuntimeAction::Redraw,
        CommandOutcome::Start(action) => RuntimeAction::Start(BackendRequest::Execute(action)),
        CommandOutcome::Lock => RuntimeAction::Lock,
    }
}

fn handle_quick_access_key(app: &mut App, key: KeyEvent) -> RuntimeAction {
    let action = match (key.code, key.modifiers) {
        (KeyCode::Esc, _) => {
            app.close_overlay();
            Some(RuntimeAction::Redraw)
        }
        (KeyCode::Down, KeyModifiers::NONE) => {
            app.move_quick_access_selection(1);
            Some(RuntimeAction::Redraw)
        }
        (KeyCode::Up, KeyModifiers::NONE) => {
            app.move_quick_access_selection(-1);
            Some(RuntimeAction::Redraw)
        }
        (KeyCode::PageDown, KeyModifiers::NONE) => {
            app.move_quick_access_selection(10);
            Some(RuntimeAction::Redraw)
        }
        (KeyCode::PageUp, KeyModifiers::NONE) => {
            app.move_quick_access_selection(-10);
            Some(RuntimeAction::Redraw)
        }
        (KeyCode::Home, KeyModifiers::CONTROL) => {
            app.move_quick_access_to_edge(false);
            Some(RuntimeAction::Redraw)
        }
        (KeyCode::End, KeyModifiers::CONTROL) => {
            app.move_quick_access_to_edge(true);
            Some(RuntimeAction::Redraw)
        }
        (KeyCode::Enter, KeyModifiers::NONE) => {
            app.activate_quick_access();
            Some(RuntimeAction::Redraw)
        }
        _ => None,
    };
    if let Some(action) = action {
        return action;
    }
    if let Some(edit) = line_edit_from_key(key) {
        app.edit_quick_access_query(edit);
        return RuntimeAction::Redraw;
    }
    match key.code {
        KeyCode::Char(character)
            if !key
                .modifiers
                .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
        {
            let mut encoded = [0; 4];
            app.append_quick_access_query(character.encode_utf8(&mut encoded));
            RuntimeAction::Redraw
        }
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
    if let Some(edit) = line_edit_from_key(key) {
        app.edit_metadata_input(edit);
        return RuntimeAction::Redraw;
    }
    match key.code {
        KeyCode::Esc => app
            .discard_import_preview()
            .map_or(RuntimeAction::Redraw, |action| {
                RuntimeAction::Start(BackendRequest::Execute(action))
            }),
        KeyCode::Char('r') if key.modifiers.is_empty() => {
            app.toggle_import_replace();
            RuntimeAction::Redraw
        }
        KeyCode::Char('o') if key.modifiers.is_empty() => {
            app.toggle_import_overwrite();
            RuntimeAction::Redraw
        }
        KeyCode::Enter => app
            .submit_import_preview()
            .map_or(RuntimeAction::Redraw, |action| {
                RuntimeAction::Start(BackendRequest::Execute(action))
            }),
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
    if let Some(edit) = line_edit_from_key(key) {
        app.edit_metadata_input(edit);
        return RuntimeAction::Redraw;
    }
    match key.code {
        KeyCode::Esc => {
            app.close_overlay();
            RuntimeAction::Redraw
        }
        KeyCode::Enter => submit(app),
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

fn line_edit_from_key(key: KeyEvent) -> Option<LineEdit> {
    match (key.code, key.modifiers) {
        (KeyCode::Backspace, KeyModifiers::NONE) => Some(LineEdit::Backspace),
        (KeyCode::Delete, KeyModifiers::NONE) => Some(LineEdit::Delete),
        (KeyCode::Left, KeyModifiers::NONE) => Some(LineEdit::Left),
        (KeyCode::Right, KeyModifiers::NONE) => Some(LineEdit::Right),
        (KeyCode::Home, KeyModifiers::NONE) => Some(LineEdit::Home),
        (KeyCode::End, KeyModifiers::NONE) => Some(LineEdit::End),
        (KeyCode::Left, modifiers)
            if modifiers == KeyModifiers::CONTROL
                || modifiers == KeyModifiers::ALT
                || modifiers == KeyModifiers::CONTROL | KeyModifiers::ALT =>
        {
            Some(LineEdit::WordLeft)
        }
        (KeyCode::Char('b'), KeyModifiers::ALT) => Some(LineEdit::WordLeft),
        (KeyCode::Right, modifiers)
            if modifiers == KeyModifiers::CONTROL
                || modifiers == KeyModifiers::ALT
                || modifiers == KeyModifiers::CONTROL | KeyModifiers::ALT =>
        {
            Some(LineEdit::WordRight)
        }
        (KeyCode::Char('f'), KeyModifiers::ALT) => Some(LineEdit::WordRight),
        (KeyCode::Char('w'), KeyModifiers::CONTROL) => Some(LineEdit::DeleteWordLeft),
        (KeyCode::Char('u'), KeyModifiers::CONTROL) => Some(LineEdit::Clear),
        _ => None,
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
    if app.append_command_filter(value) {
        return RuntimeAction::Redraw;
    }
    if app.append_quick_access_query(value) {
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
        home_state_reads: AtomicUsize,
        refreshes: AtomicUsize,
        home_state_result: Mutex<Option<std::result::Result<VaultHomeState, VaultUiError>>>,
        refresh_result: Mutex<Option<std::result::Result<VaultSnapshot, VaultUiError>>>,
        events: Mutex<Vec<&'static str>>,
        started: Mutex<Option<mpsc::Sender<()>>>,
        release: Mutex<Option<mpsc::Receiver<()>>>,
    }

    impl TrackingBackend {
        fn immediate() -> Self {
            Self {
                locks: AtomicUsize::new(0),
                home_state_reads: AtomicUsize::new(0),
                refreshes: AtomicUsize::new(0),
                home_state_result: Mutex::new(None),
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

        fn with_home_state(result: std::result::Result<VaultHomeState, VaultUiError>) -> Self {
            let backend = Self::immediate();
            *backend.home_state_result.lock().unwrap() = Some(result);
            backend
        }

        fn with_refresh_and_home_state(
            refresh: std::result::Result<VaultSnapshot, VaultUiError>,
            home_state: std::result::Result<VaultHomeState, VaultUiError>,
        ) -> Self {
            let backend = Self::with_refresh(refresh);
            *backend.home_state_result.lock().unwrap() = Some(home_state);
            backend
        }

        fn blocking(started: mpsc::Sender<()>, release: mpsc::Receiver<()>) -> Self {
            Self {
                locks: AtomicUsize::new(0),
                home_state_reads: AtomicUsize::new(0),
                refreshes: AtomicUsize::new(0),
                home_state_result: Mutex::new(None),
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
                home_state: VaultHomeState::Initialized,
            }
        }

        fn home_state(&self) -> std::result::Result<VaultHomeState, VaultUiError> {
            self.home_state_reads.fetch_add(1, Ordering::SeqCst);
            self.home_state_result
                .lock()
                .unwrap()
                .take()
                .unwrap_or(Ok(VaultHomeState::Initialized))
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
            home_state: VaultHomeState::Initialized,
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
            let completion = BackendCompletion::new(
                OperationKind::Import,
                Err(VaultUiError::new(kind, "safe failure")),
                erased.as_ref(),
            );
            apply_completion(&mut app, &erased, Ok(completion));
            assert_eq!(backend.refreshes.load(Ordering::SeqCst), 0);
            assert_eq!(backend.home_state_reads.load(Ordering::SeqCst), 0);
            assert_eq!(backend.locks.load(Ordering::SeqCst), 1);
            assert!(app.snapshot().is_none());
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
                FailureRecovery::ReconcileHomeState
            );
        }
        let missing = VaultUiError::new(VaultUiErrorKind::NotFound, "safe missing failure");
        for kind in [OperationKind::Unlock, OperationKind::Refresh] {
            assert_eq!(
                kind.failure_recovery(&missing),
                FailureRecovery::ReconcileHomeState
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
    fn lifecycle_failures_reconcile_authoritative_home_state() {
        for kind in [OperationKind::Initialize, OperationKind::Restore] {
            for home_state in [
                VaultHomeState::Absent,
                VaultHomeState::Uninitialized,
                VaultHomeState::Initialized,
            ] {
                let backend = Arc::new(TrackingBackend::with_home_state(Ok(home_state)));
                let erased: Arc<dyn VaultBackend> = backend.clone();
                let mut app = App::new(VaultDescriptor {
                    scope: "test".to_owned(),
                    scope_id: None,
                    repo_name: None,
                    home: std::path::PathBuf::from("/tmp/test-vault"),
                    home_state: if home_state.is_initialized() {
                        VaultHomeState::Absent
                    } else {
                        VaultHomeState::Initialized
                    },
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

                assert_eq!(backend.home_state_reads.load(Ordering::SeqCst), 1);
                assert_eq!(backend.locks.load(Ordering::SeqCst), 1);
                assert_eq!(app.descriptor.home_state, home_state);
                match home_state {
                    VaultHomeState::Absent | VaultHomeState::Uninitialized => {
                        assert!(matches!(app.screen, Screen::Missing));
                    }
                    VaultHomeState::Initialized => {
                        assert!(matches!(app.screen, Screen::Locked(_)));
                    }
                }
                assert_eq!(app.status.as_ref().unwrap().text, "safe lifecycle failure");
            }
        }
    }

    #[test]
    fn missing_unlock_and_refresh_reconcile_authoritative_home_state() {
        for kind in [OperationKind::Unlock, OperationKind::Refresh] {
            for home_state in [
                VaultHomeState::Absent,
                VaultHomeState::Uninitialized,
                VaultHomeState::Initialized,
            ] {
                let backend = Arc::new(TrackingBackend::with_home_state(Ok(home_state)));
                let erased: Arc<dyn VaultBackend> = backend.clone();
                let mut app = unlocked_app();
                let completion = BackendCompletion::new(
                    kind,
                    Err(VaultUiError::new(
                        VaultUiErrorKind::NotFound,
                        "safe missing failure",
                    )),
                    erased.as_ref(),
                );

                apply_completion(&mut app, &erased, Ok(completion));

                assert_eq!(backend.home_state_reads.load(Ordering::SeqCst), 1);
                assert_eq!(backend.locks.load(Ordering::SeqCst), 1);
                assert_eq!(app.descriptor.home_state, home_state);
                assert!(app.snapshot().is_none());
                match home_state {
                    VaultHomeState::Absent | VaultHomeState::Uninitialized => {
                        assert!(matches!(app.screen, Screen::Missing));
                    }
                    VaultHomeState::Initialized => {
                        assert!(matches!(app.screen, Screen::Locked(_)));
                    }
                }
                assert_eq!(app.status.as_ref().unwrap().text, "safe missing failure");
            }
        }
    }

    #[test]
    fn read_only_not_found_reconciles_a_vanished_vault() {
        for kind in [
            OperationKind::Activity,
            OperationKind::VerifyAudit,
            OperationKind::ImportPreview,
        ] {
            let backend = Arc::new(TrackingBackend::with_home_state(Ok(VaultHomeState::Absent)));
            let erased: Arc<dyn VaultBackend> = backend.clone();
            let mut app = unlocked_app();
            let completion = BackendCompletion::new(
                kind,
                Err(VaultUiError::new(
                    VaultUiErrorKind::NotFound,
                    "safe missing vault failure",
                )),
                erased.as_ref(),
            );

            apply_completion(&mut app, &erased, Ok(completion));

            assert_eq!(backend.home_state_reads.load(Ordering::SeqCst), 1);
            assert_eq!(backend.locks.load(Ordering::SeqCst), 1);
            assert!(app.snapshot().is_none());
            assert_eq!(app.descriptor.home_state, VaultHomeState::Absent);
            assert!(matches!(app.screen, Screen::Missing));
            assert_eq!(
                app.status.as_ref().unwrap().text,
                "safe missing vault failure"
            );
        }
    }

    #[test]
    fn entity_not_found_in_a_present_vault_remains_an_action_error() {
        let backend = Arc::new(TrackingBackend::with_home_state(Ok(
            VaultHomeState::Initialized,
        )));
        let erased: Arc<dyn VaultBackend> = backend.clone();
        let mut app = unlocked_app();
        let completion = BackendCompletion::new(
            OperationKind::Activity,
            Err(VaultUiError::new(
                VaultUiErrorKind::NotFound,
                "safe entity failure",
            )),
            erased.as_ref(),
        );

        apply_completion(&mut app, &erased, Ok(completion));

        assert_eq!(backend.home_state_reads.load(Ordering::SeqCst), 1);
        assert_eq!(backend.locks.load(Ordering::SeqCst), 0);
        assert!(app.snapshot().is_some());
        assert!(matches!(app.screen, Screen::Browse));
        assert_eq!(app.status.unwrap().text, "safe entity failure");
    }

    #[test]
    fn peek_not_found_uses_the_same_home_state_reconciliation() {
        for home_state in [
            VaultHomeState::Absent,
            VaultHomeState::Uninitialized,
            VaultHomeState::Initialized,
        ] {
            let backend = Arc::new(TrackingBackend::with_home_state(Ok(home_state)));
            let erased: Arc<dyn VaultBackend> = backend.clone();
            let mut app = unlocked_app();

            apply_peek_result(
                &mut app,
                &erased,
                Err(VaultUiError::new(
                    VaultUiErrorKind::NotFound,
                    "safe peek failure",
                )),
            );

            assert_eq!(backend.home_state_reads.load(Ordering::SeqCst), 1);
            match home_state {
                VaultHomeState::Absent | VaultHomeState::Uninitialized => {
                    assert_eq!(backend.locks.load(Ordering::SeqCst), 1);
                    assert!(app.snapshot().is_none());
                    assert!(matches!(app.screen, Screen::Missing));
                }
                VaultHomeState::Initialized => {
                    assert_eq!(backend.locks.load(Ordering::SeqCst), 0);
                    assert!(app.snapshot().is_some());
                    assert!(matches!(app.screen, Screen::Browse));
                }
            }
            assert_eq!(app.status.unwrap().text, "safe peek failure");
        }
    }

    #[test]
    fn failed_lifecycle_home_state_check_fails_closed() {
        let backend = Arc::new(TrackingBackend::with_home_state(Err(VaultUiError::new(
            VaultUiErrorKind::Io,
            "safe home-state failure",
        ))));
        let erased: Arc<dyn VaultBackend> = backend.clone();
        let mut app = App::new(VaultDescriptor {
            scope: "test".to_owned(),
            scope_id: None,
            repo_name: None,
            home: std::path::PathBuf::from("/tmp/test-vault"),
            home_state: VaultHomeState::Absent,
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

        assert_eq!(backend.home_state_reads.load(Ordering::SeqCst), 1);
        assert_eq!(backend.locks.load(Ordering::SeqCst), 1);
        assert_eq!(app.descriptor.home_state, VaultHomeState::Initialized);
        assert!(matches!(app.screen, Screen::Locked(_)));
        assert_eq!(
            app.status.unwrap().text,
            "safe restore failure Vault presence reconciliation also failed: safe home-state failure"
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
        let refreshed = app.snapshot().unwrap().clone();
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
        assert_eq!(app.snapshot(), Some(&refreshed));
        assert!(matches!(app.screen, Screen::Browse));
        assert_eq!(
            app.selected_entry,
            Some(EntryIdentity::Field(original)),
            "failed mutation must preserve the operator's prior selection"
        );
        assert_eq!(app.status.unwrap().text, "safe mutation failure");
    }

    #[test]
    fn committed_result_recovers_a_snapshot_and_reports_primary_success() {
        let mut app = unlocked_app();
        let refreshed = app.snapshot().unwrap().clone();
        let backend = Arc::new(TrackingBackend::with_refresh(Ok(refreshed.clone())));
        let erased: Arc<dyn VaultBackend> = backend.clone();
        let completion = BackendCompletion::new(
            OperationKind::Mutation,
            Ok(VaultActionResult::Committed {
                action: VaultCommittedAction::Mutated,
                refresh_error: VaultUiError::new(
                    VaultUiErrorKind::Io,
                    "safe initial refresh failure",
                ),
            }),
            erased.as_ref(),
        );

        apply_completion(&mut app, &erased, Ok(completion));

        assert_eq!(backend.refreshes.load(Ordering::SeqCst), 1);
        assert_eq!(backend.home_state_reads.load(Ordering::SeqCst), 0);
        assert_eq!(backend.locks.load(Ordering::SeqCst), 0);
        assert_eq!(app.snapshot(), Some(&refreshed));
        assert!(matches!(app.screen, Screen::Browse));
        assert_eq!(app.status.unwrap().text, "Vault updated.");
    }

    #[test]
    fn committed_result_remains_successful_when_snapshot_recovery_fails() {
        let backend = Arc::new(TrackingBackend::with_refresh_and_home_state(
            Err(VaultUiError::new(
                VaultUiErrorKind::Io,
                "safe retry failure",
            )),
            Ok(VaultHomeState::Initialized),
        ));
        let erased: Arc<dyn VaultBackend> = backend.clone();
        let mut app = unlocked_app();
        let completion = BackendCompletion::new(
            OperationKind::Mutation,
            Ok(VaultActionResult::Committed {
                action: VaultCommittedAction::Mutated,
                refresh_error: VaultUiError::new(
                    VaultUiErrorKind::Io,
                    "safe initial refresh failure",
                ),
            }),
            erased.as_ref(),
        );

        apply_completion(&mut app, &erased, Ok(completion));

        assert_eq!(backend.refreshes.load(Ordering::SeqCst), 1);
        assert_eq!(backend.home_state_reads.load(Ordering::SeqCst), 1);
        assert_eq!(backend.locks.load(Ordering::SeqCst), 1);
        assert!(app.snapshot().is_none());
        assert!(matches!(app.screen, Screen::Locked(_)));
        assert_eq!(
            app.status.unwrap().text,
            "Vault update completed, but its metadata refresh failed: safe initial refresh failure The automatic retry also failed: safe retry failure"
        );
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
            assert_eq!(backend.home_state_reads.load(Ordering::SeqCst), 1);
            assert_eq!(backend.locks.load(Ordering::SeqCst), 1);
            assert!(app.snapshot().is_none());
            assert!(matches!(app.screen, Screen::Locked(_)));
            assert_eq!(
                app.status.unwrap().text,
                "safe import failure Vault metadata recovery also failed: safe recovery failure"
            );
        }
    }

    #[test]
    fn failed_recovery_refresh_transitions_to_missing_when_vault_vanished() {
        let backend = Arc::new(TrackingBackend::with_refresh_and_home_state(
            Err(VaultUiError::new(
                VaultUiErrorKind::NotFound,
                "safe recovery failure",
            )),
            Ok(VaultHomeState::Absent),
        ));
        let erased: Arc<dyn VaultBackend> = backend.clone();
        let mut app = unlocked_app();
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
        assert_eq!(backend.home_state_reads.load(Ordering::SeqCst), 1);
        assert_eq!(backend.locks.load(Ordering::SeqCst), 1);
        assert_eq!(app.descriptor.home_state, VaultHomeState::Absent);
        assert!(app.snapshot().is_none());
        assert!(matches!(app.screen, Screen::Missing));
        assert_eq!(
            app.status.unwrap().text,
            "safe mutation failure Vault metadata recovery also failed: safe recovery failure"
        );
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
