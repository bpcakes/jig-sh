use std::collections::BTreeMap;

use jig_vault::{FieldKind, FieldRecord, SecretRecord, VaultSnapshot};

use crate::model::{EntryIdentity, ItemIdentity};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum BrowseEntryKind {
    Field(FieldKind),
    Legacy,
}

#[derive(Debug)]
pub(crate) struct BrowseState {
    snapshot: VaultSnapshot,
    items: Vec<BrowseItem>,
    visible_item_indices: Vec<usize>,
    #[cfg(test)]
    filter_refreshes: usize,
}

impl BrowseState {
    pub(crate) fn new(snapshot: VaultSnapshot, query: &str) -> Self {
        let mut canonical = BTreeMap::<String, Vec<BrowseEntry>>::new();
        for field in &snapshot.fields {
            canonical
                .entry(field.reference.item().to_owned())
                .or_default()
                .push(BrowseEntry::field(field));
        }
        let mut items = canonical
            .into_iter()
            .map(|(item, entries)| BrowseItem::canonical(item, entries))
            .collect::<Vec<_>>();
        if !snapshot.legacy_secrets.is_empty() {
            items.push(BrowseItem::legacy(&snapshot.legacy_secrets));
        }
        let mut state = Self {
            snapshot,
            items,
            visible_item_indices: Vec::new(),
            #[cfg(test)]
            filter_refreshes: 0,
        };
        state.refresh_filter(query);
        state
    }

    pub(crate) const fn snapshot(&self) -> &VaultSnapshot {
        &self.snapshot
    }

    pub(crate) fn refresh_filter(&mut self, query: &str) {
        let query = query.to_lowercase();
        self.visible_item_indices.clear();
        for (index, item) in self.items.iter_mut().enumerate() {
            if item.refresh_filter(&query) {
                self.visible_item_indices.push(index);
            }
        }
        #[cfg(test)]
        {
            self.filter_refreshes += 1;
        }
    }

    pub(crate) fn visible_items(&self) -> Vec<ItemIdentity> {
        self.visible_item_indices
            .iter()
            .map(|index| self.items[*index].identity.clone())
            .collect()
    }

    pub(crate) fn visible_item_rows(&self) -> Vec<(ItemIdentity, usize)> {
        self.visible_item_indices
            .iter()
            .map(|index| {
                let item = &self.items[*index];
                (item.identity.clone(), item.entries.len())
            })
            .collect()
    }

    pub(crate) fn visible_entries(&self, item: Option<&ItemIdentity>) -> Vec<EntryIdentity> {
        self.item(item)
            .into_iter()
            .flat_map(BrowseItem::visible_entries)
            .map(|entry| entry.identity.clone())
            .collect()
    }

    pub(crate) fn visible_entry_rows(
        &self,
        item: Option<&ItemIdentity>,
    ) -> Vec<(EntryIdentity, BrowseEntryKind)> {
        self.item(item)
            .into_iter()
            .flat_map(BrowseItem::visible_entries)
            .map(|entry| (entry.identity.clone(), entry.kind))
            .collect()
    }

    pub(crate) fn item_entry_count(&self, identity: &ItemIdentity) -> usize {
        self.items
            .iter()
            .find(|item| &item.identity == identity)
            .map_or(0, |item| item.entries.len())
    }

    pub(crate) fn counts(&self) -> (usize, usize, usize) {
        (
            self.items
                .iter()
                .filter(|item| matches!(item.identity, ItemIdentity::Canonical(_)))
                .count(),
            self.snapshot.fields.len(),
            self.snapshot.legacy_secrets.len(),
        )
    }

    #[cfg(test)]
    pub(crate) const fn filter_refreshes(&self) -> usize {
        self.filter_refreshes
    }

    fn item(&self, identity: Option<&ItemIdentity>) -> Option<&BrowseItem> {
        let identity = identity?;
        self.items.iter().find(|item| &item.identity == identity)
    }
}

#[derive(Debug)]
struct BrowseItem {
    identity: ItemIdentity,
    group_search_term: String,
    entries: Vec<BrowseEntry>,
    visible_entry_indices: Vec<usize>,
}

impl BrowseItem {
    fn canonical(item: String, entries: Vec<BrowseEntry>) -> Self {
        Self {
            identity: ItemIdentity::Canonical(item.clone()),
            group_search_term: item.to_lowercase(),
            entries,
            visible_entry_indices: Vec::new(),
        }
    }

    fn legacy(records: &[SecretRecord]) -> Self {
        Self {
            identity: ItemIdentity::Legacy,
            group_search_term: "legacy".to_owned(),
            entries: records.iter().map(BrowseEntry::legacy).collect(),
            visible_entry_indices: Vec::new(),
        }
    }

    fn refresh_filter(&mut self, query: &str) -> bool {
        self.visible_entry_indices = if query.is_empty() {
            (0..self.entries.len()).collect()
        } else {
            self.entries
                .iter()
                .enumerate()
                .filter_map(|(index, entry)| entry.matches(query).then_some(index))
                .collect()
        };
        query.is_empty()
            || self.group_search_term.contains(query)
            || !self.visible_entry_indices.is_empty()
    }

    fn visible_entries(&self) -> impl Iterator<Item = &BrowseEntry> {
        self.visible_entry_indices
            .iter()
            .map(|index| &self.entries[*index])
    }
}

#[derive(Debug)]
struct BrowseEntry {
    identity: EntryIdentity,
    kind: BrowseEntryKind,
    search_terms: Vec<String>,
}

impl BrowseEntry {
    fn field(record: &FieldRecord) -> Self {
        Self {
            identity: EntryIdentity::Field(record.reference.clone()),
            kind: BrowseEntryKind::Field(record.kind),
            search_terms: vec![
                record.reference.item().to_lowercase(),
                record.reference.field().to_lowercase(),
                record.reference.to_string().to_lowercase(),
                record.kind.as_str().to_owned(),
            ],
        }
    }

    fn legacy(record: &SecretRecord) -> Self {
        Self {
            identity: EntryIdentity::Legacy(record.name.clone()),
            kind: BrowseEntryKind::Legacy,
            search_terms: vec![record.name.to_lowercase()],
        }
    }

    fn matches(&self, query: &str) -> bool {
        self.search_terms.iter().any(|term| term.contains(query))
    }
}
