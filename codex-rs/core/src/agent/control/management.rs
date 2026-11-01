// Modified for OpenDIY Codex R32 (2026-11-01); see MODIFICATIONS.md.
//! Metadata-only operator access, fenced against runtime replacement at acceptance.

use super::LocalAgentControl;
use crate::agent::api::AgentControl;
use crate::agent::management::*;
use crate::codex_thread::CodexThread;
use crate::session::Submission;
use crate::session::new_submission_id;
use codex_protocol::ThreadId;
use codex_protocol::error::CodexErr;
use codex_protocol::error::Result;
use codex_protocol::protocol::AgentStatus;
use codex_protocol::protocol::Op;
use codex_protocol::turn_input::TurnInputMode;
use codex_protocol::turn_input::TurnInputRequest;
use codex_protocol::turn_input::TurnInputSubmission;
use std::collections::HashSet;
use std::sync::Arc;
use uuid::Uuid;

fn bounded(value: Option<String>) -> Option<String> {
    value.filter(|value| value.len() <= 256)
}

fn available(thread: &CodexThread) -> bool {
    !thread.session_source.is_internal() && thread.is_running() && !thread.io.admission.is_closing()
}

fn same_controller(control: &LocalAgentControl, thread: &CodexThread) -> bool {
    thread.session.services.agent_control.identity() == control.identity()
        && Arc::ptr_eq(
            &thread.session.services.local_agent_runtime.registry,
            &control.runtime.registry,
        )
}

fn status(status: &AgentStatus, active: bool) -> ManagedSubagentStatus {
    if active {
        return ManagedSubagentStatus::Running;
    }
    match status {
        AgentStatus::PendingInit => ManagedSubagentStatus::Pending,
        AgentStatus::Running | AgentStatus::Interrupted => ManagedSubagentStatus::Idle,
        AgentStatus::Completed(_) => ManagedSubagentStatus::Completed,
        AgentStatus::Errored(_) => ManagedSubagentStatus::Errored,
        // Closed is reserved for metadata whose runtime has been removed.
        AgentStatus::Shutdown | AgentStatus::NotFound => ManagedSubagentStatus::Unknown,
    }
}

fn enqueue(thread: &Arc<CodexThread>, op: Op) -> Result<()> {
    let pin = Arc::clone(&thread.residency_gate)
        .try_read_owned()
        .map_err(|_| CodexErr::InvalidRequest("managed subagent is changing residency".into()))?;
    if !available(thread) {
        return Err(CodexErr::InvalidRequest(
            "managed subagent is closing".into(),
        ));
    }
    thread
        .io
        .tx_sub
        .try_send(Submission {
            id: new_submission_id(),
            op,
            trace: None,
            turn_extension_init: None,
            parent_turn_id: None,
            root_turn_id: None,
            residency_guard: Some(pin),
            message_delivery: None,
        })
        .map_err(|_| {
            CodexErr::InvalidRequest("managed subagent input queue is unavailable".into())
        })?;
    thread.io.admission.touch();
    Ok(())
}

