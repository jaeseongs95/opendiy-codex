// Modified for OpenDIY Codex R30 (2026-10-30); see MODIFICATIONS.md.
//! Tier selection preserves user defaults and independent root conversations. Core
//! intentionally propagates a root tier to its agent tree; that existing inheritance is unchanged.

use super::session_lifecycle_requests::{
    HistoryCapabilities, recorded_params, start_recording_app_server_with_history,
};
use super::*;
use crate::app_server_session::{ResumeModelSettings, ThreadParamsMode};
use crate::chatwidget::tests::helpers::set_ultrafast_mode_test_catalog;
use codex_protocol::config_types::SERVICE_TIER_DEFAULT_REQUEST_VALUE;
use pretty_assertions::assert_eq;

fn prepare_fast_widget(app: &mut App) {
    app.chat_widget.set_model("gpt-5.4");
    set_fast_mode_test_catalog(&mut app.chat_widget);
    app.chat_widget.set_feature_enabled(Feature::FastMode, true);
    app.chat_widget.set_service_tier(Some("flex".into()));
}

async fn drain_selection(
    app: &mut App,
    server: &mut AppServerSession,
    events: &mut tokio::sync::mpsc::UnboundedReceiver<AppEvent>,
) -> Result<String> {
    let mut tui = crate::tui::test_support::make_test_tui()?;
    let mut messages = Vec::new();
    while let Ok(event) = events.try_recv() {
        if let AppEvent::InsertHistoryCell(cell) = &event {
            messages.push(lines_to_single_string(&cell.transcript_lines(200)));
        }
        Box::pin(app.handle_event(&mut tui, server, event)).await?;
    }
    Ok(messages.join("\n"))
}

fn assert_no_global_or_turn_mutations(
    requests: &super::session_lifecycle_requests::RecordedRequests,
) {
    for method in [
        "config/batchWrite",
        "config/value/write",
        "config/mcpServer/reload",
        "turn/start",
        "turn/steer",
        "turn/interrupt",
    ] {
        assert!(recorded_params(requests, method).is_empty(), "{method}");
    }
}

#[tokio::test]
async fn fast_selection_preserves_global_defaults_other_threads_and_permissions() -> Result<()> {
    assert_session_tier_selection("fast", ServiceTier::Fast.request_value()).await
}

#[tokio::test]
async fn ultrafast_selection_preserves_global_defaults_other_threads_and_permissions() -> Result<()>
{
    assert_session_tier_selection("ultrafast", "ultrafast").await
}

