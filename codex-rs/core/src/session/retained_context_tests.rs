// Modified for OpenDIY Codex R31 (2026-11-01); see MODIFICATIONS.md.
use std::sync::Arc;
use std::time::Duration;

use codex_history::RetainedContextEntry;
use codex_history::RetainedUserMessage;
use pretty_assertions::assert_eq;
use test_case::test_case;

use super::thread_settings;
use crate::session::tests::make_session_and_context;

fn delivered_message() -> RetainedUserMessage {
    RetainedUserMessage {
        origin: codex_history::UserInputOrigin::User,
        turn_id: "turn".to_owned(),
        message_id: Some("send".to_owned()),
        text: "May I publish?".to_owned(),
        complete: true,
        phase: None,
    }
}

#[test_case(false; "runtime supplementation")]
#[test_case(true; "official replacement")]
#[tokio::test]
async fn tool_catalog_updates_do_not_deadlock_a_user_reply_behind_delivery(replace: bool) {
    let (session, _) = make_session_and_context().await;
    let session = Arc::new(session);
    let active = crate::state::ActiveTurn::default();
    let (accepted, response) = tokio::sync::oneshot::channel();
    active
        .turn_state
        .lock()
        .await
        .insert_pending_user_input("reply".into(), accepted);
    *session.active_turn.lock().await = Some(active);

    // Hold the real state lock only to deterministically stop the delivered-message
    // reservation after it owns the communication boundary, before it can persist.
    let state = session.state.lock().await;
    let (mut recording, _) = session.record_delivered_assistant_message(delivered_message());
    let mut reply = Box::pin(session.notify_user_input_response(
        "reply",
        codex_protocol::request_user_input::RequestUserInputResponse {
            answers: Default::default(),
        },
    ));
    assert!(futures::poll!(reply.as_mut()).is_pending());
    // The public reply now owns active_turn and is waiting on that real boundary.
    // The former catalog implementation took persistence here before active_turn,
    // completing a cycle when the delivery reservation was released below.
    let mut update = Box::pin(async {
        if replace {
            session
                .replace_dynamic_tools(Vec::new())
                .await
                .map(|()| false)
        } else {
            session
                .supplement_runtime_dynamic_tools(
                    7,
                    tokio_util::sync::CancellationToken::new(),
                    Vec::new(),
                    &[],
                )
                .await
        }
    });
    assert!(futures::poll!(update.as_mut()).is_pending());
    drop(state);
    let completed = tokio::time::timeout(Duration::from_secs(1), async {
        tokio::join!(&mut recording, &mut reply, &mut update)
    })
    .await;
    if completed.is_err() {
        recording.abort();
    }
    let (recorded, (), updated) = completed
        .expect("catalog update must not close the active/boundary/persistence lock cycle");
    recorded.expect("delivered message persisted");
    assert!(!updated.expect("catalog update completes, with busy for runtime supplementation"));
    let reply_order = response
        .await
        .expect("official user reply accepted")
        .acceptance_order;
    let assistant_order = session
        .clone_history()
        .await
        .retained_context()
        .ordered_entries()
        .find_map(|(order, entry)| {
            matches!(entry, RetainedContextEntry::AssistantMessage(_)).then_some(order)
        })
        .expect("confirmed delivery retained");
    assert!(assistant_order < codex_history::RetainedContextOrder::Local(reply_order));
}

#[tokio::test]
async fn shutdown_retains_an_admitted_delivery_until_persistence_finishes() {
    let (session, _) = make_session_and_context().await;
    let session = Arc::new(session);
    let checkpoint = thread_settings::acquire_persistence_lock(&session).await;
    let dispatch = session.track_code_mode_message().expect("open dispatch");
    let mut shutdown = Box::pin(session.drain_code_mode_messages());
    assert!(futures::poll!(shutdown.as_mut()).is_pending());
    assert!(session.track_code_mode_message().is_none());

    let message = delivered_message();
    let (admitted, receiver) = tokio::sync::oneshot::channel();
    let outer_session = Arc::clone(&session);
    let outer_message = message.clone();
    let outer = tokio::spawn(async move {
        let (_, completion) = outer_session.record_delivered_assistant_message(outer_message);
        admitted
            .send(completion)
            .expect("receive recording admission");
        futures::future::pending::<()>().await;
    });
    let mut completion = receiver.await.expect("recording admitted");
    outer.abort();
    assert!(
        outer
            .await
            .expect_err("outer call interrupted")
            .is_cancelled()
    );
    drop(dispatch);
    let live_messages = tokio::time::timeout(Duration::from_secs(/*secs*/ 5), async {
        loop {
            let history = session.clone_history().await;
            let messages = history
                .retained_context()
                .ordered_entries()
                .filter_map(|(_, entry)| match entry {
                    RetainedContextEntry::AssistantMessage(message) => Some(message.clone()),
                    RetainedContextEntry::UserMessage(_)
                    | RetainedContextEntry::VerifiedAnswer(_) => None,
                })
                .collect::<Vec<_>>();
            if !messages.is_empty() {
                break messages;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("live admission does not wait for the checkpoint");
    assert_eq!(live_messages, vec![message]);
    assert!(futures::poll!(shutdown.as_mut()).is_pending());
    assert!(!completion.has_changed().expect("recording remains open"));

    drop(checkpoint);
    shutdown.await;
    assert!(completion.changed().await.is_err());
}

#[test_case(false; "free state")]
#[test_case(true; "busy state")]
#[tokio::test]
async fn confirmed_delivery_reserves_order_and_persists_before_a_fast_user_reply(busy: bool) {
    let (session, _) = make_session_and_context().await;
    let session = Arc::new(session);
    let checkpoint = thread_settings::acquire_persistence_lock(&session).await;
    let state = if busy {
        Some(session.state.lock().await)
    } else {
        None
    };
    let (recording, _) = session.record_delivered_assistant_message(delivered_message());
    let reply_session = Arc::clone(&session);
    let mut reply = tokio::spawn(async move { reply_session.reserve_user_input_order().await });
    drop(state);
    assert!(
        tokio::time::timeout(Duration::from_millis(/*millis*/ 100), &mut reply)
            .await
            .is_err()
    );
    drop(checkpoint);
    let reply_order = reply
        .await
        .expect("reply accepted after delivery persistence");
    recording.await.expect("confirmed delivery recorded");
    let assistant_order = session
        .clone_history()
        .await
        .retained_context()
        .ordered_entries()
        .find_map(|(order, entry)| {
            matches!(entry, RetainedContextEntry::AssistantMessage(_)).then_some(order)
        })
        .expect("confirmed delivery order");
    assert!(assistant_order < codex_history::RetainedContextOrder::Local(reply_order));
}
