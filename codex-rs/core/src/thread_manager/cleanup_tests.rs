// Modified for OpenDIY Codex R30 (2026-10-30); see MODIFICATIONS.md.
use super::*;
use codex_agent_graph_store::AgentGraphStore as _;
use codex_thread_store as store;

/// One-shot barriers retain their permits, so cancellation cannot lose a wakeup.
#[derive(Default)]
struct StoreBarrier {
    entered: tokio::sync::Notify,
    release: tokio::sync::Notify,
    finished: tokio::sync::Notify,
}

impl StoreBarrier {
    async fn block(&self) {
        self.entered.notify_one();
        self.release.notified().await;
    }
}

#[derive(Default)]
struct StoreFaults {
    shutdown: StdMutex<Option<(ThreadId, Option<Arc<StoreBarrier>>)>>,
    create: StdMutex<Option<Arc<StoreBarrier>>>,
    discard: StdMutex<Option<Arc<StoreBarrier>>>,
    delete: StdMutex<Option<(ThreadId, Arc<StoreBarrier>)>>,
    fail_delete: StdMutex<HashSet<ThreadId>>,
    lose_delete_response: StdMutex<HashSet<ThreadId>>,
    deletes: StdMutex<Vec<Vec<ThreadId>>>,
}

/// Forward every real operation to the isolated LocalThreadStore. Faults live at the
/// persistence API used by LiveThread and cleanup, never in cleanup's verdict flags.
struct FaultStore {
    inner: Arc<dyn store::ThreadStore>,
    faults: Arc<StoreFaults>,
}

struct BarrierGraph {
    inner: codex_agent_graph_store::LocalAgentGraphStore,
    open: StoreBarrier,
    close: StoreBarrier,
    child: StdMutex<Option<ThreadId>>,
}

impl codex_agent_graph_store::AgentGraphStore for BarrierGraph {
    fn upsert_thread_spawn_edge(
        &self,
        parent: ThreadId,
        child: ThreadId,
        status: codex_agent_graph_store::ThreadSpawnEdgeStatus,
    ) -> codex_agent_graph_store::AgentGraphStoreFuture<'_, ()> {
        Box::pin(async move {
            *self.child.lock().unwrap() = Some(child);
            self.open.block().await;
            self.inner
                .upsert_thread_spawn_edge(parent, child, status)
                .await
        })
    }
    fn set_thread_spawn_edge_status(
        &self,
        child: ThreadId,
        status: codex_agent_graph_store::ThreadSpawnEdgeStatus,
    ) -> codex_agent_graph_store::AgentGraphStoreFuture<'_, ()> {
        Box::pin(async move {
            self.close.block().await;
            let result = self.inner.set_thread_spawn_edge_status(child, status).await;
            self.close.finished.notify_one();
            result
        })
    }
    fn list_thread_spawn_children(
        &self,
        parent: ThreadId,
        status: Option<codex_agent_graph_store::ThreadSpawnEdgeStatus>,
    ) -> codex_agent_graph_store::AgentGraphStoreFuture<'_, Vec<ThreadId>> {
        self.inner.list_thread_spawn_children(parent, status)
    }
    fn list_thread_spawn_descendants(
        &self,
        root: ThreadId,
        status: Option<codex_agent_graph_store::ThreadSpawnEdgeStatus>,
    ) -> codex_agent_graph_store::AgentGraphStoreFuture<'_, Vec<ThreadId>> {
        self.inner.list_thread_spawn_descendants(root, status)
    }
}

macro_rules! forward {
    ($name:ident($($arg:ident: $ty:ty),* $(,)?) -> $result:ty) => {
        fn $name(&self, $($arg: $ty),*) -> store::ThreadStoreFuture<'_, $result> {
            self.inner.$name($($arg),*)
        }
    };
}

impl store::ThreadStore for FaultStore {
    // Preserve the wrapped local store's implementation identity, just as all of its
    // data and writer ownership remain real. Cleanup's local-only guard stays enabled.
    fn as_any(&self) -> &dyn std::any::Any {
        self.inner.as_any()
    }
    fn default_history_mode(&self) -> codex_protocol::protocol::ThreadHistoryMode {
        self.inner.default_history_mode()
    }
    fn supports_paginated_history_lists(&self) -> bool {
        self.inner.supports_paginated_history_lists()
    }
    forward!(
        resume_thread(params: store::ResumeThreadParams) -> Arc<Vec<codex_rollout::RolloutItem>>
    );
    forward!(append_items(params: store::AppendThreadItemsParams) -> ());
    forward!(persist_thread(id: ThreadId, context: store::PersistContext) -> ());
    forward!(flush_thread(id: ThreadId) -> ());
    forward!(load_history(params: store::LoadThreadHistoryParams) -> store::StoredThreadHistory);
    forward!(read_thread(params: store::ReadThreadParams) -> store::StoredThread);
    forward!(
        read_thread_by_rollout_path(
            params: store::ReadThreadByRolloutPathParams,
        ) -> store::StoredThread
    );
    forward!(list_threads(params: store::ListThreadsParams) -> store::ThreadPage);
    forward!(list_turns(params: store::ListTurnsParams) -> store::TurnPage);
    forward!(
        update_thread_metadata(
            params: store::UpdateThreadMetadataParams,
        ) -> Option<store::StoredThread>
    );
    forward!(record_thread_metadata(params: store::UpdateThreadMetadataParams) -> ());
    forward!(archive_thread(params: store::ArchiveThreadParams) -> ());
    forward!(unarchive_thread(params: store::ArchiveThreadParams) -> store::StoredThread);
    forward!(delete_thread(params: store::DeleteThreadParams) -> ());