async fn assert_session_tier_selection(command: &str, tier: &str) -> Result<()> {
    let (mut app, mut events, _) = make_test_app_with_channels().await;
    let model = if command == "ultrafast" {
        "gpt-6.1-sol"
    } else {
        "gpt-5.4"
    };
    app.config.model = Some(model.into());
    app.config.model_reasoning_effort = Some(ReasoningEffortConfig::Medium);
    app.config.service_tier = Some("flex".into());
    let file = app.config.codex_home.join("config.toml");
    let original = format!(
        "# Explicit defaults must survive a session selection.\nmodel = '{model}'\nmodel_reasoning_effort = 'medium'\nservice_tier = 'flex'\n"
    );
    std::fs::write(&file, &original)?;
    let (mut server, requests, proxy) = start_recording_app_server_with_history(
        &app.config,
        HistoryCapabilities::Current,
        None,
        None,
        ThreadParamsMode::Embedded,
        LoaderOverrides::without_managed_config_for_tests(),
    )
    .await?;
    let selected = server.start_thread(&app.config).await?;
    let before = selected.session.clone();
    let other = server.start_thread(&app.config).await?.session;
    // A new empty thread has a lazy rollout. Persist real history before testing resume.
    server
        .thread_inject_items(
            other.thread_id,
            vec![serde_json::from_value(serde_json::json!({
                "type": "message", "role": "assistant",
                "content": [{"type": "output_text", "text": "Independent conversation fixture"}]
            }))?],
        )
        .await?;
    let id = before.thread_id;
    app.enqueue_primary_thread_session(selected.session, selected.turns)
        .await?;
    prepare_fast_widget(&mut app);
    if command == "ultrafast" {
        set_ultrafast_mode_test_catalog(&mut app.chat_widget, model);
        app.chat_widget.set_model(model);
        app.chat_widget
            .set_feature_enabled(Feature::UltrafastMode, true);
    }
    let permissions = RuntimePermissionProfileOverride::from_config(app.chat_widget.config_ref());
    app.keymap.app.toggle_fast_mode = vec![
        crate::key_hint::ctrl(KeyCode::Char('f')),
        crate::key_hint::alt(KeyCode::Char('f')),
    ];
    let mut tui = crate::tui::test_support::make_test_tui()?;
    for (modifiers, expected_tier) in [
        (KeyModifiers::CONTROL, tier),
        (KeyModifiers::ALT, SERVICE_TIER_DEFAULT_REQUEST_VALUE),
    ] {
        while events.try_recv().is_ok() {}
        requests.lock().unwrap().clear();
        let previous = app.chat_widget.configured_service_tier();
        if command == "ultrafast" {
            app.chat_widget.apply_external_edit(format!("/{command}"));
            app.chat_widget.handle_key_event(KeyCode::Enter.into());
        } else {
            Box::pin(app.handle_tui_event(
                &mut tui,
                &mut server,
                TuiEvent::Key(KeyEvent::new(KeyCode::Char('f'), modifiers)),
            ))
            .await?;
        }
        assert_eq!(
            app.chat_widget.configured_service_tier(),
            previous,
            "selection must await Core acceptance"
        );
        let messages = drain_selection(&mut app, &mut server, &mut events).await?;
        assert!(messages.contains("for this conversation only"));
        let updates = recorded_params(&requests, "thread/settings/update");
        assert_eq!(updates.len(), 1);
        assert_eq!(updates[0]["threadId"], id.to_string());
        assert_eq!(updates[0]["serviceTier"], expected_tier);
        for (key, value) in updates[0].as_object().unwrap() {
            if key != "threadId" && key != "serviceTier" {
                assert!(value.is_null(), "unexpected override: {key}");
            }
        }
        assert_no_global_or_turn_mutations(&requests);
        let updated = next_thread_settings_updated(&mut server, id)
            .await
            .thread_settings;
        assert_eq!(updated.service_tier.as_deref(), Some(expected_tier));
        assert_eq!(updated.model, before.model);
        assert_eq!(updated.effort, before.reasoning_effort);
        assert_eq!(updated.approval_policy, before.approval_policy);
        assert_eq!(
            updated.approvals_reviewer.to_core(),
            before.approvals_reviewer
        );
        assert_eq!(updated.cwd, before.cwd);
        assert_eq!(
            updated.active_permission_profile,
            before.active_permission_profile.clone().map(Into::into)
        );
        assert_eq!(
            PermissionProfile::from_legacy_sandbox_policy_for_cwd(
                &updated.sandbox_policy.to_core(),
                updated.cwd.as_path()
            ),
            before.permission_profile
        );
        assert_eq!(
            RuntimePermissionProfileOverride::from_config(app.chat_widget.config_ref()),
            permissions
        );
        assert_eq!(app.chat_widget.current_service_tier(), Some(expected_tier));
        assert_eq!(
            app.primary_session_configured
                .as_ref()
                .unwrap()
                .service_tier
                .as_deref(),
            Some(expected_tier)
        );
        assert_eq!(app.config.service_tier.as_deref(), Some("flex"));
        assert_eq!(std::fs::read(&file)?, original.as_bytes());
    }
    let other_after = server
        .resume_thread(
            &app.local_settings,
            app.config.clone(),
            other.thread_id,
            ResumeModelSettings::PreserveExistingThread,
        )
        .await?
        .session;
    assert_eq!(other_after.service_tier, other.service_tier);
    assert_eq!(other_after.model, other.model);
    assert_eq!(other_after.permission_profile, other.permission_profile);
    let fresh = server.start_thread(&app.config).await?.session;
    assert_eq!(fresh.service_tier.as_deref(), Some("flex"));
    assert_eq!(fresh.model, before.model);
    assert_eq!(std::fs::read(&file)?, original.as_bytes());
    proxy.abort();
    Ok(())
}

