// Modified for OpenDIY Codex R28 (2026-10-29); see MODIFICATIONS.md.
use codex_protocol::ThreadId;
use sqlx::QueryBuilder;
use sqlx::Row;
use sqlx::Sqlite;

use super::super::rollout_lineage::RolloutLineage;
use super::super::rollout_lineage::RolloutLineageSegment;
use super::read::CursorScope;
use super::read::HistoryCursor;
use super::read::RolloutHistoryPosition;
use super::read::StoredThreadItemRow;
use super::read::StoredTurnRow;
use super::read::invalid_cursor;
use super::read::parse_cursor;
use super::read::serialize_cursor;
use super::read::stored_thread_item_row;
use super::read::stored_turn_row;
use super::thread_history_error;
use crate::HISTORY_PAGE_TARGET_BYTES;
use crate::ItemSortKey;
use crate::ListItemsParams;
use crate::ListItemsPosition;
use crate::SortDirection;
use crate::StoredTurnItemsView;
use crate::ThreadStoreError;
use crate::ThreadStoreResult;

pub(super) struct SegmentPage<T> {
    pub rows: Vec<T>,
    pub next_cursor: Option<String>,
    pub backwards_cursor: Option<String>,
}

pub(super) fn validate_page_size(page_size: usize) -> ThreadStoreResult<()> {
    if page_size == 0 {
        return Err(ThreadStoreError::InvalidRequest {
            message: "page size must be positive".to_string(),
        });
    }
    let limit = page_size.checked_add(1).ok_or_else(page_size_too_large)?;
    i64::try_from(limit).map_err(|_| page_size_too_large())?;
    Ok(())
}

pub(super) async fn page_turn_rows(
    pool: &sqlx::SqlitePool,
    requested_thread_id: ThreadId,
    lineage: &RolloutLineage,
    cursor: Option<&str>,
    page_size: usize,
    direction: SortDirection,
    _items_view: StoredTurnItemsView,
) -> ThreadStoreResult<SegmentPage<StoredTurnRow>> {
    let cursor = parse_cursor(cursor, requested_thread_id, CursorScope::Turns)?;
    let mut transaction = pool.begin().await.map_err(thread_history_error)?;
    ensure_projection_available(&mut transaction, lineage).await?;
    let mut rows = Vec::new();
    let mut error_bytes = 0usize;
    let mut has_more = false;
    'segments: for (segment_index, segment, segment_cursor) in
        segments_from_cursor(lineage, direction, cursor.as_ref())?
    {
        let remaining = remaining_limit(page_size, rows.len())?;
        if remaining == 0 {
            break;
        }
        // Read count-limited metadata first; even a very large error body stays in SQLite
        // until its UTF-8 size is admitted in this same read transaction.
        let mut query = QueryBuilder::<Sqlite>::new(
            "SELECT turn_id, root_turn_id, rollout_ordinal, status, length(CAST(error_json AS BLOB)) AS error_bytes, started_at, completed_at, duration_ms, first_user_item_id, final_agent_item_id FROM thread_turns WHERE thread_id = ",
        );
        query.push_bind(segment.rollout_id().to_string());
        push_segment_range(&mut query, segment)?;
        for newer_segment in &lineage.segments()[segment_index + 1..] {
            query.push(" AND NOT EXISTS (SELECT 1 FROM thread_turns AS newer_turn WHERE newer_turn.thread_id = ")
                .push_bind(newer_segment.rollout_id().to_string())
                .push(" AND newer_turn.turn_id = thread_turns.turn_id AND newer_turn.rollout_ordinal >= ")
                .push_bind(sqlite_integer(newer_segment.start_ordinal())?);
            if let Some(end) = newer_segment.end_ordinal() {
                query
                    .push(" AND newer_turn.rollout_ordinal < ")
                    .push_bind(sqlite_integer(end)?);
            }
            query.push(")");
        }
        push_cursor_clause(&mut query, direction, segment_cursor)?;
        push_order_and_limit(&mut query, direction, remaining);
        let candidates = query
            .build()
            .fetch_all(&mut *transaction)
            .await
            .map_err(thread_history_error)?;
        for candidate in candidates {
            if rows.len() == page_size {
                has_more = true;
                break 'segments;
            }
            let error_size = candidate.try_get::<Option<i64>, _>("error_bytes")?;
            let bytes = usize::try_from(error_size.unwrap_or(0)).map_err(thread_history_error)?;
            if !rows.is_empty() && error_bytes.saturating_add(bytes) > HISTORY_PAGE_TARGET_BYTES {
                has_more = true;
                break 'segments;
            }
            let error_json = if error_size.is_some() {
                sqlx::query_scalar::<_, Option<String>>(
                    "SELECT error_json FROM thread_turns WHERE thread_id = ? AND turn_id = ?",
                )
                .bind(segment.rollout_id().to_string())
                .bind(candidate.try_get::<String, _>("turn_id")?)
                .fetch_one(&mut *transaction)
                .await
                .map_err(thread_history_error)?
            } else {
                None
            };
            error_bytes = error_bytes.saturating_add(bytes);
            rows.push(stored_turn_row(
                candidate,
                segment.rollout_id(),
                error_json,
            )?);
        }
    }
    finish_page_with_more(
        requested_thread_id,
        CursorScope::Turns,
        rows,
        page_size,
        has_more,
    )
}

