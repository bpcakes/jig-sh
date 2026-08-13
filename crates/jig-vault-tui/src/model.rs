use std::collections::BTreeSet;

use jig_tui::sanitize_text;
use jig_vault::{FieldKind, FieldRecord, SecretBytes, SecretRecord, VaultReference, VaultSnapshot};

use crate::{VaultDescriptor, VaultUiError, secret_input::SecretInput};

#[derive(Debug)]
pub(crate) struct App {
    pub(crate) descriptor: VaultDescriptor,
    pub(crate) screen: Screen,
    pub(crate) snapshot: Option<VaultSnapshot>,
    pub(crate) focus: Focus,
    pub(crate) selected_item: Option<ItemIdentity>,
    pub(crate) selected_entry: Option<EntryIdentity>,
    pub(crate) filter: String,
    pub(crate) searching: bool,
    pub(crate) status: Option<StatusMessage>,
    pub(crate) tick: usize,
}

impl App {
    pub(crate) fn new(descriptor: VaultDescriptor) -> Self {
        let screen = if descriptor.exists {
            Screen::Locked(SecretInput::new())
        } else {
            Screen::Missing
        };
        Self {
            descriptor,
            screen,
            snapshot: None,
            focus: Focus::Items,
            selected_item: None,
            selected_entry: None,
            filter: String::new(),
            searching: false,
            status: None,
            tick: 0,
        }
    }

    pub(crate) fn begin_unlock(&mut self) -> Option<SecretBytes> {
        let Screen::Locked(input) = &mut self.screen else {
            return None;
        };
        if input.is_empty() {
            self.set_error("Enter the vault passphrase first.");
            return None;
        }
        let passphrase = input.take();
        self.screen = Screen::Loading("Unlocking vault");
        self.status = None;
        Some(passphrase)
    }

    pub(crate) fn begin_initialize_form(&mut self) {
        if matches!(self.screen, Screen::Missing) {
            self.screen = Screen::Initialize {
                passphrase: SecretInput::new(),
                confirmation: SecretInput::new(),
                focus: InitializeFocus::Passphrase,
            };
            self.status = None;
        }
    }

    pub(crate) fn begin_initialize(&mut self) -> Option<SecretBytes> {
        let Screen::Initialize {
            passphrase,
            confirmation,
            ..
        } = &mut self.screen
        else {
            return None;
        };
        if passphrase.is_empty() {
            self.set_error("Enter a new vault passphrase first.");
            return None;
        }
        if !passphrase.matches(confirmation) {
            self.set_error("Vault passphrase confirmation did not match.");
            return None;
        }
        let passphrase = passphrase.take();
        confirmation.clear();
        self.screen = Screen::Loading("Creating vault");
        self.status = None;
        Some(passphrase)
    }

    pub(crate) fn begin_loading(&mut self, label: &'static str) {
        self.screen = Screen::Loading(label);
        self.status = None;
    }

    pub(crate) fn apply_snapshot(&mut self, snapshot: VaultSnapshot) {
        let selected_item = self.selected_item.clone();
        let selected_entry = self.selected_entry.clone();
        self.descriptor.exists = true;
        self.snapshot = Some(snapshot);
        self.screen = Screen::Browse;
        self.status = None;
        self.selected_item = selected_item;
        self.selected_entry = selected_entry;
        self.reconcile_selection();
    }

    pub(crate) fn fail_unlock(&mut self, error: &VaultUiError) {
        self.snapshot = None;
        self.screen = Screen::Locked(SecretInput::new());
        self.status = Some(StatusMessage::error(error.message()));
    }

    pub(crate) fn fail_initialize(&mut self, error: &VaultUiError) {
        self.snapshot = None;
        self.screen = Screen::Missing;
        self.status = Some(StatusMessage::error(error.message()));
    }

    pub(crate) fn fail_action(&mut self, error: &VaultUiError) {
        self.screen = Screen::Browse;
        self.status = Some(StatusMessage::error(error.message()));
    }

    pub(crate) fn lock(&mut self) {
        self.snapshot = None;
        self.selected_item = None;
        self.selected_entry = None;
        self.filter.clear();
        self.searching = false;
        self.focus = Focus::Items;
        self.screen = Screen::Locked(SecretInput::new());
        self.status = Some(StatusMessage::info("Vault locked."));
    }

    pub(crate) fn show_help(&mut self) {
        if matches!(self.screen, Screen::Browse) {
            self.screen = Screen::Help;
            self.searching = false;
        }
    }

    pub(crate) fn close_overlay(&mut self) {
        if matches!(self.screen, Screen::Help | Screen::ConfirmMigration) {
            self.screen = Screen::Browse;
        }
    }

