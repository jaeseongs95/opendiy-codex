// Modified for OpenDIY Codex R32 (2026-11-01); see MODIFICATIONS.md.
//! Handles persistent thread-settings updates and serializes their persistence
//! with checkpoints written directly to storage.

use super::session::Session;
use super::session::SessionSettingsUpdate;
use super::step_settings::StepSettingsUpdate;
use crate::WithTurnExtensionData;
use crate::config::ConstraintResult;
use codex_history::RolloutItem;
use codex_protocol::capabilities::SelectedCapabilityRoot;
use codex_protocol::error::CodexErr;
use codex_protocol::error::Result as CodexResult;
use codex_protocol::protocol::Event;
use codex_protocol::protocol::EventMsg;
use codex_protocol::protocol::ThreadSettingsAppliedEvent;
use codex_protocol::protocol::ThreadSettingsOverrides;
use codex_protocol::protocol::ThreadSettingsSnapshot;
use codex_thread_store::PersistContext;
use codex_thread_store::ThreadStoreError;
use codex_thread_store::ThreadStoreResult;
use std::sync::Arc;
use tokio::sync::SemaphorePermit;

impl Session {
    /// Runtime only: no store writes. Admission and publication share the active-turn lock.
    pub(crate) async fn supplement_runtime_dynamic_tools(
        &self,
        owner: u64,
        disconnected: tokio_util::sync::CancellationToken,
        tools: Vec<codex_protocol::dynamic_tools::DynamicToolSpec>,
        previous: &[codex_protocol::dynamic_tools::DynamicToolSpec],
    ) -> CodexResult<bool> {
        // No persistence is touched here. Holding its permit while waiting for an
        // active turn can deadlock a user reply behind a delivered-message record.
        let active = self.active_turn.lock().await;
        if active.is_some() {
            return Ok(false);
        }
        let mut state = self.state.lock().await;
        if state.shutting_down || self.cleanup_admission.is_closing() || disconnected.is_cancelled()
        {
            return Err(CodexErr::InvalidRequest(
                "도구를 제공한 연결이 종료됐습니다.".into(),
            ));
        }
        if state
            .runtime_dynamic_tools
            .as_ref()
            .is_some_and(|overlay| !overlay.disconnected.is_cancelled() && overlay.owner != owner)
        {
            return Err(CodexErr::InvalidRequest(
                "다른 연결이 이 대화의 도구 보강을 소유하고 있습니다.".into(),
            ));
        }
        let (merged, keys) =
            merge_runtime_tools(&state.session_configuration.dynamic_tools, &tools, previous)
                .map_err(CodexErr::InvalidRequest)?;
        state.set_runtime_dynamic_tools(owner, disconnected, Arc::from(merged), keys);
        Ok(true)
    }

    pub(crate) async fn runtime_dynamic_tool_owner(
        &self,
        namespace: Option<&str>,
        name: &str,
    ) -> Option<(u64, tokio_util::sync::CancellationToken)> {
        let state = self.state.lock().await;
        let overlay = state.runtime_dynamic_tools.as_ref()?;
        overlay
            .keys
            .iter()
            .any(|(ns, tool)| ns.as_deref() == namespace && tool == name)
            .then(|| (overlay.owner, overlay.disconnected.clone()))
    }

