use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{Terminal, backend::TestBackend};

use crate::tests::{SENTINEL, browsing_app};
use crate::{
    model::{EntryIdentity, Focus},
    render,
    runtime::{handle_key, handle_paste},
};

#[test]
fn text_is_visible_in_details_and_replacement_but_secrets_stay_hidden() {
    let mut app = browsing_app();
    app.selected_entry = Some(EntryIdentity::Field(
        "jig://Production/API_URL".parse().unwrap(),
    ));
    app.focus = Focus::Details;
    for width in [72, 150] {
        let mut terminal = Terminal::new(TestBackend::new(width, 35)).unwrap();
        terminal.draw(|frame| render::draw(frame, &app)).unwrap();
        let rendered = terminal.backend().to_string();
        assert!(rendered.contains("https://example.invalid"), "{rendered}");
        assert!(!rendered.contains(std::str::from_utf8(SENTINEL).unwrap()));
    }
    app.begin_replace();
    handle_paste(&mut app, "https://replacement.invalid");
    let mut terminal = Terminal::new(TestBackend::new(100, 35)).unwrap();
    terminal.draw(|frame| render::draw(frame, &app)).unwrap();
    assert!(
        terminal
            .backend()
            .to_string()
            .contains("https://replacement.invalid")
    );
    handle_key(&mut app, KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    app.selected_entry = Some(EntryIdentity::Field(
        "jig://Production/API_TOKEN".parse().unwrap(),
    ));
    terminal.draw(|frame| render::draw(frame, &app)).unwrap();
    let rendered = terminal.backend().to_string();
    assert!(rendered.contains("Value hidden"));
    assert!(!rendered.contains(std::str::from_utf8(SENTINEL).unwrap()));
    app.lock();
    terminal.draw(|frame| render::draw(frame, &app)).unwrap();
    assert!(
        !terminal
            .backend()
            .to_string()
            .contains("https://example.invalid")
    );
}

#[test]
fn minimum_details_exposes_text_and_scrolls_wrapped_content() {
    use jig_vault::{FieldKind, SecretBytes, Vault};
    use secrecy::SecretString;

    let temp = tempfile::tempdir().unwrap();
    let vault = Vault::resolve_for_test(Some(temp.path().join("ExampleVault"))).unwrap();
    let passphrase = SecretString::from("correct horse battery staple".to_owned());
    vault.init(&passphrase).unwrap();
    for (field, value) in [
        (
            "URL",
            format!("START {} END-VALUE", "wrapped text ".repeat(80)),
        ),
        ("OTHER", "OTHER-VALUE".to_owned()),
    ] {
        vault
            .set_field(
                &passphrase,
                format!("jig://ExampleProject/{field}").parse().unwrap(),
                FieldKind::Text,
                SecretBytes::new(value.into_bytes()),
            )
            .unwrap();
    }
    let mut app = browsing_app();
    app.apply_snapshot(vault.display_snapshot(&passphrase).unwrap());
    app.selected_entry = Some(EntryIdentity::Field(
        "jig://ExampleProject/URL".parse().unwrap(),
    ));
    app.focus = Focus::Details;
    let mut terminal = Terminal::new(TestBackend::new(46, 12)).unwrap();
    terminal.draw(|frame| render::draw(frame, &app)).unwrap();
    assert!(terminal.backend().to_string().contains("Value: START"));
    let mut saw_end = false;
    for _ in 0..100 {
        handle_key(&mut app, KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        terminal.draw(|frame| render::draw(frame, &app)).unwrap();
        saw_end |= terminal.backend().to_string().contains("END-VALUE");
    }
    assert!(
        saw_end,
        "wrapped value suffix must be reachable without Peek"
    );
    handle_key(&mut app, KeyEvent::new(KeyCode::Home, KeyModifiers::NONE));
    terminal.draw(|frame| render::draw(frame, &app)).unwrap();
    assert!(terminal.backend().to_string().contains("Value: START"));
    handle_key(&mut app, KeyEvent::new(KeyCode::End, KeyModifiers::NONE));
    terminal.draw(|frame| render::draw(frame, &app)).unwrap();
    assert!(terminal.backend().to_string().contains("Updated:"));
    app.selected_entry = Some(EntryIdentity::Field(
        "jig://ExampleProject/OTHER".parse().unwrap(),
    ));
    terminal.draw(|frame| render::draw(frame, &app)).unwrap();
    assert!(terminal.backend().to_string().contains("OTHER-VALUE"));
    // Resizing to a roomy viewport clamps stale scroll offsets as well.
    handle_key(&mut app, KeyEvent::new(KeyCode::End, KeyModifiers::NONE));
    let mut terminal = Terminal::new(TestBackend::new(150, 35)).unwrap();
    terminal.draw(|frame| render::draw(frame, &app)).unwrap();
    assert!(terminal.backend().to_string().contains("OTHER-VALUE"));
}

#[test]
fn text_without_a_preview_is_shown_as_unavailable_in_details() {
    use jig_vault::{FieldKind, SecretBytes, Vault};
    use secrecy::SecretString;

    let temp = tempfile::tempdir().unwrap();
    let vault = Vault::resolve_for_test(Some(temp.path().join("ExampleVault"))).unwrap();
    let passphrase = SecretString::from("correct horse battery staple".to_owned());
    vault.init(&passphrase).unwrap();
    let reference = "jig://ExampleProject/FIELD".parse().unwrap();
    vault
        .set_field(
            &passphrase,
            reference,
            FieldKind::Text,
            SecretBytes::new(b"example".to_vec()),
        )
        .unwrap();
    let mut app = browsing_app();
    // A metadata snapshot supplies the same absent-preview state produced for
    // a malformed field, without adding corruption hooks to the public API.
    app.apply_snapshot(vault.snapshot(&passphrase).unwrap());
    app.selected_entry = Some(EntryIdentity::Field(
        "jig://ExampleProject/FIELD".parse().unwrap(),
    ));
    app.focus = Focus::Details;
    let mut terminal = Terminal::new(TestBackend::new(100, 35)).unwrap();
    terminal.draw(|frame| render::draw(frame, &app)).unwrap();
    let rendered = terminal.backend().to_string();
    assert!(rendered.contains("Text preview unavailable"), "{rendered}");
    assert!(!rendered.contains("Use Peek"), "{rendered}");
}