    pub(crate) fn confirm_migration(&mut self) {
        if self
            .snapshot
            .as_ref()
            .is_some_and(|snapshot| snapshot.format_version == 1)
        {
            self.screen = Screen::ConfirmMigration;
        }
    }

    pub(crate) fn input_mut(&mut self) -> Option<&mut SecretInput> {
        match &mut self.screen {
            Screen::Locked(input) => Some(input),
            Screen::Initialize {
                passphrase,
                confirmation,
                focus,
            } => Some(match focus {
                InitializeFocus::Passphrase => passphrase,
                InitializeFocus::Confirmation => confirmation,
            }),
            _ => None,
        }
    }

    pub(crate) fn toggle_initialize_focus(&mut self) {
        if let Screen::Initialize { focus, .. } = &mut self.screen {
            *focus = match focus {
                InitializeFocus::Passphrase => InitializeFocus::Confirmation,
                InitializeFocus::Confirmation => InitializeFocus::Passphrase,
            };
        }
    }

    pub(crate) fn cancel_initialize(&mut self) {
        if matches!(self.screen, Screen::Initialize { .. }) {
            self.screen = Screen::Missing;
            self.status = None;
        }
    }

    pub(crate) fn cycle_focus(&mut self, backwards: bool) {
        self.focus = match (self.focus, backwards) {
            (Focus::Items, false) | (Focus::Details, true) => Focus::Fields,
            (Focus::Fields, false) | (Focus::Items, true) => Focus::Details,
            (Focus::Details, false) | (Focus::Fields, true) => Focus::Items,
        };
    }

    pub(crate) fn visible_items(&self) -> Vec<ItemIdentity> {
        let Some(snapshot) = &self.snapshot else {
            return Vec::new();
        };
        let query = self.filter.to_lowercase();
        let mut items = snapshot
            .fields
            .iter()
            .map(|field| field.reference.item().to_owned())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .map(ItemIdentity::Canonical)
            .filter(|item| self.item_matches(item, &query))
            .collect::<Vec<_>>();
        if !snapshot.legacy_secrets.is_empty() {
            let legacy = ItemIdentity::Legacy;
            if self.item_matches(&legacy, &query) {
                items.push(legacy);
            }
        }
        items
    }

    pub(crate) fn visible_entries(&self) -> Vec<EntryIdentity> {
        let Some(snapshot) = &self.snapshot else {
            return Vec::new();
        };
        let query = self.filter.to_lowercase();
        match &self.selected_item {
            Some(ItemIdentity::Canonical(item)) => snapshot
                .fields
                .iter()
                .filter(|record| record.reference.item() == item)
                .filter(|record| field_matches(record, &query))
                .map(|record| EntryIdentity::Field(record.reference.clone()))
                .collect(),
            Some(ItemIdentity::Legacy) => snapshot
                .legacy_secrets
                .iter()
                .filter(|record| legacy_matches(record, &query))
                .map(|record| EntryIdentity::Legacy(record.name.clone()))
                .collect(),
            None => Vec::new(),
        }
    }

    pub(crate) fn selected_field(&self) -> Option<&FieldRecord> {
        let EntryIdentity::Field(reference) = self.selected_entry.as_ref()? else {
            return None;
        };
        self.snapshot
            .as_ref()?
            .fields
            .iter()
            .find(|record| &record.reference == reference)
    }

    pub(crate) fn selected_legacy(&self) -> Option<&SecretRecord> {
        let EntryIdentity::Legacy(name) = self.selected_entry.as_ref()? else {
            return None;
        };
        self.snapshot
            .as_ref()?
            .legacy_secrets
            .iter()
            .find(|record| &record.name == name)
    }

    pub(crate) fn move_selection(&mut self, delta: isize) {
        match self.focus {
            Focus::Items => {
                let rows = self.visible_items();
                self.selected_item = move_identity(&rows, self.selected_item.as_ref(), delta);
                self.selected_entry = None;
                self.reconcile_entry();
            }
            Focus::Fields => {
                let rows = self.visible_entries();
                self.selected_entry = move_identity(&rows, self.selected_entry.as_ref(), delta);
            }
            Focus::Details => {}
        }
    }

    pub(crate) fn move_to_edge(&mut self, end: bool) {
        match self.focus {
            Focus::Items => {
                let rows = self.visible_items();
                self.selected_item = if end {
                    rows.last().cloned()
                } else {
                    rows.first().cloned()
                };
                self.selected_entry = None;
                self.reconcile_entry();
            }
            Focus::Fields => {
                let rows = self.visible_entries();
                self.selected_entry = if end {
                    rows.last().cloned()
                } else {
                    rows.first().cloned()
                };
            }
            Focus::Details => {}
        }
    }

