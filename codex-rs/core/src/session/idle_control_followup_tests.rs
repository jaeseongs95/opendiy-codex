// Modified for OpenDIY Codex R32 (2026-11-01); see MODIFICATIONS.md.
//! Idle control admission and task ownership regressions.
//!
//! These tests exercise the real per-session reservation and task registration.
//! Loaded-runtime and parent/child coverage is in thread_manager's companion
//! tests. These fixtures alone do not prove the app-server dispatch boundary.

use super::*;
use codex_protocol::turn_input::NotSubmittedReason;
use pretty_assertions::assert_eq;

fn late_mail(trigger_turn: bool) -> InterAgentCommunication {
    InterAgentCommunication::new(
        AgentPath::try_from("/root/worker").expect("valid worker path"),
        AgentPath::root(),
        Vec::new(),
        "mail must survive idle compaction".into(),
        trigger_turn,
    )
}

#[tokio::test]
async fn idle_compact_busy_reservation_preserves_task_settings_history_and_fence() {
    let (session, context, _events) = make_session_and_context_with_rx().await;
    session
        .spawn_task(
            Arc::clone(&context),
            Vec::new(),
            HeldStepTask {
                kind: TaskKind::Regular,
                finish: Arc::new(Notify::new()),
            },
        )
        .await;
    let cancellation = session
        .active_turn
        .lock()
        .await
        .as_ref()
        .expect("active turn")
        .task
        .as_ref()
        .expect("active task")
        .cancellation_token
        .clone();
    let settings = session.thread_settings_snapshot().await;
    let history = session.clone_history().await.annotated_items().to_vec();
    let revision = session.cleanup_admission.activity_revision();

    assert!(matches!(
        session
            .reserve_compact_if_idle()
            .await
            .expect("open admission"),
        Err(NotSubmittedReason::NotIdle)
    ));

    assert_eq!(session.thread_settings_snapshot().await, settings);
    assert_eq!(session.clone_history().await.annotated_items(), history);
    assert_eq!(session.cleanup_admission.activity_revision(), revision);
    assert!(!cancellation.is_cancelled());
    assert_eq!(
        session
            .active_turn
            .lock()
            .await
            .as_ref()
            .expect("busy task remains owned")
            .task
            .as_ref()
            .expect("busy task remains registered")
            .turn_context
            .sub_id,
        context.sub_id
    );
    assert!(!session.input_queue.has_pending_mailbox_items().await);
    session.abort_all_tasks(TurnAbortReason::Replaced).await;
}

#[tokio::test]
async fn idle_compact_mail_and_closed_admission_reject_without_side_effects() {
    for trigger_turn in [false, true] {
        for closing in [false, true] {
            let (session, _context, _events) = make_session_and_context_with_rx().await;
            let communication = late_mail(trigger_turn);
            session
                .input_queue
                .enqueue_mailbox_communication(communication.clone().into(), Default::default())
                .await;
            if closing {
                session.cleanup_admission.close();
            }
            let settings = session.thread_settings_snapshot().await;
            let history = session.clone_history().await.annotated_items().to_vec();
            let revision = session.cleanup_admission.activity_revision();

            let result = session.reserve_compact_if_idle().await;
            if closing {
                assert!(result.is_err());
            } else {
                assert!(matches!(
                    result.expect("open admission"),
                    Err(NotSubmittedReason::PendingTriggerTurn)
                ));
            }

            assert!(session.active_turn.lock().await.is_none());
            assert_eq!(session.thread_settings_snapshot().await, settings);
            assert_eq!(session.clone_history().await.annotated_items(), history);
            assert_eq!(session.cleanup_admission.activity_revision(), revision);
            assert_eq!(
                session.input_queue.drain_mailbox_input_items().await.0,
                vec![TurnInput::InterAgentCommunication(communication.into())]
            );
        }
    }
}

