use jig_tui::sanitize_text;
use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap},
};

use crate::model::{
    ActivityView, App, ConvertFocus, DeleteConfirmation, EntryIdentity, FieldWriteFocus, Focus,
    ImportPreviewState, InitializeFocus, ItemIdentity, LegacyWriteFocus, ManagementForm,
    RenameFieldFocus, Screen, StatusKind, kind_label,
};
use crate::tools::{BackupFocus, ImportFocus, PassphraseFocus, RestoreFocus, ToolForm, ToolsMenu};

const ACCENT: Color = Color::Cyan;
const MUTED: Color = Color::DarkGray;
const GOOD: Color = Color::Green;
const WARN: Color = Color::Yellow;
const BAD: Color = Color::Red;
const MIN_WIDTH: u16 = 46;
const MIN_HEIGHT: u16 = 12;
const WIDE_WIDTH: u16 = 104;

pub(crate) fn draw(frame: &mut Frame, app: &App) {
    let area = frame.area();
    if area.width < MIN_WIDTH || area.height < MIN_HEIGHT {
        frame.render_widget(
            Paragraph::new(format!(
                "Terminal too small: {}x{}.\nVault TUI needs at least {MIN_WIDTH}x{MIN_HEIGHT}.\nResize, or press q to exit.",
                area.width, area.height
            ))
            .block(panel("Jig Vault"))
            .wrap(Wrap { trim: true }),
            area,
        );
        return;
    }

    match &app.screen {
        Screen::Missing => draw_missing(frame, area, app),
        Screen::Locked(input) => draw_locked(frame, area, app, input.render_label()),
        Screen::Initialize {
            passphrase,
            confirmation,
            focus,
        } => draw_initialize(
            frame,
            area,
            app,
            passphrase.render_label(),
            confirmation.render_label(),
            *focus,
        ),
        Screen::Loading(label) => draw_loading(frame, area, app, label),
        Screen::Browse
        | Screen::Help
        | Screen::ConfirmMigration
        | Screen::Form(_)
        | Screen::ConfirmDelete(_)
        | Screen::Tools(_)
        | Screen::ToolForm(_)
        | Screen::ImportPreview(_)
        | Screen::Activity(_)
        | Screen::AuditResult(_) => {
            if app.snapshot.is_some() {
                draw_browser(frame, area, app);
            } else {
                draw_missing(frame, area, app);
            }
            match &app.screen {
                Screen::Help => draw_help(frame, centered_rect(78, 74, area)),
                Screen::ConfirmMigration => {
                    draw_migration_confirmation(frame, centered_rect(72, 42, area));
                }
                Screen::Form(form) => {
                    draw_management_form(frame, centered_rect(78, 68, area), app, form);
                }
                Screen::ConfirmDelete(confirmation) => {
                    draw_delete_confirmation(frame, centered_rect(78, 52, area), app, confirmation);
                }
                Screen::Tools(menu) => draw_tools(frame, centered_rect(70, 60, area), menu),
                Screen::ToolForm(form) => {
                    draw_tool_form(frame, centered_rect(82, 72, area), app, form);
                }
                Screen::ImportPreview(preview) => {
                    draw_import_preview(frame, centered_rect(90, 82, area), app, preview);
                }
                Screen::Activity(view) => {
                    draw_activity(frame, centered_rect(90, 82, area), view);
                }
                Screen::AuditResult(verification) => {
                    draw_audit_result(frame, centered_rect(72, 48, area), verification);
                }
                _ => {}
            }
        }
    }
}

fn draw_missing(frame: &mut Frame, area: Rect, app: &App) {
    draw_public_header(frame, area, app, "absent", WARN);
    let inner = centered_rect(72, 46, area);
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(Span::styled(
                "No vault exists at this fixed scope.",
                Style::default().fg(WARN).add_modifier(Modifier::BOLD),
            )),
            Line::from(""),
            Line::from("Press i to initialize a new encrypted vault."),
            Line::from("Restore from an encrypted backup is available from this screen."),
            Line::from(""),
            Line::from("i initialize   : tools/restore   q quit"),
        ])
        .alignment(Alignment::Center)
        .block(panel("Vault not initialized"))
        .wrap(Wrap { trim: true }),
        inner,
    );
    draw_status(frame, area, app);
}

