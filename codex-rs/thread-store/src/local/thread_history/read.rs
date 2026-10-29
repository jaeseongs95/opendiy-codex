// Modified for OpenDIY Codex R28 (2026-10-29); see MODIFICATIONS.md.
use codex_protocol::ThreadId;
use codex_protocol::protocol::ThreadHistoryMode;
use serde::Deserialize;
use serde::Serialize;
use sqlx::Row;

use super::super::LocalThreadStore;
use super::super::rollout_lineage::RolloutLineage;
use super::segment_paging::HistoryPageByteTarget;
use super::segment_paging::finish_page_with_more;
use super::segment_paging::load_admitted_item;
use super::segment_paging::page_item_rows;
use super::segment_paging::page_turn_rows;
use super::segment_paging::validate_page_size;
use super::sqlite_integer;
use super::turn_lookup::find_source_turn;
use crate::ItemPage;
use crate::ListItemsParams;
use crate::ListTurnsParams;
use crate::StoredThreadItem;
use crate::StoredTurn;
use crate::StoredTurnError;
use crate::StoredTurnItemsView;
use crate::StoredTurnStatus;
use crate::ThreadStoreError;
use crate::ThreadStoreResult;
use crate::TurnPage;

#[cfg(test)]
#[path = "read_tests.rs"]
mod tests;

#[derive(Clone, Deserialize, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub(super) enum CursorScope {
    Turns,
    ItemsByCreatedAtOrdinal,
    ItemsByUpdatedAtOrdinal,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct HistoryCursor {
    pub requested_thread_id: ThreadId,
    pub rollout_ordinal: u64,
    pub include_anchor: bool,
    pub scope: CursorScope,
}

#[derive(Clone, Copy)]
pub(super) struct RolloutHistoryPosition {
    pub rollout_ordinal: i64,
}

pub(super) struct StoredTurnRow {
    pub rollout_id: ThreadId,
    pub position: RolloutHistoryPosition,
    pub turn_id: String,
    pub root_turn_id: Option<String>,
    pub status: StoredTurnStatus,
    pub error: Option<StoredTurnError>,
    pub error_json_bytes: usize,
    pub started_at: Option<i64>,
    pub completed_at: Option<i64>,
    pub duration_ms: Option<i64>,
    pub first_user_item_id: Option<String>,
    pub final_agent_item_id: Option<String>,
    pub summary_items: Vec<StoredThreadItem>,
}

pub(super) struct StoredThreadItemRow {
    pub position: RolloutHistoryPosition,
    pub item: StoredThreadItem,
}

pub(in crate::local) async fn list_turns(
    store: &LocalThreadStore,
    params: ListTurnsParams,
) -> ThreadStoreResult<TurnPage> {
    validate_thread_for_paginated_reads(
        store,
        params.thread_id,
        params.include_archived,
        "list_turns",
    )
    .await?;
    validate_page_size(params.page_size)?;
    let lineage = store
        .resolve_rollout_lineage_for_history_page(params.thread_id)
        .await?;
    let pool = store.thread_history_db().await?;
    let page = page_turn_rows(
        pool,
        params.thread_id,
        &lineage,
        params.cursor.as_deref(),
        params.page_size,
        params.sort_direction,
        params.items_view,
    )
    .await?;
    let mut budget = HistoryPageByteTarget::default();
    let mut rows = Vec::with_capacity(page.rows.len());
    let mut has_more = page.next_cursor.is_some();
    for mut turn in page.rows {
        if matches!(params.items_view, StoredTurnItemsView::Summary) {
            let Some(items) = load_summary_items(pool, &lineage, &turn, &budget).await? else {
                has_more = true;
                break;
            };
            turn.summary_items = items;
        }
        let bytes = turn
            .summary_items
            .iter()
            .fold(turn.error_json_bytes, |sum, item| {
                sum.saturating_add(item.item_json.len())
            });
        if !budget.can_add(bytes) {
            has_more = true;
            break;
        }
        budget.push(bytes);
        rows.push(turn);
    }
    let page = finish_page_with_more(
        params.thread_id,
        CursorScope::Turns,
        rows,
        params.page_size,
        has_more,
    )?;
    let turns = page
        .rows
        .into_iter()
        .map(|turn| StoredTurn {
            turn_id: turn.turn_id,
            root_turn_id: turn.root_turn_id,
            items: turn.summary_items,
            items_view: params.items_view,
            status: turn.status,
            error: turn.error,
            started_at: turn.started_at,
            completed_at: turn.completed_at,
            duration_ms: turn.duration_ms,
        })
        .collect();

    Ok(TurnPage {
        turns,
        next_cursor: page.next_cursor,
        backwards_cursor: page.backwards_cursor,
    })
}

