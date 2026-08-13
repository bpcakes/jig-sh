use std::path::PathBuf;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use jig_vault::{FieldKind, MAX_SECRET_VALUE_LEN, SecretBytes, Vault, VaultSnapshot};
use ratatui::{Terminal, backend::TestBackend};
use secrecy::SecretString;
use tempfile::tempdir;

use crate::{
    VaultDescriptor,
    model::{App, EntryIdentity, Focus, ItemIdentity, Screen},
    render,
    runtime::{RuntimeAction, handle_key, handle_paste},
    secret_input::SecretInput,
};

const SENTINEL: &[u8] = b"vault-tui-plaintext-sentinel";

fn descriptor(exists: bool) -> VaultDescriptor {
    VaultDescriptor {
        scope: "repo".to_owned(),
        scope_id: Some("scope_123".to_owned()),
        repo_name: Some("demo".to_owned()),
        home: PathBuf::from("/tmp/demo-vault"),
        exists,
    }
}

fn snapshot() -> VaultSnapshot {
    let temp = tempdir().unwrap();
    let vault = Vault::resolve_for_test(Some(temp.path().join("vault"))).unwrap();
    let passphrase = SecretString::from("correct horse battery staple".to_owned());
    vault.init(&passphrase).unwrap();
    vault
        .set_field(
            &passphrase,
            "jig://Production/API_TOKEN".parse().unwrap(),
            FieldKind::Concealed,
            SecretBytes::new(SENTINEL.to_vec()),
        )
        .unwrap();
    vault
        .set_field(
            &passphrase,
            "jig://Production/API_URL".parse().unwrap(),
            FieldKind::Text,
            SecretBytes::new(b"https://example.invalid".to_vec()),
        )
        .unwrap();
    vault
        .set_field(
            &passphrase,
            "jig://Staging/API_TOKEN".parse().unwrap(),
            FieldKind::Concealed,
            SecretBytes::new(b"staging-secret".to_vec()),
        )
        .unwrap();
    vault
        .set_secret(
            &passphrase,
            "old_token",
            SecretBytes::new(b"legacy-secret".to_vec()),
        )
        .unwrap();
    vault.snapshot(&passphrase).unwrap()
}

fn browsing_app() -> App {
    let mut app = App::new(descriptor(true));
    app.apply_snapshot(snapshot());
    app
}

#[test]
fn protected_input_debug_and_render_label_do_not_expose_source_text() {
    let mut input = SecretInput::new();
    input.paste("top-secret-界").unwrap();

    let debug = format!("{input:?}");
    let rendered = input.render_label();
    assert!(!debug.contains("top-secret"), "{debug}");
    assert!(!rendered.contains("top-secret"), "{rendered}");
    assert!(rendered.contains("bytes"), "{rendered}");

    input.backspace();
    assert_eq!(input.len(), "top-secret-".len());
    input.clear();
    assert!(input.is_empty());
}

#[test]
fn protected_paste_rejects_the_complete_overflow() {
    let mut input = SecretInput::new();
    let oversized = "x".repeat(MAX_SECRET_VALUE_LEN + 1);
    assert!(input.paste(&oversized).is_err());
    assert!(input.is_empty());
}

#[test]
fn snapshot_keeps_canonical_and_legacy_entries_separate() {
    let app = browsing_app();
    assert_eq!(
        app.visible_items(),
        vec![
            ItemIdentity::Canonical("Production".to_owned()),
            ItemIdentity::Canonical("Staging".to_owned()),
            ItemIdentity::Legacy,
        ]
    );
    assert_eq!(
        app.selected_entry,
        Some(EntryIdentity::Field(
            "jig://Production/API_TOKEN".parse().unwrap()
        ))
    );
}

#[test]
fn metadata_filter_searches_reference_kind_and_legacy_name() {
    let mut app = browsing_app();
    for character in "api_url".chars() {
        app.push_filter(character);
    }
    assert_eq!(
        app.visible_items(),
        vec![ItemIdentity::Canonical("Production".to_owned())]
    );
    assert_eq!(app.visible_entries().len(), 1);

    app.clear_filter();
    for character in "old_token".chars() {
        app.push_filter(character);
    }
    assert_eq!(app.visible_items(), vec![ItemIdentity::Legacy]);
    assert_eq!(
        app.selected_entry,
        Some(EntryIdentity::Legacy("old_token".to_owned()))
    );
}

