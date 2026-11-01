// Modified for OpenDIY Codex R31 (2026-11-01); see MODIFICATIONS.md.
//! `thread/delete` request handling.

use super::thread_processor::unsupported_thread_store_operation;
use super::*;

impl ThreadRequestProcessor {
    #[cfg(all(test, feature = "test-support"))]
    pub(crate) fn cleanup_services_for_test(
        &self,
    ) -> (
        Arc<dyn ThreadStore>,
        Arc<codex_queue_extension::QueuedItemService>,
    ) {
        (
            Arc::clone(&self.thread_store),
            Arc::clone(self.cleanup_queue_service.as_ref().expect("queue fixture")),
        )
    }

    pub(crate) async fn thread_cleanup_completed(
        &self,
        request_id: ConnectionRequestId,
        params: codex_app_server_protocol::ThreadCleanupCompletedParams,
    ) -> Result<Option<ClientResponsePayload>, JSONRPCErrorError> {
        let parent = ThreadId::from_string(&params.parent_thread_id)
            .map_err(|error| invalid_request(format!("invalid parent thread: {error}")))?;
        let current = ThreadId::from_string(&params.current_thread_id)
            .map_err(|error| invalid_request(format!("invalid current thread: {error}")))?;
        if !params.confirmed && !params.selection.is_empty() {
            return Err(invalid_request(
                "cleanup selection requires explicit confirmation",
            ));
        }
        let selection: Vec<codex_core::CompletedChildSnapshot> = params
            .selection
            .into_iter()
            .map(|item| {
                Ok(codex_core::CompletedChildSnapshot {
                    thread_id: ThreadId::from_string(&item.thread_id).map_err(|error| {
                        invalid_request(format!("invalid selected thread: {error}"))
                    })?,
                    runtime_id: item.runtime_id,
                    activity_revision: item.activity_revision,
                    last_turn_id: item.last_turn_id,
                    terminal_status: item.terminal_status,
                })
            })
            .collect::<Result<_, JSONRPCErrorError>>()?;
        let manager = Arc::clone(&self.thread_manager);
        let permit = Arc::clone(&self.thread_list_state_permit)
            .acquire_owned()
            .await
            .map_err(|error| internal_error(format!("failed to reserve thread list: {error}")))?;
        let outgoing = self.outgoing.clone();
        let processor = self.clone();
        // Keep the metadata permit and notifications with the owner on cancellation.
        let results = tokio::spawn(async move {
            let _permit = permit;
            let mut results = manager.preview_completed_children(parent, current).await?;
            let mut candidates = if params.confirmed {
                selection
            } else {
                results
                    .iter()
                    .filter_map(|result| result.snapshot.clone())
                    .collect()
            };
            candidates.sort_by_key(|item| item.thread_id.to_string());
            if candidates
                .windows(2)
                .any(|pair| pair[0].thread_id == pair[1].thread_id)
            {
                return Err(codex_protocol::error::CodexErr::InvalidRequest(
                    "duplicate cleanup selection".into(),
                ));
            }
            let mut queue_guards = Vec::new();
            let mut goal_resume_guards = Vec::new();
            let mut goal_guards = Vec::new();
            let mut accepted = Vec::new();
            let mut blocked = Vec::new();
            for candidate in candidates {
                let id = candidate.thread_id;
                if id == parent || id == current {
                    blocked.push((id, "protected"));
                    continue;
                }
                let queue_guard = if let Some(service) = &processor.cleanup_queue_service {
                    match service.cleanup_guard(id).await.map_err(|error| {
                        codex_protocol::error::CodexErr::InvalidRequest(error.to_string())
                    })? {
                        Some(guard) => Some(guard),
                        None => {
                            blocked.push((id, "pending_user_queue"));
                            continue;
                        }
                    }
                } else {
                    None
                };
                let goal_resume_guard = processor.thread_state_manager.lock_goal_resume(id).await;
                let Some(goal_guard) = processor
                    .thread_goal_processor
                    .cleanup_guard(id)
                    .await
                    .map_err(|error| {
                        codex_protocol::error::CodexErr::InvalidRequest(error.message)
                    })?
                else {
                    blocked.push((id, "unfinished_goal"));
                    continue;
                };
                queue_guards.push(queue_guard);
                goal_resume_guards.push(goal_resume_guard);
                goal_guards.push(goal_guard);
                accepted.push(candidate);
            }
            if params.confirmed {
                results = manager
                    .cleanup_completed_children(parent, current, accepted)
                    .await?;
                results.extend(blocked.into_iter().map(|(id, reason)| {
                    codex_core::CompletedChildCleanupResult {
                        thread_id: id,
                        outcome: reason.into(),
                        snapshot: None,
                    }
                }));
            } else {
                for (id, reason) in blocked {
                    if let Some(result) = results.iter_mut().find(|result| result.thread_id == id) {
                        result.outcome = reason.into();
                        result.snapshot = None;
                    }
                }
            }
            // These per-thread service fences outlive Core's writer shutdown and storage deletion.
            drop((queue_guards, goal_resume_guards, goal_guards));
            for result in &results {
                if result.outcome == "deleted" {
                    outgoing
                        .send_server_notification(ServerNotification::ThreadDeleted(
                            ThreadDeletedNotification {
                                thread_id: result.thread_id.to_string(),
                            },
                        ))
                        .await;
                }
            }
            Ok::<_, codex_protocol::error::CodexErr>(results)
        })
        .await
        .map_err(|error| internal_error(format!("cleanup owner failed: {error}")))?
        .map_err(|error| invalid_request(format!("cleanup could not be confirmed: {error}")))?;
        let response = codex_app_server_protocol::ThreadCleanupCompletedResponse {
            results: results
                .into_iter()
                .map(
                    |item| codex_app_server_protocol::CompletedChildCleanupResult {
                        thread_id: item.thread_id.to_string(),
                        outcome: item.outcome,
                        snapshot: item.snapshot.map(|item| {
                            codex_app_server_protocol::CompletedChildSnapshot {
                                thread_id: item.thread_id.to_string(),
                                runtime_id: item.runtime_id,
                                activity_revision: item.activity_revision,
                                last_turn_id: item.last_turn_id,
                                terminal_status: item.terminal_status,
                            }
                        }),
                    },
                )
                .collect(),
        };
        self.outgoing.send_response(request_id, response).await;
        Ok(None)
    }

