// Modified for OpenDIY Codex R32 (2026-11-01); see MODIFICATIONS.md.
use super::tests::make_session_and_context;
use super::tests::make_session_and_context_with_rx;
use super::*;
use codex_protocol::turn_input::TurnInputRequest;
use std::time::Duration;

fn test_io(tx_sub: Sender<Submission>, termination: SessionLoopTermination) -> SessionIo {
    let (_tx_event, rx_event) = async_channel::unbounded();
    SessionIo {
        tx_sub,
        admission: Arc::default(),
        rx_event,
        agent_status: watch::channel(AgentStatus::PendingInit).1,
        session_loop_termination: termination,
    }
}

#[tokio::test]
async fn turn_input_distinguishes_failed_delivery_from_lost_reply_without_replay() {
    let (tx_sub, rx_sub) = async_channel::bounded(1);
    drop(rx_sub);
    let io = test_io(tx_sub, completed_session_loop_termination());
    let error = io
        .submit_turn_input(
            TurnInputRequest::user_input(Vec::new()),
            TurnInputMode::StartOrSteer,
        )
        .await
        .expect_err("closed receiver rejects input before acceptance");
    assert!(
        matches!(error.details(), CodexErrorDetails::Fatal(message) if message.starts_with("turn input was not accepted:"))
    );

    let (tx_sub, rx_sub) = async_channel::bounded(1);
    let io = test_io(tx_sub, completed_session_loop_termination());
    let mut processed = 0;
    let (result, ()) = tokio::join!(
        io.submit_turn_input(
            TurnInputRequest::user_input(Vec::new()),
            TurnInputMode::StartOrSteer
        ),
        async {
            let submission = rx_sub.recv().await.expect("accepted input");
            let Op::TurnInput { reply, .. } = submission.op else {
                panic!("expected turn input");
            };
            processed += 1;
            // Processing can precede a panic that destroys the routing response.
            drop(reply);
        }
    );
    let error = result.expect_err("missing reply does not prove rejection");
    assert!(
        matches!(error.details(), CodexErrorDetails::Fatal(message) if message.starts_with("turn input outcome is unknown:"))
    );
    assert_eq!(processed, 1);
    assert!(
        rx_sub.is_empty(),
        "no automatic replay after an unknown outcome"
    );
}

#[tokio::test]
async fn eager_loop_supervisor_cleans_active_task_after_panic_without_a_waiter() {
    let (session, turn_context, events) = make_session_and_context_with_rx().await;
    session
        .spawn_task(
            turn_context,
            Vec::new(),
            super::tests::NeverEndingTask {
                kind: crate::state::TaskKind::Regular,
                listen_to_cancellation_token: true,
            },
        )
        .await;
    let handle = tokio::spawn(async { panic!("synthetic submission-loop failure") });
    let termination = supervise_session_loop(
        Arc::clone(&session),
        handle,
        AuthStorageOriginator::from_client_name("test"),
        None,
    );
    // Do not poll termination until the shutdown event proves eager cleanup ran.
    tokio::time::timeout(Duration::from_secs(5), async {
        while !matches!(
            events.recv().await.expect("shutdown event").msg,
            EventMsg::ShutdownComplete
        ) {}
    })
    .await
    .expect("cleanup runs without a shutdown waiter");
    let failure = termination
        .await
        .expect_err("panic remains a failure after cleanup");
    assert!(
        matches!(failure.as_ref(), SessionLoopFailure::Loop { source, cleanup_confirmed: true, cleanup_error: None } if source.is_panic())
    );
    assert!(
        session.active_turn.lock().await.is_none(),
        "old turn task is stopped"
    );
    assert!(session.cleanup_admission.shutdown_confirmed());
}

#[tokio::test]
async fn loop_supervisor_distinguishes_cancellation_and_preserves_cleanup_panic() {
    let (session, _) = make_session_and_context().await;
    let session = Arc::new(session);
    let handle = tokio::spawn(std::future::pending::<()>());
    handle.abort();
    let failure = supervise_session_loop(
        session,
        handle,
        AuthStorageOriginator::from_client_name("test"),
        None,
    )
    .await
    .expect_err("cancelled loop must remain distinguishable from normal shutdown");
    assert!(
        matches!(failure.as_ref(), SessionLoopFailure::Loop { source, cleanup_confirmed: true, .. } if source.is_cancelled())
    );

    struct PanicOnThreadStop;
    impl codex_extension_api::ThreadLifecycleContributor<crate::config::Config> for PanicOnThreadStop {
        fn on_thread_stop<'a>(
            &'a self,
            _input: codex_extension_api::ThreadStopInput<'a>,
        ) -> codex_extension_api::ExtensionFuture<'a, ()> {
            Box::pin(async { panic!("synthetic cleanup failure") })
        }
    }
    let (mut session, _) = make_session_and_context().await;
    let mut builder = codex_extension_api::ExtensionRegistryBuilder::<crate::config::Config>::new();
    builder.thread_lifecycle_contributor(Arc::new(PanicOnThreadStop));
    session.services.extensions = Arc::new(builder.build());
    let session = Arc::new(session);
    let failure = supervise_session_loop(
        Arc::clone(&session),
        tokio::spawn(async { panic!("synthetic submission-loop failure") }),
        AuthStorageOriginator::from_client_name("test"),
        None,
    )
    .await
    .expect_err("cleanup panic cannot become successful shutdown");
    assert!(
        matches!(failure.as_ref(), SessionLoopFailure::Loop { source, cleanup_confirmed: false, cleanup_error: Some(cleanup_error) } if source.is_panic() && cleanup_error.is_panic())
    );
    assert!(!session.cleanup_admission.shutdown_confirmed());
}

