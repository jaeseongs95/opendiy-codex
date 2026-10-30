//! Exact, preview-bound cleanup of terminal local child runtimes.
//!
//! The manager fence covers a UUID's reload, child creation and unloaded mail;
//! the runtime gate covers submitted input, including handles obtained before preview.

use super::*;
use codex_thread_store::DeleteThreadsParams;
use codex_thread_store::ListTurnsParams;
use codex_thread_store::SortDirection;
use codex_thread_store::StoredTurnItemsView;
use codex_thread_store::StoredTurnStatus;
use serde::Deserialize;
use serde::Serialize;
use std::sync::Mutex as StdMutex;
use std::sync::atomic::AtomicU8;
use std::sync::atomic::AtomicU64;
use uuid::Uuid;

#[derive(Default)]
pub(crate) struct CleanupAdmission(StdMutex<HashMap<ThreadId, AdmissionState>>);
#[derive(Default)]
struct AdmissionState {
    operations: usize,
    closing: bool,
}
pub(crate) struct CleanupOperation {
    admission: Arc<CleanupAdmission>,
    ids: Vec<ThreadId>,
}
struct CleanupReservation {
    admission: Arc<CleanupAdmission>,
    id: ThreadId,
    committed: bool,
}

impl CleanupAdmission {
    pub(crate) fn admit(
        self: &Arc<Self>,
        ids: impl IntoIterator<Item = ThreadId>,
    ) -> CodexResult<CleanupOperation> {
        let mut ids: Vec<_> = ids.into_iter().collect();
        ids.sort_unstable_by_key(ToString::to_string);
        ids.dedup();
        let mut states = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if ids
            .iter()
            .any(|id| states.get(id).is_some_and(|state| state.closing))
        {
            return Err(CodexErr::InvalidRequest(
                "thread is reserved for cleanup".into(),
            ));
        }
        for id in &ids {
            states.entry(*id).or_default().operations += 1;
        }
        Ok(CleanupOperation {
            admission: Arc::clone(self),
            ids,
        })
    }
    fn reserve(self: &Arc<Self>, id: ThreadId) -> Option<CleanupReservation> {
        let mut states = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let state = states.entry(id).or_default();
        if state.closing || state.operations != 0 {
            return None;
        }
        state.closing = true;
        Some(CleanupReservation {
            admission: Arc::clone(self),
            id,
            committed: false,
        })
    }
}
impl Drop for CleanupOperation {
    fn drop(&mut self) {
        let mut states = self
            .admission
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        for id in &self.ids {
            if let Some(state) = states.get_mut(id) {
                state.operations -= 1;
                if state.operations == 0 && !state.closing {
                    states.remove(id);
                }
            }
        }
    }
}
impl Drop for CleanupReservation {
    fn drop(&mut self) {
        if !self.committed {
            self.admission
                .0
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .remove(&self.id);
        }
        // A committed tombstone rejects stale reloads even after deletion or an uncertain shutdown.
    }
}

pub(crate) struct RuntimeAdmission {
    pub(crate) gate: Arc<RwLock<()>>,
    pub(crate) runtime_id: String,
    revision: AtomicU64,
    closing: AtomicBool,
    pub(crate) shutdown_result: AtomicU8,
    pub(crate) loop_completed: AtomicBool,
}
impl Default for RuntimeAdmission {
    fn default() -> Self {
        Self {
            gate: Arc::default(),
            runtime_id: Uuid::new_v4().to_string(),
            revision: AtomicU64::new(0),
            closing: AtomicBool::new(false),
            shutdown_result: AtomicU8::new(0),
            loop_completed: AtomicBool::new(false),
        }
    }
}
impl RuntimeAdmission {
    pub(crate) fn is_closing(&self) -> bool {
        self.closing.load(Ordering::Acquire)
    }
    pub(crate) fn touch(&self) {
        self.revision.fetch_add(1, Ordering::AcqRel);
    }
    pub(crate) fn close(&self) {
        self.closing.store(true, Ordering::Release);
    }
    pub(crate) fn shutdown_failed(&self) {
        self.shutdown_result.store(2, Ordering::Release);
    }
    pub(crate) fn shutdown_finished(&self) {
        let _ = self
            .shutdown_result
            .compare_exchange(0, 1, Ordering::AcqRel, Ordering::Acquire);
    }
    fn shutdown_confirmed(&self) -> bool {
        self.shutdown_result.load(Ordering::Acquire) == 1
            && self.loop_completed.load(Ordering::Acquire)
    }
}

