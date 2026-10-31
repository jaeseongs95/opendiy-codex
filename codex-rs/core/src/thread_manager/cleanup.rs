//! Exact, preview-bound cleanup of terminal local child runtimes.
//!
//! The manager fence covers a UUID's reload, child creation and unloaded mail;
//! the runtime gate covers submitted input, including handles obtained before preview.

use super::*;
use codex_protocol::error::CodexErrorDetails;
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
    managed_close_owner: std::sync::Mutex<Option<Uuid>>,
    #[cfg(test)]
    pub(crate) managed_close_flush_hook:
        StdMutex<Option<(Arc<tokio::sync::Notify>, Arc<tokio::sync::Notify>)>>,
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
            managed_close_owner: std::sync::Mutex::new(None),
            #[cfg(test)]
            managed_close_flush_hook: StdMutex::new(None),
            shutdown_result: AtomicU8::new(0),
            loop_completed: AtomicBool::new(false),
        }
    }
}
impl RuntimeAdmission {
    pub(crate) fn is_closing(&self) -> bool {
        self.closing.load(Ordering::Acquire)
    }
    #[cfg(test)]
    pub(crate) fn activity_revision(&self) -> u64 {
        self.revision.load(Ordering::Acquire)
    }
    pub(crate) fn touch(&self) {
        self.revision.fetch_add(1, Ordering::AcqRel);
    }
    pub(crate) fn close(&self) {
        let mut owner = self
            .managed_close_owner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        *owner = None;
        self.closing.store(true, Ordering::Release);
    }
    pub(crate) fn reserve_managed_close(&self, token: Uuid) -> bool {
        let mut owner = self
            .managed_close_owner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if self.is_closing() {
            return false;
        }
        *owner = Some(token);
        self.closing.store(true, Ordering::Release);
        true
    }
    pub(crate) fn rollback_managed_close(&self, token: Uuid) {
        let mut owner = self
            .managed_close_owner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if *owner == Some(token) {
            *owner = None;
            self.closing.store(false, Ordering::Release);
        }
    }
    pub(crate) fn shutdown_failed(&self) {
        self.shutdown_result.store(2, Ordering::Release);
    }
    pub(crate) fn shutdown_finished(&self) {
        let _ = self
            .shutdown_result
            .compare_exchange(0, 1, Ordering::AcqRel, Ordering::Acquire);
    }
    pub(crate) fn shutdown_confirmed(&self) -> bool {
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
            let stored = match self
                .state
                .read_stored_thread(ReadThreadParams {
                    thread_id: id,
                    include_archived: true,
                    include_history: false,
                })
                .await
            {
                Ok(stored) => stored,
                // An untouched live child may not have a saved record yet.
                // Missing durable ancestry cannot authorize cleanup.
                Err(error) if matches!(error.details(), CodexErrorDetails::ThreadNotFound(_)) => {
                    return Ok(false);
                }
                Err(error) => return Err(error),
            };
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
            )
            || !parent_thread
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
#[path = "cleanup_tests.rs"]
mod tests;
