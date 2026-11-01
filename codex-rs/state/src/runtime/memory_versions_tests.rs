// Modified for OpenDIY Codex R32 (2026-11-01); see MODIFICATIONS.md.
use super::*;
use crate::SqliteConfig;
use crate::Stage1JobClaimOutcome;
use crate::runtime::test_support::test_thread_metadata;
use codex_utils_absolute_path::test_support::PathExt;
use pretty_assertions::assert_eq;

#[tokio::test]
async fn versions_isolate_jobs_outputs_and_reset_without_losing_threads() -> anyhow::Result<()> {
    let home = crate::runtime::test_support::unique_temp_dir();
    let sqlite = SqliteConfig::new_for_testing(home.as_path().abs());
    let db = StateRuntime::init(sqlite.clone(), "test-provider".to_string()).await?;
    assert!(!sqlite.memories_v2_db_path().exists());
    let thread_id = ThreadId::new();
    let metadata = test_thread_metadata(home.as_path(), thread_id, home.as_path().join("project"));
    db.upsert_thread(&metadata).await?;

    for version in [MemoryVersion::V1, MemoryVersion::V2] {
        let store = db.memories_for_version(version).await?;
        let Stage1JobClaimOutcome::Claimed { ownership_token } = store
            .try_claim_stage1_job(
                thread_id,
                thread_id,
                metadata.updated_at.timestamp(),
                /*lease_seconds*/ 60,
                /*max_running_jobs*/ 1,
            )
            .await?
        else {
            panic!("each version must independently claim the same source")
        };
        assert!(
            store
                .mark_stage1_job_succeeded(
                    thread_id,
                    &ownership_token,
                    metadata.updated_at.timestamp(),
                    "raw",
                    version.directory_name(),
                    /*rollout_slug*/ None
                )
                .await?
        );
    }
    for version in [MemoryVersion::V1, MemoryVersion::V2] {
        let outputs = db
            .memories_for_version(version)
            .await?
            .list_stage1_outputs_for_global(/*n*/ 10)
            .await?;
        assert_eq!(
            outputs
                .iter()
                .map(|output| output.rollout_summary.as_str())
                .collect::<Vec<_>>(),
            vec![version.directory_name()]
        );
    }
    // Reopening under v1 must still clear existing v2 state.
    db.close().await;
    let db = StateRuntime::init(sqlite.clone(), "test-provider".to_string()).await?;
    db.delete_thread(thread_id).await?;
    for version in [MemoryVersion::V1, MemoryVersion::V2] {
        assert!(
            db.memories_for_version(version)
                .await?
                .list_stage1_outputs_for_global(/*n*/ 10)
                .await?
                .is_empty()
        );
    }
    db.upsert_thread(&metadata).await?;
    for version in [MemoryVersion::V1, MemoryVersion::V2] {
        let store = db.memories_for_version(version).await?;
        let Stage1JobClaimOutcome::Claimed { ownership_token } = store
            .try_claim_stage1_job(
                thread_id,
                thread_id,
                metadata.updated_at.timestamp(),
                /*lease_seconds*/ 60,
                /*max_running_jobs*/ 1,
            )
            .await?
        else {
            panic!("deletion must clear the job watermark")
        };
        store
            .mark_stage1_job_succeeded(
                thread_id,
                &ownership_token,
                metadata.updated_at.timestamp(),
                "raw",
                "summary",
                /*rollout_slug*/ None,
            )
            .await?;
    }
    db.clear_all_memory_data().await?;
    for version in [MemoryVersion::V1, MemoryVersion::V2] {
        assert!(
            db.memories_for_version(version)
                .await?
                .list_stage1_outputs_for_global(/*n*/ 10)
                .await?
                .is_empty()
        );
    }
    assert!(db.get_thread(thread_id).await?.is_some());
    db.close().await;
    assert!(StateRuntime::clear_memory_data_in_sqlite_home(&sqlite).await?);
    tokio::fs::remove_dir_all(home).await?;
    Ok(())
}

