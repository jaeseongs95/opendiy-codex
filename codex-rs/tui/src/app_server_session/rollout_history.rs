// Modified for OpenDIY Codex R31 (2026-11-01); see MODIFICATIONS.md.
//! Resume and read-only history loading across rollout formats.

use super::AppServerSession;
use super::AppServerStartedThread;
use super::HistoryHydrationScope;
use super::ResumeModelSettings;
use super::ThreadHistorySupport;
use super::ThreadSettingsOwner;
use super::bootstrap_request_error;
use super::is_history_pagination_unsupported;
use super::started_thread_from_resume_response;
use super::thread_resume_params_from_config;
use super::thread_session_state_from_thread_response;
use crate::legacy_core::config::Config;
use codex_app_server_client::TypedRequestError;
use codex_app_server_protocol::AskForApproval;
use codex_app_server_protocol::ClientRequest;
use codex_app_server_protocol::SortDirection;
use codex_app_server_protocol::ThreadHistoryMode;
use codex_app_server_protocol::ThreadResumeResponse;
use codex_app_server_protocol::ThreadTurnsListParams;
use codex_app_server_protocol::ThreadTurnsListResponse;
use codex_app_server_protocol::TurnItemsView;
use codex_protocol::ThreadId;
use codex_utils_absolute_path::AbsolutePathBuf;
use color_eyre::eyre::Result;

// Bound recovery to recent messages when item paging is unavailable.
const READ_ONLY_HISTORY_TURN_LIMIT: u32 = 100;

impl AppServerSession {
    pub(crate) async fn invalidate_runtime_tool_registration(&mut self) {
        self.runtime_tool_registration_generation =
            self.runtime_tool_registration_generation.wrapping_add(1);
        self.runtime_tool_registration_lifetime.cancel();
        self.runtime_tool_registration_lifetime = tokio_util::sync::CancellationToken::new();
        *self.pending_runtime_tool_resume.lock().await = None;
    }
    #[cfg(test)]
    pub(crate) async fn remember_runtime_tools_busy_for_test(&self, thread: ThreadId) {
        *self.pending_runtime_tool_resume.lock().await = Some((
            self.runtime_tool_registration_generation,
            self.connection_id,
            codex_app_server_protocol::ThreadResumeParams {
                thread_id: thread.to_string(),
                exclude_turns: true,
                runtime_tools_only: true,
                expected_runtime_id: Some(uuid::Uuid::new_v4().to_string()),
                runtime_dynamic_tools: Some(crate::cron_tools::specs()),
                previous_runtime_dynamic_tools: crate::cron_tools::previous_specs(),
                ..Default::default()
            },
        ));
    }

    /// A single retry is allowed only after an explicit busy ACK and a new idle
    /// notification for the same selected thread. This never hydrates history or sends input.
    pub(crate) async fn retry_runtime_tools_on_idle(
        &self,
        idle_thread: &str,
        selected: Option<ThreadId>,
        selection: Option<tokio_util::sync::CancellationToken>,
    ) {
        let mut pending = self.pending_runtime_tool_resume.lock().await;
        let Some((generation, connection, params)) = pending.as_ref() else {
            return;
        };
        if params.thread_id != idle_thread {
            return;
        }
        if *generation != self.runtime_tool_registration_generation
            || *connection != self.connection_id
            || selected.is_none_or(|id| id.to_string() != idle_thread)
            || !self
                .connection_live
                .load(std::sync::atomic::Ordering::Acquire)
        {
            *pending = None;
            return;
        }
        let (_, _, params) = pending.take().expect("checked pending runtime supplement");
        drop(pending);
        let Some(selection) = selection else {
            return;
        };
        let client = self.client.request_handle();
        let connection_live = self.connection_live.clone();
        let registration = self.runtime_tool_registration_lifetime.clone();
        tokio::spawn(async move {
            if registration.is_cancelled()
                || selection.is_cancelled()
                || !connection_live.load(std::sync::atomic::Ordering::Acquire)
            {
                return;
            }
            // No retry on timeout, response loss, unsupported server, or a second busy result.
            let request = tokio::time::timeout(
                std::time::Duration::from_secs(3),
                client.request_typed::<ThreadResumeResponse>(ClientRequest::ThreadResume {
                    request_id: codex_app_server_protocol::RequestId::String(
                        uuid::Uuid::new_v4().to_string(),
                    ),
                    params,
                }),
            );
            let result = tokio::select! {
                biased;
                _ = registration.cancelled() => return,
                _ = selection.cancelled() => return,
                result = request => result,
            };
            if !matches!(result, Ok(Ok(ref response)) if response.runtime_dynamic_tools_applied == Some(true))
            {
                tracing::warn!(
                    "연결 한정 도구의 유휴 보강을 확인하지 못했습니다. 자동 재전송하지 않습니다."
                );
            }
        });
    }