    pub(crate) fn push_filter(&mut self, character: char) {
        if !character.is_control() {
            self.filter.push(character);
            self.reconcile_selection();
        }
    }

    pub(crate) fn pop_filter(&mut self) {
        self.filter.pop();
        self.reconcile_selection();
    }

    pub(crate) fn clear_filter(&mut self) {
        self.filter.clear();
        self.reconcile_selection();
    }

    pub(crate) fn snapshot_counts(&self) -> (usize, usize, usize) {
        let Some(snapshot) = &self.snapshot else {
            return (0, 0, 0);
        };
        let items = snapshot
            .fields
            .iter()
            .map(|field| field.reference.item())
            .collect::<BTreeSet<_>>()
            .len();
        (items, snapshot.fields.len(), snapshot.legacy_secrets.len())
    }

    pub(crate) fn set_error(&mut self, message: &str) {
        self.status = Some(StatusMessage::error(message));
    }

    pub(crate) fn set_info(&mut self, message: &str) {
        self.status = Some(StatusMessage::info(message));
    }

    fn item_matches(&self, item: &ItemIdentity, query: &str) -> bool {
        if query.is_empty() {
            return true;
        }
        let Some(snapshot) = &self.snapshot else {
            return false;
        };
        match item {
            ItemIdentity::Canonical(item) => {
                item.to_lowercase().contains(query)
                    || snapshot.fields.iter().any(|record| {
                        record.reference.item() == item && field_matches(record, query)
                    })
            }
            ItemIdentity::Legacy => {
                "legacy".contains(query)
                    || snapshot
                        .legacy_secrets
                        .iter()
                        .any(|record| legacy_matches(record, query))
            }
        }
    }

    fn reconcile_selection(&mut self) {
        let items = self.visible_items();
        if !self
            .selected_item
            .as_ref()
            .is_some_and(|selected| items.contains(selected))
        {
            self.selected_item = items.first().cloned();
            self.selected_entry = None;
        }
        self.reconcile_entry();
    }

    fn reconcile_entry(&mut self) {
        let entries = self.visible_entries();
        if !self
            .selected_entry
            .as_ref()
            .is_some_and(|selected| entries.contains(selected))
        {
            self.selected_entry = entries.first().cloned();
        }
    }
}

fn move_identity<T: Clone + PartialEq>(
    rows: &[T],
    selected: Option<&T>,
    delta: isize,
) -> Option<T> {
    if rows.is_empty() {
        return None;
    }
    let current = selected
        .and_then(|selected| rows.iter().position(|row| row == selected))
        .unwrap_or(0);
    rows.get(current.saturating_add_signed(delta).min(rows.len() - 1))
        .cloned()
}

fn field_matches(record: &FieldRecord, query: &str) -> bool {
    query.is_empty()
        || record.reference.item().to_lowercase().contains(query)
        || record.reference.field().to_lowercase().contains(query)
        || record.reference.to_string().to_lowercase().contains(query)
        || record.kind.as_str().contains(query)
}

fn legacy_matches(record: &SecretRecord, query: &str) -> bool {
    query.is_empty() || record.name.to_lowercase().contains(query)
}

#[derive(Debug)]
pub(crate) enum Screen {
    Missing,
    Locked(SecretInput),
    Initialize {
        passphrase: SecretInput,
        confirmation: SecretInput,
        focus: InitializeFocus,
    },
    Loading(&'static str),
    Browse,
    Help,
    ConfirmMigration,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum InitializeFocus {
    Passphrase,
    Confirmation,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Focus {
    Items,
    Fields,
    Details,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum ItemIdentity {
    Canonical(String),
    Legacy,
}

impl ItemIdentity {
    pub(crate) fn label(&self, legacy_count: usize) -> String {
        match self {
            Self::Canonical(item) => sanitize_text(item),
            Self::Legacy => format!("Legacy ({legacy_count})"),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum EntryIdentity {
    Field(VaultReference),
    Legacy(String),
}

impl EntryIdentity {
    pub(crate) fn label(&self) -> String {
        match self {
            Self::Field(reference) => sanitize_text(reference.field()),
            Self::Legacy(name) => sanitize_text(name),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct StatusMessage {
    pub(crate) kind: StatusKind,
    pub(crate) text: String,
}

impl StatusMessage {
    pub(crate) fn error(message: &str) -> Self {
        Self {
            kind: StatusKind::Error,
            text: sanitize_text(message),
        }
    }

    pub(crate) fn info(message: &str) -> Self {
        Self {
            kind: StatusKind::Info,
            text: sanitize_text(message),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum StatusKind {
    Info,
    Error,
}

pub(crate) const fn kind_label(kind: FieldKind) -> &'static str {
    match kind {
        FieldKind::Concealed => "concealed",
        FieldKind::Text => "text",
    }
}
