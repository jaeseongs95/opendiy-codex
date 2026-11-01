// Modified for OpenDIY Codex R32 (2026-11-01); see MODIFICATIONS.md.
//! Memory readiness and summary reads using a loaded thread's selected pipeline.

use crate::JsonSchema;
use crate::TS;
use serde::Deserialize;
use serde::Serialize;

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct ThreadMemoryReadParams {
    /// The thread must already be started or resumed.
    pub thread_id: String,
    /// Explicit browsing of stored memory, even when automatic injection is disabled.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional = nullable)]
    pub include_disabled: Option<bool>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct ThreadMemoryReadResponse {
    /// Summary from the thread's selected memory version. Null when memory use is
    /// disabled or the selected summary is missing, unreadable, or blank.
    pub summary: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub scope: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub root: Option<String>,
    /// ready, missing, disabled, unavailable, unreadable, or oversized.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub status: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub use_memories: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub generate_memories: Option<bool>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct ThreadMemoryNoteAddParams {
    pub thread_id: String,
    /// An explicit user's addition, correction, or forget request; at most 16 KiB.
    pub note: String,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct ThreadMemoryNoteAddResponse {
    /// Saved for later consolidation. This is not a confirmation that the summary changed.
    pub pending_consolidation: bool,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct MemoryStatusParams {
    /// Required distinct consolidated threads. Defaults to 20; supported range is 1..=4096.
    #[ts(optional = nullable)]
    pub min_consolidated_threads: Option<u32>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct MemoryStatusResponse {
    pub v2_consolidated_threads: u32,
    pub v2_ready: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn memory_read_response_preserves_optional_metadata_and_legacy_decode() {
        let response: ThreadMemoryReadResponse =
            serde_json::from_value(json!({"summary": null})).unwrap();
        assert_eq!(
            serde_json::to_value(response).unwrap(),
            json!({"summary": null})
        );
        assert!(
            serde_json::from_value::<ThreadMemoryReadParams>(json!({"threadId":"fixture"}))
                .unwrap()
                .include_disabled
                .is_none()
        );
    }
}
