// Modified for OpenDIY Codex R29 (2026-10-30); see MODIFICATIONS.md.
//! Display-only retention budgets. These do not constrain model context or durable rollout.
//!
//! The byte budget counts serialized protocol payload or the retained logical source of cells;
//! allocator overhead and renderer caches have separate lifetimes and are not included. Measure
//! process peak memory separately. Canonical item payload and committed cell source each have
//! this admission ceiling. Explicit export/copy-range/search/selection operations retain their
//! own temporary sources until they end. The latest response/plan copy source, input,
//! approval/lifecycle state, and still-unprinted
//! native FIFO output have separate operational lifetimes. A single oversized payload fails explicitly rather than
//! silently dropping a prefix and presenting the suffix as the original item.

use codex_app_server_protocol::{Turn, TurnItemsView};
use serde::Serialize;
use std::io::Write;

pub(crate) const MAX_HISTORY_TURNS: usize = 30;
pub(crate) const MAX_HISTORY_ITEMS: usize = 400;
pub(crate) const MAX_HISTORY_BYTES: usize = 8 * 1024 * 1024;
pub(crate) const OVERSIZED_HISTORY: &str = "표시 이력 항목이 메모리 제한을 초과했습니다. 원본은 보존되며 다른 대화 구간을 계속 탐색할 수 있습니다.";

/// Count once at admission, without allocating a second serialized copy. This intentionally
/// stops scanning a giant string when serde writes the chunk that exceeds the display budget.
pub(crate) fn payload_bytes(value: &impl Serialize) -> Result<usize, serde_json::Error> {
    struct Counter(usize);
    impl Write for Counter {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            if bytes.len() > MAX_HISTORY_BYTES.saturating_sub(self.0) {
                return Err(std::io::Error::other(OVERSIZED_HISTORY));
            }
            self.0 += bytes.len();
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut counter = Counter(0);
    serde_json::to_writer(&mut counter, value)?;
    Ok(counter.0)
}

pub(crate) fn turn_shell(turn: &Turn) -> Turn {
    Turn {
        id: turn.id.clone(),
        root_turn_id: turn.root_turn_id.clone(),
        items: Vec::new(),
        items_view: TurnItemsView::NotLoaded,
        status: turn.status.clone(),
        error: turn.error.clone(),
        started_at: turn.started_at,
        completed_at: turn.completed_at,
        duration_ms: turn.duration_ms,
    }
}
