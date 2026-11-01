// Modified for OpenDIY Codex R32 (2026-11-01); see MODIFICATIONS.md.
//! Event-cache-to-terminal regression tests for the root-scoped subagent status row.

use super::*;
use crate::app::test_support::make_test_app;
use crate::render::renderable::Renderable;
use codex_app_server_protocol::CommandExecutionRequestApprovalParams;
use codex_app_server_protocol::RequestId;
use codex_app_server_protocol::ServerRequestResolvedNotification;
use codex_app_server_protocol::ToolRequestUserInputParams;
use codex_app_server_protocol::TurnCompletedNotification;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

fn render_bottom(app: &App) -> (u16, String) {
    let view = app.chat_widget.bottom_pane_renderable(Default::default());
    let width = 200;
    let height = view.desired_height(width);
    let area = Rect::new(0, 0, width, height);
    let mut buffer = Buffer::empty(area);
    view.render(area, &mut buffer);
    (
        height,
        buffer.content().iter().map(|cell| cell.symbol()).collect(),
    )
}

fn turn(status: TurnStatus) -> Turn {
    Turn {
        id: "turn-status".to_string(),
        root_turn_id: None,
        items: Vec::new(),
        items_view: Default::default(),
        status,
        error: None,
        started_at: None,
        completed_at: None,
        duration_ms: None,
    }
}

#[tokio::test]
async fn subagent_status_events_update_real_bottom_buffer_and_root_switch_clears_it() {
    let mut app = make_test_app().await;
    let root = ThreadId::new();
    let child = ThreadId::new();
    app.primary_thread_id = Some(root);
    app.active_thread_id = Some(root);
    app.agent_navigation.bind_status_root(root);
    app.sync_active_agent_label();
    let (empty_height, empty) = render_bottom(&app);
    assert!(!empty.contains("/subagents"));

    app.upsert_agent_picker_thread(child, None, None, false);
    assert!(render_bottom(&app).1.contains("idle 1"));
    let channel = app.ensure_thread_channel(child);
    let store = Arc::clone(&channel.store);
    store
        .lock()
        .await
        .set_active_turn_id("turn-status".to_string());
    app.refresh_pending_thread_approvals().await;
    assert!(render_bottom(&app).1.contains("running 1"));

    store
        .lock()
        .await
        .push_request(ServerRequest::CommandExecutionRequestApproval {
            request_id: RequestId::Integer(1),
            params: CommandExecutionRequestApprovalParams {
                kind: Default::default(),
                thread_id: child.to_string(),
                turn_id: "turn-status".to_string(),
                item_id: "call".to_string(),
                started_at_ms: 0,
                approval_id: None,
                environment_id: None,
                reason: None,
                network_approval_context: None,
                command: Some("echo status".to_string()),
                cwd: None,
                command_actions: None,
                additional_permissions: None,
                proposed_execpolicy_amendment: None,
                proposed_network_policy_amendments: None,
                available_decisions: None,
            },
        });
    app.refresh_pending_thread_approvals().await;
    assert!(render_bottom(&app).1.contains("approval needed 1"));
    // Viewing the child hides its inactive-approval preview, but must retain its status row.
    app.active_thread_id = Some(child);
    app.refresh_pending_thread_approvals().await;
    assert!(render_bottom(&app).1.contains("approval needed 1"));
    assert!(app.chat_widget.pending_thread_approvals().is_empty());
    store
        .lock()
        .await
        .push_notification(ServerNotification::ServerRequestResolved(
            ServerRequestResolvedNotification {
                thread_id: child.to_string(),
                request_id: RequestId::Integer(1),
            },
        ));
    store
        .lock()
        .await
        .push_request(ServerRequest::ToolRequestUserInput {
            request_id: RequestId::Integer(2),
            params: ToolRequestUserInputParams {
                thread_id: child.to_string(),
                turn_id: "turn-status".to_string(),
                item_id: "question".to_string(),
                questions: Vec::new(),
                is_blocking: true,
                auto_resolution_ms: None,
            },
        });
    app.refresh_pending_thread_approvals().await;
    assert!(render_bottom(&app).1.contains("input needed 1"));
    store
        .lock()
        .await
        .push_notification(ServerNotification::ServerRequestResolved(
            ServerRequestResolvedNotification {
                thread_id: child.to_string(),
                request_id: RequestId::Integer(2),
            },
        ));
    app.refresh_pending_thread_approvals().await;
    assert!(render_bottom(&app).1.contains("running 1"));
    for (status, label) in [
        (TurnStatus::Completed, "completed 1"),
        (TurnStatus::Failed, "failed 1"),
        (TurnStatus::Interrupted, "stopped 1"),
    ] {
        app.enqueue_thread_notification(
            child,
            ServerNotification::TurnCompleted(TurnCompletedNotification {
                thread_id: child.to_string(),
                turn: turn(status),
            }),
        )
        .await
        .expect("route terminal event from the existing event feed");
        assert!(render_bottom(&app).1.contains(label), "{label}");
    }

    let other_root = ThreadId::new();
    app.agent_navigation.bind_status_root(other_root);
    app.primary_thread_id = Some(other_root);
    app.active_thread_id = Some(other_root);
    app.refresh_pending_thread_approvals().await;
    let (new_height, rendered) = render_bottom(&app);
    assert!(!rendered.contains("/subagents"));
    assert_eq!(
        new_height, empty_height,
        "zero-agent root reserves no status-row height"
    );
    // A retained old store can change without reappearing under the new conversation root.
    store
        .lock()
        .await
        .set_active_turn_id("old-root-late-turn".to_string());
    app.refresh_pending_thread_approvals().await;
    assert!(!render_bottom(&app).1.contains("/subagents"));
}