fn draw_locked(frame: &mut Frame, area: Rect, app: &App, input: String) {
    draw_public_header(frame, area, app, "locked", WARN);
    let inner = centered_rect(72, 40, area);
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(Span::styled(
                "Vault passphrase",
                Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
            )),
            Line::from(""),
            Line::from(input),
            Line::from(""),
            Line::from(Span::styled(
                "Input is protected; only bullets and byte count are rendered.",
                Style::default().fg(MUTED),
            )),
            Line::from("Enter unlock   Ctrl-U clear   q quit"),
        ])
        .alignment(Alignment::Center)
        .block(panel("Unlock"))
        .wrap(Wrap { trim: true }),
        inner,
    );
    draw_status(frame, area, app);
}

fn draw_initialize(
    frame: &mut Frame,
    area: Rect,
    app: &App,
    passphrase: String,
    confirmation: String,
    focus: InitializeFocus,
) {
    draw_public_header(frame, area, app, "initializing", WARN);
    let inner = centered_rect(76, 58, area);
    let passphrase_style = focus_style(focus == InitializeFocus::Passphrase);
    let confirmation_style = focus_style(focus == InitializeFocus::Confirmation);
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(Span::styled("New passphrase", passphrase_style)),
            Line::from(passphrase),
            Line::from(""),
            Line::from(Span::styled("Confirm passphrase", confirmation_style)),
            Line::from(confirmation),
            Line::from(""),
            Line::from(Span::styled(
                "Use at least 12 bytes. This process keeps the credential only while unlocked.",
                Style::default().fg(MUTED),
            )),
            Line::from("Tab switch field   Enter create   Esc cancel   Ctrl-U clear"),
        ])
        .alignment(Alignment::Center)
        .block(panel("Initialize encrypted vault"))
        .wrap(Wrap { trim: true }),
        inner,
    );
    draw_status(frame, area, app);
}

fn draw_loading(frame: &mut Frame, area: Rect, app: &App, label: &str) {
    draw_public_header(frame, area, app, "working", WARN);
    let spinner = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
    frame.render_widget(
        Paragraph::new(format!(
            "{} {}…\n\nPlease wait. The owned vault worker will be joined before exit.",
            spinner[app.tick % spinner.len()],
            sanitize_text(label)
        ))
        .alignment(Alignment::Center)
        .block(panel("Jig Vault"))
        .wrap(Wrap { trim: true }),
        centered_rect(72, 34, area),
    );
}

fn draw_browser(frame: &mut Frame, area: Rect, app: &App) {
    let footer_height = if app.searching || !app.filter.is_empty() || app.status.is_some() {
        5
    } else {
        4
    };
    let outer = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2),
            Constraint::Min(6),
            Constraint::Length(footer_height),
        ])
        .split(area);
    draw_browser_header(frame, outer[0], app);
    if area.width >= WIDE_WIDTH {
        let panes = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(25),
                Constraint::Percentage(34),
                Constraint::Percentage(41),
            ])
            .split(outer[1]);
        draw_items(frame, panes[0], app);
        draw_entries(frame, panes[1], app);
        draw_details(frame, panes[2], app);
    } else {
        match app.focus {
            Focus::Items => draw_items(frame, outer[1], app),
            Focus::Fields => draw_entries(frame, outer[1], app),
            Focus::Details => draw_details(frame, outer[1], app),
        }
    }
    draw_footer(frame, outer[2], app);
}

fn draw_public_header(frame: &mut Frame, area: Rect, app: &App, state: &str, color: Color) {
    let header = Rect::new(area.x, area.y, area.width, 2.min(area.height));
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(
                " Jig Vault ",
                Style::default()
                    .fg(Color::Black)
                    .bg(ACCENT)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw("  "),
            Span::styled(
                sanitize_text(&app.descriptor.scope),
                Style::default().fg(ACCENT),
            ),
            Span::raw("  "),
            Span::styled(state, Style::default().fg(color)),
            Span::raw("  "),
            Span::styled(
                sanitize_text(&app.descriptor.home.to_string_lossy()),
                Style::default().fg(MUTED),
            ),
        ])),
        header,
    );
}

