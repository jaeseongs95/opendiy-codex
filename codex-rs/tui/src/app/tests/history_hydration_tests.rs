//! Public history requests preserve bounded metadata loading, cursor ownership and retry settings.

use super::session_lifecycle_requests::recorded_params;
use super::session_lifecycle_requests::start_recording_app_server;
use super::*;
use crate::app_server_session::HistoryHydrationScope;
use crate::legacy_core::config::TerminalResizeReflowMaxRows;
use crate::pager_overlay::TranscriptHistoryState;
use crate::session_start::SessionStartAction;
use crate::session_start::SessionStartConfig;
use crate::session_start::SessionStartOutcome;
use crate::session_start::complete_session_start;
use crate::transcript_mode::TranscriptMode;
use app_test_support::create_fake_paginated_rollout;
use app_test_support::rollout_path;
use codex_app_server_protocol::ClientRequest;
use codex_app_server_protocol::ThreadItemsListResponse;
use codex_app_server_protocol::ThreadReadParams;
use codex_app_server_protocol::ThreadResumeParams;
use codex_app_server_protocol::ThreadResumeResponse;
use codex_protocol::items::TurnItem;
use codex_protocol::items::UserMessageItem;
use codex_protocol::protocol::ItemCompletedEvent;
use codex_protocol::protocol::TurnCompleteEvent;
use codex_protocol::user_input::UserInput as CoreUserInput;
use codex_state::SqliteConfig;
use pretty_assertions::assert_eq;
use pretty_assertions::assert_ne;

async fn history_fixture(
    item_counts: &[usize],
) -> Result<(Box<App>, tempfile::TempDir, SessionTarget)> {
    let mut app = make_test_app().await;
    let codex_home = tempdir()?;
    app.config.codex_home = codex_home.path().to_path_buf().abs();
    app.config.sqlite = SqliteConfig::new_for_testing(codex_home.path().abs());
    let filename_timestamp = "2026-01-02T00-00-00";
    let timestamp = "2026-01-02T00:00:00Z";
    let id = create_fake_paginated_rollout(
        codex_home.path(),
        filename_timestamp,
        timestamp,
        "history hydration",
        Some(app.config.model_provider_id.as_str()),
        /*git_info*/ None,
    )
    .map_err(|error| color_eyre::eyre::eyre!(error))?;
    let thread_id = ThreadId::from_string(&id)?;
    let path = rollout_path(codex_home.path(), filename_timestamp, &id);
    let mut records = std::fs::read_to_string(&path)?
        .lines()
        .take(/*n*/ 1)
        .map(serde_json::from_str::<serde_json::Value>)
        .collect::<Result<Vec<_>, _>>()?;
    for (index, count) in item_counts.iter().enumerate() {
        let turn_id = format!("turn-{index}");
        let mut events = vec![EventMsg::TurnStarted(TurnStartedEvent {
            turn_attribution: None,
            turn_id: turn_id.clone(),
            root_turn_id: None,
            trace_id: None,
            started_at: None,
            model_context_window: None,
            collaboration_mode_kind: Default::default(),
        })];
        for item in 0..*count {
            events.push(EventMsg::ItemCompleted(ItemCompletedEvent {
                thread_id,
                turn_id: turn_id.clone(),
                started_at_ms: None,
                completed_at_ms: 0,
                item: TurnItem::UserMessage(UserMessageItem {
                    id: format!("item-{index}-{item}"),
                    client_id: None,
                    content: vec![CoreUserInput::Text {
                        text: format!("prompt {index} {item}"),
                        text_elements: Vec::new(),
                    }],
                }),
            }));
        }
        events.push(EventMsg::TurnComplete(TurnCompleteEvent {
            root_turn_id: None,
            turn_id,
            last_agent_message: None,
            error: None,
            started_at: None,
            completed_at: None,
            duration_ms: None,
            time_to_first_token_ms: None,
        }));
        for event in events {
            records.push(serde_json::json!({
                "timestamp": timestamp, "ordinal": records.len(),
                "type": "event_msg", "payload": event,
            }));
        }
    }
    let contents = records
        .into_iter()
        .map(|record| record.to_string())
        .collect::<Vec<_>>()
        .join("\n");
    std::fs::write(&path, format!("{contents}\n"))?;
    let target = SessionTarget {
        backend_id: crate::backend_id::BackendId::new(),
        path: Some(path),
        thread_id,
        cwd: None,
        history_mode: Some(codex_app_server_protocol::ThreadHistoryMode::Paginated),
    };
    Ok((app, codex_home, target))
}