pub(in crate::local) async fn list_items(
    store: &LocalThreadStore,
    mut params: ListItemsParams,
) -> ThreadStoreResult<ItemPage> {
    validate_thread_for_paginated_reads(
        store,
        params.thread_id,
        params.include_archived,
        "list_items",
    )
    .await?;
    if let Some(ids) = &params.item_ids {
        if params.turn_id.is_none()
            || params.position.is_some()
            || params.after_updated_at_ordinal.is_some()
            || params.sort_key != crate::ItemSortKey::CreatedAtOrdinal
            || !(1..=100).contains(&ids.len())
        {
            return Err(ThreadStoreError::InvalidRequest {
                message: "itemIds requires turnId, 1 to 100 IDs, and no cursor or update replay"
                    .to_string(),
            });
        }
        params.page_size = ids.len();
        params.sort_direction = crate::SortDirection::Asc;
    }
    validate_page_size(params.page_size)?;
    let lineage = store
        .resolve_rollout_lineage_for_history_page(params.thread_id)
        .await?;
    let pool = store.thread_history_db().await?;
    let page = page_item_rows(pool, &lineage, &params).await?;

    Ok(ItemPage {
        items: page.rows.into_iter().map(|row| row.item).collect(),
        next_cursor: page.next_cursor,
        backwards_cursor: page.backwards_cursor,
    })
}

pub(super) async fn validate_thread_for_paginated_reads(
    store: &LocalThreadStore,
    thread_id: ThreadId,
    include_archived: bool,
    operation: &'static str,
) -> ThreadStoreResult<()> {
    let Some(state_db) = store.state_db().await else {
        return Err(ThreadStoreError::Unsupported { operation });
    };
    let Some(metadata) =
        state_db
            .get_thread(thread_id)
            .await
            .map_err(|err| ThreadStoreError::Internal {
                message: format!("failed to read thread metadata: {err}"),
            })?
    else {
        return Err(ThreadStoreError::Unsupported { operation });
    };
    if metadata.archived_at.is_some() && !include_archived {
        return Err(ThreadStoreError::InvalidRequest {
            message: format!("thread {thread_id} is archived"),
        });
    }
    match metadata.history_mode {
        ThreadHistoryMode::Legacy => Err(ThreadStoreError::Unsupported { operation }),
        ThreadHistoryMode::Paginated => Ok(()),
    }
}

async fn load_summary_items(
    pool: &sqlx::SqlitePool,
    lineage: &RolloutLineage,
    turn: &StoredTurnRow,
    page_budget: &HistoryPageByteTarget,
) -> ThreadStoreResult<Option<Vec<StoredThreadItem>>> {
    // A synthetic fork boundary inherits the earliest visible source's summary.
    let (rollout_id, first_user, final_agent) =
        if matches!(turn.status, StoredTurnStatus::Interrupted)
            && turn.first_user_item_id.is_none()
            && turn.final_agent_item_id.is_none()
        {
            let source = find_source_turn(pool, lineage, &turn.turn_id).await?;
            (
                source.rollout_id,
                source.first_user_item_id,
                source.final_agent_item_id,
            )
        } else {
            (
                turn.rollout_id,
                turn.first_user_item_id.clone(),
                turn.final_agent_item_id.clone(),
            )
        };
    let Some(segment) = lineage
        .segments()
        .iter()
        .find(|segment| segment.rollout_id() == rollout_id)
    else {
        return Ok(Some(Vec::new()));
    };
    let start = sqlite_integer(segment.start_ordinal(), "rollout ordinal")?;
    let end = segment
        .end_ordinal()
        .map(|ordinal| sqlite_integer(ordinal, "rollout ordinal"))
        .transpose()?;
    let mut transaction = pool.begin().await.map_err(super::thread_history_error)?;
    let candidates = sqlx::query("SELECT turn_id, item_id, rollout_ordinal, length(CAST(item_json AS BLOB)) AS item_json_bytes FROM thread_items WHERE thread_id = ? AND turn_id = ? AND rollout_ordinal >= ? AND (? IS NULL OR rollout_ordinal < ?) AND (item_id = ? OR item_id = ?) ORDER BY rollout_ordinal ASC")
        .bind(rollout_id.to_string()).bind(&turn.turn_id).bind(start).bind(end).bind(end)
        .bind(first_user).bind(final_agent).fetch_all(&mut *transaction).await.map_err(super::thread_history_error)?;
    let mut raw_bytes = turn.error_json_bytes;
    for candidate in &candidates {
        let bytes = usize::try_from(candidate.try_get::<i64, _>("item_json_bytes")?)
            .map_err(super::thread_history_error)?;
        raw_bytes = raw_bytes.saturating_add(bytes);
    }
    if !page_budget.can_add(raw_bytes) {
        return Ok(None);
    }
    // A summary turn is indivisible. Its selected first/final items have passed
    // the page target together, and a large first turn must remain readable.
    let mut budget = HistoryPageByteTarget::for_item_lookup();
    let mut items = Vec::new();
    for candidate in candidates {
        let Some(row) =
            load_admitted_item(&mut transaction, rollout_id, candidate, &mut budget).await?
        else {
            return Err(ThreadStoreError::Internal {
                message: "unpaged summary unexpectedly split".to_string(),
            });
        };
        items.push(row.item);
    }
    Ok(Some(items))
}

