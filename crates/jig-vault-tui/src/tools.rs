use std::path::PathBuf;

use jig_vault::{MIN_MASTER_PASSPHRASE_LEN, VaultItem, VaultReference};

use crate::{VaultAction, line_editor::LineEditor, secret_input::SecretInput};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ToolChoice {
    Activity,
    VerifyAudit,
    ImportOnePassword,
    CreateBackup,
    ChangePassphrase,
    RestoreBackup,
}

impl ToolChoice {
    pub(crate) fn activation(self) -> ToolActivation {
        match self {
            Self::Activity => ToolActivation::Immediate {
                action: VaultAction::Activity { limit: 100 },
                loading_label: "Loading verified vault activity",
            },
            Self::VerifyAudit => ToolActivation::Immediate {
                action: VaultAction::VerifyAudit,
                loading_label: "Verifying vault audit chain",
            },
            Self::ImportOnePassword => ToolActivation::Form(ToolForm::ImportOnePassword {
                env_file: LineEditor::metadata(),
                item: LineEditor::metadata(),
                out_env: LineEditor::metadata(),
                replace: false,
                overwrite: false,
                dry_run: false,
                focus: ImportFocus::EnvFile,
            }),
            Self::CreateBackup => ToolActivation::Form(ToolForm::CreateBackup {
                output: LineEditor::metadata(),
                overwrite: false,
                focus: BackupFocus::Output,
            }),
            Self::ChangePassphrase => ToolActivation::Form(ToolForm::ChangePassphrase {
                new_passphrase: SecretInput::new(),
                confirmation: SecretInput::new(),
                focus: PassphraseFocus::New,
            }),
            Self::RestoreBackup => ToolActivation::Form(ToolForm::RestoreBackup {
                input: LineEditor::metadata(),
                passphrase: SecretInput::new(),
                confirmation: LineEditor::metadata(),
                focus: RestoreFocus::Input,
            }),
        }
    }
}

pub(crate) enum ToolActivation {
    Immediate {
        action: VaultAction,
        loading_label: &'static str,
    },
    Form(ToolForm),
}

#[derive(Debug)]
pub(crate) enum ToolForm {
    ExportField {
        reference: VaultReference,
        output: LineEditor,
        overwrite: bool,
        focus: ExportFocus,
    },
    ImportOnePassword {
        env_file: LineEditor,
        item: LineEditor,
        out_env: LineEditor,
        replace: bool,
        overwrite: bool,
        dry_run: bool,
        focus: ImportFocus,
    },
    CreateBackup {
        output: LineEditor,
        overwrite: bool,
        focus: BackupFocus,
    },
    ChangePassphrase {
        new_passphrase: SecretInput,
        confirmation: SecretInput,
        focus: PassphraseFocus,
    },
    RestoreBackup {
        input: LineEditor,
        passphrase: SecretInput,
        confirmation: LineEditor,
        focus: RestoreFocus,
    },
}

impl ToolForm {
    pub(crate) fn export_field(reference: VaultReference) -> Self {
        Self::ExportField {
            reference,
            output: LineEditor::metadata(),
            overwrite: false,
            focus: ExportFocus::Output,
        }
    }

    pub(crate) fn protected_input_mut(&mut self) -> Option<&mut SecretInput> {
        match self {
            Self::ChangePassphrase {
                new_passphrase,
                confirmation,
                focus,
            } => Some(match focus {
                PassphraseFocus::New => new_passphrase,
                PassphraseFocus::Confirmation => confirmation,
            }),
            Self::RestoreBackup {
                passphrase, focus, ..
            } if *focus == RestoreFocus::Passphrase => Some(passphrase),
            Self::ExportField { .. }
            | Self::ImportOnePassword { .. }
            | Self::CreateBackup { .. }
            | Self::RestoreBackup { .. } => None,
        }
    }

