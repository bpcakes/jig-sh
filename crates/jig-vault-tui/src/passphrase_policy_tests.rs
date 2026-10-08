//! New-passphrase policy feedback in the initialization and change flows.

use std::path::PathBuf;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use jig_vault::{NEW_VAULT_PASSPHRASE_POLICY, Vault, VaultHomeState};
use ratatui::{Terminal, backend::TestBackend};
use secrecy::SecretString;

use crate::{
    VaultAction, VaultDescriptor,
    commands::{CommandOutcome, UiCommand},
    model::{App, Screen},
    render,
    runtime::{BackendRequest, RuntimeAction, handle_key, handle_paste},
};

/// Long enough, but trivially guessable.
const GUESSABLE: &str = "passwordpasswordpassword";
const STRONG: &str = "otter-quartz-lantern-mosaic-velvet";

fn app(home_state: VaultHomeState) -> App {
    App::new(VaultDescriptor {
        scope: "repo".to_owned(),
        scope_id: Some("scope_123".to_owned()),
        repo_name: Some("demo".to_owned()),
        home: PathBuf::from("/tmp/demo-vault"),
        home_state,
    })
}

fn unlocked_app() -> App {
    let temp = tempfile::tempdir().unwrap();
    let vault = Vault::resolve_for_test(Some(temp.path().join("vault"))).unwrap();
    let passphrase = SecretString::from("correct horse battery staple".to_owned());
    vault.init(&passphrase).unwrap();
    let mut app = app(VaultHomeState::Initialized);
    app.apply_snapshot(vault.snapshot(&passphrase).unwrap());
    app
}

fn enter_pair(app: &mut App, value: &str) -> RuntimeAction {
    handle_paste(app, value);
    handle_key(app, KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    handle_paste(app, value);
    handle_key(app, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
}

fn rendered(app: &App) -> String {
    let mut terminal = Terminal::new(TestBackend::new(110, 30)).unwrap();
    terminal.draw(|frame| render::draw(frame, app)).unwrap();
    terminal.backend().to_string()
}

#[test]
fn initialization_reports_the_policy_without_consuming_or_showing_input() {
    let mut weak = app(VaultHomeState::Absent);
    weak.begin_initialize_form();
    assert!(matches!(
        enter_pair(&mut weak, GUESSABLE),
        RuntimeAction::Redraw
    ));
    let Screen::Initialize { passphrase, .. } = &weak.screen else {
        panic!("a guessable passphrase left the initialization form");
    };
    assert_eq!(passphrase.len(), GUESSABLE.len());
    assert_eq!(
        weak.status.as_ref().unwrap().text,
        NEW_VAULT_PASSPHRASE_POLICY
    );
    assert!(!rendered(&weak).contains(GUESSABLE));

    let mut strong = app(VaultHomeState::Absent);
    strong.begin_initialize_form();
    assert!(matches!(
        enter_pair(&mut strong, STRONG),
        RuntimeAction::Start(BackendRequest::Initialize(_))
    ));
}

#[test]
fn passphrase_change_defers_policy_to_the_recovery_capable_backend() {
    let mut weak = unlocked_app();
    assert!(matches!(
        weak.activate_direct_command(UiCommand::ChangePassphrase),
        CommandOutcome::Redraw
    ));
    assert!(matches!(
        enter_pair(&mut weak, GUESSABLE),
        RuntimeAction::Start(BackendRequest::Execute(
            VaultAction::ChangePassphrase { .. }
        ))
    ));
    assert!(!rendered(&weak).contains(GUESSABLE));

    let mut strong = unlocked_app();
    assert!(matches!(
        strong.activate_direct_command(UiCommand::ChangePassphrase),
        CommandOutcome::Redraw
    ));
    assert!(matches!(
        enter_pair(&mut strong, STRONG),
        RuntimeAction::Start(BackendRequest::Execute(
            VaultAction::ChangePassphrase { .. }
        ))
    ));
}