    /// Read a conflicting thread without taking its writer lease. This is a snapshot, not an
    /// attachment: the caller must not route turns through this session.
    pub(crate) async fn read_thread_for_viewing(
        &mut self,
        config: &Config,
        local_settings: &crate::local_settings::LocalSettings,
        thread_id: ThreadId,
    ) -> Result<(AppServerStartedThread, Option<&'static str>)> {
        self.invalidate_runtime_tool_registration().await;
        let mut thread = self.thread_read(thread_id, /*include_turns*/ false).await?;
        let mut history_notice = None;
        if let Err(error) = self
            .hydrate_initial_thread_history(
                &mut thread,
                /*turn_cursor*/ None,
                /*item_cursor*/ None,
                Some(config),
                Some(local_settings),
                HistoryHydrationScope::Initial,
            )
            .await
        {
            if thread.history_mode == ThreadHistoryMode::Legacy {
                return Err(error);
            }
            // Summary pages retain user messages and final replies without scanning
            // every tool item when the item-paging endpoint is unavailable.
            tracing::warn!("Failed to page read-only thread history; trying summary pages");
            let request_id = self.next_request_id();
            let page: ThreadTurnsListResponse = self
                .client
                .request_typed(ClientRequest::ThreadTurnsList {
                    request_id,
                    params: ThreadTurnsListParams {
                        thread_id: thread_id.to_string(),
                        cursor: None,
                        limit: Some(READ_ONLY_HISTORY_TURN_LIMIT),
                        sort_direction: Some(SortDirection::Desc),
                        items_view: Some(TurnItemsView::Summary),
                    },
                })
                .await?;
            thread.turns = page.data.into_iter().rev().collect();
            self.history_pagination.remove(&thread_id);
            history_notice = Some(
                "Showing up to 100 recent prompts and final replies. Intermediate messages and tool activity are unavailable.",
            );
        }
        let session = thread_session_state_from_thread_response(
            &thread.id,
            crate::windows_sandbox::host_from_environments(thread.environments.as_deref()),
            thread.forked_from_id.clone(),
            thread.name.clone(),
            thread.path.clone(),
            thread.model.clone().unwrap_or_default(),
            thread.model_provider.clone(),
            config.service_tier.clone(),
            AskForApproval::from(config.permissions.approval_policy.value()),
            config.approvals_reviewer,
            config.permissions.permission_profile().clone(),
            config.permissions.active_permission_profile(),
            thread.cwd.clone(),
            config.workspace_roots.clone(),
            Vec::new(),
            thread.reasoning_effort,
            local_settings,
        )
        .await
        .map_err(color_eyre::eyre::Report::msg)?;
        Ok((
            AppServerStartedThread {
                session,
                turns: thread.turns,
                blocks_direct_input: false,
                task_tools_available: false,
            },
            history_notice,
        ))
    }

    pub(crate) fn with_local_codex_home(mut self, codex_home: &AbsolutePathBuf) -> Self {
        self.task_tool_capabilities_dir = (!self.uses_embedded_app_server())
            .then(|| codex_home.join("tui-thread-reference-capabilities"));
        self
    }

    /// Retains trusted embedded-server history metadata for a later resume.
    ///
    /// Remote hints are ignored because another process can migrate the thread before resume.
    pub(crate) fn remember_thread_history_mode(
        &mut self,
        thread_id: ThreadId,
        history_mode: ThreadHistoryMode,
    ) {
        if !self.uses_embedded_app_server() {
            return;
        }
        self.history_pagination
            .entry(thread_id)
            .or_default()
            .history_mode = history_mode;
    }

    pub(crate) async fn resume_thread(
        &mut self,
        local_settings: &crate::local_settings::LocalSettings,
        config: Config,
        thread_id: ThreadId,
        model_settings: ResumeModelSettings,
    ) -> Result<AppServerStartedThread> {
        Box::pin(self.resume_thread_with_permission_overrides(
            local_settings,
            config,
            thread_id,
            model_settings,
            crate::resume_permissions::ResumePermissions::default(),
        ))
        .await
    }

