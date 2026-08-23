use jig_tui::sanitize_text;
use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap},
};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::{
    ImportFieldChange, VaultHomeState,
    browse::BrowseEntryKind,
    commands::{CommandAvailability, CommandPalette, CommandSafety, UiCommand},
    line_editor::LineEditor,
    model::{
        ActivityView, App, ConvertFocus, DeleteConfirmation, EntryIdentity, FieldWriteFocus,
        FieldWriteIntent, Focus, ImportPreviewState, InitializeFocus, ItemIdentity,
        LegacyWriteFocus, ManagementForm, MutationConfirmation, MutationConfirmationKind,
        PeekConfirmation, RenameFieldFocus, Screen, StatusKind, kind_label,
    },
    quick_access::{QuickAccess, QuickAccessTarget},
    tools::{BackupFocus, ExportFocus, ImportFocus, PassphraseFocus, RestoreFocus, ToolForm},
    viewport::{ScreenLayout, ViewportSize, ratatui_viewport, screen_layout},
};

const ACCENT: Color = Color::Cyan;
const MUTED: Color = Color::DarkGray;
const GOOD: Color = Color::Green;
const WARN: Color = Color::Yellow;
const BAD: Color = Color::Red;
const WIDE_WIDTH: u16 = 104;

pub(crate) fn draw(frame: &mut Frame, app: &App) {
    let (area, viewport) = ratatui_viewport(frame.area());
    let layout = screen_layout(&app.screen);
    if !viewport.supports(layout) {
        draw_resize_required(frame, area, viewport, layout);
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
        | Screen::ConfirmMutation(_)
        | Screen::ConfirmDelete(_)
        | Screen::Commands(_)
        | Screen::QuickAccess(_)
        | Screen::ToolForm(_)
        | Screen::ImportPreview(_)
        | Screen::Activity(_)
        | Screen::AuditResult(_)
        | Screen::ConfirmPeek(_) => {
            if app.snapshot().is_some() {
                draw_browser(frame, area, app);
            } else {
                draw_missing(frame, area, app);
            }
            match &app.screen {
                Screen::Help => draw_help(frame, centered_rect(82, 82, area), app),
                Screen::ConfirmMigration => {
                    draw_migration_confirmation(frame, centered_rect(72, 56, area));
                }
                Screen::Form(form) => {
                    draw_management_form(frame, centered_rect(78, 68, area), app, form);
                }
                Screen::ConfirmMutation(confirmation) => {
                    draw_mutation_confirmation(
                        frame,
                        centered_rect(86, 76, area),
                        app,
                        confirmation,
                    );
                }
                Screen::ConfirmDelete(confirmation) => {
                    draw_delete_confirmation(frame, centered_rect(86, 76, area), app, confirmation);
                }
                Screen::Commands(palette) => {
                    draw_command_palette(frame, centered_rect(84, 76, area), app, palette)
                }
                Screen::QuickAccess(access) => {
                    draw_quick_access(frame, centered_rect(92, 84, area), app, access)
                }
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
                Screen::ConfirmPeek(confirmation) => {
                    draw_peek_confirmation(frame, centered_rect(86, 82, area), app, confirmation);
                }
                _ => {}
            }
        }
    }
}

fn draw_resize_required(
    frame: &mut Frame,
    area: Rect,
    viewport: ViewportSize,
    layout: ScreenLayout,
) {
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(Span::styled(
                format!("{} needs more room", layout.label()),
                Style::default().fg(WARN).add_modifier(Modifier::BOLD),
            )),
            Line::from(format!(
                "Current {}x{} · required: {}x{}",
                viewport.width(),
                viewport.height(),
                layout.width(),
                layout.height(),
            )),
            Line::from("Inputs are paused; resize to continue."),
            Line::from("q exits · Esc closes when available"),
        ])
        .alignment(Alignment::Center)
        .block(panel("Jig Vault · resize required"))
        .wrap(Wrap { trim: true }),
        area,
    );
}