/// A preview binds confirmation to one runtime and one accepted-input revision.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompletedChildSnapshot {
    pub thread_id: ThreadId,
    pub runtime_id: String,
    pub activity_revision: u64,
    pub last_turn_id: String,
    pub terminal_status: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompletedChildCleanupResult {
    pub thread_id: ThreadId,
    pub outcome: String,
    pub snapshot: Option<CompletedChildSnapshot>,
}

/// Pins one thread against cleanup while a host accepts durable work for it.
pub struct ThreadActivityGuard {
    _operation: CleanupOperation,
}

impl ThreadManager {
    pub fn admit_thread_activity(&self, id: ThreadId) -> CodexResult<ThreadActivityGuard> {
        Ok(ThreadActivityGuard {
            _operation: self.state.cleanup_admission.admit([id])?,
        })
    }

    /// Read-only preview. Unloaded, host-owned, active and unknown children are preserved.
    pub async fn preview_completed_children(
        &self,
        parent: ThreadId,
        current: ThreadId,
    ) -> CodexResult<Vec<CompletedChildCleanupResult>> {
        self.validate_cleanup_context(parent, current).await?;
        let ids = self.list_agent_subtree_thread_ids(parent).await?;
        let mut entries = Vec::new();
        for id in ids.into_iter().filter(|id| *id != parent) {
            let snapshot = if id == current {
                None
            } else {
                self.completed_child_snapshot(parent, id).await?
            };
            entries.push(CompletedChildCleanupResult {
                thread_id: id,
                outcome: if id == current {
                    "protected"
                } else if snapshot.is_some() {
                    "eligible"
                } else {
                    "preserved"
                }
                .into(),
                snapshot,
            });
        }
        Ok(entries)
    }

    /// The owner task retains all Core fences if the request waiter disconnects.
    /// The host must retain its durable queue and goal mutation guards through this call.
    pub async fn cleanup_completed_children(
        &self,
        parent: ThreadId,
        current: ThreadId,
        selected: Vec<CompletedChildSnapshot>,
    ) -> CodexResult<Vec<CompletedChildCleanupResult>> {
        let manager = Self {
            state: Arc::clone(&self.state),
            _test_codex_home_guard: None,
        };
        tokio::spawn(async move {
            manager
                .cleanup_selected_children(parent, current, selected)
                .await
        })
        .await?
    }

    async fn validate_cleanup_context(
        &self,
        parent: ThreadId,
        current: ThreadId,
    ) -> CodexResult<()> {
        let parent_thread = self.get_thread(parent).await?;
        let current_thread = self.get_thread(current).await?;
        if self.state.agent_control_factory.is_some()
            || !parent_thread.is_running()
            || !current_thread.is_running()
        {
            return Err(CodexErr::InvalidRequest(
                "cleanup requires loaded, locally owned parent and current runtimes".into(),
            ));
        }
        if current != parent && current_thread.session_source.parent_thread_id() != Some(parent) {
            return Err(CodexErr::InvalidRequest(
                "current thread does not belong to the supplied parent".into(),
            ));
        }
        // Compare actual runtime ownership rather than trusting client-provided UUIDs.
        if !parent_thread
            .session
            .services
            .local_agent_runtime
            .shares_registry(&current_thread.session.services.local_agent_runtime)
        {
            return Err(CodexErr::InvalidRequest(
                "current and parent runtime ownership differs".into(),
            ));
        }
        Ok(())
    }