fn draw_browser_header(frame: &mut Frame, area: Rect, app: &App) {
    let (items, fields, legacy) = app.snapshot_counts();
    let snapshot = app
        .snapshot
        .as_ref()
        .expect("browser always has a snapshot");
    let scope = app
        .descriptor
        .repo_name
        .as_deref()
        .unwrap_or(&app.descriptor.scope);
    let audit = if snapshot.audit.torn_tail_bytes == 0 {
        "audit verified"
    } else {
        "audit torn tail"
    };
    let version_style = if snapshot.format_version == 2 {
        Style::default().fg(GOOD)
    } else {
        Style::default().fg(WARN)
    };
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(
                " Jig Vault ",
                Style::default()
                    .fg(Color::Black)
                    .bg(ACCENT)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw("  "),
            Span::styled(sanitize_text(scope), Style::default().fg(ACCENT)),
            Span::raw("  unlocked  "),
            Span::styled(format!("v{}", snapshot.format_version), version_style),
            Span::raw(format!(
                "  {audit}  {items} items · {fields} fields · {legacy} legacy"
            )),
        ])),
        area,
    );
}

fn draw_items(frame: &mut Frame, area: Rect, app: &App) {
    let rows = app.visible_items();
    let legacy_count = app
        .snapshot
        .as_ref()
        .map_or(0, |snapshot| snapshot.legacy_secrets.len());
    let items = rows
        .iter()
        .map(|identity| {
            let label = identity.label(legacy_count);
            let count = item_count(app, identity);
            ListItem::new(format!("{label}  ({count})"))
        })
        .collect::<Vec<_>>();
    let selected = app
        .selected_item
        .as_ref()
        .and_then(|identity| rows.iter().position(|row| row == identity));
    let mut state = ListState::default().with_selected(selected);
    let title = if app.focus == Focus::Items {
        "Items •"
    } else {
        "Items"
    };
    frame.render_stateful_widget(list(items, title), area, &mut state);
}

fn draw_entries(frame: &mut Frame, area: Rect, app: &App) {
    let rows = app.visible_entries();
    let items = rows
        .iter()
        .map(|identity| {
            let suffix = match identity {
                EntryIdentity::Field(reference) => app
                    .snapshot
                    .as_ref()
                    .and_then(|snapshot| {
                        snapshot
                            .fields
                            .iter()
                            .find(|field| &field.reference == reference)
                    })
                    .map(|field| format!("  [{}]", kind_label(field.kind)))
                    .unwrap_or_default(),
                EntryIdentity::Legacy(_) => "  [legacy]".to_owned(),
            };
            ListItem::new(format!("{}{}", identity.label(), suffix))
        })
        .collect::<Vec<_>>();
    let selected = app
        .selected_entry
        .as_ref()
        .and_then(|identity| rows.iter().position(|row| row == identity));
    let mut state = ListState::default().with_selected(selected);
    let base = if matches!(app.selected_item, Some(ItemIdentity::Legacy)) {
        "Legacy entries"
    } else {
        "Fields"
    };
    let title = if app.focus == Focus::Fields {
        format!("{base} •")
    } else {
        base.to_owned()
    };
    frame.render_stateful_widget(list(items, &title), area, &mut state);
}

fn draw_details(frame: &mut Frame, area: Rect, app: &App) {
    let title = if app.focus == Focus::Details {
        "Details •"
    } else {
        "Details"
    };
    let lines = if let Some(field) = app.selected_field() {
        vec![
            key_value("Reference", &field.reference.to_string()),
            key_value("Item", field.reference.item()),
            key_value("Field", field.reference.field()),
            key_value("Kind", kind_label(field.kind)),
            key_value("Length", &format!("{} bytes", field.value_len)),
            key_value("Created", &format!("{} ms", field.created_at_ms)),
            key_value("Updated", &format!("{} ms", field.updated_at_ms)),
            Line::from(""),
            Line::from(Span::styled(
                "Value hidden. Selection never decrypts it.",
                Style::default().fg(MUTED),
            )),
        ]
    } else if let Some(secret) = app.selected_legacy() {
        vec![
            key_value("Legacy name", &secret.name),
            key_value("Length", &format!("{} bytes", secret.value_len)),
            key_value("Created", &format!("{} ms", secret.created_at_ms)),
            key_value("Updated", &format!("{} ms", secret.updated_at_ms)),
            Line::from(""),
            Line::from(Span::styled(
                "Unrepresentable legacy entry; convert it to a canonical field for export.",
                Style::default().fg(WARN),
            )),
        ]
    } else {
        vec![Line::from("No entry selected.")]
    };
    frame.render_widget(
        Paragraph::new(lines)
            .block(panel(title))
            .wrap(Wrap { trim: false }),
        area,
    );
}