    fn create_thread(&self, params: store::CreateThreadParams) -> store::ThreadStoreFuture<'_, ()> {
        Box::pin(async move {
            self.inner.create_thread(params).await?;
            let barrier = self.faults.create.lock().unwrap().take();
            if let Some(barrier) = barrier {
                barrier.block().await;
            }
            Ok(())
        })
    }

    fn discard_thread(&self, id: ThreadId) -> store::ThreadStoreFuture<'_, ()> {
        Box::pin(async move {
            let barrier = self.faults.discard.lock().unwrap().take();
            if let Some(barrier) = barrier {
                barrier.block().await;
                let result = self.inner.discard_thread(id).await;
                barrier.finished.notify_one();
                return result;
            }
            self.inner.discard_thread(id).await
        })
    }

    fn shutdown_thread(&self, id: ThreadId) -> store::ThreadStoreFuture<'_, ()> {
        Box::pin(async move {
            let fault = {
                let mut fault = self.faults.shutdown.lock().unwrap();
                if fault.as_ref().is_some_and(|(target, _)| *target == id) {
                    fault.take()
                } else {
                    None
                }
            };
            if let Some((_, barrier)) = fault {
                if let Some(barrier) = barrier {
                    barrier.block().await;
                    let result = self.inner.shutdown_thread(id).await;
                    barrier.finished.notify_one();
                    return result;
                }
                return Err(store::ThreadStoreError::Internal {
                    message: "injected writer shutdown failure".into(),
                });
            }
            self.inner.shutdown_thread(id).await
        })
    }

    fn delete_threads(
        &self,
        params: store::DeleteThreadsParams,
    ) -> store::ThreadStoreFuture<'_, ()> {
        Box::pin(async move {
            self.faults
                .deletes
                .lock()
                .unwrap()
                .push(params.thread_ids.clone());
            let barrier = {
                let mut slot = self.faults.delete.lock().unwrap();
                if slot
                    .as_ref()
                    .is_some_and(|(id, _)| params.thread_ids == [*id])
                {
                    slot.take().map(|(_, barrier)| barrier)
                } else {
                    None
                }
            };
            if let Some(barrier) = &barrier {
                barrier.block().await;
            }
            if params
                .thread_ids
                .iter()
                .any(|id| self.faults.fail_delete.lock().unwrap().contains(id))
            {
                return Err(store::ThreadStoreError::Retryable {
                    message: "injected deletion failure before commit".into(),
                });
            }
            let lose_response = params.thread_ids.iter().any(|id| {
                self.faults
                    .lose_delete_response
                    .lock()
                    .unwrap()
                    .contains(id)
            });
            let result = self.inner.delete_threads(params).await;
            if let Some(barrier) = barrier {
                barrier.finished.notify_one();
            }
            result?;
            if lose_response {
                return Err(store::ThreadStoreError::OutcomeUnknown {
                    message: "injected response loss after actual local deletion".into(),
                });
            }
            Ok(())
        })
    }
}

async fn fixture() -> (
    tempfile::TempDir,
    ThreadManager,
    crate::config::Config,
    super::super::NewThread,
) {
    fixture_with_faults().await.0
}

async fn fixture_with_faults() -> (
    (
        tempfile::TempDir,
        ThreadManager,
        crate::config::Config,
        super::super::NewThread,
    ),
    Arc<StoreFaults>,
) {
    fixture_with_graph(None).await
}

async fn fixture_with_graph(
    graph: Option<Arc<dyn codex_agent_graph_store::AgentGraphStore>>,
) -> (
    (
        tempfile::TempDir,
        ThreadManager,
        crate::config::Config,
        super::super::NewThread,
    ),
    Arc<StoreFaults>,
) {
    let home = tempfile::tempdir().unwrap();
    let mut config = crate::config::test_config().await;
    config.codex_home = home.path().to_path_buf().try_into().unwrap();
    config.cwd = home.path().to_path_buf().try_into().unwrap();
    let _ = config
        .features
        .enable(codex_features::Feature::MultiAgentV2);
    let _ = config
        .features
        .disable(codex_features::Feature::WakeOnAgentMessages);
    config.model_catalog = Some(codex_models_manager::bundled_models_response().unwrap());
    let mut manager = ThreadManager::with_models_provider_and_home_for_tests(
        codex_login::CodexAuth::from_api_key("dummy"),
        config.model_provider.clone(),
        config.codex_home.to_path_buf(),
        Arc::new(codex_exec_server::EnvironmentManager::default_for_tests()),
    );
    let faults = Arc::new(StoreFaults::default());
    let state = Arc::get_mut(&mut manager.state).unwrap();
    state.thread_store = Arc::new(FaultStore {
        inner: Arc::clone(&state.thread_store),
        faults: Arc::clone(&faults),
    });
    state.agent_graph_store = graph;
    let root = manager
        .start_thread(StartThreadOptions::new(config.clone()))
        .await
        .unwrap();
    root.thread.ensure_rollout_materialized().await;
    ((home, manager, config, root), faults)
}