pub(super) async fn page_item_rows(
    pool: &sqlx::SqlitePool,
    lineage: &RolloutLineage,
    params: &ListItemsParams,
) -> ThreadStoreResult<SegmentPage<StoredThreadItemRow>> {
    // The size probe and payload read must observe the same SQLite snapshot. Dropping
    // this transaction on error or cancellation also returns its pooled connection.
    let mut transaction = pool.begin().await.map_err(thread_history_error)?;
    let mut budget = if params.item_ids.is_some() {
        HistoryPageByteTarget::for_item_lookup()
    } else {
        HistoryPageByteTarget::default()
    };
    let (cursor, anchor_item_id) = match params.position.as_ref() {
        Some(ListItemsPosition::Cursor(cursor)) => (Some(cursor.as_str()), None),
        Some(ListItemsPosition::ItemAnchor { item_id }) => (None, Some(item_id.as_str())),
        None => (None, None),
    };
    if anchor_item_id.is_some()
        && (params.sort_key != ItemSortKey::CreatedAtOrdinal
            || params.after_updated_at_ordinal.is_some())
    {
        return Err(ThreadStoreError::InvalidRequest {
            message: "item anchors require creation-order paging without an update watermark"
                .to_string(),
        });
    }
    // Update ordinals are local to a rollout. Forked lineages need a structured
    // watermark before incremental replay can safely span their segments.
    if params.after_updated_at_ordinal.is_some() && lineage.segments().len() > 1 {
        return Err(ThreadStoreError::InvalidRequest {
            message: "incremental item replay is not supported for forked threads".to_string(),
        });
    }
    if matches!(params.sort_key, ItemSortKey::UpdatedAtOrdinal) {
        let Some(after_updated_at_ordinal) = params.after_updated_at_ordinal else {
            return Err(ThreadStoreError::InvalidRequest {
                message: "update-ordinal item sorting requires an update watermark".to_string(),
            });
        };
        let [segment] = lineage.segments() else {
            return Err(ThreadStoreError::Internal {
                message: "update-ordinal item paging requires one rollout segment".to_string(),
            });
        };
        sqlite_integer(after_updated_at_ordinal)?;
        ensure_projection_available(&mut transaction, lineage).await?;
        return page_updated_item_rows(
            &mut transaction,
            segment.rollout_id(),
            params,
            cursor,
            after_updated_at_ordinal,
        )
        .await;
    }
    let cursor = if let Some(anchor_item_id) = anchor_item_id {
        let turn_id = params
            .turn_id
            .as_deref()
            .filter(|id| !id.is_empty())
            .ok_or_else(|| ThreadStoreError::InvalidRequest {
                message: "turnId is required when cursor is an item anchor".to_string(),
            })?;
        if anchor_item_id.is_empty() {
            return Err(invalid_item_anchor());
        }
        let mut anchor = None;
        for segment in lineage.segments() {
            let mut query = QueryBuilder::<Sqlite>::new(
                "SELECT rollout_ordinal FROM thread_items WHERE thread_id = ",
            );
            query.push_bind(segment.rollout_id().to_string());
            query.push(" AND item_id = ").push_bind(anchor_item_id);
            push_segment_range(&mut query, segment)?;
            query.push(" AND turn_id = ").push_bind(turn_id);
            if let Some(ordinal) = query
                .build_query_scalar::<i64>()
                .fetch_optional(&mut *transaction)
                .await
                .map_err(thread_history_error)?
            {
                anchor = Some(ordinal);
                break;
            }
        }
        Some(HistoryCursor {
            requested_thread_id: params.thread_id,
            rollout_ordinal: u64::try_from(anchor.ok_or_else(invalid_item_anchor)?)
                .map_err(thread_history_error)?,
            include_anchor: false,
            scope: CursorScope::ItemsByCreatedAtOrdinal,
        })
    } else {
        parse_cursor(
            cursor,
            params.thread_id,
            CursorScope::ItemsByCreatedAtOrdinal,
        )?
    };
    if let Some(watermark) = params.after_updated_at_ordinal {
        sqlite_integer(watermark)?;
    }
    ensure_projection_available(&mut transaction, lineage).await?;
    let mut rows = Vec::new();
    let mut has_more = false;
    'segments: for (_, segment, segment_cursor) in
        segments_from_cursor(lineage, params.sort_direction, cursor.as_ref())?
    {
        let remaining = remaining_limit(params.page_size, rows.len())?;
        if remaining == 0 {
            break;
        }
        let mut query = QueryBuilder::<Sqlite>::new(
            r#"
SELECT turn_id, item_id, rollout_ordinal, length(CAST(item_json AS BLOB)) AS item_json_bytes
FROM thread_items"#,
        );
        if params.turn_id.is_some()
            && params.item_ids.is_none()
            && params.after_updated_at_ordinal.is_none()
        {
            // A thread-wide index can scan earlier turns when a turn's final page is underfilled.
            query.push(" INDEXED BY idx_thread_items_by_turn_page");
        }
        query
            .push(" WHERE thread_id = ")
            .push_bind(segment.rollout_id().to_string());
        push_segment_range(&mut query, segment)?;
        if let Some(after_updated_at_ordinal) = params.after_updated_at_ordinal {
            query
                .push(" AND updated_at_ordinal > ")
                .push_bind(sqlite_integer(after_updated_at_ordinal)?);
        }
        if let Some(turn_id) = params.turn_id.as_deref() {
            query.push(" AND turn_id = ").push_bind(turn_id);
        }
        if let Some(item_ids) = params.item_ids.as_deref() {
            query.push(" AND item_id IN (");
            let mut ids = query.separated(", ");
            for item_id in item_ids {
                ids.push_bind(item_id);
            }
            ids.push_unseparated(")");
        }
        push_cursor_clause(&mut query, params.sort_direction, segment_cursor)?;
        push_order_and_limit(&mut query, params.sort_direction, remaining);
        let candidates = query
            .build()
            .fetch_all(&mut *transaction)
            .await
            .map_err(thread_history_error)?;
        for candidate in candidates {
            if rows.len() == params.page_size {
                has_more = true;
                break 'segments;
            }
            if let Some(row) = load_admitted_item(
                &mut transaction,
                segment.rollout_id(),
                candidate,
                &mut budget,
            )
            .await?
            {
                rows.push(row);
            } else {
                has_more = true;
                break 'segments;
            }
        }
    }
    finish_page_with_more(
        params.thread_id,
        CursorScope::ItemsByCreatedAtOrdinal,
        rows,
        params.page_size,
        has_more,
    )
}

