use super::*;
use crate::agent::api::AgentInfo;
use codex_protocol::error::CodexErrorDetails;
use codex_thread_store::PersistContext;

impl LocalAgentControl {
    /// Operator close owns the exact runtimes captured under the manager registry fence.
    /// Persistence and descendant shutdown match close_agent; delayed cleanup never follows IDs.
    pub(super) async fn close_managed_agent(
        &self,
        agent_id: ThreadId,
        captured: Vec<(ThreadId, Arc<crate::CodexThread>)>,
    ) -> CodexResult<crate::ManagedSubagentControlOutcome> {
        let control = self.clone();
        // Accepted close remains owned if the host loses its response waiter.
        tokio::spawn(async move {
            let state = control.runtime.upgrade()?;
            let mut failed = false;
            if let Some(store) = state.agent_graph_store()
                && let Err(error) = store
                    .set_thread_spawn_edge_status(
                        agent_id,
                        codex_agent_graph_store::ThreadSpawnEdgeStatus::Closed,
                    )
                    .await
            {
                tracing::warn!(%agent_id, %error, "failed to persist managed subagent close");
                failed = true;
            }
            for (id, thread) in captured {
                thread.ensure_rollout_materialized().await;
                if let Err(error) = thread.session.flush_rollout().await {
                    tracing::warn!(thread_id = %id, %error, "managed subagent flush failed");
                    failed = true;
                    continue;
                }
                // Admission was closed under the registry fence. Do not wait for
                // residency pins: queued mailbox entries release them during shutdown.
                if thread.is_running()
                    && let Err(error) = thread
                        .io
                        .tx_sub
                        .send(crate::session::Submission {
                            id: crate::session::new_submission_id(),
                            op: Op::Shutdown,
                            trace: None,
                            turn_extension_init: None,
                            parent_turn_id: None,
                            root_turn_id: None,
                            residency_guard: None,
                            message_delivery: None,
                        })
                        .await
                    && thread.is_running()
                {
                    tracing::warn!(thread_id = %id, %error, "managed subagent shutdown failed");
                    failed = true;
                    continue;
                }
                thread.wait_until_terminated().await;
                let mut threads = state.threads.write().await;
                if threads
                    .get(&id)
                    .is_some_and(|current| Arc::ptr_eq(current, &thread))
                {
                    threads.remove(&id);
                    thread.session.services.mcp_runtime.revoke_ema();
                    control.forget_v2_residency(id);
                    // Keep metadata so the operator can inspect and resume the closed child.
                    control.runtime.mailboxes.remove(id);
                }
            }
            Ok(if failed {
                crate::ManagedSubagentControlOutcome::Unknown
            } else {
                crate::ManagedSubagentControlOutcome::Applied
            })
        })
        .await?
    }

    /// Submit a shutdown request for a live agent without marking it explicitly closed in
    /// persisted spawn-edge state.
    pub(crate) async fn shutdown_live_agent(&self, agent_id: ThreadId) -> CodexResult<String> {
        let state = self.runtime.upgrade()?;
        let result = if let Ok(thread) = state.get_thread(agent_id).await {
            thread
                .session
                .ensure_rollout_materialized(PersistContext::Standard)
                .await;
            thread.session.flush_rollout().await?;
            let result = if matches!(thread.agent_status().await, AgentStatus::Shutdown) {
                Ok(String::new())
            } else {
                state
                    .send_op(
                        agent_id,
                        Op::Shutdown {},
                        /*parent_turn_id*/ None,
                        /*root_turn_id*/ None,
                        /*message_delivery*/ None,
                    )
                    .await
            };
            thread.wait_until_terminated().await;
            result
        } else {
            state
                .send_op(
                    agent_id,
                    Op::Shutdown {},
                    /*parent_turn_id*/ None,
                    /*root_turn_id*/ None,
                    /*message_delivery*/ None,
                )
                .await
        };
        let _ = state.remove_thread(&agent_id).await;
        self.forget_v2_residency(agent_id);
        self.runtime.registry.release_spawned_thread(agent_id);
        self.runtime.mailboxes.remove(agent_id);
        result
    }

    /// Mark `agent_id` as explicitly closed in persisted spawn-edge state, then shut down the
    /// agent and any live descendants reached from the in-memory tree.
    pub(crate) async fn close_agent(&self, agent_id: ThreadId) -> CodexResult<AgentInfo> {
        let state = self.runtime.upgrade()?;
        let metadata = self.get_agent_metadata(agent_id);
        let known_agent = metadata.is_some();
        let snapshot = match state.get_thread(agent_id).await {
            Ok(thread) => {
                let agent = LiveAgent {
                    thread_id: agent_id,
                    metadata: metadata.unwrap_or_default(),
                    status: thread.agent_status().await,
                };
                let config = Box::new(thread.config_snapshot().await);
                if !config.ephemeral
                    && let Some(agent_graph_store) = state.agent_graph_store()
                    && let Err(err) = agent_graph_store
                        .set_thread_spawn_edge_status(
                            agent_id,
                            codex_agent_graph_store::ThreadSpawnEdgeStatus::Closed,
                        )
                        .await
                {
                    warn!("failed to persist thread-spawn edge status for {agent_id}: {err}");
                }
                AgentInfo::Loaded { agent, config }
            }
            Err(err)
                if known_agent && matches!(err.details(), CodexErrorDetails::ThreadNotFound(_)) =>
            {
                if let Some(agent_graph_store) = state.agent_graph_store()
                    && let Err(err) = agent_graph_store
                        .set_thread_spawn_edge_status(
                            agent_id,
                            codex_agent_graph_store::ThreadSpawnEdgeStatus::Closed,
                        )
                        .await
                {
                    return Err(CodexErr::Fatal(format!(
                        "failed to persist stale thread-spawn edge status for {agent_id}: {err}"
                    )));
                }
                AgentInfo::Unloaded(metadata.unwrap_or_default())
            }
            Err(err) => return Err(err),
        };
        match Box::pin(self.shutdown_agent_tree(agent_id)).await {
            Err(err)
                if known_agent
                    && matches!(
                        err.details(),
                        CodexErrorDetails::ThreadNotFound(_) | CodexErrorDetails::InternalAgentDied
                    ) =>
            {
                Ok(snapshot)
            }
            result => result.map(|_| snapshot),
        }
    }

    /// Shut down `agent_id` and any live descendants reachable from the in-memory spawn tree.
    pub(crate) async fn shutdown_agent_tree(&self, agent_id: ThreadId) -> CodexResult<String> {
        let descendant_ids = self.runtime.live_thread_spawn_descendants(agent_id).await?;
        let result = self.shutdown_live_agent(agent_id).await;
        for descendant_id in descendant_ids {
            match self.shutdown_live_agent(descendant_id).await {
                Ok(_) => {}
                Err(err)
                    if matches!(
                        err.details(),
                        CodexErrorDetails::ThreadNotFound(_) | CodexErrorDetails::InternalAgentDied
                    ) => {}
                Err(err) => return Err(err),
            }
        }
        result
    }
}