#[tokio::test]
async fn history_hydration_metadata_tracks_missing_turns_and_stale_completions_keep_new_request()
-> Result<()> {
    let (mut app, _codex_home, target) =
        history_fixture(&[1, 1, 1, 1, 0, 1, 0, 0, 0, 0, 0, 0, 1]).await?;
    app.local_settings.transcript_mode = TranscriptMode::Terminal;
    app.local_settings.tui.terminal_resize_reflow_max_rows = Some(1);
    let (mut server, requests, proxy) = start_recording_app_server(
        &app.config,
        /*blocked_thread_list*/ None,
        /*failed_thread_name*/ None,
    )
    .await?;
    // Resume materializes the hand-written rollout into the server's item projection.
    // Exclude turns so the explicit hydration below remains the only history load.
    let _: ThreadResumeResponse = server
        .request_handle()
        .request_typed(ClientRequest::ThreadResume {
            request_id: server.next_request_id(),
            params: ThreadResumeParams {
                thread_id: target.thread_id.to_string(),
                exclude_turns: true,
                ..ThreadResumeParams::default()
            },
        })
        .await?;
    let mut thread = server
        .thread_read(target.thread_id, /*include_turns*/ false)
        .await?;
    server
        .hydrate_initial_thread_history(
            &mut thread,
            /*turn_cursor*/ None,
            /*item_cursor*/ None,
            Some(&app.config),
            Some(&app.local_settings),
            HistoryHydrationScope::Initial,
        )
        .await?;
    assert_eq!(
        thread
            .turns
            .iter()
            .flat_map(|turn| &turn.items)
            .map(ThreadItem::id)
            .collect::<Vec<_>>(),
        vec!["item-12-0"]
    );
    let old_cursor = server
        .begin_older_history_page(target.thread_id)
        .expect("older page");
    let page = server
        .thread_items_page(
            target.thread_id,
            /*turn_id*/ None,
            Some(old_cursor.clone()),
            /*limit*/ 2,
        )
        .await?;
    let items = server
        .apply_older_history_page(target.thread_id, &old_cursor, page, &mut thread.turns)
        .await?;
    assert_eq!(
        items.iter().map(ThreadItem::id).collect::<Vec<_>>(),
        vec!["item-3-0", "item-5-0"]
    );
    assert_eq!(
        thread
            .turns
            .iter()
            .flat_map(|turn| &turn.items)
            .map(ThreadItem::id)
            .collect::<Vec<_>>(),
        vec!["item-3-0", "item-5-0", "item-12-0"]
    );
    let metadata = recorded_params(&requests, "thread/turns/list");
    assert_eq!(
        metadata
            .iter()
            .map(|params| params["limit"].as_u64().unwrap())
            .collect::<Vec<_>>(),
        vec![5, 2, 2, 1]
    );
    assert_eq!(
        recorded_params(&requests, "thread/read")
            .into_iter()
            .map(serde_json::from_value::<ThreadReadParams>)
            .collect::<Result<Vec<_>, _>>()?,
        vec![ThreadReadParams {
            thread_id: target.thread_id.to_string(),
            include_turns: false,
        }]
    );

    let cursor = server
        .begin_older_history_page(target.thread_id)
        .expect("next page");
    assert_ne!(old_cursor, cursor);
    let mut tui = crate::tui::test_support::make_test_tui()?;
    // The response is stale even when its thread is no longer displayed. Ignore its error first.
    app.transcript_view.history = TranscriptHistoryState::LoadingOlder;
    for result in [
        Err("obsolete transport failure".to_string()),
        Ok(ThreadItemsListResponse {
            data: Vec::new(),
            next_cursor: None,
            backwards_cursor: None,
        }),
    ] {
        app.handle_event(
            &mut tui,
            &mut server,
            AppEvent::OlderThreadHistoryLoaded {
                generation: app.history_window.generation,
                token: None,
                thread_id: target.thread_id,
                cursor: old_cursor.clone(),
                result,
            },
        )
        .await?;
        assert!(server.is_older_history_page_pending(target.thread_id, &cursor));
        assert_eq!(
            app.transcript_view.history,
            TranscriptHistoryState::LoadingOlder
        );
    }
    server.shutdown().await?;
    proxy.await??;
    Ok(())
}

#[tokio::test]
async fn history_hydration_archived_retry_uses_first_attempt_runtime_settings() -> Result<()> {
    for action in [
        SessionStartAction::Resume(
            crate::app_server_session::ResumeModelSettings::RestoreFromThread,
            crate::resume_permissions::ResumePermissions::default(),
        ),
        SessionStartAction::Fork(crate::app_server_session::ForkPermissionMode::InheritSaved),
    ] {
        for mode in [TranscriptMode::Terminal, TranscriptMode::Owned] {
            let (mut app, codex_home, mut target) = history_fixture(&[500]).await?;
            app.config.tui_fullscreen_transcript = !mode.is_owned();
            app.config.tui_alternate_screen = codex_config::types::AltScreenMode::Always;
            app.config.terminal_resize_reflow.max_rows = TerminalResizeReflowMaxRows::Limit(1);
            app.local_settings = crate::local_settings::LocalSettings::from(&app.config);
            app.local_settings.transcript_mode = mode;
            let active_path = target.path.take().unwrap();
            let archived = codex_home.path().join("archived_sessions");
            std::fs::create_dir_all(&archived)?;
            let path = archived.join(active_path.file_name().unwrap());
            std::fs::rename(active_path, &path)?;
            target.path = Some(path);
            let (mut server, requests, proxy) = start_recording_app_server(
                &app.config,
                /*blocked_thread_list*/ None,
                /*failed_thread_name*/ None,
            )
            .await?;
            let initial = match action {
                SessionStartAction::Resume(settings, _) => {
                    server
                        .resume_thread(
                            &app.local_settings,
                            app.config.clone(),
                            target.thread_id,
                            settings,
                        )
                        .await
                }
                SessionStartAction::Fork(permission) => {
                    server
                        .fork_thread_with_permission_mode(
                            &app.local_settings,
                            app.config.clone(),
                            target.thread_id,
                            permission,
                        )
                        .await
                }
            };
            assert!(initial.is_err());
            target.backend_id = server.backend_id();
            let outcome = complete_session_start(
                &mut server,
                SessionStartConfig {
                    config: &app.config,
                    local_settings: &app.local_settings,
                },
                &AppServerTarget::Embedded,
                &target,
                action,
                initial,
                async || Ok(crate::unarchive_prompt::UnarchiveChoice::Unarchive),
            )
            .await?;
            let SessionStartOutcome::Started(started) = outcome else {
                panic!("retry starts session")
            };
            let count = started
                .turns
                .iter()
                .map(|turn| turn.items.len())
                .sum::<usize>();
            if mode.is_owned() {
                assert!(count > 1, "owned retry must use viewport budget");
            } else {
                assert_eq!(count, 1);
            }
            assert!(server.has_older_history(started.session.thread_id));
            let item_requests = recorded_params(&requests, "thread/items/list");
            let initial_limit = item_requests[0]["limit"].as_u64().unwrap();
            assert_eq!(initial_limit == 1, !mode.is_owned());
            assert_eq!(recorded_params(&requests, "thread/unarchive").len(), 1);
            server.shutdown().await?;
            proxy.await??;
        }
    }
    Ok(())
}

