use crate::dashboard::RecorderEpochId;
use unicode_width::UnicodeWidthStr;

use super::DetailDocument;

impl DetailDocument {
    fn line_count(&self) -> usize {
        1 + self.lines.len()
    }

    fn widest_line(&self) -> usize {
        std::iter::once(&self.title)
            .chain(&self.lines)
            .map(|line| UnicodeWidthStr::width(line.as_str()))
            .max()
            .unwrap_or(0)
    }
}

/// One open item detail. Each document is captured from the recorder epoch
/// that produced it and is never re-collected in place.
#[derive(Clone, Debug, Default)]
pub(crate) struct DetailState {
    pub(crate) document: Option<DetailDocument>,
    pub(crate) scroll: u16,
    pub(crate) horizontal_scroll: u16,
    pub(crate) item_epoch: Option<RecorderEpochId>,
    pub(crate) item_generated_at_ms: Option<u64>,
}

impl DetailState {
    pub(crate) fn is_open(&self) -> bool {
        self.document.is_some()
    }

    pub(crate) fn scroll_limit(&self) -> u16 {
        let lines = self.document.as_ref().map_or(0, DetailDocument::line_count);
        u16::try_from(lines.saturating_sub(1)).unwrap_or(u16::MAX)
    }

    pub(crate) fn horizontal_limit(&self) -> u16 {
        let width = self
            .document
            .as_ref()
            .map_or(0, DetailDocument::widest_line);
        u16::try_from(width.saturating_sub(1)).unwrap_or(u16::MAX)
    }

    pub(crate) fn open_document(
        &mut self,
        document: DetailDocument,
        epoch: RecorderEpochId,
        generated_at_ms: u64,
    ) {
        *self = Self {
            document: Some(document),
            item_epoch: Some(epoch),
            item_generated_at_ms: Some(generated_at_ms),
            ..Self::default()
        };
    }
}
