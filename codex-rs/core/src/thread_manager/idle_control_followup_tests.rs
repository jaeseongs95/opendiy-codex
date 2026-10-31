//! Real loaded-runtime and parent/child admission coverage without model calls.

use super::*;
use crate::error::CodexErr;
use crate::state::TaskKind;
use crate::tasks::SessionTask;
use crate::tasks::SessionTaskResult;
use tokio::sync::Notify;
use tokio_util::sync::CancellationToken;

struct HeldTask {
    kind: TaskKind,
    finish: Arc<Notify>,
}

impl SessionTask for HeldTask {
    fn kind(&self) -> TaskKind {
        self.kind
    }

    fn span_name(&self) -> &'static str {
        "session_task.idle_control_runtime_test"
    }

    async fn run(
        self: Arc<Self>,
        _session: Arc<crate::session::Session>,
        _context: Arc<crate::session::turn_context::TurnContext>,
        _input: Vec<crate::session::TurnInput>,
        cancellation: CancellationToken,
    ) -> SessionTaskResult {
        tokio::select! {
            _ = cancellation.cancelled() => {},
            _ = self.finish.notified() => {},
        }
        Ok(None)
    }
}

#[tokio::test]
async fn idle_compact_real_parent_admission_preserves_running_child_and_relationship() {
    let (_home, manager, config, root) = fixture().await;
    let worker = child(&manager, &config, &root).await;
    let child_source = worker.thread.session_source.clone();
    assert!(matches!(
        &child_source,
        SessionSource::SubAgent(SubAgentSource::ThreadSpawn { parent_thread_id, .. })
            if *parent_thread_id == root.thread_id
    ));
    let child_context = worker.thread.session.new_default_turn().await;
    worker
        .thread
        .session
        .spawn_task(
            child_context,
            Vec::new(),
            HeldTask {
                kind: TaskKind::Regular,
                finish: Arc::new(Notify::new()),
            },
        )
        .await;
    let child_cancellation = worker
        .thread
        .session
        .active_turn
        .lock()
        .await
        .as_ref()
        .expect("actual child has an active turn")
        .task
        .as_ref()
        .expect("actual child has a running task")
        .cancellation_token
        .clone();
    assert!(root.thread.session.active_turn.lock().await.is_none());
    let registry = root
        .thread
        .session
        .services
        .local_agent_runtime
        .lock_loaded_session(&root.thread.session)
        .await
        .expect("actual loaded root runtime");
    let reservation = root
        .thread
        .session
        .reserve_compact_if_idle()
        .await
        .expect("root admission is open")
        .expect("running child does not make its idle parent busy");
    drop(registry);
    let compact_context = root.thread.session.new_default_turn().await;
    assert!(
        root.thread
            .session
            .start_task_with_reservation(
                compact_context,
                Vec::new(),
                HeldTask {
                    kind: TaskKind::Compact,
                    finish: Arc::new(Notify::new()),
                },
                Some(reservation),
            )
            .await
    );
    assert!(matches!(
        root.thread.compact_if_idle(None).await.unwrap(),
        codex_protocol::turn_input::StartIfIdleSubmission::NotSubmitted {
            reason: codex_protocol::turn_input::NotSubmittedReason::NotIdle,
        }
    ));
    assert!(!child_cancellation.is_cancelled());
    assert!(
        root.thread
            .session
            .services
            .local_agent_runtime
            .shares_registry(&worker.thread.session.services.local_agent_runtime)
    );
    assert!(Arc::ptr_eq(
        &manager.get_thread(worker.thread_id).await.unwrap(),
        &worker.thread
    ));
    assert_eq!(worker.thread.session_source, child_source);
    assert!(
        worker
            .thread
            .session
            .active_turn
            .lock()
            .await
            .as_ref()
            .is_some_and(|turn| turn.task.is_some())
    );
    root.thread
        .session
        .abort_all_tasks(codex_protocol::protocol::TurnAbortReason::Replaced)
        .await;
    worker
        .thread
        .session
        .abort_all_tasks(codex_protocol::protocol::TurnAbortReason::Replaced)
        .await;
    worker.thread.shutdown_and_wait().await.unwrap();
    root.thread.shutdown_and_wait().await.unwrap();
}

#[tokio::test]
async fn idle_compact_runtime_guard_blocks_raw_remove_and_retained_core_queue_rejects() {
    let (_home, manager, _config, root) = fixture().await;
    let revision = root.thread.io.admission.activity_revision();
    let registry = root
        .thread
        .session
        .services
        .local_agent_runtime
        .lock_loaded_session(&root.thread.session)
        .await
        .expect("loaded runtime guard");
    let removal = manager.remove_thread(&root.thread_id);
    tokio::pin!(removal);
    assert!(futures::poll!(removal.as_mut()).is_pending());
    drop(registry);
    let retained = removal.await.expect("removed runtime remains retained");
    assert!(Arc::ptr_eq(&retained, &root.thread));
    assert!(
        retained.is_running(),
        "raw removal must not close the Core queue"
    );

    let outcome = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        retained.compact_if_idle(None),
    )
    .await
    .expect("actual Core queue must answer the retained runtime");
    assert!(matches!(
        outcome,
        Err(CodexErr::InvalidRequest(message)) if message == "compaction requires a loaded runtime"
    ));
    assert!(retained.session.active_turn.lock().await.is_none());
    assert_eq!(retained.io.admission.activity_revision(), revision);
    retained.shutdown_and_wait().await.unwrap();
}

