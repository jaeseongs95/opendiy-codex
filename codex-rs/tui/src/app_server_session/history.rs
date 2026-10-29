//! Bounded app-server transcript loading for resume, fork, and transcript views.

use std::collections::HashSet;

use super::AppServerSession;
use crate::history_cell::HistoryRenderMode;
use crate::legacy_core::config::Config;
use crate::local_settings::LocalSettings;
use crate::resize_reflow_cap::resize_reflow_max_rows;
use crate::thread_transcript::RawReasoningVisibility;
use crate::thread_transcript::thread_items_to_transcript_cells;
use codex_app_server_protocol::ClientRequest;
use codex_app_server_protocol::RequestId;
use codex_app_server_protocol::SortDirection;
use codex_app_server_protocol::Thread;
use codex_app_server_protocol::ThreadHistoryMode;
use codex_app_server_protocol::ThreadItem;
use codex_app_server_protocol::ThreadItemsListAnchor;
use codex_app_server_protocol::ThreadItemsListCursor;
use codex_app_server_protocol::ThreadItemsListParams;
use codex_app_server_protocol::ThreadItemsListResponse;
use codex_app_server_protocol::ThreadRevertParams;
use codex_app_server_protocol::ThreadRevertResponse;
use codex_app_server_protocol::ThreadTurnsListParams;
use codex_app_server_protocol::ThreadTurnsListResponse;
use codex_app_server_protocol::Turn;
use codex_app_server_protocol::TurnItemsView;
use codex_protocol::ThreadId;
use color_eyre::eyre::Result;
use color_eyre::eyre::WrapErr;

pub(crate) const INITIAL_HISTORY_TURN_LIMIT: u32 = 5;
pub(crate) const HISTORY_ITEM_PAGE_LIMIT: u32 = 100;
pub(crate) const HISTORY_ITEM_SCAN_LIMIT: usize = 4 * HISTORY_ITEM_PAGE_LIMIT as usize;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum HistoryHydrationScope<'a> {
    Initial,
    Complete,
    ThroughTurn(&'a str),
}

/// Limits initial hydration independently from the number of items retained by later paging.
struct HistoryLoadBudget {
    rows: Option<usize>,
    items: Option<usize>,
}

impl HistoryLoadBudget {
    fn new(
        scope: HistoryHydrationScope<'_>,
        config: Option<&Config>,
        local_settings: Option<&crate::local_settings::LocalSettings>,
        terminal_height: u16,
    ) -> Self {
        let transcript_mode = local_settings
            .map(|settings| settings.transcript_mode)
            .or_else(|| config.map(|config| LocalSettings::from(config).transcript_mode));
        if scope == HistoryHydrationScope::Initial
            && transcript_mode.is_some_and(crate::transcript_mode::TranscriptMode::is_owned)
        {
            // A few screenfuls cover the first viewport and nearby reading without coupling
            // owned history to terminal scrollback settings, including unlimited scrollback.
            return Self {
                rows: Some(usize::from(terminal_height.max(/*other*/ 1)) * 3),
                items: Some(HISTORY_ITEM_SCAN_LIMIT),
            };
        }
        let rows = local_settings
            .and_then(|settings| resize_reflow_max_rows(settings.terminal_resize_reflow()));
        let items = match (scope, config, rows) {
            (HistoryHydrationScope::Complete, _, _)
            | (HistoryHydrationScope::ThroughTurn(_), _, _)
            | (HistoryHydrationScope::Initial, Some(_), None) => None,
            (HistoryHydrationScope::Initial, Some(_), Some(max_rows)) => {
                Some(max_rows.saturating_add(HISTORY_ITEM_SCAN_LIMIT))
            }
            (HistoryHydrationScope::Initial, None, _) => Some(HISTORY_ITEM_PAGE_LIMIT as usize),
        };
        // Display hydration is finite even when native terminal scrollback is unlimited.
        let items = if scope == HistoryHydrationScope::Initial {
            Some(
                items
                    .unwrap_or(HISTORY_ITEM_SCAN_LIMIT)
                    .min(HISTORY_ITEM_SCAN_LIMIT),
            )
        } else {
            items
        };
        Self { rows, items }
    }