#[tokio::test]
async fn project_stores_share_only_source_metadata_and_filter_before_scan_limit()
-> anyhow::Result<()> {
    let home = crate::runtime::test_support::unique_temp_dir();
    let sqlite = SqliteConfig::new_for_testing(home.as_path().abs());
    let db = StateRuntime::init(sqlite.clone(), "test-provider".to_string()).await?;
    let ids = [ThreadId::new(), ThreadId::new(), ThreadId::new()];
    let roots = [home.join("a"), home.join("a-worktree"), home.join("b")];
    for (index, (id, cwd)) in ids.iter().zip(roots.iter()).enumerate() {
        let mut metadata = test_thread_metadata(&home, *id, cwd.clone());
        metadata.updated_at = chrono::Utc::now() - chrono::Duration::hours(4 - index as i64);
        db.upsert_thread(&metadata).await?;
    }
    assert_eq!(db.memory_source_candidates().await?.len(), 3);
    let key_a = "a".repeat(64);
    let key_b = "b".repeat(64);
    // A large resolved catalog must use one SQLite bind, not one per source id.
    let mut eligible_a = (0..99_998).map(|_| ThreadId::new()).collect::<Vec<_>>();
    eligible_a.extend([ids[0], ids[1]]);
    let a = db
        .memories_for_project(MemoryVersion::V2, &key_a)
        .await?
        .with_eligible_threads(eligible_a);
    let b = db
        .memories_for_project(MemoryVersion::V2, &key_b)
        .await?
        .with_eligible_threads(vec![ids[2]]);
    let owner = ThreadId::new();
    let allowed_sources = vec!["cli".to_string()];
    let params = crate::Stage1StartupClaimParams {
        scan_limit: 1,
        max_claimed: 1,
        max_age_days: 30,
        min_rollout_idle_hours: 1,
        allowed_sources: &allowed_sources,
        lease_seconds: 60,
    };
    // B is newest globally, but must not consume A's scan limit.
    let claims_a = a.claim_stage1_jobs_for_startup(owner, params).await?;
    let claims_b = b.claim_stage1_jobs_for_startup(owner, params).await?;
    assert_eq!(claims_a.len(), 1);
    assert_eq!(claims_a[0].thread.id, ids[1]);
    assert_eq!(claims_b.len(), 1);
    assert_eq!(claims_b[0].thread.id, ids[2]);
    for (store, claim, summary) in [
        (&a, &claims_a[0], "A memory"),
        (&b, &claims_b[0], "B memory"),
    ] {
        assert!(
            store
                .mark_stage1_job_succeeded(
                    claim.thread.id,
                    &claim.ownership_token,
                    claim.thread.updated_at.timestamp(),
                    "raw",
                    summary,
                    None,
                )
                .await?
        );
    }
    let outputs_a = a.get_phase2_input_selection(10, 30).await?;
    let outputs_b = b.list_stage1_outputs_for_global(10).await?;
    assert_eq!(
        outputs_a
            .iter()
            .map(|o| o.rollout_summary.as_str())
            .collect::<Vec<_>>(),
        vec!["A memory"]
    );
    assert_eq!(outputs_a[0].cwd, roots[1]);
    assert_eq!(
        outputs_b
            .iter()
            .map(|o| o.rollout_summary.as_str())
            .collect::<Vec<_>>(),
        vec!["B memory"]
    );
    assert!(
        db.memories()
            .list_stage1_outputs_for_global(10)
            .await?
            .is_empty()
    );
    assert!(
        db.memories_for_project(MemoryVersion::V1, &key_a)
            .await?
            .with_eligible_threads(vec![ids[0], ids[1]])
            .list_stage1_outputs_for_global(10)
            .await?
            .is_empty()
    );

    // Observe real database boundaries: project memory DBs contain no source catalog.
    for (key, expected) in [(&key_a, ids[1]), (&key_b, ids[2])] {
        let path = home
            .join("project_memories")
            .join(key)
            .join(MemoryVersion::V2.directory_name())
            .join("memories.sqlite");
        let pool = sqlite.open_read_only_pool(&path, None).await?;
        let stored: Vec<String> = sqlx::query_scalar("SELECT thread_id FROM stage1_outputs")
            .fetch_all(&pool)
            .await?;
        assert_eq!(stored, vec![expected.to_string()]);
        let catalog: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM sqlite_schema WHERE name = 'threads'")
                .fetch_one(&pool)
                .await?;
        assert_eq!(catalog, 0);
        pool.close().await;
    }
    let state_pool = sqlite
        .open_read_only_pool(&sqlite.state_db_path(), None)
        .await?;
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM threads")
            .fetch_one(&state_pool)
            .await?,
        3
    );
    state_pool.close().await;

    // Changed membership and disabled sources are checked during hydration too.
    assert!(
        a.clone()
            .with_eligible_threads(vec![ids[0]])
            .get_phase2_input_selection(10, 30)
            .await?
            .is_empty()
    );
    db.set_thread_memory_mode(ids[1], "disabled").await?;
    assert!(a.list_stage1_outputs_for_global(10).await?.is_empty());
    db.set_thread_memory_mode(ids[1], "enabled").await?;
    assert_eq!(a.list_stage1_outputs_for_global(10).await?.len(), 1);
    let closed = db.memories_for_project(MemoryVersion::V2, &key_a).await?;
    assert!(closed.list_stage1_outputs_for_global(10).await?.is_empty());
    assert!(
        closed
            .try_claim_stage1_job(ids[2], owner, 1, 60, 1)
            .await
            .is_err()
    );
    drop(closed);

    // Dropping one clone cannot close a pool another consumer still owns.
    let surviving = a.clone();
    drop(a);
    assert_eq!(surviving.list_stage1_outputs_for_global(10).await?.len(), 1);
    drop(surviving);
    let resumed = db
        .memories_for_project(MemoryVersion::V2, &key_a)
        .await?
        .with_eligible_threads(vec![ids[0], ids[1]]);
    assert_eq!(resumed.list_stage1_outputs_for_global(10).await?.len(), 1);
    db.clear_all_memory_data().await?;
    assert_eq!(resumed.list_stage1_outputs_for_global(10).await?.len(), 1);
    db.delete_project_thread_memory(MemoryVersion::V2, &key_a, ids[1])
        .await?;
    assert!(resumed.list_stage1_outputs_for_global(10).await?.is_empty());
    assert_eq!(b.list_stage1_outputs_for_global(10).await?.len(), 1);
    db.clear_project_memory_data(MemoryVersion::V2, &key_b)
        .await?;
    assert!(b.list_stage1_outputs_for_global(10).await?.is_empty());
    // Reset/delete use fail-closed stores with no eligible sources, but must still erase rows.
    for key in [&key_a, &key_b] {
        let path = home
            .join("project_memories")
            .join(key)
            .join(MemoryVersion::V2.directory_name())
            .join("memories.sqlite");
        let pool = sqlite.open_read_only_pool(&path, None).await?;
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM stage1_outputs")
                .fetch_one(&pool)
                .await?,
            0
        );
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM jobs WHERE kind = 'memory_stage1'")
                .fetch_one(&pool)
                .await?,
            0
        );
        pool.close().await;
    }
    assert!(db.get_thread(ids[2]).await?.is_some());
    drop(resumed);
    drop(b);
    db.close().await;
    tokio::fs::remove_dir_all(home).await?;
    Ok(())
}

