use super::invalid_request;
use super::thread_processor::ThreadRequestProcessor;
use codex_app_server_protocol::ClientResponsePayload;
use codex_app_server_protocol::JSONRPCErrorError;
use codex_app_server_protocol::SubagentControlStatus;
use codex_app_server_protocol::SubagentManagementAction;
use codex_app_server_protocol::SubagentManagementAgent;
use codex_app_server_protocol::SubagentManagementCapabilities;
use codex_app_server_protocol::SubagentManagementStatus;
use codex_app_server_protocol::SubagentMutationState;
use codex_app_server_protocol::ThreadSubagentsControlParams;
use codex_app_server_protocol::ThreadSubagentsControlResponse;
use codex_app_server_protocol::ThreadSubagentsListParams;
use codex_app_server_protocol::ThreadSubagentsListResponse;
use codex_core::ManagedSubagentAction;
use codex_core::ManagedSubagentControlOutcome;
use codex_core::ManagedSubagentStatus;
use codex_protocol::ThreadId;
use codex_protocol::user_input::UserInput;
use sha2::Digest;
use sha2::Sha256;
use std::collections::HashMap;
use uuid::Uuid;

const MAX_RECEIPTS: usize = 1024;
const MAX_TEXT_BYTES: usize = 64 * 1024;
const MAX_TURN_ID_BYTES: usize = 1024;
const MAX_CURSOR_BYTES: usize = 8192;

struct MutationReceipt {
    parent: ThreadId,
    runtime: Uuid,
    fingerprint: [u8; 32],
    status: SubagentControlStatus,
}

#[derive(Default)]
pub(super) struct SubagentMutationReceipts {
    receipts: HashMap<Uuid, MutationReceipt>,
}

impl SubagentMutationReceipts {
    fn reserve(
        &mut self,
        id: Uuid,
        parent: ThreadId,
        runtime: Uuid,
        fingerprint: [u8; 32],
    ) -> Result<Option<SubagentControlStatus>, JSONRPCErrorError> {
        if let Some(receipt) = self.receipts.get(&id) {
            if receipt.parent != parent
                || receipt.runtime != runtime
                || receipt.fingerprint != fingerprint
            {
                return Err(invalid_request(
                    "requestId was already used for another command",
                ));
            }
            return Ok(Some(receipt.status));
        }
        // Never evict accepted IDs: a late replay must not execute twice. At this
        // server-lifetime ceiling, callers must reconnect to a fresh server runtime.
        if self.receipts.len() >= MAX_RECEIPTS {
            return Ok(Some(SubagentControlStatus::Unsupported));
        }
        let rejected = self.has_unknown(parent, runtime);
        let status = if rejected {
            SubagentControlStatus::NotApplied
        } else {
            SubagentControlStatus::Unknown
        };
        self.receipts.insert(
            id,
            MutationReceipt {
                parent,
                runtime,
                fingerprint,
                status,
            },
        );
        Ok(rejected.then_some(SubagentControlStatus::NotApplied))
    }

    fn finish(&mut self, id: Uuid, status: SubagentControlStatus) {
        if let Some(receipt) = self.receipts.get_mut(&id) {
            receipt.status = status;
        }
    }

    fn has_unknown(&self, parent: ThreadId, runtime: Uuid) -> bool {
        self.receipts.values().any(|receipt| {
            receipt.parent == parent
                && receipt.runtime == runtime
                && receipt.status == SubagentControlStatus::Unknown
        })
    }

    fn reconcile(
        &self,
        parent: ThreadId,
        runtime: Uuid,
        request_id: Option<Uuid>,
    ) -> (SubagentMutationState, Option<String>) {
        if self.has_unknown(parent, runtime) {
            return (SubagentMutationState::Unknown, None);
        }
        match request_id {
            Some(id) => match self.receipts.get(&id) {
                Some(receipt)
                    if receipt.parent == parent
                        && receipt.runtime == runtime
                        && receipt.status != SubagentControlStatus::Unknown =>
                {
                    (SubagentMutationState::Clear, Some(id.to_string()))
                }
                _ => (SubagentMutationState::Unknown, None),
            },
            None => (SubagentMutationState::Clear, None),
        }
    }
}

