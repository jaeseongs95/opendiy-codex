//! Targeted reload never retargets a queued command or falls back to global config mutation.

use super::session_lifecycle_requests::{
    HistoryCapabilities, recorded_params, start_recording_app_server_with_history,
};
use super::*;
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
        let original = server.start_thread(&app.config).await?.session.thread_id;
        let selected = server.start_thread(&app.config).await?.session;
        app.active_thread_id = Some(selected.thread_id);
        app.chat_widget.handle_thread_session(selected);
        app.chat_widget
            .restore_user_message_to_composer("retained draft".into());
        requests.lock().unwrap().clear();
        while events.try_recv().is_ok() {}
        app.submit_thread_op(&mut server, original, Op::ReloadPlugins)
            .await?;
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
