// Modified for OpenDIY Codex R32 (2026-11-01); see MODIFICATIONS.md.
//! Host operator controls. These types contain metadata only, never conversation content.

use codex_protocol::ThreadId;
use codex_protocol::openai_models::ReasoningEffort;
use codex_protocol::user_input::UserInput;
use uuid::Uuid;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ManagedSubagentStatus {
    Pending,
    Running,
    Idle,
    Completed,
    Errored,
    Closed,
    Unknown,
}

#[derive(Clone, Debug, Default)]
pub struct ManagedSubagentCapabilities {
    pub send: bool,
    pub interrupt: bool,
    pub close: bool,
    pub resume: bool,
}

#[derive(Clone, Debug)]
pub struct ManagedSubagent {
    pub thread_id: ThreadId,
    pub parent_thread_id: ThreadId,
    pub runtime_id: Option<Uuid>,
    pub nickname: Option<String>,
    pub role: Option<String>,
    pub model: Option<String>,
    pub reasoning_effort: Option<ReasoningEffort>,
    pub status: ManagedSubagentStatus,
    pub active_turn_id: Option<String>,
    pub capabilities: ManagedSubagentCapabilities,
}

#[derive(Clone, Debug)]
pub struct ManagedSubagentList {
    pub parent_runtime_id: Uuid,
    pub agents: Vec<ManagedSubagent>,
}

#[derive(Clone, Debug)]
pub enum ManagedSubagentAction {
    /// Explicit operator input keeps user provenance; it cannot impersonate an agent message.
    Send {
        input: Vec<UserInput>,
    },
    Interrupt {
        expected_turn_id: String,
    },
    Close,
    Resume,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ManagedSubagentControlOutcome {
    Applied,
    NotApplied,
    Unknown,
    Unsupported,
}
