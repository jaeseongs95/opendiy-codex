// Modified for OpenDIY Codex R31 (2026-11-01); see MODIFICATIONS.md.
//! Older pages must preserve newer events and identify repeated hidden prompts precisely.

use super::*;
use crate::app::test_support::make_test_app;
use crate::history_cell::ComputerActivityCell;
use crate::history_cell::FinalMessageSeparator;
use crate::history_cell::ForkedThreadHistoryCell;
use codex_app_server_protocol::ThreadItem;
use codex_app_server_protocol::TurnItemsView;
use codex_app_server_protocol::UserInput;
use pretty_assertions::assert_eq;

fn turn(id: &str, status: TurnStatus, item_ids: &[&str]) -> Turn {
    Turn {
        id: id.to_string(),
        root_turn_id: None,
        items: item_ids
            .iter()
            .map(|id| ThreadItemPayload::UserMessage {
                id: id.to_string(),
                client_id: None,
                content: Vec::new(),
            })
            .map(ThreadItem::untimed)
            .collect(),
        items_view: TurnItemsView::Full,
        status,
        error: None,
        started_at: None,
        completed_at: None,
        duration_ms: None,
    }
}

#[test]
fn overlapping_history_keeps_live_turn_state_and_newer_items() {
    let mut current = vec![turn("shared", TurnStatus::InProgress, &["overlap", "live"])];
    let older = vec![
        turn("old", TurnStatus::Completed, &["first"]),
        turn("shared", TurnStatus::Completed, &["before", "overlap"]),
    ];
    merge_older_turns(&mut current, older);
    assert_eq!(
        current,
        vec![
            turn("old", TurnStatus::Completed, &["first"]),
            turn(
                "shared",
                TurnStatus::InProgress,
                &["before", "overlap", "live"]
            ),
        ],
    );
}

fn user_cell(message: &str) -> Arc<dyn HistoryCell> {
    Arc::new(UserHistoryCell {
        presentation: Default::default(),
        message: message.to_string(),
        text_elements: Vec::new(),
        local_image_paths: Vec::new(),
        remote_image_urls: Vec::new(),
    })
}

