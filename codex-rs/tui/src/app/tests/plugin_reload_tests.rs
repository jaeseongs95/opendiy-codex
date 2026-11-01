// Modified for OpenDIY Codex R31 (2026-11-01); see MODIFICATIONS.md.
//! Targeted reload never retargets a queued command or falls back to global config mutation.

use super::session_lifecycle_requests::{
    HistoryCapabilities, recorded_params, start_recording_app_server_with_history,
};
use super::*;
use crate::app::session_lifecycle::ThreadAttachPresentation;
use crate::app_server_session::ResumeModelSettings;
use crate::app_server_session::ThreadParamsMode;
use pretty_assertions::assert_eq;

#[tokio::test]
async fn reload_plugin_routes_captured_thread_and_unsupported_backend_fails_closed() -> Result<()> {
    for capabilities in [
        HistoryCapabilities::Current,
        HistoryCapabilities::PluginReloadUnsupported,
    ] {
        let (mut app, mut events, _) = make_test_app_with_channels().await;
        let (mut server, requests, proxy) = start_recording_app_server_with_history(
            &app.config,
            capabilities,
            None,
            None,
            ThreadParamsMode::Embedded,
            LoaderOverrides::without_managed_config_for_tests(),
        )
        .await?;
        let started = server.start_thread(&app.config).await?;
        let original = started.session.thread_id;
        app.enqueue_primary_thread_session(started.session, started.turns)
            .await?;
        let command = queued_reload_plugin(&mut app, &mut events);
        app.chat_widget
            .restore_user_message_to_composer("retained draft".into());
        requests.lock().unwrap().clear();
        let mut tui = crate::tui::test_support::make_test_tui()?;
        app.handle_event(&mut tui, &mut server, command).await?;
        let reloads = recorded_params(&requests, "thread/plugin/reload");
        assert_eq!(reloads.len(), 1);
        assert_eq!(reloads[0]["threadId"], original.to_string());
        for forbidden in [
            "config/batchWrite",
            "config/mcpServer/reload",
            "turn/start",
            "turn/steer",
            "turn/interrupt",
        ] {
            assert!(
                recorded_params(&requests, forbidden).is_empty(),
                "{forbidden}"
            );
        }
        assert_eq!(
            app.chat_widget.composer_text_with_pending(),
            "retained draft"
        );
        let history = next_history_message(&mut events);
        if capabilities == HistoryCapabilities::Current {
            assert!(history.contains("MCP reconnect requested"));
            assert!(history.contains("confirmed"));
        } else {
            assert!(history.contains("Plugin reload failed"));
            assert!(history.contains("method not found"));
        }
        proxy.abort();
    }
    Ok(())
}

/// Exercise the actual slash parser and event queue, retaining the command across a UI handoff.
fn queued_reload_plugin(
    app: &mut App,
    events: &mut tokio::sync::mpsc::UnboundedReceiver<AppEvent>,
) -> AppEvent {
    queued_runtime_command(app, events, "/reload_plugin")
}

fn queued_runtime_command(
    app: &mut App,
    events: &mut tokio::sync::mpsc::UnboundedReceiver<AppEvent>,
    command: &str,
) -> AppEvent {
    while events.try_recv().is_ok() {}
    app.chat_widget.insert_str(command);
    app.chat_widget.handle_key_event(KeyCode::Esc.into());
    app.chat_widget.handle_key_event(KeyCode::Enter.into());
    std::iter::from_fn(|| events.try_recv().ok())
        .find(|event| {
            matches!(
                event,
                AppEvent::SubmitThreadOp {
                    op: Op::ReloadPlugins { .. } | Op::SelectServiceTier { .. },
                    ..
                }
            )
        })
        .expect("slash command enqueues a captured runtime owner")
}