#[tokio::test]
async fn official_history_pages_reuse_global_cursor_and_fence_superseded_latest() -> Result<()> {
    use crate::app_server_session::HistoryDirection;
    let (mut app, _home, target) = history_fixture(&[1; 12]).await?;
    app.local_settings.transcript_mode = TranscriptMode::Terminal;
    app.local_settings.tui.terminal_resize_reflow_max_rows = Some(1);
    let (mut server, requests, proxy) = start_recording_app_server(&app.config, None, None).await?;
    let _: ThreadResumeResponse = server
        .request_handle()
        .request_typed(ClientRequest::ThreadResume {
            request_id: server.next_request_id(),
            params: ThreadResumeParams {
                thread_id: target.thread_id.to_string(),
                exclude_turns: true,
                ..Default::default()
            },
        })
        .await?;
    let mut thread = server.thread_read(target.thread_id, false).await?;
    server
        .hydrate_initial_thread_history(
            &mut thread,
            None,
            None,
            Some(&app.config),
            Some(&app.local_settings),
            HistoryHydrationScope::Initial,
        )
        .await?;
    assert_eq!(
        thread.turns.len(),
        crate::app_server_session::INITIAL_HISTORY_TURN_LIMIT as usize
    );
    assert_eq!(thread.turns.iter().flat_map(|turn| &turn.items).count(), 1);
    let older = server
        .begin_history_page(target.thread_id, HistoryDirection::Older)
        .unwrap();
    assert!(
        older.params.turn_id.is_none(),
        "ordinary history uses the official global item cursor"
    );
    assert!(matches!(
        older.params.cursor,
        Some(codex_app_server_protocol::ThreadItemsListCursor::Opaque(_))
    ));
    let stale = older.token;
    let latest = server
        .begin_history_page(target.thread_id, HistoryDirection::Latest)
        .unwrap();
    assert!(!server.is_history_token_pending(target.thread_id, &stale));
    assert!(server.is_history_token_pending(target.thread_id, &latest.token));
    let page: ThreadItemsListResponse = server
        .request_handle()
        .request_typed(ClientRequest::ThreadItemsList {
            request_id: latest.request_id,
            params: latest.params,
        })
        .await?;
    server
        .apply_older_history_page(target.thread_id, &latest.token, page, &mut thread.turns)
        .await?;
    assert!(!server.is_history_token_pending(target.thread_id, &latest.token));
    assert!(
        thread
            .turns
            .iter()
            .flat_map(|turn| &turn.items)
            .any(|item| item.id() == "item-11-0")
    );
    assert!(
        recorded_params(&requests, "thread/items/list")
            .iter()
            .all(|params| params["limit"].as_u64().unwrap() <= 100)
    );
    assert!(
        recorded_params(&requests, "thread/read")
            .iter()
            .all(
                |params| !serde_json::from_value::<ThreadReadParams>(params.clone())
                    .expect("recorded thread/read params")
                    .include_turns
            )
    );
    server.shutdown().await?;
    proxy.await??;
    Ok(())
}