fn draw_footer(frame: &mut Frame, area: Rect, app: &App) {
    let breadcrumb = if frame.area().width < WIDE_WIDTH {
        format!(
            "{} › {} › {}  ",
            item_breadcrumb(app),
            entry_breadcrumb(app),
            focus_label(app.focus)
        )
    } else {
        String::new()
    };
    let controls = if app.searching {
        "Type to filter metadata  Backspace edit  Ctrl-U clear  Enter finish  Esc finish"
    } else if app
        .snapshot
        .as_ref()
        .is_some_and(|snapshot| snapshot.format_version == 1)
    {
        "READ-ONLY v1  m migrate  / filter  Tab/h/l focus  j/k move  r refresh  L lock  ? help  q quit"
    } else {
        "/ filter  Tab/h/l focus  j/k move  r refresh  L lock  ? help  q quit"
    };
    let mut lines = vec![Line::from(format!("{breadcrumb}{controls}"))];
    if !app.searching
        && app
            .snapshot
            .as_ref()
            .is_some_and(|snapshot| snapshot.format_version == 2)
    {
        lines.push(Line::from(
            "a add  A legacy  e replace  K kind  n rename  c convert  D delete  : tools",
        ));
    }
    if app.searching || !app.filter.is_empty() {
        lines.push(Line::from(vec![
            Span::styled("/", Style::default().fg(ACCENT)),
            Span::raw(sanitize_text(&app.filter)),
        ]));
    } else if let Some(status) = &app.status {
        let color = match status.kind {
            StatusKind::Info => GOOD,
            StatusKind::Error => BAD,
        };
        lines.push(Line::from(Span::styled(
            status.text.clone(),
            Style::default().fg(color),
        )));
    }
    frame.render_widget(Paragraph::new(lines).block(panel("Keys")), area);
}

fn draw_help(frame: &mut Frame, area: Rect) {
    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(Span::styled(
                "Keyboard",
                Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
            )),
            Line::from("j/k or ↑/↓  move selection"),
            Line::from("h/l or Tab   change focused pane"),
            Line::from("/            filter item, field, reference, or legacy metadata"),
            Line::from("r            reopen and refresh authenticated metadata"),
            Line::from("m            explicitly migrate a version 1 vault"),
            Line::from("a / A        add a field / explicit legacy entry"),
            Line::from("e / K        replace a value / change field kind"),
            Line::from("n / c        rename field or item / convert legacy entry"),
            Line::from("D            delete with exact typed confirmation"),
            Line::from(":            lifecycle tools"),
            Line::from("L            lock and wipe process-local session state"),
            Line::from("q            quit"),
            Line::from(""),
            Line::from(Span::styled(
                "Values remain hidden. Operational exec/run/inject workflows stay in the CLI.",
                Style::default().fg(MUTED),
            )),
            Line::from(""),
            Line::from("Esc or ? closes help."),
        ])
        .block(panel("Vault help"))
        .wrap(Wrap { trim: true }),
        area,
    );
}

fn draw_migration_confirmation(frame: &mut Frame, area: Rect) {
    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(Span::styled(
                "Migrate vault from version 1 to version 2?",
                Style::default().fg(WARN).add_modifier(Modifier::BOLD),
            )),
            Line::from(""),
            Line::from("This one-way upgrade preserves values as concealed fields."),
            Line::from("Older Jig versions will reject the migrated vault."),
            Line::from(""),
            Line::from("Enter migrate   Esc cancel"),
        ])
        .alignment(Alignment::Center)
        .block(panel("Confirm migration"))
        .wrap(Wrap { trim: true }),
        area,
    );
}

