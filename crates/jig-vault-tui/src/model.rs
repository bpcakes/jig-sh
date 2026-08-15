use std::path::Path;

use jig_tui::sanitize_text;
use jig_vault::{
    AuditVerification, FieldKind, FieldRecord, SecretBytes, SecretName, SecretRecord, VaultItem,
    VaultReference, VaultSnapshot, VaultWriteMode, VerifiedVaultActivity,
};

use crate::{
    ImportPreview, ImportPreviewAuthorization, VaultAction, VaultDescriptor, VaultHomeState,
    VaultMutation, VaultUiError,
    browse::{BrowseEntryKind, BrowseState},
    commands::{CommandOutcome, CommandPalette, CommandPaletteScope, UiCommand},
    line_editor::{LineEdit, LineEditor},
    quick_access::{QuickAccess, QuickAccessSelection},
    secret_input::SecretInput,
    tools::{ToolActivation, ToolChoice, ToolForm},
};

#[derive(Debug)]
pub(crate) struct App {
    pub(crate) descriptor: VaultDescriptor,
    pub(crate) screen: Screen,
    browser: Option<BrowseState>,
    pub(crate) focus: Focus,
    pub(crate) selected_item: Option<ItemIdentity>,
    pub(crate) selected_entry: Option<EntryIdentity>,
    filter: LineEditor,
    pub(crate) searching: bool,
    pub(crate) status: Option<StatusMessage>,
    pub(crate) tick: usize,
    next_selection: Option<SelectionHint>,
}