#[tokio::test]
async fn idle_compact_reserved_registration_keeps_late_mail_for_next_turn() {
    for trigger_turn in [false, true] {
        let (session, context, _events) = make_session_and_context_with_rx().await;
        let reservation = session
            .reserve_compact_if_idle()
            .await
            .expect("open admission")
            .expect("idle reservation");
        assert!(
            !reservation
                .lock()
                .await
                .accepts_mailbox_delivery_for_current_turn(),
            "mail must target the next turn before context preparation starts"
        );
        let communication = late_mail(trigger_turn);
        session
            .input_queue
            .enqueue_mailbox_communication(communication.clone().into(), Default::default())
            .await;
        assert!(session.reserve_pending_work_turn().await.is_none());
        assert!(
            session
                .input_queue
                .get_pending_input(&session.active_turn)
                .await
                .0
                .is_empty()
        );

        assert!(
            session
                .start_task_with_reservation(
                    context,
                    Vec::new(),
                    HeldStepTask {
                        kind: TaskKind::Compact,
                        finish: Arc::new(Notify::new()),
                    },
                    Some(Arc::clone(&reservation)),
                )
                .await
        );

        assert!(session.input_queue.has_pending_mailbox_items().await);
        assert!(
            session
                .input_queue
                .take_pending_input_for_turn_state(reservation.as_ref())
                .await
                .is_empty(),
            "task registration must not drain mailbox into the compact turn"
        );
        assert!(
            !reservation
                .lock()
                .await
                .accepts_mailbox_delivery_for_current_turn()
        );
        {
            let active = session.active_turn.lock().await;
            assert!(Arc::ptr_eq(
                &active
                    .as_ref()
                    .expect("reserved task registered")
                    .turn_state,
                &reservation
            ));
            assert_eq!(
                active
                    .as_ref()
                    .expect("active task")
                    .task
                    .as_ref()
                    .expect("registered task")
                    .kind,
                TaskKind::Compact
            );
        }

        // Cleanup does not run an actual model turn. Prove that the existing
        // pending-work owner can still reserve trigger mail after compaction.
        session.abort_all_tasks(TurnAbortReason::Replaced).await;
        assert!(session.active_turn.lock().await.is_none());
        if trigger_turn {
            assert!(session.reserve_pending_work_turn().await.is_some());
            session.abort_all_tasks(TurnAbortReason::Replaced).await;
        }
        assert_eq!(
            session.input_queue.drain_mailbox_input_items().await.0,
            vec![TurnInput::InterAgentCommunication(communication.into())]
        );
    }
}

#[tokio::test]
async fn expected_interrupt_keeps_replacement_turn_and_cancels_only_matching_turn() {
    let (session, first, events) = make_session_and_context_with_rx().await;
    let finish = Arc::new(Notify::new());
    session
        .spawn_task(
            Arc::clone(&first),
            Vec::new(),
            HeldStepTask {
                kind: TaskKind::Regular,
                finish: Arc::clone(&finish),
            },
        )
        .await;
    finish.notify_one();
    recv_terminal_event(&events, TerminalEventKind::TurnComplete).await;

    let second = session
        .new_turn_with_default_settings("replacement-turn".into(), Default::default())
        .await;
    session
        .spawn_task(
            Arc::clone(&second),
            Vec::new(),
            HeldStepTask {
                kind: TaskKind::Regular,
                finish: Arc::new(Notify::new()),
            },
        )
        .await;
    let cancellation = session
        .active_turn
        .lock()
        .await
        .as_ref()
        .expect("replacement turn")
        .task
        .as_ref()
        .expect("replacement task")
        .cancellation_token
        .clone();
    let history = session.clone_history().await.annotated_items().to_vec();

    assert!(
        !session
            .abort_turn_if_active(&first.sub_id, TurnAbortReason::Interrupted, None)
            .await
    );
    assert!(!cancellation.is_cancelled());
    assert!(!session.is_interrupted());
    assert_eq!(session.clone_history().await.annotated_items(), history);
    assert_eq!(
        session
            .active_turn
            .lock()
            .await
            .as_ref()
            .expect("replacement retains ownership")
            .task
            .as_ref()
            .expect("replacement remains registered")
            .turn_context
            .sub_id,
        second.sub_id
    );

    assert!(
        session
            .abort_turn_if_active(&second.sub_id, TurnAbortReason::Interrupted, None)
            .await
    );
    assert!(cancellation.is_cancelled());
    let terminal = recv_terminal_event(&events, TerminalEventKind::TurnAborted).await;
    let EventMsg::TurnAborted(aborted) = terminal.msg else {
        unreachable!("terminal helper selected an aborted event")
    };
    assert_eq!(aborted.turn_id.as_deref(), Some(second.sub_id.as_str()));
    assert!(session.active_turn.lock().await.is_none());
}

#[tokio::test]
async fn untagged_interrupt_handler_keeps_existing_current_turn_semantics() {
    let (session, context, events) = make_session_and_context_with_rx().await;
    session
        .spawn_task(
            Arc::clone(&context),
            Vec::new(),
            HeldStepTask {
                kind: TaskKind::Regular,
                finish: Arc::new(Notify::new()),
            },
        )
        .await;

    // This is the existing Op::Interrupt handler, without an expected turn ID.
    crate::session::handlers::interrupt(&session).await;

    let terminal = recv_terminal_event(&events, TerminalEventKind::TurnAborted).await;
    let EventMsg::TurnAborted(aborted) = terminal.msg else {
        unreachable!("terminal helper selected an aborted event")
    };
    assert_eq!(aborted.turn_id.as_deref(), Some(context.sub_id.as_str()));
    assert!(session.is_interrupted());
    assert!(session.active_turn.lock().await.is_none());
}