#[tokio::test]
async fn fork_lineage_stays_before_repeated_older_history_pages() {
    let mut app = make_test_app().await;
    app.transcript_cells = vec![
        Arc::new(ForkedThreadHistoryCell(
            crate::history_cell::PlainHistoryCell::new(vec!["Thread forked from parent".into()]),
        )),
        Arc::new(crate::history_cell::PlainHistoryCell::new(vec![
            "recent history".into(),
        ])),
    ];

    app.prepend_older_transcript_cells(vec![Arc::new(crate::history_cell::PlainHistoryCell::new(
        vec!["older history".into()],
    ))]);
    app.prepend_older_transcript_cells(vec![Arc::new(crate::history_cell::PlainHistoryCell::new(
        vec!["oldest history".into()],
    ))]);

    let rendered = app
        .transcript_cells
        .iter()
        .flat_map(|cell| cell.display_lines(/*width*/ 80))
        .map(|line| {
            line.spans
                .into_iter()
                .map(|span| span.content.into_owned())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");
    insta::assert_snapshot!(rendered, @r###"
    Thread forked from parent
    oldest history
    older history
    recent history
    "###);
}

#[test]
fn repeated_prompt_matching_removes_only_the_hidden_occurrence() {
    let cells = vec![user_cell("repeat"), user_cell("other"), user_cell("repeat")];
    let persisted = vec![
        ("unloaded".to_string(), user_cell("repeat")),
        ("hidden".to_string(), user_cell("repeat")),
        ("middle".to_string(), user_cell("other")),
        ("visible".to_string(), user_cell("repeat")),
    ];
    assert_eq!(
        hidden_transcript_indices(&cells, persisted, &HashSet::from(["hidden", "unloaded"])),
        vec![0],
    );
}

#[tokio::test]
async fn hidden_last_item_keeps_turn_groups_and_completion_boundaries() {
    let computer = |id: &str| {
        serde_json::from_value(serde_json::json!({
            "type": "mcpToolCall", "id": id, "server": "cua_repl", "tool": "js",
            "status": "completed", "arguments": {"title": format!("Inspect {id}")},
            "appContext": null, "pluginId": null, "readOnlyHint": null,
            "result": {"content": [], "structuredContent": null},
            "error": null, "durationMs": 10
        }))
        .expect("valid completed computer call")
    };
    for status in [
        TurnStatus::Completed,
        TurnStatus::Failed,
        TurnStatus::Interrupted,
    ] {
        let mut app = make_test_app().await;
        let cwd = app.config.cwd.clone();
        let thread_id = ThreadId::new();
        let expected_completions = usize::from(status == TurnStatus::Completed);
        let mut first = turn("first", status, &[]);
        first.items = vec![
            ThreadItem::untimed(ThreadItemPayload::EnteredReviewMode {
                id: "enter-review".to_string(),
                review: "current changes".to_string(),
            }),
            computer("older-call"),
            ThreadItem::untimed(ThreadItemPayload::UserMessage {
                id: "hidden".to_string(),
                client_id: None,
                content: vec![UserInput::Text {
                    text: "internal review prompt".to_string(),
                    text_elements: Vec::new(),
                }],
            }),
        ];
        first.completed_at = Some(1_700_000_000);
        first.duration_ms = Some(125_000);
        let mut second = turn("second", TurnStatus::InProgress, &[]);
        second.items = vec![computer("newer-call")];
        let turns = vec![first, second];
        let hidden = hidden_review_item_ids(&turns);
        assert_eq!(hidden, HashSet::from(["hidden"]));
        let items = turns
            .iter()
            .flat_map(Turn::item_payloads)
            .cloned()
            .collect::<Vec<_>>();

        let cells = app.project_older_history_cells(
            items.clone(),
            &turns,
            &hidden,
            thread_id,
            &cwd,
            RawReasoningVisibility::Hidden,
        );
        let groups = cells
            .iter()
            .filter_map(|cell| cell.as_any().downcast_ref::<ComputerActivityCell>())
            .map(|cell| cell.call_ids().map(str::to_string).collect::<Vec<_>>())
            .collect::<Vec<_>>();
        let completions = cells
            .iter()
            .filter(|cell| cell.as_any().is::<FinalMessageSeparator>())
            .count();
        let visible_users = cells
            .iter()
            .filter(|cell| cell.as_any().is::<UserHistoryCell>())
            .count();
        assert_eq!(
            (groups, completions, visible_users),
            (
                vec![
                    vec!["older-call".to_string()],
                    vec!["newer-call".to_string()]
                ],
                expected_completions,
                0,
            ),
        );

        let repeated = app.project_older_history_cells(
            items,
            &turns,
            &hidden,
            thread_id,
            &cwd,
            RawReasoningVisibility::Hidden,
        );
        assert_eq!(
            repeated
                .iter()
                .filter(|cell| cell.as_any().is::<FinalMessageSeparator>())
                .count(),
            0,
        );
    }
}

#[tokio::test]
async fn browsing_waits_for_a_prompt_outside_the_initial_history_window() -> Result<()> {
    let mut app = make_test_app().await;
    let mut tui = crate::tui::test_support::make_test_tui()?;
    tui.set_owned_screen(/*owned*/ true)?;
    app.scrollback_has_older_history = true;
    app.transcript_cells = vec![
        user_cell(""),
        Arc::new(crate::history_cell::PlainHistoryCell::new(vec![
            "recent answer".into(),
        ])),
    ];
    app.handle_backtrack_esc_key(&mut tui);
    app.handle_backtrack_esc_key(&mut tui);
    assert!(app.backtrack.overlay_preview_active && app.browsing_needs_history());
    app.prepend_older_transcript_cells(vec![
        user_cell(""),
        Arc::new(crate::history_cell::PlainHistoryCell::new(vec![
            "earlier answer".into(),
        ])),
    ]);
    assert!(app.browsing_needs_history());
    app.prepend_older_transcript_cells(vec![
        user_cell(""),
        user_cell("older prompt"),
        user_cell("latest prompt"),
    ]);
    app.apply_backtrack_selection_internal(app.backtrack.nth_user_message);
    assert_eq!(
        (
            app.backtrack.nth_user_message,
            crate::app_backtrack::nth_user_position(
                &app.transcript_cells,
                app.backtrack.nth_user_message
            )
        ),
        (1, Some(2))
    );
    assert!(!app.browsing_needs_history());
    app.apply_backtrack_selection_internal(/*nth_user_message*/ 0);
    let mut review_turn = turn(
        "review",
        TurnStatus::Completed,
        &["older prompt", "latest prompt"],
    );
    for item in &mut review_turn.items {
        if let ThreadItemPayload::UserMessage { id, content, .. } = &mut item.payload {
            *content = vec![UserInput::Text {
                text: id.clone(),
                text_elements: Vec::new(),
            }];
        }
    }
    review_turn.items.insert(
        /*index*/ 0,
        ThreadItem::untimed(ThreadItemPayload::EnteredReviewMode {
            id: "enter".to_string(),
            review: "review".to_string(),
        }),
    );
    review_turn.items.insert(
        /*index*/ 2,
        ThreadItem::untimed(ThreadItemPayload::ExitedReviewMode {
            id: "exit".to_string(),
            review: "review".to_string(),
        }),
    );
    let turns = [review_turn];
    app.remove_hidden_review_cells(
        &mut tui,
        &turns,
        &hidden_review_item_ids(&turns),
        ThreadId::new(),
        &app.config.cwd.clone(),
        RawReasoningVisibility::Hidden,
    );
    assert!(!app.backtrack.overlay_preview_active);
    assert_eq!(
        app.transcript_cells
            .iter()
            .filter_map(|cell| cell.as_any().downcast_ref::<UserHistoryCell>())
            .filter(|cell| !cell.message.is_empty())
            .map(|cell| cell.message.as_str())
            .collect::<Vec<_>>(),
        vec!["latest prompt"]
    );
    tui.set_owned_screen(/*owned*/ false)?;
    Ok(())
}

#[tokio::test]
async fn history_window_releases_distant_cells_and_frozen_readers() {
    let mut app = make_test_app().await;
    let old = user_cell("old prompt");
    let weak = Arc::downgrade(&old);
    app.transcript_cells.push(old);
    app.transcript_view.jump_to_entry(&app.transcript_cells, 0);
    let bookmark = app.transcript_view.bookmark(&app.transcript_cells);
    for _ in 0..450 {
        app.transcript_cells.push(user_cell("recent prompt"));
    }
    app.transcript_view.jump_to_latest();
    app.bound_history_window(/*prefer_older*/ false);
    assert!(app.transcript_cells.len() <= crate::history_window::MAX_HISTORY_ITEMS);
    assert!(
        weak.upgrade().is_some(),
        "the independent bookmark owns its revision"
    );
    // A caller-owned operation ends explicitly; its snapshot is not a display-cache owner.
    drop(bookmark);
    assert!(
        weak.upgrade().is_none(),
        "eviction must release every strong owner"
    );
}

#[test]
fn display_payload_budget_rejects_one_giant_item_without_copying_it() {
    let item: ThreadItem = serde_json::from_value(serde_json::json!({
        "type": "agentMessage", "id": "huge",
        "text": "x".repeat(crate::history_window::MAX_HISTORY_BYTES + 1),
        "phase": null, "questions": null
    }))
    .expect("agent item");
    assert!(crate::history_window::payload_bytes(&item).is_err());
}

#[tokio::test]
async fn history_window_byte_budget_releases_older_source() {
    let mut app = make_test_app().await;
    let old: Arc<dyn HistoryCell> = Arc::new(crate::history_cell::AgentMarkdownCell::new(
        "a".repeat(5 * 1024 * 1024),
        app.config.cwd.as_path(),
    ));
    let weak = Arc::downgrade(&old);
    app.transcript_cells.push(old);
    app.transcript_cells
        .push(Arc::new(crate::history_cell::AgentMarkdownCell::new(
            "b".repeat(5 * 1024 * 1024),
            app.config.cwd.as_path(),
        )));
    assert!(app.bound_history_window(false));
    assert_eq!(app.transcript_cells.len(), 1);
    assert!(weak.upgrade().is_none());
}

#[test]
fn canonical_window_does_not_pin_an_entire_active_turn() {
    let mut current = vec![turn(
        "active",
        TurnStatus::InProgress,
        &(0..450).map(|_| "item").collect::<Vec<_>>(),
    )];
    bound_display_turns(&mut current, false);
    assert_eq!(current[0].status, TurnStatus::InProgress);
    assert_eq!(
        current[0].items.len(),
        crate::history_window::MAX_HISTORY_ITEMS
    );
}

#[tokio::test]
async fn display_window_counts_group_members_and_releases_older_cells() {
    let mut app = make_test_app().await;
    let older = user_cell("old");
    let weak = Arc::downgrade(&older);
    app.transcript_cells.push(older);
    let mut group = ComputerActivityCell::default();
    for index in 0..crate::history_window::MAX_HISTORY_ITEMS {
        group.start(crate::history_cell::new_active_mcp_tool_call(
            format!("call-{index}"),
            crate::history_cell::McpInvocation {
                server: "cua_repl".to_string(),
                tool: "js".to_string(),
                arguments: None,
            },
            false,
        ));
    }
    assert_eq!(
        group.retained_item_count(),
        crate::history_window::MAX_HISTORY_ITEMS
    );
    app.transcript_cells.push(Arc::new(group));
    app.bound_history_window(false);
    assert_eq!(app.transcript_cells.len(), 1);
    assert!(weak.upgrade().is_none());
}

#[tokio::test]
async fn display_window_reaccounts_mutable_tool_results() {
    let mut app = make_test_app().await;
    let older: Arc<dyn HistoryCell> = Arc::new(crate::history_cell::AgentMarkdownCell::new(
        "a".repeat(4 * 1024 * 1024),
        app.config.cwd.as_path(),
    ));
    let weak = Arc::downgrade(&older);
    app.transcript_cells.push(older);
    let started: ThreadItem = serde_json::from_value(serde_json::json!({
        "type": "dynamicToolCall", "id": "tool", "namespace": null, "tool": "inspect",
        "arguments": {}, "status": "inProgress", "contentItems": null,
        "success": null, "durationMs": null,
    }))
    .unwrap();
    let cell = crate::history_cell::DynamicToolCallCell::from_item(started.payload).unwrap();
    app.transcript_cells.push(Arc::new(cell.clone()));
    app.bound_history_window(false);
    assert!(weak.upgrade().is_some());
    let completed: ThreadItem = serde_json::from_value(serde_json::json!({
        "type": "dynamicToolCall", "id": "tool", "namespace": null, "tool": "inspect",
        "arguments": {}, "status": "completed", "contentItems": [
            {"type": "inputText", "text": "b".repeat(5 * 1024 * 1024)}
        ], "success": true, "durationMs": 1,
    }))
    .unwrap();
    assert!(cell.update_from_item(completed.payload));
    app.bound_history_window(false);
    assert_eq!(app.transcript_cells.len(), 1);
    assert!(weak.upgrade().is_none());
}

#[test]
fn canonical_window_retains_the_selected_edge_of_one_large_turn() {
    let ids = (0..450).map(|index| index.to_string()).collect::<Vec<_>>();
    let mut turns = vec![turn(
        "active",
        TurnStatus::InProgress,
        &ids.iter().map(String::as_str).collect::<Vec<_>>(),
    )];
    bound_display_turns(&mut turns, false);
    assert_eq!(turns[0].items.first().unwrap().id(), "50");
    assert_eq!(turns[0].items.last().unwrap().id(), "449");
}