fn draw_missing(frame: &mut Frame, area: Rect, app: &App) {
    let (state_label, headline, restore_message, footer) = match app.descriptor.home_state {
        VaultHomeState::Absent => (
            "absent",
            "No vault exists at this fixed scope.",
            "Restore from an encrypted backup is available from this screen.",
            "i initialize   : actions/restore   q quit",
        ),
        VaultHomeState::Uninitialized => (
            "uninitialized",
            "This vault home exists, but it has no initialized vault state.",
            "Restore is unavailable because it never overwrites an existing vault home.",
            "i initialize   : actions   q quit",
        ),
        VaultHomeState::Initialized => unreachable!("initialized vaults do not use missing UI"),
    };
    draw_public_header(frame, area, app, state_label, WARN);
    let inner = centered_rect(72, 46, area);
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(Span::styled(
                headline,
                Style::default().fg(WARN).add_modifier(Modifier::BOLD),
            )),
            Line::from(""),
            Line::from("Press i to initialize a new encrypted vault."),
            Line::from(restore_message),
            Line::from(""),
            Line::from(footer),
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
    let inner = centered_box(72, 8, area);
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
                "Protected bullets and byte count only.",
                Style::default().fg(MUTED),
            )),
            Line::from("Enter unlock   Ctrl-U clear   Esc quit"),
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
                "Use 12+ bytes; held only while this vault is unlocked.",
                Style::default().fg(MUTED),
            )),
            Line::from("Tab field · Enter create · Esc cancel · Ctrl-U clear"),
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
        centered_rect(72, 50, area),
    );
}