    pub(crate) fn metadata_input_mut(&mut self) -> Option<&mut LineEditor> {
        match self {
            Self::ExportField { output, focus, .. } => match focus {
                ExportFocus::Output => Some(output),
                ExportFocus::Overwrite => None,
            },
            Self::ImportOnePassword {
                env_file,
                item,
                out_env,
                focus,
                ..
            } => match focus {
                ImportFocus::EnvFile => Some(env_file),
                ImportFocus::Item => Some(item),
                ImportFocus::OutEnv => Some(out_env),
                ImportFocus::Replace | ImportFocus::Overwrite | ImportFocus::DryRun => None,
            },
            Self::CreateBackup { output, focus, .. } => match focus {
                BackupFocus::Output => Some(output),
                BackupFocus::Overwrite => None,
            },
            Self::RestoreBackup {
                input,
                confirmation,
                focus,
                ..
            } => match focus {
                RestoreFocus::Input => Some(input),
                RestoreFocus::Confirmation => Some(confirmation),
                RestoreFocus::Passphrase => None,
            },
            Self::ChangePassphrase { .. } => None,
        }
    }

    pub(crate) fn cycle_focus(&mut self, backwards: bool) {
        match self {
            Self::ExportField { focus, .. } => *focus = focus.cycle(backwards),
            Self::ImportOnePassword { focus, .. } => *focus = focus.cycle(backwards),
            Self::CreateBackup { focus, .. } => *focus = focus.cycle(backwards),
            Self::ChangePassphrase { focus, .. } => *focus = focus.cycle(),
            Self::RestoreBackup { focus, .. } => *focus = focus.cycle(backwards),
        }
    }

    pub(crate) fn toggle_choice(&mut self) {
        match self {
            Self::ExportField {
                overwrite, focus, ..
            } if *focus == ExportFocus::Overwrite => *overwrite = !*overwrite,
            Self::ImportOnePassword {
                replace,
                overwrite,
                dry_run,
                focus,
                ..
            } => match focus {
                ImportFocus::Replace => *replace = !*replace,
                ImportFocus::Overwrite => *overwrite = !*overwrite,
                ImportFocus::DryRun => *dry_run = !*dry_run,
                _ => {}
            },
            Self::CreateBackup {
                overwrite, focus, ..
            } if *focus == BackupFocus::Overwrite => *overwrite = !*overwrite,
            _ => {}
        }
    }

    pub(crate) fn submission(&mut self) -> Result<ToolSubmission, String> {
        match self {
            Self::ExportField {
                reference,
                output,
                overwrite,
                ..
            } => Ok(ToolSubmission {
                action: VaultAction::ExportField {
                    reference: reference.clone(),
                    output: required_file_path(output.as_str(), "Export output")?,
                    overwrite: *overwrite,
                },
                label: "Exporting vault field to private file",
            }),
            Self::ImportOnePassword {
                env_file,
                item,
                out_env,
                replace,
                overwrite,
                dry_run,
                ..
            } => {
                let env_file = required_file_path(env_file.as_str(), "Import source")?;
                let out_env = required_file_path(out_env.as_str(), "Generated dotenv destination")?;
                let item = VaultItem::parse(&format!("jig://{}", item.as_str()))
                    .map_err(|error| jig_tui::sanitize_text(error.message()))?;
                Ok(ToolSubmission {
                    action: VaultAction::PreviewOnePasswordImport {
                        env_file,
                        item,
                        out_env,
                        replace: *replace,
                        overwrite: *overwrite,
                        dry_run: *dry_run,
                    },
                    label: if *dry_run {
                        "Preparing 1Password dry-run preview"
                    } else {
                        "Preparing 1Password import preview"
                    },
                })
            }
            Self::CreateBackup {
                output, overwrite, ..
            } => Ok(ToolSubmission {
                action: VaultAction::CreateBackup {
                    output: required_file_path(output.as_str(), "Backup output")?,
                    overwrite: *overwrite,
                },
                label: "Creating encrypted vault backup",
            }),
            Self::ChangePassphrase {
                new_passphrase,
                confirmation,
                ..
            } => {
                if new_passphrase.len() < MIN_MASTER_PASSPHRASE_LEN {
                    return Err(format!(
                        "New vault passphrases must contain at least {MIN_MASTER_PASSPHRASE_LEN} bytes."
                    ));
                }
                if !new_passphrase.matches(confirmation) {
                    return Err("New vault passphrase confirmation did not match.".to_owned());
                }
                let new_passphrase = new_passphrase.take();
                confirmation.clear();
                Ok(ToolSubmission {
                    action: VaultAction::ChangePassphrase { new_passphrase },
                    label: "Changing vault passphrase",
                })
            }
            Self::RestoreBackup {
                input,
                passphrase,
                confirmation,
                ..
            } => {
                if passphrase.is_empty() {
                    return Err("Enter the backup vault passphrase first.".to_owned());
                }
                if confirmation.as_str() != "RESTORE" {
                    return Err("Type RESTORE exactly to install the absent vault.".to_owned());
                }
                Ok(ToolSubmission {
                    action: VaultAction::RestoreBackup {
                        input: required_file_path(input.as_str(), "Backup input")?,
                        passphrase: passphrase.take(),
                    },
                    label: "Restoring encrypted vault backup",
                })
            }
        }
    }
}

