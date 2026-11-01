// Modified for OpenDIY Codex R32 (2026-11-01); see MODIFICATIONS.md.
//! Load bounded history pages into the transcript before updating its owned or inline viewport.

use std::collections::HashSet;
use std::ops::Range;

use super::*;
use crate::history_cell::UserHistoryCell;
use crate::history_cell::is_session_history_prefix_cell;
use crate::pager_overlay::TranscriptHistoryState;
use crate::thread_transcript::RawReasoningVisibility;
use crate::thread_transcript::thread_items_to_transcript_cells;
use codex_app_server_protocol::ClientRequest;
use codex_app_server_protocol::ThreadItem;
use codex_app_server_protocol::ThreadItemsListResponse;

#[path = "history_completion.rs"]
mod completion;

impl App {
    /// Start one bounded page request shared by scrollback refill and the transcript overlay.
    pub(crate) fn request_older_history_page(
        &self,
        app_server: &mut AppServerSession,
        thread_id: ThreadId,
    ) -> bool {
        self.request_history_page(
            app_server,
            thread_id,
            crate::app_server_session::HistoryDirection::Older,
        )
    }

    pub(crate) fn request_history_page(
        &self,
        app_server: &mut AppServerSession,
        thread_id: ThreadId,
        direction: crate::app_server_session::HistoryDirection,
    ) -> bool {
        if self.chat_widget.thread_id() != Some(thread_id) || self.pending_thread_switch_resets != 0
        {
            return false;
        }
        let Some(mut request) = app_server.begin_history_page(thread_id, direction) else {
            return false;
        };
        if direction == crate::app_server_session::HistoryDirection::Latest {
            request.params.limit = Some(crate::app_server_session::initial_history_page_limit(
                &self.config,
                &self.local_settings,
                crossterm::terminal::size().map_or(24, |(_, height)| height),
            ));
        }
        let request_handle = app_server.request_handle();
        let app_event_tx = self.app_event_tx.clone();
        let generation = self.history_window.generation;
        let cursor = match request.params.cursor.as_ref() {
            Some(codex_app_server_protocol::ThreadItemsListCursor::Opaque(cursor)) => {
                cursor.clone()
            }
            _ => request.token.clone(),
        };
        tokio::spawn(async move {
            let result = request_handle
                .request_typed::<ThreadItemsListResponse>(ClientRequest::ThreadItemsList {
                    request_id: request.request_id,
                    params: request.params,
                })
                .await
                .map_err(|error| error.to_string());
            app_event_tx.send(AppEvent::OlderThreadHistoryLoaded {
                thread_id,
                generation,
                token: Some(request.token),
                cursor,
                result,
            });
        });
        true
    }

