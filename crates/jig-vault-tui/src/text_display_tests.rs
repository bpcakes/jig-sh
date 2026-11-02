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