    /// Persist before publishing so a failed write cannot change the running catalog.
    pub(crate) async fn replace_dynamic_tools(
        &self,
        tools: Vec<codex_protocol::dynamic_tools::DynamicToolSpec>,
    ) -> CodexResult<()> {
        let active = self.active_turn.lock().await;
        // User replies hold active_turn while reserving communication order;
        // delivered-message records then acquire persistence. Keep that order.
        let _settings_guard = acquire_persistence_lock(self).await;
        if active.is_some() && self.state.lock().await.runtime_dynamic_tools.is_some() {
            return Err(CodexErr::InvalidRequest(
                "연결 한정 도구를 사용하는 활성 턴의 도구 목록은 교체할 수 없습니다.".into(),
            ));
        }
        if let Some(live_thread) = self.live_thread() {
            let config = self.get_config().await;
            if !config.allow_persistent_dynamic_tool_updates {
                return Err(CodexErr::UnsupportedOperation(
                    "The host has disabled dynamic tool updates for persistent threads.".into(),
                ));
            }
            let persistence_error = |error: ThreadStoreError| match error {
                ThreadStoreError::Unsupported { .. } => {
                    CodexErr::UnsupportedOperation(error.to_string())
                }
                _ => CodexErr::Fatal(format!("failed to replace dynamic tools: {error}")),
            };
            live_thread
                .append_items(&[RolloutItem::DynamicToolsUpdated {
                    thread_id: self.thread_id(),
                    tools: tools.clone(),
                }])
                .await
                .map_err(persistence_error)?;
            live_thread.flush().await.map_err(persistence_error)?;
        }
        let tools = Arc::from(tools);
        let mut state = self.state.lock().await;
        state.persisted_dynamic_tools = Arc::clone(&tools);
        state.session_configuration.dynamic_tools = tools;
        // An explicit official replacement supersedes the connection's old catalog snapshot.
        state.runtime_dynamic_tools = None;
        Ok(())
    }

    /// Captures and flushes current settings under the shared persistence permit.
    pub(crate) async fn checkpoint_thread_settings(&self) -> ThreadStoreResult<()> {
        let _settings_guard = acquire_persistence_lock(self).await;
        if let Some(live_thread) = self.live_thread() {
            live_thread
                .append_items(&[RolloutItem::EventMsg(applied_event(self).await)])
                .await?;
            live_thread.flush().await?;
        }
        Ok(())
    }
}

/// Applies standalone thread settings. The caller holds the persistence permit through notification.
#[expect(
    clippy::await_holding_invalid_type,
    reason = "the active-turn guard makes continuation invalidation atomic with turn admission"
)]
pub(super) async fn update(
    session: &Session,
    overrides: impl Into<WithTurnExtensionData<ThreadSettingsOverrides>>,
) -> ConstraintResult<ThreadSettingsSnapshot> {
    let updates = prepare_update(overrides, &session.services.selected_capability_roots);
    let commit = session.update_settings(updates).await?;
    let active_turn = session.active_turn.lock().await;
    // A registered running task retains its lineage so it can be recovered after a daemon
    // restart. Reservations and finishing turns have no interrupted work to continue.
    if active_turn
        .as_ref()
        .is_none_or(|active_turn| active_turn.task.is_none())
    {
        session.state.lock().await.last_started_turn_id = None;
    }
    Ok(commit.snapshot)
}

/// Converts protocol overrides into the internal settings update shape.
pub(super) fn prepare_update(
    overrides: impl Into<WithTurnExtensionData<ThreadSettingsOverrides>>,
    roots: &[SelectedCapabilityRoot],
) -> SessionSettingsUpdate {
    let WithTurnExtensionData {
        request: overrides,
        turn_extension_init,
    } = overrides.into();
    let ThreadSettingsOverrides {
        model_context_window,
        environments: environment_requests,
        runtime_workspace_roots,
        profile_workspace_roots,
        approval_policy,
        approvals_reviewer,
        sandbox_policy,
        permission_profile,
        active_permission_profile,
        windows_sandbox_level,
        model,
        effort,
        summary,
        service_tier,
        collaboration_mode,
        personality,
        disabled_plugin_ids,
    } = overrides;
    SessionSettingsUpdate {
        model_context_window,
        turn_extension_init,
        step_settings: StepSettingsUpdate {
            model,
            effort,
            collaboration_mode,
            reasoning_summary: summary,
            service_tier,
            personality,
            approval_policy,
            approvals_reviewer,
        },
        environments: environment_requests.map(|requests| requests.select(roots)),
        runtime_workspace_roots,
        profile_workspace_roots,
        sandbox_policy,
        permission_profile,
        active_permission_profile,
        windows_sandbox_level,
        disabled_plugin_ids,
        ..Default::default()
    }
}

/// Acquires the shared permit before capturing or changing persistent settings.
pub(super) async fn acquire_persistence_lock(session: &Session) -> SemaphorePermit<'_> {
    session
        .thread_settings_persistence
        .acquire()
        .await
        .unwrap_or_else(|_| unreachable!("thread settings persistence semaphore is never closed"))
}