impl App {
    pub(crate) fn new(descriptor: VaultDescriptor) -> Self {
        let screen = if descriptor.home_state.is_initialized() {
            Screen::Locked(SecretInput::new())
        } else {
            Screen::Missing
        };
        Self {
            descriptor,
            screen,
            browser: None,
            focus: Focus::Items,
            selected_item: None,
            selected_entry: None,
            filter: LineEditor::search(),
            searching: false,
            status: None,
            tick: 0,
            next_selection: None,
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
        if let Err(error) = passphrase.validate_new_vault_passphrase() {
            self.set_error(&error);
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
        self.install_snapshot(snapshot);
    }

    pub(crate) fn apply_recovery_snapshot(&mut self, snapshot: VaultSnapshot) {
        self.next_selection = None;
        self.install_snapshot(snapshot);
    }

    fn install_snapshot(&mut self, snapshot: VaultSnapshot) {
        let previous_item = self.selected_item.clone();
        let previous_entry = self.selected_entry.clone();
        self.descriptor.home_state = VaultHomeState::Initialized;
        self.browser = Some(BrowseState::new(snapshot, self.filter.as_str()));
        self.screen = Screen::Browse;
        self.status = None;
        if let Some(hint) = self.next_selection.take() {
            self.selected_item = Some(hint.item);
            self.selected_entry = hint.entry;
        } else {
            self.selected_item = previous_item;
            self.selected_entry = previous_entry;
        }
        self.reconcile_selection();
    }

    pub(crate) fn fail_unlock(&mut self, error: &VaultUiError) {
        self.browser = None;
        self.next_selection = None;
        self.screen = Screen::Locked(SecretInput::new());
        self.status = Some(StatusMessage::error(error.message()));
    }

    pub(crate) fn fail_action(&mut self, error: &VaultUiError) {
        self.next_selection = None;
        self.screen = Screen::Browse;
        self.status = Some(StatusMessage::error(error.message()));
    }

    pub(crate) fn fail_lifecycle(&mut self, error: &VaultUiError, home_state: VaultHomeState) {
        self.browser = None;
        self.next_selection = None;
        self.descriptor.home_state = home_state;
        self.screen = match home_state {
            VaultHomeState::Absent | VaultHomeState::Uninitialized => Screen::Missing,
            VaultHomeState::Initialized => Screen::Locked(SecretInput::new()),
        };
        self.status = Some(StatusMessage::error(error.message()));
    }

    pub(crate) fn lock(&mut self) {
        self.browser = None;
        self.next_selection = None;
        self.selected_item = None;
        self.selected_entry = None;
        self.clear_filter_projection();
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
        if matches!(
            self.screen,
            Screen::Help
                | Screen::ConfirmMigration
                | Screen::Form(_)
                | Screen::ConfirmMutation(_)
                | Screen::ConfirmDelete(_)
                | Screen::Commands(_)
                | Screen::QuickAccess(_)
                | Screen::ToolForm(_)
                | Screen::Activity(_)
                | Screen::AuditResult(_)
                | Screen::ConfirmPeek(_)
        ) {
            self.screen = if self.snapshot().is_some() {
                Screen::Browse
            } else {
                Screen::Missing
            };
            self.status = None;
        }
    }

    pub(crate) fn confirm_migration(&mut self) {
        if self
            .snapshot()
            .is_some_and(|snapshot| snapshot.format_version == 1)
        {
            self.screen = Screen::ConfirmMigration;
        }
    }

    pub(crate) fn protected_input_mut(&mut self) -> Option<&mut SecretInput> {
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
            Screen::Form(form) => form.protected_input_mut(),
            Screen::ToolForm(form) => form.protected_input_mut(),
            _ => None,
        }
    }

    fn metadata_input_mut(&mut self) -> Option<&mut LineEditor> {
        match &mut self.screen {
            Screen::Form(form) => form.metadata_input_mut(),
            Screen::ToolForm(form) => form.metadata_input_mut(),
            Screen::ConfirmMutation(confirmation) => Some(&mut confirmation.input),
            Screen::ConfirmDelete(confirmation) => Some(&mut confirmation.input),
            Screen::ImportPreview(preview) => Some(&mut preview.confirmation),
            Screen::ConfirmPeek(confirmation) => Some(&mut confirmation.input),
            _ => None,
        }
    }

    pub(crate) fn metadata_input_is_active(&mut self) -> bool {
        self.metadata_input_mut().is_some()
    }

    pub(crate) fn handle_metadata_append(&mut self, value: &str) -> bool {
        let Some(input) = self.metadata_input_mut() else {
            return false;
        };
        let accepted = input.insert(value);
        if !accepted {
            self.set_error("Metadata input exceeds the interactive size limit.");
        }
        true
    }

    #[cfg(test)]
    pub(crate) fn clear_metadata_input(&mut self) {
        if let Some(input) = self.metadata_input_mut() {
            input.clear();
        }
    }

    pub(crate) fn edit_metadata_input(&mut self, edit: LineEdit) -> bool {
        let Some(input) = self.metadata_input_mut() else {
            return false;
        };
        input.apply(edit);
        true
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

    pub(crate) fn open_command_palette(&mut self, scope: CommandPaletteScope) {
        let palette = CommandPalette::for_app(self, scope);
        self.screen = Screen::Commands(palette);
        self.searching = false;
        self.status = None;
    }

    pub(crate) fn move_command_selection(&mut self, delta: isize) {
        if let Screen::Commands(palette) = &mut self.screen {
            palette.move_selection(delta);
        }
    }

    pub(crate) fn append_command_filter(&mut self, value: &str) -> bool {
        let Screen::Commands(palette) = &mut self.screen else {
            return false;
        };
        if !palette.append_filter(value) {
            self.set_error("Action filter exceeds the interactive size limit.");
        }
        true
    }

    pub(crate) fn edit_command_filter(&mut self, edit: LineEdit) {
        if let Screen::Commands(palette) = &mut self.screen {
            palette.edit_filter(edit);
        }
    }

    pub(crate) fn open_quick_access(&mut self) {
        if self.snapshot().is_none() {
            return;
        }
        self.screen = Screen::QuickAccess(QuickAccess::for_app(self));
        self.searching = false;
        self.status = None;
    }

    pub(crate) fn move_quick_access_selection(&mut self, delta: isize) {
        if let Screen::QuickAccess(access) = &mut self.screen {
            access.move_selection(delta);
        }
    }

    pub(crate) fn move_quick_access_to_edge(&mut self, end: bool) {
        if let Screen::QuickAccess(access) = &mut self.screen {
            access.move_to_edge(end);
        }
    }

    pub(crate) fn append_quick_access_query(&mut self, value: &str) -> bool {
        let Screen::QuickAccess(access) = &mut self.screen else {
            return false;
        };
        if !access.append_query(value) {
            self.set_error("Quick Access query exceeds the interactive size limit.");
        } else {
            self.status = None;
        }
        true
    }

    pub(crate) fn edit_quick_access_query(&mut self, edit: LineEdit) {
        if let Screen::QuickAccess(access) = &mut self.screen {
            access.edit_query(edit);
            self.status = None;
        }
    }

    pub(crate) fn activate_quick_access(&mut self) {
        let selection = match &self.screen {
            Screen::QuickAccess(access) => {
                access.selected_target().map(|target| target.selection())
            }
            _ => None,
        };
        let Some(selection) = selection else {
            self.set_error("No metadata matches the current Quick Access query.");
            return;
        };
        self.clear_filter_projection();
        match selection {
            QuickAccessSelection::Item(item) => {
                self.selected_item = Some(item);
                self.selected_entry = None;
                self.focus = Focus::Items;
                self.reconcile_entry();
            }
            QuickAccessSelection::Entry { item, entry } => {
                self.selected_item = Some(item);
                self.selected_entry = Some(entry);
                self.focus = Focus::Fields;
            }
        }
        self.screen = Screen::Browse;
        self.open_command_palette(CommandPaletteScope::Context);
    }

    pub(crate) fn activate_selected_command(&mut self) -> CommandOutcome {
        let Screen::Commands(palette) = &self.screen else {
            return CommandOutcome::Redraw;
        };
        let Some(entry) = palette.selected_entry() else {
            self.set_error("No action matches the current filter.");
            return CommandOutcome::Redraw;
        };
        if let crate::commands::CommandAvailability::Disabled(reason) = entry.availability {
            self.set_error(reason);
            return CommandOutcome::Redraw;
        }
        self.screen = if self.snapshot().is_some() {
            Screen::Browse
        } else {
            Screen::Missing
        };
        self.status = None;
        self.activate_command(entry.command)
    }

    pub(crate) fn activate_direct_command(&mut self, command: UiCommand) -> CommandOutcome {
        match command.availability(self) {
            crate::commands::CommandAvailability::Enabled => self.activate_command(command),
            crate::commands::CommandAvailability::Disabled(reason) => {
                self.set_error(reason);
                CommandOutcome::Redraw
            }
        }
    }

    fn activate_command(&mut self, command: UiCommand) -> CommandOutcome {
        if let Some(choice) = command.tool_choice() {
            return self.activate_tool_choice(choice);
        }
        match command {
            UiCommand::CreateItem => self.begin_create_item(),
            UiCommand::AddField => self.begin_add(),
            UiCommand::AddLegacy => self.begin_add_legacy(),
            UiCommand::ReplaceValue => self.begin_replace(),
            UiCommand::ChangeKind => self.begin_change_kind(),
            UiCommand::RenameSelection => self.begin_rename(),
            UiCommand::ConvertLegacy => self.begin_convert(),
            UiCommand::DeleteSelection => self.begin_delete(),
            UiCommand::ExportField => self.begin_export(),
            UiCommand::PeekField => self.begin_peek(),
            UiCommand::Refresh => {
                self.begin_loading("Refreshing vault metadata");
                return CommandOutcome::Start(VaultAction::Refresh);
            }
            UiCommand::MigrateToV2 => self.confirm_migration(),
            UiCommand::Lock => return CommandOutcome::Lock,
            UiCommand::Activity
            | UiCommand::VerifyAudit
            | UiCommand::ImportOnePassword
            | UiCommand::CreateBackup
            | UiCommand::ChangePassphrase
            | UiCommand::RestoreBackup => {
                unreachable!("tool commands returned before ordinary activation")
            }
        }
        CommandOutcome::Redraw
    }

    fn activate_tool_choice(&mut self, choice: ToolChoice) -> CommandOutcome {
        match choice.activation() {
            ToolActivation::Immediate {
                action,
                loading_label,
            } => {
                self.begin_loading(loading_label);
                CommandOutcome::Start(action)
            }
            ToolActivation::Form(form) => {
                self.screen = Screen::ToolForm(form);
                self.status = None;
                CommandOutcome::Redraw
            }
        }
    }

    pub(crate) fn apply_activity(&mut self, activity: VerifiedVaultActivity) {
        self.screen = Screen::Activity(ActivityView {
            activity,
            selected: 0,
        });
        self.status = None;
    }

    pub(crate) fn move_activity_selection(&mut self, delta: isize) {
        if let Screen::Activity(view) = &mut self.screen {
            if view.activity.records.is_empty() {
                view.selected = 0;
            } else {
                view.selected = view
                    .selected
                    .saturating_add_signed(delta)
                    .min(view.activity.records.len() - 1);
            }
        }
    }

    pub(crate) fn apply_audit_result(&mut self, verification: AuditVerification) {
        self.screen = Screen::AuditResult(verification);
        self.status = None;
    }

    pub(crate) fn apply_import_preview(&mut self, preview: ImportPreview) {
        self.screen = Screen::ImportPreview(ImportPreviewState {
            preview,
            confirmation: LineEditor::metadata(),
        });
        self.status = None;
    }

    pub(crate) fn discard_import_preview(&mut self) -> Option<VaultAction> {
        let screen = std::mem::replace(&mut self.screen, Screen::Browse);
        let Screen::ImportPreview(state) = screen else {
            self.screen = screen;
            return None;
        };
        let ImportPreviewAuthorization::Commit(plan) = state.preview.authorization else {
            self.status = None;
            return None;
        };
        self.begin_loading("Discarding 1Password import preview");
        Some(VaultAction::DiscardOnePasswordImport { plan })
    }

    pub(crate) fn finish_import_discard(&mut self) {
        self.screen = Screen::Browse;
        self.status = Some(StatusMessage::info("1Password import preview discarded."));
    }

    pub(crate) fn toggle_import_replace(&mut self) {
        if let Screen::ImportPreview(state) = &mut self.screen {
            state.preview.replace = !state.preview.replace;
        }
    }

    pub(crate) fn toggle_import_overwrite(&mut self) {
        if let Screen::ImportPreview(state) = &mut self.screen {
            state.preview.overwrite = !state.preview.overwrite;
        }
    }

    pub(crate) fn submit_import_preview(&mut self) -> Option<VaultAction> {
        let screen = std::mem::replace(&mut self.screen, Screen::Browse);
        let Screen::ImportPreview(state) = screen else {
            self.screen = screen;
            return None;
        };
        if state.preview.is_dry_run() {
            self.status = Some(StatusMessage::info(
                "1Password dry-run preview completed without resolving values or changing files.",
            ));
            return None;
        }
        if state
            .preview
            .rows
            .iter()
            .any(|row| row.change.replaces_existing())
            && !state.preview.replace
        {
            self.screen = Screen::ImportPreview(state);
            self.set_error("Existing fields require Replace; press r to enable it.");
            return None;
        }
        if state.preview.destination_exists && !state.preview.overwrite {
            self.screen = Screen::ImportPreview(state);
            self.set_error("The dotenv destination requires Overwrite; press o to enable it.");
            return None;
        }
        let required_confirmation = state.required_confirmation();
        if state.confirmation.as_str() != required_confirmation {
            let message = state.invalid_confirmation_message();
            self.screen = Screen::ImportPreview(state);
            self.set_error(message);
            return None;
        }
        if let Some(first) = state.preview.rows.first() {
            self.next_selection = Some(SelectionHint {
                item: ItemIdentity::Canonical(first.reference.item().to_owned()),
                entry: Some(EntryIdentity::Field(first.reference.clone())),
            });
        }
        let preview = state.preview;
        let ImportPreviewAuthorization::Commit(plan) = preview.authorization else {
            unreachable!("dry-run previews returned before commit")
        };
        self.begin_loading("Resolving and importing 1Password values");
        Some(VaultAction::CommitOnePasswordImport {
            plan,
            replace: preview.replace,
            overwrite: preview.overwrite,
        })
    }

    pub(crate) fn apply_restore(&mut self) {
        self.descriptor.home_state = VaultHomeState::Initialized;
        self.browser = None;
        self.next_selection = None;
        self.screen = Screen::Locked(SecretInput::new());
        self.status = Some(StatusMessage::info(
            "Encrypted backup restored. Enter its vault passphrase to unlock.",
        ));
    }

    pub(crate) fn begin_add(&mut self) {
        if !self.require_writable_v2() {
            return;
        }
        let Some(ItemIdentity::Canonical(item)) = &self.selected_item else {
            self.set_error("Select a canonical item or press I to create a new item first.");
            return;
        };
        self.screen = Screen::Form(ManagementForm::write_field(
            FieldWriteIntent::AddField,
            LineEditor::prefilled_metadata(item.clone()),
            LineEditor::metadata(),
            FieldKind::Concealed,
            FieldWriteFocus::Field,
        ));
        self.status = None;
    }

    pub(crate) fn begin_create_item(&mut self) {
        if !self.require_writable_v2() {
            return;
        }
        self.screen = Screen::Form(ManagementForm::write_field(
            FieldWriteIntent::CreateItem,
            LineEditor::metadata(),
            LineEditor::metadata(),
            FieldKind::Concealed,
            FieldWriteFocus::Item,
        ));
        self.status = None;
    }

    pub(crate) fn begin_add_legacy(&mut self) {
        if !self.require_writable_v2() {
            return;
        }
        self.screen = Screen::Form(ManagementForm::WriteLegacy {
            mode: VaultWriteMode::Create,
            name: LineEditor::metadata(),
            value: SecretInput::new(),
            value_file: LineEditor::metadata(),
            focus: LegacyWriteFocus::Name,
        });
        self.status = None;
    }

    pub(crate) fn begin_replace(&mut self) {
        if !self.require_writable_v2() {
            return;
        }
        let form = match self.selected_entry.clone() {
            Some(EntryIdentity::Field(reference)) => {
                let kind = self
                    .selected_field()
                    .map_or(FieldKind::Concealed, |field| field.kind);
                ManagementForm::write_field(
                    FieldWriteIntent::ReplaceValue,
                    LineEditor::prefilled_metadata(reference.item().to_owned()),
                    LineEditor::prefilled_metadata(reference.field().to_owned()),
                    kind,
                    FieldWriteFocus::Value,
                )
            }
            Some(EntryIdentity::Legacy(name)) => ManagementForm::WriteLegacy {
                mode: VaultWriteMode::Replace,
                name: LineEditor::prefilled_metadata(name),
                value: SecretInput::new(),
                value_file: LineEditor::metadata(),
                focus: LegacyWriteFocus::Value,
            },
            None => {
                self.set_error("Select a field or legacy entry to replace.");
                return;
            }
        };
        self.screen = Screen::Form(form);
        self.status = None;
    }

    pub(crate) fn begin_change_kind(&mut self) {
        if !self.require_writable_v2() {
            return;
        }
        let Some(field) = self.selected_field() else {
            self.set_error("Select a canonical field to change its kind.");
            return;
        };
        self.screen = Screen::Form(ManagementForm::ChangeKind {
            reference: field.reference.clone(),
            from: field.kind,
            to: toggled_kind(field.kind),
        });
        self.status = None;
    }

    pub(crate) fn begin_rename(&mut self) {
        if !self.require_writable_v2() {
            return;
        }
        let form = if self.focus == Focus::Items {
            match self.selected_item.clone() {
                Some(ItemIdentity::Canonical(source)) => ManagementForm::RenameItem {
                    source,
                    destination: LineEditor::metadata(),
                },
                Some(ItemIdentity::Legacy) => {
                    self.set_error(
                        "Legacy entries are renamed by converting them to canonical fields.",
                    );
                    return;
                }
                None => {
                    self.set_error("Select an item to rename.");
                    return;
                }
            }
        } else {
            match self.selected_entry.clone() {
                Some(EntryIdentity::Field(source)) => ManagementForm::RenameField {
                    destination_item: LineEditor::prefilled_metadata(source.item().to_owned()),
                    destination_field: LineEditor::metadata(),
                    source,
                    focus: RenameFieldFocus::Field,
                },
                Some(EntryIdentity::Legacy(_)) => {
                    self.set_error("Press c to convert the selected legacy entry.");
                    return;
                }
                None => {
                    self.set_error("Select a field to rename or move.");
                    return;
                }
            }
        };
        self.screen = Screen::Form(form);
        self.status = None;
    }

    pub(crate) fn begin_convert(&mut self) {
        if !self.require_writable_v2() {
            return;
        }
        let Some(EntryIdentity::Legacy(source)) = self.selected_entry.clone() else {
            self.set_error("Select a legacy entry to convert.");
            return;
        };
        self.screen = Screen::Form(ManagementForm::ConvertLegacy {
            source,
            item: LineEditor::metadata(),
            field: LineEditor::metadata(),
            kind: FieldKind::Concealed,
            focus: ConvertFocus::Item,
        });
        self.status = None;
    }

    pub(crate) fn begin_delete(&mut self) {
        if !self.require_writable_v2() {
            return;
        }
        let target = if self.focus == Focus::Items {
            match self.selected_item.clone() {
                Some(ItemIdentity::Canonical(item)) => {
                    let count = self.item_entry_count(&ItemIdentity::Canonical(item.clone()));
                    DeleteTarget::Item { item, count }
                }
                Some(ItemIdentity::Legacy) => {
                    self.set_error("Bulk legacy deletion is disabled; select one legacy entry.");
                    return;
                }
                None => {
                    self.set_error("Select an item to delete.");
                    return;
                }
            }
        } else {
            match self.selected_entry.clone() {
                Some(EntryIdentity::Field(reference)) => DeleteTarget::Field(reference),
                Some(EntryIdentity::Legacy(name)) => DeleteTarget::Legacy(name),
                None => {
                    self.set_error("Select a field or legacy entry to delete.");
                    return;
                }
            }
        };
        self.screen = Screen::ConfirmDelete(DeleteConfirmation {
            target,
            input: LineEditor::metadata(),
        });
        self.status = None;
    }

    pub(crate) fn begin_export(&mut self) {
        match self.selected_entry.clone() {
            Some(EntryIdentity::Field(reference)) => {
                self.screen = Screen::ToolForm(ToolForm::export_field(reference));
                self.status = None;
            }
            Some(EntryIdentity::Legacy(_)) => self.set_error(
                "Legacy values cannot be exported directly; convert the entry to a canonical field first.",
            ),
            None => self.set_error("Select a canonical field to export."),
        }
    }

    pub(crate) fn begin_peek(&mut self) {
        match self.selected_entry.clone() {
            Some(EntryIdentity::Field(reference)) => {
                self.screen = Screen::ConfirmPeek(PeekConfirmation {
                    reference,
                    input: LineEditor::metadata(),
                });
                self.status = None;
            }
            Some(EntryIdentity::Legacy(_)) => self.set_error(
                "Legacy values cannot be previewed directly; convert the entry to a canonical field first.",
            ),
            None => self.set_error("Select a canonical field to preview."),
        }
    }

    pub(crate) fn cycle_form_focus(&mut self, backwards: bool) {
        match &mut self.screen {
            Screen::Form(form) => form.cycle_focus(backwards),
            Screen::ToolForm(form) => form.cycle_focus(backwards),
            _ => {}
        }
    }

    pub(crate) fn toggle_form_choice(&mut self) {
        match &mut self.screen {
            Screen::Form(form) => form.toggle_kind(),
            Screen::ToolForm(form) => form.toggle_choice(),
            _ => {}
        }
    }

    pub(crate) fn submit_form(&mut self) -> Option<VaultAction> {
        let screen = std::mem::replace(&mut self.screen, Screen::Browse);
        let Screen::Form(mut form) = screen else {
            self.screen = screen;
            return None;
        };
        let submission = {
            let snapshot = self
                .snapshot()
                .expect("management submissions require an unlocked vault snapshot");
            form.submission(snapshot)
        };
        match submission {
            Ok(submission) => {
                let confirmation = self
                    .snapshot()
                    .map(|snapshot| submission.confirmation(snapshot))
                    .expect("management submissions require an unlocked vault snapshot");
                if let Some(kind) = confirmation {
                    self.screen = Screen::ConfirmMutation(MutationConfirmation {
                        kind,
                        submission,
                        input: LineEditor::metadata(),
                    });
                    self.status = None;
                    None
                } else {
                    Some(self.start_form_submission(submission))
                }
            }
            Err(message) => {
                self.screen = Screen::Form(form);
                self.set_error(&message);
                None
            }
        }
    }

    pub(crate) fn submit_mutation_confirmation(&mut self) -> Option<VaultAction> {
        let screen = std::mem::replace(&mut self.screen, Screen::Browse);
        let Screen::ConfirmMutation(confirmation) = screen else {
            self.screen = screen;
            return None;
        };
        if confirmation.input.as_str() != confirmation.kind.required_input() {
            let message = confirmation.kind.invalid_message();
            self.screen = Screen::ConfirmMutation(confirmation);
            self.set_error(message);
            return None;
        }
        Some(self.start_form_submission(confirmation.submission))
    }

    fn start_form_submission(&mut self, submission: FormSubmission) -> VaultAction {
        let action = self.authorize_mutation(submission.mutation);
        self.next_selection = submission.selection;
        self.begin_loading(submission.label);
        action
    }

    fn authorize_mutation(&self, mutation: VaultMutation) -> VaultAction {
        let revision = self
            .snapshot()
            .map(|snapshot| snapshot.revision.clone())
            .expect("management mutations require an unlocked vault snapshot");
        VaultAction::Mutate { revision, mutation }
    }

    pub(crate) fn submit_tool_form(&mut self) -> Option<VaultAction> {
        let fallback = if self.snapshot().is_some() {
            Screen::Browse
        } else {
            Screen::Missing
        };
        let screen = std::mem::replace(&mut self.screen, fallback);
        let Screen::ToolForm(mut form) = screen else {
            self.screen = screen;
            return None;
        };
        match form.submission() {
            Ok(submission) => {
                self.begin_loading(submission.label);
                Some(submission.action)
            }
            Err(message) => {
                self.screen = Screen::ToolForm(form);
                self.set_error(&message);
                None
            }
        }
    }

    pub(crate) fn submit_delete(&mut self) -> Option<VaultAction> {
        let screen = std::mem::replace(&mut self.screen, Screen::Browse);
        let Screen::ConfirmDelete(confirmation) = screen else {
            self.screen = screen;
            return None;
        };
        if confirmation.input.as_str() != confirmation.target.required_confirmation() {
            self.screen = Screen::ConfirmDelete(confirmation);
            self.set_error("Deletion confirmation did not match the required text.");
            return None;
        }
        let (mutation, label) = confirmation.target.into_mutation();
        let action = self.authorize_mutation(mutation);
        self.next_selection = None;
        self.begin_loading(label);
        Some(action)
    }

    pub(crate) fn submit_peek(&mut self) -> Option<VaultReference> {
        let screen = std::mem::replace(&mut self.screen, Screen::Browse);
        let Screen::ConfirmPeek(confirmation) = screen else {
            self.screen = screen;
            return None;
        };
        if confirmation.input.as_str() != "PEEK" {
            self.screen = Screen::ConfirmPeek(confirmation);
            self.set_error("Type PEEK exactly to open the controlled terminal preview.");
            return None;
        }
        self.begin_loading("Preparing controlled terminal preview");
        Some(confirmation.reference)
    }

    pub(crate) fn complete_peek(&mut self, bytes_written: usize) {
        self.screen = Screen::Browse;
        self.status = Some(StatusMessage::info(&format!(
            "Controlled preview cleared after {bytes_written} source bytes."
        )));
    }

    pub(crate) fn lock_after_inactivity(&mut self) {
        self.lock();
        self.status = Some(StatusMessage::info(
            "Vault locked after five minutes without terminal input.",
        ));
    }

    pub(crate) fn is_unlocked(&self) -> bool {
        self.snapshot().is_some()
    }

    pub(crate) fn snapshot(&self) -> Option<&VaultSnapshot> {
        self.browser.as_ref().map(BrowseState::snapshot)
    }

    pub(crate) const fn filter(&self) -> &LineEditor {
        &self.filter
    }

    pub(crate) fn visible_items(&self) -> Vec<ItemIdentity> {
        self.browser
            .as_ref()
            .map_or_else(Vec::new, BrowseState::visible_items)
    }

    pub(crate) fn visible_item_rows(&self) -> Vec<(ItemIdentity, usize)> {
        self.browser
            .as_ref()
            .map_or_else(Vec::new, BrowseState::visible_item_rows)
    }

    pub(crate) fn visible_entries(&self) -> Vec<EntryIdentity> {
        self.browser.as_ref().map_or_else(Vec::new, |browser| {
            browser.visible_entries(self.selected_item.as_ref())
        })
    }

    pub(crate) fn visible_entry_rows(&self) -> Vec<(EntryIdentity, BrowseEntryKind)> {
        self.browser.as_ref().map_or_else(Vec::new, |browser| {
            browser.visible_entry_rows(self.selected_item.as_ref())
        })
    }

    pub(crate) fn item_entry_count(&self, identity: &ItemIdentity) -> usize {
        self.browser
            .as_ref()
            .map_or(0, |browser| browser.item_entry_count(identity))
    }

    pub(crate) fn selected_field(&self) -> Option<&FieldRecord> {
        let EntryIdentity::Field(reference) = self.selected_entry.as_ref()? else {
            return None;
        };
        self.snapshot()?
            .fields
            .iter()
            .find(|record| &record.reference == reference)
    }

    pub(crate) fn selected_legacy(&self) -> Option<&SecretRecord> {
        let EntryIdentity::Legacy(name) = self.selected_entry.as_ref()? else {
            return None;
        };
        self.snapshot()?
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

    pub(crate) fn append_filter(&mut self, value: &str) {
        let previous_len = self.filter.as_str().len();
        if !self.filter.insert(value) {
            self.set_error("Search filter exceeds the interactive size limit.");
            return;
        }
        if self.filter.as_str().len() != previous_len {
            self.refresh_filter_projection();
            self.reconcile_selection();
        }
    }

    pub(crate) fn clear_filter(&mut self) {
        if !self.filter.is_empty() {
            self.clear_filter_projection();
            self.reconcile_selection();
        }
    }

    pub(crate) fn edit_filter(&mut self, edit: LineEdit) {
        let previous_len = self.filter.as_str().len();
        self.filter.apply(edit);
        if edit.changes_text() && self.filter.as_str().len() != previous_len {
            self.refresh_filter_projection();
            self.reconcile_selection();
        }
    }

    pub(crate) fn snapshot_counts(&self) -> (usize, usize, usize) {
        self.browser.as_ref().map_or((0, 0, 0), BrowseState::counts)
    }

    pub(crate) fn set_error(&mut self, message: &str) {
        self.status = Some(StatusMessage::error(message));
    }

    pub(crate) fn set_info(&mut self, message: &str) {
        self.status = Some(StatusMessage::info(message));
    }

    fn require_writable_v2(&mut self) -> bool {
        if self
            .snapshot()
            .is_none_or(|snapshot| snapshot.format_version != 2)
        {
            self.set_error("Vault management requires version 2; press m to migrate first.");
            return false;
        }
        true
    }

    fn clear_filter_projection(&mut self) {
        self.filter.clear();
        self.refresh_filter_projection();
    }

    fn refresh_filter_projection(&mut self) {
        if let Some(browser) = &mut self.browser {
            browser.refresh_filter(self.filter.as_str());
        }
    }

    #[cfg(test)]
    pub(crate) fn browse_filter_refreshes(&self) -> usize {
        self.browser
            .as_ref()
            .map_or(0, BrowseState::filter_refreshes)
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

fn parse_item(value: &str) -> Result<VaultItem, String> {
    VaultItem::parse(&format!("jig://{value}")).map_err(|error| sanitize_text(error.message()))
}

fn parse_reference(item: &str, field: &str) -> Result<VaultReference, String> {
    VaultReference::parse(&format!("jig://{item}/{field}"))
        .map_err(|error| sanitize_text(error.message()))
}

fn parse_legacy_name(value: &str) -> Result<String, String> {
    let name = SecretName::parse(value).map_err(|error| sanitize_text(error.message()))?;
    if VaultReference::parse(&format!("jig://{}", name.as_str())).is_ok() {
        return Err("That name is a canonical ITEM/FIELD; create a field instead.".to_owned());
    }
    Ok(name.as_str().to_owned())
}

fn validate_value(kind: FieldKind, value: &SecretInput) -> Result<(), String> {
    if kind == FieldKind::Concealed && value.len() < 4 {
        return Err("Concealed values must contain at least 4 bytes.".to_owned());
    }
    Ok(())
}

fn take_validated_value(
    value: &mut SecretInput,
    value_file: &str,
    kind: FieldKind,
) -> Result<SecretBytes, String> {
    if value_file.is_empty() {
        validate_value(kind, value)?;
        return Ok(value.take());
    }
    if !value.is_empty() {
        return Err("Choose either protected input or a value file, not both.".to_owned());
    }
    let mut loaded = SecretInput::from_regular_file(Path::new(value_file))
        .map_err(|error| sanitize_text(&error.to_string()))?;
    validate_value(kind, &loaded)?;
    Ok(loaded.take())
}

const fn toggled_kind(kind: FieldKind) -> FieldKind {
    match kind {
        FieldKind::Concealed => FieldKind::Text,
        FieldKind::Text => FieldKind::Concealed,
    }
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
    Form(ManagementForm),
    ConfirmMutation(MutationConfirmation),
    ConfirmDelete(DeleteConfirmation),
    Commands(CommandPalette),
    QuickAccess(QuickAccess),
    ToolForm(ToolForm),
    ImportPreview(ImportPreviewState),
    Activity(ActivityView),
    AuditResult(AuditVerification),
    ConfirmPeek(PeekConfirmation),
}

#[derive(Debug)]
pub(crate) struct ImportPreviewState {
    pub(crate) preview: ImportPreview,
    pub(crate) confirmation: LineEditor,
}

impl ImportPreviewState {
    pub(crate) fn required_confirmation(&self) -> &'static str {
        if self.preview.has_redaction_downgrade() {
            "IMPORT TEXT"
        } else {
            "IMPORT"
        }
    }

    fn invalid_confirmation_message(&self) -> &'static str {
        if self.preview.has_redaction_downgrade() {
            "Type IMPORT TEXT exactly to acknowledge the redaction downgrade and commit the previewed import."
        } else {
            "Type IMPORT exactly to resolve and commit the previewed import."
        }
    }
}

#[derive(Debug)]
pub(crate) struct ActivityView {
    pub(crate) activity: VerifiedVaultActivity,
    pub(crate) selected: usize,
}

#[derive(Debug)]
pub(crate) struct PeekConfirmation {
    pub(crate) reference: VaultReference,
    pub(crate) input: LineEditor,
}

#[derive(Debug)]
pub(crate) struct MutationConfirmation {
    pub(crate) kind: MutationConfirmationKind,
    submission: FormSubmission,
    pub(crate) input: LineEditor,
}

#[derive(Debug)]
pub(crate) enum MutationConfirmationKind {
    EmptyTextReplacement {
        reference: VaultReference,
        redaction_downgrade: bool,
    },
    RedactionDowngrade {
        reference: VaultReference,
    },
}

impl MutationConfirmationKind {
    const fn required_input(&self) -> &'static str {
        match self {
            Self::EmptyTextReplacement { .. } => "CLEAR",
            Self::RedactionDowngrade { .. } => "TEXT",
        }
    }

    const fn invalid_message(&self) -> &'static str {
        match self {
            Self::EmptyTextReplacement { .. } => {
                "Empty replacement confirmation must be CLEAR exactly."
            }
            Self::RedactionDowngrade { .. } => "Redaction downgrade must be TEXT exactly.",
        }
    }
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

#[derive(Clone, Debug)]
struct SelectionHint {
    item: ItemIdentity,
    entry: Option<EntryIdentity>,
}

#[derive(Debug)]
pub(crate) enum ManagementForm {
    WriteField {
        intent: FieldWriteIntent,
        item: LineEditor,
        field: LineEditor,
        kind: FieldKind,
        value: SecretInput,
        value_file: LineEditor,
        focus: FieldWriteFocus,
    },
    WriteLegacy {
        mode: VaultWriteMode,
        name: LineEditor,
        value: SecretInput,
        value_file: LineEditor,
        focus: LegacyWriteFocus,
    },
    ChangeKind {
        reference: VaultReference,
        from: FieldKind,
        to: FieldKind,
    },
    RenameField {
        source: VaultReference,
        destination_item: LineEditor,
        destination_field: LineEditor,
        focus: RenameFieldFocus,
    },
    RenameItem {
        source: String,
        destination: LineEditor,
    },
    ConvertLegacy {
        source: String,
        item: LineEditor,
        field: LineEditor,
        kind: FieldKind,
        focus: ConvertFocus,
    },
}

impl ManagementForm {
    fn write_field(
        intent: FieldWriteIntent,
        item: LineEditor,
        field: LineEditor,
        kind: FieldKind,
        focus: FieldWriteFocus,
    ) -> Self {
        Self::WriteField {
            intent,
            item,
            field,
            kind,
            value: SecretInput::new(),
            value_file: LineEditor::metadata(),
            focus,
        }
    }

    pub(crate) fn protected_input_mut(&mut self) -> Option<&mut SecretInput> {
        match self {
            Self::WriteField { value, focus, .. } if *focus == FieldWriteFocus::Value => {
                Some(value)
            }
            Self::WriteLegacy { value, focus, .. } if *focus == LegacyWriteFocus::Value => {
                Some(value)
            }
            _ => None,
        }
    }

    pub(crate) fn metadata_input_mut(&mut self) -> Option<&mut LineEditor> {
        match self {
            Self::WriteField {
                item,
                field,
                value_file,
                focus,
                ..
            } => match focus {
                FieldWriteFocus::Item => Some(item),
                FieldWriteFocus::Field => Some(field),
                FieldWriteFocus::File => Some(value_file),
                FieldWriteFocus::Kind | FieldWriteFocus::Value => None,
            },
            Self::WriteLegacy {
                name,
                value_file,
                focus,
                ..
            } => match focus {
                LegacyWriteFocus::Name => Some(name),
                LegacyWriteFocus::File => Some(value_file),
                LegacyWriteFocus::Value => None,
            },
            Self::RenameField {
                destination_item,
                destination_field,
                focus,
                ..
            } => match focus {
                RenameFieldFocus::Item => Some(destination_item),
                RenameFieldFocus::Field => Some(destination_field),
            },
            Self::RenameItem { destination, .. } => Some(destination),
            Self::ConvertLegacy {
                item, field, focus, ..
            } => match focus {
                ConvertFocus::Item => Some(item),
                ConvertFocus::Field => Some(field),
                ConvertFocus::Kind => None,
            },
            Self::ChangeKind { .. } => None,
        }
    }

    fn cycle_focus(&mut self, backwards: bool) {
        match self {
            Self::WriteField { focus, .. } => *focus = focus.cycle(backwards),
            Self::WriteLegacy { focus, .. } => *focus = focus.cycle(backwards),
            Self::RenameField { focus, .. } => *focus = focus.cycle(backwards),
            Self::ConvertLegacy { focus, .. } => *focus = focus.cycle(backwards),
            Self::ChangeKind { .. } | Self::RenameItem { .. } => {}
        }
    }

    fn toggle_kind(&mut self) {
        match self {
            Self::WriteField { kind, .. } | Self::ConvertLegacy { kind, .. } => {
                *kind = toggled_kind(*kind);
            }
            Self::ChangeKind { to, .. } => *to = toggled_kind(*to),
            _ => {}
        }
    }

    fn submission(&mut self, snapshot: &VaultSnapshot) -> Result<FormSubmission, String> {
        match self {
            Self::WriteField {
                intent,
                item,
                field,
                kind,
                value,
                value_file,
                ..
            } => {
                let reference = parse_reference(item.as_str(), field.as_str())?;
                intent.validate_destination(&reference, snapshot)?;
                let value = take_validated_value(value, value_file.as_str(), *kind)?;
                let mutation = VaultMutation::SetField {
                    reference: reference.clone(),
                    kind: *kind,
                    value,
                    mode: intent.write_mode(),
                };
                Ok(FormSubmission {
                    mutation,
                    label: intent.loading_label(),
                    selection: Some(SelectionHint {
                        item: ItemIdentity::Canonical(reference.item().to_owned()),
                        entry: Some(EntryIdentity::Field(reference)),
                    }),
                })
            }
            Self::WriteLegacy {
                mode,
                name,
                value,
                value_file,
                ..
            } => {
                let name = parse_legacy_name(name.as_str())?;
                let value = take_validated_value(value, value_file.as_str(), FieldKind::Concealed)?;
                Ok(FormSubmission {
                    mutation: VaultMutation::SetLegacy {
                        name: name.clone(),
                        value,
                        mode: *mode,
                    },
                    label: match mode {
                        VaultWriteMode::Create => "Creating legacy vault entry",
                        VaultWriteMode::Replace => "Replacing legacy vault entry",
                        VaultWriteMode::Upsert => "Writing legacy vault entry",
                    },
                    selection: Some(SelectionHint {
                        item: ItemIdentity::Legacy,
                        entry: Some(EntryIdentity::Legacy(name)),
                    }),
                })
            }
            Self::ChangeKind {
                reference,
                from,
                to,
            } => {
                if from == to {
                    return Err("Choose a different field kind before saving.".to_owned());
                }
                Ok(FormSubmission {
                    mutation: VaultMutation::ChangeFieldKind {
                        reference: reference.clone(),
                        kind: *to,
                    },
                    label: "Changing field kind",
                    selection: Some(SelectionHint {
                        item: ItemIdentity::Canonical(reference.item().to_owned()),
                        entry: Some(EntryIdentity::Field(reference.clone())),
                    }),
                })
            }
            Self::RenameField {
                source,
                destination_item,
                destination_field,
                ..
            } => {
                let destination =
                    parse_reference(destination_item.as_str(), destination_field.as_str())?;
                if &destination == source {
                    return Err("Field rename destination must differ from the source.".to_owned());
                }
                Ok(FormSubmission {
                    mutation: VaultMutation::RenameField {
                        source: source.clone(),
                        destination: destination.clone(),
                    },
                    label: "Renaming vault field",
                    selection: Some(SelectionHint {
                        item: ItemIdentity::Canonical(destination.item().to_owned()),
                        entry: Some(EntryIdentity::Field(destination)),
                    }),
                })
            }
            Self::RenameItem {
                source,
                destination,
            } => {
                let source = parse_item(source)?;
                let destination = parse_item(destination.as_str())?;
                if source == destination {
                    return Err("Item rename destination must differ from the source.".to_owned());
                }
                Ok(FormSubmission {
                    mutation: VaultMutation::RenameItem {
                        source,
                        destination: destination.clone(),
                    },
                    label: "Renaming vault item",
                    selection: Some(SelectionHint {
                        item: ItemIdentity::Canonical(destination.as_str().to_owned()),
                        entry: None,
                    }),
                })
            }
            Self::ConvertLegacy {
                source,
                item,
                field,
                kind,
                ..
            } => {
                let reference = parse_reference(item.as_str(), field.as_str())?;
                Ok(FormSubmission {
                    mutation: VaultMutation::ConvertLegacy {
                        name: source.clone(),
                        reference: reference.clone(),
                        kind: *kind,
                    },
                    label: "Converting legacy vault entry",
                    selection: Some(SelectionHint {
                        item: ItemIdentity::Canonical(reference.item().to_owned()),
                        entry: Some(EntryIdentity::Field(reference)),
                    }),
                })
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum FieldWriteIntent {
    CreateItem,
    AddField,
    ReplaceValue,
}

impl FieldWriteIntent {
    pub(crate) const fn title(self) -> &'static str {
        match self {
            Self::CreateItem => "Create item + first field",
            Self::AddField => "Add field",
            Self::ReplaceValue => "Replace field value",
        }
    }

    const fn loading_label(self) -> &'static str {
        match self {
            Self::CreateItem => "Creating vault item and first field",
            Self::AddField => "Creating vault field",
            Self::ReplaceValue => "Replacing vault field",
        }
    }

    const fn write_mode(self) -> VaultWriteMode {
        match self {
            Self::CreateItem | Self::AddField => VaultWriteMode::Create,
            Self::ReplaceValue => VaultWriteMode::Replace,
        }
    }

    fn validate_destination(
        self,
        reference: &VaultReference,
        snapshot: &VaultSnapshot,
    ) -> Result<(), String> {
        if self == Self::CreateItem
            && snapshot
                .fields
                .iter()
                .any(|field| field.reference.item() == reference.item())
        {
            return Err(format!(
                "Item jig://{} already exists; use Add field instead.",
                sanitize_text(reference.item())
            ));
        }
        Ok(())
    }
}

#[derive(Debug)]
struct FormSubmission {
    mutation: VaultMutation,
    label: &'static str,
    selection: Option<SelectionHint>,
}

impl FormSubmission {
    fn confirmation(&self, snapshot: &VaultSnapshot) -> Option<MutationConfirmationKind> {
        let redaction_downgrade = match &self.mutation {
            VaultMutation::SetField {
                reference,
                kind: FieldKind::Text,
                ..
            } if snapshot.fields.iter().any(|field| {
                field.reference == *reference && field.kind == FieldKind::Concealed
            }) =>
            {
                Some(reference.clone())
            }
            VaultMutation::ChangeFieldKind {
                reference,
                kind: FieldKind::Text,
            }
            | VaultMutation::ConvertLegacy {
                reference,
                kind: FieldKind::Text,
                ..
            } => Some(reference.clone()),
            _ => None,
        };

        match &self.mutation {
            VaultMutation::SetField {
                reference,
                kind: FieldKind::Text,
                value,
                mode: VaultWriteMode::Replace,
            } if value.is_empty() => Some(MutationConfirmationKind::EmptyTextReplacement {
                reference: reference.clone(),
                redaction_downgrade: redaction_downgrade.is_some(),
            }),
            _ => redaction_downgrade
                .map(|reference| MutationConfirmationKind::RedactionDowngrade { reference }),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum FieldWriteFocus {
    Item,
    Field,
    Kind,
    Value,
    File,
}

impl FieldWriteFocus {
    const fn cycle(self, backwards: bool) -> Self {
        match (self, backwards) {
            (Self::Item, false) | (Self::Kind, true) => Self::Field,
            (Self::Field, false) | (Self::Value, true) => Self::Kind,
            (Self::Kind, false) | (Self::File, true) => Self::Value,
            (Self::Value, false) | (Self::Item, true) => Self::File,
            (Self::File, false) | (Self::Field, true) => Self::Item,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum LegacyWriteFocus {
    Name,
    Value,
    File,
}

impl LegacyWriteFocus {
    const fn cycle(self, backwards: bool) -> Self {
        match (self, backwards) {
            (Self::Name, false) | (Self::File, true) => Self::Value,
            (Self::Value, false) | (Self::Name, true) => Self::File,
            (Self::File, false) | (Self::Value, true) => Self::Name,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RenameFieldFocus {
    Item,
    Field,
}

impl RenameFieldFocus {
    const fn cycle(self, _backwards: bool) -> Self {
        match self {
            Self::Item => Self::Field,
            Self::Field => Self::Item,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ConvertFocus {
    Item,
    Field,
    Kind,
}

impl ConvertFocus {
    const fn cycle(self, backwards: bool) -> Self {
        match (self, backwards) {
            (Self::Item, false) | (Self::Kind, true) => Self::Field,
            (Self::Field, false) | (Self::Item, true) => Self::Kind,
            (Self::Kind, false) | (Self::Field, true) => Self::Item,
        }
    }
}

#[derive(Debug)]
pub(crate) struct DeleteConfirmation {
    pub(crate) target: DeleteTarget,
    pub(crate) input: LineEditor,
}

#[derive(Debug)]
pub(crate) enum DeleteTarget {
    Field(VaultReference),
    Item { item: String, count: usize },
    Legacy(String),
}

impl DeleteTarget {
    pub(crate) fn display_label(&self) -> String {
        let label = match self {
            Self::Field(reference) => format!("field {reference}"),
            Self::Item { item, count } => format!("item jig://{item} and its {count} fields"),
            Self::Legacy(name) => format!("legacy entry {name}"),
        };
        sanitize_text(&label)
    }

    pub(crate) fn required_confirmation(&self) -> String {
        match self {
            Self::Field(reference) => reference.to_string(),
            Self::Item { .. } => "DELETE".to_owned(),
            Self::Legacy(name) if sanitize_text(name) == *name => name.clone(),
            Self::Legacy(_) => "DELETE".to_owned(),
        }
    }

    fn into_mutation(self) -> (VaultMutation, &'static str) {
        match self {
            Self::Field(reference) => (
                VaultMutation::RemoveField { reference },
                "Removing vault field",
            ),
            Self::Item { item, .. } => (
                VaultMutation::RemoveItem {
                    item: VaultItem::parse(&format!("jig://{item}"))
                        .expect("selected item identity remains valid"),
                },
                "Removing vault item",
            ),
            Self::Legacy(name) => (
                VaultMutation::RemoveLegacy { name },
                "Removing legacy vault entry",
            ),
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