fn parse_uuid(value: &str, field: &str) -> Result<Uuid, JSONRPCErrorError> {
    Uuid::parse_str(value).map_err(|_| invalid_request(format!("invalid {field}")))
}

fn parse_thread_id(value: &str) -> Result<ThreadId, JSONRPCErrorError> {
    ThreadId::from_string(value).map_err(|_| invalid_request("invalid thread id"))
}

fn bounded_metadata(value: Option<String>, max_bytes: usize) -> Option<String> {
    value.filter(|value| value.len() <= max_bytes && !value.chars().any(char::is_control))
}

fn validate_command(params: &ThreadSubagentsControlParams) -> Result<(), JSONRPCErrorError> {
    match params.action {
        SubagentManagementAction::Send => {
            if params.text.as_ref().is_none_or(|text| {
                text.trim().is_empty() || text.len() > MAX_TEXT_BYTES || text.contains('\0')
            }) || params.expected_turn_id.is_some()
                || params.expected_runtime_id.is_none()
            {
                return Err(invalid_request(
                    "send requires a live runtime and 1..65536 UTF-8 bytes of text",
                ));
            }
        }
        SubagentManagementAction::Interrupt => {
            if params
                .expected_turn_id
                .as_ref()
                .is_none_or(|id| id.is_empty() || id.len() > MAX_TURN_ID_BYTES || id.contains('\0'))
                || params.text.is_some()
                || params.expected_runtime_id.is_none()
            {
                return Err(invalid_request(
                    "interrupt requires a live runtime and expectedTurnId",
                ));
            }
        }
        SubagentManagementAction::Close => {
            if params.text.is_some() || params.expected_turn_id.is_some() {
                return Err(invalid_request(
                    "close does not accept text or expectedTurnId",
                ));
            }
        }
        SubagentManagementAction::Resume => {
            if params.text.is_some()
                || params.expected_turn_id.is_some()
                || params.expected_runtime_id.is_some()
            {
                return Err(invalid_request(
                    "resume requires expectedRuntimeId: null and no input",
                ));
            }
        }
    }
    Ok(())
}

fn management_agent_to_wire(agent: codex_core::ManagedSubagent) -> SubagentManagementAgent {
    SubagentManagementAgent {
        id: agent.thread_id.to_string(),
        parent_thread_id: agent.parent_thread_id.to_string(),
        runtime_id: agent.runtime_id.map(|id| id.to_string()),
        nickname: bounded_metadata(agent.nickname, 256),
        role: bounded_metadata(agent.role, 256),
        model: bounded_metadata(agent.model, 256),
        effort: agent
            .reasoning_effort
            .map(|effort| effort.to_string())
            .and_then(|effort| bounded_metadata(Some(effort), 64)),
        status: match agent.status {
            ManagedSubagentStatus::Pending => SubagentManagementStatus::Pending,
            ManagedSubagentStatus::Running => SubagentManagementStatus::Running,
            ManagedSubagentStatus::Idle => SubagentManagementStatus::Idle,
            ManagedSubagentStatus::Completed => SubagentManagementStatus::Completed,
            ManagedSubagentStatus::Errored => SubagentManagementStatus::Errored,
            ManagedSubagentStatus::Closed => SubagentManagementStatus::Closed,
            ManagedSubagentStatus::Unknown => SubagentManagementStatus::Unknown,
        },
        active_turn_id: bounded_metadata(agent.active_turn_id, MAX_TURN_ID_BYTES),
        capabilities: SubagentManagementCapabilities {
            send: agent.capabilities.send,
            interrupt: agent.capabilities.interrupt,
            close: agent.capabilities.close,
            resume: agent.capabilities.resume,
        },
    }
}