    async fn cleanup_owned_descendant(
        &self,
        parent: ThreadId,
        mut id: ThreadId,
    ) -> CodexResult<bool> {
        let mut seen = HashSet::new();
        while id != parent {
            if !seen.insert(id) {
                return Ok(false);
            }
            let stored = self
                .state
                .read_stored_thread(ReadThreadParams {
                    thread_id: id,
                    include_archived: true,
                    include_history: false,
                })
                .await?;
            let Some(recorded_parent) = stored.parent_thread_id else {
                return Ok(false);
            };
            if stored.source.parent_thread_id() != Some(recorded_parent) {
                return Ok(false);
            }
            if let Ok(loaded) = self.get_thread(id).await {
                if loaded.session_source.parent_thread_id() != Some(recorded_parent) {
                    return Ok(false);
                }
            }
            id = recorded_parent;
        }
        Ok(!seen.is_empty())
    }

    async fn completed_child_snapshot(
        &self,
        parent: ThreadId,
        id: ThreadId,
    ) -> CodexResult<Option<CompletedChildSnapshot>> {
        if !self.cleanup_owned_descendant(parent, id).await? {
            return Ok(None);
        }
        let Ok(thread) = self.get_thread(id).await else {
            return Ok(None);
        };
        let parent_thread = self.get_thread(parent).await?;
        if self.state.agent_control_factory.is_some()
            || thread.multi_agent_version() != Some(codex_protocol::protocol::MultiAgentVersion::V2)
            || !matches!(
                thread.session_source,
                SessionSource::SubAgent(SubAgentSource::ThreadSpawn { .. })
            )
            || !parent_thread
                .session
                .services
                .local_agent_runtime
                .shares_registry(&thread.session.services.local_agent_runtime)
            || thread.io.admission.is_closing()
        {
            return Ok(None);
        }
        let Ok(_gate) = Arc::clone(&thread.residency_gate).try_write_owned() else {
            return Ok(None);
        };
        self.completed_child_snapshot_under_gate(&thread).await
    }

    async fn completed_child_snapshot_under_gate(
        &self,
        thread: &Arc<CodexThread>,
    ) -> CodexResult<Option<CompletedChildSnapshot>> {
        let id = thread.session.thread_id;
        if !thread.cleanup_is_idle().await
            || !matches!(
                thread.agent_status().await,
                AgentStatus::Completed(_) | AgentStatus::Errored(_) | AgentStatus::Interrupted
            )
        {
            return Ok(None);
        }
        let Some(last_turn_id) = thread.session.cleanup_last_started_turn().await else {
            return Ok(None);
        };
        thread.flush_rollout().await?;
        let stored = self
            .state
            .read_stored_thread(ReadThreadParams {
                thread_id: id,
                include_archived: true,
                include_history: false,
            })
            .await?;
        if stored.parent_thread_id != thread.session_source.parent_thread_id()
            || stored.source.parent_thread_id() != stored.parent_thread_id
            || stored.parent_thread_id.is_none()
        {
            return Ok(None);
        }
        let turns = self
            .state
            .thread_store
            .list_turns(ListTurnsParams {
                thread_id: id,
                include_archived: true,
                cursor: None,
                page_size: 1,
                sort_direction: SortDirection::Desc,
                items_view: StoredTurnItemsView::NotLoaded,
            })
            .await
            .map_err(|error| {
                CodexErr::Fatal(format!("failed to verify terminal child turn: {error}"))
            })?;
        let Some(turn) = turns.turns.first() else {
            return Ok(None);
        };
        if turn.turn_id != last_turn_id {
            return Ok(None);
        }
        let status = match turn.status {
            StoredTurnStatus::Completed => "completed",
            StoredTurnStatus::Failed => "failed",
            StoredTurnStatus::Interrupted => "cancelled",
            StoredTurnStatus::InProgress => return Ok(None),
        };
        Ok(Some(CompletedChildSnapshot {
            thread_id: id,
            runtime_id: thread.io.admission.runtime_id.clone(),
            activity_revision: thread.io.admission.revision.load(Ordering::Acquire),
            last_turn_id,
            terminal_status: status.into(),
        }))
    }

