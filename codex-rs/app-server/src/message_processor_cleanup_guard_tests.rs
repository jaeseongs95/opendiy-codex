// Modified for OpenDIY Codex R30 (2026-10-31); see docs/r30-final-boundary-validation-20261031.ko.md.
//! Host guards exercised through real cleanup ownership, terminal V2 runtimes and local storage.
//! Opt-in only: `cargo test -p codex-app-server --features test-support cleanup_guard_tests --lib`.
use super::message_processor_tracing_tests::{
    build_test_processor_with_state_db, read_response_from,
};
use super::*;
use crate::outgoing_message::{OutgoingEnvelope, OutgoingMessage};
use crate::transport::ConnectionOrigin;
use anyhow::{Context, Result};
use app_test_support::MockResponsesConfig;
use codex_app_server_protocol::{
    CompletedChildSnapshot, InitializeResponse, RequestId, ServerNotification,
    ServerNotificationEnvelope, ThreadCleanupCompletedParams, ThreadCleanupCompletedResponse,
    ThreadGoalSetParams, ThreadStartResponse,
};
use codex_core::config::ConfigBuilder;
use codex_core::{CodexThread, StartThreadOptions, TurnInput};
use codex_features::Feature;
use codex_goal_extension::{
    GoalObjectiveUpdate, GoalService, GoalSetRequest, GoalTokenBudgetUpdate,
};
use codex_protocol::items::TurnItem;
use codex_protocol::protocol::{AgentStatus, EventMsg, MultiAgentVersion, Op, ThreadGoalStatus};
use codex_protocol::user_input::UserInput;
use codex_thread_store::{LocalThreadStore, ReadThreadParams, ThreadStore, ThreadStoreError};
use futures::FutureExt;
use serde_json::json;
use tempfile::TempDir;
use tokio::sync::mpsc;
use wiremock::MockServer;

const DEADLINE: Duration = Duration::from_secs(30);
const CONNECTION: ConnectionId = ConnectionId(701);
const OBSERVER: ConnectionId = ConnectionId(702);

async fn send(
    processor: &Arc<MessageProcessor>,
    session: &Arc<ConnectionSessionState>,
    id: ConnectionId,
    request: serde_json::Value,
) {
    processor
        .process_request(
            id,
            serde_json::from_value(request).unwrap(),
            &AppServerTransport::WebSocket {
                bind_address: "127.0.0.1:0".parse().unwrap(),
            },
            Arc::clone(session),
        )
        .await;
}

async fn initialize(
    processor: &Arc<MessageProcessor>,
    rx: &mut mpsc::Receiver<OutgoingEnvelope>,
    id: ConnectionId,
) -> Arc<ConnectionSessionState> {
    let session = Arc::new(ConnectionSessionState::new(ConnectionOrigin::WebSocket));
    send(
        processor,
        &session,
        id,
        json!({"id":1,"method":"initialize","params":{
            "clientInfo":{"name":"cleanup-guard-test","version":"1"},
            "capabilities":{"experimentalApi":true}
        }}),
    )
    .await;
    let _: InitializeResponse = read_response_from(rx, id, 1).await;
    assert!(session.initialized());
    // Mirror lib.rs's registration after writing the initialize response.
    processor
        .connection_initialized(id, session.request_attestation())
        .await;
    session
}

fn input(text: &str) -> TurnInput {
    TurnInput::UserInput {
        content: vec![UserInput::Text {
            text: text.into(),
            text_elements: Vec::new(),
        }],
        client_id: None,
    }
}

async fn set_goal(
    service: &GoalService,
    state: &codex_state::StateRuntime,
    id: ThreadId,
    objective: &str,
    status: ThreadGoalStatus,
) -> Result<()> {
    service
        .set_thread_goal(
            state,
            GoalSetRequest {
                thread_id: id,
                objective: GoalObjectiveUpdate::Set(objective),
                status: Some(status),
                token_budget: GoalTokenBudgetUpdate::Set(Some(17)),
                max_goal_token_budget: None,
            },
            std::future::ready(Ok(())),
        )
        .await?;
    Ok(())
}

