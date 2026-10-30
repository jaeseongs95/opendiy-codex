// Modified for OpenDIY Codex R30 (2026-10-30); see MODIFICATIONS.md.
use super::CursorHistory;
use super::HistoryHydrationScope;
use super::HistoryLoadBudget;
use super::advancing_cursor;
use crate::legacy_core::config::ConfigBuilder;
use crate::legacy_core::config::TerminalResizeReflowMaxRows;
use crate::local_settings::LocalSettings;
use codex_config::types::AltScreenMode;
use pretty_assertions::assert_eq;

#[test]
fn advancing_cursor_rejects_repeated_cursors() {
    let mut seen_cursors = CursorHistory::default();
    assert_eq!(
        advancing_cursor(
            /*current*/ None,
            Some("first".to_string()),
            &mut seen_cursors,
        ),
        Some("first".to_string())
    );
    assert_eq!(
        advancing_cursor(Some("first"), Some("second".to_string()), &mut seen_cursors,),
        Some("second".to_string())
    );
    assert_eq!(
        advancing_cursor(Some("second"), Some("first".to_string()), &mut seen_cursors,),
        None
    );
    assert_eq!(
        advancing_cursor(Some("second"), /*next*/ None, &mut seen_cursors),
        None
    );
}

#[tokio::test]
async fn owned_initial_history_stops_after_viewport_or_scan_budget() {
    let codex_home = tempfile::tempdir().expect("temporary codex home");
    let mut config = ConfigBuilder::default()
        .codex_home(codex_home.path().to_path_buf())
        .build()
        .await
        .expect("config");
    config.tui_fullscreen_transcript = true;
    config.tui_alternate_screen = AltScreenMode::Always;
    config.terminal_resize_reflow.max_rows = TerminalResizeReflowMaxRows::Disabled;
    let local_settings = LocalSettings::from(&config);
    let budget = HistoryLoadBudget::new(
        HistoryHydrationScope::Initial,
        Some(&config),
        Some(&local_settings),
        /*terminal_height*/ 10,
    );

    assert_eq!(
        [
            budget.next_page_size(/*rendered_rows*/ 0, /*scanned_items*/ 0),
            budget.next_page_size(/*rendered_rows*/ 29, /*scanned_items*/ 10),
            budget.next_page_size(/*rendered_rows*/ 30, /*scanned_items*/ 11),
            budget.next_page_size(/*rendered_rows*/ 0, /*scanned_items*/ 399),
            budget.next_page_size(/*rendered_rows*/ 0, /*scanned_items*/ 400),
        ],
        [Some(30), Some(100), None, Some(1), None],
    );

    let complete = HistoryLoadBudget::new(
        HistoryHydrationScope::Complete,
        Some(&config),
        /*local_settings*/ None,
        /*terminal_height*/ 10,
    );
    assert_eq!(
        complete.next_page_size(/*rendered_rows*/ 10_000, /*scanned_items*/ 10_000),
        Some(100),
    );

    config.tui_fullscreen_transcript = false;
    // The mode selected at launch wins even if thread config changes or is unavailable.
    for config in [Some(&config), None] {
        let budget = HistoryLoadBudget::new(
            HistoryHydrationScope::Initial,
            config,
            Some(&local_settings),
            /*terminal_height*/ 10,
        );
        assert_eq!(
            [
                budget.next_page_size(/*rendered_rows*/ 0, /*scanned_items*/ 0),
                budget.next_page_size(/*rendered_rows*/ 30, /*scanned_items*/ 11),
            ],
            [Some(30), None],
        );
    }
}

#[test]
fn cursor_loop_detection_does_not_retain_every_visited_page() {
    let mut seen = CursorHistory::default();
    let mut current = None;
    for index in 0..2000 {
        current = advancing_cursor(
            current.as_deref(),
            Some(format!("cursor-{index}")),
            &mut seen,
        );
        assert!(current.is_some());
        assert!(seen.len() <= crate::history_window::MAX_HISTORY_ITEMS + 1);
    }
    assert_eq!(
        advancing_cursor(current.as_deref(), current.clone(), &mut seen),
        None
    );
}

#[test]
fn cursor_history_still_rejects_cycles_larger_than_its_recent_cache() {
    let mut seen = CursorHistory::default();
    let mut current = None;
    let mut stopped = false;
    for index in 0..4096 {
        current = advancing_cursor(
            current.as_deref(),
            Some(format!("cursor-{}", index % 513)),
            &mut seen,
        );
        assert!(seen.len() <= crate::history_window::MAX_HISTORY_ITEMS + 1);
        if current.is_none() {
            stopped = true;
            break;
        }
    }
    assert!(
        stopped,
        "a cycle must terminate even after recent-cursor eviction"
    );
}

#[test]
fn unmaterialized_history_recovery_requires_exact_structured_contract() {
    use codex_app_server_client::TypedRequestError;
    use codex_app_server_protocol::JSONRPCErrorError;
    let data = serde_json::json!({
        "kind": "historyProjectionUnavailable", "reason": "unmaterialized"
    });
    for (code, data, expected) in [
        (-32600, Some(data.clone()), true),
        (-32601, Some(data.clone()), false),
        (-32603, Some(data.clone()), false),
        (-32000, Some(data), false),
        (-32600, None, false),
        (-32600, Some(serde_json::json!("unmaterialized")), false),
        (
            -32600,
            Some(serde_json::json!([
                "historyProjectionUnavailable",
                "unmaterialized"
            ])),
            false,
        ),
        (
            -32600,
            Some(
                serde_json::json!({ "kind": "historyProjectionUnavailable", "reason": "unknown" }),
            ),
            false,
        ),
        (
            -32600,
            Some(
                serde_json::json!({ "kind": "historyProjectionUnavailable", "reason": "deleted" }),
            ),
            false,
        ),
        (
            -32600,
            Some(serde_json::json!({ "kind": "historyProjectionUnavailable", "reason": 1 })),
            false,
        ),
        (
            -32600,
            Some(serde_json::json!({ "kind": "unknown", "reason": "unmaterialized" })),
            false,
        ),
        (
            -32600,
            Some(serde_json::json!({ "kind": "historyProjectionUnavailable" })),
            false,
        ),
    ] {
        let error = color_eyre::Report::new(TypedRequestError::Server {
            method: "thread/read".to_string(),
            source: JSONRPCErrorError {
                code,
                message: "thread is not materialized yet".to_string(),
                data,
            },
        })
        .wrap_err("initial history hydration");
        assert_eq!(super::is_unmaterialized_history_error(&error), expected);
    }
    let transport = color_eyre::Report::new(TypedRequestError::Transport {
        method: "thread/read".to_string(),
        source: std::io::Error::new(std::io::ErrorKind::PermissionDenied, "not materialized yet"),
    });
    assert!(!super::is_unmaterialized_history_error(&transport));
}