async fn page_updated_item_rows(
    transaction: &mut sqlx::Transaction<'_, Sqlite>,
    rollout_id: ThreadId,
    params: &ListItemsParams,
    cursor: Option<&str>,
    after_updated_at_ordinal: u64,
) -> ThreadStoreResult<SegmentPage<StoredThreadItemRow>> {
    let cursor = parse_cursor(
        cursor,
        params.thread_id,
        CursorScope::ItemsByUpdatedAtOrdinal,
    )?;
    let mut query = QueryBuilder::<Sqlite>::new(
        r#"
SELECT turn_id, item_id, updated_at_ordinal AS rollout_ordinal, length(CAST(item_json AS BLOB)) AS item_json_bytes
FROM thread_items
WHERE thread_id =
        "#,
    );
    query
        .push_bind(rollout_id.to_string())
        .push(" AND updated_at_ordinal > ")
        .push_bind(sqlite_integer(after_updated_at_ordinal)?);
    if let Some(turn_id) = params.turn_id.as_deref() {
        query.push(" AND turn_id = ").push_bind(turn_id);
    }
    if let Some(cursor) = cursor {
        let comparator = match (params.sort_direction, cursor.include_anchor) {
            (SortDirection::Asc, true) => ">=",
            (SortDirection::Asc, false) => ">",
            (SortDirection::Desc, true) => "<=",
            (SortDirection::Desc, false) => "<",
        };
        query
            .push(" AND updated_at_ordinal ")
            .push(comparator)
            .push(" ")
            .push_bind(sqlite_integer(cursor.rollout_ordinal)?);
    }
    let order = match params.sort_direction {
        SortDirection::Asc => "ASC",
        SortDirection::Desc => "DESC",
    };
    query
        .push(" ORDER BY updated_at_ordinal ")
        .push(order)
        .push(" LIMIT ")
        .push_bind(remaining_limit(params.page_size, /*row_count*/ 0)?);
    let candidates = query
        .build()
        .fetch_all(&mut **transaction)
        .await
        .map_err(thread_history_error)?;
    let mut rows = Vec::new();
    let mut budget = HistoryPageByteTarget::default();
    let mut has_more = false;
    for candidate in candidates {
        if rows.len() == params.page_size {
            has_more = true;
            break;
        }
        // Keep the update-order position rather than the item's creation ordinal.
        let position = candidate.try_get::<i64, _>("rollout_ordinal")?;
        if let Some(mut row) =
            load_admitted_item(transaction, rollout_id, candidate, &mut budget).await?
        {
            row.position.rollout_ordinal = position;
            rows.push(row);
        } else {
            has_more = true;
            break;
        }
    }
    finish_page_with_more(
        params.thread_id,
        CursorScope::ItemsByUpdatedAtOrdinal,
        rows,
        params.page_size,
        has_more,
    )
}