async fn child(
    manager: &ThreadManager,
    config: &crate::config::Config,
    parent: &super::super::NewThread,
) -> super::super::NewThread {
    manager
        .state
        .spawn_child_thread(
            StartThreadOptions {
                session_source: Some(SessionSource::SubAgent(SubAgentSource::ThreadSpawn {
                    parent_thread_id: parent.thread_id,
                    depth: 1,
                    agent_path: None,
                    agent_nickname: None,
                    agent_role: None,
                })),
                ..StartThreadOptions::new(config.clone())
            },
            parent
                .thread
                .session
                .services
                .local_agent_runtime
                .control(parent.thread.session.session_id()),
            Some(parent.thread_id),
            None,
        )
        .await
        .unwrap()
}

async fn complete(thread: &Arc<CodexThread>) {
    use codex_protocol::protocol::EventMsg;
    use codex_protocol::protocol::TurnCompleteEvent;
    use codex_protocol::protocol::TurnStartedEvent;
    let turn = thread.session.new_default_turn().await;
    thread.session.record_started_turn(&turn.sub_id, None).await;
    thread
        .session
        .send_event(
            &turn,
            EventMsg::TurnStarted(TurnStartedEvent {
                turn_id: turn.sub_id.clone(),
                root_turn_id: None,
                model_context_window: None,
                turn_attribution: None,
                trace_id: None,
                started_at: None,
                collaboration_mode_kind: Default::default(),
            }),
        )
        .await;
    thread
        .session
        .send_event(
            &turn,
            EventMsg::TurnComplete(TurnCompleteEvent {
                root_turn_id: None,
                turn_id: turn.sub_id.clone(),
                started_at: None,
                last_agent_message: Some("done".into()),
                error: None,
                completed_at: None,
                duration_ms: None,
                time_to_first_token_ms: None,
            }),
        )
        .await;
    *thread.session.active_turn.lock().await = None;
    thread.flush_rollout().await.unwrap();
}

async fn stored(manager: &ThreadManager, id: ThreadId) -> bool {
    manager
        .state
        .thread_store
        .read_thread(ReadThreadParams {
            thread_id: id,
            include_archived: true,
            include_history: false,
        })
        .await
        .is_ok()
}

#[tokio::test]
async fn cleanup_deletes_exact_terminal_leaf_and_preserves_root_and_unknown_sibling() {
    let (_home, manager, config, root) = fixture().await;
    let done = child(&manager, &config, &root).await;
    let unknown = child(&manager, &config, &root).await;
    complete(&done.thread).await;
    let preview = manager
        .preview_completed_children(root.thread_id, root.thread_id)
        .await
        .unwrap();
    let selected = preview
        .into_iter()
        .filter_map(|entry| entry.snapshot)
        .collect::<Vec<_>>();
    assert_eq!(selected.len(), 1);
    assert_eq!(selected[0].thread_id, done.thread_id);
    let result = manager
        .cleanup_completed_children(root.thread_id, root.thread_id, selected)
        .await
        .unwrap();
    assert_eq!(result[0].outcome, "deleted");
    assert!(!stored(&manager, done.thread_id).await);
    assert!(stored(&manager, root.thread_id).await);
    assert!(manager.get_thread(unknown.thread_id).await.is_ok());
    assert!(done.thread.submit(Op::Interrupt).await.is_err());
    assert!(
        manager
            .state
            .cleanup_admission
            .admit([done.thread_id])
            .is_err()
    );
}

#[tokio::test]
async fn cleanup_preserves_unselected_descendant_and_rejects_stale_arc_activity() {
    let (_home, manager, config, root) = fixture().await;
    let parent = child(&manager, &config, &root).await;
    let leaf = child(&manager, &config, &parent).await;
    complete(&parent.thread).await;
    complete(&leaf.thread).await;
    let preview = manager
        .preview_completed_children(root.thread_id, root.thread_id)
        .await
        .unwrap();
    let parent_snapshot = preview
        .iter()
        .find(|entry| entry.thread_id == parent.thread_id)
        .unwrap()
        .snapshot
        .clone()
        .unwrap();
    let leaf_snapshot = preview
        .iter()
        .find(|entry| entry.thread_id == leaf.thread_id)
        .unwrap()
        .snapshot
        .clone()
        .unwrap();
    let result = manager
        .cleanup_completed_children(root.thread_id, root.thread_id, vec![parent_snapshot])
        .await
        .unwrap();
    assert_eq!(result[0].outcome, "has_descendants");
    assert!(stored(&manager, parent.thread_id).await);
    assert!(stored(&manager, leaf.thread_id).await);
    // This handle was obtained before preview; it still passes through common admission.
    leaf.thread.submit(Op::Interrupt).await.unwrap();
    let dispatched = Arc::clone(&leaf.thread.residency_gate).write_owned().await;
    drop(dispatched);
    let result = manager
        .cleanup_completed_children(root.thread_id, root.thread_id, vec![leaf_snapshot])
        .await
        .unwrap();
    assert_eq!(result[0].outcome, "changed");
    assert!(stored(&manager, leaf.thread_id).await);
}