#[tokio::test]
async fn shutdown_waiter_rejects_abnormal_loop_termination() {
    let handle = tokio::spawn(async { panic!("synthetic loop failure") });
    let (tx_sub, rx_sub) = async_channel::bounded(1);
    drop(rx_sub);
    let io = test_io(tx_sub, session_loop_termination_from_handle(handle));
    let error = io
        .shutdown_and_wait()
        .await
        .expect_err("loop join error is not cleanup success");
    assert!(
        error
            .to_string()
            .contains("session loop panicked; cleanup unconfirmed")
    );
}

#[tokio::test]
async fn actual_submission_loop_panic_closes_writer_and_does_not_replay_input() {
    struct PanicOnTurnStart(Arc<std::sync::atomic::AtomicUsize>);
    impl codex_extension_api::TurnLifecycleContributor for PanicOnTurnStart {
        fn on_turn_start<'a>(
            &'a self,
            _input: codex_extension_api::TurnStartInput<'a>,
        ) -> codex_extension_api::ExtensionFuture<'a, ()> {
            Box::pin(async move {
                self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                panic!("synthetic turn-start failure");
            })
        }
    }
    let (mut session, context) = make_session_and_context().await;
    let store = Arc::new(codex_thread_store::InMemoryThreadStore::default());
    let thread_store: Arc<dyn ThreadStore> = store.clone();
    let config = session.get_config().await;
    let live_thread = LiveThread::create(
        Arc::clone(&thread_store),
        CreateThreadParams {
            attached_thread: false,
            creator_user_id: None,
            creator_account_id: None,
            session_id: session.session_id(),
            thread_id: session.thread_id,
            extra_config: None,
            forked_from_id: None,
            parent_thread_id: None,
            source: SessionSource::Exec,
            thread_source: None,
            originator: "test_originator".to_string(),
            base_instructions: codex_protocol::models::BaseInstructions::default(),
            dynamic_tools: Vec::new(),
            selected_capability_roots: Vec::new(),
            multi_agent_version: None,
            history_mode: Default::default(),
            subagent_history_start_ordinal: None,
            history_base: None,
            initial_window_id: uuid::Uuid::now_v7().to_string(),
            runtime_workspace_roots: None,
            metadata: ThreadPersistenceMetadata {
                cwd: Some(config.cwd.to_path_buf()),
                model_provider: config.model_provider_id.clone(),
                memory_mode: ThreadMemoryMode::Disabled,
            },
        },
    )
    .await
    .expect("create synthetic persistence writer");
    session.services.thread_store = thread_store;
    session.services.live_thread = Some(live_thread);
    let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let mut builder = codex_extension_api::ExtensionRegistryBuilder::<crate::config::Config>::new();
    builder.turn_lifecycle_contributor(Arc::new(PanicOnTurnStart(Arc::clone(&calls))));
    session.services.extensions = Arc::new(builder.build());
    let session = Arc::new(session);
    let (tx_sub, rx_sub) = async_channel::bounded(1);
    let handle = tokio::spawn(submission_loop(
        Arc::clone(&session),
        context.config,
        rx_sub,
    ));
    let mut io = test_io(
        tx_sub,
        supervise_session_loop(
            Arc::clone(&session),
            handle,
            AuthStorageOriginator::from_client_name("test"),
            None,
        ),
    );
    io.admission = Arc::clone(&session.cleanup_admission);
    let error = io
        .submit_turn_input(
            TurnInputRequest::user_input(Vec::new()),
            TurnInputMode::StartOrSteer,
        )
        .await
        .expect_err("turn-start panic loses its routing reply");
    assert!(error.to_string().contains("turn input outcome is unknown"));
    io.shutdown_and_wait()
        .await
        .expect("confirmed cleanup permits unloading the failed loop");
    let failure = io
        .session_loop_termination
        .clone()
        .await
        .expect_err("original panic is retained separately from cleanup success");
    assert!(
        matches!(failure.as_ref(), SessionLoopFailure::Loop { source, cleanup_confirmed: true, .. } if source.is_panic())
    );
    assert!(session.active_turn.lock().await.is_none());
    assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 1);
    assert_eq!(
        store.calls().await.shutdown_thread,
        1,
        "writer closes exactly once"
    );
}
