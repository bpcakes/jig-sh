use std::path::PathBuf;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use jig_vault::{FieldKind, MAX_SECRET_VALUE_LEN, SecretBytes, Vault, VaultSnapshot};
use ratatui::{Terminal, backend::TestBackend};
use secrecy::SecretString;
use tempfile::tempdir;

use crate::{
    ImportPlanToken, ImportPreview, ImportPreviewAuthorization, ImportPreviewRow, VaultAction,
    VaultDescriptor, VaultMutation,
    model::{
        App, DeleteTarget, EntryIdentity, Focus, ItemIdentity, ManagementForm,
        MutationConfirmation, MutationConfirmationKind, Screen,
    },
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
    assert_eq!(value_file, &source.to_string_lossy());
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
    let Screen::Form(ManagementForm::WriteField { value, mode, .. }) = &app.screen else {
        panic!("expected field replacement form");
    };
    assert!(value.is_empty());
    assert_eq!(*mode, jig_vault::VaultWriteMode::Replace);

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
    *item = "Imported".to_owned();
    *field = "TOKEN".to_owned();
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
    if let Some(input) = app.metadata_input_mut() {
        input.clear();
    }
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
    assert!(confirmation.target.label().contains("2 fields"));
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
fn tools_palette_opens_verified_activity_and_audit_results() {
    let mut app = browsing_app();
    handle_key(
        &mut app,
        KeyEvent::new(KeyCode::Char(':'), KeyModifiers::NONE),
    );
    assert!(matches!(app.screen, Screen::Tools(_)));
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
fn onepassword_form_previews_metadata_before_exact_commit_confirmation() {
    let mut app = browsing_app();
    handle_key(
        &mut app,
        KeyEvent::new(KeyCode::Char(':'), KeyModifiers::NONE),
    );
    handle_key(&mut app, KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    handle_key(&mut app, KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
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
            kind: FieldKind::Concealed,
            replaces_existing: true,
        }],
        destination_exists: true,
    });
    handle_paste(&mut app, "IMPORT");
    assert!(matches!(
        handle_key(&mut app, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
        RuntimeAction::Redraw
    ));
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
fn backup_and_passphrase_forms_emit_metadata_only_actions() {
    let mut app = browsing_app();
    app.open_tools();
    app.move_tool_selection(3);
    app.activate_tool();
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

    app.apply_snapshot(snapshot());
    app.open_tools();
    app.move_tool_selection(4);
    app.activate_tool();
    handle_paste(&mut app, std::str::from_utf8(SENTINEL).unwrap());
    handle_key(&mut app, KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    handle_paste(&mut app, std::str::from_utf8(SENTINEL).unwrap());
    let action = submit_key(&mut app);
    let debug = format!("{action:?}");
    assert!(!debug.contains(std::str::from_utf8(SENTINEL).unwrap()));
    assert!(matches!(action, VaultAction::ChangePassphrase { .. }));
}

#[test]
fn absent_restore_form_protects_passphrase_and_requires_restore_text() {
    let mut app = App::new(descriptor(false));
    app.open_tools();
    app.activate_tool();
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

#[test]
fn export_and_peek_are_canonical_controlled_sinks() {
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
    app.begin_export();
    assert!(matches!(app.screen, Screen::Browse));
    assert!(app.status.as_ref().unwrap().text.contains("convert"));
    app.begin_peek();
    assert!(matches!(app.screen, Screen::Browse));
    assert!(app.status.as_ref().unwrap().text.contains("convert"));
}

#[test]
fn locking_drops_pending_protected_tool_inputs_and_metadata() {
    let mut app = browsing_app();
    app.open_tools();
    app.move_tool_selection(4);
    app.activate_tool();
    handle_paste(&mut app, std::str::from_utf8(SENTINEL).unwrap());
    assert!(matches!(app.screen, Screen::ToolForm(_)));

    app.lock();

    assert!(app.snapshot.is_none());
    assert!(app.selected_entry.is_none());
    assert!(app.filter.is_empty());
    assert!(matches!(app.screen, Screen::Locked(_)));
    assert!(!format!("{:?}", app.screen).contains(std::str::from_utf8(SENTINEL).unwrap()));
}
