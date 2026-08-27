use std::{cell::Cell, collections::BTreeMap};

use jig_tui::{FuzzyMatchScore, PreparedFuzzyText, RankedFuzzyText, best_ranked_fuzzy_match};
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

#[derive(Debug)]
struct QuickAccessEntry {
    target: QuickAccessTarget,
    search_terms: Vec<RankedFuzzyText>,
}

impl QuickAccessEntry {
    fn new(target: QuickAccessTarget) -> Self {
        let search_terms = match &target {
            QuickAccessTarget::Item { item, .. } => vec![
                RankedFuzzyText::new(0, item),
                RankedFuzzyText::new(1, &format!("jig://{item}")),
                RankedFuzzyText::new(2, "item"),
            ],
            QuickAccessTarget::Field { reference, kind } => vec![
                RankedFuzzyText::new(0, reference.field()),
                RankedFuzzyText::new(1, reference.item()),
                RankedFuzzyText::new(2, &reference.to_string()),
                RankedFuzzyText::new(3, kind.as_str()),
                RankedFuzzyText::new(4, "field"),
            ],
            QuickAccessTarget::LegacyGroup { .. } => vec![
                RankedFuzzyText::new(0, "legacy"),
                RankedFuzzyText::new(1, "legacy entries"),
                RankedFuzzyText::new(2, "group"),
            ],
            QuickAccessTarget::LegacyEntry { name } => vec![
                RankedFuzzyText::new(0, name),
                RankedFuzzyText::new(1, "legacy"),
                RankedFuzzyText::new(2, "legacy entry"),
            ],
        };
        Self {
            target,
            search_terms,
        }
    }

    fn match_score(&self, query: &PreparedFuzzyText) -> Option<(usize, FuzzyMatchScore)> {
        best_ranked_fuzzy_match(&self.search_terms, query)
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
    entries: Vec<QuickAccessEntry>,
    query: LineEditor,
    visible_indices: Vec<usize>,
    selected: usize,
    list_offset: Cell<usize>,
    list_viewport_height: Cell<u16>,
}

impl QuickAccess {
    pub(crate) fn for_app(app: &App) -> Self {
        let mut targets = Vec::new();
        if let Some(snapshot) = app.snapshot() {
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
        let entries = targets
            .into_iter()
            .map(QuickAccessEntry::new)
            .collect::<Vec<_>>();
        let selected = entries
            .iter()
            .position(|entry| entry.target.matches_app_selection(app))
            .unwrap_or(0);
        let visible_indices = (0..entries.len()).collect();
        Self {
            entries,
            query: LineEditor::search(),
            visible_indices,
            selected,
            list_offset: Cell::new(0),
            list_viewport_height: Cell::new(0),
        }
    }

    pub(crate) fn query(&self) -> &LineEditor {
        &self.query
    }

    pub(crate) fn visible_targets(&self) -> impl Iterator<Item = &QuickAccessTarget> {
        self.visible_indices
            .iter()
            .map(|index| &self.entries[*index].target)
    }

    pub(crate) fn visible_len(&self) -> usize {
        self.visible_indices.len()
    }

    pub(crate) fn selected_row(&self) -> Option<usize> {
        (!self.visible_indices.is_empty()).then_some(self.selected)
    }

    pub(crate) fn selected_target(&self) -> Option<&QuickAccessTarget> {
        self.visible_indices
            .get(self.selected)
            .map(|index| &self.entries[*index].target)
    }

    pub(crate) fn move_selection(&mut self, delta: isize) {
        let len = self.visible_indices.len();
        if len == 0 {
            self.selected = 0;
        } else {
            self.selected = self.selected.saturating_add_signed(delta).min(len - 1);
        }
    }

    pub(crate) fn move_to_edge(&mut self, end: bool) {
        let len = self.visible_indices.len();
        self.selected = if end { len.saturating_sub(1) } else { 0 };
    }

    pub(crate) fn append_query(&mut self, value: &str) -> bool {
        let previous_len = self.query.as_str().len();
        if !self.query.insert(value) {
            return false;
        }
        if self.query.as_str().len() != previous_len {
            self.refresh_query_results();
        }
        true
    }

    pub(crate) fn edit_query(&mut self, edit: LineEdit) {
        let previous_len = self.query.as_str().len();
        self.query.apply(edit);
        if edit.changes_text() && self.query.as_str().len() != previous_len {
            self.refresh_query_results();
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

    fn refresh_query_results(&mut self) {
        self.visible_indices = if self.query.is_empty() {
            (0..self.entries.len()).collect()
        } else {
            let query = PreparedFuzzyText::new(self.query.as_str());
            let mut matches = self
                .entries
                .iter()
                .enumerate()
                .filter_map(|(index, entry)| entry.match_score(&query).map(|score| (score, index)))
                .collect::<Vec<_>>();
            matches.sort_by_key(|(score, index)| (*score, *index));
            matches.into_iter().map(|(_, index)| index).collect()
        };
        self.selected = 0;
        self.list_offset.set(0);
    }
}