#[tokio::test]
async fn cleanup_never_deletes_after_real_writer_shutdown_failure() {
    let ((_home, manager, config, root), faults) = fixture_with_faults().await;
    let done = child(&manager, &config, &root).await;
    complete(&done.thread).await;
    let selected = manager
        .preview_completed_children(root.thread_id, root.thread_id)
        .await
        .unwrap()
        .into_iter()
        .filter_map(|entry| entry.snapshot)
        .collect();
    *faults.shutdown.lock().unwrap() = Some((done.thread_id, None));
    let result = manager
        .cleanup_completed_children(root.thread_id, root.thread_id, selected)
        .await
        .unwrap();
    assert_eq!(result[0].outcome, "shutdown_unconfirmed");
    assert!(stored(&manager, done.thread_id).await);
    assert!(manager.get_thread(done.thread_id).await.is_ok());
    assert!(done.thread.submit(Op::Interrupt).await.is_err());
    assert_eq!(
        done.thread
            .io
            .admission
            .shutdown_result
            .load(Ordering::Acquire),
        2
    );
    assert!(faults.deletes.lock().unwrap().is_empty());
}

// 장치는 시작 future의 하위 정리 결과만 제어한다. 상위 종료 계층과
// 격리 LocalThreadStore는 실제 생산 구현을 그대로 사용한다.
async fn assert_mcp_failure_preserved(
    manager: &ThreadManager,
    faults: &StoreFaults,
    root: ThreadId,
    done: &super::super::NewThread,
) {
    tokio::time::timeout(
        std::time::Duration::from_secs(10),
        done.thread.wait_until_terminated(),
    )
    .await
    .unwrap();
    // 분리된 정리 소유자가 최종 판정까지 이 잠금을 보유한다.
    let gate = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        Arc::clone(&done.thread.residency_gate).write_owned(),
    )
    .await
    .unwrap();
    drop(gate);
    let error = done
        .thread
        .session
        .services
        .mcp_runtime
        .shutdown()
        .await
        .expect_err("the real MCP shutdown must retain its failure");
    assert!(
        error
            .to_string()
            .contains("test startup cleanup termination unconfirmed")
    );
    assert!(!done.thread.io.admission.shutdown_confirmed());
    assert!(faults.deletes.lock().unwrap().is_empty());
    assert!(stored(manager, done.thread_id).await);
    assert!(stored(manager, root).await);
    let registered = manager.get_thread(done.thread_id).await.unwrap();
    assert!(Arc::ptr_eq(&registered, &done.thread));
    assert!(manager.admit_thread_activity(done.thread_id).is_err());
    assert!(manager.admit_thread_activity(root).is_ok());
    // 정리 전에 얻은 Arc다. 소유 잠금 해제 후 실제 닫힌 진입 경로의 거부를 확인한다.
    assert!(done.thread.submit(Op::Interrupt).await.is_err());
}

#[tokio::test]
async fn cleanup_mcp_shutdown_failure_never_deletes_and_preserves_registered_owner() {
    let ((_home, manager, config, root), faults) = fixture_with_faults().await;
    let done = child(&manager, &config, &root).await;
    complete(&done.thread).await;
    // 초기 준비와 게시가 끝난 뒤 하위 시작 결과를 설치한다.
    done.thread.session.refresh_mcp_if_dirty().await;
    let selected = selection(&manager, root.thread_id).await;
    let probe = done
        .thread
        .session
        .services
        .mcp_runtime
        .install_unconfirmed_startup_for_test();
    probe.release();
    let result = manager
        .cleanup_completed_children(root.thread_id, root.thread_id, selected)
        .await
        .unwrap();
    assert_eq!(result[0].outcome, "shutdown_unconfirmed");
    assert_mcp_failure_preserved(&manager, &faults, root.thread_id, &done).await;
}

#[tokio::test]
async fn cleanup_mcp_shutdown_owner_survives_request_cancellation() {
    let ((_home, manager, config, root), faults) = fixture_with_faults().await;
    let done = child(&manager, &config, &root).await;
    complete(&done.thread).await;
    // 초기 준비와 게시가 끝난 뒤 하위 시작 결과를 설치한다.
    done.thread.session.refresh_mcp_if_dirty().await;
    let selected = selection(&manager, root.thread_id).await;
    let probe = done
        .thread
        .session
        .services
        .mcp_runtime
        .install_unconfirmed_startup_for_test();
    let owner_manager = copy_manager(&manager);
    let request = tokio::spawn(async move {
        owner_manager
            .cleanup_completed_children(root.thread_id, root.thread_id, selected)
            .await
    });
    tokio::time::timeout(
        std::time::Duration::from_secs(10),
        probe.wait_for_shutdown(),
    )
    .await
    .unwrap();
    request.abort();
    assert!(request.await.unwrap_err().is_cancelled());
    assert!(faults.deletes.lock().unwrap().is_empty());
    assert!(stored(&manager, done.thread_id).await);
    assert!(manager.admit_thread_activity(done.thread_id).is_err());
    probe.release();
    assert_mcp_failure_preserved(&manager, &faults, root.thread_id, &done).await;
}