fn segments_from_cursor<'a>(
    lineage: &'a RolloutLineage,
    direction: SortDirection,
    cursor: Option<&'a HistoryCursor>,
) -> ThreadStoreResult<Vec<(usize, &'a RolloutLineageSegment, Option<&'a HistoryCursor>)>> {
    let segments = lineage.segments();
    let cursor_index = cursor
        .map(|cursor| {
            lineage
                .segment_index_for_ordinal(cursor.rollout_ordinal)
                .ok_or_else(|| invalid_cursor("position outside thread lineage"))
        })
        .transpose()?;
    let indexes: Vec<usize> = match direction {
        SortDirection::Asc => (cursor_index.unwrap_or(0)..segments.len()).collect(),
        SortDirection::Desc => {
            let end = cursor_index.unwrap_or_else(|| segments.len().saturating_sub(1));
            (0..=end).rev().collect()
        }
    };
    Ok(indexes
        .into_iter()
        .map(|index| {
            let segment_cursor = if Some(index) == cursor_index {
                cursor
            } else {
                None
            };
            (index, &segments[index], segment_cursor)
        })
        .collect())
}

fn push_segment_range(
    query: &mut QueryBuilder<Sqlite>,
    segment: &RolloutLineageSegment,
) -> ThreadStoreResult<()> {
    query
        .push(" AND rollout_ordinal >= ")
        .push_bind(sqlite_integer(segment.start_ordinal())?);
    if let Some(end_ordinal) = segment.end_ordinal() {
        query
            .push(" AND rollout_ordinal < ")
            .push_bind(sqlite_integer(end_ordinal)?);
    }
    Ok(())
}

fn push_cursor_clause(
    query: &mut QueryBuilder<Sqlite>,
    direction: SortDirection,
    cursor: Option<&HistoryCursor>,
) -> ThreadStoreResult<()> {
    if let Some(cursor) = cursor {
        let comparator = match (direction, cursor.include_anchor) {
            (SortDirection::Asc, true) => ">=",
            (SortDirection::Asc, false) => ">",
            (SortDirection::Desc, true) => "<=",
            (SortDirection::Desc, false) => "<",
        };
        query
            .push(" AND rollout_ordinal ")
            .push(comparator)
            .push(" ")
            .push_bind(sqlite_integer(cursor.rollout_ordinal)?);
    }
    Ok(())
}

fn push_order_and_limit(query: &mut QueryBuilder<Sqlite>, direction: SortDirection, limit: i64) {
    let order = match direction {
        SortDirection::Asc => "ASC",
        SortDirection::Desc => "DESC",
    };
    query
        .push(" ORDER BY rollout_ordinal ")
        .push(order)
        .push(" LIMIT ")
        .push_bind(limit);
}

fn remaining_limit(page_size: usize, row_count: usize) -> ThreadStoreResult<i64> {
    let limit = page_size
        .checked_add(1)
        .and_then(|limit| limit.checked_sub(row_count))
        .ok_or_else(page_size_too_large)?;
    i64::try_from(limit).map_err(|_| page_size_too_large())
}