    async fn cleanup_selected_children(
        &self,
        parent: ThreadId,
        current: ThreadId,
        mut selected: Vec<CompletedChildSnapshot>,
    ) -> CodexResult<Vec<CompletedChildCleanupResult>> {
        self.validate_cleanup_context(parent, current).await?;
        if self
            .state
            .thread_store
            .as_any()
            .downcast_ref::<LocalThreadStore>()
            .is_none()
        {
            return Err(CodexErr::InvalidRequest(
                "completed-child cleanup requires the local thread store".into(),
            ));
        }
        let mut seen = HashSet::new();
        if selected
            .iter()
            .any(|snapshot| !seen.insert(snapshot.thread_id))
        {
            return Err(CodexErr::InvalidRequest(
                "duplicate cleanup selection".into(),
            ));
        }
        // Retry only the fixed selection as successful leaf removal exposes selected parents.
        let mut results = Vec::new();
        while !selected.is_empty() {
            let before = selected.len();
            let mut deferred = Vec::new();
            for expected in selected {
                let id = expected.thread_id;
                let result = self.cleanup_one_child(parent, current, &expected).await;
                match result {
                    Ok(outcome) if outcome == "has_descendants" => deferred.push(expected),
                    Ok(outcome) => results.push(CompletedChildCleanupResult {
                        thread_id: id,
                        outcome,
                        snapshot: None,
                    }),
                    Err(error) => results.push(CompletedChildCleanupResult {
                        thread_id: id,
                        outcome: format!("unconfirmed: {error}"),
                        snapshot: None,
                    }),
                }
            }
            if deferred.len() == before {
                results.extend(
                    deferred
                        .into_iter()
                        .map(|snapshot| CompletedChildCleanupResult {
                            thread_id: snapshot.thread_id,
                            outcome: "has_descendants".into(),
                            snapshot: None,
                        }),
                );
                break;
            }
            selected = deferred;
        }
        Ok(results)
    }