    fn next_page_size(&self, rendered_rows: usize, scanned_items: usize) -> Option<u32> {
        let remaining_rows = self.rows.map(|budget| budget.saturating_sub(rendered_rows));
        let remaining_items = self
            .items
            .map(|budget| budget.saturating_sub(scanned_items));
        if remaining_rows == Some(0) || remaining_items == Some(0) {
            return None;
        }
        let limit = remaining_items
            .unwrap_or(HISTORY_ITEM_PAGE_LIMIT as usize)
            .min(HISTORY_ITEM_PAGE_LIMIT as usize);
        // After the first page, hidden items must not reduce requests to one item.
        let limit = if scanned_items != 0 {
            limit
        } else {
            limit.min(remaining_rows.unwrap_or(limit))
        };
        Some(limit as u32)
    }
}

pub(crate) fn initial_history_page_limit(
    config: &Config,
    settings: &LocalSettings,
    height: u16,
) -> u32 {
    HistoryLoadBudget::new(
        HistoryHydrationScope::Initial,
        Some(config),
        Some(settings),
        height,
    )
    .next_page_size(0, 0)
    .unwrap_or(HISTORY_ITEM_PAGE_LIMIT)
}

pub(crate) fn thread_items_page_params(
    thread_id: ThreadId,
    turn_id: Option<&str>,
    cursor: Option<String>,
    limit: u32,
) -> ThreadItemsListParams {
    ThreadItemsListParams {
        thread_id: thread_id.to_string(),
        turn_id: turn_id.map(str::to_string),
        cursor: cursor.map(ThreadItemsListCursor::Opaque),
        limit: Some(limit),
        sort_direction: Some(SortDirection::Desc),
    }
}

/// Keep the recent fast-path set finite without losing long-cycle detection. Brent's online
/// checkpoint detects cycles of any length in constant additional space and requires no RPCs.
#[derive(Clone, Debug, Default)]
struct CursorHistory {
    recent: HashSet<String>,
    checkpoint: Option<String>,
    power: usize,
    distance: usize,
}

impl CursorHistory {
    #[cfg(test)]
    fn len(&self) -> usize {
        self.recent.len()
    }
}

