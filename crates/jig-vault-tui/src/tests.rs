use std::path::PathBuf;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use jig_vault::{FieldKind, MAX_SECRET_VALUE_LEN, SecretBytes, Vault, VaultSnapshot};
use ratatui::{Terminal, backend::TestBackend};
use secrecy::SecretString;
use tempfile::tempdir;

use crate::{
    ImportFieldChange, ImportPlanToken, ImportPreview, ImportPreviewAuthorization,
    ImportPreviewRow, VaultAction, VaultDescriptor, VaultMutation,
    commands::{
        CommandAvailability, CommandOutcome, CommandPaletteScope, PlatformCapabilities, UiCommand,
    },
    line_editor::{METADATA_INPUT_LIMIT, SEARCH_INPUT_LIMIT},
    model::{
        App, DeleteTarget, EntryIdentity, FieldWriteFocus, FieldWriteIntent, Focus, ItemIdentity,
        ManagementForm, MutationConfirmation, MutationConfirmationKind, Screen,
    },
    quick_access::QuickAccessTarget,
    render,
    runtime::{BackendRequest, RuntimeAction, handle_key, handle_paste},
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

fn empty_snapshot() -> VaultSnapshot {
    let temp = tempdir().unwrap();
    let vault = Vault::resolve_for_test(Some(temp.path().join("empty-vault"))).unwrap();
    let passphrase = SecretString::from("correct horse battery staple".to_owned());
    vault.init(&passphrase).unwrap();
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
fn protected_input_loads_exact_binary_regular_file_and_rejects_oversize() {
    let temp = tempdir().unwrap();
    let source = temp.path().join("value.bin");
    let exact = b"binary\0value\xff\n";
    std::fs::write(&source, exact).unwrap();

    let mut input = SecretInput::from_regular_file(&source).unwrap();
    assert_eq!(input.take().as_slice(), exact);

    let oversized = temp.path().join("oversized.bin");
    let file = std::fs::File::create(&oversized).unwrap();
    file.set_len((MAX_SECRET_VALUE_LEN + 1) as u64).unwrap();
    let error = SecretInput::from_regular_file(&oversized)
        .unwrap_err()
        .to_string();
    assert!(error.contains("exceeds"), "{error}");
}

#[cfg(unix)]
#[test]
fn protected_input_rejects_symlink_value_sources() {
    use std::os::unix::fs::symlink;

    let temp = tempdir().unwrap();
    let source = temp.path().join("value.bin");
    let link = temp.path().join("value-link.bin");
    std::fs::write(&source, b"safe-size-secret").unwrap();
    symlink(&source, &link).unwrap();

    let error = SecretInput::from_regular_file(&link)
        .unwrap_err()
        .to_string();
    assert!(error.contains("must not be a symlink"), "{error}");
}

#[cfg(windows)]
#[test]
fn protected_input_rejects_windows_reparse_value_sources() {
    use std::os::windows::fs::symlink_file;

    let temp = tempdir().unwrap();
    let source = temp.path().join("value.bin");
    let link = temp.path().join("value-link.bin");
    std::fs::write(&source, b"safe-size-secret").unwrap();
    if let Err(error) = symlink_file(&source, &link) {
        if error.kind() == std::io::ErrorKind::PermissionDenied
            || error.raw_os_error()
                == Some(windows_sys::Win32::Foundation::ERROR_PRIVILEGE_NOT_HELD as i32)
        {
            return;
        }
        panic!("failed to create Windows symlink fixture: {error}");
    }

    let error = SecretInput::from_regular_file(&link)
        .unwrap_err()
        .to_string();
    assert!(error.contains("symlink or reparse point"), "{error}");
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
    app.append_filter("api_url");
    assert_eq!(
        app.visible_items(),
        vec![ItemIdentity::Canonical("Production".to_owned())]
    );
    assert_eq!(app.visible_entries().len(), 1);

    app.clear_filter();
    app.append_filter("old_token");
    assert_eq!(app.visible_items(), vec![ItemIdentity::Legacy]);
    assert_eq!(
        app.selected_entry,
        Some(EntryIdentity::Legacy("old_token".to_owned()))
    );
}

#[test]
fn metadata_and_search_pastes_are_bounded_and_rejected_atomically() {
    let mut app = browsing_app();
    app.begin_add();
    handle_paste(&mut app, "PREFIX");
    handle_paste(&mut app, &"x".repeat(METADATA_INPUT_LIMIT));
    let Screen::Form(ManagementForm::WriteField { field, .. }) = &app.screen else {
        panic!("expected field form");
    };
    assert_eq!(field.as_str(), "PREFIX");
    assert!(
        app.status
            .as_ref()
            .is_some_and(|status| status.text.contains("interactive size limit"))
    );

    app.close_overlay();
    app.searching = true;
    app.append_filter("api");
    handle_paste(&mut app, &"x".repeat(SEARCH_INPUT_LIMIT));
    assert_eq!(app.filter.as_str(), "api");
    assert!(
        app.status
            .as_ref()
            .is_some_and(|status| status.text.contains("interactive size limit"))
    );
}

#[test]
fn metadata_editor_supports_cursor_delete_and_word_editing() {
    let mut app = browsing_app();
    app.begin_create_item();
    handle_paste(&mut app, "ac");
    handle_key(&mut app, KeyEvent::new(KeyCode::Left, KeyModifiers::NONE));
    handle_key(
        &mut app,
        KeyEvent::new(KeyCode::Char('b'), KeyModifiers::NONE),
    );
    let Screen::Form(ManagementForm::WriteField { item, .. }) = &app.screen else {
        panic!("expected create-item form");
    };
    assert_eq!(item.as_str(), "abc");
    assert_eq!(item.cursor(), 2);

    handle_key(&mut app, KeyEvent::new(KeyCode::Home, KeyModifiers::NONE));
    handle_key(&mut app, KeyEvent::new(KeyCode::Delete, KeyModifiers::NONE));
    handle_key(&mut app, KeyEvent::new(KeyCode::End, KeyModifiers::NONE));
    handle_key(
        &mut app,
        KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE),
    );
    let Screen::Form(ManagementForm::WriteField { item, .. }) = &app.screen else {
        panic!("expected create-item form");
    };
    assert_eq!(item.as_str(), "b");

    handle_key(
        &mut app,
        KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL),
    );
    handle_paste(&mut app, "alpha beta gamma");
    handle_key(
        &mut app,
        KeyEvent::new(KeyCode::Left, KeyModifiers::CONTROL),
    );
    handle_key(
        &mut app,
        KeyEvent::new(KeyCode::Char('w'), KeyModifiers::CONTROL),
    );
    handle_paste(&mut app, "new ");
    let Screen::Form(ManagementForm::WriteField { item, .. }) = &app.screen else {
        panic!("expected create-item form");
    };
    assert_eq!(item.as_str(), "alpha new gamma");
}

#[test]
fn search_and_command_palette_share_insertion_cursor_behavior() {
    let mut app = browsing_app();
    handle_key(
        &mut app,
        KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE),
    );
    handle_paste(&mut app, "api token");
    handle_key(
        &mut app,
        KeyEvent::new(KeyCode::Left, KeyModifiers::CONTROL),
    );
    handle_paste(&mut app, "new ");
    assert_eq!(app.filter.as_str(), "api new token");
    handle_key(&mut app, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    app.clear_filter();

    handle_key(
        &mut app,
        KeyEvent::new(KeyCode::Char(':'), KeyModifiers::NONE),
    );
    handle_paste(&mut app, "backp");
    handle_key(&mut app, KeyEvent::new(KeyCode::Left, KeyModifiers::NONE));
    handle_key(
        &mut app,
        KeyEvent::new(KeyCode::Char('u'), KeyModifiers::NONE),
    );
    let Screen::Commands(palette) = &app.screen else {
        panic!("expected command palette");
    };
    assert_eq!(palette.filter.as_str(), "backup");
    assert_eq!(
        palette.selected_entry().map(|entry| entry.command),
        Some(UiCommand::CreateBackup)
    );
}