fn draw_management_form(frame: &mut Frame, area: Rect, app: &App, form: &ManagementForm) {
    frame.render_widget(Clear, area);
    let (title, mut lines) = match form {
        ManagementForm::WriteField {
            mode,
            item,
            field,
            kind,
            value,
            value_file,
            focus,
        } => {
            let action = match mode {
                jig_vault::VaultWriteMode::Create => "Create field",
                jig_vault::VaultWriteMode::Replace => "Replace field value",
                jig_vault::VaultWriteMode::Upsert => "Write field",
            };
            let mut lines = vec![
                form_value_line("Item", item, *focus == FieldWriteFocus::Item, false),
                form_value_line("Field", field, *focus == FieldWriteFocus::Field, false),
                form_value_line(
                    "Kind",
                    kind_label(*kind),
                    *focus == FieldWriteFocus::Kind,
                    false,
                ),
                form_value_line(
                    "Value",
                    &value.render_label(),
                    *focus == FieldWriteFocus::Value,
                    true,
                ),
                form_value_line(
                    "Value file",
                    value_file,
                    *focus == FieldWriteFocus::File,
                    false,
                ),
                Line::from(""),
            ];
            if *mode == jig_vault::VaultWriteMode::Replace {
                lines.push(Line::from(Span::styled(
                    "The current value was not loaded. It remains unchanged until Save succeeds.",
                    Style::default().fg(WARN),
                )));
            }
            (action, lines)
        }
        ManagementForm::WriteLegacy {
            mode,
            name,
            value,
            value_file,
            focus,
        } => {
            let action = match mode {
                jig_vault::VaultWriteMode::Create => "Create legacy entry",
                jig_vault::VaultWriteMode::Replace => "Replace legacy value",
                jig_vault::VaultWriteMode::Upsert => "Write legacy entry",
            };
            let mut lines = vec![
                form_value_line("Name", name, *focus == LegacyWriteFocus::Name, false),
                form_value_line(
                    "Value",
                    &value.render_label(),
                    *focus == LegacyWriteFocus::Value,
                    true,
                ),
                form_value_line(
                    "Value file",
                    value_file,
                    *focus == LegacyWriteFocus::File,
                    false,
                ),
                Line::from(""),
            ];
            if *mode == jig_vault::VaultWriteMode::Replace {
                lines.push(Line::from(Span::styled(
                    "The current value was not loaded. It remains unchanged until Save succeeds.",
                    Style::default().fg(WARN),
                )));
            }
            (action, lines)
        }
        ManagementForm::ChangeKind {
            reference,
            from,
            to,
        } => (
            "Change field kind",
            vec![
                key_value("Reference", &reference.to_string()),
                key_value("Current kind", kind_label(*from)),
                form_value_line("New kind", kind_label(*to), true, false),
                Line::from(""),
                Line::from("Space toggles the target kind."),
            ],
        ),
        ManagementForm::RenameField {
            source,
            destination_item,
            destination_field,
            focus,
        } => (
            "Rename or move field",
            vec![
                key_value("Source", &source.to_string()),
                form_value_line(
                    "Destination item",
                    destination_item,
                    *focus == RenameFieldFocus::Item,
                    false,
                ),
                form_value_line(
                    "Destination field",
                    destination_field,
                    *focus == RenameFieldFocus::Field,
                    false,
                ),
            ],
        ),
        ManagementForm::RenameItem {
            source,
            destination,
        } => (
            "Rename item",
            vec![
                key_value("Source", &format!("jig://{source}")),
                form_value_line("Destination item", destination, true, false),
            ],
        ),
        ManagementForm::ConvertLegacy {
            source,
            item,
            field,
            kind,
            focus,
        } => (
            "Convert legacy entry",
            vec![
                key_value("Legacy source", source),
                form_value_line("Item", item, *focus == ConvertFocus::Item, false),
                form_value_line("Field", field, *focus == ConvertFocus::Field, false),
                form_value_line(
                    "Kind",
                    kind_label(*kind),
                    *focus == ConvertFocus::Kind,
                    false,
                ),
                Line::from(""),
                Line::from("Conversion atomically moves the existing encrypted value."),
            ],
        ),
    };
    lines.push(Line::from(""));
    lines.push(Line::from(
        "Tab switch field   Space toggle kind   Enter save   Esc cancel   Ctrl-U clear",
    ));
    if let Some(status) = &app.status {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            status.text.clone(),
            Style::default().fg(match status.kind {
                StatusKind::Info => GOOD,
                StatusKind::Error => BAD,
            }),
        )));
    }
    frame.render_widget(
        Paragraph::new(lines)
            .block(panel(title))
            .wrap(Wrap { trim: true }),
        area,
    );
}