impl LocalAgentControl {
    pub(super) async fn managed_list(&self, parent_id: ThreadId) -> Result<ManagedSubagentList> {
        let state = self.runtime.upgrade()?;
        let observed_parent = state.get_thread(parent_id).await?;
        let version = observed_parent.managed_multi_agent_version().await;
        let threads = state.threads.read().await;
        let parent = threads
            .get(&parent_id)
            .filter(|parent| {
                Arc::ptr_eq(parent, &observed_parent)
                    && available(parent)
                    && same_controller(self, parent)
            })
            .ok_or(CodexErr::ThreadNotFound(parent_id))?;
        if parent.multi_agent_version().unwrap_or(version)
            != codex_protocol::protocol::MultiAgentVersion::V2
        {
            return Err(CodexErr::UnsupportedOperation(
                "operator subagent management requires multi-agent v2".into(),
            ));
        }
        let parent_runtime_id = parent.runtime_id();
        let mut agents = Vec::new();
        for metadata in self.runtime.registry.managed_children(parent_id)? {
            let Some(child_id) = metadata.agent_id else {
                continue;
            };
            let child = threads.get(&child_id).filter(|child| {
                child.session_source.parent_thread_id() == Some(parent_id)
                    && same_controller(self, child)
            });
            let mut entry = ManagedSubagent {
                thread_id: child_id,
                parent_thread_id: parent_id,
                runtime_id: child.map(|child| child.runtime_id()),
                nickname: bounded(metadata.agent_nickname),
                role: bounded(metadata.agent_role),
                model: None,
                reasoning_effort: None,
                status: ManagedSubagentStatus::Unknown,
                active_turn_id: None,
                capabilities: ManagedSubagentCapabilities::default(),
            };
            if let Some(child) = child {
                let config = child.config_snapshot().await;
                entry.model = bounded(Some(config.model));
                entry.reasoning_effort = config.reasoning_effort;
                {
                    let active = child.session.active_turn.lock().await;
                    if let Some(task) = active
                        .as_ref()
                        .and_then(|turn| turn.task.as_ref())
                        .filter(|task| !task.cancellation_token.is_cancelled())
                    {
                        entry.active_turn_id = bounded(Some(task.turn_context.sub_id.clone()));
                        entry.model = bounded(Some(
                            task.turn_context.capture_current_model_info().slug.clone(),
                        ));
                        entry.reasoning_effort = task.turn_context.effective_reasoning_effort();
                    }
                }
                entry.status = status(
                    &child.io.agent_status.borrow(),
                    entry.active_turn_id.is_some(),
                );
                if available(child) {
                    entry.capabilities.send = true;
                    entry.capabilities.interrupt = entry.active_turn_id.is_some();
                    entry.capabilities.close = true;
                }
            } else if !threads.contains_key(&child_id) {
                if self.runtime.registry.is_managed_closed(child_id) {
                    entry.status = ManagedSubagentStatus::Closed;
                }
                entry.capabilities.resume = true;
            }
            agents.push(entry);
        }
        Ok(ManagedSubagentList {
            parent_runtime_id,
            agents,
        })
    }