#[test]
fn quick_access_fuzzy_searches_metadata_without_rendering_values() {
    let mut app = browsing_app();
    app.searching = true;
    app.append_filter("old");

    assert!(matches!(
        handle_key(
            &mut app,
            KeyEvent::new(KeyCode::Char('p'), KeyModifiers::CONTROL),
        ),
        RuntimeAction::Redraw
    ));
    assert!(!app.searching);
    handle_paste(&mut app, "apiurl");

    let Screen::QuickAccess(access) = &app.screen else {
        panic!("expected Quick Access");
    };
    assert!(matches!(
        access.selected_target(),
        Some(QuickAccessTarget::Field { reference, kind })
            if reference.to_string() == "jig://Production/API_URL"
                && *kind == FieldKind::Text
    ));

    let backend = TestBackend::new(110, 30);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal.draw(|frame| render::draw(frame, &app)).unwrap();
    let rendered = terminal.backend().to_string();
    assert!(rendered.contains("Quick Access"), "{rendered}");
    assert!(rendered.contains("jig://Production/API_URL"), "{rendered}");
    assert!(
        rendered.contains("encrypted value is never loaded"),
        "{rendered}"
    );
    assert!(
        !rendered.contains(std::str::from_utf8(SENTINEL).unwrap()),
        "{rendered}"
    );
}

#[test]
fn quick_access_searches_item_kind_and_legacy_metadata() {
    let mut app = browsing_app();
    app.open_quick_access();
    handle_paste(&mut app, "staging");
    let Screen::QuickAccess(access) = &app.screen else {
        panic!("expected Quick Access");
    };
    assert!(matches!(
        access.selected_target(),
        Some(QuickAccessTarget::Item { item, .. }) if item == "Staging"
    ));

    handle_key(
        &mut app,
        KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL),
    );
    handle_paste(&mut app, "concealed");
    let Screen::QuickAccess(access) = &app.screen else {
        panic!("expected Quick Access");
    };
    assert!(matches!(
        access.selected_target(),
        Some(QuickAccessTarget::Field { kind, .. }) if *kind == FieldKind::Concealed
    ));

    handle_key(
        &mut app,
        KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL),
    );
    handle_paste(&mut app, "oldtoken");
    let Screen::QuickAccess(access) = &app.screen else {
        panic!("expected Quick Access");
    };
    assert!(matches!(
        access.selected_target(),
        Some(QuickAccessTarget::LegacyEntry { name }) if name == "old_token"
    ));
}

#[test]
fn quick_access_hands_the_exact_target_to_context_actions() {
    let mut app = browsing_app();
    app.open_quick_access();
    handle_paste(&mut app, "apiurl");
    assert!(matches!(
        handle_key(&mut app, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
        RuntimeAction::Redraw
    ));

    assert_eq!(
        app.selected_item,
        Some(ItemIdentity::Canonical("Production".to_owned()))
    );
    assert_eq!(
        app.selected_entry,
        Some(EntryIdentity::Field(
            "jig://Production/API_URL".parse().unwrap()
        ))
    );
    assert_eq!(app.focus, Focus::Fields);
    let Screen::Commands(palette) = &app.screen else {
        panic!("expected contextual actions");
    };
    assert_eq!(palette.scope, CommandPaletteScope::Context);
    assert!(
        palette
            .entries
            .iter()
            .any(|entry| entry.command == UiCommand::ReplaceValue)
    );
    assert!(
        palette
            .entries
            .iter()
            .any(|entry| entry.command == UiCommand::ChangeKind)
    );
}

#[test]
fn quick_access_rejects_query_overflow_and_recovers_from_no_matches() {
    let mut app = browsing_app();
    app.open_quick_access();
    handle_paste(&mut app, "api");
    handle_paste(&mut app, &"x".repeat(SEARCH_INPUT_LIMIT));
    let Screen::QuickAccess(access) = &app.screen else {
        panic!("expected Quick Access");
    };
    assert_eq!(access.query().as_str(), "api");
    assert!(
        app.status
            .as_ref()
            .is_some_and(|status| status.text.contains("interactive size limit"))
    );

    handle_key(
        &mut app,
        KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL),
    );
    handle_paste(&mut app, "no-such-metadata");
    handle_key(&mut app, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert!(matches!(app.screen, Screen::QuickAccess(_)));
    assert!(
        app.status
            .as_ref()
            .is_some_and(|status| status.text.contains("No metadata matches"))
    );

    handle_key(
        &mut app,
        KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE),
    );
    assert!(app.status.is_none());
}

#[test]
fn compact_quick_access_keeps_far_selection_visible_when_navigation_reverses() {
    let mut expanded = snapshot();
    let template = expanded.fields[0].clone();
    expanded.fields = (0..40)
        .map(|index| {
            let mut field = template.clone();
            field.reference = format!("jig://Bulk/FIELD_{index:02}").parse().unwrap();
            field
        })
        .collect();
    expanded.legacy_secrets.clear();
    let mut app = App::new(descriptor(true));
    app.apply_snapshot(expanded);
    app.open_quick_access();
    handle_key(&mut app, KeyEvent::new(KeyCode::End, KeyModifiers::CONTROL));

    let backend = TestBackend::new(64, 16);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal.draw(|frame| render::draw(frame, &app)).unwrap();
    let at_end = terminal.backend().to_string();
    assert!(at_end.contains("FIELD_39"), "{at_end}");

    handle_key(&mut app, KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
    terminal.draw(|frame| render::draw(frame, &app)).unwrap();
    let reversed = terminal.backend().to_string();
    assert!(reversed.contains("FIELD_38"), "{reversed}");
    assert!(reversed.contains("FIELD_39"), "{reversed}");
}

#[test]
fn active_metadata_editor_renders_a_cursor_and_horizontal_window() {
    let mut app = browsing_app();
    app.begin_create_item();
    handle_paste(&mut app, &format!("START-{}-END", "x".repeat(180)));
    let backend = TestBackend::new(100, 28);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal.draw(|frame| render::draw(frame, &app)).unwrap();
    let at_end = terminal.backend().to_string();
    assert!(at_end.contains('‹'), "{at_end}");
    assert!(at_end.contains("-END▌"), "{at_end}");

    handle_key(&mut app, KeyEvent::new(KeyCode::Home, KeyModifiers::NONE));
    terminal.draw(|frame| render::draw(frame, &app)).unwrap();
    let at_home = terminal.backend().to_string();
    assert!(at_home.contains("▌START-"), "{at_home}");
}

#[test]
fn protected_values_remain_append_only_and_outside_the_metadata_editor() {
    let mut app = browsing_app();
    app.focus = Focus::Fields;
    app.begin_replace();
    handle_paste(&mut app, "protected-value");
    assert!(matches!(
        handle_key(&mut app, KeyEvent::new(KeyCode::Left, KeyModifiers::NONE)),
        RuntimeAction::Ignore
    ));
    handle_key(
        &mut app,
        KeyEvent::new(KeyCode::Char('!'), KeyModifiers::NONE),
    );
    match submit_key(&mut app) {
        VaultAction::Mutate {
            mutation: VaultMutation::SetField { value, .. },
            ..
        } => assert_eq!(value.as_slice(), b"protected-value!"),
        other => panic!("unexpected action: {other:?}"),
    }
}

#[test]
fn typed_confirmations_can_be_corrected_at_the_cursor() {
    let mut app = browsing_app();
    app.focus = Focus::Items;
    app.begin_delete();
    handle_paste(&mut app, "DELEE");
    handle_key(&mut app, KeyEvent::new(KeyCode::Left, KeyModifiers::NONE));
    handle_key(
        &mut app,
        KeyEvent::new(KeyCode::Char('T'), KeyModifiers::SHIFT),
    );
    assert!(matches!(
        submit_key(&mut app),
        VaultAction::Mutate {
            mutation: VaultMutation::RemoveItem { .. },
            ..
        }
    ));
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
    assert!(rendered.contains("Esc quit"), "{rendered}");
    assert!(
        !rendered.contains("vault-tui-plaintext-sentinel"),
        "{rendered}"
    );
}

#[test]
fn locked_q_is_protected_input_and_escape_remains_quit() {
    let mut app = App::new(descriptor(true));

    assert!(matches!(
        handle_key(
            &mut app,
            KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE)
        ),
        RuntimeAction::Redraw
    ));
    let Screen::Locked(input) = &app.screen else {
        panic!("expected locked input");
    };
    assert_eq!(input.len(), 1);
    assert!(matches!(
        handle_key(&mut app, KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)),
        RuntimeAction::Quit
    ));
}

