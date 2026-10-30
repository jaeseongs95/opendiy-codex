// Modified for OpenDIY Codex R30 (2026-10-30); see MODIFICATIONS.md.
//! Handles persistent thread-settings updates and serializes their persistence
//! with checkpoints written directly to storage.

use super::session::Session;
use super::session::SessionSettingsUpdate;
use super::step_settings::StepSettingsUpdate;
use crate::WithTurnExtensionData;
use crate::config::ConstraintResult;
use codex_history::RolloutItem;
use codex_protocol::capabilities::SelectedCapabilityRoot;
use codex_protocol::protocol::Event;
use codex_protocol::protocol::EventMsg;
use codex_protocol::protocol::ThreadSettingsAppliedEvent;
use codex_protocol::protocol::ThreadSettingsOverrides;
use codex_protocol::protocol::ThreadSettingsSnapshot;
use codex_thread_store::PersistContext;
use codex_thread_store::ThreadStoreResult;
use tokio::sync::SemaphorePermit;

impl Session {
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
