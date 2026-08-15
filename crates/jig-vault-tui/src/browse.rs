use jig_vault::VaultSnapshot;

#[derive(Debug)]
pub(crate) struct BrowseState {
    snapshot: VaultSnapshot,
}

impl BrowseState {
    pub(crate) const fn new(snapshot: VaultSnapshot) -> Self {
        Self { snapshot }
    }

    pub(crate) const fn snapshot(&self) -> &VaultSnapshot {
        &self.snapshot
    }
}
