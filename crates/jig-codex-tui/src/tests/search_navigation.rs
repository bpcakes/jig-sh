//! Search, filtering, keyboard navigation, and detail-pane scrolling behavior.

use super::*;

#[test]
fn search_filters_details_and_enter_selects_exact_path_while_loading() {
    let mut app = app(homes());
    assert_eq!(
        handle_key(
            &mut app,
            KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE)
        ),
        Action::Redraw
    );
    for character in "work".chars() {
        handle_key(
            &mut app,
            KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE),
        );
    }
    assert_eq!(app.visible_indices(), vec![1]);
    assert_eq!(app.selected_path(), Some(PathBuf::from("/tmp/.codex-work")));
    assert_eq!(
        handle_key(&mut app, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
        Action::Select
    );
}

#[test]
fn search_supports_fuzzy_subsequence_matching() {
    let mut app = app(homes());
    for character in "cdxwk".chars() {
        app.push_filter(character);
    }
    assert_eq!(app.visible_indices(), vec![1]);
}

#[test]
fn inspection_updates_refresh_the_home_search_index() {
    let mut app = app(homes());
    for character in "person".chars() {
        app.push_filter(character);
    }
    assert!(app.visible_indices().is_empty());
    assert_eq!(app.selected, None);

    app.apply_update(ready_update(1));

    assert_eq!(app.visible_indices(), vec![1]);
    assert_eq!(app.selected, Some(1));
}

#[test]
fn search_prioritizes_home_names_over_a_matching_common_path() {
    let homes = vec![
        Home {
            path: PathBuf::from("/Users/workman/.codex"),
            name: "codex".into(),
            current: true,
        },
        Home {
            path: PathBuf::from("/Users/workman/.codex-work"),
            name: "codex-work".into(),
            current: false,
        },
    ];
    let mut app = app(homes);

    for character in "work".chars() {
        app.push_filter(character);
    }

    assert_eq!(app.visible_indices(), vec![1, 0]);
    assert_eq!(
        app.selected_path(),
        Some(PathBuf::from("/Users/workman/.codex-work"))
    );
}

#[test]
fn large_home_list_search_filters_and_ranks_stably() {
    let mut homes = (0..2_048)
        .map(|index| Home {
            path: PathBuf::from(format!("/tmp/codex-home-{index}")),
            name: format!("codex-{index}"),
            current: index == 0,
        })
        .collect::<Vec<_>>();
    homes[1_024].name = "production".into();
    homes[1_536].name = "production-secondary".into();
    homes[2_047].path = PathBuf::from("/tmp/production-archive");
    let mut app = app(homes);

    for character in "PrOdUcTiOn".chars() {
        app.push_filter(character);
    }

    assert_eq!(app.visible_indices(), vec![1_024, 1_536, 2_047]);
    assert_eq!(app.selected, Some(1_024));
}

#[test]
fn escape_leaves_search_before_it_cancels() {
    let mut app = app(homes());
    app.searching = true;
    assert_eq!(
        handle_key(&mut app, KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)),
        Action::Redraw
    );
    assert!(!app.searching);
    assert_eq!(
        handle_key(&mut app, KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)),
        Action::Cancel
    );
}

#[test]
fn control_u_clears_the_filter_while_search_remains_active() {
    let mut app = app(homes());
    app.searching = true;
    for character in "work".chars() {
        app.push_filter(character);
    }

    assert_eq!(
        handle_key(
            &mut app,
            KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL)
        ),
        Action::Redraw
    );
    assert!(app.filter.is_empty());
    assert!(app.searching);
}

#[test]
fn search_mode_keeps_list_paging_keys_active() {
    let homes = (0..25)
        .map(|index| Home {
            path: PathBuf::from(format!("/tmp/.codex-{index}")),
            name: format!("codex-{index}"),
            current: index == 0,
        })
        .collect();
    let mut app = app(homes);
    app.searching = true;

    for (key, expected) in [
        (KeyCode::End, 24),
        (KeyCode::PageUp, 14),
        (KeyCode::Home, 0),
        (KeyCode::PageDown, 10),
    ] {
        assert_eq!(
            handle_key(&mut app, KeyEvent::new(key, KeyModifiers::NONE)),
            Action::Redraw
        );
        assert_eq!(app.selected, Some(expected));
        assert!(app.searching);
    }
}