fn required_file_path(value: &str, label: &str) -> Result<PathBuf, String> {
    if value.is_empty() {
        return Err(format!("{label} path is required."));
    }
    if value == "-" {
        return Err(format!(
            "{label} must be a file path; stdin/stdout is unsupported."
        ));
    }
    Ok(PathBuf::from(value))
}

pub(crate) struct ToolSubmission {
    pub(crate) action: VaultAction,
    pub(crate) label: &'static str,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ExportFocus {
    Output,
    Overwrite,
}

impl ExportFocus {
    const fn cycle(self, _backwards: bool) -> Self {
        match self {
            Self::Output => Self::Overwrite,
            Self::Overwrite => Self::Output,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ImportFocus {
    EnvFile,
    Item,
    OutEnv,
    Replace,
    Overwrite,
    DryRun,
}

impl ImportFocus {
    fn cycle(self, backwards: bool) -> Self {
        const ORDER: [ImportFocus; 6] = [
            ImportFocus::EnvFile,
            ImportFocus::Item,
            ImportFocus::OutEnv,
            ImportFocus::Replace,
            ImportFocus::Overwrite,
            ImportFocus::DryRun,
        ];
        cycle_value(self, backwards, &ORDER)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum BackupFocus {
    Output,
    Overwrite,
}

impl BackupFocus {
    const fn cycle(self, _backwards: bool) -> Self {
        match self {
            Self::Output => Self::Overwrite,
            Self::Overwrite => Self::Output,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PassphraseFocus {
    New,
    Confirmation,
}

impl PassphraseFocus {
    const fn cycle(self) -> Self {
        match self {
            Self::New => Self::Confirmation,
            Self::Confirmation => Self::New,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RestoreFocus {
    Input,
    Passphrase,
    Confirmation,
}

impl RestoreFocus {
    fn cycle(self, backwards: bool) -> Self {
        const ORDER: [RestoreFocus; 3] = [
            RestoreFocus::Input,
            RestoreFocus::Passphrase,
            RestoreFocus::Confirmation,
        ];
        cycle_value(self, backwards, &ORDER)
    }
}

fn cycle_value<T: Copy + Eq, const N: usize>(current: T, backwards: bool, order: &[T; N]) -> T {
    let mut index = 0;
    while index < N {
        if order[index] == current {
            let next = if backwards {
                if index == 0 { N - 1 } else { index - 1 }
            } else if index + 1 == N {
                0
            } else {
                index + 1
            };
            return order[next];
        }
        index += 1;
    }
    current
}
