use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::{
    VaultAction,
    line_editor::{LineEdit, LineEditor},
    model::{App, EntryIdentity, Focus, ItemIdentity},
    tools::ToolChoice,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum UiCommand {
    CreateItem,
    AddField,
    AddLegacy,
    ReplaceValue,
    ChangeKind,
    RenameSelection,
    ConvertLegacy,
    DeleteSelection,
    ExportField,
    PeekField,
    Refresh,
    MigrateToV2,
    Activity,
    VerifyAudit,
    ImportOnePassword,
    CreateBackup,
    ChangePassphrase,
    RestoreBackup,
    Lock,
}

impl UiCommand {
    pub(crate) const ALL: [Self; 19] = [
        Self::CreateItem,
        Self::AddField,
        Self::AddLegacy,
        Self::ReplaceValue,
        Self::ChangeKind,
        Self::RenameSelection,
        Self::ConvertLegacy,
        Self::DeleteSelection,
        Self::ExportField,
        Self::PeekField,
        Self::Refresh,
        Self::MigrateToV2,
        Self::Activity,
        Self::VerifyAudit,
        Self::ImportOnePassword,
        Self::CreateBackup,
        Self::ChangePassphrase,
        Self::RestoreBackup,
        Self::Lock,
    ];

    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::CreateItem => "Create item + first field",
            Self::AddField => "Add field",
            Self::AddLegacy => "Add explicit legacy entry",
            Self::ReplaceValue => "Replace selected value",
            Self::ChangeKind => "Change selected field kind",
            Self::RenameSelection => "Rename or move selection",
            Self::ConvertLegacy => "Convert legacy entry",
            Self::DeleteSelection => "Delete selection",
            Self::ExportField => "Export field to private file",
            Self::PeekField => "Controlled terminal preview",
            Self::Refresh => "Refresh authenticated metadata",
            Self::MigrateToV2 => "Migrate vault to version 2",
            Self::Activity => "Verified activity",
            Self::VerifyAudit => "Verify audit chain",
            Self::ImportOnePassword => "Import 1Password dotenv",
            Self::CreateBackup => "Create encrypted backup",
            Self::ChangePassphrase => "Change vault passphrase",
            Self::RestoreBackup => "Restore encrypted backup",
            Self::Lock => "Lock vault",
        }
    }

    const fn short_label(self) -> &'static str {
        match self {
            Self::CreateItem => "create item",
            Self::AddField => "add field",
            Self::AddLegacy => "legacy",
            Self::ReplaceValue => "replace",
            Self::ChangeKind => "kind",
            Self::RenameSelection => "rename",
            Self::ConvertLegacy => "convert",
            Self::DeleteSelection => "delete",
            Self::ExportField => "export",
            Self::PeekField => "peek",
            Self::Refresh => "refresh",
            Self::MigrateToV2 => "migrate",
            Self::Activity => "activity",
            Self::VerifyAudit => "audit",
            Self::ImportOnePassword => "1Password import",
            Self::CreateBackup => "backup",
            Self::ChangePassphrase => "passphrase",
            Self::RestoreBackup => "restore",
            Self::Lock => "lock",
        }
    }

    pub(crate) const fn category(self) -> &'static str {
        match self {
            Self::CreateItem
            | Self::AddField
            | Self::AddLegacy
            | Self::ReplaceValue
            | Self::ChangeKind
            | Self::RenameSelection
            | Self::ConvertLegacy
            | Self::DeleteSelection => "Manage",
            Self::ExportField | Self::PeekField => "Reveal",
            Self::Refresh | Self::MigrateToV2 | Self::Activity | Self::VerifyAudit => "Inspect",
            Self::ImportOnePassword
            | Self::CreateBackup
            | Self::ChangePassphrase
            | Self::RestoreBackup
            | Self::Lock => "Lifecycle",
        }
    }

    pub(crate) const fn safety(self) -> CommandSafety {
        match self {
            Self::DeleteSelection | Self::MigrateToV2 | Self::RestoreBackup => {
                CommandSafety::Destructive
            }
            Self::ExportField | Self::PeekField => CommandSafety::Disclosure,
            _ => CommandSafety::Ordinary,
        }
    }

    pub(crate) const fn binding(self) -> Option<CommandBinding> {
        match self {
            Self::CreateItem => Some(CommandBinding::shifted('I', "I")),
            Self::AddField => Some(CommandBinding::plain('a', "a")),
            Self::AddLegacy => Some(CommandBinding::shifted('A', "A")),
            Self::ReplaceValue => Some(CommandBinding::plain('e', "e")),
            Self::ChangeKind => Some(CommandBinding::shifted('K', "K")),
            Self::RenameSelection => Some(CommandBinding::plain('n', "n")),
            Self::ConvertLegacy => Some(CommandBinding::plain('c', "c")),
            Self::DeleteSelection => Some(CommandBinding::shifted('D', "D")),
            Self::ExportField => Some(CommandBinding::plain('x', "x")),
            Self::PeekField => Some(CommandBinding::plain('p', "p")),
            Self::Refresh => Some(CommandBinding::plain('r', "r")),
            Self::MigrateToV2 => Some(CommandBinding::plain('m', "m")),
            Self::Lock => Some(CommandBinding::shifted('L', "L")),
            Self::Activity
            | Self::VerifyAudit
            | Self::ImportOnePassword
            | Self::CreateBackup
            | Self::ChangePassphrase
            | Self::RestoreBackup => None,
        }
    }

    pub(crate) fn from_key(key: KeyEvent) -> Option<Self> {
        Self::ALL.into_iter().find(|command| {
            command
                .binding()
                .is_some_and(|binding| binding.matches(key))
        })
    }

    pub(crate) fn availability(self, app: &App) -> CommandAvailability {
        let format_version = app
            .snapshot
            .as_ref()
            .map(|snapshot| snapshot.format_version);
        let writable = format_version == Some(2);
        match self {
            Self::CreateItem | Self::AddLegacy => {
                if writable {
                    CommandAvailability::Enabled
                } else {
                    CommandAvailability::Disabled("Vault management requires version 2.")
                }
            }
            Self::AddField => {
                if !writable {
                    CommandAvailability::Disabled("Vault management requires version 2.")
                } else if matches!(app.selected_item, Some(ItemIdentity::Canonical(_))) {
                    CommandAvailability::Enabled
                } else {
                    CommandAvailability::Disabled(
                        "Select a canonical item or create a new item first.",
                    )
                }
            }
            Self::ReplaceValue => {
                if !writable {
                    CommandAvailability::Disabled("Vault management requires version 2.")
                } else if app.selected_entry.is_some() {
                    CommandAvailability::Enabled
                } else {
                    CommandAvailability::Disabled("Select a field or legacy entry first.")
                }
            }
            Self::ChangeKind => {
                if !writable {
                    CommandAvailability::Disabled("Vault management requires version 2.")
                } else if matches!(app.selected_entry, Some(EntryIdentity::Field(_))) {
                    CommandAvailability::Enabled
                } else {
                    CommandAvailability::Disabled("Select a canonical field first.")
                }
            }
            Self::RenameSelection => {
                if !writable {
                    CommandAvailability::Disabled("Vault management requires version 2.")
                } else if app.focus == Focus::Items {
                    match app.selected_item {
                        Some(ItemIdentity::Canonical(_)) => CommandAvailability::Enabled,
                        Some(ItemIdentity::Legacy) => CommandAvailability::Disabled(
                            "Convert legacy entries instead of renaming the group.",
                        ),
                        None => CommandAvailability::Disabled("Select an item first."),
                    }
                } else {
                    match app.selected_entry {
                        Some(EntryIdentity::Field(_)) => CommandAvailability::Enabled,
                        Some(EntryIdentity::Legacy(_)) => CommandAvailability::Disabled(
                            "Convert the legacy entry instead of renaming it.",
                        ),
                        None => CommandAvailability::Disabled("Select a field first."),
                    }
                }
            }
            Self::ConvertLegacy => {
                if !writable {
                    CommandAvailability::Disabled("Vault management requires version 2.")
                } else if matches!(app.selected_entry, Some(EntryIdentity::Legacy(_))) {
                    CommandAvailability::Enabled
                } else {
                    CommandAvailability::Disabled("Select a legacy entry first.")
                }
            }
            Self::DeleteSelection => {
                if !writable {
                    CommandAvailability::Disabled("Vault management requires version 2.")
                } else if app.focus == Focus::Items {
                    match app.selected_item {
                        Some(ItemIdentity::Canonical(_)) => CommandAvailability::Enabled,
                        Some(ItemIdentity::Legacy) => CommandAvailability::Disabled(
                            "Select one legacy entry; bulk legacy deletion is disabled.",
                        ),
                        None => CommandAvailability::Disabled("Select an item first."),
                    }
                } else if app.selected_entry.is_some() {
                    CommandAvailability::Enabled
                } else {
                    CommandAvailability::Disabled("Select a field or legacy entry first.")
                }
            }
            Self::ExportField | Self::PeekField => match app.selected_entry {
                Some(EntryIdentity::Field(_)) => CommandAvailability::Enabled,
                Some(EntryIdentity::Legacy(_)) => CommandAvailability::Disabled(
                    "Convert the legacy entry to a canonical field first.",
                ),
                None => CommandAvailability::Disabled("Select a canonical field first."),
            },
            Self::Refresh | Self::Activity | Self::VerifyAudit | Self::Lock => {
                if app.is_unlocked() {
                    CommandAvailability::Enabled
                } else {
                    CommandAvailability::Disabled("Unlock the vault first.")
                }
            }
            Self::MigrateToV2 => match format_version {
                Some(1) => CommandAvailability::Enabled,
                Some(_) => CommandAvailability::Disabled("The vault already uses version 2."),
                None => CommandAvailability::Disabled("Unlock the vault first."),
            },
            Self::ImportOnePassword | Self::CreateBackup | Self::ChangePassphrase => {
                if writable {
                    CommandAvailability::Enabled
                } else {
                    CommandAvailability::Disabled("An unlocked version 2 vault is required.")
                }
            }
            Self::RestoreBackup => {
                if app.snapshot.is_some() || app.descriptor.exists {
                    CommandAvailability::Disabled("Restore requires a completely absent vault.")
                } else if cfg!(target_os = "linux") {
                    CommandAvailability::Enabled
                } else {
                    CommandAvailability::Disabled("Restore is currently supported only on Linux.")
                }
            }
        }
    }

    pub(crate) fn visible_in_state(self, app: &App) -> bool {
        if app.snapshot.is_none() {
            return self == Self::RestoreBackup && !app.descriptor.exists;
        }
        match app
            .snapshot
            .as_ref()
            .map(|snapshot| snapshot.format_version)
        {
            Some(1) => matches!(
                self,
                Self::Refresh | Self::MigrateToV2 | Self::Activity | Self::VerifyAudit | Self::Lock
            ),
            Some(2) => self != Self::MigrateToV2 && self != Self::RestoreBackup,
            _ => false,
        }
    }

    pub(crate) fn relevant_to_context(self, app: &App) -> bool {
        if !self.visible_in_state(app) {
            return false;
        }
        if app
            .snapshot
            .as_ref()
            .is_some_and(|snapshot| snapshot.format_version == 1)
        {
            return self == Self::MigrateToV2;
        }
        match app.focus {
            Focus::Items => matches!(
                self,
                Self::CreateItem
                    | Self::AddField
                    | Self::AddLegacy
                    | Self::RenameSelection
                    | Self::DeleteSelection
            ),
            Focus::Fields | Focus::Details => match app.selected_entry {
                Some(EntryIdentity::Field(_)) => matches!(
                    self,
                    Self::AddField
                        | Self::ReplaceValue
                        | Self::ChangeKind
                        | Self::RenameSelection
                        | Self::DeleteSelection
                        | Self::ExportField
                        | Self::PeekField
                ),
                Some(EntryIdentity::Legacy(_)) => matches!(
                    self,
                    Self::AddLegacy
                        | Self::ReplaceValue
                        | Self::ConvertLegacy
                        | Self::DeleteSelection
                        | Self::ExportField
                        | Self::PeekField
                ),
                None => matches!(self, Self::CreateItem | Self::AddField | Self::AddLegacy),
            },
        }
    }

    pub(crate) const fn tool_choice(self) -> Option<ToolChoice> {
        match self {
            Self::Activity => Some(ToolChoice::Activity),
            Self::VerifyAudit => Some(ToolChoice::VerifyAudit),
            Self::ImportOnePassword => Some(ToolChoice::ImportOnePassword),
            Self::CreateBackup => Some(ToolChoice::CreateBackup),
            Self::ChangePassphrase => Some(ToolChoice::ChangePassphrase),
            Self::RestoreBackup => Some(ToolChoice::RestoreBackup),
            _ => None,
        }
    }

    pub(crate) fn hint(self) -> String {
        self.binding().map_or_else(
            || self.short_label().to_owned(),
            |binding| format!("{} {}", binding.label, self.short_label()),
        )
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum CommandSafety {
    Ordinary,
    Disclosure,
    Destructive,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum CommandAvailability {
    Enabled,
    Disabled(&'static str),
}

impl CommandAvailability {
    pub(crate) const fn is_enabled(self) -> bool {
        matches!(self, Self::Enabled)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct CommandBinding {
    code: KeyCode,
    allow_shift: bool,
    pub(crate) label: &'static str,
}

impl CommandBinding {
    const fn plain(character: char, label: &'static str) -> Self {
        Self {
            code: KeyCode::Char(character),
            allow_shift: false,
            label,
        }
    }

    const fn shifted(character: char, label: &'static str) -> Self {
        Self {
            code: KeyCode::Char(character),
            allow_shift: true,
            label,
        }
    }

    fn matches(self, key: KeyEvent) -> bool {
        if key.code != self.code {
            return false;
        }
        key.modifiers.is_empty() || (self.allow_shift && key.modifiers == KeyModifiers::SHIFT)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum CommandPaletteScope {
    Context,
    Universal,
}

#[derive(Debug)]
pub(crate) enum CommandOutcome {
    Redraw,
    Start(VaultAction),
    Lock,
}

impl CommandPaletteScope {
    pub(crate) const fn title(self) -> &'static str {
        match self {
            Self::Context => "Actions for selection",
            Self::Universal => "All vault actions",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct CommandEntry {
    pub(crate) command: UiCommand,
    pub(crate) availability: CommandAvailability,
}

#[derive(Debug)]
pub(crate) struct CommandPalette {
    pub(crate) scope: CommandPaletteScope,
    pub(crate) entries: Vec<CommandEntry>,
    pub(crate) selected: usize,
    pub(crate) filter: LineEditor,
}

impl CommandPalette {
    pub(crate) fn for_app(app: &App, scope: CommandPaletteScope) -> Self {
        let entries = UiCommand::ALL
            .into_iter()
            .filter(|command| match scope {
                CommandPaletteScope::Context => command.relevant_to_context(app),
                CommandPaletteScope::Universal => command.visible_in_state(app),
            })
            .map(|command| CommandEntry {
                command,
                availability: command.availability(app),
            })
            .collect();
        Self {
            scope,
            entries,
            selected: 0,
            filter: LineEditor::command(),
        }
    }

    pub(crate) fn visible_entries(&self) -> Vec<CommandEntry> {
        let filter = self.filter.as_str().to_lowercase();
        self.entries
            .iter()
            .copied()
            .filter(|entry| {
                filter.is_empty()
                    || entry.command.label().to_lowercase().contains(&filter)
                    || entry.command.category().to_lowercase().contains(&filter)
                    || entry
                        .command
                        .binding()
                        .is_some_and(|binding| binding.label.to_lowercase().contains(&filter))
            })
            .collect()
    }

    pub(crate) fn selected_entry(&self) -> Option<CommandEntry> {
        self.visible_entries().get(self.selected).copied()
    }

    pub(crate) fn move_selection(&mut self, delta: isize) {
        let len = self.visible_entries().len();
        if len == 0 {
            self.selected = 0;
        } else {
            self.selected = self.selected.saturating_add_signed(delta).min(len - 1);
        }
    }

    pub(crate) fn append_filter(&mut self, value: &str) -> bool {
        let accepted = self.filter.insert(value);
        if !accepted {
            return false;
        }
        self.reconcile_selection();
        true
    }

    pub(crate) fn edit_filter(&mut self, edit: LineEdit) {
        self.filter.apply(edit);
        self.reconcile_selection();
    }

    fn reconcile_selection(&mut self) {
        self.selected = self
            .selected
            .min(self.visible_entries().len().saturating_sub(1));
    }
}