pub(super) fn parse_cursor(
    cursor: Option<&str>,
    requested_thread_id: ThreadId,
    scope: CursorScope,
) -> ThreadStoreResult<Option<HistoryCursor>> {
    let Some(cursor) = cursor else {
        return Ok(None);
    };
    let cursor_value: HistoryCursor =
        serde_json::from_str(cursor).map_err(|_| invalid_cursor(cursor))?;
    if cursor_value.requested_thread_id != requested_thread_id || cursor_value.scope != scope {
        return Err(invalid_cursor(cursor));
    }
    Ok(Some(cursor_value))
}

pub(super) fn serialize_cursor(
    requested_thread_id: ThreadId,
    scope: CursorScope,
    rollout_ordinal: i64,
    include_anchor: bool,
) -> ThreadStoreResult<String> {
    let rollout_ordinal =
        u64::try_from(rollout_ordinal).map_err(|_| invalid_cursor("negative rollout ordinal"))?;
    serde_json::to_string(&HistoryCursor {
        requested_thread_id,
        rollout_ordinal,
        include_anchor,
        scope,
    })
    .map_err(super::thread_history_error)
}

pub(super) fn stored_turn_row(
    row: sqlx::sqlite::SqliteRow,
    rollout_id: ThreadId,
    error_json: Option<String>,
) -> ThreadStoreResult<StoredTurnRow> {
    let status = match row.try_get::<String, _>("status")?.as_str() {
        "completed" => StoredTurnStatus::Completed,
        "interrupted" => StoredTurnStatus::Interrupted,
        "failed" => StoredTurnStatus::Failed,
        "inProgress" => StoredTurnStatus::InProgress,
        status => {
            return Err(ThreadStoreError::Internal {
                message: format!("unknown stored turn status: {status}"),
            });
        }
    };
    let error_json_bytes = error_json.as_ref().map_or(0, String::len);
    let error = error_json
        .as_deref()
        .map(serde_json::from_str)
        .transpose()
        .map_err(super::thread_history_error)?;
    Ok(StoredTurnRow {
        rollout_id,
        position: RolloutHistoryPosition {
            rollout_ordinal: row.try_get("rollout_ordinal")?,
        },
        turn_id: row.try_get("turn_id")?,
        root_turn_id: row.try_get("root_turn_id")?,
        status,
        error,
        error_json_bytes,
        started_at: row.try_get("started_at")?,
        completed_at: row.try_get("completed_at")?,
        duration_ms: row.try_get("duration_ms")?,
        first_user_item_id: row.try_get("first_user_item_id")?,
        final_agent_item_id: row.try_get("final_agent_item_id")?,
        summary_items: Vec::new(),
    })
}

pub(super) fn stored_thread_item_row(
    row: sqlx::sqlite::SqliteRow,
) -> ThreadStoreResult<StoredThreadItemRow> {
    let rollout_ordinal = row.try_get::<i64, _>("rollout_ordinal")?;
    if rollout_ordinal < 0 {
        return Err(ThreadStoreError::Internal {
            message: format!("invalid stored item rollout ordinal: {rollout_ordinal}"),
        });
    }
    Ok(StoredThreadItemRow {
        position: RolloutHistoryPosition { rollout_ordinal },
        item: stored_thread_item(row)?,
    })
}

fn stored_thread_item(row: sqlx::sqlite::SqliteRow) -> ThreadStoreResult<StoredThreadItem> {
    let updated_at_ordinal = stored_updated_at_ordinal(row.try_get("updated_at_ordinal")?)?;
    Ok(StoredThreadItem {
        turn_id: row.try_get("turn_id")?,
        item_id: row.try_get("item_id")?,
        updated_at_ordinal,
        created_at_ms: row.try_get("created_at_ms")?,
        started_at_ms: row.try_get("started_at_ms")?,
        completed_at_ms: row.try_get("completed_at_ms")?,
        item_json: row.try_get::<String, _>("item_json")?.into_bytes(),
    })
}

fn stored_updated_at_ordinal(updated_at_ordinal: i64) -> ThreadStoreResult<u64> {
    u64::try_from(updated_at_ordinal).map_err(|_| ThreadStoreError::Internal {
        message: format!("invalid stored item updated-at ordinal: {updated_at_ordinal}"),
    })
}

pub(super) fn invalid_cursor(cursor: &str) -> ThreadStoreError {
    ThreadStoreError::InvalidRequest {
        message: format!("invalid cursor: {cursor}"),
    }
}