fn advancing_cursor(
    current: Option<&str>,
    next: Option<String>,
    history: &mut CursorHistory,
) -> Option<String> {
    if history.power == 0 {
        history.power = 1;
        history.checkpoint = current.map(str::to_owned);
    }
    if history.recent.len() >= crate::history_window::MAX_HISTORY_ITEMS {
        history.recent.clear();
    }
    if let Some(current) = current {
        history.recent.insert(current.to_string());
    }
    let next = next?;
    if !history.recent.insert(next.clone()) || history.checkpoint.as_ref() == Some(&next) {
        return None;
    }
    history.distance = history.distance.saturating_add(1);
    if history.distance >= history.power {
        history.checkpoint = Some(next.clone());
        history.power = history.power.saturating_mul(2);
        history.distance = 0;
    }
    Some(next)
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum HistoryDirection {
    #[default]
    Older,
    Newer,
    Latest,
}

pub(crate) struct HistoryPageRequest {
    pub(crate) request_id: RequestId,
    pub(crate) token: String,
    pub(crate) params: ThreadItemsListParams,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct ThreadHistoryPagination {
    pub(super) history_mode: ThreadHistoryMode,
    next_turn_cursor: Option<String>,
    next_item_cursor: Option<String>,
    seen_turn_cursors: CursorHistory,
    seen_item_cursors: CursorHistory,
    loading_older: bool,
    loading_token: Option<String>,
    loading_cursor: Option<String>,
    direction: HistoryDirection,
    newer_item_cursor: Option<String>,
    newer_turn_cursor: Option<String>,
    seek_newer_metadata: bool,
    older_scope: Option<String>,
    newer_scope: Option<String>,
    older_anchor: Option<ThreadItemsListCursor>,
    newer_anchor: Option<ThreadItemsListCursor>,
    oldest_item: Option<(String, String)>,
    errors: Vec<String>,
}

impl AppServerSession {
    pub(crate) fn take_history_errors(&mut self, thread_id: ThreadId) -> Vec<String> {
        self.history_pagination
            .get_mut(&thread_id)
            .map(|state| std::mem::take(&mut state.errors))
            .unwrap_or_default()
    }

    pub(crate) fn has_newer_history(&self, thread_id: ThreadId) -> bool {
        self.history_pagination
            .get(&thread_id)
            .is_some_and(|state| state.newer_scope.is_some() || state.newer_item_cursor.is_some())
    }

    pub(crate) fn pending_history_direction(&self, thread_id: ThreadId) -> HistoryDirection {
        self.history_pagination
            .get(&thread_id)
            .map_or(HistoryDirection::Older, |state| state.direction)
    }

    pub(crate) fn is_history_token_pending(&self, thread_id: ThreadId, token: &str) -> bool {
        self.history_pagination
            .get(&thread_id)
            .is_some_and(|state| {
                state.loading_older && state.loading_token.as_deref() == Some(token)
            })
    }

    pub(crate) fn cancel_history_page(&mut self, thread_id: ThreadId) {
        if let Some(state) = self.history_pagination.get_mut(&thread_id) {
            state.loading_older = false;
            state.loading_token = None;
        }
    }

    /// The official page state/request path is shared by both directions and latest return.
    pub(crate) fn begin_history_page(
        &mut self,
        thread_id: ThreadId,
        direction: HistoryDirection,
    ) -> Option<HistoryPageRequest> {
        let request_id = self.next_request_id();
        let state = self.history_pagination.get_mut(&thread_id)?;
        if state.loading_older && direction != HistoryDirection::Latest {
            return None;
        }
        let (scope, cursor) = match direction {
            HistoryDirection::Older => (
                state.older_scope.clone(),
                state.older_anchor.clone().or_else(|| {
                    state
                        .next_item_cursor
                        .clone()
                        .map(ThreadItemsListCursor::Opaque)
                }),
            ),
            HistoryDirection::Newer => (
                state.newer_scope.clone(),
                state.newer_anchor.clone().or_else(|| {
                    state
                        .newer_item_cursor
                        .clone()
                        .map(ThreadItemsListCursor::Opaque)
                }),
            ),
            HistoryDirection::Latest => (None, None),
        };
        if scope.is_none() && cursor.is_none() && direction != HistoryDirection::Latest {
            return None;
        }
        let mut params =
            thread_items_page_params(thread_id, scope.as_deref(), None, HISTORY_ITEM_PAGE_LIMIT);
        params.cursor = cursor;
        params.sort_direction = Some(if direction == HistoryDirection::Newer {
            SortDirection::Asc
        } else {
            SortDirection::Desc
        });
        if state.direction != direction || direction == HistoryDirection::Latest {
            // A deliberate reversal/re-anchor starts a new traversal, not a cursor cycle.
            state.seen_item_cursors = CursorHistory::default();
            state.seen_turn_cursors = CursorHistory::default();
        }
        let token = format!("history:{request_id:?}");
        state.loading_older = true;
        state.loading_cursor = match params.cursor.as_ref() {
            Some(ThreadItemsListCursor::Opaque(cursor)) => Some(cursor.clone()),
            _ => None,
        };
        state.loading_token = Some(token.clone());
        state.direction = direction;
        Some(HistoryPageRequest {
            request_id,
            token,
            params,
        })
    }

    /// Exact retained-edge anchors reuse the protocol's existing per-turn item position.
    /// A page outside that turn continues through the existing NotLoaded metadata cursor.
    pub(crate) fn reanchor_history_window(
        &mut self,
        thread_id: ThreadId,
        turns: &[Turn],
        direction: HistoryDirection,
    ) {
        let Some(state) = self.history_pagination.get_mut(&thread_id) else {
            return;
        };
        if direction == HistoryDirection::Latest {
            state.newer_scope = None;
            state.newer_anchor = None;
            state.newer_item_cursor = None;
            state.seek_newer_metadata = false;
            return;
        }
        let (turn, newer) = if direction == HistoryDirection::Older {
            (turns.iter().rev().find(|turn| !turn.items.is_empty()), true)
        } else {
            (turns.iter().find(|turn| !turn.items.is_empty()), false)
        };
        let Some(turn) = turn else {
            return;
        };
        let item = if newer {
            turn.items.last()
        } else {
            turn.items.first()
        };
        let anchor = item.map(|item| {
            ThreadItemsListCursor::Anchor(ThreadItemsListAnchor::Item {
                item_id: item.id().to_string(),
            })
        });
        if newer {
            state.newer_scope = Some(turn.id.clone());
            state.newer_anchor = anchor;
            state.newer_item_cursor = None;
            // The reverse cursor of a whole metadata page may lie beyond this retained edge.
            // Reuse the older continuation and seek through the bounded retained shells first.
            state.newer_turn_cursor = state.next_turn_cursor.clone();
            state.seek_newer_metadata = true;
        } else {
            state.older_scope = Some(turn.id.clone());
            state.older_anchor = anchor;
            state.next_item_cursor = None;
        }
    }

    async fn advance_scoped_history(
        &mut self,
        thread_id: ThreadId,
        state: &mut ThreadHistoryPagination,
        turns: &mut Vec<Turn>,
    ) -> Result<()> {
        let newer = state.direction == HistoryDirection::Newer;
        let (scope, cursor) = if newer {
            (&state.newer_scope, &state.newer_item_cursor)
        } else {
            (&state.older_scope, &state.next_item_cursor)
        };
        let Some(current) = scope.clone() else {
            return Ok(());
        };
        if cursor.is_some() {
            return Ok(());
        }
        let next_index = |turns: &[Turn]| {
            turns
                .iter()
                .position(|turn| turn.id == current)
                .and_then(|index| {
                    if newer {
                        (index + 1 < turns.len()).then_some(index + 1)
                    } else {
                        index.checked_sub(1)
                    }
                })
        };
        let mut next = next_index(turns).map(|index| turns[index].id.clone());
        // Metadata can overlap retained shells when reversing direction. Skip those pages
        // with a finite scan budget rather than selecting an earlier turn as "newer".
        for _ in 0..4 {
            if next.is_some() {
                break;
            }
            let seeking = newer && state.seek_newer_metadata;
            let cursor = if newer {
                state.newer_turn_cursor.take()
            } else {
                state.next_turn_cursor.take()
            };
            if cursor.is_none() && !seeking {
                break;
            }
            let mut page = self
                .thread_turns_page_direction(
                    thread_id,
                    cursor.clone(),
                    INITIAL_HISTORY_TURN_LIMIT,
                    if newer {
                        SortDirection::Asc
                    } else {
                        SortDirection::Desc
                    },
                )
                .await?;
            if seeking {
                if let Some(index) = page.data.iter().position(|turn| turn.id == current) {
                    page.data.drain(..index);
                    state.seek_newer_metadata = false;
                } else {
                    page.data.clear();
                    if page.next_cursor.is_none() {
                        state.seek_newer_metadata = false;
                    }
                }
            }
            let first_new = page
                .data
                .iter()
                .find(|turn| !turns.iter().any(|known| known.id == turn.id))
                .map(|turn| turn.id.clone());
            let next_cursor = advancing_cursor(
                cursor.as_deref(),
                page.next_cursor,
                &mut state.seen_turn_cursors,
            );
            if seeking && next_cursor.is_none() {
                state.seek_newer_metadata = false;
            }
            if newer {
                state.newer_turn_cursor = next_cursor;
                if !state.seek_newer_metadata {
                    state.next_turn_cursor = page.backwards_cursor;
                }
            } else {
                state.next_turn_cursor = next_cursor;
                state.newer_turn_cursor = page.backwards_cursor;
            }
            if newer {
                for turn in page.data {
                    if !turns.iter().any(|known| known.id == turn.id) {
                        turns.push(turn);
                    }
                }
            } else {
                for turn in page.data {
                    if !turns.iter().any(|known| known.id == turn.id) {
                        turns.insert(0, turn);
                    }
                }
            }
            next = next_index(turns)
                .map(|index| turns[index].id.clone())
                .or(first_new);
        }
        let more_metadata = if newer {
            state.newer_turn_cursor.is_some()
        } else {
            state.next_turn_cursor.is_some()
        };
        let resume_anchor = if next.is_none() && more_metadata {
            next = Some(current.clone());
            turns
                .iter()
                .find(|turn| turn.id == current)
                .and_then(|turn| {
                    if newer {
                        turn.items.last()
                    } else {
                        turn.items.first()
                    }
                })
                .map(|item| {
                    ThreadItemsListCursor::Anchor(ThreadItemsListAnchor::Item {
                        item_id: item.id().to_string(),
                    })
                })
        } else {
            None
        };
        if newer {
            state.newer_scope = next;
            state.newer_anchor = resume_anchor;
        } else {
            state.older_scope = next;
            state.older_anchor = resume_anchor;
        }
        Ok(())
    }

    /// An unavailable turn remains explicitly unloaded. The next manual navigation can
    /// select its neighbor; projection/network errors are never retried in a background loop.
    pub(crate) async fn fail_history_page(
        &mut self,
        thread_id: ThreadId,
        rejected_turn: Option<&str>,
        turns: &mut Vec<Turn>,
    ) -> Result<()> {
        let Some(mut state) = self.history_pagination.get(&thread_id).cloned() else {
            return Ok(());
        };
        let newer = state.direction == HistoryDirection::Newer;
        if let Some(turn_id) = rejected_turn {
            if newer {
                state.newer_scope = Some(turn_id.to_string());
                state.newer_anchor = None;
            } else {
                state.older_scope = Some(turn_id.to_string());
                state.older_anchor = None;
            }
        }
        let scoped = if newer {
            state.newer_scope.is_some()
        } else {
            state.older_scope.is_some()
        };
        if scoped {
            if newer {
                state.newer_item_cursor = None;
            } else {
                state.next_item_cursor = None;
            }
            self.advance_scoped_history(thread_id, &mut state, turns)
                .await?;
        } else if !newer {
            state.older_scope = state
                .oldest_item
                .as_ref()
                .map(|(turn, _)| turn.clone())
                .or_else(|| turns.last().map(|turn| turn.id.clone()));
            state.older_anchor = state.oldest_item.as_ref().map(|(_, item)| {
                ThreadItemsListCursor::Anchor(ThreadItemsListAnchor::Item {
                    item_id: item.clone(),
                })
            });
            state.next_item_cursor = None;
        }
        state.loading_older = false;
        state.loading_token = None;
        self.history_pagination.insert(thread_id, state);
        Ok(())
    }
    pub(crate) async fn revert_thread(
        &mut self,
        thread_id: ThreadId,
        before_turn_id: String,
        retained_turns: &[Turn],
    ) -> std::result::Result<ThreadRevertResponse, codex_app_server_client::TypedRequestError> {
        let request_id = self.next_request_id();
        let response: ThreadRevertResponse = self
            .client
            .request_typed(ClientRequest::ThreadRevert {
                request_id,
                params: ThreadRevertParams {
                    thread_id: thread_id.to_string(),
                    before_turn_id,
                },
            })
            .await?;
        // Older cursors in the retained prefix remain valid. If the entire displayed window
        // disappeared, resume paging at the replacement history's end instead.
        if retained_turns.iter().all(|turn| turn.items.is_empty()) {
            self.history_pagination.insert(
                thread_id,
                ThreadHistoryPagination {
                    history_mode: ThreadHistoryMode::Paginated,
                    next_turn_cursor: response.turns_backwards_cursor.clone(),
                    next_item_cursor: response.items_backwards_cursor.clone(),
                    ..ThreadHistoryPagination::default()
                },
            );
        } else if let Some(page) = self.history_pagination.get_mut(&thread_id) {
            page.loading_older = false;
        }
        Ok(response)
    }

    pub(crate) fn has_older_history(&self, thread_id: ThreadId) -> bool {
        self.history_pagination
            .get(&thread_id)
            .is_some_and(|page| page.next_item_cursor.is_some() || page.older_scope.is_some())
    }

    pub(crate) fn begin_older_history_page(&mut self, thread_id: ThreadId) -> Option<String> {
        let page = self.history_pagination.get_mut(&thread_id)?;
        if page.loading_older {
            return None;
        }
        let cursor = page.next_item_cursor.clone()?;
        page.loading_older = true;
        page.loading_token = None;
        page.loading_cursor = Some(cursor.clone());
        page.direction = HistoryDirection::Older;
        Some(cursor)
    }

    /// Match a completion to the page still pending for this thread, before interpreting its result.
    pub(crate) fn is_older_history_page_pending(&self, thread_id: ThreadId, cursor: &str) -> bool {
        self.history_pagination.get(&thread_id).is_some_and(|page| {
            page.loading_older
                && (page.loading_token.as_deref() == Some(cursor)
                    || page.loading_cursor.as_deref() == Some(cursor)
                    || (page.loading_token.is_none()
                        && page.next_item_cursor.as_deref() == Some(cursor)))
        })
    }

    pub(crate) fn cancel_older_history_page(&mut self, thread_id: ThreadId, cursor: &str) {
        if let Some(page) = self.history_pagination.get_mut(&thread_id)
            && (page.loading_token.as_deref() == Some(cursor)
                || page.loading_cursor.as_deref() == Some(cursor)
                || (page.loading_token.is_none()
                    && page.next_item_cursor.as_deref() == Some(cursor)))
        {
            page.loading_older = false;
            page.loading_token = None;
        }
    }

    pub(crate) async fn apply_older_history_page(
        &mut self,
        thread_id: ThreadId,
        cursor: &str,
        page: ThreadItemsListResponse,
        turns: &mut Vec<Turn>,
    ) -> Result<Vec<ThreadItem>> {
        if !self.is_older_history_page_pending(thread_id, cursor) {
            return Ok(Vec::new());
        }
        let Some(mut state) = self.history_pagination.get(&thread_id).cloned() else {
            return Ok(Vec::new());
        };
        if state.direction == HistoryDirection::Latest {
            turns.clear();
            let metadata = self
                .thread_turns_page(thread_id, None, INITIAL_HISTORY_TURN_LIMIT)
                .await?;
            state.next_turn_cursor = metadata.next_cursor;
            state.newer_turn_cursor = metadata.backwards_cursor;
            state.older_scope = None;
            state.newer_scope = None;
            state.seek_newer_metadata = false;
            state.next_item_cursor = None;
            state.newer_item_cursor = None;
            turns.extend(metadata.data.into_iter().rev());
        }
        let items = self
            .merge_thread_item_page(thread_id, page, &mut state, turns)
            .await?;
        if state.direction != HistoryDirection::Latest {
            self.advance_scoped_history(thread_id, &mut state, turns)
                .await?;
        }
        state.loading_older = false;
        state.loading_token = None;
        self.history_pagination.insert(thread_id, state);
        Ok(items)
    }

    pub(crate) async fn thread_items_page(
        &mut self,
        thread_id: ThreadId,
        turn_id: Option<&str>,
        cursor: Option<String>,
        limit: u32,
    ) -> Result<ThreadItemsListResponse> {
        let request_id = self.next_request_id();
        self.client
            .request_typed(ClientRequest::ThreadItemsList {
                request_id,
                params: thread_items_page_params(thread_id, turn_id, cursor, limit),
            })
            .await
            .wrap_err("failed to load a bounded thread item page")
    }

    pub(crate) async fn thread_turns_page(
        &mut self,
        thread_id: ThreadId,
        cursor: Option<String>,
        limit: u32,
    ) -> Result<ThreadTurnsListResponse> {
        self.thread_turns_page_direction(thread_id, cursor, limit, SortDirection::Desc)
            .await
    }

    async fn thread_turns_page_direction(
        &mut self,
        thread_id: ThreadId,
        cursor: Option<String>,
        limit: u32,
        direction: SortDirection,
    ) -> Result<ThreadTurnsListResponse> {
        let request_id = self.next_request_id();
        self.client
            .request_typed(ClientRequest::ThreadTurnsList {
                request_id,
                params: ThreadTurnsListParams {
                    thread_id: thread_id.to_string(),
                    cursor,
                    limit: Some(limit),
                    sort_direction: Some(direction),
                    items_view: Some(TurnItemsView::NotLoaded),
                },
            })
            .await
            .wrap_err("failed to load a bounded thread history page")
    }

    async fn merge_thread_item_page(
        &mut self,
        thread_id: ThreadId,
        page: ThreadItemsListResponse,
        state: &mut ThreadHistoryPagination,
        turns: &mut Vec<Turn>,
    ) -> Result<Vec<ThreadItem>> {
        let newer = state.direction == HistoryDirection::Newer;
        let next = if newer {
            &mut state.newer_item_cursor
        } else {
            &mut state.next_item_cursor
        };
        *next = advancing_cursor(
            next.as_deref(),
            page.next_cursor,
            &mut state.seen_item_cursors,
        );
        if newer {
            state.newer_anchor = None;
        } else {
            state.older_anchor = None;
        }
        // The server-provided reverse cursor is reused for global traversal. Exact window
        // edges below use the existing per-turn Item anchor after source eviction.
        if if newer {
            state.newer_scope.is_none()
        } else {
            state.older_scope.is_none()
        } {
            if newer {
                state.next_item_cursor = page.backwards_cursor;
            } else {
                state.newer_item_cursor = page.backwards_cursor;
            }
        }
        let mut missing_turn_ids = page
            .data
            .iter()
            .filter(|entry| !turns.iter().any(|turn| turn.id == entry.turn_id))
            .map(|entry| entry.turn_id.clone())
            .collect::<HashSet<_>>();
        let mut items = Vec::new();
        for entry in page.data {
            while !turns.iter().any(|turn| turn.id == entry.turn_id) {
                let cursor_slot = if newer {
                    &mut state.newer_turn_cursor
                } else {
                    &mut state.next_turn_cursor
                };
                let Some(cursor) = cursor_slot.take() else {
                    break;
                };
                // Fetch only the remaining item-backed turns so metadata does not run ahead
                // of this page. Empty turns between them may require another bounded request.
                let limit = missing_turn_ids.len().min(HISTORY_ITEM_PAGE_LIMIT as usize) as u32;
                let page = self
                    .thread_turns_page_direction(
                        thread_id,
                        Some(cursor.clone()),
                        limit,
                        if newer {
                            SortDirection::Asc
                        } else {
                            SortDirection::Desc
                        },
                    )
                    .await?;
                let next = advancing_cursor(
                    Some(&cursor),
                    page.next_cursor,
                    &mut state.seen_turn_cursors,
                );
                if newer {
                    state.newer_turn_cursor = next;
                    state.next_turn_cursor = page.backwards_cursor;
                } else {
                    state.next_turn_cursor = next;
                    state.newer_turn_cursor = page.backwards_cursor;
                }
                for turn in &page.data {
                    missing_turn_ids.remove(&turn.id);
                }
                if newer {
                    for turn in page.data {
                        if !turns.iter().any(|known| known.id == turn.id) {
                            turns.push(turn);
                        }
                    }
                } else {
                    turns.splice(0..0, page.data.into_iter().rev());
                }
            }
            if let Some(turn) = turns.iter_mut().find(|turn| turn.id == entry.turn_id)
                && !turn.items.iter().any(|item| item.id() == entry.item.id())
            {
                items.push(entry.item.clone());
                if !newer {
                    state.oldest_item = Some((entry.turn_id.clone(), entry.item.id().to_string()));
                }
                if newer {
                    turn.items.push(entry.item);
                } else {
                    turn.items.insert(/*index*/ 0, entry.item);
                }
                turn.items_view = TurnItemsView::Summary;
            }
        }
        if !newer {
            items.reverse();
        }
        Ok(items)
    }

    /// 페이지 형식 이력은 크기를 제한한 차례·항목 페이지로 불러온다.
    ///
    /// 이전 형식 이력은 서버의 페이징 지원 여부와 관계없이 기존 내용을 보존하기 위해
    /// `thread/read(includeTurns: true)`로 불러온다.
    pub(crate) async fn hydrate_initial_thread_history(
        &mut self,
        thread: &mut Thread,
        turn_cursor: Option<String>,
        item_cursor: Option<String>,
        config: Option<&Config>,
        local_settings: Option<&crate::local_settings::LocalSettings>,
        scope: HistoryHydrationScope<'_>,
    ) -> Result<()> {
        let thread_id = ThreadId::from_string(&thread.id)
            .wrap_err("invalid thread id in bounded history response")?;
        if thread.history_mode == ThreadHistoryMode::Legacy {
            if thread.turns.is_empty() {
                thread.turns = Box::pin(self.thread_read(thread_id, /*include_turns*/ true))
                    .await?
                    .turns;
            }
            self.history_pagination.entry(thread_id).or_default();
            return Ok(());
        }

        let page = self
            .thread_turns_page(thread_id, turn_cursor, INITIAL_HISTORY_TURN_LIMIT)
            .await?;
        thread.turns = page.data.into_iter().rev().collect();
        let mut state = ThreadHistoryPagination {
            history_mode: thread.history_mode.clone(),
            next_turn_cursor: page.next_cursor,
            newer_turn_cursor: page.backwards_cursor,
            next_item_cursor: item_cursor,
            ..ThreadHistoryPagination::default()
        };
        let (width, height) = crossterm::terminal::size().unwrap_or(/*default*/ (80, 24));
        let width = width.max(/*other*/ 1);
        let budget = HistoryLoadBudget::new(scope, config, local_settings, height);
        let mut initial_bytes = 0usize;
        let mut scanned_items = 0;
        let mut rendered_rows = 0;
        while let Some(limit) = budget.next_page_size(rendered_rows, scanned_items) {
            let result = self
                .thread_items_page(
                    thread_id,
                    /*turn_id*/ None,
                    state.next_item_cursor.clone(),
                    limit,
                )
                .await;
            let page = match result {
                Ok(page) => page,
                Err(error) if scope == HistoryHydrationScope::Initial => {
                    // Compatibility and transport failures keep their official caller recovery.
                    // Only explicit unavailable projection payloads become an unloaded UI state.
                    let structured = error
                        .downcast_ref::<codex_app_server_client::TypedRequestError>()
                        .and_then(|error| match error {
                            codex_app_server_client::TypedRequestError::Server {
                                source, ..
                            } => source.data.as_ref(),
                            _ => None,
                        })
                        .and_then(|data| data.get("kind"))
                        .and_then(serde_json::Value::as_str);
                    if structured != Some("historyProjectionUnavailable") {
                        return Err(error);
                    }
                    state.errors.push(format!("{error:#}"));
                    break;
                }
                Err(error) => return Err(error),
            };
            if scope == HistoryHydrationScope::Initial {
                let bytes = page.data.iter().try_fold(0usize, |sum, entry| {
                    crate::history_window::payload_bytes(&entry.item)
                        .map(|size| sum.saturating_add(size))
                });
                let Ok(bytes) = bytes else {
                    state
                        .errors
                        .push(crate::history_window::OVERSIZED_HISTORY.to_string());
                    state.older_scope = page.data.first().map(|entry| entry.turn_id.clone());
                    state.next_item_cursor = None;
                    self.advance_scoped_history(thread_id, &mut state, &mut thread.turns)
                        .await?;
                    break;
                };
                if initial_bytes.saturating_add(bytes) > crate::history_window::MAX_HISTORY_BYTES {
                    break;
                }
                initial_bytes += bytes;
            }
            if page.data.is_empty() {
                state.next_item_cursor = None;
                break;
            }
            scanned_items = scanned_items.saturating_add(page.data.len());
            let items = self
                .merge_thread_item_page(thread_id, page, &mut state, &mut thread.turns)
                .await?;
            // Finish the anchor turn so prompt editing can detect earlier steers,
            // but do not scan history older than the displayed transcript.
            if let HistoryHydrationScope::ThroughTurn(anchor) = scope
                && let Some(index) = thread.turns.iter().position(|turn| turn.id == anchor)
                && thread.turns[..index]
                    .iter()
                    .any(|turn| !turn.items.is_empty())
            {
                break;
            }
            if let Some((config, local_settings)) = config.zip(local_settings) {
                rendered_rows = rendered_history_rows(
                    thread_id,
                    thread,
                    items,
                    config,
                    local_settings,
                    width,
                    rendered_rows,
                );
            } else {
                rendered_rows = rendered_rows.saturating_add(items.len());
            }
            if state.next_item_cursor.is_none() {
                break;
            }
        }
        if config.is_some() {
            self.history_pagination.insert(thread_id, state);
        }
        Ok(())
    }
}

fn rendered_history_rows(
    thread_id: ThreadId,
    thread: &Thread,
    items: Vec<ThreadItem>,
    config: &Config,
    local_settings: &crate::local_settings::LocalSettings,
    width: u16,
    rendered_rows: usize,
) -> usize {
    let visibility = if config.show_raw_agent_reasoning {
        RawReasoningVisibility::Visible
    } else {
        RawReasoningVisibility::Hidden
    };
    let mode = if local_settings.tui.raw_output_mode {
        HistoryRenderMode::Raw
    } else {
        HistoryRenderMode::Rich
    };
    thread_items_to_transcript_cells(
        Some(thread_id),
        &thread.cwd,
        items,
        visibility,
        Some(config),
    )
    .into_iter()
    .fold(rendered_rows, |rows, cell| {
        let height = usize::from(cell.desired_height_for_mode(width, mode));
        rows + height + usize::from(height != 0 && rows != 0 && !cell.is_stream_continuation())
    })
}

#[cfg(test)]
#[path = "history_tests.rs"]
mod tests;
