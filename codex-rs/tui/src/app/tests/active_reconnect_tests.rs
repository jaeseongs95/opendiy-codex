// Modified for OpenDIY Codex R32 (2026-11-01); see MODIFICATIONS.md.
//! Active recovery keeps local input and policy while reusing ordinary thread resume.

use super::*;
use crate::app::reconnect::ReconnectPresentation;
use crate::app::reconnect::reconnect;
use codex_config::types::KeybindingSpec;
use codex_config::types::KeybindingsSpec;
use pretty_assertions::assert_eq;
use serde_json::json;
use tokio::net::TcpListener;

use super::disconnect::serve_reconnect_requests;

#[tokio::test]
async fn reconnect_keeps_other_backend_mcp_cron_on_the_new_app_queue() -> Result<()> {
    use crate::app::backend_sessions::BackendSessions;
    use crate::dynamic_tools_mcp::{DynamicToolMcpServer, ThreadToolTransport, ToolServices};

    let (mut app, mut events, _ops) = make_test_app_with_channels().await;
    let caller_a = ThreadId::new();
    let cwd = app.config.cwd.to_path_buf();
    app.active_thread_id = Some(caller_a);
    app.primary_thread_id = Some(caller_a);
    let cached_a = test_thread_session(caller_a, cwd.clone());
    app.ensure_thread_channel(caller_a)
        .store
        .lock()
        .await
        .set_session(cached_a.clone(), Vec::new());
    app.chat_widget.handle_thread_session(cached_a);
    let (session_a, _observed_a, worker_a) =
        crate::app::app_server_requests::retained_reply_tests::connection().await?;
    let owner_a = session_a.backend_id();
    let (mut session_b, observed_b, worker_b) =
        super::session_lifecycle_requests::start_recording_remote_app_server(&app.config).await?;
    let started_b = session_b.start_thread(&app.config).await?;
    let caller_b = started_b.session.thread_id;
    let original_b = session_b
        .thread_read(caller_b, /*include_turns*/ false)
        .await?;
    assert_eq!(original_b.id, caller_b.to_string());
    assert!(!original_b.session_id.is_empty());
    // This regression needs readable history for TUI selection, not an untouched
    // thread's metadata-only rejoin. Prepare it through the real recording backend.
    session_b
        .thread_inject_items(caller_b, vec![App::side_boundary_prompt_item()])
        .await?;
    let prepared_b = session_b
        .thread_read(caller_b, /*include_turns*/ true)
        .await?;
    assert_eq!(prepared_b.id, original_b.id);
    assert_eq!(prepared_b.session_id, original_b.session_id);
    assert_eq!(
        prepared_b.status,
        codex_app_server_protocol::ThreadStatus::Idle
    );
    assert!(prepared_b.path.as_ref().is_some_and(|path| path.is_file()));
    app.ensure_thread_channel(caller_b)
        .store
        .lock()
        .await
        .set_session(started_b.session, started_b.turns);
    let connection_b = session_b.cron_tool_connection();
    let server_b = Arc::new(
        DynamicToolMcpServer::start(
            session_b.request_handle(),
            codex_app_server_protocol::ThreadStartParams::default(),
            app.config.features.get().clone(),
            app.app_event_tx.clone(),
            app.dynamic_tool_status_updates.clone(),
            None,
            ToolServices {
                task_tools: true,
                cron: Some(connection_b.clone()),
                worktrees: None,
            },
        )
        .await?,
    );
    let transport_b = ThreadToolTransport::Mcp(Arc::clone(&server_b));
    let mut overrides = None;
    transport_b.configure_mcp(&mut overrides);
    let config_b = overrides.unwrap().remove("mcp_servers.codex_tui").unwrap();
    let client = codex_http_client::HttpClientBuilder::new().build_direct()?;
    let call = |id, arguments| {
        client.post(config_b["url"].as_str().unwrap())
            .header("Authorization", config_b["http_headers"]["Authorization"].as_str().unwrap())
            .header("Accept", "application/json, text/event-stream")
            .header("MCP-Protocol-Version", "2026-07-28")
            .header("MCP-Method", "tools/call")
            .header("MCP-Name", "cron")
            .json(&json!({"jsonrpc":"2.0","id":id,"method":"tools/call","params": {
                "name":"cron", "arguments":arguments, "_meta": {
                    "threadId":caller_b, "turnId":"caller-turn", "callId":format!("after-reconnect-{id}"),
                    "io.modelcontextprotocol/protocolVersion":"2026-07-28",
                    "io.modelcontextprotocol/clientCapabilities":{}
                }
            }}))
    };
    session_b = session_b.with_thread_tool_transport(transport_b);
    let mut backends = BackendSessions::from_initial(session_a, vec![session_b]);
    let slot_a = backends.slot(owner_a).unwrap();
    let slot_b = backends.slot(connection_b.backend).unwrap();

    let listener = TcpListener::bind("127.0.0.1:0").await?;
    app.app_server_target = AppServerTarget::Remote {
        endpoint: crate::resolve_remote_addr(&format!("ws://{}", listener.local_addr()?))?,
    };
    let server_a = tokio::spawn(async move {
        let (stream, _) = listener.accept().await?;
        let thread = json!({
            "id": caller_a, "sessionId": caller_a, "preview": "A", "ephemeral": false,
            "modelProvider": "test-provider", "createdAt": 1, "updatedAt": 2,
            "status": {"type": "idle"}, "cwd": cwd, "cliVersion": "0.0.0",
            "source": "cli", "turns": []
        });
        serve_reconnect_requests(tokio_tungstenite::accept_async(stream).await?, |request| {
            std::future::ready(Some(match request.method.as_str() {
                "thread/resume" => json!({"result": {"thread": thread,
                    "model": "gpt-test", "modelProvider": "test-provider", "cwd": cwd,
                    "approvalPolicy": "never", "approvalsReviewer": "user",
                    "sandbox": {"type": "dangerFullAccess"}, "reasoningEffort": null}}),
                "thread/read" => json!({"result": {"thread": thread}}),
                "thread/list" | "thread/loaded/list" => {
                    json!({"result": {"data": [], "nextCursor": null}})
                }
                "thread/goal/get" => json!({"result": {"goal": null}}),
                method => panic!("unexpected reconnect request: {method}"),
            }))
        })
        .await
    });
    let old_sender = app.app_event_tx.clone();
    let mut tui = crate::tui::test_support::make_test_tui()?;
    assert!(app.begin_reconnect());
    let connected = reconnect(
        owner_a,
        app.app_server_target.clone(),
        app.config.clone(),
        app.local_settings.clone(),
        Some(caller_a),
        None,
        backends.session_at(slot_a).thread_tool_transport(),
        ReconnectPresentation::Conversation,
    )
    .await?;
    app.finish_reconnect(
        &mut tui,
        backends.session_at_mut(slot_a),
        &mut events,
        connected,
        CODEX_CLI_VERSION,
    )
    .await?;
    assert!(old_sender.app_event_tx.is_closed());
    // First observe the original failure through B's existing HTTP endpoint and closed queue.
    let failed = call(0, json!({"action":"list"}))
        .send()
        .await?
        .text()
        .await?;
    assert!(failed.contains("예약 처리 결과 미확인"), "{failed}");
    // This is the same collection-wide handoff used immediately after finish_reconnect in startup.
    backends.update_tool_event_senders(&app.app_event_tx);
    assert!(old_sender.app_event_tx.is_closed());
    assert!(!app.reconnect.offline);
    app.select_agent_thread(&mut tui, backends.session_at_mut(slot_b), caller_b)
        .await?;
    assert!(!app.thread_unavailable(caller_b));
    let resumes_b =
        super::session_lifecycle_requests::recorded_params(&observed_b, "thread/resume");
    assert_eq!(resumes_b.len(), 1);
    assert_eq!(resumes_b[0]["threadId"], caller_b.to_string());
    let reattached_b = backends
        .session_at_mut(slot_b)
        .thread_read(caller_b, /*include_turns*/ false)
        .await?;
    assert_eq!(reattached_b.id, original_b.id);
    assert_eq!(reattached_b.session_id, original_b.session_id);
    app.handle_app_server_event(
        backends.session_at(slot_b),
        codex_app_server_client::AppServerEvent::ServerNotification(Box::new(
            turn_started_notification(caller_b, "caller-turn"),
        )),
    )
    .await;
    assert_eq!(
        app.active_turn_id_for_thread(caller_b).await.as_deref(),
        Some("caller-turn")
    );
    let current_b = backends.session_at(slot_b).cron_tool_connection();
    assert_eq!(current_b.id, connection_b.id);
    assert!(Arc::ptr_eq(&current_b.live, &connection_b.live));
    assert!(connection_b.live.load(std::sync::atomic::Ordering::Acquire));

    fn mcp_result(body: &str) -> serde_json::Value {
        if let Ok(response) = serde_json::from_str::<serde_json::Value>(body) {
            return response.get("result").expect("MCP result").clone();
        }
        body.lines()
            .filter_map(|line| line.strip_prefix("data:"))
            .filter_map(|line| serde_json::from_str::<serde_json::Value>(line.trim()).ok())
            .find_map(|response| response.get("result").cloned())
            .expect("MCP result in event stream")
    }
    let mut original = None;
    for (step, (id, arguments, expected)) in [
        (0, json!({"action":"list"}), "등록된 예약이 없습니다"),
        (
            1,
            json!({"action":"add","every":"1m","prompt":"B 기존 MCP의 새 예약"}),
            "등록",
        ),
        (2, json!({"action":"list"}), "B 기존 MCP의 새 예약"),
        (
            1,
            json!({"action":"add","every":"1m","prompt":"B 기존 MCP의 새 예약"}),
            "등록",
        ),
        (
            1,
            json!({"action":"add","every":"2m","prompt":"거부해야 할 다른 요청"}),
            "동일 호출 식별자에 다른 요청",
        ),
        (3, json!({"action":"list"}), "B 기존 MCP의 새 예약"),
    ]
    .into_iter()
    .enumerate()
    {
        let request = call(id, arguments);
        let pending = tokio::spawn(async move { request.send().await?.text().await });
        let queued = tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                if let Some(AppEvent::CronToolCall(request)) = events.recv().await {
                    break request;
                }
            }
        })
        .await?;
        assert_eq!(queued.connection.backend, connection_b.backend);
        assert_eq!(queued.connection.id, connection_b.id);
        assert_eq!(queued.params.thread_id, caller_b.to_string());
        let origin = backends.slot(queued.connection.backend).unwrap();
        app.handle_event(
            &mut tui,
            backends.session_at_mut(origin),
            AppEvent::CronToolCall(queued),
        )
        .await?;
        let body = pending.await??;
        let result = mcp_result(&body);
        let text = result["content"][0]["text"].as_str().expect("MCP text");
        assert!(text.contains(expected), "{body}");
        assert_eq!(result["isError"] == json!(true), step == 4, "{body}");
        if step == 1 {
            original = Some(result.clone());
        } else if step == 3 {
            assert_eq!(Some(&result), original.as_ref());
        }
        if matches!(step, 2 | 5) {
            assert_eq!(text.lines().count(), 1);
            assert!(text.contains(&caller_b.to_string()));
            assert!(!text.contains(&caller_a.to_string()));
        }
    }
    // The ordinary unscoped /cron list also proves there is only one job across A and B.
    drain_history(
        &mut app,
        &mut tui,
        backends.session_at_mut(slot_b),
        &mut events,
    )
    .await?;
    let before = app.transcript_cells.len();
    app.handle_event(
        &mut tui,
        backends.session_at_mut(slot_b),
        AppEvent::CronCommand {
            thread_id: None,
            args: "list".into(),
        },
    )
    .await?;
    drain_history(
        &mut app,
        &mut tui,
        backends.session_at_mut(slot_b),
        &mut events,
    )
    .await?;
    let global = lines_to_single_string(
        &app.transcript_cells[before..]
            .iter()
            .flat_map(|cell| cell.transcript_lines(200))
            .collect::<Vec<_>>(),
    );
    assert_eq!(
        global
            .lines()
            .filter(|line| line.contains("prompt="))
            .count(),
        1
    );
    assert_eq!(global.matches("B 기존 MCP의 새 예약").count(), 1);
    assert!(global.contains(&caller_b.to_string()));
    assert!(!global.contains(&caller_a.to_string()));
    assert!(
        observed_b
            .lock()
            .unwrap()
            .iter()
            .any(|request| request.method == "thread/read")
    );
    // Updating only the queue must never revive a suspended MCP connection.
    server_b.suspend();
    backends.update_tool_event_senders(&app.app_event_tx);
    let body = call(4, json!({"action":"list"}))
        .send()
        .await?
        .text()
        .await?;
    assert!(
        body.contains("TUI is reconnecting; tool was not sent"),
        "{body}"
    );
    backends.shutdown().await?;
    worker_a.await??;
    worker_b.await??;
    server_a.await??;
    Ok(())
}