#[tokio::test]
async fn project_claims_fence_stale_owners_across_independent_pools() -> anyhow::Result<()> {
    let home = crate::runtime::test_support::unique_temp_dir();
    let sqlite = SqliteConfig::new_for_testing(home.as_path().abs());
    let db = StateRuntime::init(sqlite.clone(), "test-provider".to_string()).await?;
    let id = ThreadId::new();
    db.upsert_thread(&test_thread_metadata(&home, id, home.join("a")))
        .await?;
    let key = "c".repeat(64);
    let a = db
        .memories_for_project(MemoryVersion::V2, &key)
        .await?
        .with_eligible_threads(vec![id]);
    let b = db
        .memories_for_project(MemoryVersion::V2, &key)
        .await?
        .with_eligible_threads(vec![id]);
    let (first, second) = tokio::join!(
        a.try_claim_stage1_job(id, ThreadId::new(), 100, 60, 1),
        b.try_claim_stage1_job(id, ThreadId::new(), 100, 60, 1),
    );
    let results = [first?, second?];
    assert_eq!(
        results
            .iter()
            .filter(|r| matches!(r, Stage1JobClaimOutcome::Claimed { .. }))
            .count(),
        1
    );
    let old_token = results
        .iter()
        .find_map(|r| match r {
            Stage1JobClaimOutcome::Claimed { ownership_token } => Some(ownership_token.clone()),
            _ => None,
        })
        .expect("one winner");
    let path = home
        .join("project_memories")
        .join(&key)
        .join(MemoryVersion::V2.directory_name())
        .join("memories.sqlite");
    let pool = sqlite.open_read_write_pool(&path).await?;
    sqlx::query("UPDATE jobs SET lease_until = 0 WHERE kind = 'memory_stage1'")
        .execute(&pool)
        .await?;
    let Stage1JobClaimOutcome::Claimed {
        ownership_token: new_token,
    } = b
        .try_claim_stage1_job(id, ThreadId::new(), 101, 60, 1)
        .await?
    else {
        panic!("expired lease must be reassignable")
    };
    assert!(
        !a.mark_stage1_job_succeeded(id, &old_token, 100, "stale", "stale", None)
            .await?
    );
    assert!(
        b.mark_stage1_job_succeeded(id, &new_token, 101, "fresh", "fresh", None)
            .await?
    );
    let summary: String = sqlx::query_scalar("SELECT rollout_summary FROM stage1_outputs")
        .fetch_one(&pool)
        .await?;
    assert_eq!(summary, "fresh");
    pool.close().await;
    drop(a);
    drop(b);
    db.close().await;
    tokio::fs::remove_dir_all(home).await?;
    Ok(())
}