#[tokio::test]
async fn idle_compact_loaded_identity_rejects_old_session_after_real_same_id_reopen() {
    let (_home, manager, config, root) = fixture().await;
    let old = child(&manager, &config, &root).await;
    complete(&old.thread).await;
    let path = old.thread.rollout_path().unwrap();
    old.thread.shutdown_and_wait().await.unwrap();
    manager
        .remove_thread_if_matches(&old.thread_id, &old.thread)
        .await
        .expect("old runtime removed");
    let context = manager
        .state
        .thread_store
        .load_latest_model_context(store::LoadThreadHistoryParams {
            thread_id: old.thread_id,
            include_archived: true,
        })
        .await
        .unwrap();
    let reopened = manager
        .state
        .spawn_child_thread(
            StartThreadOptions {
                initial_history: InitialHistory::Resumed(codex_history::ResumedHistory {
                    history_revision: context.revision,
                    conversation_id: context.thread_id,
                    history: Arc::new(context.items),
                    rollout_path: Some(path),
                }),
                session_source: Some(old.thread.session_source.clone()),
                ..StartThreadOptions::new(config)
            },
            root.thread
                .session
                .services
                .local_agent_runtime
                .control(root.thread.session.session_id()),
            Some(root.thread_id),
            None,
        )
        .await
        .unwrap();
    assert_eq!(reopened.thread_id, old.thread_id);
    assert!(!Arc::ptr_eq(&reopened.thread, &old.thread));
    assert!(Arc::ptr_eq(
        &manager.get_thread(old.thread_id).await.unwrap(),
        &reopened.thread
    ));

    // Call the runtime identity check directly: a closed old submission queue
    // cannot stand in for evidence that the same-ID replacement was rejected.
    assert!(matches!(
        old.thread
            .session
            .services
            .local_agent_runtime
            .lock_loaded_session(&old.thread.session)
            .await,
        Err(CodexErr::InvalidRequest(message))
            if message == "loaded compaction runtime changed or is closing"
    ));
    drop(
        reopened
            .thread
            .session
            .services
            .local_agent_runtime
            .lock_loaded_session(&reopened.thread.session)
            .await
            .expect("same-ID replacement itself remains admissible"),
    );
    assert!(reopened.thread.session.active_turn.lock().await.is_none());
    reopened.thread.shutdown_and_wait().await.unwrap();
    root.thread.shutdown_and_wait().await.unwrap();
}

#[tokio::test]
async fn idle_compact_registry_removal_while_waiting_rejects_before_reservation() {
    let (_home, manager, _config, root) = fixture().await;
    let revision = root.thread.io.admission.activity_revision();
    let mut registry = manager.state.threads.write().await;
    let compact = root.thread.session.compact_if_idle("stale-compact".into());
    tokio::pin!(compact);
    assert!(futures::poll!(compact.as_mut()).is_pending());
    let removed = registry.remove(&root.thread_id).expect("loaded root");
    drop(registry);

    assert!(matches!(
        compact.await,
        Err(CodexErr::InvalidRequest(message))
            if message == "compaction requires a loaded runtime"
    ));
    assert!(removed.session.active_turn.lock().await.is_none());
    assert_eq!(removed.io.admission.activity_revision(), revision);
    removed.shutdown_and_wait().await.unwrap();
}

#[tokio::test]
async fn expected_interrupt_actual_core_queue_keeps_replacement_task_and_mail() {
    use codex_protocol::protocol::Op;
    use codex_protocol::protocol::TurnAbortReason;

    let (_home, _manager, _config, root) = fixture().await;
    let first = root.thread.session.new_default_turn().await;
    root.thread
        .session
        .spawn_task(
            Arc::clone(&first),
            Vec::new(),
            HeldTask {
                kind: TaskKind::Regular,
                finish: Arc::new(Notify::new()),
            },
        )
        .await;
    root.thread
        .session
        .abort_turn_if_active(&first.sub_id, TurnAbortReason::Replaced, None)
        .await;
    let second = root.thread.session.new_default_turn().await;
    root.thread
        .session
        .spawn_task(
            Arc::clone(&second),
            Vec::new(),
            HeldTask {
                kind: TaskKind::Regular,
                finish: Arc::new(Notify::new()),
            },
        )
        .await;
    let cancellation = root
        .thread
        .session
        .active_turn
        .lock()
        .await
        .as_ref()
        .unwrap()
        .task
        .as_ref()
        .unwrap()
        .cancellation_token
        .clone();
    let communication = codex_protocol::protocol::InterAgentCommunication::new(
        codex_protocol::AgentPath::root(),
        codex_protocol::AgentPath::root(),
        Vec::new(),
        "replacement mail".into(),
        false,
    );
    root.thread
        .session
        .input_queue
        .enqueue_mailbox_communication(communication.into(), Default::default())
        .await;
    let (reply, decision) = tokio::sync::oneshot::channel();
    root.thread
        .submit(Op::InterruptTurn {
            turn_id: first.sub_id.clone(),
            reply,
        })
        .await
        .unwrap();
    assert!(!decision.await.expect("Core must reject the stale turn ID"));
    assert!(!cancellation.is_cancelled());
    assert!(
        root.thread
            .session
            .input_queue
            .has_pending_mailbox_items()
            .await
    );
    assert_eq!(
        root.thread
            .session
            .active_turn
            .lock()
            .await
            .as_ref()
            .unwrap()
            .task
            .as_ref()
            .unwrap()
            .turn_context
            .sub_id,
        second.sub_id
    );
    root.thread
        .session
        .abort_all_tasks(TurnAbortReason::Replaced)
        .await;
    root.thread.shutdown_and_wait().await.unwrap();
}