#[tokio::test]
async fn explicit_complete_history_does_not_inherit_the_display_item_budget() -> Result<()> {
    let (app, _home, target) = history_fixture(&[450]).await?;
    let (mut server, requests, proxy) = start_recording_app_server(&app.config, None, None).await?;
    let _: ThreadResumeResponse = server
        .request_handle()
        .request_typed(ClientRequest::ThreadResume {
            request_id: server.next_request_id(),
            params: ThreadResumeParams {
                thread_id: target.thread_id.to_string(),
                exclude_turns: true,
                ..Default::default()
            },
        })
        .await?;
    let mut thread = server.thread_read(target.thread_id, false).await?;
    server
        .hydrate_initial_thread_history(
            &mut thread,
            None,
            None,
            None,
            None,
            HistoryHydrationScope::Complete,
        )
        .await?;
    assert_eq!(
        thread.turns.iter().flat_map(|turn| &turn.items).count(),
        450
    );
    assert!(recorded_params(&requests, "thread/items/list").len() >= 5);
    let mut anchored = server.thread_read(target.thread_id, false).await?;
    server
        .hydrate_initial_thread_history(
            &mut anchored,
            None,
            None,
            None,
            None,
            HistoryHydrationScope::ThroughTurn("turn-0"),
        )
        .await?;
    assert_eq!(
        anchored.turns.iter().flat_map(|turn| &turn.items).count(),
        450
    );
    server.shutdown().await?;
    proxy.await??;
    Ok(())
}

#[tokio::test]
async fn fifty_turn_window_returns_through_every_evicted_turn_in_both_directions() -> Result<()> {
    use crate::app_server_session::HistoryDirection;
    let (mut app, _home, target) = history_fixture(&[1; 50]).await?;
    app.local_settings.transcript_mode = TranscriptMode::Terminal;
    app.local_settings.tui.terminal_resize_reflow_max_rows = Some(1);
    let (mut server, requests, proxy) = start_recording_app_server(&app.config, None, None).await?;
    let _: ThreadResumeResponse = server
        .request_handle()
        .request_typed(ClientRequest::ThreadResume {
            request_id: server.next_request_id(),
            params: ThreadResumeParams {
                thread_id: target.thread_id.to_string(),
                exclude_turns: true,
                ..Default::default()
            },
        })
        .await?;
    let mut thread = server.thread_read(target.thread_id, false).await?;
    server
        .hydrate_initial_thread_history(
            &mut thread,
            None,
            None,
            Some(&app.config),
            Some(&app.local_settings),
            HistoryHydrationScope::Initial,
        )
        .await?;
    let older = server
        .begin_history_page(target.thread_id, HistoryDirection::Older)
        .unwrap();
    assert!(older.params.turn_id.is_none());
    let page: ThreadItemsListResponse = server
        .request_handle()
        .request_typed(ClientRequest::ThreadItemsList {
            request_id: older.request_id,
            params: older.params,
        })
        .await?;
    server
        .apply_older_history_page(target.thread_id, &older.token, page, &mut thread.turns)
        .await?;
    // This is the canonical source window's oldest-edge retention policy, not a server limit.
    thread
        .turns
        .truncate(crate::history_window::MAX_HISTORY_TURNS);
    assert_eq!(thread.turns.last().unwrap().id, "turn-29");
    server.reanchor_history_window(target.thread_id, &thread.turns, HistoryDirection::Older);
    let mut traversed = Vec::new();
    for _ in 0..64 {
        let Some(request) = server.begin_history_page(target.thread_id, HistoryDirection::Newer)
        else {
            break;
        };
        let page: ThreadItemsListResponse = server
            .request_handle()
            .request_typed(ClientRequest::ThreadItemsList {
                request_id: request.request_id,
                params: request.params,
            })
            .await?;
        let items = server
            .apply_older_history_page(target.thread_id, &request.token, page, &mut thread.turns)
            .await?;
        traversed.extend(items.into_iter().map(|item| item.id().to_string()));
        let excess = thread
            .turns
            .len()
            .saturating_sub(crate::history_window::MAX_HISTORY_TURNS);
        thread.turns.drain(..excess);
        server.reanchor_history_window(target.thread_id, &thread.turns, HistoryDirection::Newer);
    }
    assert!(!server.has_newer_history(target.thread_id));
    assert_eq!(
        traversed,
        (30..50)
            .map(|index| format!("item-{index}-0"))
            .collect::<Vec<_>>()
    );
    assert_eq!(thread.turns.first().unwrap().id, "turn-20");
    let mut returned = Vec::new();
    for _ in 0..128 {
        let Some(request) = server.begin_history_page(target.thread_id, HistoryDirection::Older)
        else {
            break;
        };
        let page: ThreadItemsListResponse = server
            .request_handle()
            .request_typed(ClientRequest::ThreadItemsList {
                request_id: request.request_id,
                params: request.params,
            })
            .await?;
        returned.extend(
            server
                .apply_older_history_page(target.thread_id, &request.token, page, &mut thread.turns)
                .await?
                .into_iter()
                .map(|item| item.id().to_string()),
        );
        thread
            .turns
            .truncate(crate::history_window::MAX_HISTORY_TURNS);
        server.reanchor_history_window(target.thread_id, &thread.turns, HistoryDirection::Older);
    }
    assert!(!server.has_older_history(target.thread_id));
    assert_eq!(
        returned,
        (0..20)
            .rev()
            .map(|index| format!("item-{index}-0"))
            .collect::<Vec<_>>()
    );
    let metadata = recorded_params(&requests, "thread/turns/list");
    assert!(
        metadata
            .iter()
            .any(|params| params["sortDirection"] == "asc")
    );
    assert!(
        metadata
            .iter()
            .all(|params| params["itemsView"] == "notLoaded")
    );
    server.shutdown().await?;
    proxy.await??;
    Ok(())
}