#[tokio::test]
async fn project_keys_reject_path_inputs_before_creating_directories() -> anyhow::Result<()> {
    let home = crate::runtime::test_support::unique_temp_dir();
    let sqlite = SqliteConfig::new_for_testing(home.as_path().abs());
    let db = StateRuntime::init(sqlite, "test-provider".to_string()).await?;
    for key in ["", "../other", "C:/outside", "gggg", &"a".repeat(65)] {
        assert!(
            db.memories_for_project(MemoryVersion::V2, key)
                .await
                .is_err()
        );
    }
    assert!(!home.join("project_memories").exists());
    db.close().await;
    tokio::fs::remove_dir_all(home).await?;
    Ok(())
}

#[tokio::test]
async fn memory_source_catalog_limit_fails_explicitly_without_truncation() -> anyhow::Result<()> {
    let home = crate::runtime::test_support::unique_temp_dir();
    let sqlite = SqliteConfig::new_for_testing(home.as_path().abs());
    let db = StateRuntime::init(sqlite.clone(), "test-provider".to_string()).await?;
    let pool = sqlite.open_read_write_pool(&sqlite.state_db_path()).await?;
    sqlx::query(
        r#"
WITH RECURSIVE source_ids(n) AS (
    VALUES(1) UNION ALL SELECT n + 1 FROM source_ids WHERE n < 100000
)
INSERT INTO threads (
    id, rollout_path, created_at, updated_at, source, model_provider, cwd,
    title, sandbox_policy, approval_mode
)
SELECT printf('00000000-0000-4000-8000-%012x', n), '', 0, 0, 'cli', 'test-provider',
    'project', '', '', '' FROM source_ids
        "#,
    )
    .execute(&pool)
    .await?;
    assert_eq!(db.memory_source_candidates().await?.len(), 100_000);
    db.upsert_thread(&test_thread_metadata(
        &home,
        ThreadId::new(),
        home.join("a"),
    ))
    .await?;
    let error = db
        .memory_source_candidates()
        .await
        .expect_err("oversized catalog must fail closed");
    assert!(error.to_string().contains("exceeds 100000"));
    pool.close().await;
    db.close().await;
    tokio::fs::remove_dir_all(home).await?;
    Ok(())
}