#[tokio::test]
async fn cleanup_mcp_shutdown_timeout_never_restarts_deletion_after_late_failure() {
    let ((_home, manager, config, root), faults) = fixture_with_faults().await;
    let done = child(&manager, &config, &root).await;
    complete(&done.thread).await;
    // 초기 준비와 게시가 끝난 뒤 하위 시작 결과를 설치한다.
    done.thread.session.refresh_mcp_if_dirty().await;
    let selected = selection(&manager, root.thread_id).await;
    let probe = done
        .thread
        .session
        .services
        .mcp_runtime
        .install_unconfirmed_startup_for_test();
    let owner_manager = copy_manager(&manager);
    let request = tokio::spawn(async move {
        owner_manager
            .cleanup_completed_children(root.thread_id, root.thread_id, selected)
            .await
    });
    tokio::time::timeout(
        std::time::Duration::from_secs(10),
        probe.wait_for_shutdown(),
    )
    .await
    .unwrap();
    // 실제 AsyncManagedClient::shutdown의 시작 취소 이후에만 시계를 전진시킨다.
    tokio::time::pause();
    tokio::time::advance(std::time::Duration::from_secs(31)).await;
    let result = request.await.unwrap().unwrap();
    tokio::time::resume();
    assert_eq!(result[0].outcome, "shutdown_unconfirmed");
    assert!(faults.deletes.lock().unwrap().is_empty());
    assert!(stored(&manager, done.thread_id).await);
    assert!(manager.admit_thread_activity(done.thread_id).is_err());
    probe.release();
    assert_mcp_failure_preserved(&manager, &faults, root.thread_id, &done).await;
}

async fn selection(manager: &ThreadManager, root: ThreadId) -> Vec<CompletedChildSnapshot> {
    manager
        .preview_completed_children(root, root)
        .await
        .unwrap()
        .into_iter()
        .filter_map(|entry| entry.snapshot)
        .collect()
}

fn copy_manager(manager: &ThreadManager) -> ThreadManager {
    ThreadManager {
        state: Arc::clone(&manager.state),
        _test_codex_home_guard: None,
    }
}

#[tokio::test]
async fn cleanup_timeout_retains_data_and_fences_until_late_writer_shutdown() {
    let ((_home, manager, config, root), faults) = fixture_with_faults().await;
    let done = child(&manager, &config, &root).await;
    complete(&done.thread).await;
    let selected = selection(&manager, root.thread_id).await;
    let barrier = Arc::new(StoreBarrier::default());
    *faults.shutdown.lock().unwrap() = Some((done.thread_id, Some(Arc::clone(&barrier))));
    let owner_manager = copy_manager(&manager);
    let request = tokio::spawn(async move {
        owner_manager
            .cleanup_completed_children(root.thread_id, root.thread_id, selected)
            .await
    });
    wait_for(&barrier.entered).await;
    assert!(manager.admit_thread_activity(done.thread_id).is_err());
    // Advance the production deadline only after its real writer shutdown is blocked.
    tokio::time::pause();
    tokio::time::advance(std::time::Duration::from_secs(31)).await;
    let result = request.await.unwrap().unwrap();
    tokio::time::resume();
    assert_eq!(result[0].outcome, "shutdown_unconfirmed");
    assert!(stored(&manager, done.thread_id).await);
    assert!(faults.deletes.lock().unwrap().is_empty());
    barrier.release.notify_one();
    wait_for(&barrier.finished).await;
    // A late successful writer close must not restart deletion behind the reported timeout.
    assert!(stored(&manager, done.thread_id).await);
    assert!(faults.deletes.lock().unwrap().is_empty());
    assert!(manager.admit_thread_activity(done.thread_id).is_err());
    assert!(done.thread.submit(Op::Interrupt).await.is_err());
}

#[tokio::test]
async fn cleanup_owner_survives_request_cancellation_at_storage_boundary() {
    let ((_home, manager, config, root), faults) = fixture_with_faults().await;
    let done = child(&manager, &config, &root).await;
    complete(&done.thread).await;
    let selected = selection(&manager, root.thread_id).await;
    let barrier = Arc::new(StoreBarrier::default());
    *faults.delete.lock().unwrap() = Some((done.thread_id, Arc::clone(&barrier)));
    let owner_manager = copy_manager(&manager);
    let request = tokio::spawn(async move {
        owner_manager
            .cleanup_completed_children(root.thread_id, root.thread_id, selected)
            .await
    });
    wait_for(&barrier.entered).await;
    assert!(done.thread.io.admission.shutdown_confirmed());
    request.abort();
    assert!(request.await.unwrap_err().is_cancelled());
    assert!(manager.admit_thread_activity(done.thread_id).is_err());
    assert!(manager.admit_thread_activity(root.thread_id).is_ok());
    assert!(stored(&manager, done.thread_id).await);
    let old_arc = Arc::clone(&done.thread);
    let stale_submission = tokio::spawn(async move { old_arc.submit(Op::Interrupt).await });
    barrier.release.notify_one();
    wait_for(&barrier.finished).await;
    // The stale Arc waits for the owner's write gate, then observes closing.
    assert!(stale_submission.await.unwrap().is_err());
    assert!(!stored(&manager, done.thread_id).await);
    assert!(manager.get_thread(done.thread_id).await.is_err());
    assert_eq!(*faults.deletes.lock().unwrap(), vec![vec![done.thread_id]]);
    assert!(stored(&manager, root.thread_id).await);
}

