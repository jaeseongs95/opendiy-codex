// Modified for OpenDIY Codex R32 (2026-11-01); see MODIFICATIONS.md.
//! Bounded replay-buffer policy for per-thread app-server events.

use super::ServerNotification;
use super::ThreadBufferedEvent;
use super::ThreadEventStore;
use std::borrow::Cow;

// Keep merged text finite so continued streaming still reaches bounded replay eviction.
const MAX_COALESCED_AGENT_MESSAGE_DELTA_BYTES: usize = 4 * 1024;
const MAX_BUFFERED_AGENT_MESSAGE_DELTA_BYTES: usize = 256 * 1024;

impl ThreadEventStore {
    pub(super) fn push_replay_notification(&mut self, notification: Cow<'_, ServerNotification>) {
        if let ServerNotification::AgentMessageDelta(delta) = notification.as_ref()
            && delta.delta.len() > MAX_BUFFERED_AGENT_MESSAGE_DELTA_BYTES
        {
            return;
        }

        if let ServerNotification::AgentMessageDelta(delta) = notification.as_ref()
            && let Some(ThreadBufferedEvent::Notification(previous)) = self.buffer.back_mut()
            && let ServerNotification::AgentMessageDelta(previous) = previous.as_mut()
            && previous.thread_id == delta.thread_id
            && previous.turn_id == delta.turn_id
            && previous.item_id == delta.item_id
            && previous.delta.len().saturating_add(delta.delta.len())
                <= MAX_COALESCED_AGENT_MESSAGE_DELTA_BYTES
        {
            previous.delta.push_str(&delta.delta);
            if let Some(size) = self.buffered_event_sizes.back_mut() {
                *size += delta.delta.len();
            }
            self.buffered_display_bytes += delta.delta.len();
            self.buffered_agent_message_delta_bytes = self
                .buffered_agent_message_delta_bytes
                .saturating_add(delta.delta.len());
            self.evict_overflowing_events();
            return;
        }

        if !matches!(
            notification.as_ref(),
            ServerNotification::AgentMessageDelta(_)
        ) && crate::history_window::payload_bytes(notification.as_ref()).is_err()
        {
            return;
        }
        self.push_buffered_event(ThreadBufferedEvent::Notification(Box::new(
            notification.into_owned(),
        )));
    }

    pub(super) fn display_event_bytes(event: &ThreadBufferedEvent) -> usize {
        match event {
            // Requests are protected operational state, outside the display cache budget.
            ThreadBufferedEvent::Request(_) => 0,
            ThreadBufferedEvent::Notification(notification) => {
                if let ServerNotification::AgentMessageDelta(delta) = notification.as_ref() {
                    delta.delta.len()
                } else {
                    crate::history_window::payload_bytes(notification.as_ref())
                        .unwrap_or(crate::history_window::MAX_HISTORY_BYTES + 1)
                }
            }
            ThreadBufferedEvent::HistoryEntryResponse(_)
            | ThreadBufferedEvent::FeedbackSubmission(_) => 0,
        }
    }

    pub(super) fn push_buffered_event(&mut self, event: ThreadBufferedEvent) {
        let size = Self::display_event_bytes(&event);
        if size > crate::history_window::MAX_HISTORY_BYTES {
            return;
        }
        self.buffered_event_sizes.push_back(size);
        self.buffered_display_bytes += size;
        if let ThreadBufferedEvent::Notification(notification) = &event
            && let ServerNotification::AgentMessageDelta(delta) = notification.as_ref()
        {
            self.buffered_agent_message_delta_bytes = self
                .buffered_agent_message_delta_bytes
                .saturating_add(delta.delta.len());
        }
        self.buffer.push_back(event);
        self.evict_overflowing_events();
    }

    fn evict_overflowing_events(&mut self) {
        while self
            .buffer
            .iter()
            .filter(|event| {
                !matches!(event, ThreadBufferedEvent::Request(request)
                if self.pending_interactive_replay.should_replay_snapshot_request(request))
            })
            .count()
            > self.capacity.min(crate::history_window::MAX_HISTORY_ITEMS)
            || self.buffered_display_bytes > crate::history_window::MAX_HISTORY_BYTES
            || self.buffered_agent_message_delta_bytes > MAX_BUFFERED_AGENT_MESSAGE_DELTA_BYTES
        {
            let Some(index) = self.buffer.iter().position(|event| {
                !matches!(event,
                ThreadBufferedEvent::Request(request)
                    if self.pending_interactive_replay.should_replay_snapshot_request(request))
            }) else {
                break;
            };
            let Some(removed) = self.buffer.remove(index) else {
                break;
            };
            self.buffered_display_bytes = self
                .buffered_display_bytes
                .saturating_sub(self.buffered_event_sizes.remove(index).unwrap_or(0));
            match removed {
                ThreadBufferedEvent::Notification(notification) => {
                    if let ServerNotification::AgentMessageDelta(delta) = notification.as_ref() {
                        self.buffered_agent_message_delta_bytes = self
                            .buffered_agent_message_delta_bytes
                            .saturating_sub(delta.delta.len());
                    }
                }
                ThreadBufferedEvent::Request(request) => self
                    .pending_interactive_replay
                    .note_evicted_server_request(request.as_ref()),
                ThreadBufferedEvent::HistoryEntryResponse(_)
                | ThreadBufferedEvent::FeedbackSubmission(_) => {}
            }
        }
    }
}

#[cfg(test)]
#[path = "thread_event_buffer_tests.rs"]
mod tests;
