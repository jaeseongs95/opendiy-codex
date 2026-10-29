// Modified for OpenDIY Codex R28 (2026-10-29); see MODIFICATIONS.md.
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
            .all(|params| params["includeTurns"] == false)
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
    let mut returned = Vec::new();
    for _ in 0..4 {
        let request = server
            .begin_history_page(target.thread_id, HistoryDirection::Older)
            .unwrap();
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
                .await?,
        );
        if !returned.is_empty() {
            break;
        }
    }
    assert_eq!(returned.first().unwrap().id(), "item-28-0");
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