    pub(super) async fn handle_older_history_page(
        &mut self,
        tui: &mut tui::Tui,
        app_server: &mut AppServerSession,
        thread_id: ThreadId,
        cursor: &str,
        result: Result<ThreadItemsListResponse, String>,
    ) -> Result<()> {
        if !app_server.is_older_history_page_pending(thread_id, cursor) {
            return Ok(());
        }
        use crate::app_server_session::HistoryDirection;
        let direction = app_server.pending_history_direction(thread_id);
        if self.chat_widget.thread_id() != Some(thread_id)
            || self.pending_thread_switch_resets != 0
            || (direction == HistoryDirection::Older
                && tui.is_owned_screen()
                && !self.scrollback_has_older_history)
        {
            app_server.cancel_older_history_page(thread_id, cursor);
            return Ok(());
        }
        let page = match result {
            Ok(page) => page,
            Err(error) => {
                if let Some(channel) = self.thread_event_channels.get(&thread_id) {
                    let store = Arc::clone(&channel.store);
                    let mut turns = std::mem::take(&mut store.lock().await.turns);
                    let failure = if error.contains("historyItemTooLarge")
                        || error.contains("historyPageTooLarge")
                    {
                        app_server
                            .fail_history_page(thread_id, None, &mut turns)
                            .await
                    } else {
                        app_server.cancel_older_history_page(thread_id, cursor);
                        Ok(())
                    };
                    merge_older_turns(&mut store.lock().await.turns, turns);
                    failure?;
                }
                self.scrollback_has_older_history = app_server.has_older_history(thread_id);
                self.chat_widget.add_error_message(error);
                self.transcript_view.history = TranscriptHistoryState::Failed;
                tui.frame_requester().schedule_frame();
                return Ok(());
            }
        };
        let Some(store) = self
            .thread_event_channels
            .get(&thread_id)
            .map(|channel| Arc::clone(&channel.store))
        else {
            app_server.cancel_older_history_page(thread_id, cursor);
            return Ok(());
        };
        let page_fits = page
            .data
            .iter()
            .try_fold(0usize, |sum, entry| {
                crate::history_window::payload_bytes(&entry.item)
                    .map(|size| sum.saturating_add(size))
            })
            .is_ok_and(|bytes| bytes <= crate::history_window::MAX_HISTORY_BYTES);
        if !page_fits {
            let mut turns = std::mem::take(&mut store.lock().await.turns);
            let rejected = page.data.first().map(|entry| entry.turn_id.as_str());
            let failure = app_server
                .fail_history_page(thread_id, rejected, &mut turns)
                .await;
            merge_older_turns(&mut store.lock().await.turns, turns);
            failure?;
            self.scrollback_has_older_history = app_server.has_older_history(thread_id);
            self.chat_widget
                .add_error_message(crate::history_window::OVERSIZED_HISTORY.to_string());
            self.transcript_view.history = TranscriptHistoryState::Failed;
            tui.frame_requester().schedule_frame();
            return Ok(());
        }
        let (cwd, mut turns) = {
            let mut store = store.lock().await;
            (
                store
                    .session
                    .as_ref()
                    .map_or_else(|| self.config.cwd.clone(), |session| session.cwd.clone()),
                std::mem::take(&mut store.turns),
            )
        };
        let items = match app_server
            .apply_older_history_page(thread_id, cursor, page, &mut turns)
            .await
        {
            Ok(items) => items,
            Err(error) => {
                merge_older_turns(&mut store.lock().await.turns, turns);
                return Err(error);
            }
        };
        if direction == HistoryDirection::Latest {
            self.transcript_cells
                .retain(|cell| is_session_history_prefix_cell(cell.as_ref()));
            self.transcript_view.retain_window(&self.transcript_cells);
            if let Some(Overlay::Transcript(overlay)) = self.overlay.as_mut() {
                overlay.retain_window(&self.transcript_cells);
            }
            self.transcript_view.jump_to_latest();
        }
        let hidden_item_ids = hidden_review_item_ids(&turns);
        let visibility = if self.config.show_raw_agent_reasoning {
            RawReasoningVisibility::Visible
        } else {
            RawReasoningVisibility::Hidden
        };
        let width = tui.terminal.last_known_screen_size.width;
        self.remove_hidden_review_cells(tui, &turns, &hidden_item_ids, thread_id, &cwd, visibility);
        let cells = self.project_older_history_cells(
            items,
            &turns,
            &hidden_item_ids,
            thread_id,
            &cwd,
            visibility,
        );
        if direction == HistoryDirection::Older {
            let inserted = self.prepend_older_transcript_cells(cells);
            self.transcript_view
                .history_loaded(&self.transcript_cells, inserted.clone());
            if !inserted.is_empty() {
                self.join_older_activity_group(inserted.end, &turns);
            }
        } else {
            for cell in cells {
                if let Some(Overlay::Transcript(overlay)) = self.overlay.as_mut() {
                    overlay.insert_cell(Arc::clone(&cell));
                }
                self.transcript_cells.push(cell);
            }
            self.transcript_view
                .history_loaded(&self.transcript_cells, 0..0);
        }
        {
            let mut store = store.lock().await;
            merge_older_turns(&mut store.turns, turns);
            bound_display_turns(&mut store.turns, direction == HistoryDirection::Older);
            app_server.reanchor_history_window(thread_id, &store.turns, direction);
        }
        self.bound_history_window(direction == HistoryDirection::Older);
        if direction == HistoryDirection::Latest {
            self.history_window.evicted = false;
        }
        self.scrollback_has_older_history = app_server.has_older_history(thread_id);
        if self.backtrack.overlay_preview_active
            && self.backtrack.nth_user_message == usize::MAX
            && !self.scrollback_has_older_history
        {
            self.cancel_transcript_browsing(tui);
            self.chat_widget.add_info_message(
                "No previous message to edit.".to_string(),
                /*hint*/ None,
            );
        }

        if tui.is_owned_screen() {
            self.finish_owned_history_page(tui, app_server, thread_id);
            return Ok(());
        }
        self.finish_inline_history_page(tui, app_server, thread_id, width);
        Ok(())
    }