impl ThreadRequestProcessor {
    pub(crate) async fn thread_subagents_list(
        &self,
        params: ThreadSubagentsListParams,
    ) -> Result<Option<ClientResponsePayload>, JSONRPCErrorError> {
        let parent = parse_thread_id(&params.parent_thread_id)?;
        let limit = params.limit.unwrap_or(25);
        if !(1..=100).contains(&limit) {
            return Err(invalid_request("limit must be between 1 and 100"));
        }
        let reconcile_id = params
            .reconcile_request_id
            .as_deref()
            .map(|id| parse_uuid(id, "reconcileRequestId"))
            .transpose()?;
        let cursor = params
            .cursor
            .as_deref()
            .map(|cursor| {
                if cursor.len() > MAX_CURSOR_BYTES {
                    return Err(invalid_request("cursor is too long"));
                }
                let (runtime, after) = cursor
                    .split_once(':')
                    .ok_or_else(|| invalid_request("invalid cursor"))?;
                Ok((
                    parse_uuid(runtime, "cursor runtime")?,
                    parse_thread_id(after)?,
                ))
            })
            .transpose()?;
        let snapshot = self
            .thread_manager
            .list_managed_subagents(parent)
            .await
            .map_err(|_| invalid_request("subagent management is unavailable for this parent"))?;
        if snapshot
            .agents
            .iter()
            .any(|agent| agent.parent_thread_id != parent || agent.thread_id == parent)
        {
            return Err(invalid_request("invalid direct-child management snapshot"));
        }
        if cursor.is_some_and(|(runtime, _)| runtime != snapshot.parent_runtime_id) {
            return Err(invalid_request(
                "parent runtime changed; restart pagination",
            ));
        }
        let mut agents = snapshot.agents;
        agents.sort_by_key(|agent| agent.thread_id.to_string());
        let mut agents = agents.into_iter().filter(|agent| {
            cursor.is_none_or(|(_, after)| agent.thread_id.to_string() > after.to_string())
        });
        let mut page = Vec::new();
        for _ in 0..limit {
            let Some(agent) = agents.next() else { break };
            page.push(management_agent_to_wire(agent));
        }
        let next_cursor = if agents.next().is_some() {
            page.last()
                .map(|agent| format!("{}:{}", snapshot.parent_runtime_id, agent.id))
        } else {
            None
        };
        let (mutation_state, reconciled_request_id) = self
            .subagent_mutations
            .lock()
            .await
            .reconcile(parent, snapshot.parent_runtime_id, reconcile_id);
        Ok(Some(
            ThreadSubagentsListResponse {
                parent_thread_id: parent.to_string(),
                parent_runtime_id: snapshot.parent_runtime_id.to_string(),
                agents: page,
                next_cursor,
                mutation_state,
                reconciled_request_id,
            }
            .into(),
        ))
    }

