// Modified for OpenDIY Codex R32 (2026-11-01); see MODIFICATIONS.md.
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
        reservation: uuid::Uuid,
    ) -> CodexResult<crate::ManagedSubagentControlOutcome> {
        let control = self.clone();
        // Accepted close remains owned if the host loses its response waiter.
        tokio::spawn(async move {
            let rollback = || {
                for (_, thread) in &captured {
                    thread.io.admission.rollback_managed_close(reservation);
                }
            };
            let Ok(state) = control.runtime.upgrade() else {
                rollback();
                return Ok(crate::ManagedSubagentControlOutcome::NotApplied);
            };
            // No close status or shutdown is committed until every captured runtime is durable.
            for (id, thread) in &captured {
                thread.ensure_rollout_materialized().await;
                let flush = managed_close_flush(thread).await;
                if let Err(error) = flush {
                    tracing::warn!(thread_id = %id, %error, "managed subagent flush failed before close");
                    rollback();
                    return Ok(crate::ManagedSubagentControlOutcome::NotApplied);
                }
            }
            let threads = state.threads.read().await;
            if captured.iter().any(|(id, thread)| threads.get(id).is_none_or(|current| !Arc::ptr_eq(current, thread))) {
                rollback();
                return Ok(crate::ManagedSubagentControlOutcome::NotApplied);
            }
            for (id, thread) in &captured {
                thread.io.admission.close(); // Commit consumes our rollback ownership.
                control.runtime.registry.set_managed_closed(*id, true);
            }
            drop(threads);
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
                if !thread.io.admission.shutdown_confirmed() {
                    tracing::warn!(thread_id = %id, "managed subagent shutdown remains unconfirmed");
                    failed = true;
                    continue;
                }
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
        let thread = state.get_thread(agent_id).await?;
        if thread.is_running() {
            thread
                .session
                .ensure_rollout_materialized(PersistContext::Standard)
                .await;
            thread.session.flush_rollout().await?;
        }
        let result = if !thread.is_running()
            || matches!(thread.agent_status().await, AgentStatus::Shutdown)
        {
            Ok(String::new())
        } else {
            // Do not look up the ID again after persistence I/O: it may now name a successor.
            state.submit_shutdown_to_thread(&thread).await
        };
        thread.wait_until_terminated().await;
        state
            .remove_stopped_thread_if_matches(&thread, || {
                self.forget_v2_residency(agent_id);
                self.runtime.registry.release_spawned_thread(agent_id);
                self.runtime.mailboxes.remove(agent_id);
            })
            .await?;
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

async fn managed_close_flush(thread: &crate::CodexThread) -> std::io::Result<()> {
    #[cfg(test)]
    {
        let hook = thread
            .io
            .admission
            .managed_close_flush_hook
            .lock()
            .unwrap()
            .take();
        if let Some((entered, release)) = hook {
            entered.notify_one();
            release.notified().await;
            return Err(std::io::Error::other(
                "injected managed close flush failure",
            ));
        }
    }
    thread.session.flush_rollout().await
}

#[cfg(test)]
mod tests {
    use super::*;
    use core_test_support::PathBufExt;
    use futures::FutureExt;

    #[tokio::test]
    async fn legacy_shutdown_preserves_failed_cleanup_and_a_replacement_handle() {
        let home = tempfile::tempdir().expect("tempdir");
        let mut config = crate::config::test_config().await;
        config.codex_home = home.path().to_path_buf().abs();
        config.cwd = config.codex_home.clone();
        let manager = crate::ThreadManager::with_models_provider_and_home_for_tests(
            codex_login::CodexAuth::from_api_key("dummy"),
            config.model_provider.clone(),
            config.codex_home.to_path_buf(),
            Arc::new(codex_exec_server::EnvironmentManager::default_for_tests()),
        );
        let control = manager.agent_control();
        let state = control.runtime.upgrade().expect("manager state");
        let source = manager
            .start_thread(crate::StartThreadOptions::new(config))
            .await
            .expect("start source");
        let make_thread = |io| {
            Arc::new(crate::CodexThread::new(
                Arc::clone(&source.thread.session),
                io,
                (&source.session_configured).into(),
                source.thread.rollout_path(),
                source.thread.session_source.clone(),
            ))
        };
        let mut io = source.thread.io.clone();
        io.session_loop_termination = std::future::ready(Err(Arc::new(
            crate::session::SessionLoopFailure::CleanupUnconfirmed,
        )))
        .boxed()
        .shared();
        let failed = make_thread(io);
        state
            .threads
            .write()
            .await
            .insert(source.thread_id, Arc::clone(&failed));
        let error = control
            .shutdown_live_agent(source.thread_id)
            .await
            .expect_err("lost cleanup confirmation cannot remove the handle");
        assert!(error.to_string().contains("cleanup unconfirmed"));
        assert!(Arc::ptr_eq(
            &manager
                .get_thread(source.thread_id)
                .await
                .expect("failed cleanup is retained"),
            &failed
        ));
        source
            .thread
            .shutdown_and_wait()
            .await
            .expect("finish real source cleanup");

        let (entered_tx, entered_rx) = tokio::sync::oneshot::channel();
        let (release_tx, release_rx) = tokio::sync::oneshot::channel();
        let mut io = source.thread.io.clone();
        io.session_loop_termination = async move {
            let _ = entered_tx.send(());
            release_rx.await.expect("release old cleanup");
            Ok(())
        }
        .boxed()
        .shared();
        let old = make_thread(io);
        state.threads.write().await.insert(source.thread_id, old);
        let (result, ()) = tokio::join!(control.shutdown_live_agent(source.thread_id), async {
            entered_rx
                .await
                .expect("shutdown waits for the captured runtime");
            state
                .threads
                .write()
                .await
                .insert(source.thread_id, Arc::clone(&source.thread));
            release_tx.send(()).expect("complete old cleanup");
        });
        result.expect("old cleanup confirmed");
        assert!(Arc::ptr_eq(
            &manager
                .get_thread(source.thread_id)
                .await
                .expect("replacement remains registered"),
            &source.thread
        ));
        control
            .shutdown_live_agent(source.thread_id)
            .await
            .expect("normal confirmed shutdown can remove its own handle");
        assert!(manager.get_thread(source.thread_id).await.is_err());
    }
}
