use std::{
    sync::Arc,
    time::{Duration, Instant},
};

use anyhow::{Context, Result};
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers};
use jig_tui::{CooperativeWorker, TerminalSession, is_actionable_key, require_terminal};
use jig_vault::{SecretBytes, VaultSnapshot};

use crate::{
    VaultAction, VaultActionResult, VaultBackend, VaultUiError, VaultUiErrorKind,
    model::{App, Focus, Screen},
    render,
};

const EVENT_POLL_INTERVAL: Duration = Duration::from_millis(50);
const SPINNER_INTERVAL: Duration = Duration::from_millis(100);

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
            let action = match event::read().context("failed to read Vault TUI input")? {
                Event::Key(key) if is_actionable_key(key) => handle_key(&mut app, key),
                Event::Paste(value) => handle_paste(&mut app, &value),
                Event::Resize(_, _) => RuntimeAction::Redraw,
                _ => RuntimeAction::Ignore,
            };
            match action {
                RuntimeAction::Ignore => {}
                RuntimeAction::Redraw => dirty = true,
                RuntimeAction::Lock => {
                    if worker.is_none() {
                        backend.lock();
                        app.lock();
                        dirty = true;
                    }
                }
                RuntimeAction::Start(request) => {
                    if worker.is_none() {
                        worker = Some(ActionWorker::spawn(Arc::clone(&backend), request)?);
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
    match completion.result {
        Ok(VaultActionResult::Snapshot(snapshot)) => {
            app.apply_snapshot(snapshot);
            let message = match completion.kind {
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
                | OperationKind::Backup
                | OperationKind::Restore => "Vault operation completed.",
            };
            app.set_info(message);
        }
        Ok(VaultActionResult::Activity(records)) => app.apply_activity(records),
        Ok(VaultActionResult::Audit(verification)) => app.apply_audit_result(verification),
        Ok(VaultActionResult::ImportPreview(preview)) => app.apply_import_preview(preview),
        Ok(VaultActionResult::BackupCreated {
            output,
            bytes_written,
            backup_version,
            snapshot,
        }) => {
            app.apply_snapshot(snapshot);
            app.set_info(&format!(
                "Encrypted backup v{backup_version} written to {} ({bytes_written} bytes).",
                output.display()
            ));
        }
        Ok(VaultActionResult::Restored { .. }) => app.apply_restore(),
        Ok(_) => app.fail_action(&VaultUiError::new(
            VaultUiErrorKind::Other,
            "Vault backend returned an unexpected action result.",
        )),
        Err(error) => {
            if error.kind() == VaultUiErrorKind::Authentication {
                backend.lock();
                if completion.kind == OperationKind::Restore {
                    app.fail_restore(&error);
                } else {
                    app.fail_unlock(&error);
                }
                return;
            }
            if let Some(Ok(snapshot)) = completion.refreshed_after_error {
                app.apply_snapshot(snapshot);
                app.set_error(error.message());
                return;
            }
            match completion.kind {
                OperationKind::Unlock => app.fail_unlock(&error),
                OperationKind::Initialize => app.fail_initialize(&error),
                OperationKind::Restore => app.fail_restore(&error),
                OperationKind::Refresh
                | OperationKind::Migrate
                | OperationKind::Mutation
                | OperationKind::Activity
                | OperationKind::VerifyAudit
                | OperationKind::ImportPreview
                | OperationKind::Import
                | OperationKind::Backup
                | OperationKind::Passphrase => app.fail_action(&error),
            }
        }
    }
}

#[derive(Debug)]
pub(crate) enum RuntimeAction {
    Ignore,
    Redraw,
    Lock,
    Start(BackendRequest),
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
            Self::Execute(VaultAction::Activity { .. }) => OperationKind::Activity,
            Self::Execute(VaultAction::VerifyAudit) => OperationKind::VerifyAudit,
            Self::Execute(VaultAction::ImportOnePassword { preview: true, .. }) => {
                OperationKind::ImportPreview
            }
            Self::Execute(VaultAction::ImportOnePassword { .. }) => OperationKind::Import,
            Self::Execute(VaultAction::CreateBackup { .. }) => OperationKind::Backup,
            Self::Execute(VaultAction::ChangePassphrase { .. }) => OperationKind::Passphrase,
            Self::Execute(VaultAction::RestoreBackup { .. }) => OperationKind::Restore,
            Self::Execute(_) => OperationKind::Mutation,
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
    Import,
    Backup,
    Passphrase,
    Restore,
}

struct BackendCompletion {
    kind: OperationKind,
    result: std::result::Result<VaultActionResult, VaultUiError>,
    refreshed_after_error: Option<std::result::Result<VaultSnapshot, VaultUiError>>,
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
            let refreshed_after_error =
                if matches!(kind, OperationKind::Mutation | OperationKind::Import)
                    && result.as_ref().is_err_and(|error| {
                        matches!(
                            error.kind(),
                            VaultUiErrorKind::Conflict | VaultUiErrorKind::NotFound
                        )
                    })
                {
                    Some(backend.refresh())
                } else {
                    None
                };
            BackendCompletion {
                kind,
                result,
                refreshed_after_error,
            }
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
        Screen::Locked(_) => return handle_protected_key(app, key, false),
        Screen::Initialize { .. } => return handle_protected_key(app, key, true),
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
        Screen::ConfirmDelete(_) => return handle_delete_key(app, key),
        Screen::Tools(_) => return handle_tools_key(app, key),
        Screen::ToolForm(_) => return handle_tool_form_key(app, key),
        Screen::ImportPreview(_) => return handle_import_preview_key(app, key),
        Screen::Activity(_) => return handle_activity_key(app, key),
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
                app.push_filter(character);
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

fn handle_protected_key(app: &mut App, key: KeyEvent, initializing: bool) -> RuntimeAction {
    match key.code {
        KeyCode::Esc if initializing => {
            app.cancel_initialize();
            RuntimeAction::Redraw
        }
        KeyCode::Esc => RuntimeAction::Quit,
        KeyCode::Tab | KeyCode::BackTab if initializing => {
            app.toggle_initialize_focus();
            RuntimeAction::Redraw
        }
        KeyCode::Enter if initializing => app
            .begin_initialize()
            .map_or(RuntimeAction::Redraw, |passphrase| {
                RuntimeAction::Start(BackendRequest::Initialize(passphrase))
            }),
        KeyCode::Enter => app
            .begin_unlock()
            .map_or(RuntimeAction::Redraw, |passphrase| {
                RuntimeAction::Start(BackendRequest::Unlock(passphrase))
            }),
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
            if !initializing
                && character == 'q'
                && app
                    .protected_input_mut()
                    .is_some_and(|input| input.is_empty())
            {
                return RuntimeAction::Quit;
            }
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
            if app.metadata_input_mut().is_some() || app.protected_input_mut().is_some() {
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
            } else if let Some(input) = app.metadata_input_mut() {
                input.pop();
            }
            RuntimeAction::Redraw
        }
        KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            if let Some(input) = app.protected_input_mut() {
                input.clear();
            } else if let Some(input) = app.metadata_input_mut() {
                input.clear();
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
        KeyCode::Esc => {
            app.close_overlay();
            RuntimeAction::Redraw
        }
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
            if let Some(input) = app.metadata_input_mut() {
                input.pop();
            }
            RuntimeAction::Redraw
        }
        KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            if let Some(input) = app.metadata_input_mut() {
                input.clear();
            }
            RuntimeAction::Redraw
        }
        KeyCode::Char(character)
            if !key
                .modifiers
                .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
        {
            if let Some(input) = app.metadata_input_mut() {
                input.push(character);
            }
            RuntimeAction::Redraw
        }
        _ => RuntimeAction::Ignore,
    }
}

fn handle_delete_key(app: &mut App, key: KeyEvent) -> RuntimeAction {
    match key.code {
        KeyCode::Esc => {
            app.close_overlay();
            RuntimeAction::Redraw
        }
        KeyCode::Enter => app.submit_delete().map_or(RuntimeAction::Redraw, |action| {
            RuntimeAction::Start(BackendRequest::Execute(action))
        }),
        KeyCode::Backspace => {
            if let Some(input) = app.metadata_input_mut() {
                input.pop();
            }
            RuntimeAction::Redraw
        }
        KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            if let Some(input) = app.metadata_input_mut() {
                input.clear();
            }
            RuntimeAction::Redraw
        }
        KeyCode::Char(character)
            if !key
                .modifiers
                .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
        {
            if let Some(input) = app.metadata_input_mut() {
                input.push(character);
            }
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
    } else if let Some(input) = app.metadata_input_mut() {
        input.push(character);
    }
    RuntimeAction::Redraw
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
    if let Some(input) = app.metadata_input_mut() {
        input.push_str(value);
        return RuntimeAction::Redraw;
    }
    if app.searching {
        for character in value.chars() {
            app.push_filter(character);
        }
        return RuntimeAction::Redraw;
    }
    RuntimeAction::Ignore
}

#[allow(dead_code)]
fn _snapshot_is_metadata_only(_: &VaultSnapshot) {}