async fn make_terminal(thread: &Arc<CodexThread>, interrupted: bool) -> Result<()> {
    assert_eq!(thread.multi_agent_version(), Some(MultiAgentVersion::V2));
    let command = if interrupted {
        if cfg!(windows) {
            "ping.exe -n 61 127.0.0.1 > NUL"
        } else {
            "sleep 60"
        }
    } else {
        "echo cleanup-terminal"
    };
    let submission = thread
        .submit(Op::RunUserShellCommand {
            command: command.into(),
            timeout_ms: Some(if interrupted { 65_000 } else { 5_000 }),
        })
        .await?;
    timeout(DEADLINE, async {
        let mut command_completed = false;
        loop {
            let event = thread.next_event().await?;
            match event.msg {
                EventMsg::TurnStarted(start) if interrupted && start.turn_id == submission => {
                    thread.submit(Op::Interrupt).await?;
                }
                EventMsg::TurnAborted(end)
                    if interrupted && end.turn_id.as_deref() == Some(submission.as_str()) =>
                {
                    assert_eq!(
                        end.reason,
                        codex_protocol::protocol::TurnAbortReason::Interrupted
                    );
                    break;
                }
                EventMsg::ItemCompleted(end) => {
                    if !interrupted && let TurnItem::CommandExecution(command) = end.item {
                        assert_eq!(
                            command.exit_code,
                            Some(0),
                            "the real shell task must succeed"
                        );
                        command_completed = true;
                    }
                }
                EventMsg::TurnComplete(end) if end.turn_id == submission => {
                    assert!(!interrupted, "중단할 실제 작업이 먼저 정상 종료했습니다");
                    assert!(
                        command_completed,
                        "a real task must precede the terminal event"
                    );
                    break;
                }
                EventMsg::Error(error) => panic!("terminal fixture failed: {error:?}"),
                _ => {}
            }
        }
        Ok::<_, anyhow::Error>(())
    })
    .await??;
    thread.ensure_rollout_materialized().await;
    thread.flush_rollout().await?;
    let status = thread.agent_status().await;
    if interrupted {
        assert_eq!(status, AgentStatus::Interrupted);
    } else {
        assert!(matches!(status, AgentStatus::Completed(_)));
    }
    Ok(())
}

struct Fixture {
    _home: TempDir,
    server: MockServer,
    processor: Arc<MessageProcessor>,
    session: Arc<ConnectionSessionState>,
    _observer_session: Arc<ConnectionSessionState>,
    rx: mpsc::Receiver<OutgoingEnvelope>,
    store: Arc<dyn ThreadStore>,
    queue: Arc<QueuedItemService>,
    goals: Arc<GoalService>,
    state: codex_core::StateDbHandle,
    parent: ThreadId,
    child: ThreadId,
    sibling: ThreadId,
    snapshot: CompletedChildSnapshot,
    sibling_queue_id: String,
}

impl Fixture {
    async fn new() -> Result<Self> {
        Self::new_with_interrupted_child(false).await
    }