#[tokio::test]
async fn idle_compact_mail_arriving_while_admission_waits_keeps_producer_wake() {
    let (session, _context, _events) = make_session_and_context_with_rx().await;
    let settings = session.thread_settings_snapshot().await;
    let history = session.clone_history().await.annotated_items().to_vec();
    let revision = session.cleanup_admission.activity_revision();

    // The first poll stops exactly at the active-turn mutex. The producer can
    // enqueue while admission is waiting; no elapsed-time race is involved.
    let active = session.active_turn.lock().await;
    let reservation = session.reserve_compact_if_idle();
    tokio::pin!(reservation);
    assert!(futures::poll!(reservation.as_mut()).is_pending());
    let communication = late_mail(true);
    session
        .input_queue
        .enqueue_mailbox_communication(communication.clone().into(), Default::default())
        .await;
    drop(active);

    assert!(matches!(
        reservation.await.expect("open admission"),
        Err(NotSubmittedReason::PendingTriggerTurn)
    ));
    assert!(session.active_turn.lock().await.is_none());
    assert!(session.input_queue.has_trigger_turn_mailbox_items().await);
    assert_eq!(session.thread_settings_snapshot().await, settings);
    assert_eq!(session.clone_history().await.annotated_items(), history);
    assert_eq!(session.cleanup_admission.activity_revision(), revision);

    // The rejected compact reservation must not claim or erase the producer's
    // work: the real pending-work admission can still own the queued trigger.
    assert!(session.reserve_pending_work_turn().await.is_some());
    session.abort_all_tasks(TurnAbortReason::Replaced).await;
    assert_eq!(
        session.input_queue.drain_mailbox_input_items().await.0,
        vec![TurnInput::InterAgentCommunication(communication.into())]
    );
}

#[tokio::test]
async fn idle_compact_stale_reservation_never_revives_or_steals_replacement_mail() {
    for replacement_running in [false, true] {
        let (session, stale_context, _events) = make_session_and_context_with_rx().await;
        let stale = session
            .reserve_compact_if_idle()
            .await
            .expect("open admission")
            .expect("initial idle reservation");
        session.abort_all_tasks(TurnAbortReason::Replaced).await;
        assert!(session.active_turn.lock().await.is_none());

        let replacement = session
            .new_turn_with_default_settings("replacement-owner".into(), Default::default())
            .await;
        if replacement_running {
            session
                .spawn_task(
                    Arc::clone(&replacement),
                    Vec::new(),
                    HeldStepTask {
                        kind: TaskKind::Regular,
                        finish: Arc::new(Notify::new()),
                    },
                )
                .await;
        }
        let replacement_cancellation = session
            .active_turn
            .lock()
            .await
            .as_ref()
            .and_then(|turn| turn.task.as_ref())
            .map(|task| task.cancellation_token.clone());
        let communication = late_mail(true);
        session
            .input_queue
            .enqueue_mailbox_communication(communication.clone().into(), Default::default())
            .await;
        let history = session.clone_history().await.annotated_items().to_vec();
        let settings = session.thread_settings_snapshot().await;
        let revision = session.cleanup_admission.activity_revision();

        assert!(
            !session
                .start_task_with_reservation(
                    stale_context,
                    Vec::new(),
                    HeldStepTask {
                        kind: TaskKind::Compact,
                        finish: Arc::new(Notify::new()),
                    },
                    Some(stale),
                )
                .await
        );

        assert_eq!(session.clone_history().await.annotated_items(), history);
        assert_eq!(session.thread_settings_snapshot().await, settings);
        assert_eq!(session.cleanup_admission.activity_revision(), revision);
        {
            let active = session.active_turn.lock().await;
            if replacement_running {
                let task = active
                    .as_ref()
                    .expect("replacement owner remains")
                    .task
                    .as_ref()
                    .expect("replacement remains registered");
                assert_eq!(task.turn_context.sub_id, replacement.sub_id);
                assert_eq!(task.kind, TaskKind::Regular);
                assert!(!replacement_cancellation.as_ref().unwrap().is_cancelled());
            } else {
                assert!(active.is_none(), "a cancelled reservation must not revive");
            }
        }
        assert_eq!(
            session.input_queue.drain_mailbox_input_items().await.0,
            vec![TurnInput::InterAgentCommunication(communication.into())],
            "stale registration must not take the replacement owner's mail"
        );
        session.abort_all_tasks(TurnAbortReason::Replaced).await;
    }
}

#[tokio::test]
async fn idle_compact_rechecks_closing_after_waiting_for_active_turn_lock() {
    let (session, _context, _events) = make_session_and_context_with_rx().await;
    let revision = session.cleanup_admission.activity_revision();
    let active = session.active_turn.lock().await;
    let reservation = session.reserve_compact_if_idle();
    tokio::pin!(reservation);
    assert!(futures::poll!(reservation.as_mut()).is_pending());
    session.cleanup_admission.close();
    drop(active);

    assert!(reservation.await.is_err());
    assert!(session.active_turn.lock().await.is_none());
    assert_eq!(session.cleanup_admission.activity_revision(), revision);
}