fn draw_delete_confirmation(
    frame: &mut Frame,
    area: Rect,
    app: &App,
    confirmation: &DeleteConfirmation,
) {
    frame.render_widget(Clear, area);
    let required = confirmation.target.required_confirmation();
    let mut lines = vec![
        Line::from(Span::styled(
            format!("Permanently remove {}?", confirmation.target.label()),
            Style::default().fg(BAD).add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from("There is no trash or undo."),
        Line::from(format!("Type exactly: {}", sanitize_text(&required))),
        form_value_line("Confirmation", &confirmation.input, true, false),
        Line::from(""),
        Line::from("Enter delete   Esc cancel   Ctrl-U clear"),
    ];
    if let Some(status) = &app.status {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            status.text.clone(),
            Style::default().fg(BAD),
        )));
    }
    frame.render_widget(
        Paragraph::new(lines)
            .block(panel("Confirm permanent deletion"))
            .wrap(Wrap { trim: true }),
        area,
    );
}

fn draw_tools(frame: &mut Frame, area: Rect, menu: &ToolsMenu) {
    frame.render_widget(Clear, area);
    let items = menu
        .choices
        .iter()
        .map(|choice| ListItem::new(choice.label()))
        .collect::<Vec<_>>();
    let mut state = ListState::default().with_selected(Some(menu.selected));
    frame.render_stateful_widget(
        list(items, "Vault tools")
            .highlight_style(
                Style::default()
                    .bg(Color::Blue)
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            )
            .highlight_symbol("› "),
        area,
        &mut state,
    );
    let footer = Rect::new(
        area.x.saturating_add(2),
        area.bottom().saturating_sub(2),
        area.width.saturating_sub(4),
        1,
    );
    frame.render_widget(
        Paragraph::new("j/k move   Enter open   Esc close").style(Style::default().fg(MUTED)),
        footer,
    );
}

fn draw_tool_form(frame: &mut Frame, area: Rect, app: &App, form: &ToolForm) {
    frame.render_widget(Clear, area);
    let (title, mut lines) = match form {
        ToolForm::ImportOnePassword {
            env_file,
            item,
            out_env,
            replace,
            overwrite,
            dry_run,
            focus,
        } => (
            "1Password dotenv import",
            vec![
                form_value_line(
                    "Source .env",
                    env_file,
                    *focus == ImportFocus::EnvFile,
                    false,
                ),
                form_value_line("Item", item, *focus == ImportFocus::Item, false),
                form_value_line(
                    "Generated .env",
                    out_env,
                    *focus == ImportFocus::OutEnv,
                    false,
                ),
                toggle_line("Replace fields", *replace, *focus == ImportFocus::Replace),
                toggle_line(
                    "Overwrite generated file",
                    *overwrite,
                    *focus == ImportFocus::Overwrite,
                ),
                toggle_line("Dry run", *dry_run, *focus == ImportFocus::DryRun),
                Line::from(""),
                Line::from(Span::styled(
                    "Preview parses metadata and checks collisions without invoking `op`.",
                    Style::default().fg(MUTED),
                )),
            ],
        ),
        ToolForm::CreateBackup {
            output,
            overwrite,
            focus,
        } => (
            "Create encrypted backup",
            vec![
                form_value_line(
                    "Backup output",
                    output,
                    *focus == BackupFocus::Output,
                    false,
                ),
                toggle_line(
                    "Overwrite regular file",
                    *overwrite,
                    *focus == BackupFocus::Overwrite,
                ),
                Line::from(""),
                Line::from(Span::styled(
                    "The archive is independently encrypted and installed as a private file.",
                    Style::default().fg(MUTED),
                )),
            ],
        ),
        ToolForm::ChangePassphrase {
            new_passphrase,
            confirmation,
            focus,
        } => (
            "Change vault passphrase",
            vec![
                form_value_line(
                    "New passphrase",
                    &new_passphrase.render_label(),
                    *focus == PassphraseFocus::New,
                    true,
                ),
                form_value_line(
                    "Confirm passphrase",
                    &confirmation.render_label(),
                    *focus == PassphraseFocus::Confirmation,
                    true,
                ),
                Line::from(""),
                Line::from(Span::styled(
                    "The current process credential changes only after atomic rotation succeeds.",
                    Style::default().fg(WARN),
                )),
            ],
        ),
        ToolForm::RestoreBackup {
            input,
            passphrase,
            confirmation,
            focus,
        } => (
            "Restore encrypted backup",
            vec![
                form_value_line("Backup input", input, *focus == RestoreFocus::Input, false),
                form_value_line(
                    "Backup vault passphrase",
                    &passphrase.render_label(),
                    *focus == RestoreFocus::Passphrase,
                    true,
                ),
                form_value_line(
                    "Type RESTORE",
                    confirmation,
                    *focus == RestoreFocus::Confirmation,
                    false,
                ),
                Line::from(""),
                Line::from(Span::styled(
                    "Restore is available only for a completely absent target and rechecks that invariant at installation.",
                    Style::default().fg(WARN),
                )),
            ],
        ),
    };
    lines.push(Line::from(""));
    lines.push(Line::from(
        "Tab switch field   Space toggle option   Enter continue   Esc cancel   Ctrl-U clear",
    ));
    append_status(&mut lines, app);
    frame.render_widget(
        Paragraph::new(lines)
            .block(panel(title))
            .wrap(Wrap { trim: true }),
        area,
    );
}