/// Applies persistent settings and emits the resulting thread-owned snapshot.
pub(super) async fn apply_update(
    session: &Session,
    submission_id: String,
    updates: SessionSettingsUpdate,
) -> ConstraintResult<()> {
    let _settings_guard = acquire_persistence_lock(session).await;
    let commit = session.update_settings(updates).await?;
    emit_applied(session, submission_id, commit.snapshot, true).await;
    Ok(())
}

/// The caller owns the settings persistence permit. Use the existing store's durability fence
/// before acknowledging a context edit, without waiting for its event consumer.
pub(super) async fn persist_context_window_settings(
    session: &Session,
    snapshot: &ThreadSettingsSnapshot,
) -> std::io::Result<()> {
    if let Some(live_thread) = session.live_thread() {
        live_thread
            .append_items(&[RolloutItem::EventMsg(EventMsg::ThreadSettingsApplied(
                ThreadSettingsAppliedEvent {
                    thread_id: Some(session.thread_id()),
                    thread_settings: snapshot.clone(),
                },
            ))])
            .await
            .map_err(std::io::Error::other)?;
        live_thread
            .persist(PersistContext::Standard)
            .await
            .map_err(std::io::Error::other)?;
        live_thread.flush().await.map_err(std::io::Error::other)?;
    }
    Ok(())
}

/// Emits the snapshot published by one successful settings update.
pub(super) async fn emit_applied(
    session: &Session,
    submission_id: String,
    snapshot: ThreadSettingsSnapshot,
    persist: bool,
) {
    let msg = EventMsg::ThreadSettingsApplied(ThreadSettingsAppliedEvent {
        thread_id: Some(session.thread_id()),
        thread_settings: snapshot,
    });
    let event = Event {
        id: submission_id,
        msg,
    };
    if persist {
        session
            .send_event_raw_without_materializing_rollout(event)
            .await;
    } else {
        session.send_event_raw_with_persistence(event, false).await;
    }
}

/// Builds a current thread-owned snapshot for storage checkpoints.
pub(super) async fn applied_event(session: &Session) -> EventMsg {
    EventMsg::ThreadSettingsApplied(ThreadSettingsAppliedEvent {
        thread_id: Some(session.thread_id()),
        thread_settings: session.thread_settings_snapshot().await,
    })
}

type RuntimeToolKey = (Option<String>, String);

#[cfg(test)]
mod runtime_merge_tests {
    use super::*;
    use codex_protocol::dynamic_tools::{
        DynamicToolFunctionSpec, DynamicToolNamespaceSpec, DynamicToolNamespaceTool,
        DynamicToolSpec,
    };

    fn namespace(description: &str, tools: &[(&str, &str)]) -> DynamicToolSpec {
        DynamicToolSpec::Namespace(DynamicToolNamespaceSpec {
            name: "local_tools".into(),
            description: description.into(),
            tools: tools
                .iter()
                .map(|(name, description)| {
                    DynamicToolNamespaceTool::Function(DynamicToolFunctionSpec {
                        name: (*name).into(),
                        description: (*description).into(),
                        input_schema: serde_json::json!({"type":"object", "properties":{}}),
                        defer_loading: false,
                    })
                })
                .collect(),
        })
    }

    #[test]
    fn namespace_merge_preserves_custom_members_and_rejects_unknown_definitions() {
        let function = DynamicToolSpec::Function(DynamicToolFunctionSpec {
            name: "local_tools".into(),
            description: "custom".into(),
            input_schema: serde_json::json!({"type":"object", "properties":{}}),
            defer_loading: false,
        });
        let namespaced = namespace("host", &[("cron", "new")]);
        assert!(merge_runtime_tools(&[function.clone()], &[namespaced.clone()], &[]).is_err());
        assert!(merge_runtime_tools(&[namespaced], &[function], &[]).is_err());
        let base = namespace("user namespace", &[("custom", "keep"), ("cron", "old")]);
        let incoming = namespace("host namespace", &[("cron", "new"), ("control", "new")]);
        assert!(merge_runtime_tools(&[base.clone()], &[incoming.clone()], &[]).is_err());
        let (merged, keys) = merge_runtime_tools(
            &[base],
            &[incoming],
            &[namespace("old namespace", &[("cron", "old")])],
        )
        .unwrap();
        assert_eq!(
            merged,
            vec![namespace(
                "user namespace",
                &[("custom", "keep"), ("cron", "new"), ("control", "new")]
            )]
        );
        assert_eq!(
            keys,
            vec![
                (Some("local_tools".into()), "cron".into()),
                (Some("local_tools".into()), "control".into())
            ]
        );
    }
}