#[tokio::test]
async fn reconnect_restores_launch_reviewer_without_a_profile_override() -> Result<()> {
    let (mut app, _events, _ops) = make_test_app_with_channels().await;
    let mut cached = test_thread_session(ThreadId::new(), app.config.cwd.to_path_buf());
    cached.approvals_reviewer = ApprovalsReviewer::AutoReview;
    app.primary_thread_id = Some(cached.thread_id);
    app.config.approvals_reviewer = ApprovalsReviewer::AutoReview;
    app.harness_overrides.approvals_reviewer = Some(ApprovalsReviewer::AutoReview);
    app.remember_launch_permissions();
    let mut resumed = cached.clone();
    resumed.approvals_reviewer = ApprovalsReviewer::User;
    app.restore_runtime_permissions(&mut resumed, &cached);
    assert_eq!(resumed, cached);
    Ok(())
}

#[tokio::test]
async fn reconnect_restores_history_permissions_and_resumes_unsent_input() -> Result<()> {
    for (recovered_queue, edit_offline, resume_error_code, deferred_notice, notice_enabled) in [
        (true, false, -32603, false, false),
        (true, false, -32603, false, true),
        (false, false, -32603, false, false),
        (true, true, -32603, false, false),
        (true, false, -32600, false, false),
        (false, false, -32600, false, false),
        (true, true, -32600, false, false),
        (true, false, -32603, true, true),
    ] {
        let pending_profile = !recovered_queue && resume_error_code == -32600;
        let (mut app, mut events, mut ops) = make_test_app_with_channels().await;
        app.local_settings.tui.show_server_version_notice = notice_enabled;
        // Reconstructed widgets should use the same hint on every test terminal.
        app.local_settings.tui.keymap.chat.edit_queued_message =
            Some(KeybindingsSpec::One(KeybindingSpec("alt-up".into())));
        let id = ThreadId::new();
        let cwd = app.config.cwd.clone();
        app.config.model = Some("gpt-test".into());
        // Avoid platform-specific path widths in the mode-preservation snapshot.
        app.local_settings.tui.status_line = Some(vec!["model-with-reasoning".into()]);
        app.config
            .permissions
            .set_permission_profile(PermissionProfile::read_only())?;
        app.runtime_approval_policy_override = Some(RuntimeApprovalPolicyOverride::Explicit(
            AskForApproval::OnRequest,
        ));
        app.runtime_permission_profile_override =
            Some(RuntimePermissionProfileOverride::from_config(&app.config));
        app.active_thread_id = Some(id);
        app.primary_thread_id = Some(id);
        let mut thread_session = test_thread_session(id, cwd.to_path_buf());
        thread_session.approval_policy = AskForApproval::OnRequest;
        thread_session.permission_profile = PermissionProfile::read_only();
        app.ensure_thread_channel(id)
            .store
            .lock()
            .await
            .set_session(thread_session.clone(), Vec::new());
        app.chat_widget.handle_thread_session(thread_session);
        app.chat_widget.set_collaboration_mask(
            crate::collaboration_modes::plan_mask(app.model_catalog.as_ref()).unwrap(),
        );
        let cached_mode = app.chat_widget.effective_collaboration_mode().with_updates(
            Some("gpt-test".into()),
            Some(None),
            /*developer_instructions*/ None,
        );
        // A newer server can report a mode changed by another client while disconnected.
        let server_mode = (!recovered_queue).then(|| CollaborationMode {
            mode: ModeKind::Default,
            settings: Settings {
                developer_instructions: Some("Updated by another client".into()),
                ..cached_mode.settings.clone()
            },
        });
        let expected_mode = server_mode.clone().unwrap_or(cached_mode);
        let expected_submitted_mode = expected_mode.clone();
        assert!(!app.model_catalog.collaboration_modes.is_empty());
        if edit_offline {
            app.chat_widget
                .restore_user_message_to_composer("unacknowledged prompt".into());
            app.chat_widget
                .handle_key_event(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
            next_user_turn_op(&mut ops);
        }
        if recovered_queue {
            app.chat_widget
                .set_queue_autosend_suppressed(/*suppressed*/ true);
            app.chat_widget
                .restore_user_message_to_composer("old queued input".into());
            app.chat_widget
                .handle_key_event(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        }
        app.chat_widget
            .restore_user_message_to_composer("kept draft".into());
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let endpoint = crate::resolve_remote_addr(&format!("ws://{}", listener.local_addr()?))?;
        app.app_server_target = AppServerTarget::Remote { endpoint };
        if notice_enabled {
            let (notice, key) = crate::status::remote_connection::pending_server_version_notice(
                &app.local_settings.tui,
                &app.app_server_target,
                /*server_home*/ None,
                "2.1.0",
                Some("2.0.0"),
                /*last_shown*/ None,
            )
            .expect("older server should have a pending notice");
            app.reconnect.seen_version_notice = Some(key);
            if deferred_notice {
                app.pending_server_version_notice = Some(notice);
                app.update_server_version_overview_notice("2.1.0", Some("2.0.0"));
            }
        }
        let thread = json!({
            "id": id, "sessionId": id, "preview": "only once", "ephemeral": false,
            "modelProvider": "test-provider", "createdAt": 1, "updatedAt": 2,
            "status": {"type": "idle"}, "cwd": cwd, "cliVersion": "0.0.0", "source": "cli",
            "turns": [test_turn("accepted", TurnStatus::Completed, vec![ThreadItemPayload::UserMessage {
                id: "input-1".into(), client_id: None, content: vec![UserInput::Text { text: "only once".into(), text_elements: Vec::new() }],
            }])]
        });
        let server = tokio::spawn(async move {
            let mut methods = Vec::new();
            for attempt in 0..2 {
                let (stream, _) = listener.accept().await?;
                methods.extend(serve_reconnect_requests(tokio_tungstenite::accept_async(stream).await?, |request| std::future::ready(match request.method.as_str() {
                    "thread/resume" if attempt == 0 => Some(json!({"error": {"code": resume_error_code, "message":
                        if resume_error_code == -32600 {
                            format!("thread {id} is closing; retry thread/resume after the thread is closed")
                        } else {
                            "temporarily unavailable".into()
                        }
                    }})),
                    "thread/resume" => {
                        let params = request.params.as_ref().unwrap();
                        assert_eq!(params["threadId"], id.to_string());
                        assert!(params["model"].is_null());
                        let mut result = json!({"thread": thread, "model": "gpt-test", "modelProvider": "test-provider", "cwd": cwd,
                            "approvalPolicy": "never", "approvalsReviewer": "user", "sandbox": {"type": "dangerFullAccess"}, "reasoningEffort": null});
                        if let Some(mode) = &server_mode {
                            result["collaborationMode"] = json!(mode);
                        }
                        Some(json!({"result": result}))
                    }
                    "thread/read" => Some(json!({"result": {"thread": thread}})),
                    "thread/list" | "thread/loaded/list" => Some(json!({"result": {"data": [], "nextCursor": null}})),
                    "thread/goal/get" => Some(json!({"result": {"goal": null}})),
                    "turn/start" => {
                        assert!(!edit_offline);
                        let params = request.params.as_ref().unwrap();
                        assert_eq!(params["input"][0]["text"], if recovered_queue { "old queued input" } else { "fresh follow-up" });
                        assert_eq!(params["approvalPolicy"], if pending_profile { "never" } else { "on-request" });
                        assert_eq!(params["sandboxPolicy"]["type"], if pending_profile { json!(null) } else { json!("readOnly") });
                        assert_eq!(params["collaborationMode"], json!(expected_submitted_mode));
                        Some(json!({"result": {"turn": {"id": "fresh", "items": [], "status": "inProgress"}}}))
                    }
                    method => panic!("unexpected reconnect request: {method}"),
                })).await?);
            }
            Ok::<_, color_eyre::Report>(methods)
        });
        let mut session = crate::start_embedded_app_server_for_picker(&app.config).await?;
        std::fs::write(
            app.config
                .codex_home
                .join("tui-thread-reference-capabilities"),
            "not a directory",
        )?;
        session.remember_task_tool_thread(id);
        let transport = crate::dynamic_tools_mcp::ThreadToolTransport::Mcp(Arc::new(
            crate::dynamic_tools_mcp::DynamicToolMcpServer::start(
                session.request_handle(),
                codex_app_server_protocol::ThreadStartParams::default(),
                app.config.features.get().clone(),
                app.app_event_tx.clone(),
                app.dynamic_tool_status_updates.clone(),
                /*managed_requirement*/ None,
                crate::dynamic_tools_mcp::ToolServices {
                    task_tools: true,
                    cron: None,
                    worktrees: None,
                },
            )
            .await?,
        ));
        let mut overrides = None;
        transport.configure_mcp(&mut overrides);
        let mcp = overrides.unwrap().remove("mcp_servers.codex_tui").unwrap();
        let client = codex_http_client::HttpClientBuilder::new().build_direct()?;
        let call_mcp = || {
            client
                .post(mcp["url"].as_str().unwrap())
                .header(
                    "Authorization",
                    mcp["http_headers"]["Authorization"].as_str().unwrap(),
                )
                .header("Accept", "application/json, text/event-stream")
                .header("MCP-Protocol-Version", "2026-07-28")
                .header("MCP-Method", "tools/call")
                .header("MCP-Name", "list_threads")
                .json(
                    &json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {
                        "name": "list_threads", "arguments": {}, "_meta": {"threadId": id,
                        "io.modelcontextprotocol/protocolVersion": "2026-07-28",
                        "io.modelcontextprotocol/clientCapabilities": {}}
                    }}),
                )
        };

        app.chat_widget.remote_connection =
            crate::status::remote_connection::remote_connection_status_value(
                &app.app_server_target,
                Some("1.0.0"),
            );
        let mut tui = crate::tui::test_support::make_test_tui()?;
        if pending_profile {
            app.runtime_approvals_reviewer_override = Some(ApprovalsReviewer::User);
            app.agents_overview.requested_permission_profiles.insert(
                id,
                PermissionProfileSelection {
                    profile_id: "server-only".into(),
                    approval_policy: None,
                    approvals_reviewer: None,
                    display_label: "server-only".into(),
                },
            );
        }
        if edit_offline {
            let view = app.agents_overview_view(Vec::new(), /*selected_thread_id*/ None);
            app.chat_widget.show_bottom_pane_view(Box::new(view));
        }
        let plugin_reload_owner_generation = app.chat_widget.thread_runtime_generation;
        app.begin_reconnect();
        if deferred_notice {
            assert_eq!(
                app.agents_overview
                    .view_state
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .server_version_notice,
                None
            );
        }
        if edit_offline {
            for key in [KeyCode::Esc, KeyCode::Enter] {
                app.handle_tui_event(&mut tui, &mut session, TuiEvent::Key(key.into()))
                    .await?;
            }
            assert!(!app.chat_widget.has_active_view());
            assert_eq!(app.chat_widget.composer_text_with_pending(), "kept draft");
            app.handle_tui_event(
                &mut tui,
                &mut session,
                TuiEvent::Key(KeyEvent::new(KeyCode::Up, KeyModifiers::ALT)),
            )
            .await?;
            assert!(!app.chat_widget.has_queued_follow_up_messages());
            assert!(
                !app.chat_widget
                    .capture_thread_input_state()
                    .unwrap()
                    .recovered_queue
            );
        }
        let stale_picker_refresh = app.agent_navigation.begin_picker_refresh(id).unwrap();
        app.last_subagent_backfill_attempt = Some(id);
        app.rate_limit_refresh_state
            .start(
                RateLimitRefreshOrigin::Recovery,
                &mut app.rate_limit_hard_stop_generation,
            )
            .unwrap();
        let before_disconnect = Instant::now();
        app.recap.note_focus_lost(before_disconnect);
        for _ in 0..3 {
            app.recap
                .note_turn_finished(&TurnStatus::Completed, before_disconnect);
        }
        app.schedule_recap_check(id, Instant::now());
        app.pending_managed_worktree_creation = true;
        app.agents_overview
            .view_state
            .lock()
            .unwrap()
            .creating_worktree = true;
        let old_sender = app.app_event_tx.clone();
        let backend_id = session.backend_id();
        let connected = reconnect(
            session.backend_id(),
            app.app_server_target.clone(),
            app.config.clone(),
            app.local_settings.clone(),
            Some(id),
            /*remote_cwd*/ None,
            transport,
            ReconnectPresentation::Conversation,
        )
        .await?;
        let paused = call_mcp().send().await?.text().await?;
        assert!(
            paused.contains("TUI is reconnecting; tool was not sent"),
            "{paused}"
        );
        app.finish_reconnect(&mut tui, &mut session, &mut events, connected, "2.1.0")
            .await?;
        assert_eq!(session.backend_id(), backend_id);
        assert_ne!(
            app.chat_widget.thread_runtime_generation,
            plugin_reload_owner_generation
        );
        if edit_offline {
            assert!(!app.chat_widget.has_active_view());
            assert!(
                app.agents_overview
                    .view_state
                    .lock()
                    .unwrap()
                    .connection_notice
                    .is_none()
            );
        }
        assert!(app.pending_server_profiles.is_empty());
        assert!(app.agents_overview.requested_permission_profiles.is_empty());
        if pending_profile {
            assert_eq!(
                app.resume_permission_overrides(&app.config),
                crate::resume_permissions::ResumePermissions {
                    approvals_reviewer: true,
                    ..Default::default()
                }
            );
        }
        assert!(!app.pending_managed_worktree_creation);
        assert!(
            !app.agents_overview
                .view_state
                .lock()
                .unwrap()
                .creating_worktree
        );
        assert!(!app.reconnect.offline);
        assert!(!app.thread_unavailable(id));
        assert_eq!(app.last_subagent_backfill_attempt, None);
        assert!(
            app.rate_limit_refresh_state
                .start(
                    RateLimitRefreshOrigin::Recovery,
                    &mut app.rate_limit_hard_stop_generation
                )
                .is_some()
        );
        assert!(app.agent_navigation.begin_picker_refresh(id).is_some());
        assert!(
            app.agent_navigation
                .finish_picker_refresh(id, stale_picker_refresh)
                .is_none()
        );
        // Let the rebound timer become due without depending on machine uptime.
        tokio::time::pause();
        tokio::time::advance(recap::RECAP_DELAY).await;
        tokio::time::resume();
        let mut deferred = Vec::new();
        tokio::time::timeout(Duration::from_secs(/*secs*/ 5), async {
            loop {
                let event = events.recv().await.expect("rebound recap event");
                if matches!(event, AppEvent::CheckRecap { thread_id } if thread_id == id) {
                    break;
                }
                deferred.push(event);
            }
        })
        .await?;
        for event in deferred {
            app.app_event_tx.send(event);
        }
        let ready = call_mcp().send().await?.text().await?;
        assert!(ready.contains("threads"), "{ready}");
        assert_eq!(
            app.chat_widget.remote_connection,
            crate::status::remote_connection::remote_connection_status_value(
                &app.app_server_target,
                Some("2.0.0")
            )
        );
        assert!(old_sender.app_event_tx.is_closed());
        assert!(session.task_tools_available(id));
        assert_eq!(app.current_displayed_thread_id(), Some(id));
        assert_eq!(
            app.chat_widget.effective_collaboration_mode(),
            expected_mode
        );
        assert!(app.model_catalog.collaboration_modes.is_empty());
        assert!(
            app.chat_widget
                .model_catalog()
                .collaboration_modes
                .is_empty()
        );
        assert_eq!(
            app.chat_widget.composer_text_with_pending(),
            if edit_offline {
                "old queued input"
            } else {
                "kept draft"
            }
        );
        let history = drain_history(&mut app, &mut tui, &mut session, &mut events).await?;
        let notices = history
            .lines()
            .filter(|line| {
                line.contains("Couldn't confirm whether")
                    || line.contains("background Codex service")
            })
            .collect::<Vec<_>>()
            .join("\n");
        if deferred_notice {
            assert_snapshot!(notices, @r###"
⚠ A background Codex service is running v2.0.0, older than your Codex CLI
"###);
        } else if edit_offline {
            assert!(notices.contains("unacknowledged prompt"));
        } else {
            assert_eq!(notices, "");
        }
        assert_eq!(history.matches("only once").count(), 1);
        if edit_offline {
            assert!(app.chat_widget.has_queued_follow_up_messages());
            assert!(!app.chat_widget.maybe_send_next_queued_input());
            app.handle_tui_event(
                &mut tui,
                &mut session,
                TuiEvent::Key(KeyEvent::new(KeyCode::Up, KeyModifiers::ALT)),
            )
            .await?;
            assert_eq!(
                app.chat_widget.composer_text_with_pending(),
                "unacknowledged prompt"
            );
            assert!(ops.try_recv().is_err());
        } else if recovered_queue {
            assert!(!app.chat_widget.has_queued_follow_up_messages());
            assert_snapshot!(
                "restored_conversation",
                render_bottom_popup(&app.chat_widget, /*width*/ 80)
            );
        } else {
            app.chat_widget
                .handle_key_event(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL));
            app.chat_widget
                .restore_user_message_to_composer("fresh follow-up".into());
            app.chat_widget
                .handle_key_event(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
            while let Ok(event) = events.try_recv() {
                app.handle_event(&mut tui, &mut session, event).await?;
            }
        }
        while let Ok(event) = events.try_recv() {
            app.handle_event(&mut tui, &mut session, event).await?;
        }
        // Deliver a retained pre-reconnect runtime command through the normal event handler.
        // A recovered transport with the same thread UUID must never receive this old command.
        app.handle_event(
            &mut tui,
            &mut session,
            AppEvent::SubmitThreadOp {
                thread_id: id,
                op: Op::ReloadPlugins {
                    owner_generation: plugin_reload_owner_generation,
                },
            },
        )
        .await?;
        app.handle_event(
            &mut tui,
            &mut session,
            AppEvent::SubmitThreadOp {
                thread_id: id,
                op: Op::SelectServiceTier {
                    owner_generation: plugin_reload_owner_generation,
                    service_tier: Some(ServiceTier::Fast.request_value().into()),
                },
            },
        )
        .await?;
        let stale_messages = std::iter::from_fn(|| events.try_recv().ok())
            .filter_map(|event| {
                if let AppEvent::InsertHistoryCell(cell) = event {
                    Some(lines_to_single_string(&cell.display_lines(160)))
                } else {
                    None
                }
            })
            .collect::<Vec<_>>()
            .join("\n");
        assert!(stale_messages.contains("Stale plugin reload"));
        assert!(stale_messages.contains("Stale service tier selection"));
        assert!(!stale_messages.contains("Service tier set"));
        assert!(!stale_messages.contains("Reloaded plugin configuration"));
        session.shutdown().await?;
        let methods = server.await??;
        assert!(
            !methods.iter().any(
                |method| method == "thread/plugin/reload" || method == "thread/settings/update"
            )
        );
        assert_eq!(
            methods
                .iter()
                .filter(|method| *method == "thread/resume")
                .count(),
            2
        );
        assert_eq!(
            methods
                .iter()
                .filter(|method| *method == "turn/start")
                .count(),
            usize::from(!edit_offline)
        );
    }
    Ok(())
}

#[tokio::test]
async fn reconnect_reconciles_offscreen_pending_profile_before_restoring_permissions() -> Result<()>
{
    let (mut app, mut events, _) = make_test_app_with_channels().await;
    let primary = ThreadId::new();
    let displayed = ThreadId::new();
    let cwd = app.config.cwd.to_path_buf();
    app.config
        .permissions
        .set_permission_profile(PermissionProfile::Disabled)?;
    app.runtime_permission_profile_override =
        Some(RuntimePermissionProfileOverride::from_config(&app.config));
    app.primary_thread_id = Some(primary);
    app.active_thread_id = Some(displayed);
    for id in [primary, displayed] {
        let mut session = test_thread_session(id, cwd.clone());
        session.permission_profile = PermissionProfile::Disabled;
        app.ensure_thread_channel(id)
            .store
            .lock()
            .await
            .set_session(session.clone(), Vec::new());
        if id == primary {
            app.primary_session_configured = Some(session);
            app.upsert_agent_picker_thread(
                id, /*agent_nickname*/ None, /*agent_role*/ None,
                /*is_closed*/ false,
            );
        } else {
            app.chat_widget.handle_thread_session(session);
        }
    }
    app.pending_server_profiles.insert(
        primary,
        PermissionProfileSelection {
            profile_id: "server-only".into(),
            approval_policy: None,
            approvals_reviewer: None,
            display_label: "server-only".into(),
        },
    );

    let listener = TcpListener::bind("127.0.0.1:0").await?;
    app.app_server_target = AppServerTarget::Remote {
        endpoint: crate::resolve_remote_addr(&format!("ws://{}", listener.local_addr()?))?,
    };
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await?;
        serve_reconnect_requests(tokio_tungstenite::accept_async(stream).await?, |request| {
            let id = request.params.as_ref().and_then(|params| params["threadId"].as_str());
            let thread = |id: ThreadId| json!({
                "id": id, "sessionId": id, "preview": "task", "ephemeral": false,
                "modelProvider": "test-provider", "createdAt": 1, "updatedAt": 2,
                "status": {"type": "idle"}, "cwd": cwd, "cliVersion": "0.0.0",
                "source": "cli", "turns": []
            });
            std::future::ready(Some(match request.method.as_str() {
                "thread/resume" => {
                    let pending = id == Some(primary.to_string().as_str());
                    json!({"result": {"thread": thread(if pending { primary } else { displayed }),
                        "model": "gpt-test", "modelProvider": "test-provider", "cwd": cwd,
                        "approvalPolicy": if pending { "on-request" } else { "never" },
                        "approvalsReviewer": "user",
                        "sandbox": {"type": if pending { "readOnly" } else { "dangerFullAccess" }},
                        "activePermissionProfile": if pending { Some(json!({"id": "server-only"})) } else { None },
                        "reasoningEffort": null}})
                }
                "thread/read" => json!({"result": {"thread": thread(primary)}}),
                "turn/start" => {
                    let params = request.params.as_ref().unwrap();
                    assert_eq!(params["permissions"], serde_json::Value::Null);
                    assert_eq!(params["approvalPolicy"], "on-request");
                    assert_eq!(params["sandboxPolicy"], json!(null));
                    json!({"result": {"turn": {"id": "fresh", "items": [], "status": "inProgress"}}})
                }
                "thread/list" | "thread/loaded/list" =>
                    json!({"result": {"data": [], "nextCursor": null}}),
                "thread/goal/get" => json!({"result": {"goal": null}}),
                method => panic!("unexpected reconnect request: {method}"),
            }))
        })
        .await
    });
    let mut session = crate::start_embedded_app_server_for_picker(&app.config).await?;
    let mut tui = crate::tui::test_support::make_test_tui()?;
    app.begin_reconnect();
    let connected = reconnect(
        session.backend_id(),
        app.app_server_target.clone(),
        app.config.clone(),
        app.local_settings.clone(),
        Some(displayed),
        /*remote_cwd*/ None,
        session.thread_tool_transport(),
        ReconnectPresentation::Conversation,
    )
    .await?;
    app.finish_reconnect(
        &mut tui,
        &mut session,
        &mut events,
        connected,
        CODEX_CLI_VERSION,
    )
    .await?;
    assert!(app.pending_server_profiles.contains_key(&primary));

    app.select_agent_thread(&mut tui, &mut session, primary)
        .await?;
    assert!(!app.thread_unavailable(primary));
    assert!(!app.pending_server_profiles.contains_key(&primary));
    assert_eq!(
        app.chat_widget
            .config_ref()
            .permissions
            .permission_profile(),
        &PermissionProfile::read_only()
    );
    assert_eq!(
        app.chat_widget
            .config_ref()
            .permissions
            .active_permission_profile()
            .as_ref()
            .map(|profile| profile.id.as_str()),
        Some("server-only")
    );
    app.chat_widget
        .restore_user_message_to_composer("safe follow-up".into());
    app.chat_widget
        .handle_key_event(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    while let Ok(event) = events.try_recv() {
        if matches!(event, AppEvent::CodexOp(AppCommand::UserTurn { .. })) {
            app.handle_event(&mut tui, &mut session, event).await?;
        }
    }
    session.shutdown().await?;
    let methods = server.await??;
    assert_eq!(
        methods
            .iter()
            .filter(|method| *method == "turn/start")
            .count(),
        1
    );
    Ok(())
}

