// Modified for OpenDIY Codex R30 (2026-10-30); see MODIFICATIONS.md.
//! Explicit browsing and user corrections for the loaded thread's memory scope.
use super::*;
use codex_app_server_protocol::ThreadMemoryNoteAddParams;
use codex_app_server_protocol::ThreadMemoryNoteAddResponse;
use codex_app_server_protocol::ThreadMemoryReadParams;
use codex_app_server_protocol::ThreadMemoryReadResponse;
use std::io::Read;

use codex_core::config::MEMORY_SUMMARY_MAX_BYTES as MAX_SUMMARY_BYTES;

impl ThreadRequestProcessor {
    pub(crate) async fn thread_memory_read(
        &self,
        params: ThreadMemoryReadParams,
    ) -> Result<Option<ClientResponsePayload>, JSONRPCErrorError> {
        let (thread_id, thread) = self.load_thread(&params.thread_id).await?;
        let config = thread.config().await;
        let snapshot = thread.config_snapshot().await;
        let root = config
            .memories
            .memory_root(&config.codex_home)
            .filter(|_| config.memory_environment_supported(snapshot.environment_selections()));
        let scope = if config.memories.scope == codex_config::types::MemoryScope::Project {
            "project"
        } else {
            "global"
        };
        let enabled = config.features.enabled(Feature::MemoryTool);
        let (summary, status) = if root.is_none() {
            (None, "unavailable")
        } else if !enabled || !config.memories.use_memories && params.include_disabled != Some(true)
        {
            (None, "disabled")
        } else if root.as_ref().is_some_and(|root| {
            codex_core::config::validate_scoped_memory_root(root.as_path()).is_err()
        }) {
            (None, "unreadable")
        } else {
            let path = root
                .as_ref()
                .expect("checked root")
                .join("memory_summary.md");
            tokio::task::spawn_blocking(move || {
                let file = match std::fs::File::open(path) {
                    Ok(file) => file,
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                        return (None, "missing");
                    }
                    Err(_) => return (None, "unreadable"),
                };
                let mut bytes = Vec::new();
                if file
                    .take(MAX_SUMMARY_BYTES + 1)
                    .read_to_end(&mut bytes)
                    .is_err()
                {
                    return (None, "unreadable");
                }
                if bytes.len() as u64 > MAX_SUMMARY_BYTES {
                    return (None, "oversized");
                }
                match String::from_utf8(bytes) {
                    Ok(summary) if !summary.trim().is_empty() => (Some(summary), "ready"),
                    Ok(_) => (None, "missing"),
                    Err(_) => (None, "unreadable"),
                }
            })
            .await
            .map_err(|error| internal_error(format!("memory read failed: {error}")))?
        };
        let generate = if let Some(db) = thread.state_db() {
            db.get_thread_memory_mode(thread_id)
                .await
                .map_err(|error| {
                    internal_error(format!("memory generation state unavailable: {error}"))
                })?
                .map_or(config.memories.generate_memories, |mode| mode == "enabled")
        } else {
            false
        };
        Ok(Some(
            ThreadMemoryReadResponse {
                summary,
                scope: Some(scope.to_string()),
                root: root.as_ref().map(|root| root.display().to_string()),
                status: Some(status.to_string()),
                use_memories: Some(enabled && config.memories.use_memories),
                generate_memories: Some(enabled && generate),
            }
            .into(),
        ))
    }

    pub(crate) async fn thread_memory_note_add(
        &self,
        params: ThreadMemoryNoteAddParams,
    ) -> Result<Option<ClientResponsePayload>, JSONRPCErrorError> {
        let (_, thread) = self.load_thread(&params.thread_id).await?;
        let config = thread.config().await;
        let snapshot = thread.config_snapshot().await;
        if !config.features.enabled(Feature::MemoryTool)
            || !config.memory_environment_supported(snapshot.environment_selections())
        {
            return Err(invalid_params(
                "memory is disabled or unavailable for this environment",
            ));
        }
        let root = config
            .memories
            .memory_root(&config.codex_home)
            .ok_or_else(|| invalid_params("project memory identity is unavailable"))?;
        codex_memories_write::add_user_memory_note(root.as_path(), &params.note)
            .await
            .map_err(|error| invalid_params(format!("memory note was not saved: {error}")))?;
        Ok(Some(
            ThreadMemoryNoteAddResponse {
                pending_consolidation: true,
            }
            .into(),
        ))
    }
}