    async fn new_with_interrupted_child(interrupted: bool) -> Result<Self> {
        let home = TempDir::new()?;
        let server = MockServer::start().await;
        MockResponsesConfig::new(&server.uri())
            .enable_feature(Feature::Sqlite)
            .enable_feature(Feature::MultiAgentV2)
            .enable_feature(Feature::Goals)
            .disable_feature(Feature::WakeOnAgentMessages)
            .with_extra_config("[analytics]\nenabled = false")
            .write(home.path())?;
        let mut config = ConfigBuilder::default()
            .codex_home(home.path().to_path_buf())
            .build()
            .await?;
        config.cwd = codex_utils_absolute_path::AbsolutePathBuf::try_from(home.path())?;
        config.model_catalog = Some(codex_models_manager::bundled_models_response()?);
        config.sqlite = codex_state::SqliteConfig::new_for_testing(config.codex_home.clone());
        let state = codex_core::init_state_db(&config)
            .await
            .context("isolated metadata database")?;
        let config = Arc::new(config);
        let auth = AuthManager::shared_from_config(config.as_ref(), false).await?;
        let (processor, mut rx) =
            build_test_processor_with_state_db(Arc::clone(&config), auth, Some(Arc::clone(&state)))
                .await;
        let session = initialize(&processor, &mut rx, CONNECTION).await;
        let observer_session = initialize(&processor, &mut rx, OBSERVER).await;
        send(
            &processor,
            &session,
            CONNECTION,
            json!({"id":2,"method":"thread/start","params":{"model":"mock-model"}}),
        )
        .await;
        let started: ThreadStartResponse = read_response_from(&mut rx, CONNECTION, 2).await;
        let parent = ThreadId::from_string(&started.thread.id)?;
        let manager = &processor.thread_processor.thread_manager;
        let root = manager.get_thread(parent).await?;
        root.ensure_rollout_materialized().await;
        root.flush_rollout().await?;
        let child = manager
            .spawn_child_for_cleanup_test(parent, StartThreadOptions::new(config.as_ref().clone()))
            .await?;
        let sibling = manager
            .spawn_child_for_cleanup_test(parent, StartThreadOptions::new(config.as_ref().clone()))
            .await?;
        make_terminal(&child.thread, interrupted).await?;
        make_terminal(&sibling.thread, false).await?;
        // Wait for the actual task's idle/terminal boundary, not a forged status or UUID.
        timeout(DEADLINE, async {
            loop {
                let preview = manager.preview_completed_children(parent, parent).await?;
                if [child.thread_id, sibling.thread_id].iter().all(|id| {
                    preview
                        .iter()
                        .any(|item| item.thread_id == *id && item.snapshot.is_some())
                }) {
                    break Ok::<_, anyhow::Error>(());
                }
                tokio::task::yield_now().await;
            }
        })
        .await??;
        let (store, queue) = processor.thread_processor.cleanup_services_for_test();
        let goals = processor.thread_goal_processor.goal_service_for_test();
        // Seed pre-existing host data through official stores, with no dispatch or model call.
        let durable_queue = LocalQueueStore::new(Arc::clone(&state));
        let payload = serde_json::to_string(&input("preserved host work"))?;
        let sibling_queue_id = durable_queue.enqueue(sibling.thread_id, payload).await?.id;
        set_goal(
            &goals,
            &state,
            parent,
            "parent goal",
            ThreadGoalStatus::Paused,
        )
        .await?;
        set_goal(
            &goals,
            &state,
            sibling.thread_id,
            "sibling goal",
            ThreadGoalStatus::Paused,
        )
        .await?;
        send(
            &processor,
            &session,
            CONNECTION,
            json!({"id":3,"method":"thread/cleanupCompleted","params":{
                "parentThreadId":parent,"currentThreadId":parent,"confirmed":false
            }}),
        )
        .await;
        let preview: ThreadCleanupCompletedResponse =
            read_response_from(&mut rx, CONNECTION, 3).await;
        let snapshot = preview
            .results
            .iter()
            .find(|item| item.thread_id == child.thread_id.to_string())
            .and_then(|item| item.snapshot.clone())
            .context("real terminal child must be selectable")?;
        assert_eq!(
            snapshot.terminal_status,
            if interrupted {
                "cancelled"
            } else {
                "completed"
            }
        );
        assert!(!snapshot.last_turn_id.is_empty());
        Ok(Self {
            _home: home,
            server,
            processor,
            session,
            _observer_session: observer_session,
            rx,
            store,
            queue,
            goals,
            state,
            parent,
            child: child.thread_id,
            sibling: sibling.thread_id,
            snapshot,
            sibling_queue_id,
        })
    }

    async fn barrier(
        &self,
    ) -> (
        tokio::sync::oneshot::Receiver<()>,
        tokio::sync::oneshot::Sender<()>,
    ) {
        self.store
            .as_any()
            .downcast_ref::<LocalThreadStore>()
            .expect("real local store")
            .install_delete_barrier_for_test(self.child)
            .await
    }

    fn params(&self) -> ThreadCleanupCompletedParams {
        ThreadCleanupCompletedParams {
            parent_thread_id: self.parent.to_string(),
            current_thread_id: self.parent.to_string(),
            confirmed: true,
            selection: vec![self.snapshot.clone()],
        }
    }

