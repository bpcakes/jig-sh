use super::{App, Focus, move_identity};

impl App {
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
            Focus::Details => self.details_scroll.move_by(delta),
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
            Focus::Details => self.details_scroll.to_edge(end),
        }
    }
}