#[tokio::test]
async fn oversized_display_item_stays_available_to_complete_and_anchor_queries() -> Result<()> {
    use crate::app_server_session::HistoryDirection;
    let (mut app, _home, target) = history_fixture(&[1, 1]).await?;
    app.local_settings.transcript_mode = TranscriptMode::Owned;
    let path = target.path.as_ref().unwrap();
    let mut records = std::fs::read_to_string(path)?
        .lines()
        .map(serde_json::from_str::<serde_json::Value>)
        .collect::<Result<Vec<_>, _>>()?;
    let size = crate::history_window::MAX_HISTORY_BYTES + 1024;
    for record in &mut records {
        if record["type"] != "event_msg" {
            continue;
        }
        let mut event: EventMsg = serde_json::from_value(record["payload"].clone())?;
        if let EventMsg::ItemCompleted(completed) = &mut event
            && completed.turn_id == "turn-1"
            && let TurnItem::UserMessage(item) = &mut completed.item
        {
            item.content = vec![CoreUserInput::Text {
                text: "x".repeat(size),
                text_elements: Vec::new(),
            }];
            record["payload"] = serde_json::to_value(event)?;
        }
    }
    let contents = records
        .into_iter()
        .map(|record| record.to_string())
        .collect::<Vec<_>>()
        .join("\n");
    std::fs::write(path, format!("{contents}\n"))?;
    drop(contents);
    let (mut server, _requests, proxy) =
        start_recording_app_server(&app.config, None, None).await?;
    let _: ThreadResumeResponse = server
        .request_handle()
        .request_typed(ClientRequest::ThreadResume {
            request_id: server.next_request_id(),
            params: ThreadResumeParams {
                thread_id: target.thread_id.to_string(),
                exclude_turns: true,
                ..Default::default()
            },
        })
        .await?;
    let mut initial = server.thread_read(target.thread_id, false).await?;
    server
        .hydrate_initial_thread_history(
            &mut initial,
            None,
            None,
            Some(&app.config),
            Some(&app.local_settings),
            HistoryHydrationScope::Initial,
        )
        .await?;
    assert_eq!(
        server.take_history_errors(target.thread_id),
        [crate::history_window::OVERSIZED_HISTORY]
    );
    assert!(initial.turns.iter().all(|turn| turn.items.is_empty()));
    let request = server
        .begin_history_page(target.thread_id, HistoryDirection::Older)
        .unwrap();
    assert_eq!(request.params.turn_id.as_deref(), Some("turn-0"));
    let page: ThreadItemsListResponse = server
        .request_handle()
        .request_typed(ClientRequest::ThreadItemsList {
            request_id: request.request_id,
            params: request.params,
        })
        .await?;
    let items = server
        .apply_older_history_page(target.thread_id, &request.token, page, &mut initial.turns)
        .await?;
    assert_eq!(items[0].id(), "item-0-0");
    for scope in [
        HistoryHydrationScope::Complete,
        HistoryHydrationScope::ThroughTurn("turn-1"),
    ] {
        let mut complete = server.thread_read(target.thread_id, false).await?;
        server
            .hydrate_initial_thread_history(&mut complete, None, None, None, None, scope)
            .await?;
        let giant = complete
            .turns
            .iter()
            .flat_map(|turn| &turn.items)
            .find(|item| item.id() == "item-1-0")
            .unwrap();
        let ThreadItem::UserMessage { content, .. } = giant else {
            panic!("user item");
        };
        assert!(content.iter().any(|input| matches!(input, codex_app_server_protocol::UserInput::Text { text, .. } if text.len() == size)));
    }
    server.shutdown().await?;
    proxy.await??;
    Ok(())
}