    async fn request_cleanup(&self) {
        assert_eq!(
            self.params().selection.len(),
            1,
            "empty selection cannot satisfy this regression"
        );
        send(
            &self.processor,
            &self.session,
            CONNECTION,
            json!({"id":4,"method":"thread/cleanupCompleted","params":self.params()}),
        )
        .await;
    }

    async fn assert_preserved(&self, id: ThreadId) -> Result<()> {
        let stored = self
            .store
            .read_thread(ReadThreadParams {
                thread_id: id,
                include_archived: true,
                include_history: false,
            })
            .await?;
        assert_eq!(stored.thread_id, id);
        assert!(stored.rollout_path.context("durable rollout")?.is_file());
        assert!(
            self.processor
                .thread_processor
                .thread_manager
                .get_thread(id)
                .await
                .is_ok()
        );
        Ok(())
    }

    async fn assert_family(&self) -> Result<()> {
        for (id, objective) in [(self.parent, "parent goal"), (self.sibling, "sibling goal")] {
            self.assert_preserved(id).await?;
            let queued = self.queue.list(id).await?;
            if id == self.parent {
                assert!(queued.is_empty());
            } else {
                assert_eq!(queued.len(), 1);
                assert_eq!(queued[0].id, self.sibling_queue_id);
            }
            let goal = self
                .goals
                .get_thread_goal(&self.state, id)
                .await?
                .context("preserved goal")?;
            assert_eq!(goal.objective, objective);
            assert_eq!(goal.status, ThreadGoalStatus::Paused);
            assert_eq!(goal.token_budget, Some(17));
        }
        Ok(())
    }

    async fn finish(self) -> Result<()> {
        self.assert_family().await?;
        drop(self.rx);
        self.processor.clear_runtime_references();
        timeout(DEADLINE, self.processor.shutdown_threads()).await?;
        // No mounted model response: an accidental call, including during teardown, fails.
        assert!(
            self.server.received_requests().await.unwrap().is_empty(),
            "model HTTP requests must remain zero"
        );
        Ok(())
    }
}

#[tokio::test]
async fn cleanup_guard_pending_queue_preserves_selected_terminal_v2_child() -> Result<()> {
    // enqueue on the loaded service wakes and consumes work. Use real cancelled
    // terminal state, which both normal queue wake paths deliberately preserve,
    // and seed pre-existing durable work through the official queue store.
    let mut f = Box::pin(Fixture::new_with_interrupted_child(true)).await?;
    let item = LocalQueueStore::new(Arc::clone(&f.state))
        .enqueue(
            f.child,
            serde_json::to_string(&input("pending child input"))?,
        )
        .await?;
    let before = f.queue.list(f.child).await?;
    let goal_before = f.goals.get_thread_goal(&f.state, f.child).await?;
    assert_eq!(before.len(), 1);
    assert_eq!(before[0].id, item.id);
    assert!(goal_before.is_none());
    assert_eq!(
        f.processor
            .thread_processor
            .thread_manager
            .get_thread(f.child)
            .await?
            .agent_status()
            .await,
        AgentStatus::Interrupted
    );
    let (mut entered, release) = f.barrier().await;
    f.request_cleanup().await;
    let response: ThreadCleanupCompletedResponse =
        read_response_from(&mut f.rx, CONNECTION, 4).await;
    let after = f.queue.list(f.child).await?;
    let goal_after = f.goals.get_thread_goal(&f.state, f.child).await?;
    let model_requests = f.server.received_requests().await.unwrap().len();
    eprintln!(
        "큐 보존 관측: 응답={response:?}, 이전큐={before:?}, 이후큐={after:?}, 이전목표={goal_before:?}, 이후목표={goal_after:?}, 모델요청수={model_requests}"
    );
    assert_eq!(model_requests, 0);
    assert_eq!(response.results.len(), 1);
    assert_eq!(response.results[0].thread_id, f.child.to_string());
    assert_eq!(response.results[0].outcome, "pending_user_queue");
    assert!(
        matches!(
            entered.try_recv(),
            Err(tokio::sync::oneshot::error::TryRecvError::Empty)
        ),
        "storage deletion must not be entered"
    );
    assert_eq!(after, before);
    assert_eq!(goal_after, goal_before);
    f.assert_preserved(f.child).await?;
    drop(release);
    f.finish().await
}