#[tokio::test]
async fn reconnect_exhaustion_and_unknown_initial_thread_stay_offline() -> Result<()> {
    let (mut app, _, _) = make_test_app_with_channels().await;
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    app.app_server_target = AppServerTarget::Remote {
        endpoint: crate::resolve_remote_addr(&format!("ws://{}", listener.local_addr()?))?,
    };
    drop(listener);
    tokio::time::pause();
    let start = tokio::time::Instant::now();
    let reconnect_thread = |id| {
        reconnect(
            crate::backend_id::BackendId::new(),
            app.app_server_target.clone(),
            app.config.clone(),
            app.local_settings.clone(),
            id,
            /*remote_cwd*/ None,
            crate::dynamic_tools_mcp::ThreadToolTransport::Dynamic,
            ReconnectPresentation::Conversation,
        )
    };
    assert!(reconnect_thread(None).await.is_err());
    let error = reconnect_thread(Some(ThreadId::new()))
        .await
        .err()
        .expect("reconnect should time out");
    assert_eq!(start.elapsed().as_secs(), 120);
    app.begin_reconnect();
    app.chat_widget.reconnect_failed();
    assert_snapshot!(
        "reconnect_failed",
        format!(
            "{error}\n{}",
            render_bottom_popup(&app.chat_widget, /*width*/ 80)
        )
    );
    tokio::time::resume();
    Ok(())
}

