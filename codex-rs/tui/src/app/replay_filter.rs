// Modified for OpenDIY Codex R33 (2026-11-02); see MODIFICATIONS.md.
//! Helpers for deciding which buffered events to replay when switching threads.

use codex_app_server_protocol::CodexErrorInfo;
use codex_app_server_protocol::ServerNotification;
use codex_app_server_protocol::ServerRequest;
use codex_app_server_protocol::ThreadItemPayload;
use codex_app_server_protocol::TurnStatus;
use std::collections::HashMap;
use std::collections::HashSet;

use super::ThreadBufferedEvent;
use super::ThreadEventSnapshot;

/// Completed history and the live buffer can overlap after pagination. Suppress only
/// identical message displays in this copy; keep terminal events and live questions.
pub(super) fn omit_messages_covered_by_history(snapshot: &mut ThreadEventSnapshot) {
    let Some(session) = &snapshot.session else {
        return;
    };
    let thread_id = session.thread_id.to_string();
    let mut covered = snapshot
        .turns
        .iter()
        .filter(|turn| turn.status == TurnStatus::Completed)
        .flat_map(|turn| {
            turn.item_payloads().filter_map(move |item| {
                (matches!(
                    item,
                    ThreadItemPayload::UserMessage { .. } | ThreadItemPayload::AgentMessage { .. }
                ) && !item.id().is_empty())
                .then_some(((turn.id.clone(), item.id().to_string()), item))
            })
        })
        .collect::<HashMap<_, _>>();
    // The same ID with a newer payload is not covered by an older history page.
    for event in &snapshot.events {
        let ThreadBufferedEvent::Notification(notification) = event else {
            continue;
        };
        let (event_thread, turn_id, items): (_, _, &[codex_app_server_protocol::ThreadItem]) =
            match notification.as_ref() {
                ServerNotification::ItemCompleted(n) => (
                    &n.thread_id,
                    n.turn_id.as_str(),
                    std::slice::from_ref(&n.item),
                ),
                ServerNotification::TurnCompleted(n) => {
                    (&n.thread_id, n.turn.id.as_str(), &n.turn.items)
                }
                _ => continue,
            };
        if event_thread != &thread_id {
            continue;
        }
        for item in items {
            let key = (turn_id.to_string(), item.id().to_string());
            if covered
                .get(&key)
                .is_some_and(|previous| *previous != &item.payload)
            {
                covered.remove(&key);
            }
        }
    }
    let mut completed = HashSet::new();
    snapshot.events.reverse();
    snapshot.events.retain_mut(|event| {
        let ThreadBufferedEvent::Notification(notification) = event else {
            return true;
        };
        match notification.as_mut() {
            ServerNotification::ItemCompleted(n) if n.thread_id == thread_id => {
                let key = (n.turn_id.clone(), n.item.id().to_string());
                if covered
                    .get(&key)
                    .is_some_and(|previous| *previous == &n.item.payload)
                {
                    completed.insert((n.turn_id.clone(), n.item.id().to_string()));
                    match &mut n.item.payload {
                        ThreadItemPayload::UserMessage { .. } => return false,
                        // The item handler still restores questions and completion state.
                        ThreadItemPayload::AgentMessage { text, .. } => text.clear(),
                        _ => {}
                    }
                }
            }
            ServerNotification::TurnCompleted(n) if n.thread_id == thread_id => {
                for item in &mut n.turn.items {
                    let key = (n.turn.id.clone(), item.id().to_string());
                    if covered
                        .get(&key)
                        .is_some_and(|previous| *previous == &item.payload)
                        && let ThreadItemPayload::AgentMessage { id, text, .. } = &mut item.payload
                    {
                        completed.insert((n.turn.id.clone(), id.clone()));
                        text.clear();
                    }
                }
            }
            ServerNotification::AgentMessageDelta(n) if n.thread_id == thread_id => {
                return !completed.contains(&(n.turn_id.clone(), n.item_id.clone()));
            }
            _ => {}
        }
        true
    });
    snapshot.events.reverse();
}

pub(super) fn snapshot_has_pending_interactive_request(snapshot: &ThreadEventSnapshot) -> bool {
    snapshot.events.iter().any(|event| {
        matches!(
            event,
            ThreadBufferedEvent::Request(request)
                if matches!(
                    request.as_ref(),
                    ServerRequest::CommandExecutionRequestApproval { .. }
                        | ServerRequest::FileChangeRequestApproval { .. }
                        | ServerRequest::McpServerElicitationRequest { .. }
                        | ServerRequest::PermissionsRequestApproval { .. }
                        | ServerRequest::ToolRequestUserInput { .. }
                )
        )
    })
}

/// A later turn resolves an earlier precaution; replay must preserve restored input.
pub(super) fn omit_resolved_misalignment_errors(
    snapshot: &mut ThreadEventSnapshot,
    latest_turn: &str,
) {
    snapshot.events.retain(|event| !matches!(
        event,
        ThreadBufferedEvent::Notification(notification)
            if matches!(notification.as_ref(), ServerNotification::Error(n)
                if n.turn_id != latest_turn
                    && n.error.codex_error_info == Some(CodexErrorInfo::MisalignmentPolicyViolation))
    ));
    let completed_turns = snapshot.events.iter_mut().filter_map(|event| match event {
        ThreadBufferedEvent::Notification(notification) => match notification.as_mut() {
            ServerNotification::TurnCompleted(n) => Some(&mut n.turn),
            _ => None,
        },
        _ => None,
    });
    for turn in snapshot.turns.iter_mut().chain(completed_turns) {
        if turn.id != latest_turn
            && turn.error.as_ref().is_some_and(|error| {
                error.codex_error_info == Some(CodexErrorInfo::MisalignmentPolicyViolation)
            })
        {
            turn.error = None;
        }
    }
}

/// A completed item's full text replaces its earlier streaming deltas during replay.
/// Keep deltas without a later completion so an in-progress or truncated stream still renders.
/// Other events are barriers: streaming text may need to flush before a tool or prompt.
/// This only changes the replay snapshot; the live notification store remains untouched.
pub(super) fn omit_completed_agent_deltas(events: &mut Vec<ThreadBufferedEvent>) {
    let mut completed = HashSet::new();
    events.reverse();
    events.retain(|event| {
        if let ThreadBufferedEvent::Notification(notification) = event {
            match notification.as_ref() {
                ServerNotification::ItemCompleted(notification) => {
                    if let ThreadItemPayload::AgentMessage { id, .. } = &notification.item.payload {
                        completed.insert((
                            notification.thread_id.clone(),
                            notification.turn_id.clone(),
                            id.clone(),
                        ));
                    } else {
                        completed.clear();
                    }
                }
                ServerNotification::AgentMessageDelta(notification) => {
                    return !completed.contains(&(
                        notification.thread_id.clone(),
                        notification.turn_id.clone(),
                        notification.item_id.clone(),
                    ));
                }
                _ => completed.clear(),
            }
        } else {
            completed.clear();
        }
        true
    });
    events.reverse();
}