#[tokio::test]
async fn cleanup_guard_unfinished_goal_preserves_selected_terminal_v2_child() -> Result<()> {
    let mut f = Box::pin(Fixture::new()).await?;
    set_goal(
        &f.goals,
        &f.state,
        f.child,
        "unfinished child goal",
        ThreadGoalStatus::Paused,
    )
    .await?;
    let before = f.goals.get_thread_goal(&f.state, f.child).await?.unwrap();
    let (mut entered, release) = f.barrier().await;
    f.request_cleanup().await;
    let response: ThreadCleanupCompletedResponse =
        read_response_from(&mut f.rx, CONNECTION, 4).await;
    assert_eq!(response.results.len(), 1);
    assert_eq!(response.results[0].thread_id, f.child.to_string());
    assert_eq!(response.results[0].outcome, "unfinished_goal");
    assert!(
        matches!(
            entered.try_recv(),
            Err(tokio::sync::oneshot::error::TryRecvError::Empty)
        ),
        "storage deletion must not be entered"
    );
    assert_eq!(
        f.goals.get_thread_goal(&f.state, f.child).await?,
        Some(before)
    );
    f.assert_preserved(f.child).await?;
    drop(release);
    f.finish().await
}

#[tokio::test]
async fn cleanup_guard_exact_selection_preserves_unknown_active_and_unselected_descendant()
-> Result<()> {
    let mut f = Box::pin(Fixture::new()).await?;
    let manager = &f.processor.thread_processor.thread_manager;
    let config = manager.get_thread(f.parent).await?.config().await;
    let unknown = manager
        .spawn_child_for_cleanup_test(f.parent, StartThreadOptions::new(config.as_ref().clone()))
        .await?;
    let descendant = manager
        .spawn_child_for_cleanup_test(f.sibling, StartThreadOptions::new(config.as_ref().clone()))
        .await?;
    let active = manager
        .spawn_child_for_cleanup_test(f.parent, StartThreadOptions::new(config.as_ref().clone()))
        .await?;
    for thread in [&unknown.thread, &descendant.thread] {
        thread.ensure_rollout_materialized().await;
        thread.flush_rollout().await?;
        assert!(!matches!(
            thread.agent_status().await,
            AgentStatus::Completed(_)
        ));
    }
    // A real cancellable shell task holds the active runtime; no model is involved.
    let command = if cfg!(windows) {
        "powershell.exe -NoLogo -NoProfile -NonInteractive -Command \"Start-Sleep -Seconds 60\""
    } else {
        "sleep 60"
    };
    active
        .thread
        .submit(Op::RunUserShellCommand {
            command: command.into(),
            timeout_ms: Some(65_000),
        })
        .await?;
    timeout(DEADLINE, async {
        loop {
            if matches!(
                active.thread.next_event().await?.msg,
                EventMsg::TurnStarted(_)
            ) {
                break;
            }
        }
        Ok::<_, anyhow::Error>(())
    })
    .await??;
    assert_eq!(active.thread.agent_status().await, AgentStatus::Running);
    active.thread.ensure_rollout_materialized().await;
    active.thread.flush_rollout().await?;
    let preview = manager
        .preview_completed_children(f.parent, f.parent)
        .await?;
    for id in [unknown.thread_id, descendant.thread_id, active.thread_id] {
        let item = preview
            .iter()
            .find(|item| item.thread_id == id)
            .context("preserved runtime must be present in preview")?;
        assert_eq!(item.outcome, "preserved");
        assert!(item.snapshot.is_none());
    }
    let (entered, release) = f.barrier().await;
    f.request_cleanup().await;
    timeout(DEADLINE, entered).await??;
    f.assert_family().await?;
    for id in [unknown.thread_id, descendant.thread_id, active.thread_id] {
        f.assert_preserved(id).await?;
    }
    release
        .send(())
        .expect("selected child reached real deletion");
    let response: ThreadCleanupCompletedResponse =
        read_response_from(&mut f.rx, CONNECTION, 4).await;
    assert_eq!(response.results.len(), 1);
    assert_eq!(response.results[0].thread_id, f.child.to_string());
    assert_eq!(response.results[0].outcome, "deleted");
    assert!(manager.get_thread(f.child).await.is_err());
    for (id, parent) in [
        (unknown.thread_id, f.parent),
        (descendant.thread_id, f.sibling),
        (active.thread_id, f.parent),
    ] {
        f.assert_preserved(id).await?;
        let stored = f
            .store
            .read_thread(ReadThreadParams {
                thread_id: id,
                include_archived: true,
                include_history: false,
            })
            .await?;
        assert_eq!(stored.parent_thread_id, Some(parent));
    }
    assert_eq!(active.thread.agent_status().await, AgentStatus::Running);
    f.finish().await
}