    /// Remove prompts revealed as internal review input by an older page's review marker.
    fn remove_hidden_review_cells(
        &mut self,
        tui: &mut tui::Tui,
        turns: &[Turn],
        hidden_item_ids: &HashSet<&str>,
        thread_id: ThreadId,
        cwd: &AbsolutePathBuf,
        visibility: RawReasoningVisibility,
    ) {
        if hidden_item_ids.is_empty() {
            return;
        }
        let user_items = turns
            .iter()
            .flat_map(Turn::item_payloads)
            .filter(|item| matches!(item, ThreadItemPayload::UserMessage { .. }))
            .map(|item| (item.id().to_string(), item.clone()))
            .collect::<Vec<_>>();
        let projected_user_cells = thread_items_to_transcript_cells(
            Some(thread_id),
            cwd,
            user_items.iter().map(|(_, item)| item.clone()),
            visibility,
            Some(&self.config),
        );
        let persisted_user_cells = user_items
            .into_iter()
            .zip(projected_user_cells)
            .map(|((item_id, _), cell)| (item_id, cell))
            .collect();
        let indices = hidden_transcript_indices(
            &self.transcript_cells,
            persisted_user_cells,
            hidden_item_ids,
        );
        if indices.is_empty() {
            return;
        }
        if self.backtrack.overlay_preview_active {
            let selected_index = crate::app_backtrack::nth_user_position(
                &self.transcript_cells,
                self.backtrack.nth_user_message,
            );
            if selected_index.is_some_and(|selected| indices.contains(&selected)) {
                self.cancel_transcript_browsing(tui);
            } else {
                let removed_visible_users = indices
                    .iter()
                    .filter(|&&index| {
                        selected_index.is_some_and(|selected| index < selected)
                            && crate::app_backtrack::user_count(std::slice::from_ref(
                                &self.transcript_cells[index],
                            )) != 0
                    })
                    .count();
                self.backtrack.nth_user_message = self
                    .backtrack
                    .nth_user_message
                    .saturating_sub(removed_visible_users);
            }
        }
        for index in indices {
            self.transcript_cells.remove(index);
        }
        self.native_history.retain(&self.transcript_cells);
        self.transcript_view.restart_search();
        self.transcript_view
            .history_loaded(&self.transcript_cells, 0..0);
        if let Some(Overlay::Transcript(overlay)) = self.overlay.as_mut() {
            overlay.replace_cells(self.transcript_cells.clone());
        }
    }

    /// Project successful turn completion metadata after the corresponding page's final item.
    fn project_older_history_cells(
        &mut self,
        items: Vec<ThreadItemPayload>,
        turns: &[Turn],
        hidden_item_ids: &HashSet<&str>,
        thread_id: ThreadId,
        cwd: &AbsolutePathBuf,
        visibility: RawReasoningVisibility,
    ) -> Vec<Arc<dyn HistoryCell>> {
        let mut cells = Vec::new();
        for (items, completed_turn) in completion::group_completed_turn_items(items, turns) {
            // Internal prompts stay invisible, but still separate adjacent tool groups.
            for visible in items.split(|item| hidden_item_ids.contains(item.id())) {
                cells.extend(thread_items_to_transcript_cells(
                    Some(thread_id),
                    cwd,
                    visible.iter().cloned(),
                    visibility,
                    Some(&self.config),
                ));
            }
            if let Some(turn) = completed_turn
                && let Some(completion) = self
                    .chat_widget
                    .completion_cell(turn, Some(ReplayKind::ResumeInitialMessages))
            {
                cells.push(Arc::new(completion));
            }
        }
        cells
    }

    /// Insert each page once, keeping session headers and any backtrack selection in place.
    fn prepend_older_transcript_cells(&mut self, cells: Vec<Arc<dyn HistoryCell>>) -> Range<usize> {
        if self.backtrack.overlay_preview_active {
            let added_prompts = crate::app_backtrack::user_count(&cells);
            self.backtrack.nth_user_message = if self.backtrack.nth_user_message == usize::MAX {
                added_prompts.checked_sub(1).unwrap_or(usize::MAX)
            } else {
                self.backtrack
                    .nth_user_message
                    .saturating_add(added_prompts)
            };
        }
        let index = if let Some(Overlay::Transcript(overlay)) = self.overlay.as_mut() {
            overlay.prepend(cells.clone())
        } else {
            self.transcript_cells
                .iter()
                .rposition(|cell| is_session_history_prefix_cell(cell.as_ref()))
                .map_or(/*default*/ 0, |index| index.saturating_add(/*rhs*/ 1))
        };
        let inserted = index..index + cells.len();
        self.transcript_cells.splice(index..index, cells);
        inserted
    }

