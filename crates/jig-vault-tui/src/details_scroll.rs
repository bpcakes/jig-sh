use std::cell::{Cell, RefCell};

use crate::model::EntryIdentity;

#[derive(Debug, Default)]
pub(crate) struct DetailsScroll {
    identity: RefCell<Option<EntryIdentity>>,
    offset: Cell<u16>,
    maximum: Cell<u16>,
}

impl DetailsScroll {
    pub(crate) fn position(
        &self,
        identity: &Option<EntryIdentity>,
        lines: usize,
        height: u16,
    ) -> u16 {
        if *self.identity.borrow() != *identity {
            self.identity.replace(identity.clone());
            self.offset.set(0);
        }
        let maximum = lines
            .saturating_sub(usize::from(height))
            .min(usize::from(u16::MAX)) as u16;
        self.maximum.set(maximum);
        self.offset.set(self.offset.get().min(maximum));
        self.offset.get()
    }

    pub(crate) fn move_by(&self, delta: isize) {
        self.offset.set(
            usize::from(self.offset.get())
                .saturating_add_signed(delta)
                .min(usize::from(self.maximum.get())) as u16,
        );
    }

    pub(crate) fn to_edge(&self, end: bool) {
        self.offset.set(if end { self.maximum.get() } else { 0 });
    }
}
