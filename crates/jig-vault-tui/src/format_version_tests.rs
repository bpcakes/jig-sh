//! Capability and confirmation behavior for each readable vault format.

use std::path::PathBuf;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use jig_vault::{LATEST_VAULT_FORMAT_VERSION, Vault, VaultHomeState, VaultSnapshot};
use ratatui::{Terminal, backend::TestBackend};
use secrecy::SecretString;

use crate::{
    VaultAction, VaultDescriptor,
    commands::{CommandAvailability, PlatformCapabilities, UiCommand},
    model::{App, Screen},
    render,
    runtime::{BackendRequest, RuntimeAction, handle_key},
};

fn snapshot_for_format(version: u32) -> VaultSnapshot {
    let temp = tempfile::tempdir().unwrap();
    let vault = Vault::resolve_for_test(Some(temp.path().join("vault"))).unwrap();
    let passphrase = SecretString::from("correct horse battery staple".to_owned());
    vault.init_format_for_test(&passphrase, version).unwrap();
    vault.snapshot(&passphrase).unwrap()
}

fn app_for_format(version: u32) -> App {
    let mut app = App::new(VaultDescriptor {
        scope: "repo".to_owned(),
        scope_id: Some("scope_123".to_owned()),
        repo_name: Some("demo".to_owned()),
        home: PathBuf::from("/tmp/demo-vault"),
        home_state: VaultHomeState::Initialized,
    });
    app.apply_snapshot(snapshot_for_format(version));
    app
}

fn all_platforms() -> PlatformCapabilities {
    PlatformCapabilities::ALL
}

fn enabled(app: &App, command: UiCommand) -> bool {
    command.visible_in_state(app)
        && matches!(
            command.availability_with_capabilities(app, all_platforms()),
            CommandAvailability::Enabled
        )
}

fn render(app: &App) -> String {
    let mut terminal = Terminal::new(TestBackend::new(110, 30)).unwrap();
    terminal.draw(|frame| render::draw(frame, app)).unwrap();
    terminal.backend().to_string()
}

fn confirm_migration_from(version: u32) {
    let mut app = app_for_format(version);
    assert!(enabled(&app, UiCommand::MigrateToLatest), "v{version}");
    handle_key(
        &mut app,
        KeyEvent::new(KeyCode::Char('m'), KeyModifiers::NONE),
    );
    assert!(matches!(app.screen, Screen::ConfirmMigration), "v{version}");
    let rendered = render(&app);
    assert!(
        rendered.contains(&format!(
            "Migrate vault from version {version} to version {LATEST_VAULT_FORMAT_VERSION}?"
        )),
        "{rendered}"
    );
    assert!(
        rendered.contains("Older Jig versions will reject"),
        "{rendered}"
    );
    assert!(matches!(
        handle_key(&mut app, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
        RuntimeAction::Start(BackendRequest::Execute(VaultAction::MigrateToLatest))
    ));
}

/// Commands that every field-kind format exposes once unlocked.
const MANAGEMENT: [UiCommand; 6] = [
    UiCommand::CreateItem,
    UiCommand::AddLegacy,
    UiCommand::ImportOnePassword,
    UiCommand::CreateBackup,
    UiCommand::ChangePassphrase,
    UiCommand::Activity,
];

#[test]
fn version_one_is_read_only_and_offers_only_explicit_migration() {
    let app = app_for_format(1);
    for command in MANAGEMENT {
        if command != UiCommand::Activity {
            assert!(!enabled(&app, command), "{command:?}");
        }
    }
    assert!(enabled(&app, UiCommand::Activity));
    confirm_migration_from(1);
}

#[test]
fn version_two_keeps_management_and_offers_migration_to_latest() {
    let app = app_for_format(2);
    for command in MANAGEMENT {
        assert!(enabled(&app, command), "{command:?}");
    }
    assert!(!UiCommand::RestoreBackup.visible_in_state(&app));
    confirm_migration_from(2);
}

#[test]
fn latest_version_exposes_complete_controls_without_migration() {
    let mut app = app_for_format(LATEST_VAULT_FORMAT_VERSION);
    for command in MANAGEMENT {
        assert!(enabled(&app, command), "{command:?}");
    }
    for command in UiCommand::ALL {
        let expected = !matches!(
            command,
            UiCommand::MigrateToLatest | UiCommand::RestoreBackup
        );
        assert_eq!(command.visible_in_state(&app), expected, "{command:?}");
    }
    assert_eq!(
        UiCommand::MigrateToLatest.availability_with_capabilities(&app, all_platforms()),
        CommandAvailability::Disabled("The vault already uses the latest version.")
    );
    handle_key(
        &mut app,
        KeyEvent::new(KeyCode::Char('m'), KeyModifiers::NONE),
    );
    assert!(!matches!(app.screen, Screen::ConfirmMigration));
    let rendered = render(&app);
    assert!(
        rendered.contains(&format!("v{LATEST_VAULT_FORMAT_VERSION}")),
        "{rendered}"
    );
}