pub(super) fn finish_page_with_more<T: HasPosition>(
    requested_thread_id: ThreadId,
    scope: CursorScope,
    mut rows: Vec<T>,
    page_size: usize,
    has_more: bool,
) -> ThreadStoreResult<SegmentPage<T>> {
    rows.truncate(page_size);
    let backwards_cursor = rows
        .first()
        .map(|row| {
            serialize_cursor(
                requested_thread_id,
                scope.clone(),
                row.position().rollout_ordinal,
                /*include_anchor*/ true,
            )
        })
        .transpose()?;
    let next_cursor = if has_more {
        rows.last()
            .map(|row| {
                serialize_cursor(
                    requested_thread_id,
                    scope,
                    row.position().rollout_ordinal,
                    /*include_anchor*/ false,
                )
            })
            .transpose()?
    } else {
        None
    };
    Ok(SegmentPage {
        rows,
        next_cursor,
        backwards_cursor,
    })
}

/// A page-size target, never an item-readability limit. An indivisible first
/// item or summary turn may exceed it; explicit ID reads have no continuation.
#[derive(Default)]
pub(super) struct HistoryPageByteTarget {
    bytes: usize,
    entries: usize,
    unpaged: bool,
}

impl HistoryPageByteTarget {
    pub(super) fn for_item_lookup() -> Self {
        Self {
            unpaged: true,
            ..Self::default()
        }
    }

    pub(super) fn can_add(&self, bytes: usize) -> bool {
        self.unpaged
            || self.entries == 0
            || self.bytes.saturating_add(bytes) <= HISTORY_PAGE_TARGET_BYTES
    }

    pub(super) fn push(&mut self, bytes: usize) {
        self.bytes = self.bytes.saturating_add(bytes);
        self.entries = self.entries.saturating_add(1);
    }
}

async fn ensure_projection_available(
    transaction: &mut sqlx::Transaction<'_, Sqlite>,
    lineage: &RolloutLineage,
) -> ThreadStoreResult<()> {
    for segment in lineage.segments() {
        let checkpoint = sqlx::query_scalar::<_, i64>(
            "SELECT 1 FROM thread_history_projection_state WHERE thread_id = ?",
        )
        .bind(segment.rollout_id().to_string())
        .fetch_optional(&mut **transaction)
        .await
        .map_err(thread_history_error)?;
        if checkpoint.is_none() {
            return Err(ThreadStoreError::HistoryProjectionUnavailable);
        }
    }
    Ok(())
}

pub(super) async fn load_admitted_item(
    transaction: &mut sqlx::Transaction<'_, Sqlite>,
    rollout_id: ThreadId,
    candidate: sqlx::sqlite::SqliteRow,
    budget: &mut HistoryPageByteTarget,
) -> ThreadStoreResult<Option<StoredThreadItemRow>> {
    let bytes = usize::try_from(candidate.try_get::<i64, _>("item_json_bytes")?)
        .map_err(thread_history_error)?;
    if !budget.can_add(bytes) {
        return Ok(None);
    }
    // No payload is fetched until its exact UTF-8 byte length has passed admission.
    // This SELECT shares the probe's transaction, including concurrent item updates.
    let row = sqlx::query("SELECT turn_id, item_id, rollout_ordinal, updated_at_ordinal, created_at_ms, started_at_ms, completed_at_ms, item_json FROM thread_items WHERE thread_id = ? AND turn_id = ? AND item_id = ?")
        .bind(rollout_id.to_string())
        .bind(candidate.try_get::<String, _>("turn_id")?)
        .bind(candidate.try_get::<String, _>("item_id")?)
        .fetch_one(&mut **transaction).await.map_err(thread_history_error)?;
    let row = stored_thread_item_row(row)?;
    budget.push(bytes);
    Ok(Some(row))
}

pub(super) trait HasPosition {
    fn position(&self) -> RolloutHistoryPosition;
}

impl HasPosition for StoredTurnRow {
    fn position(&self) -> RolloutHistoryPosition {
        self.position
    }
}

impl HasPosition for StoredThreadItemRow {
    fn position(&self) -> RolloutHistoryPosition {
        self.position
    }
}

fn sqlite_integer(value: u64) -> ThreadStoreResult<i64> {
    i64::try_from(value).map_err(|_| ThreadStoreError::InvalidRequest {
        message: "rollout ordinal exceeds SQLite integer range".to_string(),
    })
}

fn page_size_too_large() -> ThreadStoreError {
    ThreadStoreError::InvalidRequest {
        message: "page size is too large".to_string(),
    }
}

fn invalid_item_anchor() -> ThreadStoreError {
    ThreadStoreError::InvalidRequest {
        message: "cursor.itemId does not identify an item in the requested history scope"
            .to_string(),
    }
}