#[tokio::test]
async fn reconnect_allows_slow_hydration_but_bounds_a_stalled_server() -> Result<()> {
    for delay in [15, 150] {
        let (mut app, mut events, _) = make_test_app_with_channels().await;
        app.config.model = Some("gpt-test".into());
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let endpoint = crate::RemoteAppServerEndpoint::WebSocket {
            websocket_url: format!("ws://{}", listener.local_addr()?),
            auth_token: None,
        };
        let id = ThreadId::new();
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await?;
            serve_reconnect_requests(
                tokio_tungstenite::accept_async(stream).await?,
                move |request| async move {
                    assert_eq!(request.method, "thread/resume");
                    tokio::time::pause();
                    tokio::time::sleep(Duration::from_secs(delay)).await;
                    tokio::time::resume();
                    Some(json!({"error": {"code": -32600, "message": "thread no longer exists"}}))
                },
            )
            .await
        });
        let start = tokio::time::Instant::now();
        let result = reconnect(
            crate::backend_id::BackendId::new(),
            AppServerTarget::Remote {
                endpoint: endpoint.clone(),
            },
            app.config.clone(),
            app.local_settings.clone(),
            Some(id),
            /*remote_cwd*/ None,
            crate::dynamic_tools_mcp::ThreadToolTransport::Dynamic,
            ReconnectPresentation::Conversation,
        )
        .await;
        let elapsed = start.elapsed().as_secs();
        if delay == 15 {
            app.app_server_target = AppServerTarget::Remote { endpoint };
            app.active_thread_id = Some(id);
            app.primary_thread_id = Some(id);
            let cached = test_thread_session(id, app.config.cwd.to_path_buf());
            app.ensure_thread_channel(id)
                .store
                .lock()
                .await
                .set_session(cached, Vec::new());
            app.chat_widget
                .restore_user_message_to_composer("unavailable draft".into());
            app.begin_reconnect();
            let mut session = crate::start_embedded_app_server_for_picker(&app.config).await?;
            let mut tui = crate::tui::test_support::make_test_tui()?;
            app.finish_reconnect(
                &mut tui,
                &mut session,
                &mut events,
                result?,
                CODEX_CLI_VERSION,
            )
            .await?;
            assert!(app.thread_unavailable(id));
            app.handle_tui_event(
                &mut tui,
                &mut session,
                TuiEvent::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
            )
            .await?;
            assert_eq!(
                app.chat_widget.composer_text_with_pending(),
                "unavailable draft"
            );
            let history = drain_history(&mut app, &mut tui, &mut session, &mut events).await?;
            let history = &history[history.find("• This conversation is unavailable").unwrap()..];
            assert_snapshot!(
                "unavailable_conversation",
                format!(
                    "{history}\n{}",
                    render_bottom_popup(&app.chat_widget, /*width*/ 80)
                )
                .replace(
                    &test_path_buf("/tmp/project").display().to_string(),
                    "/tmp/project"
                )
            );
            session.shutdown().await?;
            assert!((15..120).contains(&elapsed), "{elapsed}");
            let methods = server.await??;
            assert_eq!(
                methods
                    .iter()
                    .filter(|method| *method == "initialize")
                    .count(),
                1
            );
        } else {
            tokio::time::resume();
            assert_eq!(
                result.err().expect("resume should time out").to_string(),
                "Automatic reconnect timed out while resuming the conversation."
            );
            assert_eq!(elapsed, 120);
            server.abort();
        }
    }
    Ok(())
}

pub(super) async fn drain_history(
    app: &mut App,
    tui: &mut tui::Tui,
    session: &mut AppServerSession,
    events: &mut mpsc::UnboundedReceiver<AppEvent>,
) -> Result<String> {
    while let Ok(event) = events.try_recv() {
        if matches!(
            event,
            AppEvent::InsertHistoryCell(_)
                | AppEvent::CodexOp(AppCommand::UserTurn { .. })
                | AppEvent::BeginThreadSwitchHistoryReplayBuffer
                | AppEvent::EndInitialHistoryReplayBuffer
        ) {
            app.handle_event(tui, session, event).await?;
        }
    }
    Ok(lines_to_single_string(
        &app.transcript_cells
            .iter()
            .flat_map(|cell| cell.transcript_lines(/*width*/ 80))
            .collect::<Vec<_>>(),
    ))
}