#[tokio::test]
async fn live_unmaterialized_resume_keeps_session_input_and_incomplete_history() -> Result<()> {
    use super::session_lifecycle_requests::HistoryCapabilities;
    use super::session_lifecycle_requests::start_recording_app_server_with_history;
    use crate::app_server_session::ResumeModelSettings;
    use codex_app_server_client::AppServerEvent;
    use codex_app_server_protocol::ThreadHistoryMode;
    use codex_app_server_protocol::ThreadStartParams;
    use codex_app_server_protocol::ThreadStartResponse;
    use codex_app_server_protocol::UserInput;

    for history_mode in [ThreadHistoryMode::Legacy, ThreadHistoryMode::Paginated] {
        let (mut app, mut events, _) = make_test_app_with_channels().await;
        let home = tempdir()?;
        let model =
            app_test_support::create_mock_responses_server_repeating_assistant("Done").await;
        app_test_support::MockResponsesConfig::new(&model.uri()).write(home.path())?;
        app.config = ConfigBuilder::default()
            .codex_home(home.path().to_path_buf())
            .build()
            .await?;
        app.local_settings = crate::local_settings::LocalSettings::from(&app.config);
        app.local_settings.transcript_mode = TranscriptMode::Owned;
        let mut tui = crate::tui::test_support::make_test_tui()?;
        tui.set_owned_screen(true)?;
        let init = app.chatwidget_init_for_forked_or_resumed_thread(
            &mut tui,
            app.config.clone(),
            /*initial_user_message*/ None,
        );
        app.replace_chat_widget(ChatWidget::new_with_app_event(init));
        let (mut server, requests, proxy) = start_recording_app_server_with_history(
            &app.config,
            HistoryCapabilities::ForwardLiveEvents,
            None,
            None,
            crate::app_server_session::ThreadParamsMode::Remote,
            codex_config::LoaderOverrides::without_managed_config_for_tests(),
        )
        .await?;
        let backend_id = server.backend_id();
        let request_id = server.next_request_id();
        let started: ThreadStartResponse = server
            .request_handle()
            .request_typed(ClientRequest::ThreadStart {
                request_id,
                params: ThreadStartParams {
                    history_mode: Some(history_mode),
                    ..Default::default()
                },
            })
            .await?;
        let thread_id = ThreadId::from_string(&started.thread.id)?;
        let path = started.thread.path.as_ref().expect("local writer path");
        let resumed = server
            .resume_thread(
                &app.local_settings,
                app.config.clone(),
                thread_id,
                ResumeModelSettings::RestoreFromThread,
            )
            .await?;
        assert_eq!(resumed.session.thread_id, thread_id);
        assert!(resumed.turns.is_empty());
        assert!(server.has_incomplete_history(thread_id));
        assert!(!path.exists());
        assert!(model.received_requests().await.unwrap().is_empty());
        let before = server.thread_read(thread_id, false).await?;
        assert_eq!(before.session_id, started.thread.session_id);
        let original_session = resumed.session.clone();
        app.enqueue_primary_thread_session(resumed.session, resumed.turns)
            .await?;

        // Both empty attachments queue their End event. Consuming the older event must
        // use the active thread, never display the other thread's unavailable state.
        let normal = server.start_thread(&app.config).await?;
        let normal_id = normal.session.thread_id;
        app.store_active_thread_receiver().await;
        app.clear_active_thread().await;
        app.enqueue_primary_thread_session(normal.session, normal.turns)
            .await?;
        let mut end_events = 0;
        while let Ok(event) = events.try_recv() {
            end_events += usize::from(matches!(event, AppEvent::EndInitialHistoryReplayBuffer));
            app.handle_event(&mut tui, &mut server, event).await?;
        }
        assert_eq!(end_events, 2);
        assert_eq!(app.chat_widget.thread_id(), Some(normal_id));
        assert_eq!(
            app.transcript_view.history,
            TranscriptHistoryState::Complete
        );
        assert_eq!(app.active_thread_id, Some(normal_id));
        let home = crate::tui::TuiEvent::Key(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Home,
            crossterm::event::KeyModifiers::CONTROL,
        ));
        assert!(app.handle_owned_transcript_event(&mut tui, &mut server, &home)?);
        assert_eq!(
            app.transcript_view.history,
            TranscriptHistoryState::Complete
        );
        assert!(server.has_incomplete_history(thread_id));
        app.store_active_thread_receiver().await;
        app.clear_active_thread().await;
        app.enqueue_primary_thread_session(original_session, Vec::new())
            .await?;
        let mut displayed_errors = String::new();
        while let Ok(event) = events.try_recv() {
            if let AppEvent::InsertHistoryCell(cell) = &event {
                displayed_errors.push_str(&lines_to_single_string(&cell.display_lines(1000)));
            }
            app.handle_event(&mut tui, &mut server, event).await?;
        }
        assert!(
            displayed_errors.is_empty(),
            "정상 저장 대기는 오류 셀을 만들지 않는다"
        );
        assert_eq!(app.chat_widget.thread_id(), Some(thread_id));
        assert_ne!(app.transcript_view.history, TranscriptHistoryState::Failed);
        assert_ne!(
            app.transcript_view.history,
            TranscriptHistoryState::Complete
        );
        assert!(
            server.has_incomplete_history(thread_id),
            "저장 대기는 미조회 이력의 보호를 유지한다"
        );
        assert!(
            server.take_history_errors(thread_id).is_empty(),
            "정상 저장 대기는 표시할 오류를 남기지 않는다"
        );

        let history_requests_before_home = recorded_params(&requests, "thread/items/list").len();
        assert_eq!(app.active_thread_id, Some(thread_id));
        assert!(app.handle_owned_transcript_event(&mut tui, &mut server, &home)?);
        assert_ne!(app.transcript_view.history, TranscriptHistoryState::Failed);
        assert_ne!(
            app.transcript_view.history,
            TranscriptHistoryState::Complete
        );
        assert_eq!(
            recorded_params(&requests, "thread/items/list").len(),
            history_requests_before_home,
            "Home must not invent a retry without a cursor"
        );
        let footer = app
            .transcript_view
            .footer_with_navigation(100, crate::motion::MotionMode::Reduced, "")
            .expect("저장 대기 안내")
            .text
            .to_string();
        assert!(footer.contains("Waiting for history to be saved"));
        assert!(!footer.contains("Retry"));
        app.transcript_view.begin_search();
        app.transcript_view.paste_search("missing history");
        assert!(!app.transcript_view.advance_search(&app.transcript_cells));
        let search_footer = app
            .transcript_view
            .footer_with_navigation(100, crate::motion::MotionMode::Reduced, "")
            .expect("검색의 저장 대기 안내")
            .text
            .to_string();
        assert!(search_footer.contains("Waiting for history to be saved"));
        assert!(!search_footer.contains("Retry"));
        app.transcript_view.jump_to_latest();

        let approval_policy = app.config.permissions.approval_policy.value();
        let permission_profile = app.config.permissions.permission_profile().clone();
        app.handle_tui_event(
            &mut tui,
            &mut server,
            crate::tui::TuiEvent::Paste("First real input".to_string()),
        )
        .await?;
        assert_eq!(
            app.chat_widget.composer_text_with_pending(),
            "First real input"
        );
        app.handle_tui_event(
            &mut tui,
            &mut server,
            crate::tui::TuiEvent::Key(crossterm::event::KeyEvent::from(
                crossterm::event::KeyCode::Enter,
            )),
        )
        .await?;
        let mut submitted_user_turns = 0;
        while let Ok(event) = events.try_recv() {
            if matches!(&event, AppEvent::CodexOp(AppCommand::UserTurn { .. })) {
                submitted_user_turns += 1;
            }
            app.handle_event(&mut tui, &mut server, event).await?;
        }
        assert_eq!(
            submitted_user_turns, 1,
            "Enter submits through the app queue once"
        );
        assert_eq!(
            app.config.permissions.approval_policy.value(),
            approval_policy
        );
        assert_eq!(
            app.config.permissions.permission_profile(),
            &permission_profile
        );
        let turn_id = tokio::time::timeout(std::time::Duration::from_secs(30), async {
            let mut started_turn_id = None;
            loop {
                let event = server
                    .next_event()
                    .await
                    .expect("live event stream remains connected");
                let AppServerEvent::ServerNotification(notification) = event.event else {
                    continue;
                };
                let (notified_thread_id, notified_turn_id) = match notification.as_ref() {
                    ServerNotification::TurnStarted(event) => (&event.thread_id, &event.turn.id),
                    ServerNotification::TurnCompleted(event) => (&event.thread_id, &event.turn.id),
                    ServerNotification::ItemStarted(event) => (&event.thread_id, &event.turn_id),
                    ServerNotification::ItemCompleted(event) => (&event.thread_id, &event.turn_id),
                    _ => continue,
                };
                if notified_thread_id != &started.thread.id {
                    continue;
                }
                if matches!(notification.as_ref(), ServerNotification::TurnStarted(_)) {
                    assert!(
                        started_turn_id.replace(notified_turn_id.clone()).is_none(),
                        "one submitted input starts one turn"
                    );
                }
                let turn_id = started_turn_id
                    .as_ref()
                    .expect("turn starts before its items or completion");
                assert_eq!(notified_turn_id, turn_id);
                let completed =
                    matches!(notification.as_ref(), ServerNotification::TurnCompleted(_));
                let turn_id = turn_id.clone();
                app.enqueue_thread_notification(thread_id, *notification)
                    .await?;
                if completed {
                    break Ok::<_, color_eyre::Report>(turn_id);
                }
            }
        })
        .await??;
        let snapshot = app.thread_event_channels[&thread_id]
            .store
            .lock()
            .await
            .snapshot();
        assert!(
            snapshot.turns.is_empty(),
            "live events do not rewrite the hydration snapshot"
        );
        assert!(snapshot.events.iter().any(
            |event| matches!(event, ThreadBufferedEvent::Notification(notification)
                if matches!(notification.as_ref(), ServerNotification::TurnStarted(event)
                    if event.thread_id == started.thread.id && event.turn.id == turn_id))
        ));
        assert!(snapshot.events.iter().any(
            |event| matches!(event, ThreadBufferedEvent::Notification(notification)
                if matches!(notification.as_ref(), ServerNotification::TurnCompleted(event)
                    if event.thread_id == started.thread.id && event.turn.id == turn_id
                        && event.turn.status == codex_app_server_protocol::TurnStatus::Completed))
        ));
        assert_eq!(snapshot.events.iter().filter(|event|
            matches!(event, ThreadBufferedEvent::Notification(notification)
                if matches!(notification.as_ref(), ServerNotification::ItemCompleted(event)
                    if event.thread_id == started.thread.id && event.turn_id == turn_id
                        && matches!(&event.item, ThreadItem::UserMessage { content, .. }
                            if content.iter().any(|input| matches!(input, UserInput::Text { text, .. }
                                if text == "First real input")))))).count(), 1);
        assert_eq!(
            snapshot
                .events
                .iter()
                .filter(
                    |event| matches!(event, ThreadBufferedEvent::Notification(notification)
                if matches!(notification.as_ref(), ServerNotification::ItemCompleted(event)
                    if event.thread_id == started.thread.id && event.turn_id == turn_id
                        && matches!(&event.item, ThreadItem::AgentMessage { text, .. }
                            if text == "Done")))
                )
                .count(),
            1
        );

        // Use the main loop's existing drain path: active receiver -> ChatWidget -> app queue.
        while app
            .active_thread_rx
            .as_ref()
            .is_some_and(|rx| !rx.is_empty())
        {
            app.drain_active_thread_events(&mut tui).await?;
            while let Ok(event) = events.try_recv() {
                app.handle_event(&mut tui, &mut server, event).await?;
            }
        }
        assert_eq!(app.chat_widget.last_turn_id(), Some(turn_id.as_str()));
        let displayed = app
            .transcript_cells
            .iter()
            .map(|cell| lines_to_single_string(&cell.transcript_lines(1000)))
            .collect::<Vec<_>>()
            .join("\n");
        assert_eq!(displayed.matches("First real input").count(), 1);
        assert_eq!(displayed.matches("Done").count(), 1);

        assert!(
            server.has_incomplete_history(thread_id),
            "live events do not prove earlier history complete"
        );
        assert_ne!(app.transcript_view.history, TranscriptHistoryState::Failed);
        assert_ne!(
            app.transcript_view.history,
            TranscriptHistoryState::Complete
        );
        assert_eq!(server.backend_id(), backend_id);
        assert_eq!(
            server.thread_read(thread_id, false).await?.session_id,
            started.thread.session_id
        );
        let submitted = recorded_params(&requests, "turn/start");
        assert_eq!(submitted.len(), 1);
        assert_eq!(submitted[0]["threadId"], started.thread.id);
        assert_eq!(submitted[0]["input"][0]["text"], "First real input");
        assert_eq!(model.received_requests().await.unwrap().len(), 1);

        // Explicit successful hydration can clear the marker; there is no automatic retry.
        let rehydrated = server
            .resume_thread(
                &app.local_settings,
                app.config.clone(),
                thread_id,
                ResumeModelSettings::RestoreFromThread,
            )
            .await?;
        assert!(!server.has_incomplete_history(thread_id));
        assert!(server.take_history_errors(thread_id).is_empty());
        app.store_active_thread_receiver().await;
        app.clear_active_thread().await;
        app.enqueue_primary_thread_session(rehydrated.session.clone(), rehydrated.turns.clone())
            .await?;
        while let Ok(event) = events.try_recv() {
            app.handle_event(&mut tui, &mut server, event).await?;
        }
        assert_eq!(
            app.transcript_view.history,
            TranscriptHistoryState::Complete
        );
        assert!(rehydrated.turns.iter().flat_map(|turn| &turn.items).any(|item|
            matches!(item, ThreadItem::UserMessage { content, .. } if content.iter().any(|input|
                matches!(input, UserInput::Text { text, .. } if text == "First real input")))));
        server.shutdown().await?;
        proxy.await??;
    }
    Ok(())
}