    async fn cleanup_one_child(
        &self,
        parent: ThreadId,
        current: ThreadId,
        expected: &CompletedChildSnapshot,
    ) -> CodexResult<String> {
        let id = expected.thread_id;
        if id == parent || id == current {
            return Ok("protected".into());
        }
        let Some(mut reservation) = self.state.cleanup_admission.reserve(id) else {
            return Ok("busy".into());
        };
        self.validate_cleanup_context(parent, current).await?;
        if !self.cleanup_owned_descendant(parent, id).await?
            || !self
                .list_agent_subtree_thread_ids(parent)
                .await?
                .contains(&id)
        {
            return Ok("protected".into());
        }
        if self
            .list_agent_subtree_thread_ids(id)
            .await?
            .iter()
            .any(|child| *child != id)
        {
            return Ok("has_descendants".into());
        }
        let Ok(thread) = self.get_thread(id).await else {
            return Ok("unknown".into());
        };
        let parent_thread = self.get_thread(parent).await?;
        if thread.multi_agent_version() != Some(codex_protocol::protocol::MultiAgentVersion::V2)
            || !matches!(
            thread.session_source,
            SessionSource::SubAgent(SubAgentSource::ThreadSpawn { .. })
        ) || !parent_thread
            .session
            .services
            .local_agent_runtime
            .shares_registry(&thread.session.services.local_agent_runtime)
        {
            return Ok("protected".into());
        }
        let Ok(gate) = Arc::clone(&thread.residency_gate).try_write_owned() else {
            return Ok("busy".into());
        };
        if self
            .completed_child_snapshot_under_gate(&thread)
            .await?
            .as_ref()
            != Some(expected)
        {
            return Ok("changed".into());
        }
        if !self
            .get_thread(id)
            .await
            .is_ok_and(|registered| Arc::ptr_eq(&registered, &thread))
        {
            return Ok("changed".into());
        }
        if let Some(store) = self
            .state
            .thread_store
            .as_any()
            .downcast_ref::<LocalThreadStore>()
        {
            store
                .ensure_no_attached_threads(&[id])
                .await
                .map_err(|error| CodexErr::InvalidRequest(error.to_string()))?;
        }
        if !thread.session.close_for_cleanup().await {
            return Ok("busy".into());
        }
        reservation.committed = true;
        match tokio::time::timeout(
            std::time::Duration::from_secs(30),
            thread.io.shutdown_for_cleanup(&gate),
        )
        .await
        {
            Ok(Ok(())) if thread.io.admission.shutdown_confirmed() => {}
            _ => return Ok("shutdown_unconfirmed".into()),
        }
        if !self
            .get_thread(id)
            .await
            .is_ok_and(|registered| Arc::ptr_eq(&registered, &thread))
            || self
                .list_agent_subtree_thread_ids(id)
                .await?
                .iter()
                .any(|child| *child != id)
        {
            return Ok("changed".into());
        }
        // Never expand this exact one-ID operation into a subtree deletion.
        if let Err(error) = self
            .state
            .thread_store
            .delete_threads(DeleteThreadsParams {
                thread_ids: vec![id],
            })
            .await
        {
            return Ok(format!(
                "storage_unconfirmed_partial_deletion_possible: {error}"
            ));
        }
        self.remove_thread_if_matches(&id, &thread).await;
        thread
            .session
            .services
            .local_agent_runtime
            .forget_cleaned_child(id);
        Ok("deleted".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    async fn fixture() -> (
        tempfile::TempDir,
        ThreadManager,
        crate::config::Config,
        super::super::NewThread,
    ) {
        let home = tempfile::tempdir().unwrap();
        let mut config = crate::config::test_config().await;
        config.codex_home = home.path().to_path_buf().try_into().unwrap();
        config.cwd = home.path().to_path_buf().try_into().unwrap();
        let _ = config
            .features
            .enable(codex_features::Feature::MultiAgentV2);
        let manager = ThreadManager::with_models_provider_and_home_for_tests(
            codex_login::CodexAuth::from_api_key("dummy"),
            config.model_provider.clone(),
            config.codex_home.to_path_buf(),
            Arc::new(codex_exec_server::EnvironmentManager::default_for_tests()),
        );
        let root = manager
            .start_thread(StartThreadOptions::new(config.clone()))
            .await
            .unwrap();
        root.thread.ensure_rollout_materialized().await;
        (home, manager, config, root)
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
    async fn cleanup_never_deletes_after_unconfirmed_shutdown() {
        let (_home, manager, config, root) = fixture().await;
        let done = child(&manager, &config, &root).await;
        complete(&done.thread).await;
        let selected = manager
            .preview_completed_children(root.thread_id, root.thread_id)
            .await
            .unwrap()
            .into_iter()
            .filter_map(|entry| entry.snapshot)
            .collect();
        // Model a writer/MCP failure recorded by the real shutdown handler.
        done.thread.io.admission.shutdown_failed();
        let result = manager
            .cleanup_completed_children(root.thread_id, root.thread_id, selected)
            .await
            .unwrap();
        assert_eq!(result[0].outcome, "shutdown_unconfirmed");
        assert!(stored(&manager, done.thread_id).await);
        assert!(manager.get_thread(done.thread_id).await.is_ok());
        assert!(done.thread.submit(Op::Interrupt).await.is_err());
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
}