    /// Continue explicit history traversal without rewriting native scrollback.
    fn finish_owned_history_page(
        &mut self,
        tui: &mut tui::Tui,
        app_server: &mut AppServerSession,
        thread_id: ThreadId,
    ) {
        let previous = self.transcript_view.history;
        self.transcript_view.history = if self.scrollback_has_older_history {
            TranscriptHistoryState::Partial
        } else {
            TranscriptHistoryState::Complete
        };
        let continue_to_start = previous == TranscriptHistoryState::LoadingBeginning;
        if continue_to_start && !self.scrollback_has_older_history {
            if self.transcript_view.is_select_mode() {
                self.transcript_view
                    .complete_select_beginning(&self.transcript_cells);
            } else {
                self.transcript_view
                    .jump_to_entry(&self.transcript_cells, /*index*/ 0);
            }
        }
        if self.scrollback_has_older_history
            && (continue_to_start
                || self.browsing_needs_history()
                || self.transcript_view.needs_history(&self.transcript_cells))
            && self.request_older_history_page(app_server, thread_id)
        {
            self.transcript_view.history = if continue_to_start {
                TranscriptHistoryState::LoadingBeginning
            } else {
                TranscriptHistoryState::LoadingOlder
            };
        }
        if self.backtrack.overlay_preview_active {
            self.apply_backtrack_selection_internal(self.backtrack.nth_user_message);
        }
        tui.frame_requester().schedule_frame();
    }

    /// Preserve the legacy overlay and inline scrollback refill behavior.
    fn finish_inline_history_page(
        &mut self,
        tui: &mut tui::Tui,
        app_server: &mut AppServerSession,
        thread_id: ThreadId,
        width: u16,
    ) {
        let mut continue_to_start = false;
        if let Some(Overlay::Transcript(overlay)) = self.overlay.as_mut() {
            let previous_state = overlay.set_history_state(if self.scrollback_has_older_history {
                TranscriptHistoryState::Partial
            } else {
                TranscriptHistoryState::Complete
            });
            continue_to_start = previous_state == TranscriptHistoryState::LoadingBeginning
                && self.scrollback_has_older_history;
        } else {
            let wrap_width = self.chat_widget.history_wrap_width(width);
            let rendered_rows = self
                .render_transcript_lines_for_reflow(wrap_width)
                .lines
                .len();
            self.schedule_immediate_resize_reflow(tui);
            if self.scrollback_history_needs_top_up(rendered_rows)
                && self.request_older_history_page(app_server, thread_id)
            {
                return;
            }
        }
        if continue_to_start
            && self.request_older_history_page(app_server, thread_id)
            && let Some(Overlay::Transcript(overlay)) = self.overlay.as_mut()
        {
            overlay.set_history_state(TranscriptHistoryState::LoadingBeginning);
        }
        if self.backtrack.overlay_preview_active {
            self.apply_backtrack_selection_internal(self.backtrack.nth_user_message);
        }
        tui.frame_requester().schedule_frame();
    }
}

/// Classify internal review prompts across page boundaries before rendering any older items.
fn hidden_review_item_ids(turns: &[Turn]) -> HashSet<&str> {
    let mut hidden = HashSet::new();
    let mut review_mode = false;
    for (index, turn) in turns.iter().enumerate() {
        let hidden_nested_review_turn = index
            .checked_sub(/*rhs*/ 1)
            .and_then(|previous| turns.get(previous))
            .is_some_and(|previous| {
                crate::app_backtrack::is_hidden_nested_review_turn(previous, turn)
            });
        for item in &turn.items {
            match &item.payload {
                ThreadItemPayload::EnteredReviewMode { .. } => review_mode = true,
                ThreadItemPayload::ExitedReviewMode { .. } => review_mode = false,
                ThreadItemPayload::UserMessage { .. }
                    if review_mode || hidden_nested_review_turn =>
                {
                    hidden.insert(item.id());
                }
                _ => {}
            }
        }
    }
    hidden
}

