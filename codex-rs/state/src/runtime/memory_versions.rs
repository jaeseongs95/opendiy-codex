// Modified for OpenDIY Codex R31 (2026-11-01); see MODIFICATIONS.md.
//! Creates isolated v2 memory state on demand while sharing the source thread catalog.
//! Legacy reset and thread deletion cover all existing global versions.
//! Project stores own their pools without a process-wide scope cache.

use super::MemoryStore;
use super::StateRuntime;
use codex_protocol::MemoryVersion;
use codex_protocol::ThreadId;
use sqlx::Row;
use std::path::PathBuf;
use std::sync::Arc;

impl StateRuntime {
    pub async fn memories_for_version(
        &self,
        version: MemoryVersion,
    ) -> anyhow::Result<MemoryStore> {
        match version {
            MemoryVersion::V1 => Ok(self.memories.clone()),
            MemoryVersion::V2 => self
                .memories_v2
                .get_or_try_init(|| async {
                    let pool = self.sqlite.open_memories_v2_db().await?;
                    Ok(MemoryStore::new(Arc::new(pool), Arc::clone(&self.pool)))
                })
                .await
                .cloned(),
        }
    }

    /// Project jobs and outputs are isolated; source threads remain in the global catalog.
    /// The returned store owns its pool and is fail-closed until its source ids are supplied.
    pub async fn memories_for_project(
        &self,
        version: MemoryVersion,
        project_key: &str,
    ) -> anyhow::Result<MemoryStore> {
        let pool = self
            .sqlite
            .open_project_memories_db(version, project_key)
            .await?;
        Ok(MemoryStore::new(Arc::new(pool), Arc::clone(&self.pool))
            .with_eligible_threads(Vec::new()))
    }

    /// Return only source identities for project matching, without reading conversation data.
    /// Do not truncate before identity matching: unrelated projects must not starve a scope.
    pub async fn memory_source_candidates(&self) -> anyhow::Result<Vec<(ThreadId, PathBuf)>> {
        let rows =
            sqlx::query("SELECT id, cwd FROM threads WHERE memory_mode = 'enabled' LIMIT 100001")
                .fetch_all(self.pool.as_ref())
                .await?;
        anyhow::ensure!(
            rows.len() <= 100_000,
            "memory source catalog exceeds 100000 threads"
        );
        rows.into_iter()
            .map(|row| {
                let id: String = row.try_get("id")?;
                let cwd: String = row.try_get("cwd")?;
                Ok((ThreadId::try_from(id.as_str())?, PathBuf::from(cwd)))
            })
            .collect()
    }

    /// Reset only the explicitly selected project/version; legacy reset remains global.
    pub async fn clear_project_memory_data(
        &self,
        version: MemoryVersion,
        project_key: &str,
    ) -> anyhow::Result<()> {
        self.memories_for_project(version, project_key)
            .await?
            .clear_memory_data()
            .await
    }

    /// Delete one project's source output without modifying the global thread catalog.
    pub async fn delete_project_thread_memory(
        &self,
        version: MemoryVersion,
        project_key: &str,
        thread_id: ThreadId,
    ) -> anyhow::Result<()> {
        self.memories_for_project(version, project_key)
            .await?
            .delete_thread_memory(thread_id)
            .await
    }

    pub async fn clear_all_memory_data(&self) -> anyhow::Result<()> {
        self.memories.clear_memory_data().await?;
        if tokio::fs::try_exists(self.sqlite.memories_v2_db_path()).await? {
            self.memories_for_version(MemoryVersion::V2)
                .await?
                .clear_memory_data()
                .await?;
        }
        Ok(())
    }

    pub(super) async fn delete_versioned_thread_memory(
        &self,
        thread_id: ThreadId,
    ) -> anyhow::Result<()> {
        self.memories.delete_thread_memory(thread_id).await?;
        if tokio::fs::try_exists(self.sqlite.memories_v2_db_path()).await? {
            self.memories_for_version(MemoryVersion::V2)
                .await?
                .delete_thread_memory(thread_id)
                .await?;
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "memory_versions_tests.rs"]
mod tests;
