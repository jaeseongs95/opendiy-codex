// Modified for OpenDIY Codex R31 (2026-11-01); see MODIFICATIONS.md.
//! Selects a coherent memory pipeline and its generated-artifact namespace.

use schemars::JsonSchema;
use serde::Deserialize;
use serde::Serialize;
use ts_rs::TS;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "lowercase")]
pub enum MemoryVersion {
    #[default]
    V1,
    V2,
}

impl MemoryVersion {
    /// Sibling roots keep v1 cleanup and rollback independent of v2 artifacts.
    pub fn directory_name(self) -> &'static str {
        match self {
            Self::V1 => "memories",
            Self::V2 => "memories_v2",
        }
    }
}