/// Match repeated user messages from newest to oldest so identical visible prompts stay distinct.
fn hidden_transcript_indices(
    cells: &[Arc<dyn HistoryCell>],
    mut persisted_user_cells: Vec<(String, Arc<dyn HistoryCell>)>,
    hidden_item_ids: &HashSet<&str>,
) -> Vec<usize> {
    let mut indices = Vec::new();
    for (index, cell) in cells.iter().enumerate().rev() {
        let Some(user_cell) = cell.as_any().downcast_ref::<UserHistoryCell>() else {
            continue;
        };
        let Some(position) = persisted_user_cells.iter().rposition(|(_, projected)| {
            projected
                .as_any()
                .downcast_ref::<UserHistoryCell>()
                .is_some_and(|projected| {
                    projected.message == user_cell.message
                        && projected.text_elements == user_cell.text_elements
                        && projected.local_image_paths == user_cell.local_image_paths
                        && projected.remote_image_urls == user_cell.remote_image_urls
                })
        }) else {
            continue;
        };
        if hidden_item_ids.contains(persisted_user_cells[position].0.as_str()) {
            indices.push(index);
        }
        persisted_user_cells.truncate(position);
    }
    indices
}

/// Preserve events received while the request was in flight and deduplicate overlapping pages.
fn merge_older_turns(current_turns: &mut Vec<Turn>, mut older_turns: Vec<Turn>) {
    older_turns.retain_mut(|turn| {
        let Some(current) = current_turns
            .iter_mut()
            .find(|current| current.id == turn.id)
        else {
            return true;
        };
        let items = std::mem::take(&mut turn.items)
            .into_iter()
            .filter(|item| !current.items.iter().any(|known| known.id() == item.id()))
            .collect::<Vec<_>>();
        current.items.splice(0..0, items);
        false
    });
    current_turns.splice(0..0, older_turns);
}

fn admit_display_item(item: &ThreadItem, items: &mut usize, bytes: &mut usize) -> bool {
    if *items >= crate::history_window::MAX_HISTORY_ITEMS {
        return false;
    }
    let Ok(size) = crate::history_window::payload_bytes(item) else {
        return false;
    };
    if bytes.saturating_add(size) > crate::history_window::MAX_HISTORY_BYTES {
        return false;
    }
    *items += 1;
    *bytes += size;
    true
}

/// Replay readers may copy only the bounded display projection, never the full input history.
pub(super) fn clone_display_turns(turns: &[Turn]) -> Vec<Turn> {
    let mut items = 0usize;
    let mut bytes = 0usize;
    let mut retained = turns
        .iter()
        .rev()
        .take(crate::history_window::MAX_HISTORY_TURNS)
        .map(|turn| {
            let mut copy = crate::history_window::turn_shell(turn);
            copy.items = turn
                .items
                .iter()
                .rev()
                .filter(|item| admit_display_item(item, &mut items, &mut bytes))
                .cloned()
                .collect();
            copy.items.reverse();
            copy.items_view = if copy.items.len() == turn.items.len() {
                turn.items_view
            } else if copy.items.is_empty() {
                codex_app_server_protocol::TurnItemsView::NotLoaded
            } else {
                codex_app_server_protocol::TurnItemsView::Summary
            };
            copy
        })
        .collect::<Vec<_>>();
    retained.reverse();
    retained
}

pub(super) fn bound_display_turns(turns: &mut Vec<Turn>, prefer_older: bool) {
    let count = turns.len();
    if count > crate::history_window::MAX_HISTORY_TURNS {
        if prefer_older {
            turns.truncate(crate::history_window::MAX_HISTORY_TURNS);
        } else {
            turns.drain(..count - crate::history_window::MAX_HISTORY_TURNS);
        }
    }
    let mut items = 0usize;
    let mut bytes = 0usize;
    let visit = |turn: &mut Turn, items: &mut usize, bytes: &mut usize| {
        let count = turn.items.len();
        turn.items
            .retain(|item| admit_display_item(item, items, bytes));
        if count != turn.items.len() {
            turn.items_view = if turn.items.is_empty() {
                codex_app_server_protocol::TurnItemsView::NotLoaded
            } else {
                codex_app_server_protocol::TurnItemsView::Summary
            };
        }
    };
    if prefer_older {
        for turn in turns.iter_mut() {
            visit(turn, &mut items, &mut bytes);
        }
    } else {
        for turn in turns.iter_mut().rev() {
            turn.items.reverse();
            visit(turn, &mut items, &mut bytes);
            turn.items.reverse();
        }
    }
}

#[cfg(test)]
#[path = "history_pagination_tests.rs"]
mod tests;