fn merge_runtime_tools(
    base: &[codex_protocol::dynamic_tools::DynamicToolSpec],
    incoming: &[codex_protocol::dynamic_tools::DynamicToolSpec],
    previous: &[codex_protocol::dynamic_tools::DynamicToolSpec],
) -> Result<
    (
        Vec<codex_protocol::dynamic_tools::DynamicToolSpec>,
        Vec<RuntimeToolKey>,
    ),
    String,
> {
    use codex_protocol::dynamic_tools::{DynamicToolNamespaceTool, DynamicToolSpec};
    fn expected(
        previous: &[DynamicToolSpec],
        namespace: Option<&str>,
        current: &codex_protocol::dynamic_tools::DynamicToolFunctionSpec,
    ) -> bool {
        previous.iter().any(|spec| match spec {
            DynamicToolSpec::Function(tool) => namespace.is_none() && tool == current,
            DynamicToolSpec::Namespace(ns) => {
                namespace == Some(ns.name.as_str())
                    && ns.tools.iter().any(|tool| {
                        let DynamicToolNamespaceTool::Function(tool) = tool;
                        tool == current
                    })
            }
        })
    }
    let mut merged = base.to_vec();
    let mut keys = Vec::new();
    for spec in incoming {
        if merged.iter().any(|existing| match (existing, spec) {
            (DynamicToolSpec::Function(function), DynamicToolSpec::Namespace(namespace))
            | (DynamicToolSpec::Namespace(namespace), DynamicToolSpec::Function(function)) => {
                function.name == namespace.name
            }
            _ => false,
        }) {
            return Err("기존 사용자 정의 도구와 같은 이름의 다른 종류가 충돌합니다.".into());
        }
        match spec {
            DynamicToolSpec::Function(tool) => {
                if let Some(existing) = merged.iter_mut().find_map(|spec| match spec {
                    DynamicToolSpec::Function(existing) if existing.name == tool.name => {
                        Some(existing)
                    }
                    _ => None,
                }) {
                    if existing != tool && !expected(previous, None, existing) {
                        return Err(format!("기존 사용자 정의 도구와 충돌합니다: {}", tool.name));
                    }
                    *existing = tool.clone();
                } else {
                    merged.push(spec.clone());
                }
                keys.push((None, tool.name.clone()));
            }
            DynamicToolSpec::Namespace(ns) => {
                if let Some(existing) = merged.iter_mut().find_map(|spec| match spec {
                    DynamicToolSpec::Namespace(existing) if existing.name == ns.name => {
                        Some(existing)
                    }
                    _ => None,
                }) {
                    for tool in &ns.tools {
                        let DynamicToolNamespaceTool::Function(tool) = tool;
                        if let Some(current) = existing.tools.iter_mut().find_map(|entry| {
                            let DynamicToolNamespaceTool::Function(current) = entry;
                            (current.name == tool.name).then_some(current)
                        }) {
                            if current != tool && !expected(previous, Some(&ns.name), current) {
                                return Err(format!(
                                    "기존 사용자 정의 도구와 충돌합니다: {}.{}",
                                    ns.name, tool.name
                                ));
                            }
                            *current = tool.clone();
                        } else {
                            existing
                                .tools
                                .push(DynamicToolNamespaceTool::Function(tool.clone()));
                        }
                    }
                } else {
                    merged.push(spec.clone());
                }
                for tool in &ns.tools {
                    let DynamicToolNamespaceTool::Function(tool) = tool;
                    keys.push((Some(ns.name.clone()), tool.name.clone()));
                }
            }
        }
    }
    Ok((merged, keys))
}
