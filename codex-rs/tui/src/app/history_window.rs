// Modified for OpenDIY Codex R31 (2026-11-01); see MODIFICATIONS.md.
//! Bound committed display sources around the reader, including all frozen view owners.

use super::*;
use crate::history_window::{MAX_HISTORY_BYTES, MAX_HISTORY_ITEMS, MAX_HISTORY_TURNS};
use std::sync::Weak;

pub(super) struct HistoryCellWindow {
    pub(super) generation: u64,
    sources: HashMap<usize, (Weak<dyn HistoryCell>, u64, usize)>,
    pub(super) evicted: bool,
}

impl Default for HistoryCellWindow {
    fn default() -> Self {
        static NEXT_GENERATION: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        Self {
            sources: HashMap::new(),
            evicted: false,
            generation: NEXT_GENERATION.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
        }
    }
}

impl HistoryCellWindow {
    fn bytes(&mut self, cell: &Arc<dyn HistoryCell>) -> usize {
        let key = Arc::as_ptr(cell).cast::<()>() as usize;
        let source = Arc::downgrade(cell);
        let revision = cell.retention_revision();
        let entry = self
            .sources
            .entry(key)
            .or_insert_with(|| (source.clone(), revision, cell.retained_source_bytes()));
        if !entry.0.ptr_eq(&source) || entry.1 != revision {
            *entry = (source, revision, cell.retained_source_bytes());
        }
        entry.2
    }
}

impl App {
    /// Returns whether the display changed. Limits refer to committed cells/logical source;
    /// canonical item payload and layout caches are bounded independently. Lifecycle, input,
    /// and pending approvals are deliberately not stored in this display window.
    pub(super) fn bound_history_window(&mut self, prefer_older: bool) -> bool {
        if self.transcript_cells.is_empty() {
            return false;
        }
        let header = self
            .transcript_cells
            .iter()
            .rposition(|cell| history_cell::is_session_history_prefix_cell(cell.as_ref()))
            .map_or(0, |index| index + 1);
        if header == self.transcript_cells.len() {
            return false;
        }
        let focus = if prefer_older {
            header
        } else {
            self.transcript_view
                .window_focus(&self.transcript_cells)
                .max(header)
        }
        .min(self.transcript_cells.len() - 1);
        self.history_window
            .sources
            .retain(|_, (source, _, _)| source.strong_count() != 0);
        let mut replaced = false;
        let sizes = self
            .transcript_cells
            .iter_mut()
            .map(|cell| {
                let size = self.history_window.bytes(cell);
                if size > MAX_HISTORY_BYTES || cell.retained_item_count() > MAX_HISTORY_ITEMS {
                    *cell = Arc::new(history_cell::new_error_event(
                        crate::history_window::OVERSIZED_HISTORY.to_string(),
                    ));
                    replaced = true;
                    self.history_window.bytes(cell)
                } else {
                    size
                }
            })
            .collect::<Vec<_>>();
        let mut start = focus;
        let mut end = focus + 1;
        let mut bytes = sizes[focus];
        let mut items = self.transcript_cells[focus].retained_item_count();
        let mut prompts = crate::app_backtrack::user_count(&self.transcript_cells[start..end]);
        // Alternating expansion retains nearby context rather than every visited page. The
        // latest-following focus naturally spends its whole budget on the preceding entries.
        loop {
            let candidate = if end < self.transcript_cells.len()
                && (start == header || end - focus <= focus - start)
            {
                Some(end)
            } else if start > header {
                Some(start - 1)
            } else if end < self.transcript_cells.len() {
                Some(end)
            } else {
                None
            };
            let Some(index) = candidate else {
                break;
            };
            let added_prompts =
                crate::app_backtrack::user_count(&self.transcript_cells[index..index + 1]);
            if items.saturating_add(self.transcript_cells[index].retained_item_count())
                > MAX_HISTORY_ITEMS
                || bytes.saturating_add(sizes[index]) > MAX_HISTORY_BYTES
                || prompts + added_prompts > MAX_HISTORY_TURNS
            {
                break;
            }
            bytes += sizes[index];
            items += self.transcript_cells[index].retained_item_count();
            prompts += added_prompts;
            if index < start {
                start = index;
            } else {
                end = index + 1;
            }
        }
        if !replaced && start == header && end == self.transcript_cells.len() {
            return false;
        }
        let removed_prompts =
            crate::app_backtrack::user_count(&self.transcript_cells[header..start]);
        self.transcript_cells.drain(end..);
        self.transcript_cells.drain(header..start);
        if self.backtrack.overlay_preview_active && self.backtrack.nth_user_message != usize::MAX {
            self.backtrack.nth_user_message = self
                .backtrack
                .nth_user_message
                .saturating_sub(removed_prompts);
        }
        self.transcript_view.retain_window(&self.transcript_cells);
        if let Some(Overlay::Transcript(overlay)) = self.overlay.as_mut() {
            overlay.retain_window(&self.transcript_cells);
        }
        self.last_rendered_history_tail = None;
        self.last_thread_usage_status_cell = None;
        let retained = self
            .transcript_cells
            .iter()
            .map(|cell| Arc::as_ptr(cell).cast::<()>() as usize)
            .collect::<HashSet<_>>();
        self.history_window
            .sources
            .retain(|key, _| retained.contains(key));
        self.history_window.evicted = true;
        true
    }
}