    pub(crate) async fn thread_subagents_control(
        &self,
        params: ThreadSubagentsControlParams,
    ) -> Result<Option<ClientResponsePayload>, JSONRPCErrorError> {
        validate_command(&params)?;
        let parent = parse_thread_id(&params.parent_thread_id)?;
        let child = parse_thread_id(&params.agent_id)?;
        let parent_runtime = parse_uuid(
            &params.expected_parent_runtime_id,
            "expectedParentRuntimeId",
        )?;
        let child_runtime = params
            .expected_runtime_id
            .as_deref()
            .map(|id| parse_uuid(id, "expectedRuntimeId"))
            .transpose()?;
        let request_id = parse_uuid(&params.request_id, "requestId")?;
        let fingerprint: [u8; 32] = Sha256::digest(
            serde_json::to_vec(&params).map_err(|_| invalid_request("invalid command"))?,
        )
        .into();
        if let Some(status) = self.subagent_mutations.lock().await.reserve(
            request_id,
            parent,
            parent_runtime,
            fingerprint,
        )? {
            return Ok(Some(ThreadSubagentsControlResponse { status }.into()));
        }
        let action = match params.action {
            SubagentManagementAction::Send => ManagedSubagentAction::Send {
                input: vec![UserInput::Text {
                    text: params.text.unwrap_or_default(),
                    text_elements: Vec::new(),
                }],
            },
            SubagentManagementAction::Interrupt => ManagedSubagentAction::Interrupt {
                expected_turn_id: params.expected_turn_id.unwrap_or_default(),
            },
            SubagentManagementAction::Close => ManagedSubagentAction::Close,
            SubagentManagementAction::Resume => ManagedSubagentAction::Resume,
        };
        // The core owns authority, runtime fences and the operation's acceptance point.
        // An error after entry is conservatively unknown; it is never retried here.
        let status = match self
            .thread_manager
            .control_managed_subagent(parent, parent_runtime, child, child_runtime, action)
            .await
        {
            Ok(ManagedSubagentControlOutcome::Applied) => SubagentControlStatus::Applied,
            Ok(ManagedSubagentControlOutcome::NotApplied) => SubagentControlStatus::NotApplied,
            Ok(ManagedSubagentControlOutcome::Unsupported) => SubagentControlStatus::Unsupported,
            Ok(ManagedSubagentControlOutcome::Unknown) => SubagentControlStatus::Unknown,
            Err(error) => {
                let error_kind = match error.details() {
                    codex_protocol::error::CodexErrorDetails::InvalidRequest(_) => "invalidRequest",
                    codex_protocol::error::CodexErrorDetails::ThreadNotFound(_) => "threadNotFound",
                    codex_protocol::error::CodexErrorDetails::UnsupportedOperation(_) => {
                        "unsupported"
                    }
                    _ => "runtimeFailure",
                };
                tracing::warn!(%parent, %request_id, error_kind, "subagent control outcome is unknown");
                SubagentControlStatus::Unknown
            }
        };
        self.subagent_mutations
            .lock()
            .await
            .finish(request_id, status);
        Ok(Some(ThreadSubagentsControlResponse { status }.into()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn receipt_replays_cannot_repeat_effects_or_change_intent() {
        let mut book = SubagentMutationReceipts::default();
        let parent = ThreadId::new();
        let runtime = Uuid::now_v7();
        let id = Uuid::now_v7();
        assert_eq!(book.reserve(id, parent, runtime, [1; 32]).unwrap(), None);
        assert_eq!(
            book.reserve(id, parent, runtime, [1; 32]).unwrap(),
            Some(SubagentControlStatus::Unknown)
        );
        assert!(book.reserve(id, parent, runtime, [2; 32]).is_err());
        assert_eq!(
            book.reserve(Uuid::now_v7(), parent, runtime, [3; 32])
                .unwrap(),
            Some(SubagentControlStatus::NotApplied)
        );
        book.finish(id, SubagentControlStatus::Applied);
        assert_eq!(
            book.reserve(id, parent, runtime, [1; 32]).unwrap(),
            Some(SubagentControlStatus::Applied)
        );
        assert_eq!(
            book.reconcile(parent, runtime, Some(id)),
            (SubagentMutationState::Clear, Some(id.to_string()))
        );
        assert_eq!(
            book.reconcile(parent, Uuid::now_v7(), Some(id)),
            (SubagentMutationState::Unknown, None)
        );
        assert_eq!(
            book.reconcile(parent, runtime, Some(Uuid::now_v7())),
            (SubagentMutationState::Unknown, None)
        );
    }

    #[test]
    fn receipt_ceiling_never_evicts_accepted_ids() {
        let mut book = SubagentMutationReceipts::default();
        let parent = ThreadId::new();
        let runtime = Uuid::now_v7();
        let first = Uuid::now_v7();
        for index in 0..MAX_RECEIPTS {
            let id = if index == 0 { first } else { Uuid::now_v7() };
            assert_eq!(book.reserve(id, parent, runtime, [1; 32]).unwrap(), None);
            book.finish(id, SubagentControlStatus::NotApplied);
        }
        assert_eq!(
            book.reserve(Uuid::now_v7(), parent, runtime, [1; 32])
                .unwrap(),
            Some(SubagentControlStatus::Unsupported)
        );
        assert_eq!(
            book.reserve(first, parent, runtime, [1; 32]).unwrap(),
            Some(SubagentControlStatus::NotApplied)
        );
        assert_eq!(book.receipts.len(), MAX_RECEIPTS);
    }

    #[test]
    fn text_limit_counts_utf8_bytes_and_turn_ids_are_opaque() {
        let mut request = ThreadSubagentsControlParams {
            parent_thread_id: "parent".into(),
            expected_parent_runtime_id: "runtime".into(),
            agent_id: "child".into(),
            expected_runtime_id: Some("runtime".into()),
            action: SubagentManagementAction::Send,
            request_id: "request".into(),
            text: Some("가".repeat(MAX_TEXT_BYTES / 3 + 1)),
            expected_turn_id: None,
        };
        assert!(validate_command(&request).is_err());
        request.action = SubagentManagementAction::Interrupt;
        request.text = None;
        request.expected_turn_id = Some("turn-1".into());
        assert!(validate_command(&request).is_ok());
        request.expected_turn_id = Some("turn-1\0".into());
        assert!(validate_command(&request).is_err());
        request.action = SubagentManagementAction::Send;
        request.expected_turn_id = None;
        request.text = Some("instruction\0".into());
        assert!(validate_command(&request).is_err());
    }

    #[tokio::test]
    async fn concurrent_same_request_has_one_execution_owner() {
        let book =
            std::sync::Arc::new(tokio::sync::Mutex::new(SubagentMutationReceipts::default()));
        let parent = ThreadId::new();
        let runtime = Uuid::now_v7();
        let id = Uuid::now_v7();
        let mut tasks = Vec::new();
        for _ in 0..8 {
            let book = std::sync::Arc::clone(&book);
            tasks.push(tokio::spawn(async move {
                book.lock()
                    .await
                    .reserve(id, parent, runtime, [1; 32])
                    .unwrap()
                    .is_none()
            }));
        }
        let mut owners = 0;
        for task in tasks {
            owners += usize::from(task.await.unwrap());
        }
        assert_eq!(owners, 1);
        assert_eq!(
            book.lock().await.reconcile(parent, runtime, Some(id)),
            (SubagentMutationState::Unknown, None)
        );
    }
    #[tokio::test]
    async fn distinct_connection_requests_reconcile_an_unadmitted_rejection() {
        let book =
            std::sync::Arc::new(tokio::sync::Mutex::new(SubagentMutationReceipts::default()));
        let parent = ThreadId::new();
        let runtime = Uuid::now_v7();
        let first = Uuid::now_v7();
        let second = Uuid::now_v7();
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let (finish_tx, finish_rx) = tokio::sync::oneshot::channel();
        let first_connection = std::sync::Arc::clone(&book);
        let running = tokio::spawn(async move {
            assert_eq!(
                first_connection
                    .lock()
                    .await
                    .reserve(first, parent, runtime, [1; 32])
                    .unwrap(),
                None
            );
            started_tx.send(()).unwrap();
            finish_rx.await.unwrap();
            first_connection
                .lock()
                .await
                .finish(first, SubagentControlStatus::Applied);
        });
        started_rx.await.unwrap();
        let second_connection = std::sync::Arc::clone(&book);
        let rejected = tokio::spawn(async move {
            second_connection
                .lock()
                .await
                .reserve(second, parent, runtime, [2; 32])
                .unwrap()
        })
        .await
        .unwrap();
        assert_eq!(rejected, Some(SubagentControlStatus::NotApplied));
        {
            let mut receipts = book.lock().await;
            assert_eq!(receipts.receipts.len(), 2);
            assert_eq!(
                receipts.reserve(first, parent, runtime, [1; 32]).unwrap(),
                Some(SubagentControlStatus::Unknown)
            );
            assert_eq!(
                receipts.reserve(second, parent, runtime, [2; 32]).unwrap(),
                Some(SubagentControlStatus::NotApplied)
            );
            assert!(receipts.reserve(second, parent, runtime, [3; 32]).is_err());
            assert_eq!(
                receipts.reconcile(parent, runtime, Some(second)),
                (SubagentMutationState::Unknown, None)
            );
        }
        finish_tx.send(()).unwrap();
        running.await.unwrap();
        let mut receipts = book.lock().await;
        assert_eq!(
            receipts.reconcile(parent, runtime, Some(second)),
            (SubagentMutationState::Clear, Some(second.to_string()))
        );
        assert_eq!(
            receipts.reserve(second, parent, runtime, [2; 32]).unwrap(),
            Some(SubagentControlStatus::NotApplied)
        );
        let third = Uuid::now_v7();
        assert_eq!(
            receipts.reserve(third, parent, runtime, [3; 32]).unwrap(),
            None
        );
        receipts.finish(third, SubagentControlStatus::Applied);
        assert_eq!(
            receipts.reconcile(parent, runtime, Some(third)),
            (SubagentMutationState::Clear, Some(third.to_string()))
        );
    }

    #[test]
    fn terminal_rejections_share_the_bounded_receipt_ceiling() {
        let mut book = SubagentMutationReceipts::default();
        let parent = ThreadId::new();
        let runtime = Uuid::now_v7();
        let first = Uuid::now_v7();
        assert_eq!(book.reserve(first, parent, runtime, [1; 32]).unwrap(), None);
        for _ in 1..MAX_RECEIPTS {
            assert_eq!(
                book.reserve(Uuid::now_v7(), parent, runtime, [2; 32])
                    .unwrap(),
                Some(SubagentControlStatus::NotApplied)
            );
        }
        assert_eq!(book.receipts.len(), MAX_RECEIPTS);
        assert_eq!(
            book.reserve(Uuid::now_v7(), parent, runtime, [2; 32])
                .unwrap(),
            Some(SubagentControlStatus::Unsupported)
        );
        assert_eq!(
            book.reserve(first, parent, runtime, [1; 32]).unwrap(),
            Some(SubagentControlStatus::Unknown)
        );
        assert_eq!(book.receipts.len(), MAX_RECEIPTS);
    }

    #[test]
    fn retained_shutdown_runtime_serializes_as_unknown_without_actions() {
        let parent = ThreadId::new();
        let runtime = Uuid::now_v7();
        let child = codex_core::ManagedSubagent {
            thread_id: ThreadId::new(),
            parent_thread_id: parent,
            runtime_id: Some(runtime),
            nickname: None,
            role: None,
            model: None,
            reasoning_effort: None,
            status: ManagedSubagentStatus::Unknown,
            active_turn_id: None,
            capabilities: codex_core::ManagedSubagentCapabilities::default(),
        };
        let response = ThreadSubagentsListResponse {
            parent_thread_id: parent.to_string(),
            parent_runtime_id: Uuid::now_v7().to_string(),
            agents: vec![management_agent_to_wire(child)],
            next_cursor: None,
            mutation_state: SubagentMutationState::Unknown,
            reconciled_request_id: None,
        };
        let wire = serde_json::to_value(response).unwrap();
        let agent = &wire["agents"][0];
        assert_eq!(agent["status"], "unknown");
        assert_eq!(agent["runtimeId"], runtime.to_string());
        assert!(agent.get("activeTurnId").is_none());
        assert_eq!(
            agent["capabilities"],
            serde_json::json!({
                "send": false, "interrupt": false, "close": false, "resume": false
            })
        );
        assert_eq!(agent.as_object().unwrap().len(), 5);
        let decoded: ThreadSubagentsListResponse = serde_json::from_value(wire).unwrap();
        assert_eq!(decoded.agents[0].status, SubagentManagementStatus::Unknown);
        let runtime_id = runtime.to_string();
        assert_eq!(
            decoded.agents[0].runtime_id.as_deref(),
            Some(runtime_id.as_str())
        );
    }
}