#[tokio::test]
async fn reload_plugin_and_fast_queued_before_screen_or_backend_replacement_sends_zero_rpcs()
-> Result<()> {
    // Same-backend navigation is intentionally rejected too; users can issue a new command.
    for (switch_backend, command, stale_error) in [
        (false, "/reload_plugin", "Stale plugin reload"),
        (true, "/reload_plugin", "Stale plugin reload"),
        (false, "/fast", "Stale service tier selection"),
        (true, "/fast", "Stale service tier selection"),
    ] {
        let (mut app, mut events, _) = make_test_app_with_channels().await;
        let timestamp = "2026-01-01T00-00-00";
        let saved = app_test_support::create_fake_rollout(
            app.config.codex_home.as_path(),
            timestamp,
            "2026-01-01T00:00:00Z",
            "reload owner fixture",
            Some(&app.config.model_provider_id),
            None,
        )
        .expect("saved rollout");
        let id = ThreadId::from_string(&saved)?;
        let other_home = tempdir()?;
        let mut other_config = app.config.clone();
        other_config.codex_home = other_home.path().to_path_buf().abs();
        other_config.sqlite = codex_state::SqliteConfig::new_for_testing(other_home.path().abs());
        // Independent homes avoid shared writer leases while deliberately reusing the native UUID.
        let source =
            app_test_support::rollout_path(app.config.codex_home.as_path(), timestamp, &saved);
        let destination = app_test_support::rollout_path(other_home.path(), timestamp, &saved);
        std::fs::create_dir_all(destination.parent().unwrap())?;
        std::fs::copy(source, destination)?;
        let (mut origin, origin_requests, origin_proxy) = start_recording_app_server_with_history(
            &app.config,
            HistoryCapabilities::Current,
            None,
            None,
            ThreadParamsMode::Embedded,
            LoaderOverrides::without_managed_config_for_tests(),
        )
        .await?;
        let (mut other, other_requests, other_proxy) = start_recording_app_server_with_history(
            &other_config,
            HistoryCapabilities::Current,
            None,
            None,
            ThreadParamsMode::Embedded,
            LoaderOverrides::without_managed_config_for_tests(),
        )
        .await?;
        assert_ne!(origin.backend_id(), other.backend_id());
        let original = origin
            .resume_thread(
                &app.local_settings,
                app.config.clone(),
                id,
                ResumeModelSettings::PreserveExistingThread,
            )
            .await?;
        let mut tui = crate::tui::test_support::make_test_tui()?;
        app.replace_chat_widget_with_app_server_thread(
            &mut tui,
            original,
            ThreadAttachPresentation::SessionLineage,
            None,
        )
        .await?;
        app.chat_widget.set_model("gpt-5.4");
        set_fast_mode_test_catalog(&mut app.chat_widget);
        app.chat_widget.set_feature_enabled(Feature::FastMode, true);
        let owner = app.chat_widget.thread_runtime_generation;
        let queued = queued_runtime_command(&mut app, &mut events, command);
        let replacement = if switch_backend {
            other
                .resume_thread(
                    &crate::local_settings::LocalSettings::from(&other_config),
                    other_config,
                    id,
                    ResumeModelSettings::PreserveExistingThread,
                )
                .await?
        } else {
            origin.start_thread(&app.config).await?
        };
        if switch_backend {
            assert!(
                !replacement.blocks_direct_input,
                "replacement runtime must be writable"
            );
            assert_eq!(
                replacement.session.thread_id, id,
                "both native backends loaded the same UUID"
            );
        }
        app.replace_chat_widget_with_app_server_thread(
            &mut tui,
            replacement,
            ThreadAttachPresentation::SessionLineage,
            None,
        )
        .await?;
        assert_ne!(app.chat_widget.thread_runtime_generation, owner);
        app.chat_widget
            .restore_user_message_to_composer("keep replacement draft".into());
        origin_requests.lock().unwrap().clear();
        other_requests.lock().unwrap().clear();
        while events.try_recv().is_ok() {}
        let displayed_server = if switch_backend {
            &mut other
        } else {
            &mut origin
        };
        app.handle_event(&mut tui, displayed_server, queued).await?;
        assert!(origin_requests.lock().unwrap().is_empty());
        assert!(other_requests.lock().unwrap().is_empty());
        assert_eq!(
            app.chat_widget.composer_text_with_pending(),
            "keep replacement draft"
        );
        let history = next_history_message(&mut events);
        assert!(history.contains(stale_error));
        assert!(!history.contains("Service tier set"));
        assert!(!history.contains("Reloaded plugin configuration"));
        origin_proxy.abort();
        other_proxy.abort();
    }
    Ok(())
}