#[tokio::test]
async fn cleanup_reports_each_target_after_middle_storage_failure() {
    let ((_home, manager, config, root), faults) = fixture_with_faults().await;
    let first = child(&manager, &config, &root).await;
    let failed = child(&manager, &config, &root).await;
    let last = child(&manager, &config, &root).await;
    let unselected = child(&manager, &config, &root).await;
    for thread in [
        &first.thread,
        &failed.thread,
        &last.thread,
        &unselected.thread,
    ] {
        complete(thread).await;
    }
    let snapshots = selection(&manager, root.thread_id).await;
    let selected = [first.thread_id, failed.thread_id, last.thread_id]
        .into_iter()
        .map(|id| {
            snapshots
                .iter()
                .find(|snapshot| snapshot.thread_id == id)
                .unwrap()
                .clone()
        })
        .collect();
    faults.fail_delete.lock().unwrap().insert(failed.thread_id);
    let result = manager
        .cleanup_completed_children(root.thread_id, root.thread_id, selected)
        .await
        .unwrap();
    assert_eq!(result.len(), 3);
    assert_eq!(result[0].outcome, "deleted");
    assert!(
        result[1]
            .outcome
            .starts_with("storage_unconfirmed_partial_deletion_possible:")
    );
    assert_eq!(result[2].outcome, "deleted");
    assert!(!stored(&manager, first.thread_id).await);
    assert!(stored(&manager, failed.thread_id).await);
    assert!(!stored(&manager, last.thread_id).await);
    assert!(stored(&manager, unselected.thread_id).await);
    assert!(stored(&manager, root.thread_id).await);
    assert!(failed.thread.io.admission.shutdown_confirmed());
    assert!(failed.thread.submit(Op::Interrupt).await.is_err());
    assert_eq!(
        *faults.deletes.lock().unwrap(),
        vec![
            vec![first.thread_id],
            vec![failed.thread_id],
            vec![last.thread_id]
        ]
    );
}

#[tokio::test]
async fn cleanup_keeps_uncertain_owner_closed_after_store_deleted_but_response_was_lost() {
    let ((_home, manager, config, root), faults) = fixture_with_faults().await;
    let lost = child(&manager, &config, &root).await;
    let sibling = child(&manager, &config, &root).await;
    complete(&lost.thread).await;
    let selected = selection(&manager, root.thread_id).await;
    faults
        .lose_delete_response
        .lock()
        .unwrap()
        .insert(lost.thread_id);
    let result = manager
        .cleanup_completed_children(root.thread_id, root.thread_id, selected)
        .await
        .unwrap();
    assert!(
        result[0]
            .outcome
            .starts_with("storage_unconfirmed_partial_deletion_possible:")
    );
    assert!(!stored(&manager, lost.thread_id).await);
    // The store error must not be advertised as successful unregistering, nor may
    // the old Arc or reload route reopen the now-missing record after uncertainty.
    assert!(Arc::ptr_eq(
        &manager.get_thread(lost.thread_id).await.unwrap(),
        &lost.thread
    ));
    assert!(lost.thread.submit(Op::Interrupt).await.is_err());
    assert!(manager.admit_thread_activity(lost.thread_id).is_err());
    assert!(manager.get_thread(sibling.thread_id).await.is_ok());
    assert!(stored(&manager, root.thread_id).await);
    assert_eq!(*faults.deletes.lock().unwrap(), vec![vec![lost.thread_id]]);
}

#[tokio::test]
async fn cancelled_child_acquisition_keeps_parent_pin_through_writer_rollback() {
    let ((_home, manager, config, root), faults) = fixture_with_faults().await;
    let parent = child(&manager, &config, &root).await;
    complete(&parent.thread).await;
    let selected = selection(&manager, root.thread_id).await;
    let create = Arc::new(StoreBarrier::default());
    let discard = Arc::new(StoreBarrier::default());
    *faults.create.lock().unwrap() = Some(Arc::clone(&create));
    *faults.discard.lock().unwrap() = Some(Arc::clone(&discard));
    let spawning_manager = copy_manager(&manager);
    let parent_id = parent.thread_id;
    let spawn = tokio::spawn(async move { child(&spawning_manager, &config, &parent).await });
    wait_for(&create.entered).await;
    spawn.abort();
    assert!(matches!(spawn.await, Err(error) if error.is_cancelled()));
    assert!(manager.state.cleanup_admission.reserve(parent_id).is_none());
    create.release.notify_one();
    wait_for(&discard.entered).await;
    let result = manager
        .cleanup_completed_children(root.thread_id, root.thread_id, selected)
        .await
        .unwrap();
    assert_eq!(result[0].outcome, "busy");
    assert!(stored(&manager, parent_id).await);
    assert!(faults.deletes.lock().unwrap().is_empty());
    discard.release.notify_one();
    wait_for(&discard.finished).await;
    // The rollback callback signals before SessionStartup releases its pin.
    wait_until_unpinned(&manager, parent_id).await;
    assert!(stored(&manager, parent_id).await);
    assert_eq!(manager.list_thread_ids().await.len(), 2);
}

