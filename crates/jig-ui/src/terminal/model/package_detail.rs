use std::cell::Cell;

use jig_contract::status_provider::v1::AcceptanceCheck;

use super::{App, PackageView, SourceView, Tab};
#[cfg(test)]
use super::{fallback, wire::AcceptanceCheckWire};

const DETAIL_VISIBLE_ROW_FLOOR: usize = 10;
pub(crate) const DETAIL_SECTION_ITEM_LIMIT: usize = 500;
pub(crate) const EXTENSION_ROW_LIMIT: usize = 200;

#[derive(Clone, Debug)]
pub(crate) struct AcceptanceCheckView {
    pub(crate) ordinal: u64,
    pub(crate) id: Option<String>,
    pub(crate) state: String,
    pub(crate) category: String,
    pub(crate) target: Option<String>,
    pub(crate) source: Option<SourceView>,
}

#[cfg(test)]
impl From<AcceptanceCheckWire> for AcceptanceCheckView {
    fn from(wire: AcceptanceCheckWire) -> Self {
        Self {
            ordinal: wire.ordinal,
            id: wire.id,
            state: fallback(wire.state, "unknown"),
            category: fallback(wire.category, "unknown"),
            target: wire.target,
            source: wire.source.map(Into::into),
        }
    }
}

impl From<AcceptanceCheck> for AcceptanceCheckView {
    fn from(check: AcceptanceCheck) -> Self {
        Self {
            ordinal: u64::from(check.ordinal),
            id: check.id.as_deref().map(jig_tui::sanitize_text),
            state: jig_tui::sanitize_text(&check.state),
            category: super::category_name(check.category).to_string(),
            target: check.target.as_deref().map(jig_tui::sanitize_text),
            source: check.source.map(Into::into),
        }
    }
}

#[derive(Debug, Default)]
pub(crate) struct PackageDetailState {
    provider_id: Option<String>,
    package_id: Option<String>,
    scroll: usize,
    scroll_limit: Cell<usize>,
}

impl App {
    pub(crate) fn open_package_detail(&mut self) -> bool {
        if self.tab != Tab::Packages {
            return false;
        }
        let Some((provider_id, package_id, scroll_limit)) =
            self.current_provider().and_then(|provider| {
                self.selected_package().map(|package| {
                    (
                        provider.id.clone(),
                        package.id.clone(),
                        package.detail_scroll_limit_hint(),
                    )
                })
            })
        else {
            return false;
        };
        self.package_detail.provider_id = Some(provider_id);
        self.package_detail.package_id = Some(package_id);
        self.package_detail.scroll = 0;
        self.package_detail.scroll_limit.set(scroll_limit);
        true
    }

    pub(crate) fn close_package_detail(&mut self) -> bool {
        let was_open = self.package_detail.package_id.take().is_some();
        self.package_detail.provider_id = None;
        self.package_detail.scroll = 0;
        self.package_detail.scroll_limit.set(0);
        was_open
    }

    pub(crate) const fn package_detail_is_open(&self) -> bool {
        self.package_detail.package_id.is_some()
    }

    pub(crate) fn detail_package(&self) -> Option<&PackageView> {
        let provider_id = self.package_detail.provider_id.as_deref()?;
        let package_id = self.package_detail.package_id.as_deref()?;
        let provider = self.current_provider()?;
        if provider.id != provider_id {
            return None;
        }
        provider
            .packages
            .iter()
            .find(|package| package.id == package_id)
    }

    pub(crate) const fn package_detail_scroll(&self) -> usize {
        self.package_detail.scroll
    }

    pub(crate) fn scroll_package_detail(&mut self, delta: isize) {
        let limit = self.package_detail.scroll_limit.get();
        self.package_detail.scroll = self
            .package_detail
            .scroll
            .min(limit)
            .saturating_add_signed(delta)
            .min(limit);
    }

    pub(crate) fn move_package_detail_to_edge(&mut self, end: bool) {
        self.package_detail.scroll = if end {
            self.package_detail.scroll_limit.get()
        } else {
            0
        };
    }

    pub(crate) fn set_package_detail_scroll_limit(&self, limit: usize) {
        self.package_detail.scroll_limit.set(limit);
    }

    pub(super) fn reconcile_package_detail(&mut self) {
        let Some(provider_id) = self.package_detail.provider_id.as_deref() else {
            return;
        };
        if self
            .current_provider()
            .is_none_or(|provider| provider.id != provider_id)
        {
            self.close_package_detail();
            return;
        }
        let Some(package_id) = self.package_detail.package_id.as_deref() else {
            return;
        };
        let selected = self
            .package_rows()
            .iter()
            .position(|package| package.id == package_id);
        let Some(index) = selected else {
            self.close_package_detail();
            return;
        };
        self.package_index = index;
        let limit = self
            .detail_package()
            .map(PackageView::detail_scroll_limit_hint)
            .unwrap_or_default();
        self.package_detail.scroll_limit.set(limit);
        self.package_detail.scroll = self
            .package_detail
            .scroll
            .min(self.package_detail.scroll_limit.get());
    }
}

impl PackageView {
    fn detail_scroll_limit_hint(&self) -> usize {
        self.detail_row_count_hint()
            .saturating_sub(DETAIL_VISIBLE_ROW_FLOOR)
    }

    fn detail_row_count_hint(&self) -> usize {
        let facet_rows = [
            &self.specification,
            &self.implementation,
            &self.verification,
        ]
        .into_iter()
        .map(|facet| 1 + usize::from(facet.digest.is_some()))
        .sum::<usize>();
        let acceptance_rows = self
            .acceptance_checks
            .iter()
            .take(DETAIL_SECTION_ITEM_LIMIT)
            .map(|check| 1 + usize::from(check.target.is_some()))
            .sum::<usize>();
        let dependency_rows = self.dependencies.len().min(DETAIL_SECTION_ITEM_LIMIT);
        let blocker_rows = self.blockers.len().min(DETAIL_SECTION_ITEM_LIMIT);
        let evidence_rows = self.evidence.len().min(DETAIL_SECTION_ITEM_LIMIT);
        let extension_rows = usize::from(!self.extensions.is_empty()) * EXTENSION_ROW_LIMIT;

        11 + facet_rows
            + dependency_rows
            + acceptance_rows
            + blocker_rows
            + evidence_rows
            + extension_rows
    }
}
