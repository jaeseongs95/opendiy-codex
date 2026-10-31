use anyhow::Result;
use app_test_support::MockResponsesConfig;
use app_test_support::TestAppServer;
use app_test_support::create_mock_responses_server_repeating_assistant;
use codex_app_server_protocol::ClientRequest;
use codex_app_server_protocol::SubagentControlStatus;
use codex_app_server_protocol::SubagentManagementAction;
use codex_app_server_protocol::SubagentMutationState;
use codex_app_server_protocol::ThreadLoadedListParams;
use codex_app_server_protocol::ThreadLoadedListResponse;
use codex_app_server_protocol::ThreadStartParams;
use codex_app_server_protocol::ThreadStartResponse;
use codex_app_server_protocol::ThreadSubagentsControlParams;
use codex_app_server_protocol::ThreadSubagentsControlResponse;
use codex_app_server_protocol::ThreadSubagentsListParams;
use codex_app_server_protocol::ThreadSubagentsListResponse;
use tempfile::TempDir;
use uuid::Uuid;

#[tokio::test]
async fn subagent_management_rejects_unrelated_runtime_and_reconciles_only_its_parent() -> Result<()>
{
    let server = create_mock_responses_server_repeating_assistant("Done").await;
    let home = TempDir::new()?;
    MockResponsesConfig::new(&server.uri())
        .with_extra_config("[features.multi_agent_v2]\nenabled = true")
        .write(home.path())?;
    let mut app = TestAppServer::builder()
        .with_codex_home(home.path())
        .build_initialized()
        .await?;
    let parent: ThreadStartResponse = app
        .request(|request_id| ClientRequest::ThreadStart {
            request_id,
            params: ThreadStartParams::default(),
        })
        .await?;
    let unrelated: ThreadStartResponse = app
        .request(|request_id| ClientRequest::ThreadStart {
            request_id,
            params: ThreadStartParams::default(),
        })
        .await?;
    let parent_id = parent.thread.id;
    let unrelated_id = unrelated.thread.id;
    let snapshot: ThreadSubagentsListResponse = app
        .request(|request_id| ClientRequest::ThreadSubagentsList {
            request_id,
            params: ThreadSubagentsListParams {
                parent_thread_id: parent_id.clone(),
                cursor: None,
                limit: Some(100),
                reconcile_request_id: None,
            },
        })
        .await?;
    assert_eq!(snapshot.parent_thread_id, parent_id);
    assert!(Uuid::parse_str(&snapshot.parent_runtime_id).is_ok());
    assert!(snapshot.agents.is_empty());
    assert_eq!(snapshot.mutation_state, SubagentMutationState::Clear);
    let unrelated_snapshot: ThreadSubagentsListResponse = app
        .request(|request_id| ClientRequest::ThreadSubagentsList {
            request_id,
            params: ThreadSubagentsListParams {
                parent_thread_id: unrelated_id.clone(),
                cursor: None,
                limit: None,
                reconcile_request_id: None,
            },
        })
        .await?;
    let mutation_id = Uuid::now_v7().to_string();
    let command = ThreadSubagentsControlParams {
        parent_thread_id: parent_id.clone(),
        expected_parent_runtime_id: snapshot.parent_runtime_id,
        agent_id: unrelated_id.clone(),
        expected_runtime_id: Some(unrelated_snapshot.parent_runtime_id),
        action: SubagentManagementAction::Close,
        request_id: mutation_id.clone(),
        text: None,
        expected_turn_id: None,
    };
    for _ in 0..2 {
        let response: ThreadSubagentsControlResponse = app
            .request(|request_id| ClientRequest::ThreadSubagentsControl {
                request_id,
                params: command.clone(),
            })
            .await?;
        assert_eq!(response.status, SubagentControlStatus::NotApplied);
    }
    for (thread_id, expected_state, expected_receipt) in [
        (
            parent_id,
            SubagentMutationState::Clear,
            Some(mutation_id.clone()),
        ),
        (unrelated_id.clone(), SubagentMutationState::Unknown, None),
    ] {
        let response: ThreadSubagentsListResponse = app
            .request(|request_id| ClientRequest::ThreadSubagentsList {
                request_id,
                params: ThreadSubagentsListParams {
                    parent_thread_id: thread_id,
                    cursor: None,
                    limit: None,
                    reconcile_request_id: Some(mutation_id.clone()),
                },
            })
            .await?;
        assert_eq!(response.mutation_state, expected_state);
        assert_eq!(response.reconciled_request_id, expected_receipt);
    }
    let loaded: ThreadLoadedListResponse = app
        .request(|request_id| ClientRequest::ThreadLoadedList {
            request_id,
            params: ThreadLoadedListParams::default(),
        })
        .await?;
    assert!(loaded.data.contains(&unrelated_id));
    app.shutdown_gracefully().await?;
    Ok(())
}