#[test]
fn exact_selection_survives_reordered_refresh() {
    let mut app = browsing_app();
    app.focus = Focus::Fields;
    app.move_selection(1);
    let selected = app.selected_entry.clone();
    let mut refreshed = app.snapshot.clone().unwrap();
    refreshed.fields.reverse();

    app.apply_snapshot(refreshed);

    assert_eq!(app.selected_entry, selected);
}

#[test]
fn locked_typing_and_paste_never_put_plaintext_in_a_frame() {
    let mut app = App::new(descriptor(true));
    assert!(matches!(
        handle_paste(&mut app, "vault-tui-plaintext-sentinel"),
        RuntimeAction::Redraw
    ));
    let backend = TestBackend::new(90, 24);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal.draw(|frame| render::draw(frame, &app)).unwrap();
    let rendered = terminal.backend().to_string();

    assert!(rendered.contains("Vault passphrase"), "{rendered}");
    assert!(
        !rendered.contains("vault-tui-plaintext-sentinel"),
        "{rendered}"
    );
}

#[test]
fn wide_browser_renders_three_metadata_panes_without_values() {
    let backend = TestBackend::new(132, 32);
    let mut terminal = Terminal::new(backend).unwrap();
    let app = browsing_app();
    terminal.draw(|frame| render::draw(frame, &app)).unwrap();
    let rendered = terminal.backend().to_string();

    assert!(rendered.contains("Items"), "{rendered}");
    assert!(rendered.contains("Fields"), "{rendered}");
    assert!(rendered.contains("Details"), "{rendered}");
    assert!(
        rendered.contains("jig://Production/API_TOKEN"),
        "{rendered}"
    );
    assert!(rendered.contains("Value hidden"), "{rendered}");
    assert!(
        !rendered.contains(std::str::from_utf8(SENTINEL).unwrap()),
        "{rendered}"
    );
}

#[test]
fn compact_browser_uses_focused_pane_and_breadcrumb() {
    let backend = TestBackend::new(70, 22);
    let mut terminal = Terminal::new(backend).unwrap();
    let mut app = browsing_app();
    app.focus = Focus::Details;
    terminal.draw(|frame| render::draw(frame, &app)).unwrap();
    let rendered = terminal.backend().to_string();

    assert!(rendered.contains("Details"), "{rendered}");
    assert!(rendered.contains("Production"), "{rendered}");
    assert!(rendered.contains("API_TOKEN"), "{rendered}");
    assert!(
        !rendered.contains(std::str::from_utf8(SENTINEL).unwrap()),
        "{rendered}"
    );
}

#[test]
fn minimum_size_message_is_actionable() {
    let backend = TestBackend::new(40, 8);
    let mut terminal = Terminal::new(backend).unwrap();
    let app = browsing_app();
    terminal.draw(|frame| render::draw(frame, &app)).unwrap();
    let rendered = terminal.backend().to_string();

    assert!(rendered.contains("Terminal too small"), "{rendered}");
    assert!(rendered.contains("press q"), "{rendered}");
}

#[test]
fn browsing_keys_change_focus_search_migrate_lock_and_quit() {
    let mut app = browsing_app();
    assert!(matches!(
        handle_key(&mut app, KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE)),
        RuntimeAction::Redraw
    ));
    assert_eq!(app.focus, Focus::Fields);
    assert!(matches!(
        handle_key(
            &mut app,
            KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE)
        ),
        RuntimeAction::Redraw
    ));
    assert!(app.searching);
    assert!(matches!(
        handle_key(&mut app, KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)),
        RuntimeAction::Redraw
    ));
    assert!(!app.searching);
    assert!(matches!(
        handle_key(
            &mut app,
            KeyEvent::new(KeyCode::Char('L'), KeyModifiers::SHIFT)
        ),
        RuntimeAction::Lock
    ));
    assert!(matches!(
        handle_key(
            &mut app,
            KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE)
        ),
        RuntimeAction::Quit
    ));
}

#[test]
fn absent_vault_can_enter_and_cancel_initialization() {
    let mut app = App::new(descriptor(false));
    handle_key(
        &mut app,
        KeyEvent::new(KeyCode::Char('i'), KeyModifiers::NONE),
    );
    assert!(matches!(app.screen, Screen::Initialize { .. }));
    handle_key(&mut app, KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    assert!(matches!(app.screen, Screen::Missing));
}