    pub(crate) async fn resume_thread_with_permission_overrides(
        &mut self,
        local_settings: &crate::local_settings::LocalSettings,
        config: Config,
        thread_id: ThreadId,
        model_settings: ResumeModelSettings,
        permission_overrides: crate::resume_permissions::ResumePermissions,
    ) -> Result<AppServerStartedThread> {
        self.invalidate_runtime_tool_registration().await;
        if self.thread_params_mode().settings_owner() == ThreadSettingsOwner::Client {
            self.thread_tool_transport().validate_config(&config)?;
        }
        let session_config = if matches!(
            model_settings,
            ResumeModelSettings::RestoreFromThread | ResumeModelSettings::PreserveExistingThread
        ) {
            config.clone()
        } else {
            self.session_config_with_effective_service_tier(&config)
        };
        let mut params = thread_resume_params_from_config(
            session_config,
            thread_id,
            self.thread_params_mode(),
            self.remote_cwd_override.as_deref(),
            model_settings,
            permission_overrides,
        );
        if self.thread_params_mode().settings_owner() == ThreadSettingsOwner::Client
            && model_settings == ResumeModelSettings::OverrideFromCurrentConfig
        {
            params.model_provider = self.explicit_model_provider(&config);
        }
        if self.thread_params_mode().settings_owner() == ThreadSettingsOwner::Client {
            self.thread_tool_transport().configure_resume(&mut params);
        }
        let mut rollout_maintenance_guard = None;
        params.exclude_turns = if self.history_support == ThreadHistorySupport::Paginated {
            let known_legacy_history = self
                .history_pagination
                .get(&thread_id)
                .is_some_and(|state| state.history_mode == ThreadHistoryMode::Legacy)
                && (!self.uses_embedded_app_server()
                    || ({
                        // The guard prevents migration through the full resume,
                        // regardless of the server's migration feature settings.
                        rollout_maintenance_guard =
                            codex_rollout::try_acquire_rollout_maintenance_lock(
                                config.codex_home.as_path(),
                            )
                            .ok()
                            .flatten();
                        rollout_maintenance_guard.is_some()
                    } && self
                        .thread_read(thread_id, /*include_turns*/ false)
                        .await
                        .is_ok_and(|thread| thread.history_mode == ThreadHistoryMode::Legacy)));
            !known_legacy_history
        } else {
            false
        };
        if params.exclude_turns {
            rollout_maintenance_guard = None;
        }
        let request_id = self.next_request_id();
        let resume_response = self
            .client
            .request_typed(ClientRequest::ThreadResume {
                request_id,
                params: params.clone(),
            })
            .await;
        drop(rollout_maintenance_guard);
        let mut response: ThreadResumeResponse = match resume_response {
            Ok(response) => response,
            Err(TypedRequestError::Server { source, .. })
                if params.exclude_turns && is_history_pagination_unsupported(&source) =>
            {
                self.history_support = ThreadHistorySupport::LegacyOnly;
                params.exclude_turns = false;
                let request_id = self.next_request_id();
                self.client
                    .request_typed(ClientRequest::ThreadResume {
                        request_id,
                        params: params.clone(),
                    })
                    .await
                    .map_err(|err| {
                        bootstrap_request_error("thread/resume failed during TUI bootstrap", err)
                    })?
            }
            Err(err) => {
                return Err(bootstrap_request_error(
                    "thread/resume failed during TUI bootstrap",
                    err,
                ));
            }
        };
        if params.runtime_dynamic_tools.is_some() {
            match (
                response.runtime_dynamic_tools_applied,
                response.runtime_dynamic_tools_runtime_id.as_deref(),
            ) {
                (Some(false), Some(runtime_id)) => {
                    *self.pending_runtime_tool_resume.lock().await = Some((
                        self.runtime_tool_registration_generation,
                        self.connection_id,
                        codex_app_server_protocol::ThreadResumeParams {
                            thread_id: thread_id.to_string(),
                            exclude_turns: true,
                            runtime_tools_only: true,
                            expected_runtime_id: Some(runtime_id.to_string()),
                            runtime_dynamic_tools: params.runtime_dynamic_tools.clone(),
                            previous_runtime_dynamic_tools: params
                                .previous_runtime_dynamic_tools
                                .clone(),
                            ..Default::default()
                        },
                    ));
                }
                (Some(true), Some(_)) => {}
                _ => {
                    tracing::warn!("서버가 연결 한정 도구 보강과 실행 세대를 확인하지 않았습니다.")
                }
            }
        }
        if let Err(error) = self
            .hydrate_initial_thread_history(
                &mut response.thread,
                response.turns_backwards_cursor.clone(),
                response.items_backwards_cursor.clone(),
                Some(&config),
                Some(local_settings),
                HistoryHydrationScope::Initial,
            )
            .await
        {
            if !super::history::is_unmaterialized_history_error(&error) {
                return Err(error);
            }
            // Resume succeeded. Keep its original session and live subscription while
            // explicitly retaining that history has not been queried successfully.
            self.mark_initial_history_unavailable(thread_id, response.thread.history_mode, &error);
        }
        let fork_parent_title = self
            .fork_parent_title_from_app_server(response.thread.forked_from_id.as_deref())
            .await;
        let mut started = started_thread_from_resume_response(
            response,
            local_settings,
            self.thread_params_mode(),
        )
        .await?;
        started.session.fork_parent_title = fork_parent_title;
        if self.task_tools_available(thread_id) {
            self.remember_task_tool_thread(thread_id);
            started.task_tools_available = true;
        }
        Ok(started)
    }
}

#[cfg(test)]
#[path = "rollout_history_tests.rs"]
mod tests;