#[tokio::test]
async fn generic_projection_unavailable_survives_resume_and_error_display() -> Result<()> {
    for reason in [None, Some("unknown"), Some("deleted")] {
        check_projection_unavailable_error_display(reason).await?;
    }
    Ok(())
}

async fn check_projection_unavailable_error_display(reason: Option<&'static str>) -> Result<()> {
    use super::session_lifecycle_requests::HistoryCapabilities;
    use super::session_lifecycle_requests::start_recording_app_server_with_history;
    use crate::app_server_session::ResumeModelSettings;
    let (mut app, mut events, _) = make_test_app_with_channels().await;
    let (_fixture, home, target) = history_fixture(&[1]).await?;
    app.config.codex_home = home.path().to_path_buf().abs();
    app.config.sqlite = SqliteConfig::new_for_testing(home.path().abs());
    app.local_settings.transcript_mode = TranscriptMode::Owned;
    let (mut server, _, proxy) = start_recording_app_server_with_history(
        &app.config,
        HistoryCapabilities::ItemsProjectionUnavailable(reason),
        None,
        None,
        crate::app_server_session::ThreadParamsMode::Remote,
        codex_config::LoaderOverrides::without_managed_config_for_tests(),
    )
    .await?;
    let resumed = server
        .resume_thread(
            &app.local_settings,
            app.config.clone(),
            target.thread_id,
            ResumeModelSettings::RestoreFromThread,
        )
        .await?;
    assert!(
        !resumed.turns.is_empty(),
        "already loaded turn metadata must be preserved"
    );
    assert!(server.has_incomplete_history(target.thread_id));
    app.enqueue_primary_thread_session(resumed.session, resumed.turns)
        .await?;
    let mut tui = crate::tui::test_support::make_test_tui()?;
    tui.set_owned_screen(true)?;
    let mut displayed_errors = String::new();
    while let Ok(event) = events.try_recv() {
        if let AppEvent::InsertHistoryCell(cell) = &event {
            displayed_errors.push_str(&lines_to_single_string(&cell.display_lines(1000)));
        }
        app.handle_event(&mut tui, &mut server, event).await?;
    }
    assert!(
        displayed_errors
            .contains("display history projection is unavailable; retry after history persistence")
    );
    assert_eq!(app.chat_widget.thread_id(), Some(target.thread_id));
    assert_eq!(app.transcript_view.history, TranscriptHistoryState::Failed);
    assert!(server.has_incomplete_history(target.thread_id));
    assert!(server.take_history_errors(target.thread_id).is_empty());
    assert!(
        !app.thread_event_channels[&target.thread_id]
            .store
            .lock()
            .await
            .turns
            .is_empty()
    );
    server.shutdown().await?;
    proxy.await??;
    Ok(())
}
