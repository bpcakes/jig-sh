use std::{cell::Cell, collections::BTreeMap};

use jig_tui::{FuzzyMatchScore, fuzzy_match_score};
use jig_vault::{FieldKind, VaultReference};

use crate::{
    line_editor::{LineEdit, LineEditor},
    model::{App, EntryIdentity, Focus, ItemIdentity},
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum QuickAccessTarget {
    Item {
        item: String,
        field_count: usize,
    },
    Field {
        reference: VaultReference,
        kind: FieldKind,
    },
    LegacyGroup {
        entry_count: usize,
    },
    LegacyEntry {
        name: String,
    },
}

impl QuickAccessTarget {
    pub(crate) const fn badge(&self) -> &'static str {
        match self {
            Self::Item { .. } => "ITEM",
            Self::Field { .. } => "FIELD",
            Self::LegacyGroup { .. } => "GROUP",
            Self::LegacyEntry { .. } => "LEGACY",
        }
    }

    pub(crate) fn title(&self) -> &str {
        match self {
            Self::Item { item, .. } => item,
            Self::Field { reference, .. } => reference.field(),
            Self::LegacyGroup { .. } => "Legacy",
            Self::LegacyEntry { name } => name,
        }
    }

    fn match_score(&self, query: &str) -> Option<(usize, FuzzyMatchScore)> {
        let fields = match self {
            Self::Item { item, .. } => vec![
                (0, item.clone()),
                (1, format!("jig://{item}")),
                (2, "item".to_owned()),
            ],
            Self::Field { reference, kind } => vec![
                (0, reference.field().to_owned()),
                (1, reference.item().to_owned()),
                (2, reference.to_string()),
                (3, kind.as_str().to_owned()),
                (4, "field".to_owned()),
            ],
            Self::LegacyGroup { .. } => vec![
                (0, "legacy".to_owned()),
                (1, "legacy entries".to_owned()),
                (2, "group".to_owned()),
            ],
            Self::LegacyEntry { name } => vec![
                (0, name.clone()),
                (1, "legacy".to_owned()),
                (2, "legacy entry".to_owned()),
            ],
        };
        fields
            .into_iter()
            .filter_map(|(priority, field)| {
                fuzzy_match_score(&field, query).map(|score| (priority, score))
            })
            .min()
    }

    fn matches_app_selection(&self, app: &App) -> bool {
        match (self, app.focus) {
            (Self::Item { item, .. }, Focus::Items) => {
                app.selected_item == Some(ItemIdentity::Canonical(item.clone()))
            }
            (Self::LegacyGroup { .. }, Focus::Items) => {
                app.selected_item == Some(ItemIdentity::Legacy)
            }
            (Self::Field { reference, .. }, Focus::Fields | Focus::Details) => {
                app.selected_entry == Some(EntryIdentity::Field(reference.clone()))
            }
            (Self::LegacyEntry { name }, Focus::Fields | Focus::Details) => {
                app.selected_entry == Some(EntryIdentity::Legacy(name.clone()))
            }
            _ => false,
        }
    }

    pub(crate) fn selection(&self) -> QuickAccessSelection {
        match self {
            Self::Item { item, .. } => {
                QuickAccessSelection::Item(ItemIdentity::Canonical(item.clone()))
            }
            Self::Field { reference, .. } => QuickAccessSelection::Entry {
                item: ItemIdentity::Canonical(reference.item().to_owned()),
                entry: EntryIdentity::Field(reference.clone()),
            },
            Self::LegacyGroup { .. } => QuickAccessSelection::Item(ItemIdentity::Legacy),
            Self::LegacyEntry { name } => QuickAccessSelection::Entry {
                item: ItemIdentity::Legacy,
                entry: EntryIdentity::Legacy(name.clone()),
            },
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum QuickAccessSelection {
    Item(ItemIdentity),
    Entry {
        item: ItemIdentity,
        entry: EntryIdentity,
    },
}

#[derive(Debug)]
pub(crate) struct QuickAccess {
    pub(crate) targets: Vec<QuickAccessTarget>,
    pub(crate) query: LineEditor,
    pub(crate) selected: usize,
    list_offset: Cell<usize>,
    list_viewport_height: Cell<u16>,
}

impl QuickAccess {
    pub(crate) fn for_app(app: &App) -> Self {
        let mut targets = Vec::new();
        if let Some(snapshot) = &app.snapshot {
            let mut items = BTreeMap::new();
            for field in &snapshot.fields {
                *items
                    .entry(field.reference.item().to_owned())
                    .or_insert(0usize) += 1;
            }
            targets.extend(
                items
                    .into_iter()
                    .map(|(item, field_count)| QuickAccessTarget::Item { item, field_count }),
            );

            let mut fields = snapshot.fields.iter().collect::<Vec<_>>();
            fields.sort_by_key(|field| field.reference.to_string());
            targets.extend(fields.into_iter().map(|field| QuickAccessTarget::Field {
                reference: field.reference.clone(),
                kind: field.kind,
            }));

            if !snapshot.legacy_secrets.is_empty() {
                targets.push(QuickAccessTarget::LegacyGroup {
                    entry_count: snapshot.legacy_secrets.len(),
                });
                let mut legacy = snapshot.legacy_secrets.iter().collect::<Vec<_>>();
                legacy.sort_by(|left, right| left.name.cmp(&right.name));
                targets.extend(
                    legacy
                        .into_iter()
                        .map(|entry| QuickAccessTarget::LegacyEntry {
                            name: entry.name.clone(),
                        }),
                );
            }
        }
        let selected = targets
            .iter()
            .position(|target| target.matches_app_selection(app))
            .unwrap_or(0);
        Self {
            targets,
            query: LineEditor::search(),
            selected,
            list_offset: Cell::new(0),
            list_viewport_height: Cell::new(0),
        }
    }

    pub(crate) fn visible_indices(&self) -> Vec<usize> {
        let query = self.query.as_str();
        if query.is_empty() {
            return (0..self.targets.len()).collect();
        }
        let mut matches = self
            .targets
            .iter()
            .enumerate()
            .filter_map(|(index, target)| target.match_score(query).map(|score| (score, index)))
            .collect::<Vec<_>>();
        matches.sort_by_key(|(score, index)| (*score, *index));
        matches.into_iter().map(|(_, index)| index).collect()
    }

    pub(crate) fn selected_target(&self) -> Option<&QuickAccessTarget> {
        self.visible_indices()
            .get(self.selected)
            .and_then(|index| self.targets.get(*index))
    }

    pub(crate) fn move_selection(&mut self, delta: isize) {
        let len = self.visible_indices().len();
        if len == 0 {
            self.selected = 0;
        } else {
            self.selected = self.selected.saturating_add_signed(delta).min(len - 1);
        }
    }

    pub(crate) fn move_to_edge(&mut self, end: bool) {
        let len = self.visible_indices().len();
        self.selected = if end { len.saturating_sub(1) } else { 0 };
    }

    pub(crate) fn append_query(&mut self, value: &str) -> bool {
        if !self.query.insert(value) {
            return false;
        }
        self.selected = 0;
        self.list_offset.set(0);
        true
    }

    pub(crate) fn edit_query(&mut self, edit: LineEdit) {
        self.query.apply(edit);
        if edit.changes_text() {
            self.selected = 0;
            self.list_offset.set(0);
        } else {
            self.reconcile_selection();
        }
    }

    pub(crate) fn list_offset_for_viewport(&self, height: u16) -> usize {
        if self.list_viewport_height.replace(height) != height {
            self.list_offset.set(0);
        }
        self.list_offset.get()
    }

    pub(crate) fn set_list_offset(&self, offset: usize) {
        self.list_offset.set(offset);
    }

    fn reconcile_selection(&mut self) {
        self.selected = self
            .selected
            .min(self.visible_indices().len().saturating_sub(1));
    }
}