#[tokio::test]
async fn fast_selection_failure_unsupported_queue_ack_and_lost_reply_preserve_local_state()
-> Result<()> {
    assert_failed_session_tier_selection("fast").await
}

#[tokio::test]
async fn ultrafast_selection_failure_unsupported_queue_ack_and_lost_reply_preserve_local_state()
-> Result<()> {
    assert_failed_session_tier_selection("ultrafast").await
}

async fn assert_failed_session_tier_selection(command: &str) -> Result<()> {
    for capability in [
        HistoryCapabilities::ThreadSettingsUpdateUnsupported,
        HistoryCapabilities::ThreadSettingsUpdateFails,
        HistoryCapabilities::ThreadSettingsUpdateQueueOnly,
        HistoryCapabilities::ThreadSettingsUpdateReplyLost,
        HistoryCapabilities::ThreadSettingsUpdateUnconfirmed,
        HistoryCapabilities::ThreadSettingsUpdateMalformed,
    ] {
        let (mut app, mut events, _) = make_test_app_with_channels().await;
        let model = if command == "ultrafast" {
            "gpt-6.1-sol"
        } else {
            "gpt-5.4"
        };
        app.config.model = Some(model.into());
        app.config.service_tier = Some("flex".into());
        let file = app.config.codex_home.join("config.toml");
        let original = "service_tier = 'flex'\n";
        std::fs::write(&file, original)?;
        let (mut server, requests, proxy) = start_recording_app_server_with_history(
            &app.config,
            capability,
            None,
            None,
            ThreadParamsMode::Embedded,
            LoaderOverrides::without_managed_config_for_tests(),
        )
        .await?;
        let started = server.start_thread(&app.config).await?;
        app.enqueue_primary_thread_session(started.session, started.turns)
            .await?;
        prepare_fast_widget(&mut app);
        let cache_before = app.primary_session_configured.clone();
        while events.try_recv().is_ok() {}
        requests.lock().unwrap().clear();
        if command == "ultrafast" {
            set_ultrafast_mode_test_catalog(&mut app.chat_widget, model);
            app.chat_widget.set_model(model);
            app.chat_widget
                .set_feature_enabled(Feature::UltrafastMode, true);
            app.chat_widget.apply_external_edit(format!("/{command}"));
            app.chat_widget.handle_key_event(KeyCode::Enter.into());
        } else {
            app.chat_widget.toggle_fast_mode_from_ui();
        }
        app.chat_widget
            .apply_external_edit("follow-up after tier selection".into());
        app.chat_widget.handle_key_event(KeyCode::Enter.into());
        assert!(app.chat_widget.has_queued_follow_up_messages());
        let messages = drain_selection(&mut app, &mut server, &mut events).await?;
        assert!(!messages.contains("Service tier set"));
        assert!(messages.contains("retained"));
        assert_eq!(app.chat_widget.current_service_tier(), Some("flex"));
        assert_eq!(app.primary_session_configured, cache_before);
        assert_eq!(app.config.service_tier.as_deref(), Some("flex"));
        assert_eq!(std::fs::read(&file)?, original.as_bytes());
        assert_eq!(
            recorded_params(&requests, "thread/settings/update").len(),
            1
        );
        assert_no_global_or_turn_mutations(&requests);
        assert!(app.chat_widget.has_queued_follow_up_messages());
        assert!(!app.chat_widget.maybe_send_next_queued_input());
        proxy.abort();
    }
    Ok(())
}