#[test]
fn tab_focuses_the_detail_pane_and_navigation_scrolls_it() {
    let mut app = app(homes());
    assert_eq!(
        handle_key(&mut app, KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE)),
        Action::Redraw
    );
    assert_eq!(app.focus, Focus::Details);
    app.set_detail_scroll_limit(5);
    handle_key(&mut app, KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    assert_eq!(app.detail_scroll, 1);
    assert_eq!(app.selected, Some(0));
}

#[test]
fn end_then_up_moves_immediately_in_the_detail_pane() {
    let mut app = app(homes());
    app.apply_update(ready_update(0));
    app.focus = Focus::Details;
    let max_scroll = 12;
    app.set_detail_scroll_limit(max_scroll);

    handle_key(&mut app, KeyEvent::new(KeyCode::End, KeyModifiers::NONE));
    assert_eq!(app.detail_scroll, max_scroll);
    handle_key(&mut app, KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
    assert_eq!(app.detail_scroll, max_scroll - 1);
    handle_key(&mut app, KeyEvent::new(KeyCode::PageUp, KeyModifiers::NONE));
    assert!(app.detail_scroll < max_scroll - 1);
    handle_key(&mut app, KeyEvent::new(KeyCode::Home, KeyModifiers::NONE));
    assert_eq!(app.detail_scroll, 0);
}

#[test]
fn search_cursor_uses_terminal_cell_width() {
    let backend = TestBackend::new(120, 30);
    let mut terminal = Terminal::new(backend).unwrap();
    let mut app = app(homes());
    app.searching = true;
    app.push_filter('界');

    terminal.draw(|frame| render::draw(frame, &app)).unwrap();
    terminal.backend_mut().assert_cursor_position((10, 28));

    app.clear_filter();
    app.push_filter('e');
    app.push_filter('\u{301}');
    terminal.draw(|frame| render::draw(frame, &app)).unwrap();
    terminal.backend_mut().assert_cursor_position((9, 28));

    app.clear_filter();
    app.push_filter('\u{201c}');
    app.push_filter('\u{fe01}');
    terminal.draw(|frame| render::draw(frame, &app)).unwrap();
    terminal.backend_mut().assert_cursor_position((9, 28));
}

#[test]
fn wrapped_detail_content_scrolls_to_its_final_rendered_row() {
    let backend = TestBackend::new(70, 14);
    let mut terminal = Terminal::new(backend).unwrap();
    let mut app = app(homes());
    let mut update = ready_update(0);
    update.details["inspection_error"] = json!(format!(
        "{} TAIL-MARKER",
        "long inspection failure ".repeat(20)
    ));
    app.apply_update(update);
    app.finish_inspection(None);
    app.focus = Focus::Details;

    terminal.draw(|frame| render::draw(frame, &app)).unwrap();
    handle_key(&mut app, KeyEvent::new(KeyCode::End, KeyModifiers::NONE));
    assert!(app.detail_scroll > 0);
    terminal.draw(|frame| render::draw(frame, &app)).unwrap();

    let rendered = terminal.backend().to_string();
    assert!(rendered.contains("TAIL-MARKER"), "{rendered}");
}

#[test]
fn short_detail_content_does_not_overscroll() {
    let backend = TestBackend::new(120, 30);
    let mut terminal = Terminal::new(backend).unwrap();
    let mut app = app(homes());
    app.focus = Focus::Details;

    terminal.draw(|frame| render::draw(frame, &app)).unwrap();
    handle_key(&mut app, KeyEvent::new(KeyCode::End, KeyModifiers::NONE));

    assert_eq!(app.detail_scroll, 0);
}