fn draw_import_preview(frame: &mut Frame, area: Rect, app: &App, state: &ImportPreviewState) {
    frame.render_widget(Clear, area);
    let preview = &state.preview;
    let max_rows = usize::from(area.height.saturating_sub(13));
    let mut lines = vec![
        key_value("Source", &preview.env_file.to_string_lossy()),
        key_value("Item", &format!("jig://{}", preview.item.as_str())),
        key_value("Destination", &preview.out_env.to_string_lossy()),
        toggle_line("Replace fields (r)", preview.replace, false),
        toggle_line("Overwrite file (o)", preview.overwrite, false),
        Line::from(""),
    ];
    for row in preview.rows.iter().take(max_rows) {
        lines.push(Line::from(format!(
            "{} → {}  [{}]  {}",
            sanitize_text(&row.variable),
            sanitize_text(&row.reference.to_string()),
            kind_label(row.kind),
            if row.replaces_existing {
                "replace"
            } else {
                "create"
            }
        )));
    }
    if preview.rows.len() > max_rows {
        lines.push(Line::from(Span::styled(
            format!("… {} additional fields", preview.rows.len() - max_rows),
            Style::default().fg(MUTED),
        )));
    }
    lines.push(Line::from(""));
    if preview.dry_run {
        lines.push(Line::from(Span::styled(
            "Dry run: no 1Password values were resolved and no files or fields will change.",
            Style::default().fg(GOOD),
        )));
        lines.push(Line::from("Enter finish dry run   Esc close"));
    } else {
        lines.push(Line::from(Span::styled(
            "Commit will invoke `op`, atomically import fields, then install the private dotenv file.",
            Style::default().fg(WARN),
        )));
        lines.push(form_value_line(
            "Type IMPORT",
            &state.confirmation,
            true,
            false,
        ));
        lines.push(Line::from(
            "r/o toggle permissions   Enter commit   Esc cancel   Ctrl-U clear",
        ));
    }
    append_status(&mut lines, app);
    frame.render_widget(
        Paragraph::new(lines)
            .block(panel("1Password import preview"))
            .wrap(Wrap { trim: true }),
        area,
    );
}

fn draw_activity(frame: &mut Frame, area: Rect, view: &ActivityView) {
    frame.render_widget(Clear, area);
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage(65),
            Constraint::Min(5),
            Constraint::Length(1),
        ])
        .split(area);
    let items = view
        .records
        .iter()
        .map(|record| {
            let subject = record.subject.as_deref().unwrap_or("—");
            ListItem::new(format!(
                "{}  {}  {}",
                record.timestamp_ms,
                sanitize_text(&record.action),
                sanitize_text(subject)
            ))
        })
        .collect::<Vec<_>>();
    let selected = (!view.records.is_empty()).then_some(view.selected);
    let mut state = ListState::default().with_selected(selected);
    frame.render_stateful_widget(
        list(items, "Verified activity (newest first)"),
        chunks[0],
        &mut state,
    );
    let details = view.records.get(view.selected).map_or_else(
        || vec![Line::from("No verified activity records.")],
        |record| {
            vec![
                key_value("Action", &record.action),
                key_value("Subject", record.subject.as_deref().unwrap_or("—")),
                key_value("Outcome", record.outcome.as_deref().unwrap_or("—")),
                key_value("Timestamp", &format!("{} ms", record.timestamp_ms)),
                key_value("Event", &record.event_id),
            ]
        },
    );
    frame.render_widget(
        Paragraph::new(details)
            .block(panel("Safe metadata"))
            .wrap(Wrap { trim: true }),
        chunks[1],
    );
    frame.render_widget(
        Paragraph::new("j/k move   PageUp/PageDown   Enter/Esc close")
            .style(Style::default().fg(MUTED)),
        chunks[2],
    );
}