    pub(super) async fn managed_control(
        &self,
        parent_id: ThreadId,
        expected_parent_runtime_id: Uuid,
        child_id: ThreadId,
        expected_child_runtime_id: Option<Uuid>,
        action: ManagedSubagentAction,
    ) -> Result<ManagedSubagentControlOutcome> {
        use ManagedSubagentControlOutcome as Outcome;
        let Ok(state) = self.runtime.upgrade() else {
            return Ok(Outcome::NotApplied);
        };
        let Ok(observed_parent) = state.get_thread(parent_id).await else {
            return Ok(Outcome::NotApplied);
        };
        if observed_parent.runtime_id() != expected_parent_runtime_id {
            return Ok(Outcome::NotApplied);
        }
        let version = observed_parent.managed_multi_agent_version().await;
        let threads = state.threads.read().await;
        let Some(parent) = threads
            .get(&parent_id)
            .filter(|parent| {
                parent.runtime_id() == expected_parent_runtime_id
                    && available(parent)
                    && same_controller(self, parent)
            })
            .cloned()
        else {
            return Ok(Outcome::NotApplied);
        };
        if parent.multi_agent_version().unwrap_or(version)
            != codex_protocol::protocol::MultiAgentVersion::V2
        {
            return Ok(Outcome::Unsupported);
        }
        let Ok(children) = self.runtime.registry.managed_children(parent_id) else {
            return Ok(Outcome::Unsupported);
        };
        let known = children
            .into_iter()
            .any(|metadata| metadata.agent_id == Some(child_id));
        if !known || child_id == parent_id {
            return Ok(Outcome::NotApplied);
        }
        let child = threads.get(&child_id).cloned();
        if child.as_ref().is_some_and(|child| {
            child.session_source.parent_thread_id() != Some(parent_id)
                || !same_controller(self, child)
        }) || child.as_ref().map(|child| child.runtime_id()) != expected_child_runtime_id
        {
            return Ok(Outcome::NotApplied);
        }
        match action {
            ManagedSubagentAction::Send { input } => {
                let Some(child) = child.filter(|child| available(child)) else {
                    return Ok(Outcome::NotApplied);
                };
                if input.is_empty() {
                    return Ok(Outcome::NotApplied);
                }
                let (reply, receipt) = tokio::sync::oneshot::channel();
                // Queue acceptance is synchronous inside the registry fence. Waiting for
                // Core's routing receipt must not retain that fence over runtime work.
                if enqueue(
                    &child,
                    Op::TurnInput {
                        request: Box::new(TurnInputRequest::user_input(input)),
                        mode: TurnInputMode::StartOrSteer,
                        reply,
                    },
                )
                .is_err()
                {
                    return Ok(Outcome::NotApplied);
                }
                drop(threads);
                match receipt.await {
                    Ok(Ok(
                        TurnInputSubmission::Started { .. } | TurnInputSubmission::Steered { .. },
                    )) => Ok(Outcome::Applied),
                    Ok(Ok(TurnInputSubmission::NotSubmitted { .. })) => Ok(Outcome::NotApplied),
                    Ok(Err(_)) | Err(_) => Ok(Outcome::Unknown),
                }
            }
            ManagedSubagentAction::Interrupt { expected_turn_id } => {
                let Some(child) = child.filter(|child| available(child)) else {
                    return Ok(Outcome::NotApplied);
                };
                let mut active = child.session.active_turn.lock().await;
                let Some(task) =
                    active
                        .as_mut()
                        .and_then(|turn| turn.task.as_mut())
                        .filter(|task| {
                            task.turn_context.sub_id == expected_turn_id
                                && !task.cancellation_token.is_cancelled()
                        })
                else {
                    return Ok(Outcome::NotApplied);
                };
                // Cancellation and identity comparison share the active-turn lock.
                child.session.cancel_exact_task(task);
                child.session.mark_interrupted();
                let turn = active.take();
                drop(active);
                drop(threads);
                if let Some(turn) = turn {
                    let session = Arc::clone(&child.session);
                    tokio::spawn(async move {
                        session.finish_managed_turn_abort(turn).await;
                    });
                }
                Ok(Outcome::Applied)
            }
            ManagedSubagentAction::Close => {
                let Some(_child) = child.filter(|child| available(child)) else {
                    return Ok(Outcome::NotApplied);
                };
                // Capture descendant runtime objects, not reusable thread IDs.
                let mut ids = HashSet::from([child_id]);
                loop {
                    let old_len = ids.len();
                    for (id, thread) in threads.iter() {
                        if thread
                            .session_source
                            .parent_thread_id()
                            .is_some_and(|parent| ids.contains(&parent))
                            && same_controller(self, thread)
                        {
                            ids.insert(*id);
                        }
                    }
                    if ids.len() == old_len {
                        break;
                    }
                }
                let captured = ids
                    .into_iter()
                    .filter_map(|id| threads.get(&id).map(|thread| (id, Arc::clone(thread))))
                    .collect::<Vec<_>>();
                let reservation = uuid::Uuid::new_v4();
                let mut reserved: Vec<Arc<crate::CodexThread>> = Vec::new();
                for (_, thread) in &captured {
                    if !thread.io.admission.reserve_managed_close(reservation) {
                        for thread in reserved {
                            thread.io.admission.rollback_managed_close(reservation);
                        }
                        return Ok(Outcome::NotApplied);
                    }
                    reserved.push(Arc::clone(thread));
                }
                drop(threads);
                self.close_managed_agent(child_id, captured, reservation)
                    .await
            }
            ManagedSubagentAction::Resume => {
                if child.is_some() || expected_child_runtime_id.is_some() {
                    return Ok(Outcome::NotApplied);
                }
                drop(threads);
                let config = parent.session.get_config().await.as_ref().clone();
                // The captured parent is revalidated atomically with child registration.
                if let Err(error) = self
                    .ensure_v2_agent_loaded_for_operator(config, child_id, parent)
                    .await
                {
                    tracing::warn!(%child_id, %error, "managed child resume did not complete");
                    return Ok(if state.threads.read().await.contains_key(&child_id) {
                        Outcome::Unknown
                    } else {
                        Outcome::NotApplied
                    });
                }
                self.runtime.registry.set_managed_closed(child_id, false);
                if let Some(store) = state.agent_graph_store() {
                    store.set_thread_spawn_edge_status(child_id,
                        codex_agent_graph_store::ThreadSpawnEdgeStatus::Open).await
                        .map_err(|error| CodexErr::Fatal(format!("managed child resumed but open status could not be persisted: {error}")))?;
                }
                Ok(Outcome::Applied)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn management_status_discards_completion_and_error_content() {
        assert_eq!(
            status(
                &AgentStatus::Completed(Some("private completion".into())),
                false
            ),
            ManagedSubagentStatus::Completed
        );
        assert_eq!(
            status(&AgentStatus::Errored("private failure".into()), false),
            ManagedSubagentStatus::Errored
        );
        assert_eq!(
            status(&AgentStatus::Interrupted, false),
            ManagedSubagentStatus::Idle
        );
        assert_eq!(
            status(&AgentStatus::Completed(None), true),
            ManagedSubagentStatus::Running
        );
        assert!(bounded(Some("x".repeat(1000))).is_none());
    }
}