#[tokio::test]
async fn cleanup_rejects_previous_generation_after_real_writer_reopen() {
    let ((_home, manager, config, root), faults) = fixture_with_faults().await;
    let done = child(&manager, &config, &root).await;
    complete(&done.thread).await;
    let old = selection(&manager, root.thread_id).await.pop().unwrap();
    let path = done.thread.rollout_path().unwrap();
    done.thread.io.shutdown_and_wait().await.unwrap();
    manager
        .remove_thread_if_matches(&done.thread_id, &done.thread)
        .await;
    let history = manager
        .legacy_initial_history_from_rollout_path(path)
        .await
        .unwrap();
    let reopened = manager
        .state
        .spawn_child_thread(
            StartThreadOptions {
                initial_history: history,
                session_source: Some(done.thread.session_source.clone()),
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
    assert_eq!(reopened.thread_id, done.thread_id);
    assert_ne!(reopened.thread.io.admission.runtime_id, old.runtime_id);
    assert!(!Arc::ptr_eq(&reopened.thread, &done.thread));
    // The passive subscription commit must reject an old Arc after a real
    // shutdown/reopen, even though the public thread ID is unchanged.
    assert!(matches!(
        manager
            .lock_loaded_thread(done.thread_id, &done.thread)
            .await,
        Err(LoadedThreadUnavailable::Replaced)
    ));
    drop(
        manager
            .lock_loaded_thread(reopened.thread_id, &reopened.thread)
            .await
            .unwrap(),
    );
    complete(&reopened.thread).await;
    let mut stale_generation = selection(&manager, root.thread_id).await.pop().unwrap();
    // All fields describe the current eligible runtime except its execution generation.
    // This isolates the generation check from the new turn's activity/terminal identity.
    stale_generation.runtime_id = old.runtime_id;
    let result = manager
        .cleanup_completed_children(root.thread_id, root.thread_id, vec![stale_generation])
        .await
        .unwrap();
    assert_eq!(result[0].outcome, "changed");
    assert!(stored(&manager, done.thread_id).await);
    assert!(Arc::ptr_eq(
        &manager.get_thread(done.thread_id).await.unwrap(),
        &reopened.thread
    ));
    assert!(faults.deletes.lock().unwrap().is_empty());
    assert!(!reopened.thread.io.admission.is_closing());
}

#[tokio::test]
async fn cancelled_spawn_keeps_parent_pin_until_closed_edge_is_durable() {
    let graph_home = tempfile::tempdir().unwrap();
    let db = codex_state::StateRuntime::init(
        codex_state::SqliteConfig::new_for_testing(
            graph_home.path().to_path_buf().try_into().unwrap(),
        ),
        "test-provider".into(),
    )
    .await
    .unwrap();
    let graph = Arc::new(BarrierGraph {
        inner: codex_agent_graph_store::LocalAgentGraphStore::new(db),
        open: StoreBarrier::default(),
        close: StoreBarrier::default(),
        child: StdMutex::new(None),
    });
    let ((_home, manager, config, root), faults) = fixture_with_graph(Some(graph.clone())).await;
    let parent = child(&manager, &config, &root).await;
    complete(&parent.thread).await;
    let parent_id = parent.thread_id;
    let selected = selection(&manager, root.thread_id).await;
    let control = parent
        .thread
        .session
        .services
        .local_agent_runtime
        .control(parent.thread.session.session_id());
    let spawn = tokio::spawn(async move {
        control
            .spawn_agent(
                config,
                Vec::new(),
                Some(SessionSource::SubAgent(SubAgentSource::ThreadSpawn {
                    parent_thread_id: parent_id,
                    depth: 2,
                    agent_path: None,
                    agent_nickname: None,
                    agent_role: None,
                })),
            )
            .await
    });
    wait_for(&graph.open.entered).await;
    let child_id = graph.child.lock().unwrap().unwrap();
    spawn.abort();
    assert!(matches!(spawn.await, Err(error) if error.is_cancelled()));
    assert!(manager.state.cleanup_admission.reserve(parent_id).is_none());
    graph.open.release.notify_one();
    wait_for(&graph.close.entered).await;
    // This is after Open's task has returned (and dropped its separate pin),
    // while PendingSpawn's cancellation owner is still writing Closed.
    assert!(manager.state.cleanup_admission.reserve(parent_id).is_none());
    let result = manager
        .cleanup_completed_children(root.thread_id, root.thread_id, selected)
        .await
        .unwrap();
    assert_eq!(result[0].outcome, "busy");
    assert!(faults.deletes.lock().unwrap().is_empty());
    graph.close.release.notify_one();
    wait_for(&graph.close.finished).await;
    wait_until_unpinned(&manager, parent_id).await;
    assert!(
        graph
            .list_thread_spawn_children(
                parent_id,
                Some(codex_agent_graph_store::ThreadSpawnEdgeStatus::Open)
            )
            .await
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        graph
            .list_thread_spawn_children(
                parent_id,
                Some(codex_agent_graph_store::ThreadSpawnEdgeStatus::Closed)
            )
            .await
            .unwrap(),
        vec![child_id]
    );
    assert!(manager.get_thread(child_id).await.is_err());
    assert!(stored(&manager, parent_id).await);
}

async fn wait_until_unpinned(manager: &ThreadManager, id: ThreadId) {
    tokio::time::timeout(std::time::Duration::from_secs(10), async {
        loop {
            if let Some(reservation) = manager.state.cleanup_admission.reserve(id) {
                drop(reservation);
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn accepted_non_trigger_mail_remains_a_cleanup_blocker_after_submit_guard_drops() {
    let ((_home, manager, config, root), faults) = fixture_with_faults().await;
    let done = child(&manager, &config, &root).await;
    complete(&done.thread).await;
    let selected = selection(&manager, root.thread_id).await;
    done.thread
        .submit(Op::InterAgentCommunication {
            communication: codex_protocol::protocol::InterAgentCommunication::new(
                codex_protocol::AgentPath::root(),
                codex_protocol::AgentPath::try_from("/root/worker").unwrap(),
                Vec::new(),
                "pending work".into(),
                false,
            ),
            start_options: Default::default(),
        })
        .await
        .unwrap();
    // Fair FIFO write acquisition fences completion of the accepted submission.
    let dispatched = Arc::clone(&done.thread.residency_gate).write_owned().await;
    drop(dispatched);
    assert!(done.thread.session.active_turn.lock().await.is_none());
    assert!(
        done.thread
            .session
            .input_queue
            .has_pending_mailbox_items()
            .await
    );
    assert!(!done.thread.cleanup_is_idle().await);
    let result = manager
        .cleanup_completed_children(root.thread_id, root.thread_id, selected)
        .await
        .unwrap();
    assert_eq!(result[0].outcome, "changed");
    assert!(stored(&manager, done.thread_id).await);
    assert!(faults.deletes.lock().unwrap().is_empty());
    assert!(
        done.thread
            .session
            .input_queue
            .has_pending_mailbox_items()
            .await
    );
}

#[test]
fn cleanup_fences_exact_ids_and_retains_committed_tombstones() {
    let admission = Arc::new(CleanupAdmission::default());
    let child = ThreadId::new();
    let sibling = ThreadId::new();
    let spawn = admission.admit([child]).unwrap();
    assert!(admission.reserve(child).is_none());
    drop(spawn);
    let reservation = admission.reserve(child).unwrap();
    assert!(admission.admit([child]).is_err());
    assert!(admission.admit([sibling]).is_ok());
    drop(reservation);
    assert!(admission.admit([child]).is_ok());
    let mut reservation = admission.reserve(child).unwrap();
    reservation.committed = true;
    drop(reservation);
    assert!(admission.admit([child]).is_err());
    assert!(admission.admit([sibling]).is_ok());
}
#[tokio::test]
async fn runtime_gate_blocks_cleanup_until_accepted_input_is_released() {
    let admission = RuntimeAdmission::default();
    let input = Arc::clone(&admission.gate).read_owned().await;
    assert!(Arc::clone(&admission.gate).try_write_owned().is_err());
    drop(input);
    let _owner = Arc::clone(&admission.gate).try_write_owned().unwrap();
    admission.close();
    assert!(admission.is_closing());
    assert!(!admission.shutdown_confirmed());
    admission.shutdown_finished();
    assert!(!admission.shutdown_confirmed());
    admission.loop_completed.store(true, Ordering::Release);
    assert!(admission.shutdown_confirmed());
    admission.shutdown_failed();
    assert!(!admission.shutdown_confirmed());
}

async fn wait_for(notification: &tokio::sync::Notify) {
    tokio::time::timeout(std::time::Duration::from_secs(10), notification.notified())
        .await
        .expect("real owner did not reach the expected storage barrier");
}

#[tokio::test]
async fn loaded_runtime_read_guard_linearizes_raw_removal_without_touching_activity() {
    let (_home, manager, _, root) = fixture().await;
    let revision = root.thread.io.admission.activity_revision();
    let guard = manager
        .lock_loaded_thread(root.thread_id, &root.thread)
        .await
        .unwrap();
    let removal = manager.remove_thread(&root.thread_id);
    tokio::pin!(removal);
    assert!(
        futures::poll!(removal.as_mut()).is_pending(),
        "raw removal must await the subscription commit"
    );
    drop(guard);
    let removed = removal.await.unwrap();
    assert!(Arc::ptr_eq(&removed, &root.thread));
    assert!(matches!(
        manager
            .lock_loaded_thread(root.thread_id, &root.thread)
            .await,
        Err(LoadedThreadUnavailable::NotLoaded)
    ));
    assert_eq!(root.thread.io.admission.activity_revision(), revision);
    removed.shutdown_and_wait().await.unwrap();
}