fn draw_browser(frame: &mut Frame, area: Rect, app: &App) {
    let filter_visible = app.searching || !app.filter().is_empty();
    let retained_filter_and_status = !app.searching && filter_visible && app.status.is_some();
    let footer_height = if filter_visible || app.status.is_some() {
        5 + u16::from(retained_filter_and_status)
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
    let snapshot = app.snapshot().expect("browser always has a snapshot");
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
    let rows = app.visible_item_rows();
    let title = if app.focus == Focus::Items {
        "Items •"
    } else {
        "Items"
    };
    if rows.is_empty() {
        let message = if app.filter().is_empty() {
            vec![
                Line::from("No items yet."),
                Line::from("Press I to create an item with its first field."),
            ]
        } else {
            vec![Line::from("No items match the current filter.")]
        };
        frame.render_widget(
            Paragraph::new(message)
                .block(panel(title))
                .wrap(Wrap { trim: true }),
            area,
        );
        return;
    }
    let items = rows
        .iter()
        .map(|(identity, count)| {
            let label = identity.label(*count);
            ListItem::new(format!("{label}  ({count})"))
        })
        .collect::<Vec<_>>();
    let selected = app
        .selected_item
        .as_ref()
        .and_then(|identity| rows.iter().position(|(row, _)| row == identity));
    let mut state = ListState::default().with_selected(selected);
    frame.render_stateful_widget(list(items, title), area, &mut state);
}

fn draw_entries(frame: &mut Frame, area: Rect, app: &App) {
    let rows = app.visible_entry_rows();
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
    if rows.is_empty() {
        let message = if !app.filter().is_empty() {
            "No fields match the current filter."
        } else if app.selected_item.is_none() {
            "Create an item with its first field to get started."
        } else {
            "No fields are available for this item."
        };
        frame.render_widget(
            Paragraph::new(message)
                .block(panel(&title))
                .wrap(Wrap { trim: true }),
            area,
        );
        return;
    }
    let items = rows
        .iter()
        .map(|(identity, kind)| {
            let suffix = match kind {
                BrowseEntryKind::Field(kind) => format!("  [{}]", kind_label(*kind)),
                BrowseEntryKind::Legacy => "  [legacy]".to_owned(),
            };
            ListItem::new(format!("{}{}", identity.label(), suffix))
        })
        .collect::<Vec<_>>();
    let selected = app
        .selected_entry
        .as_ref()
        .and_then(|identity| rows.iter().position(|(row, _)| row == identity));
    let mut state = ListState::default().with_selected(selected);
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
        "Type to filter  ←/→ cursor  Ctrl-←/→ words  Ctrl-W delete word  Enter/Esc finish"
            .to_owned()
    } else if app
        .snapshot()
        .is_some_and(|snapshot| snapshot.format_version == 1)
    {
        format!(
            "READ-ONLY v1  / filter  Ctrl-P find  Tab/h/l focus  j/k move  {}  ? help  q quit",
            common_action_hints(app)
        )
    } else {
        format!(
            "/ filter  Ctrl-P find  Tab/h/l focus  j/k move  {}  ? help  q quit",
            common_action_hints(app)
        )
    };
    let mut lines = vec![Line::from(format!("{breadcrumb}{controls}"))];
    if !app.searching {
        lines.push(Line::from(context_action_hints(app, frame.area().width)));
    }
    if app.searching || !app.filter().is_empty() {
        let mut spans = vec![Span::styled("/", Style::default().fg(ACCENT))];
        spans.extend(editor_spans(
            app.filter(),
            app.searching,
            usize::from(area.width.saturating_sub(3)),
            Style::default().fg(ACCENT),
        ));
        lines.push(Line::from(spans));
    }
    if let Some(status) = &app.status {
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

fn common_action_hints(app: &App) -> String {
    [UiCommand::MigrateToV2, UiCommand::Refresh, UiCommand::Lock]
        .into_iter()
        .filter(|command| command.visible_in_state(app))
        .filter(|command| command.availability(app).is_enabled())
        .map(UiCommand::hint)
        .collect::<Vec<_>>()
        .join("  ")
}

fn context_action_hints(app: &App, width: u16) -> String {
    let limit = if width < WIDE_WIDTH { 3 } else { 7 };
    let mut hints = UiCommand::ALL
        .into_iter()
        .filter(|command| command.relevant_to_context(app))
        .filter(|command| command.availability(app).is_enabled())
        .filter(|command| command.binding().is_some())
        .take(limit)
        .map(UiCommand::hint)
        .collect::<Vec<_>>();
    hints.push("Enter actions".to_owned());
    hints.push(": all".to_owned());
    hints.join("  ")
}

fn command_help_lines(app: &App) -> Vec<Line<'static>> {
    let commands = UiCommand::ALL
        .into_iter()
        .filter(|command| command.visible_in_state(app))
        .filter_map(|command| {
            command
                .binding()
                .map(|binding| format!("{}  {}", binding.label, command.label()))
        })
        .collect::<Vec<_>>();
    commands
        .chunks(3)
        .map(|chunk| {
            Line::from(
                chunk
                    .iter()
                    .map(|entry| format!("{entry:<24}"))
                    .collect::<String>()
                    .trim_end()
                    .to_owned(),
            )
        })
        .collect()
}

fn draw_help(frame: &mut Frame, area: Rect, app: &App) {
    frame.render_widget(Clear, area);
    let mut lines = vec![
        Line::from(Span::styled(
            "Navigation",
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
        )),
        Line::from("j/k or ↑/↓  move selection"),
        Line::from("h/l or Tab   change focused pane"),
        Line::from("Home/End     first or last row"),
        Line::from("/            filter item, field, reference, or legacy metadata"),
        Line::from("Ctrl-P       Quick Access for items, fields, and legacy metadata"),
        Line::from("Enter        actions for the current selection"),
        Line::from(":            search every available vault action"),
        Line::from("Text inputs  ←/→ cursor · Home/End · Ctrl-←/→ words · Ctrl-W delete word"),
        Line::from("?            close this help"),
        Line::from("q            quit"),
        Line::from("             auto-locks after five minutes without terminal input"),
        Line::from(""),
        Line::from(Span::styled(
            "Direct actions",
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
        )),
    ];
    lines.extend(command_help_lines(app));
    lines.extend([
        Line::from(""),
        Line::from(Span::styled(
            "Concealed values remain hidden. Text values are visible while typed.",
            Style::default().fg(MUTED),
        )),
        Line::from(""),
        Line::from("Esc or ? closes help."),
    ]);
    frame.render_widget(
        Paragraph::new(lines)
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
            intent,
            item,
            field,
            kind,
            value,
            value_file,
            focus,
        } => {
            let action = intent.title();
            let mut lines = vec![
                editor_value_line("Item", item, *focus == FieldWriteFocus::Item, area.width),
                editor_value_line("Field", field, *focus == FieldWriteFocus::Field, area.width),
                form_value_line(
                    "Kind",
                    kind_label(*kind),
                    *focus == FieldWriteFocus::Kind,
                    false,
                ),
                field_value_line(*kind, value, *focus == FieldWriteFocus::Value, area.width),
                editor_value_line(
                    "Value file",
                    value_file,
                    *focus == FieldWriteFocus::File,
                    area.width,
                ),
                Line::from(""),
            ];
            if *intent == FieldWriteIntent::CreateItem {
                lines.push(Line::from(Span::styled(
                    "Items are created atomically with their first field.",
                    Style::default().fg(MUTED),
                )));
            } else if *intent == FieldWriteIntent::ReplaceValue {
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
                editor_value_line("Name", name, *focus == LegacyWriteFocus::Name, area.width),
                form_value_line(
                    "Value",
                    &value.render_label(),
                    *focus == LegacyWriteFocus::Value,
                    true,
                ),
                editor_value_line(
                    "Value file",
                    value_file,
                    *focus == LegacyWriteFocus::File,
                    area.width,
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
                editor_value_line(
                    "Destination item",
                    destination_item,
                    *focus == RenameFieldFocus::Item,
                    area.width,
                ),
                editor_value_line(
                    "Destination field",
                    destination_field,
                    *focus == RenameFieldFocus::Field,
                    area.width,
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
                editor_value_line("Destination item", destination, true, area.width),
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
                editor_value_line("Item", item, *focus == ConvertFocus::Item, area.width),
                editor_value_line("Field", field, *focus == ConvertFocus::Field, area.width),
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
        "Tab field · Space toggle · Enter save · Esc cancel",
    ));
    lines.push(Line::from(
        "Arrows move · Ctrl-arrows word · Ctrl-W delete · Ctrl-U clear",
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

fn draw_mutation_confirmation(
    frame: &mut Frame,
    area: Rect,
    app: &App,
    confirmation: &MutationConfirmation,
) {
    frame.render_widget(Clear, area);
    let (title, mut lines) = match &confirmation.kind {
        MutationConfirmationKind::EmptyTextReplacement {
            reference,
            redaction_downgrade,
        } => ("Confirm empty text replacement", {
            let mut lines = vec![
                Line::from(Span::styled(
                    "Replace this encrypted field with an empty text value?",
                    Style::default().fg(WARN).add_modifier(Modifier::BOLD),
                )),
                Line::from(""),
                key_value_clipped("Reference", &reference.to_string(), area.width),
                Line::from(""),
                Line::from("This clears the current value; the existing value was not loaded."),
            ];
            if *redaction_downgrade {
                lines.push(Line::from(Span::styled(
                    "It also stops treating the field as an output-redaction needle.",
                    Style::default().fg(BAD),
                )));
            }
            lines.extend([
                Line::from(""),
                Line::from("Type CLEAR exactly, then press Enter."),
            ]);
            lines
        }),
        MutationConfirmationKind::RedactionDowngrade { reference } => (
            "Confirm redaction downgrade",
            vec![
                Line::from(Span::styled(
                    "Change this field from concealed to text?",
                    Style::default().fg(WARN).add_modifier(Modifier::BOLD),
                )),
                Line::from(""),
                key_value_clipped("Reference", &reference.to_string(), area.width),
                Line::from("The value remains encrypted at rest."),
                Line::from(Span::styled(
                    "Text fields are not output-redaction needles and may appear unmasked in command output.",
                    Style::default().fg(BAD),
                )),
                Line::from(""),
                Line::from("Type TEXT exactly, then press Enter."),
            ],
        ),
    };
    lines.extend([
        editor_value_line("Confirmation", &confirmation.input, true, area.width),
        Line::from(""),
        Line::from("Enter confirm   Esc cancel   Ctrl-U clear"),
    ]);
    append_status(&mut lines, app);
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
            format!(
                "Permanently remove {}?",
                confirmation.target.display_label()
            ),
            Style::default().fg(BAD).add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from("There is no trash or undo."),
        Line::from(format!("Type exactly: {}", sanitize_text(&required))),
        editor_value_line("Confirmation", &confirmation.input, true, area.width),
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

fn draw_peek_confirmation(
    frame: &mut Frame,
    area: Rect,
    app: &App,
    confirmation: &PeekConfirmation,
) {
    frame.render_widget(Clear, area);
    let mut lines = vec![
        Line::from(Span::styled(
            "Reveal this field in the alternate terminal screen?",
            Style::default().fg(BAD).add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        key_value_clipped("Reference", &confirmation.reference.to_string(), area.width),
        Line::from("The escaped value is written outside Ratatui."),
        Line::from(Span::styled(
            "Scrollback, multiplexers, and recordings may capture it.",
            Style::default().fg(WARN),
        )),
        Line::from("It clears after one key or ten seconds."),
        Line::from(""),
        Line::from("Type PEEK exactly to accept this disclosure:"),
        editor_value_line("Confirmation", &confirmation.input, true, area.width),
        Line::from(""),
        Line::from("Enter reveal   Esc cancel   Ctrl-U clear"),
    ];
    append_status(&mut lines, app);
    frame.render_widget(
        Paragraph::new(lines)
            .block(panel("Controlled terminal preview"))
            .wrap(Wrap { trim: true }),
        area,
    );
}

fn draw_command_palette(frame: &mut Frame, area: Rect, app: &App, palette: &CommandPalette) {
    frame.render_widget(Clear, area);
    let footer_height = if app.status.is_some() { 6 } else { 5 };
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(4), Constraint::Length(footer_height)])
        .split(area);
    let visible = palette.visible_entries();
    let mut items = visible
        .iter()
        .map(|entry| {
            let binding = entry.command.binding().map_or_else(
                || "    ".to_owned(),
                |binding| format!("[{}] ", binding.label),
            );
            let style = match entry.availability {
                CommandAvailability::Disabled(_) => Style::default().fg(MUTED),
                CommandAvailability::Enabled => match entry.command.safety() {
                    CommandSafety::Ordinary => Style::default(),
                    CommandSafety::Disclosure => Style::default().fg(WARN),
                    CommandSafety::Destructive => Style::default().fg(BAD),
                },
            };
            let mut spans = vec![
                Span::styled(binding, Style::default().fg(ACCENT)),
                Span::styled(entry.command.label(), style),
                Span::styled(
                    format!("  {}", entry.command.category()),
                    Style::default().fg(MUTED),
                ),
            ];
            if let CommandAvailability::Disabled(reason) = entry.availability {
                spans.push(Span::styled(
                    format!("  — {reason}"),
                    Style::default().fg(MUTED),
                ));
            }
            ListItem::new(Line::from(spans))
        })
        .collect::<Vec<_>>();
    if items.is_empty() {
        items.push(ListItem::new(Line::from(Span::styled(
            "No matching actions.",
            Style::default().fg(MUTED),
        ))));
    }
    let selected = (!visible.is_empty()).then_some(palette.selected);
    let mut state = ListState::default().with_selected(selected);
    frame.render_stateful_widget(list(items, palette.scope.title()), chunks[0], &mut state);
    let mut filter = vec![Span::styled("Filter: ", Style::default().fg(ACCENT))];
    filter.extend(editor_spans(
        &palette.filter,
        true,
        usize::from(chunks[1].width.saturating_sub(10)),
        Style::default().fg(ACCENT),
    ));
    let mut lines = vec![
        Line::from(filter),
        Line::from("↑/↓ choose   ←/→ cursor   Ctrl-←/→ words")
            .style(Style::default().fg(MUTED)),
        Line::from(
            "Enter run/open   Backspace/Delete edit   Ctrl-W delete word   Ctrl-U clear   Esc close",
        )
            .style(Style::default().fg(MUTED)),
    ];
    if let Some(status) = &app.status {
        lines.push(Line::from(Span::styled(
            status.text.clone(),
            Style::default().fg(match status.kind {
                StatusKind::Info => GOOD,
                StatusKind::Error => BAD,
            }),
        )));
    }
    frame.render_widget(Paragraph::new(lines).block(panel("Command")), chunks[1]);
}

fn draw_quick_access(frame: &mut Frame, area: Rect, app: &App, access: &QuickAccess) {
    frame.render_widget(Clear, area);
    let footer_height = if app.status.is_some() { 6 } else { 5 };
    let outer = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(4), Constraint::Length(footer_height)])
        .split(area);
    if outer[0].width >= 70 && outer[0].height >= 10 {
        let body = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(56), Constraint::Percentage(44)])
            .split(outer[0]);
        draw_quick_access_list(frame, body[0], access);
        draw_quick_access_preview(frame, body[1], access.selected_target());
    } else {
        draw_quick_access_list(frame, outer[0], access);
    }

    let mut query = vec![Span::styled("Find: ", Style::default().fg(ACCENT))];
    query.extend(editor_spans(
        access.query(),
        true,
        usize::from(outer[1].width.saturating_sub(8)),
        Style::default().fg(ACCENT),
    ));
    let mut lines = vec![
        Line::from(query),
        Line::from("↑/↓ choose   PgUp/PgDn move   ←/→ cursor   Ctrl-←/→ words")
            .style(Style::default().fg(MUTED)),
        Line::from(
            "Enter actions   Backspace/Delete edit   Ctrl-W word   Ctrl-U clear   Esc close",
        )
        .style(Style::default().fg(MUTED)),
    ];
    if let Some(status) = &app.status {
        lines.push(Line::from(Span::styled(
            status.text.clone(),
            Style::default().fg(match status.kind {
                StatusKind::Info => GOOD,
                StatusKind::Error => BAD,
            }),
        )));
    }
    frame.render_widget(
        Paragraph::new(lines).block(panel("Metadata-only search")),
        outer[1],
    );
}

fn draw_quick_access_list(frame: &mut Frame, area: Rect, access: &QuickAccess) {
    let mut items = access
        .visible_targets()
        .map(|target| {
            ListItem::new(Line::from(vec![
                Span::styled(
                    format!("{:<7} ", target.badge()),
                    Style::default().fg(MUTED),
                ),
                Span::raw(sanitize_text(target.title())),
            ]))
        })
        .collect::<Vec<_>>();
    if items.is_empty() {
        items.push(ListItem::new(Line::from(Span::styled(
            "No metadata matches.",
            Style::default().fg(MUTED),
        ))));
    }
    let mut state = ListState::default()
        .with_offset(access.list_offset_for_viewport(area.height))
        .with_selected(access.selected_row());
    frame.render_stateful_widget(
        list(items, &format!("Quick Access · {}", access.visible_len())),
        area,
        &mut state,
    );
    access.set_list_offset(state.offset());
}

fn draw_quick_access_preview(frame: &mut Frame, area: Rect, target: Option<&QuickAccessTarget>) {
    let lines = match target {
        Some(QuickAccessTarget::Item { item, field_count }) => vec![
            key_value("Type", "Item"),
            key_value("Reference", &format!("jig://{item}")),
            key_value("Fields", &field_count.to_string()),
            Line::from(""),
            Line::from(Span::styled(
                "Quick Access uses metadata only.",
                Style::default().fg(MUTED),
            )),
        ],
        Some(QuickAccessTarget::Field { reference, kind }) => vec![
            key_value("Type", "Field"),
            key_value("Reference", &reference.to_string()),
            key_value("Item", reference.item()),
            key_value("Field", reference.field()),
            key_value("Kind", kind_label(*kind)),
            Line::from(""),
            Line::from(Span::styled(
                "The encrypted value is never loaded.",
                Style::default().fg(MUTED),
            )),
        ],
        Some(QuickAccessTarget::LegacyGroup { entry_count }) => vec![
            key_value("Type", "Legacy group"),
            key_value("Entries", &entry_count.to_string()),
            Line::from(""),
            Line::from(Span::styled(
                "Legacy values remain encrypted and hidden.",
                Style::default().fg(MUTED),
            )),
        ],
        Some(QuickAccessTarget::LegacyEntry { name }) => vec![
            key_value("Type", "Legacy entry"),
            key_value("Name", name),
            Line::from(""),
            Line::from(Span::styled(
                "Convert this entry before field-only actions.",
                Style::default().fg(WARN),
            )),
            Line::from(Span::styled(
                "The encrypted value is never loaded.",
                Style::default().fg(MUTED),
            )),
        ],
        None => vec![Line::from("No result selected.")],
    };
    frame.render_widget(
        Paragraph::new(lines)
            .block(panel("Selected metadata"))
            .wrap(Wrap { trim: false }),
        area,
    );
}

fn draw_tool_form(frame: &mut Frame, area: Rect, app: &App, form: &ToolForm) {
    frame.render_widget(Clear, area);
    let (title, mut lines) = match form {
        ToolForm::ExportField {
            reference,
            output,
            overwrite,
            focus,
        } => (
            "Export field to private file",
            vec![
                key_value_clipped("Reference", &reference.to_string(), area.width),
                editor_value_line(
                    "Output file",
                    output,
                    *focus == ExportFocus::Output,
                    area.width,
                ),
                toggle_line(
                    "Overwrite regular file",
                    *overwrite,
                    *focus == ExportFocus::Overwrite,
                ),
                Line::from(""),
                Line::from(Span::styled(
                    "Exact bytes are written directly to a hardened owner-only file.",
                    Style::default().fg(WARN),
                )),
            ],
        ),
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
                editor_value_line(
                    "Source .env",
                    env_file,
                    *focus == ImportFocus::EnvFile,
                    area.width,
                ),
                editor_value_line("Item", item, *focus == ImportFocus::Item, area.width),
                editor_value_line(
                    "Generated .env",
                    out_env,
                    *focus == ImportFocus::OutEnv,
                    area.width,
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
                editor_value_line(
                    "Backup output",
                    output,
                    *focus == BackupFocus::Output,
                    area.width,
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
                editor_value_line(
                    "Backup input",
                    input,
                    *focus == RestoreFocus::Input,
                    area.width,
                ),
                form_value_line(
                    "Backup vault passphrase",
                    &passphrase.render_label(),
                    *focus == RestoreFocus::Passphrase,
                    true,
                ),
                editor_value_line(
                    "Type RESTORE",
                    confirmation,
                    *focus == RestoreFocus::Confirmation,
                    area.width,
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
        "Tab field · Space toggle · Enter continue · Esc cancel",
    ));
    lines.push(Line::from(
        "Arrows move · Ctrl-arrows word · Ctrl-W delete · Ctrl-U clear",
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
    let has_redaction_downgrade = preview.has_redaction_downgrade();
    let content_rows = usize::from(area.height.saturating_sub(2));
    let fixed_rows =
        10 + usize::from(has_redaction_downgrade) * 2 + usize::from(app.status.is_some()) * 2;
    let row_budget = content_rows.saturating_sub(fixed_rows);
    let visible_rows = if preview.rows.len() > row_budget {
        row_budget.saturating_sub(1)
    } else {
        preview.rows.len()
    };
    let mut lines = vec![
        key_value_clipped("Source", &preview.env_file.to_string_lossy(), area.width),
        key_value_clipped(
            "Item",
            &format!("jig://{}", preview.item.as_str()),
            area.width,
        ),
        key_value_clipped(
            "Destination",
            &preview.out_env.to_string_lossy(),
            area.width,
        ),
        toggle_line("Replace fields (r)", preview.replace, false),
        toggle_line("Overwrite file (o)", preview.overwrite, false),
        Line::from(""),
    ];
    for row in preview.rows.iter().take(visible_rows) {
        let change = match row.change {
            ImportFieldChange::Create { kind } => {
                format!("[{}]  create", kind_label(kind))
            }
            ImportFieldChange::Replace {
                previous_kind,
                kind,
            } => format!(
                "[{} → {}]  replace",
                kind_label(previous_kind),
                kind_label(kind)
            ),
        };
        lines.push(clipped_line(
            &format!("{} → {}  {change}", row.variable, row.reference),
            area.width,
        ));
    }
    if row_budget > 0 && preview.rows.len() > visible_rows {
        lines.push(Line::from(Span::styled(
            format!("… {} additional fields", preview.rows.len() - visible_rows),
            Style::default().fg(MUTED),
        )));
    }
    lines.push(Line::from(""));
    if has_redaction_downgrade {
        lines.push(Line::from(Span::styled(
            "Concealed → text: output redaction will be disabled.",
            Style::default().fg(BAD).add_modifier(Modifier::BOLD),
        )));
        lines.push(Line::from(Span::styled(
            "The affected value may appear unmasked in command output.",
            Style::default().fg(WARN),
        )));
    }
    if preview.is_dry_run() {
        lines.push(Line::from(Span::styled(
            "Dry run: no 1Password values were resolved and nothing changed.",
            Style::default().fg(GOOD),
        )));
        lines.push(Line::from("Enter finish dry run   Esc close"));
    } else {
        lines.push(Line::from(Span::styled(
            "Commit resolves `op`, updates the vault, then writes the private .env.",
            Style::default().fg(WARN),
        )));
        lines.push(editor_value_line(
            &format!("Type {}", state.required_confirmation()),
            &state.confirmation,
            true,
            area.width,
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
            Constraint::Length(2),
        ])
        .split(area);
    let items = view
        .activity
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
    let selected = (!view.activity.records.is_empty()).then_some(view.selected);
    let mut state = ListState::default().with_selected(selected);
    frame.render_stateful_widget(
        list(items, "Verified activity (newest first)"),
        chunks[0],
        &mut state,
    );
    let details = view.activity.records.get(view.selected).map_or_else(
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
    let audit = &view.activity.audit;
    let audit_status = if audit.torn_tail_bytes == 0 {
        Line::from(format!(
            "Audit verified: {} authenticated events",
            audit.event_count
        ))
        .style(Style::default().fg(GOOD))
    } else {
        Line::from(format!(
            "Warning: verified prefix; {} unauthenticated trailing bytes ignored",
            audit.torn_tail_bytes
        ))
        .style(Style::default().fg(WARN))
    };
    frame.render_widget(
        Paragraph::new(vec![
            audit_status,
            Line::from("j/k move   PageUp/PageDown   Enter/Esc close")
                .style(Style::default().fg(MUTED)),
        ]),
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

fn field_value_line(
    kind: jig_vault::FieldKind,
    value: &crate::secret_input::SecretInput,
    focused: bool,
    area_width: u16,
) -> Line<'static> {
    if kind == jig_vault::FieldKind::Text
        && let Some(text) = value.visible_text()
    {
        let marker = if focused { "›" } else { " " };
        let style = if focused {
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)
        } else {
            Style::default()
        };
        let prefix = format!("{marker} Value: ");
        let available = content_width(area_width)
            .saturating_sub(prefix.width())
            .max(1);
        let text = if text.is_empty() {
            "(empty)".to_owned()
        } else {
            fit_text_end(text, available)
        };
        return Line::from(vec![Span::styled(prefix, style), Span::styled(text, style)]);
    }
    form_value_line("Value", &value.render_label(), focused, true)
}

fn editor_value_line(
    label: &str,
    editor: &LineEditor,
    focused: bool,
    area_width: u16,
) -> Line<'static> {
    let marker = if focused { "›" } else { " " };
    let style = if focused {
        Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)
    } else {
        Style::default()
    };
    let prefix = format!("{marker} {label}: ");
    let prefix_width = Line::from(prefix.as_str()).width();
    let available = usize::from(area_width)
        .saturating_sub(prefix_width)
        .saturating_sub(2)
        .max(1);
    let mut spans = vec![Span::styled(prefix, style)];
    spans.extend(editor_spans(editor, focused, available, style));
    Line::from(spans)
}

fn editor_spans(
    editor: &LineEditor,
    focused: bool,
    max_width: usize,
    style: Style,
) -> Vec<Span<'static>> {
    if !focused {
        let value = if editor.is_empty() {
            "(empty)".to_owned()
        } else {
            sanitize_text(editor.as_str())
        };
        return vec![Span::styled(value, style)];
    }

    let window = editor.window(max_width.max(1));
    let mut spans = Vec::with_capacity(5);
    if window.clipped_left {
        spans.push(Span::styled("‹", Style::default().fg(MUTED)));
    }
    spans.push(Span::styled(sanitize_text(&window.before), style));
    spans.push(Span::styled(
        "▌",
        Style::default()
            .fg(Color::Black)
            .bg(ACCENT)
            .add_modifier(Modifier::BOLD),
    ));
    spans.push(Span::styled(sanitize_text(&window.after), style));
    if window.clipped_right {
        spans.push(Span::styled("›", Style::default().fg(MUTED)));
    }
    spans
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

fn item_breadcrumb(app: &App) -> String {
    app.selected_item.as_ref().map_or_else(
        || "Items".to_owned(),
        |item| item.label(app.item_entry_count(item)),
    )
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

fn key_value_clipped(key: &str, value: &str, area_width: u16) -> Line<'static> {
    let prefix = format!("{key}: ");
    let available = content_width(area_width).saturating_sub(prefix.width());
    Line::from(vec![
        Span::styled(prefix, Style::default().fg(MUTED)),
        Span::raw(fit_text(value, available)),
    ])
}

fn clipped_line(value: &str, area_width: u16) -> Line<'static> {
    Line::from(fit_text(value, content_width(area_width)))
}

fn content_width(area_width: u16) -> usize {
    usize::from(area_width.saturating_sub(2))
}

fn fit_text(value: &str, max_width: usize) -> String {
    let value = sanitize_text(value);
    if value.width() <= max_width {
        return value;
    }
    if max_width == 0 {
        return String::new();
    }
    if max_width == 1 {
        return "…".to_owned();
    }

    let retained_width = max_width - 1;
    let mut output = String::new();
    let mut width = 0_usize;
    for character in value.chars() {
        let character_width = character.width().unwrap_or(0);
        if width.saturating_add(character_width) > retained_width {
            break;
        }
        output.push(character);
        width += character_width;
    }
    output.push('…');
    output
}

fn fit_text_end(value: &str, max_width: usize) -> String {
    if max_width == 0 {
        return String::new();
    }
    let start = tail_window_start(value, max_width);
    if start == 0 {
        return sanitize_text(value);
    }
    if max_width == 1 {
        return "…".to_owned();
    }

    let start = tail_window_start(value, max_width - 1);
    let mut output = String::from("…");
    output.push_str(&sanitize_text(&value[start..]));
    output
}

fn tail_window_start(value: &str, max_width: usize) -> usize {
    let mut start = value.len();
    let mut width = 0_usize;
    for (index, character) in value.char_indices().rev() {
        let mut encoded = [0_u8; 4];
        let character_width = sanitize_text(character.encode_utf8(&mut encoded))
            .width()
            .max(1);
        if width.saturating_add(character_width) > max_width {
            break;
        }
        width += character_width;
        start = index;
    }
    start
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

fn centered_box(max_width: u16, height: u16, area: Rect) -> Rect {
    let width = max_width.min(area.width.saturating_sub(2));
    let height = height.min(area.height);
    Rect::new(
        area.x.saturating_add(area.width.saturating_sub(width) / 2),
        area.y
            .saturating_add(area.height.saturating_sub(height) / 2),
        width,
        height,
    )
}

fn panel(title: &str) -> Block<'_> {
    Block::default().borders(Borders::ALL).title(title)
}