#[derive(Clone, Copy, Debug)]
enum Release {
    Normal,
    Disconnect,
    AbortRequest,
    Panic,
}

async fn delete_at_barrier(mode: Release) -> Result<()> {
    let mut f = Box::pin(Fixture::new()).await?;
    let (entered, release) = f.barrier().await;
    let request = if matches!(mode, Release::AbortRequest) {
        // Abort the actual handler future, not just a sender that already queued it.
        // The same handler is reached by the initialized dispatcher in other cases.
        let processor = Arc::clone(&f.processor);
        let params = f.params();
        assert_eq!(params.selection.len(), 1);
        Some(tokio::spawn(async move {
            processor
                .thread_processor
                .thread_cleanup_completed(
                    ConnectionRequestId {
                        connection_id: CONNECTION,
                        request_id: RequestId::Integer(4),
                    },
                    params,
                )
                .await
        }))
    } else {
        f.request_cleanup().await;
        None
    };
    timeout(DEADLINE, entered).await??;
    if let Some(request) = request {
        request.abort();
        assert!(
            request
                .await
                .expect_err("handler future must be cancelled")
                .is_cancelled()
        );
    }
    // This is inside real LocalThreadStore::delete_threads, after Core shutdown,
    // under host queue/goal guards and local lifecycle/writer/reference checks.
    assert!(f.queue.list(f.child).await?.is_empty());
    assert!(f.goals.get_thread_goal(&f.state, f.child).await?.is_none());
    let stored = f
        .store
        .read_thread(ReadThreadParams {
            thread_id: f.child,
            include_archived: true,
            include_history: false,
        })
        .await?;
    let child_rollout = stored.rollout_path.context("selected child's rollout")?;
    assert!(child_rollout.is_file());

    let queue = Arc::clone(&f.queue);
    let child = f.child;
    let mut enqueue =
        tokio::spawn(async move { queue.enqueue(child, input("racing input")).await });
    assert!(
        timeout(Duration::from_millis(50), &mut enqueue)
            .await
            .is_err(),
        "same-thread enqueue must wait until storage deletion ends"
    );
    // Use the actual request's outer goal fence, which also protects the interval
    // after shutdown unregisters the goal runtime. Do not bypass it with DB writes.
    let processor = Arc::clone(&f.processor);
    let mut goal = tokio::spawn(async move {
        processor
            .thread_goal_processor
            .thread_goal_set(
                ConnectionRequestId {
                    connection_id: OBSERVER,
                    request_id: RequestId::Integer(5),
                },
                ThreadGoalSetParams {
                    thread_id: child.to_string(),
                    origin: None,
                    objective: Some("racing goal".into()),
                    status: Some(codex_app_server_protocol::ThreadGoalStatus::Paused),
                    token_budget: None,
                },
            )
            .await
    });
    assert!(
        timeout(Duration::from_millis(50), &mut goal).await.is_err(),
        "same-thread goal mutation must wait until storage deletion ends"
    );
    f.assert_family().await?;
    // The guards are per target: both unrelated host services are still writable.
    let other = ThreadId::new();
    let other_item = timeout(DEADLINE, f.queue.enqueue(other, input("unrelated work"))).await??;
    timeout(
        DEADLINE,
        set_goal(
            &f.goals,
            &f.state,
            f.sibling,
            "sibling goal",
            ThreadGoalStatus::Paused,
        ),
    )
    .await??;

    let mut disconnect = if matches!(mode, Release::Disconnect) {
        let processor = Arc::clone(&f.processor);
        let session = Arc::clone(&f.session);
        let mut future =
            Box::pin(async move { processor.connection_closed(CONNECTION, &session).await });
        assert!(
            future.as_mut().now_or_never().is_none(),
            "disconnect must drain the already admitted owner"
        );
        assert!(f.session.rpc_gate.is_closed());
        Some(future)
    } else {
        None
    };
    match mode {
        Release::Normal => {
            release.send(()).expect("owner is waiting at storage");
        }
        Release::Disconnect | Release::AbortRequest => {
            drop(release);
        }
        Release::Panic => {
            let panicked = tokio::spawn(async move {
                let _release_on_unwind = release;
                panic!("intentional fixture barrier holder panic");
            })
            .await
            .expect_err("fixture holder must panic");
            assert!(panicked.is_panic());
        }
    }
    let enqueue_result = timeout(DEADLINE, &mut enqueue).await??;
    let goal_result = timeout(DEADLINE, &mut goal).await??;
    eprintln!(
        "삭제 장벽 해제 관측: 경로={mode:?}, 큐결과={enqueue_result:?}, 목표성공={}, 목표오류={:?}",
        goal_result.is_ok(),
        goal_result.as_ref().err()
    );
    drop((enqueue, goal));

    // Drain real output while waiting; bounded outgoing capacity is not the barrier.
    let mut deleted = 0;
    let response = timeout(DEADLINE, async {
        loop {
            match f
                .rx
                .recv()
                .await
                .context("outgoing closed before cleanup result")?
            {
                OutgoingEnvelope::Broadcast {
                    message:
                        OutgoingMessage::AppServerNotification(ServerNotificationEnvelope {
                            notification: ServerNotification::ThreadDeleted(event),
                            ..
                        }),
                } => {
                    assert_eq!(event.thread_id, f.child.to_string());
                    deleted += 1;
                    if matches!(mode, Release::AbortRequest) {
                        break Ok::<_, anyhow::Error>(None);
                    }
                }
                OutgoingEnvelope::ToConnection {
                    connection_id: CONNECTION,
                    message: OutgoingMessage::Response(response),
                    ..
                } if response.id == RequestId::Integer(4) => {
                    assert!(
                        !matches!(mode, Release::AbortRequest),
                        "cancelled handler must not send a response"
                    );
                    break Ok(Some(serde_json::from_value::<
                        ThreadCleanupCompletedResponse,
                    >(serde_json::to_value(
                        response.result,
                    )?)?));
                }
                OutgoingEnvelope::ToConnection {
                    message: OutgoingMessage::Error(error),
                    ..
                } if error.id == RequestId::Integer(4) => panic!("cleanup failed: {error:?}"),
                _ => {}
            }
        }
    })
    .await??;
    assert_eq!(
        deleted, 1,
        "one real deletion notification must precede the result"
    );
    if let Some(response) = response {
        assert_eq!(response.results.len(), 1);
        assert_eq!(response.results[0].thread_id, f.child.to_string());
        assert_eq!(response.results[0].outcome, "deleted");
    } else {
        assert!(matches!(mode, Release::AbortRequest));
    }
    if let Some(future) = disconnect.as_mut() {
        timeout(DEADLINE, future).await?;
    }
    // Observe every independent deletion surface before asserting an error variant.
    // LocalThreadStore's documented missing-rollout result is InvalidRequest,
    // whereas ThreadNotFound belongs to the store's delete operation contract.
    let metadata_before_read = f.state.get_thread(f.child).await?;
    let read_result = f
        .store
        .read_thread(ReadThreadParams {
            thread_id: f.child,
            include_archived: true,
            include_history: false,
        })
        .await;
    let metadata_after_read = f.state.get_thread(f.child).await?;
    let loaded = f
        .processor
        .thread_processor
        .thread_manager
        .get_thread(f.child)
        .await;
    let queued = f.queue.list(f.child).await?;
    let remaining_goal = f.goals.get_thread_goal(&f.state, f.child).await?;
    let model_requests = f.server.received_requests().await.unwrap().len();
    let current_path = codex_rollout::find_thread_path_by_id_str(
        f._home.path(),
        &f.child.to_string(),
        Some(f.state.as_ref()),
    )
    .await?;
    let archived_path = codex_rollout::find_archived_thread_path_by_id_str(
        f._home.path(),
        &f.child.to_string(),
        Some(f.state.as_ref()),
    )
    .await?;
    let plain_path = codex_rollout::plain_rollout_path(&child_rollout);
    let compressed_path = plain_path.with_extension("jsonl.zst");
    eprintln!(
        "실제 삭제 관측: 경로={mode:?}, 대상={}, 원본파일={}, 평문파일={}, 압축파일={}, 현재검색={current_path:?}, 보관검색={archived_path:?}, 읽기={read_result:?}, 읽기전행={:?}, 읽기후행={:?}, runtime존재={}, runtime오류={:?}, 잔존큐={queued:?}, 잔존목표={remaining_goal:?}, 모델요청수={model_requests}",
        f.child,
        child_rollout.exists(),
        plain_path.exists(),
        compressed_path.exists(),
        metadata_before_read.as_ref().map(|row| (
            row.id,
            &row.rollout_path,
            row.attached_thread_parent_id
        )),
        metadata_after_read.as_ref().map(|row| (
            row.id,
            &row.rollout_path,
            row.attached_thread_parent_id
        )),
        loaded.is_ok(),
        loaded.as_ref().err()
    );
    assert!(
        matches!(
            &enqueue_result,
            Err(codex_queue_extension::QueueServiceError::CoreSubmissionError(error))
                if matches!(
                    error.details(),
                    codex_protocol::error::CodexErrorDetails::InvalidRequest(message)
                        if message == "thread is reserved for cleanup"
                )
        ),
        "완료된 정리 예약이 같은 대상의 대기 생산자를 거절해야 합니다: {enqueue_result:?}"
    );
    let goal_error = goal_result
        .err()
        .context("삭제된 대상의 목표 변경은 거절되어야 합니다")?;
    assert_eq!(goal_error.code, -32600);
    assert_eq!(goal_error.message, format!("thread not found: {}", f.child));
    assert!(!child_rollout.exists());
    assert!(!plain_path.exists());
    assert!(!compressed_path.exists());
    assert!(current_path.is_none());
    assert!(archived_path.is_none());
    assert!(metadata_before_read.is_none());
    assert!(metadata_after_read.is_none());
    let error = read_result.expect_err("삭제된 대상을 저장소에서 읽을 수 없어야 합니다");
    let ThreadStoreError::InvalidRequest { message } = error else {
        panic!("기존 LocalThreadStore 부재 계약과 다른 오류입니다: {error:?}");
    };
    assert_eq!(
        message,
        format!("no rollout found for thread id {}", f.child)
    );
    assert!(loaded.is_err());
    assert!(queued.is_empty());
    assert!(remaining_goal.is_none());
    assert_eq!(model_requests, 0);
    assert_eq!(f.queue.list(other).await?[0].id, other_item.id);
    f.finish().await
}

#[tokio::test]
async fn cleanup_guard_storage_barrier_serializes_mutations_until_real_delete() -> Result<()> {
    Box::pin(delete_at_barrier(Release::Normal)).await
}

#[tokio::test]
async fn cleanup_guard_disconnect_at_storage_barrier_preserves_owner_and_family() -> Result<()> {
    Box::pin(delete_at_barrier(Release::Disconnect)).await
}

#[tokio::test]
async fn cleanup_guard_panicking_barrier_holder_releases_real_delete() -> Result<()> {
    Box::pin(delete_at_barrier(Release::Panic)).await
}

#[tokio::test]
async fn cleanup_guard_cancelled_handler_keeps_host_guards_until_real_delete() -> Result<()> {
    Box::pin(delete_at_barrier(Release::AbortRequest)).await
}