    pub(crate) async fn thread_delete(
        &self,
        request_id: ConnectionRequestId,
        params: ThreadDeleteParams,
    ) -> Result<Option<ClientResponsePayload>, JSONRPCErrorError> {
        let mut deleted_thread_ids = Vec::new();
        let result = {
            let _thread_list_state_permit = self.acquire_thread_list_state_permit().await?;
            self.thread_delete_response(params, &mut deleted_thread_ids)
                .await
        };
        match result {
            Ok(response) => {
                self.outgoing
                    .send_response(request_id.clone(), response)
                    .await;
                self.send_thread_deleted_notifications(deleted_thread_ids)
                    .await;
                Ok(None)
            }
            Err(error) => Err(error),
        }
    }

    async fn thread_delete_response(
        &self,
        params: ThreadDeleteParams,
        deleted_thread_ids: &mut Vec<String>,
    ) -> Result<ThreadDeleteResponse, JSONRPCErrorError> {
        let thread_id = ThreadId::from_string(&params.thread_id)
            .map_err(|err| invalid_request(format!("invalid thread id: {err}")))?;

        let thread_ids = self.state_db_spawn_subtree_thread_ids(thread_id).await?;

        self.validate_root_thread_delete(thread_id, thread_ids.len() > 1)
            .await?;
        if let Some(store) = self
            .thread_store
            .as_any()
            .downcast_ref::<LocalThreadStore>()
        {
            store
                .ensure_no_attached_threads(&thread_ids)
                .await
                .map_err(thread_store_delete_error)?;
        }
        for thread_id_to_delete in thread_ids.iter().copied() {
            self.prepare_thread_for_delete(thread_id_to_delete).await?;
        }

        let mut delete_order: Vec<_> = thread_ids.iter().skip(1).rev().copied().collect();
        delete_order.push(thread_id);

        self.thread_store
            .delete_threads(StoreDeleteThreadsParams {
                thread_ids: delete_order.clone(),
            })
            .await
            .map_err(thread_store_delete_error)?;

        deleted_thread_ids.extend(
            delete_order
                .into_iter()
                .map(|thread_id| thread_id.to_string()),
        );
        Ok(ThreadDeleteResponse {})
    }

    async fn send_thread_deleted_notifications(&self, deleted_thread_ids: Vec<String>) {
        for thread_id in deleted_thread_ids {
            self.outgoing
                .send_server_notification(ServerNotification::ThreadDeleted(
                    ThreadDeletedNotification { thread_id },
                ))
                .await;
        }
    }

    async fn validate_root_thread_delete(
        &self,
        thread_id: ThreadId,
        has_descendants: bool,
    ) -> Result<(), JSONRPCErrorError> {
        if let Ok(thread) = self.thread_manager.get_thread(thread_id).await {
            if !thread.config_snapshot().await.ephemeral {
                return Ok(());
            }
            return Err(invalid_request(format!(
                "thread is not persisted and cannot be deleted: {thread_id}"
            )));
        }
        match self
            .thread_store
            .read_thread(StoreReadThreadParams {
                thread_id,
                include_archived: true,
                include_history: false,
            })
            .await
        {
            Ok(_) => Ok(()),
            Err(ThreadStoreError::ThreadNotFound { .. }) => {
                if has_descendants {
                    return Ok(());
                }
                let Some(state_db) = self.state_db.as_ref() else {
                    return Err(thread_store_delete_error(
                        ThreadStoreError::ThreadNotFound { thread_id },
                    ));
                };
                if state_db
                    .get_thread(thread_id)
                    .await
                    .map_err(|err| {
                        internal_error(format!(
                            "failed to read app-server state for {thread_id}: {err}"
                        ))
                    })?
                    .is_some()
                {
                    Ok(())
                } else {
                    Err(thread_store_delete_error(
                        ThreadStoreError::ThreadNotFound { thread_id },
                    ))
                }
            }
            Err(err) => Err(thread_store_delete_error(err)),
        }
    }

    async fn prepare_thread_for_delete(
        &self,
        thread_id: ThreadId,
    ) -> Result<(), JSONRPCErrorError> {
        self.prepare_thread_for_removal(thread_id, "delete").await?;
        if let Some(log_db) = self.log_db.as_ref() {
            log_db.flush().await;
        }
        Ok(())
    }
}

fn thread_store_delete_error(err: ThreadStoreError) -> JSONRPCErrorError {
    match err {
        ThreadStoreError::ThreadNotFound { thread_id } => {
            invalid_request(format!("thread not found: {thread_id}"))
        }
        ThreadStoreError::InvalidRequest { message } | ThreadStoreError::Conflict { message } => {
            invalid_request(message)
        }
        ThreadStoreError::Unsupported { operation } => {
            unsupported_thread_store_operation(operation)
        }
        err => internal_error(format!("failed to delete thread: {err}")),
    }
}