fn draw_audit_result(frame: &mut Frame, area: Rect, verification: &jig_vault::AuditVerification) {
    frame.render_widget(Clear, area);
    let state = if verification.torn_tail_bytes == 0 {
        Span::styled(
            "Verified",
            Style::default().fg(GOOD).add_modifier(Modifier::BOLD),
        )
    } else {
        Span::styled(
            "Verified prefix with torn tail",
            Style::default().fg(WARN).add_modifier(Modifier::BOLD),
        )
    };
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(state),
            Line::from(""),
            key_value(
                "Authenticated events",
                &verification.event_count.to_string(),
            ),
            key_value(
                "Ignored torn-tail bytes",
                &verification.torn_tail_bytes.to_string(),
            ),
            Line::from(""),
            Line::from(Span::styled(
                "Raw audit details and MAC material are not shown in the TUI.",
                Style::default().fg(MUTED),
            )),
            Line::from("Enter or Esc closes this result."),
        ])
        .block(panel("Audit verification"))
        .wrap(Wrap { trim: true }),
        area,
    );
}

fn toggle_line(label: &str, enabled: bool, focused: bool) -> Line<'static> {
    form_value_line(label, if enabled { "yes" } else { "no" }, focused, false)
}

fn append_status(lines: &mut Vec<Line<'static>>, app: &App) {
    if let Some(status) = &app.status {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            status.text.clone(),
            Style::default().fg(match status.kind {
                StatusKind::Info => GOOD,
                StatusKind::Error => BAD,
            }),
        )));
    }
}

fn form_value_line(label: &str, value: &str, focused: bool, protected: bool) -> Line<'static> {
    let marker = if focused { "›" } else { " " };
    let style = if focused {
        Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)
    } else {
        Style::default()
    };
    let value = if value.is_empty() && !protected {
        "(empty)".to_owned()
    } else {
        sanitize_text(value)
    };
    Line::from(vec![
        Span::styled(format!("{marker} {label}: "), style),
        Span::styled(value, style),
    ])
}

fn list<'a>(items: Vec<ListItem<'a>>, title: &'a str) -> List<'a> {
    List::new(items)
        .block(panel(title))
        .highlight_style(
            Style::default()
                .bg(Color::Blue)
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("› ")
}

fn item_count(app: &App, identity: &ItemIdentity) -> usize {
    let Some(snapshot) = &app.snapshot else {
        return 0;
    };
    match identity {
        ItemIdentity::Canonical(item) => snapshot
            .fields
            .iter()
            .filter(|field| field.reference.item() == item)
            .count(),
        ItemIdentity::Legacy => snapshot.legacy_secrets.len(),
    }
}

fn item_breadcrumb(app: &App) -> String {
    let legacy_count = app
        .snapshot
        .as_ref()
        .map_or(0, |snapshot| snapshot.legacy_secrets.len());
    app.selected_item
        .as_ref()
        .map_or_else(|| "Items".to_owned(), |item| item.label(legacy_count))
}

fn entry_breadcrumb(app: &App) -> String {
    app.selected_entry
        .as_ref()
        .map_or_else(|| "Fields".to_owned(), EntryIdentity::label)
}

const fn focus_label(focus: Focus) -> &'static str {
    match focus {
        Focus::Items => "Items",
        Focus::Fields => "Fields",
        Focus::Details => "Details",
    }
}

fn key_value(key: &str, value: &str) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!("{key}: "), Style::default().fg(MUTED)),
        Span::raw(sanitize_text(value)),
    ])
}

fn focus_style(focused: bool) -> Style {
    if focused {
        Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(MUTED)
    }
}

fn draw_status(frame: &mut Frame, area: Rect, app: &App) {
    let Some(status) = &app.status else {
        return;
    };
    let color = match status.kind {
        StatusKind::Info => GOOD,
        StatusKind::Error => BAD,
    };
    let status_area = Rect::new(
        area.x.saturating_add(1),
        area.bottom().saturating_sub(2),
        area.width.saturating_sub(2),
        1,
    );
    frame.render_widget(
        Paragraph::new(Span::styled(
            status.text.clone(),
            Style::default().fg(color),
        ))
        .alignment(Alignment::Center),
        status_area,
    );
}

fn centered_rect(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(area);
    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(vertical[1])[1]
}

fn panel(title: &str) -> Block<'_> {
    Block::default().borders(Borders::ALL).title(title)
}