#[test]
fn loading_ignores_lock_shortcut_without_discarding_the_worker_result() {
    let mut app = browsing_app();
    app.begin_loading("Testing operation");

    assert!(matches!(
        handle_key(
            &mut app,
            KeyEvent::new(KeyCode::Char('L'), KeyModifiers::SHIFT)
        ),
        RuntimeAction::Ignore
    ));
    assert!(matches!(app.screen, Screen::Loading("Testing operation")));
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
fn very_large_browser_frame_remains_metadata_only() {
    let backend = TestBackend::new(608, 113);
    let mut terminal = Terminal::new(backend).unwrap();
    let app = browsing_app();
    terminal.draw(|frame| render::draw(frame, &app)).unwrap();
    let rendered = terminal.backend().to_string();

    assert!(rendered.contains("Items"), "{rendered}");
    assert!(rendered.contains("Fields"), "{rendered}");
    assert!(rendered.contains("Details"), "{rendered}");
    assert!(!rendered.contains(std::str::from_utf8(SENTINEL).unwrap()));
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

fn submit_key(app: &mut App) -> VaultAction {
    let RuntimeAction::Start(BackendRequest::Execute(action)) =
        handle_key(app, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
    else {
        panic!("form did not produce a backend action: {:?}", app.screen);
    };
    action
}

fn select_legacy(app: &mut App) {
    app.selected_item = Some(ItemIdentity::Legacy);
    app.selected_entry = Some(EntryIdentity::Legacy("old_token".to_owned()));
    app.focus = Focus::Fields;
}

#[test]
fn create_field_form_separates_metadata_kind_and_protected_value() {
    let mut app = browsing_app();
    app.begin_add();
    assert!(matches!(
        app.screen,
        Screen::Form(ManagementForm::WriteField { .. })
    ));

    handle_paste(&mut app, "NEW_FIELD");
    handle_key(&mut app, KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    handle_key(
        &mut app,
        KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE),
    );
    handle_key(&mut app, KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    handle_paste(&mut app, std::str::from_utf8(SENTINEL).unwrap());

    let debug = format!("{:?}", app.screen);
    assert!(
        !debug.contains(std::str::from_utf8(SENTINEL).unwrap()),
        "{debug}"
    );
    let backend = TestBackend::new(100, 28);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal.draw(|frame| render::draw(frame, &app)).unwrap();
    let rendered = terminal.backend().to_string();
    assert!(rendered.contains("bytes"), "{rendered}");
    assert!(
        !rendered.contains(std::str::from_utf8(SENTINEL).unwrap()),
        "{rendered}"
    );
    let action = submit_key(&mut app);
    let action_debug = format!("{action:?}");
    assert!(
        !action_debug.contains(std::str::from_utf8(SENTINEL).unwrap()),
        "{action_debug}"
    );
    match action {
        VaultAction::Mutate {
            mutation:
                VaultMutation::SetField {
                    reference,
                    kind,
                    mode,
                    value: _,
                },
            ..
        } => {
            assert_eq!(reference.to_string(), "jig://Production/NEW_FIELD");
            assert_eq!(kind, FieldKind::Text);
            assert_eq!(mode, jig_vault::VaultWriteMode::Create);
        }
        other => panic!("unexpected action: {other:?}"),
    }
}

#[test]
fn create_item_shortcut_writes_first_field_and_selects_its_exact_identity() {
    let temp = tempdir().unwrap();
    let vault = Vault::resolve_for_test(Some(temp.path().join("create-item-vault"))).unwrap();
    let passphrase = SecretString::from("correct horse battery staple".to_owned());
    vault.init(&passphrase).unwrap();
    let mut app = App::new(descriptor(true));
    app.apply_snapshot(vault.snapshot(&passphrase).unwrap());

    assert!(matches!(
        handle_key(
            &mut app,
            KeyEvent::new(KeyCode::Char('I'), KeyModifiers::SHIFT)
        ),
        RuntimeAction::Redraw
    ));
    let Screen::Form(ManagementForm::WriteField {
        intent,
        item,
        field,
        focus,
        ..
    }) = &app.screen
    else {
        panic!("expected create-item field form");
    };
    assert_eq!(*intent, FieldWriteIntent::CreateItem);
    assert!(item.is_empty());
    assert!(field.is_empty());
    assert_eq!(*focus, FieldWriteFocus::Item);
    let backend = TestBackend::new(100, 28);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal.draw(|frame| render::draw(frame, &app)).unwrap();
    let rendered = terminal.backend().to_string();
    assert!(rendered.contains("Create item + first field"), "{rendered}");
    assert!(
        rendered.contains("created atomically with their first field"),
        "{rendered}"
    );

    handle_paste(&mut app, "Fresh");
    handle_key(&mut app, KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    handle_paste(&mut app, "API_KEY");
    handle_key(&mut app, KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    handle_key(&mut app, KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    handle_paste(&mut app, std::str::from_utf8(SENTINEL).unwrap());

    let action = submit_key(&mut app);
    let VaultAction::Mutate {
        mutation:
            VaultMutation::SetField {
                reference,
                kind,
                value,
                mode,
            },
        ..
    } = action
    else {
        panic!("unexpected action: {action:?}");
    };
    assert_eq!(reference.to_string(), "jig://Fresh/API_KEY");
    assert_eq!(mode, jig_vault::VaultWriteMode::Create);
    vault
        .write_field(&passphrase, reference.clone(), kind, value, mode)
        .unwrap();

    app.apply_snapshot(vault.snapshot(&passphrase).unwrap());
    assert_eq!(
        app.selected_item,
        Some(ItemIdentity::Canonical("Fresh".to_owned()))
    );
    assert_eq!(app.selected_entry, Some(EntryIdentity::Field(reference)));
}

#[test]
fn empty_vault_explains_item_creation_and_context_actions_offer_it() {
    let mut app = App::new(descriptor(true));
    app.apply_snapshot(empty_snapshot());
    let backend = TestBackend::new(110, 28);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal.draw(|frame| render::draw(frame, &app)).unwrap();
    let rendered = terminal.backend().to_string();
    assert!(rendered.contains("No items yet"), "{rendered}");
    assert!(rendered.contains("Press I to create an item"), "{rendered}");
    assert!(rendered.contains("with its first field"), "{rendered}");

    handle_key(&mut app, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    let Screen::Commands(palette) = &app.screen else {
        panic!("expected empty-vault context actions");
    };
    assert!(palette.entries.iter().any(|entry| {
        entry.command == UiCommand::CreateItem && entry.availability == CommandAvailability::Enabled
    }));
    assert!(palette.entries.iter().any(|entry| {
        entry.command == UiCommand::AddField
            && matches!(entry.availability, CommandAvailability::Disabled(_))
    }));
}

#[test]
fn add_field_never_changes_meaning_when_legacy_is_selected() {
    let mut app = browsing_app();
    select_legacy(&mut app);
    handle_key(
        &mut app,
        KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE),
    );
    assert!(matches!(app.screen, Screen::Browse));
    assert!(
        app.status
            .as_ref()
            .is_some_and(|status| status.text.contains("create a new item"))
    );
}

#[test]
fn create_item_rejects_an_existing_item_before_consuming_the_value() {
    let mut app = browsing_app();
    app.begin_create_item();
    handle_paste(&mut app, "Production");
    handle_key(&mut app, KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    handle_paste(&mut app, "ANOTHER_FIELD");
    handle_key(&mut app, KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    handle_key(&mut app, KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    handle_paste(&mut app, std::str::from_utf8(SENTINEL).unwrap());

    assert!(matches!(
        handle_key(&mut app, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
        RuntimeAction::Redraw
    ));
    let Screen::Form(ManagementForm::WriteField { value, .. }) = &app.screen else {
        panic!("expected correctable create-item form");
    };
    assert!(!value.is_empty());
    assert!(
        app.status
            .as_ref()
            .is_some_and(|status| status.text.contains("already exists"))
    );
}

#[test]
fn create_field_form_can_load_exact_bytes_from_a_regular_file() {
    let temp = tempdir().unwrap();
    let source = temp.path().join("secret.bin");
    let exact = b"binary\0field-value";
    std::fs::write(&source, exact).unwrap();
    let mut app = browsing_app();
    app.begin_add();
    handle_paste(&mut app, "BINARY_FIELD");
    handle_key(&mut app, KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    handle_key(&mut app, KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    handle_key(&mut app, KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    handle_paste(&mut app, &source.to_string_lossy());

    match submit_key(&mut app) {
        VaultAction::Mutate {
            mutation: VaultMutation::SetField { value, .. },
            ..
        } => {
            assert_eq!(value.as_slice(), exact)
        }
        other => panic!("unexpected action: {other:?}"),
    }
}

#[test]
fn invalid_concealed_value_file_can_be_corrected_and_retried() {
    let temp = tempdir().unwrap();
    let source = temp.path().join("secret.bin");
    std::fs::write(&source, b"bad").unwrap();
    let mut app = browsing_app();
    app.begin_add();
    handle_paste(&mut app, "RETRY_FIELD");
    handle_key(&mut app, KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    handle_key(&mut app, KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    handle_key(&mut app, KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    handle_paste(&mut app, &source.to_string_lossy());

    assert!(matches!(
        handle_key(&mut app, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
        RuntimeAction::Redraw
    ));
    let Screen::Form(ManagementForm::WriteField {
        value, value_file, ..
    }) = &app.screen
    else {
        panic!("expected field form after validation failure");
    };
    assert!(value.is_empty());
    assert_eq!(value_file.as_str(), source.to_string_lossy());
    assert_eq!(
        app.status.as_ref().unwrap().text,
        "Concealed values must contain at least 4 bytes."
    );

    let corrected = b"corrected-binary\0value";
    std::fs::write(&source, corrected).unwrap();
    match submit_key(&mut app) {
        VaultAction::Mutate {
            mutation: VaultMutation::SetField { value, .. },
            ..
        } => {
            assert_eq!(value.as_slice(), corrected)
        }
        other => panic!("unexpected action: {other:?}"),
    }
}

#[test]
fn replacement_form_starts_empty_and_does_not_render_existing_value() {
    let mut app = browsing_app();
    app.focus = Focus::Fields;
    app.begin_replace();
    let Screen::Form(ManagementForm::WriteField { value, intent, .. }) = &app.screen else {
        panic!("expected field replacement form");
    };
    assert!(value.is_empty());
    assert_eq!(*intent, FieldWriteIntent::ReplaceValue);

    let backend = TestBackend::new(100, 28);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal.draw(|frame| render::draw(frame, &app)).unwrap();
    let rendered = terminal.backend().to_string();
    assert!(
        rendered.contains("current value was not loaded"),
        "{rendered}"
    );
    assert!(
        !rendered.contains(std::str::from_utf8(SENTINEL).unwrap()),
        "{rendered}"
    );
}

#[test]
fn empty_text_replacement_requires_exact_clear_confirmation() {
    let mut app = browsing_app();
    let reference = "jig://Production/API_URL".parse().unwrap();
    app.selected_item = Some(ItemIdentity::Canonical("Production".to_owned()));
    app.selected_entry = Some(EntryIdentity::Field(reference));
    app.focus = Focus::Fields;
    app.begin_replace();

    assert!(matches!(
        handle_key(&mut app, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
        RuntimeAction::Redraw
    ));
    let Screen::ConfirmMutation(confirmation) = &app.screen else {
        panic!("expected empty text replacement confirmation");
    };
    let MutationConfirmationKind::EmptyTextReplacement { reference, .. } = &confirmation.kind
    else {
        panic!("expected empty text replacement confirmation");
    };
    assert_eq!(reference.to_string(), "jig://Production/API_URL");

    let backend = TestBackend::new(100, 28);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal.draw(|frame| render::draw(frame, &app)).unwrap();
    let rendered = terminal.backend().to_string();
    assert!(
        rendered.contains("Confirm empty text replacement"),
        "{rendered}"
    );
    assert!(rendered.contains("Type CLEAR exactly"), "{rendered}");
    assert!(!rendered.contains("https://example.invalid"), "{rendered}");

    handle_paste(&mut app, "clear");
    assert!(matches!(
        handle_key(&mut app, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
        RuntimeAction::Redraw
    ));
    assert!(matches!(&app.screen, Screen::ConfirmMutation(_)));
    assert!(
        app.status
            .as_ref()
            .is_some_and(|status| status.text.contains("CLEAR exactly"))
    );

    handle_key(
        &mut app,
        KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL),
    );
    handle_paste(&mut app, "CLEAR");
    let RuntimeAction::Start(BackendRequest::Execute(VaultAction::Mutate {
        mutation:
            VaultMutation::SetField {
                reference,
                kind,
                value,
                mode,
            },
        ..
    })) = handle_key(&mut app, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
    else {
        panic!("exact confirmation did not produce an empty replacement action");
    };
    assert_eq!(reference.to_string(), "jig://Production/API_URL");
    assert_eq!(kind, FieldKind::Text);
    assert!(value.is_empty());
    assert_eq!(mode, jig_vault::VaultWriteMode::Replace);
}

#[test]
fn non_empty_text_replacement_does_not_require_clear_confirmation() {
    let mut app = browsing_app();
    let reference = "jig://Production/API_URL".parse().unwrap();
    app.selected_item = Some(ItemIdentity::Canonical("Production".to_owned()));
    app.selected_entry = Some(EntryIdentity::Field(reference));
    app.focus = Focus::Fields;
    app.begin_replace();
    handle_paste(&mut app, "replacement value");

    match submit_key(&mut app) {
        VaultAction::Mutate {
            mutation: VaultMutation::SetField { value, mode, .. },
            ..
        } => {
            assert_eq!(value.as_slice(), b"replacement value");
            assert_eq!(mode, jig_vault::VaultWriteMode::Replace);
        }
        other => panic!("unexpected action: {other:?}"),
    }
}

#[test]
fn concealed_to_text_requires_exact_warning_confirmation_but_upgrade_does_not() {
    let mut app = browsing_app();
    app.focus = Focus::Fields;
    app.begin_change_kind();

    assert!(matches!(
        handle_key(&mut app, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
        RuntimeAction::Redraw
    ));
    let Screen::ConfirmMutation(confirmation) = &app.screen else {
        panic!("expected redaction downgrade confirmation");
    };
    let MutationConfirmationKind::RedactionDowngrade { reference } = &confirmation.kind else {
        panic!("expected concealed-to-text confirmation");
    };
    assert_eq!(reference.to_string(), "jig://Production/API_TOKEN");

    let backend = TestBackend::new(100, 28);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal.draw(|frame| render::draw(frame, &app)).unwrap();
    let rendered = terminal.backend().to_string();
    assert!(
        rendered.contains("Confirm redaction downgrade"),
        "{rendered}"
    );
    assert!(
        rendered.contains("not output-redaction needles"),
        "{rendered}"
    );
    assert!(rendered.contains("Type TEXT exactly"), "{rendered}");

    handle_paste(&mut app, "text");
    assert!(matches!(
        handle_key(&mut app, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
        RuntimeAction::Redraw
    ));
    assert!(matches!(app.screen, Screen::ConfirmMutation(_)));
    assert!(
        app.status
            .as_ref()
            .is_some_and(|status| status.text.contains("TEXT exactly"))
    );

    handle_key(
        &mut app,
        KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL),
    );
    handle_paste(&mut app, "TEXT");
    match submit_key(&mut app) {
        VaultAction::Mutate {
            mutation: VaultMutation::ChangeFieldKind { reference, kind },
            ..
        } => {
            assert_eq!(reference.to_string(), "jig://Production/API_TOKEN");
            assert_eq!(kind, FieldKind::Text);
        }
        other => panic!("unexpected action: {other:?}"),
    }

    app.apply_snapshot(snapshot());
    let text_reference = "jig://Production/API_URL".parse().unwrap();
    app.selected_item = Some(ItemIdentity::Canonical("Production".to_owned()));
    app.selected_entry = Some(EntryIdentity::Field(text_reference));
    app.focus = Focus::Fields;
    app.begin_change_kind();
    match submit_key(&mut app) {
        VaultAction::Mutate {
            mutation: VaultMutation::ChangeFieldKind { kind, .. },
            ..
        } => assert_eq!(kind, FieldKind::Concealed),
        other => panic!("unexpected action: {other:?}"),
    }
}

#[test]
fn replacement_and_legacy_conversion_share_the_redaction_downgrade_policy() {
    let mut app = browsing_app();
    app.focus = Focus::Fields;
    app.begin_replace();
    let Screen::Form(ManagementForm::WriteField { kind, .. }) = &mut app.screen else {
        panic!("expected field replacement form");
    };
    *kind = FieldKind::Text;
    handle_paste(&mut app, "replacement text");
    assert!(matches!(
        handle_key(&mut app, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
        RuntimeAction::Redraw
    ));
    assert!(matches!(
        app.screen,
        Screen::ConfirmMutation(MutationConfirmation {
            kind: MutationConfirmationKind::RedactionDowngrade { .. },
            ..
        })
    ));

    app.apply_snapshot(snapshot());
    select_legacy(&mut app);
    app.begin_convert();
    let Screen::Form(ManagementForm::ConvertLegacy {
        item, field, kind, ..
    }) = &mut app.screen
    else {
        panic!("expected legacy conversion form");
    };
    item.clear();
    field.clear();
    assert!(item.insert("Imported"));
    assert!(field.insert("TOKEN"));
    *kind = FieldKind::Text;
    assert!(matches!(
        handle_key(&mut app, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
        RuntimeAction::Redraw
    ));
    assert!(matches!(
        app.screen,
        Screen::ConfirmMutation(MutationConfirmation {
            kind: MutationConfirmationKind::RedactionDowngrade { .. },
            ..
        })
    ));
}

#[test]
fn field_and_item_rename_forms_emit_typed_actions() {
    let mut app = browsing_app();
    app.apply_snapshot(snapshot());
    app.focus = Focus::Fields;
    app.begin_rename();
    handle_paste(&mut app, "TOKEN_RENAMED");
    match submit_key(&mut app) {
        VaultAction::Mutate {
            mutation:
                VaultMutation::RenameField {
                    source,
                    destination,
                },
            ..
        } => {
            assert_eq!(source.to_string(), "jig://Production/API_TOKEN");
            assert_eq!(destination.to_string(), "jig://Production/TOKEN_RENAMED");
        }
        other => panic!("unexpected action: {other:?}"),
    }

    app.apply_snapshot(snapshot());
    app.focus = Focus::Items;
    app.begin_rename();
    handle_paste(&mut app, "ProdRenamed");
    match submit_key(&mut app) {
        VaultAction::Mutate {
            mutation:
                VaultMutation::RenameItem {
                    source,
                    destination,
                },
            ..
        } => {
            assert_eq!(source.as_str(), "Production");
            assert_eq!(destination.as_str(), "ProdRenamed");
        }
        other => panic!("unexpected action: {other:?}"),
    }
}

#[test]
fn legacy_create_replace_convert_and_delete_are_explicit() {
    let mut app = browsing_app();
    app.begin_add_legacy();
    handle_paste(&mut app, "another_old_token");
    handle_key(&mut app, KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    handle_paste(&mut app, "new-legacy-value");
    match submit_key(&mut app) {
        VaultAction::Mutate {
            mutation: VaultMutation::SetLegacy { name, mode, .. },
            ..
        } => {
            assert_eq!(name, "another_old_token");
            assert_eq!(mode, jig_vault::VaultWriteMode::Create);
        }
        other => panic!("unexpected action: {other:?}"),
    }

    app.apply_snapshot(snapshot());
    select_legacy(&mut app);
    app.begin_replace();
    handle_paste(&mut app, "replacement-legacy-value");
    match submit_key(&mut app) {
        VaultAction::Mutate {
            mutation: VaultMutation::SetLegacy { name, mode, .. },
            ..
        } => {
            assert_eq!(name, "old_token");
            assert_eq!(mode, jig_vault::VaultWriteMode::Replace);
        }
        other => panic!("unexpected action: {other:?}"),
    }

    app.apply_snapshot(snapshot());
    select_legacy(&mut app);
    app.begin_convert();
    handle_paste(&mut app, "Imported");
    handle_key(&mut app, KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    handle_paste(&mut app, "TOKEN");
    match submit_key(&mut app) {
        VaultAction::Mutate {
            mutation:
                VaultMutation::ConvertLegacy {
                    name,
                    reference,
                    kind,
                },
            ..
        } => {
            assert_eq!(name, "old_token");
            assert_eq!(reference.to_string(), "jig://Imported/TOKEN");
            assert_eq!(kind, FieldKind::Concealed);
        }
        other => panic!("unexpected action: {other:?}"),
    }

    app.apply_snapshot(snapshot());
    select_legacy(&mut app);
    app.begin_delete();
    let Screen::ConfirmDelete(confirmation) = &app.screen else {
        panic!("expected delete confirmation");
    };
    assert!(matches!(confirmation.target, DeleteTarget::Legacy(_)));
    handle_paste(&mut app, "old_token");
    match submit_key(&mut app) {
        VaultAction::Mutate {
            mutation: VaultMutation::RemoveLegacy { name },
            ..
        } => {
            assert_eq!(name, "old_token")
        }
        other => panic!("unexpected action: {other:?}"),
    }
}

#[test]
fn change_kind_form_rejects_noop_selection() {
    let mut app = browsing_app();
    app.focus = Focus::Fields;
    app.begin_change_kind();
    handle_key(
        &mut app,
        KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE),
    );

    assert!(matches!(
        handle_key(&mut app, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
        RuntimeAction::Redraw
    ));
    assert!(matches!(
        app.screen,
        Screen::Form(ManagementForm::ChangeKind { from, to, .. }) if from == to
    ));
    assert!(
        app.status
            .as_ref()
            .is_some_and(|status| status.text.contains("different field kind"))
    );
}

#[test]
fn field_and_item_deletion_require_exact_typed_confirmation() {
    let mut app = browsing_app();
    app.focus = Focus::Fields;
    app.begin_delete();
    handle_paste(&mut app, "wrong");
    assert!(matches!(
        handle_key(&mut app, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
        RuntimeAction::Redraw
    ));
    assert!(matches!(app.screen, Screen::ConfirmDelete(_)));
    app.clear_metadata_input();
    handle_paste(&mut app, "jig://Production/API_TOKEN");
    match submit_key(&mut app) {
        VaultAction::Mutate {
            mutation: VaultMutation::RemoveField { reference },
            ..
        } => {
            assert_eq!(reference.to_string(), "jig://Production/API_TOKEN");
        }
        other => panic!("unexpected action: {other:?}"),
    }

    app.apply_snapshot(snapshot());
    app.focus = Focus::Items;
    app.begin_delete();
    let Screen::ConfirmDelete(confirmation) = &app.screen else {
        panic!("expected item delete confirmation");
    };
    assert!(confirmation.target.display_label().contains("2 fields"));
    handle_paste(&mut app, "DELETE");
    match submit_key(&mut app) {
        VaultAction::Mutate {
            mutation: VaultMutation::RemoveItem { item },
            ..
        } => {
            assert_eq!(item.as_str(), "Production")
        }
        other => panic!("unexpected action: {other:?}"),
    }
}

#[test]
fn legacy_delete_confirmation_sanitizes_unsafe_identity_text() {
    let unsafe_name = "old\u{1b}[31m\u{202e}token".to_owned();
    let mut unsafe_snapshot = snapshot();
    unsafe_snapshot.legacy_secrets[0].name = unsafe_name.clone();
    let mut app = App::new(descriptor(true));
    app.apply_snapshot(unsafe_snapshot);
    app.selected_item = Some(ItemIdentity::Legacy);
    app.selected_entry = Some(EntryIdentity::Legacy(unsafe_name.clone()));
    app.focus = Focus::Fields;
    app.begin_delete();

    let backend = TestBackend::new(100, 28);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal.draw(|frame| render::draw(frame, &app)).unwrap();
    let rendered = terminal.backend().to_string();

    assert!(!rendered.contains('\u{1b}'), "{rendered}");
    assert!(!rendered.contains('\u{202e}'), "{rendered}");
    assert!(
        rendered.contains("legacy entry old�[31m�token"),
        "{rendered}"
    );
    assert!(rendered.contains("Type exactly: DELETE"), "{rendered}");

    handle_paste(&mut app, "DELETE");
    match submit_key(&mut app) {
        VaultAction::Mutate {
            mutation: VaultMutation::RemoveLegacy { name },
            ..
        } => assert_eq!(name, unsafe_name),
        other => panic!("unexpected action: {other:?}"),
    }
}

#[test]
fn universal_palette_filters_and_opens_verified_activity() {
    let mut app = browsing_app();
    handle_key(
        &mut app,
        KeyEvent::new(KeyCode::Char(':'), KeyModifiers::NONE),
    );
    assert!(matches!(
        app.screen,
        Screen::Commands(ref palette) if palette.scope == CommandPaletteScope::Universal
    ));
    handle_paste(&mut app, "activity");
    match submit_key(&mut app) {
        VaultAction::Activity { limit } => assert_eq!(limit, 100),
        other => panic!("unexpected action: {other:?}"),
    }

    let temp = tempdir().unwrap();
    let vault = Vault::resolve_for_test(Some(temp.path().join("activity-vault"))).unwrap();
    let passphrase = SecretString::from("correct horse battery staple".to_owned());
    vault.init(&passphrase).unwrap();
    vault
        .set_field(
            &passphrase,
            "jig://Production/API_TOKEN".parse().unwrap(),
            FieldKind::Concealed,
            SecretBytes::new(b"activity-secret-sentinel".to_vec()),
        )
        .unwrap();
    let mut activity = vault.activity(&passphrase, 10).unwrap();
    activity.audit.torn_tail_bytes = 17;
    app.apply_activity(activity);
    let backend = TestBackend::new(100, 28);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal.draw(|frame| render::draw(frame, &app)).unwrap();
    let rendered = terminal.backend().to_string();
    assert!(rendered.contains("Verified activity"), "{rendered}");
    assert!(rendered.contains("field_batch_apply"), "{rendered}");
    assert!(rendered.contains("verified prefix"), "{rendered}");
    assert!(rendered.contains("17 unauthenticated"), "{rendered}");
    assert!(
        !rendered.contains(std::str::from_utf8(SENTINEL).unwrap()),
        "{rendered}"
    );

    let mut verification = snapshot().audit;
    verification.latest_mac = Some(std::str::from_utf8(SENTINEL).unwrap().to_owned());
    let expected_event_count = verification.event_count.to_string();
    app.apply_audit_result(verification);
    terminal.draw(|frame| render::draw(frame, &app)).unwrap();
    let rendered = terminal.backend().to_string();
    assert!(rendered.contains("Authenticated events"), "{rendered}");
    assert!(rendered.contains(&expected_event_count), "{rendered}");
    assert!(
        !rendered.contains(std::str::from_utf8(SENTINEL).unwrap()),
        "{rendered}"
    );
}

#[test]
fn enter_opens_context_actions_and_disabled_reasons_remain_visible() {
    let mut app = browsing_app();
    app.focus = Focus::Fields;
    assert!(matches!(
        handle_key(&mut app, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
        RuntimeAction::Redraw
    ));
    let Screen::Commands(palette) = &app.screen else {
        panic!("expected contextual command palette");
    };
    assert_eq!(palette.scope, CommandPaletteScope::Context);
    assert!(
        palette
            .entries
            .iter()
            .any(|entry| entry.command == UiCommand::ReplaceValue)
    );
    assert!(
        palette
            .entries
            .iter()
            .all(|entry| entry.command.relevant_to_context(&app))
    );

    app.close_overlay();
    select_legacy(&mut app);
    app.open_command_palette(CommandPaletteScope::Context);
    let Screen::Commands(palette) = &app.screen else {
        panic!("expected contextual command palette");
    };
    let export_index = palette
        .entries
        .iter()
        .position(|entry| entry.command == UiCommand::ExportField)
        .expect("legacy context should explain why export is unavailable");
    assert!(matches!(
        palette.entries[export_index].availability,
        CommandAvailability::Disabled(_)
    ));
    app.move_command_selection(export_index as isize);
    assert!(matches!(
        handle_key(&mut app, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
        RuntimeAction::Redraw
    ));
    assert!(matches!(app.screen, Screen::Commands(_)));
    assert!(
        app.status
            .as_ref()
            .is_some_and(|status| status.text.contains("canonical field"))
    );

    let backend = TestBackend::new(110, 30);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal.draw(|frame| render::draw(frame, &app)).unwrap();
    let rendered = terminal.backend().to_string();
    assert!(rendered.contains("canonical field"), "{rendered}");
}

#[test]
fn direct_action_bindings_require_exact_modifiers() {
    assert_eq!(
        UiCommand::from_key(KeyEvent::new(KeyCode::Char('e'), KeyModifiers::CONTROL)),
        None
    );
    assert_eq!(
        UiCommand::from_key(KeyEvent::new(KeyCode::Char('e'), KeyModifiers::ALT)),
        None
    );

    let mut app = browsing_app();
    assert!(matches!(
        handle_key(
            &mut app,
            KeyEvent::new(KeyCode::Char('e'), KeyModifiers::ALT)
        ),
        RuntimeAction::Ignore
    ));
    assert!(matches!(app.screen, Screen::Browse));
    assert!(matches!(
        handle_key(
            &mut app,
            KeyEvent::new(KeyCode::Char('e'), KeyModifiers::NONE)
        ),
        RuntimeAction::Redraw
    ));
    assert!(matches!(app.screen, Screen::Form(_)));
}

#[test]
fn help_and_footer_render_catalog_backed_action_discovery() {
    let mut app = browsing_app();
    app.focus = Focus::Fields;
    let backend = TestBackend::new(140, 40);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal.draw(|frame| render::draw(frame, &app)).unwrap();
    let footer = terminal.backend().to_string();
    assert!(footer.contains("e replace"), "{footer}");
    assert!(footer.contains("Enter actions"), "{footer}");
    assert!(footer.contains(": all"), "{footer}");

    app.show_help();
    terminal.draw(|frame| render::draw(frame, &app)).unwrap();
    let help = terminal.backend().to_string();
    for command in UiCommand::ALL
        .into_iter()
        .filter(|command| command.visible_in_state(&app))
        .filter(|command| command.binding().is_some())
    {
        assert!(
            help.contains(command.label()),
            "missing {command:?}: {help}"
        );
    }
}

#[test]
fn browser_footer_keeps_feedback_visible_while_filtering() {
    let mut app = browsing_app();
    app.append_filter("api");
    app.set_error("Export failed without changing the vault.");

    let backend = TestBackend::new(120, 32);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal.draw(|frame| render::draw(frame, &app)).unwrap();
    let failed = terminal.backend().to_string();
    assert!(failed.contains("/api"), "{failed}");
    assert!(
        failed.contains("Export failed without changing the vault."),
        "{failed}"
    );

    app.set_info("Vault metadata refreshed.");
    terminal.draw(|frame| render::draw(frame, &app)).unwrap();
    let succeeded = terminal.backend().to_string();
    assert!(succeeded.contains("/api"), "{succeeded}");
    assert!(
        succeeded.contains("Vault metadata refreshed."),
        "{succeeded}"
    );

    app.searching = true;
    app.set_error("Search filter exceeds the interactive size limit.");
    terminal.draw(|frame| render::draw(frame, &app)).unwrap();
    let searching = terminal.backend().to_string();
    assert!(searching.contains("/api"), "{searching}");
    assert!(
        searching.contains("Search filter exceeds the interactive size limit."),
        "{searching}"
    );
}

#[test]
fn platform_capabilities_gate_private_output_without_disabling_portable_actions() {
    let mut app = browsing_app();
    app.focus = Focus::Fields;
    app.selected_entry = Some(EntryIdentity::Field(
        "jig://Production/API_TOKEN".parse().unwrap(),
    ));

    for command in [
        UiCommand::ExportField,
        UiCommand::ImportOnePassword,
        UiCommand::CreateBackup,
    ] {
        assert_eq!(
            command.availability_with_capabilities(&app, PlatformCapabilities::PORTABLE_ONLY),
            CommandAvailability::Disabled(
                "Private file output is currently supported only on Unix."
            ),
            "{command:?}"
        );
        assert_eq!(command.availability(&app).is_enabled(), cfg!(unix));
    }

    for command in [UiCommand::PeekField, UiCommand::ChangePassphrase] {
        assert_eq!(
            command.availability_with_capabilities(&app, PlatformCapabilities::PORTABLE_ONLY),
            CommandAvailability::Enabled,
            "{command:?}"
        );
    }

    app.selected_entry = None;
    assert_eq!(
        UiCommand::ExportField
            .availability_with_capabilities(&app, PlatformCapabilities::PORTABLE_ONLY),
        CommandAvailability::Disabled("Select a canonical field first.")
    );

    let absent = App::new(descriptor(false));
    assert_eq!(
        UiCommand::RestoreBackup
            .availability_with_capabilities(&absent, PlatformCapabilities::PORTABLE_ONLY),
        CommandAvailability::Disabled("Restore is currently supported only on Linux.")
    );
}

#[cfg(unix)]
#[test]
fn onepassword_form_previews_metadata_before_exact_commit_confirmation() {
    let mut app = browsing_app();
    handle_key(
        &mut app,
        KeyEvent::new(KeyCode::Char(':'), KeyModifiers::NONE),
    );
    handle_paste(&mut app, "1password");
    handle_key(&mut app, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert!(matches!(app.screen, Screen::ToolForm(_)));

    handle_paste(&mut app, "/tmp/source.env");
    handle_key(&mut app, KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    handle_paste(&mut app, "Production");
    handle_key(&mut app, KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    handle_paste(&mut app, "/tmp/generated.env");
    match submit_key(&mut app) {
        VaultAction::PreviewOnePasswordImport {
            env_file,
            item,
            out_env,
            dry_run,
            ..
        } => {
            assert_eq!(env_file, PathBuf::from("/tmp/source.env"));
            assert_eq!(item.as_str(), "Production");
            assert_eq!(out_env, PathBuf::from("/tmp/generated.env"));
            assert!(!dry_run);
        }
        other => panic!("unexpected action: {other:?}"),
    }

    app.apply_import_preview(ImportPreview {
        env_file: PathBuf::from("/tmp/source.env"),
        item: "jig://Production".parse().unwrap(),
        out_env: PathBuf::from("/tmp/generated.env"),
        replace: false,
        overwrite: false,
        authorization: ImportPreviewAuthorization::Commit(ImportPlanToken::generate()),
        rows: vec![ImportPreviewRow {
            variable: "TOKEN".to_owned(),
            reference: "jig://Production/TOKEN".parse().unwrap(),
            change: ImportFieldChange::Replace {
                previous_kind: FieldKind::Concealed,
                kind: FieldKind::Concealed,
            },
        }],
        destination_exists: true,
    });
    handle_paste(&mut app, "IMPORT");
    assert!(matches!(
        handle_key(&mut app, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
        RuntimeAction::Redraw
    ));
    assert!(matches!(
        handle_key(
            &mut app,
            KeyEvent::new(KeyCode::Char('r'), KeyModifiers::CONTROL),
        ),
        RuntimeAction::Ignore
    ));
    assert!(matches!(
        handle_key(
            &mut app,
            KeyEvent::new(KeyCode::Char('o'), KeyModifiers::ALT),
        ),
        RuntimeAction::Ignore
    ));
    let Screen::ImportPreview(preview) = &app.screen else {
        panic!("expected the import preview to remain open");
    };
    assert!(!preview.preview.replace);
    assert!(!preview.preview.overwrite);
    handle_key(
        &mut app,
        KeyEvent::new(KeyCode::Char('r'), KeyModifiers::NONE),
    );
    handle_key(
        &mut app,
        KeyEvent::new(KeyCode::Char('o'), KeyModifiers::NONE),
    );
    match submit_key(&mut app) {
        VaultAction::CommitOnePasswordImport {
            replace, overwrite, ..
        } => {
            assert!(replace);
            assert!(overwrite);
        }
        other => panic!("unexpected action: {other:?}"),
    }
}

#[test]
fn import_redaction_downgrade_requires_specific_confirmation() {
    let mut app = browsing_app();
    app.apply_import_preview(ImportPreview {
        env_file: PathBuf::from("/tmp/source.env"),
        item: "jig://Production".parse().unwrap(),
        out_env: PathBuf::from("/tmp/generated.env"),
        replace: true,
        overwrite: false,
        authorization: ImportPreviewAuthorization::Commit(ImportPlanToken::generate()),
        rows: vec![
            ImportPreviewRow {
                variable: "TOKEN".to_owned(),
                reference: "jig://Production/TOKEN".parse().unwrap(),
                change: ImportFieldChange::Replace {
                    previous_kind: FieldKind::Concealed,
                    kind: FieldKind::Text,
                },
            },
            ImportPreviewRow {
                variable: "MODE".to_owned(),
                reference: "jig://Production/MODE".parse().unwrap(),
                change: ImportFieldChange::Create {
                    kind: FieldKind::Text,
                },
            },
        ],
        destination_exists: false,
    });

    let backend = TestBackend::new(110, 32);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal.draw(|frame| render::draw(frame, &app)).unwrap();
    let rendered = terminal.backend().to_string();
    assert!(rendered.contains("concealed → text"), "{rendered}");
    assert!(rendered.contains("redaction needles"), "{rendered}");
    assert!(rendered.contains("Type IMPORT TEXT"), "{rendered}");

    handle_paste(&mut app, "IMPORT");
    assert!(matches!(
        handle_key(&mut app, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
        RuntimeAction::Redraw
    ));
    assert!(matches!(app.screen, Screen::ImportPreview(_)));
    assert!(
        app.status
            .as_ref()
            .is_some_and(|status| status.text.contains("IMPORT TEXT exactly"))
    );

    handle_key(
        &mut app,
        KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL),
    );
    handle_paste(&mut app, "IMPORT TEXT");
    assert!(matches!(
        submit_key(&mut app),
        VaultAction::CommitOnePasswordImport {
            replace: true,
            overwrite: false,
            ..
        }
    ));
}

#[test]
fn import_preview_escape_emits_a_token_scoped_discard_action() {
    let mut app = browsing_app();
    let plan = ImportPlanToken::generate();
    app.apply_import_preview(ImportPreview {
        env_file: PathBuf::from("/tmp/source.env"),
        item: "jig://Production".parse().unwrap(),
        out_env: PathBuf::from("/tmp/generated.env"),
        replace: false,
        overwrite: false,
        authorization: ImportPreviewAuthorization::Commit(plan.clone()),
        rows: Vec::new(),
        destination_exists: false,
    });

    let RuntimeAction::Start(BackendRequest::Execute(VaultAction::DiscardOnePasswordImport {
        plan: discarded,
    })) = handle_key(&mut app, KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE))
    else {
        panic!("commit-capable preview did not produce a discard action");
    };
    assert_eq!(discarded, plan);
    assert!(matches!(app.screen, Screen::Loading(_)));

    app.finish_import_discard();
    assert!(matches!(app.screen, Screen::Browse));
    assert!(
        app.status
            .as_ref()
            .is_some_and(|status| status.text.contains("preview discarded"))
    );

    app.apply_import_preview(ImportPreview {
        env_file: PathBuf::from("/tmp/source.env"),
        item: "jig://Production".parse().unwrap(),
        out_env: PathBuf::from("/tmp/generated.env"),
        replace: false,
        overwrite: false,
        authorization: ImportPreviewAuthorization::DryRun,
        rows: Vec::new(),
        destination_exists: false,
    });
    assert!(matches!(
        handle_key(&mut app, KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)),
        RuntimeAction::Redraw
    ));
    assert!(matches!(app.screen, Screen::Browse));
}

#[cfg(unix)]
#[test]
fn backup_form_emits_metadata_only_action() {
    let mut app = browsing_app();
    assert!(matches!(
        app.activate_direct_command(UiCommand::CreateBackup),
        CommandOutcome::Redraw
    ));
    handle_paste(&mut app, "/tmp/vault-backup.jig");
    handle_key(&mut app, KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    handle_key(
        &mut app,
        KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE),
    );
    match submit_key(&mut app) {
        VaultAction::CreateBackup { output, overwrite } => {
            assert_eq!(output, PathBuf::from("/tmp/vault-backup.jig"));
            assert!(overwrite);
        }
        other => panic!("unexpected action: {other:?}"),
    }
}

#[test]
fn passphrase_form_emits_metadata_only_action() {
    let mut app = browsing_app();
    assert!(matches!(
        app.activate_direct_command(UiCommand::ChangePassphrase),
        CommandOutcome::Redraw
    ));
    handle_paste(&mut app, std::str::from_utf8(SENTINEL).unwrap());
    handle_key(&mut app, KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    handle_paste(&mut app, std::str::from_utf8(SENTINEL).unwrap());
    let action = submit_key(&mut app);
    let debug = format!("{action:?}");
    assert!(!debug.contains(std::str::from_utf8(SENTINEL).unwrap()));
    assert!(matches!(action, VaultAction::ChangePassphrase { .. }));
}

#[cfg(target_os = "linux")]
#[test]
fn absent_restore_form_protects_passphrase_and_requires_restore_text() {
    let mut app = App::new(descriptor(false));
    assert!(matches!(
        app.activate_direct_command(UiCommand::RestoreBackup),
        CommandOutcome::Redraw
    ));
    handle_paste(&mut app, "/tmp/vault-backup.jig");
    handle_key(&mut app, KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    handle_paste(&mut app, std::str::from_utf8(SENTINEL).unwrap());
    let backend = TestBackend::new(90, 24);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal.draw(|frame| render::draw(frame, &app)).unwrap();
    let rendered = terminal.backend().to_string();
    assert!(!rendered.contains(std::str::from_utf8(SENTINEL).unwrap()));
    handle_key(&mut app, KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    handle_paste(&mut app, "RESTORE");
    let action = submit_key(&mut app);
    assert!(!format!("{action:?}").contains(std::str::from_utf8(SENTINEL).unwrap()));
    assert!(matches!(action, VaultAction::RestoreBackup { .. }));
}

#[cfg(unix)]
#[test]
fn export_form_emits_a_canonical_private_sink() {
    let mut app = browsing_app();
    handle_key(
        &mut app,
        KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE),
    );
    assert!(matches!(app.screen, Screen::ToolForm(_)));
    handle_paste(&mut app, "/tmp/private-export.bin");
    handle_key(&mut app, KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    handle_key(
        &mut app,
        KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE),
    );
    match submit_key(&mut app) {
        VaultAction::ExportField {
            reference,
            output,
            overwrite,
        } => {
            assert_eq!(reference.to_string(), "jig://Production/API_TOKEN");
            assert_eq!(output, PathBuf::from("/tmp/private-export.bin"));
            assert!(overwrite);
        }
        other => panic!("unexpected action: {other:?}"),
    }

    app.apply_snapshot(snapshot());
    select_legacy(&mut app);
    app.begin_export();
    assert!(matches!(app.screen, Screen::Browse));
    assert!(app.status.as_ref().unwrap().text.contains("convert"));
}

#[test]
fn peek_is_a_canonical_controlled_terminal_sink() {
    let mut app = browsing_app();
    handle_key(
        &mut app,
        KeyEvent::new(KeyCode::Char('p'), KeyModifiers::NONE),
    );
    assert!(matches!(app.screen, Screen::ConfirmPeek(_)));
    let backend = TestBackend::new(100, 28);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal.draw(|frame| render::draw(frame, &app)).unwrap();
    let rendered = terminal.backend().to_string();
    assert!(
        rendered.contains("Controlled terminal preview"),
        "{rendered}"
    );
    assert!(!rendered.contains(std::str::from_utf8(SENTINEL).unwrap()));

    handle_paste(&mut app, "PEEK");
    let RuntimeAction::Peek(reference) =
        handle_key(&mut app, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
    else {
        panic!("peek confirmation did not produce direct output action");
    };
    assert_eq!(reference.to_string(), "jig://Production/API_TOKEN");

    app.apply_snapshot(snapshot());
    select_legacy(&mut app);
    app.begin_peek();
    assert!(matches!(app.screen, Screen::Browse));
    assert!(app.status.as_ref().unwrap().text.contains("convert"));
}

#[test]
fn locking_drops_pending_protected_tool_inputs_and_metadata() {
    let mut app = browsing_app();
    assert!(matches!(
        app.activate_direct_command(UiCommand::ChangePassphrase),
        CommandOutcome::Redraw
    ));
    handle_paste(&mut app, std::str::from_utf8(SENTINEL).unwrap());
    assert!(matches!(app.screen, Screen::ToolForm(_)));

    app.lock();

    assert!(app.snapshot.is_none());
    assert!(app.selected_entry.is_none());
    assert!(app.filter.is_empty());
    assert!(matches!(app.screen, Screen::Locked(_)));
    assert!(!format!("{:?}", app.screen).contains(std::str::from_utf8(SENTINEL).unwrap()));
}
