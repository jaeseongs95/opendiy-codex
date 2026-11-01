// Modified for OpenDIY Codex R32 (2026-11-01); see MODIFICATIONS.md.
//! New method contracts through the real initialized JSON-RPC dispatcher and listener.
use super::message_processor_tracing_tests::{
    build_test_processor_with_state_db, read_response_from,
};
use super::*;
use crate::outgoing_message::{OutgoingEnvelope, OutgoingMessage};
use crate::transport::{AppServerTransport, ConnectionOrigin};
use anyhow::Result;
use app_test_support::{MockResponsesConfig, create_mock_responses_server_sequence_unchecked};
use codex_app_server_protocol::{
    InitializeResponse, RequestId, ServerNotification, ServerNotificationEnvelope,
    ThreadResumeResponse, ThreadStartResponse, TurnStartIfIdleResponse,
};
use codex_core::config::ConfigBuilder;
use core_test_support::responses;
use core_test_support::streaming_sse::{StreamingSseChunk, start_streaming_sse_server};
use serde_json::json;
use std::sync::atomic::{AtomicUsize, Ordering};
use tempfile::TempDir;
use tokio::sync::{mpsc, oneshot};
use tokio::time::{Duration, timeout};

type Receiver = mpsc::Receiver<OutgoingEnvelope>;
const DEADLINE: Duration = Duration::from_secs(10);

async fn setup(uri: &str) -> Result<(TempDir, Arc<Config>, Arc<MessageProcessor>, Receiver)> {
    let home = TempDir::new()?;
    MockResponsesConfig::new(uri)
        .with_approval_policy("on-request")
        .write(home.path())?;
    let mut config = ConfigBuilder::default()
        .codex_home(home.path().to_path_buf())
        .build()
        .await?;
    config.model_catalog = Some(codex_models_manager::bundled_models_response()?);
    config.sqlite = codex_state::SqliteConfig::new_for_testing(config.codex_home.clone());
    config.features.enable(codex_features::Feature::Sqlite)?;
    let state_db = codex_core::init_state_db(&config)
        .await
        .expect("passive fixture must initialize its isolated state database");
    let config = Arc::new(config);
    let auth = AuthManager::shared_from_config(config.as_ref(), false).await?;
    let (processor, rx) =
        build_test_processor_with_state_db(Arc::clone(&config), auth, Some(state_db)).await;
    Ok((home, config, processor, rx))
}

async fn send(
    processor: &Arc<MessageProcessor>,
    id: ConnectionId,
    session: &Arc<ConnectionSessionState>,
    request: serde_json::Value,
) {
    processor
        .process_request(
            id,
            serde_json::from_value(request).unwrap(),
            &AppServerTransport::WebSocket {
                bind_address: "127.0.0.1:0".parse().unwrap(),
            },
            Arc::clone(session),
        )
        .await;
}

async fn initialize(
    processor: &Arc<MessageProcessor>,
    rx: &mut Receiver,
    id: ConnectionId,
) -> Arc<ConnectionSessionState> {
    let session = Arc::new(ConnectionSessionState::new(ConnectionOrigin::WebSocket));
    send(
        processor,
        id,
        &session,
        json!({"id":1,"method":"initialize","params":{
            "clientInfo":{"name":format!("passive-test-{}",id.0),"version":"1"},
            "capabilities":{"experimentalApi":true}
        }}),
    )
    .await;
    let _: InitializeResponse = read_response_from(rx, id, 1).await;
    assert!(session.initialized());
    // 실제 WebSocket 전송 루프도 initialize 응답 뒤 이 등록을 완료한다.
    processor
        .connection_initialized(id, session.request_attestation())
        .await;
    session
}

async fn start(
    processor: &Arc<MessageProcessor>,
    rx: &mut Receiver,
    id: ConnectionId,
    session: &Arc<ConnectionSessionState>,
) -> ThreadStartResponse {
    send(
        processor,
        id,
        session,
        json!({"id":2,"method":"thread/start","params":{"model":"mock-model"}}),
    )
    .await;
    read_response_from(rx, id, 2).await
}

async fn error(rx: &mut Receiver, id: ConnectionId, request_id: i64) -> String {
    error_details(rx, id, request_id).await.message
}

async fn error_details(
    rx: &mut Receiver,
    id: ConnectionId,
    request_id: i64,
) -> codex_app_server_protocol::JSONRPCErrorError {
    loop {
        if let Some(OutgoingEnvelope::ToConnection {
            connection_id,
            message: OutgoingMessage::Error(error),
            ..
        }) = timeout(DEADLINE, rx.recv()).await.unwrap()
        {
            if connection_id == id && error.id == RequestId::Integer(request_id) {
                return error.error;
            }
        }
    }
}

fn assert_passive_unavailable(error: codex_app_server_protocol::JSONRPCErrorError, reason: &str) {
    assert_eq!(error.code, crate::error_code::INVALID_REQUEST_ERROR_CODE);
    assert_eq!(
        error.data,
        Some(json!({
            "kind": "threadSubscriptionUnavailable", "reason": reason,
        }))
    );
}

async fn finish(processor: &Arc<MessageProcessor>, rx: Receiver) {
    drop(rx);
    processor.clear_runtime_references();
    processor.shutdown_threads().await;
}

#[tokio::test]
async fn strict_user_idle_two_connections_admit_one_and_preserve_loser_settings() -> Result<()> {
    let (release, gate) = oneshot::channel();
    let (server, _) = start_streaming_sse_server(vec![vec![
        StreamingSseChunk {
            gate: None,
            body: responses::sse(vec![responses::ev_response_created("strict")]),
        },
        StreamingSseChunk {
            gate: Some(gate),
            body: responses::sse(vec![responses::ev_completed("strict")]),
        },
    ]])
    .await;
    let (_home, _, processor, mut rx) = setup(&server.uri()).await?;
    let a = ConnectionId(1);
    let b = ConnectionId(2);
    let sa = initialize(&processor, &mut rx, a).await;
    let sb = initialize(&processor, &mut rx, b).await;
    let started = start(&processor, &mut rx, a, &sa).await;
    let id = started.thread.id.clone();
    let input = |text: &str| json!([{"type":"text","text":text,"text_elements":[]}]);
    tokio::join!(
        send(
            &processor,
            a,
            &sa,
            json!({"id":10,"method":"turn/startUserIfIdle","params":{
                "threadId":id,"input":input("winner-a"),"model":"mock-model-a","effort":"high","clientUserMessageId":"user-a"
            }})
        ),
        send(
            &processor,
            b,
            &sb,
            json!({"id":11,"method":"turn/startUserIfIdle","params":{
                "threadId":id,"input":input("winner-b"),"model":"mock-model-b","effort":"low","clientUserMessageId":"user-b"
            }})
        ),
    );
    let mut outcomes = Vec::new();
    while outcomes.len() < 2 {
        if let Some(OutgoingEnvelope::ToConnection {
            connection_id,
            message: OutgoingMessage::Response(response),
            ..
        }) = timeout(DEADLINE, rx.recv()).await?
        {
            if matches!(response.id, RequestId::Integer(10 | 11)) {
                outcomes.push((
                    connection_id,
                    serde_json::from_value::<TurnStartIfIdleResponse>(serde_json::to_value(
                        response.result,
                    )?)?,
                ));
            }
        }
    }
    assert_eq!(
        outcomes
            .iter()
            .filter(|(_, r)| matches!(r, TurnStartIfIdleResponse::Started { .. }))
            .count(),
        1
    );
    assert_eq!(
        outcomes
            .iter()
            .filter(|(_, r)| matches!(r, TurnStartIfIdleResponse::SkippedBusy))
            .count(),
        1
    );
    let winner = outcomes
        .iter()
        .find(|(_, r)| matches!(r, TurnStartIfIdleResponse::Started { .. }))
        .unwrap()
        .0;
    timeout(DEADLINE, server.wait_for_request_count(1)).await?;
    let runtime = processor
        .thread_processor
        .thread_manager
        .get_thread(ThreadId::from_string(&id)?)
        .await?;
    let snapshot = runtime.config_snapshot().await;
    assert_eq!(
        snapshot.model,
        if winner == a {
            "mock-model-a"
        } else {
            "mock-model-b"
        }
    );
    assert_eq!(
        snapshot.reasoning_effort,
        Some(if winner == a {
            codex_protocol::openai_models::ReasoningEffort::High
        } else {
            codex_protocol::openai_models::ReasoningEffort::Low
        })
    );
    assert_eq!(
        snapshot.approval_policy,
        codex_protocol::protocol::AskForApproval::OnRequest
    );
    // A declined full-parameter probe must not alter the accepted turn or steer input.
    send(&processor,b,&sb,json!({"id":12,"method":"turn/startUserIfIdle","params":{
        "threadId":id,"input":input("BUSY_MUST_NOT_EXIST"),"model":"loser","approvalPolicy":"never"
    }})).await;
    let busy: TurnStartIfIdleResponse = read_response_from(&mut rx, b, 12).await;
    assert_eq!(busy, TurnStartIfIdleResponse::SkippedBusy);
    assert_eq!(runtime.config_snapshot().await.model, snapshot.model);
    release.send(()).unwrap();
    loop {
        if let Some(OutgoingEnvelope::ToConnection {
            message: OutgoingMessage::AppServerNotification(notification),
            ..
        }) = timeout(DEADLINE, rx.recv()).await?
        {
            if matches!(
                notification.notification,
                ServerNotification::TurnCompleted(_)
            ) {
                break;
            }
        }
    }
    send(
        &processor,
        a,
        &sa,
        json!({"id":13,"method":"thread/read","params":{"threadId":id,"includeTurns":true}}),
    )
    .await;
    let read: serde_json::Value = read_response_from(&mut rx, a, 13).await;
    let history = read.to_string();
    assert!(!history.contains("BUSY_MUST_NOT_EXIST"));
    assert!(history.contains(if winner == a { "winner-a" } else { "winner-b" }));
    assert!(!history.contains(if winner == a { "winner-b" } else { "winner-a" }));
    assert_eq!(server.requests().await.len(), 1);
    finish(&processor, rx).await;
    server.shutdown().await;
    Ok(())
}

#[tokio::test]
async fn passive_duplicate_unsubscribe_disconnect_and_missing_runtime_do_not_resume() -> Result<()>
{
    let server = create_mock_responses_server_sequence_unchecked(Vec::new()).await;
    let (_home, _, processor, mut rx) = setup(&server.uri()).await?;
    let owner = ConnectionId(1);
    let observer = ConnectionId(2);
    let sa = initialize(&processor, &mut rx, owner).await;
    let sb = initialize(&processor, &mut rx, observer).await;
    let thread = start(&processor, &mut rx, owner, &sa).await.thread;
    let id = ThreadId::from_string(&thread.id)?;
    let runtime = processor
        .thread_processor
        .thread_manager
        .get_thread(id)
        .await?;
    for request_id in [20, 21] {
        send(&processor,observer,&sb,json!({"id":request_id,"method":"thread/subscribePassive","params":{"threadId":thread.id}})).await;
        let response: ThreadResumeResponse =
            read_response_from(&mut rx, observer, request_id).await;
        assert!(response.thread.turns.is_empty());
        assert!(response.initial_turns_page.is_none());
        assert!(response.turns_backwards_cursor.is_none());
        assert!(response.items_backwards_cursor.is_none());
        assert_eq!(response.model, runtime.config_snapshot().await.model);
    }
    let states = processor.thread_processor.subscription_state_for_test();
    assert_eq!(states.subscribed_connection_ids(id).await.len(), 2);
    send(
        &processor,
        observer,
        &sb,
        json!({"id":22,"method":"thread/unsubscribe","params":{"threadId":thread.id}}),
    )
    .await;
    let _: serde_json::Value = read_response_from(&mut rx, observer, 22).await;
    assert_eq!(states.subscribed_connection_ids(id).await, vec![owner]);
    send(
        &processor,
        observer,
        &sb,
        json!({"id":23,"method":"thread/subscribePassive","params":{"threadId":thread.id}}),
    )
    .await;
    let _: ThreadResumeResponse = read_response_from(&mut rx, observer, 23).await;
    processor.connection_closed(observer, &sb).await;
    assert_eq!(states.subscribed_connection_ids(id).await, vec![owner]);
    let missing = ThreadId::new();
    send(&processor,owner,&sa,json!({"id":24,"method":"thread/subscribePassive","params":{"threadId":missing.to_string()}})).await;
    assert_passive_unavailable(error_details(&mut rx, owner, 24).await, "notLoaded");
    assert!(
        processor
            .thread_processor
            .thread_manager
            .get_thread(missing)
            .await
            .is_err()
    );
    assert!(Arc::ptr_eq(
        &runtime,
        &processor
            .thread_processor
            .thread_manager
            .get_thread(id)
            .await?
    ));
    finish(&processor, rx).await;
    Ok(())
}

#[tokio::test]
async fn new_methods_reject_uninitialized_and_invalid_user_payloads() -> Result<()> {
    let server = create_mock_responses_server_sequence_unchecked(Vec::new()).await;
    let (_home, _, processor, mut rx) = setup(&server.uri()).await?;
    let id = ConnectionId(1);
    let session = Arc::new(ConnectionSessionState::new(ConnectionOrigin::WebSocket));
    for (n, method, params) in [
        (
            30,
            "thread/subscribePassive",
            json!({"threadId":ThreadId::new().to_string()}),
        ),
        (
            31,
            "turn/startUserIfIdle",
            json!({"threadId":ThreadId::new().to_string(),"input":[]}),
        ),
    ] {
        send(
            &processor,
            id,
            &session,
            json!({"id":n,"method":method,"params":params}),
        )
        .await;
        assert!(error(&mut rx, id, n).await.contains("Not initialized"));
    }
    let session = initialize(&processor, &mut rx, id).await;
    let thread = start(&processor, &mut rx, id, &session).await.thread;
    for (n, extra) in [
        (32, json!({"input":[]})),
        (
            33,
            json!({"input":[],"toolOutput":{"name":"x","output":"x"}}),
        ),
        (
            34,
            json!({"input":[{"type":"text","text":"x","text_elements":[]}],"parentTurnId":"forged"}),
        ),
    ] {
        let mut params = json!({"threadId":thread.id});
        params
            .as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        send(
            &processor,
            id,
            &session,
            json!({"id":n,"method":"turn/startUserIfIdle","params":params}),
        )
        .await;
        assert!(error(&mut rx, id, n).await.contains("turn/startUserIfIdle"));
    }
    assert_eq!(
        processor
            .thread_processor
            .thread_manager
            .list_thread_ids()
            .await
            .len(),
        1
    );
    finish(&processor, rx).await;
    Ok(())
}

/// A real extension observes lifecycle emission, rather than a mocked passive verdict.
#[tokio::test]
async fn passive_listener_replays_general_pending_but_emits_no_idle_lifecycle() -> Result<()> {
    use codex_app_server_protocol::{
        DynamicToolCallParams, McpServerElicitationRequest, McpServerElicitationRequestParams,
        ServerRequestPayload, ThreadSubscribePassiveParams,
    };
    struct IdleRecorder(Arc<AtomicUsize>);
    impl codex_extension_api::ThreadLifecycleContributor<Config> for IdleRecorder {
        fn on_thread_idle<'a>(
            &'a self,
            _: codex_extension_api::ThreadIdleInput<'a>,
        ) -> codex_extension_api::ExtensionFuture<'a, ()> {
            Box::pin(async move {
                self.0.fetch_add(1, Ordering::SeqCst);
            })
        }
    }
    let server = create_mock_responses_server_sequence_unchecked(Vec::new()).await;
    let (_home, config, processor, mut rx) = setup(&server.uri()).await?;
    let owner = ConnectionId(1);
    let observer = ConnectionId(2);
    let _sa = initialize(&processor, &mut rx, owner).await;
    let _sb = initialize(&processor, &mut rx, observer).await;
    let idle = Arc::new(AtomicUsize::new(0));
    let mut extensions = codex_extension_api::ExtensionRegistryBuilder::<Config>::new();
    extensions.thread_lifecycle_contributor(Arc::new(IdleRecorder(Arc::clone(&idle))));
    let auth = AuthManager::shared_from_config(config.as_ref(), false).await?;
    // Only the manager's extension registry differs; the production listener,
    // membership, outgoing callbacks and passive handler remain real.
    let manager = Arc::new(codex_core::ThreadManager::new(
        &config,
        Arc::clone(&auth),
        codex_core::build_models_manager(&config, Arc::clone(&auth)),
        codex_core::CodexAppsToolsCache::default(),
        codex_protocol::protocol::SessionSource::VSCode,
        Arc::new(codex_exec_server::EnvironmentManager::default_for_tests()),
        Arc::new(extensions.build()),
        Arc::new(codex_home::CodexHomeUserInstructionsProvider::new(
            config.codex_home.clone(),
        )),
        None,
        codex_core::passthrough_image_store(),
        codex_core::thread_store_from_config(&config, None),
        None,
        "11111111-1111-4111-8111-111111111111".into(),
        None,
        None,
    ));
    let mut host = processor.thread_processor.clone();
    host.thread_manager = Arc::clone(&manager);
    let created = manager
        .start_thread(codex_core::StartThreadOptions::new((*config).clone()))
        .await?;
    host.try_attach_thread_listener(created.thread_id, Arc::clone(&created.thread), vec![owner])
        .await;
    processor
        .outgoing
        .enable_user_verification_connection(owner)
        .await;
    let (ordinary, _ordinary_waiter) = processor
        .outgoing
        .send_request_to_connections(
            Some(&[owner]),
            ServerRequestPayload::DynamicToolCall(DynamicToolCallParams {
                thread_id: created.thread_id.to_string(),
                turn_id: "turn-1".into(),
                call_id: "call-1".into(),
                namespace: None,
                tool: "ordinary".into(),
                arguments: json!({}),
            }),
            Some(created.thread_id),
        )
        .await;
    let (_verification, mut verification_waiter) = processor
        .outgoing
        .send_request_to_connections(
            Some(&[owner]),
            ServerRequestPayload::McpServerElicitationRequest(McpServerElicitationRequestParams {
                thread_id: created.thread_id.to_string(),
                turn_id: Some("turn-1".into()),
                server_name: "service".into(),
                request: McpServerElicitationRequest::UserVerification {
                    meta: None,
                    title: "verify".into(),
                    description: String::new(),
                    challenge: "AQID".into(),
                },
            }),
            Some(created.thread_id),
        )
        .await;
    while rx.try_recv().is_ok() {}
    assert!(
        matches!(
            verification_waiter.try_recv(),
            Err(tokio::sync::oneshot::error::TryRecvError::Empty)
        ),
        "verification callback must actually be pending for the exclusion check"
    );
    host.thread_subscribe_passive(
        ConnectionRequestId {
            connection_id: observer,
            request_id: RequestId::Integer(50),
        },
        ThreadSubscribePassiveParams {
            thread_id: created.thread_id.to_string(),
        },
    )
    .await
    .expect("existing loaded runtime must accept passive subscription");
    let response: ThreadResumeResponse = read_response_from(&mut rx, observer, 50).await;
    assert!(response.thread.turns.is_empty());
    let replay = timeout(DEADLINE, rx.recv())
        .await?
        .expect("ordinary replay");
    assert!(
        matches!(replay,OutgoingEnvelope::ToConnection{connection_id,message:OutgoingMessage::Request(request),..}
        if connection_id==observer && request.id()==&ordinary)
    );
    assert!(rx.try_recv().is_err(), "verification must not be replayed");
    assert!(matches!(
        verification_waiter.try_recv(),
        Err(tokio::sync::oneshot::error::TryRecvError::Empty)
    ));
    assert_eq!(
        idle.load(Ordering::SeqCst),
        0,
        "observation must not dispatch goal/queue idle hooks"
    );
    // Positive control: the same runtime and real registry do record an explicit
    // lifecycle call, demonstrating that the zero above can detect the old tail.
    created
        .thread
        .emit_thread_idle_lifecycle_if_idle(codex_extension_api::ThreadIdleCause::Completed)
        .await;
    assert_eq!(idle.load(Ordering::SeqCst), 1);
    drop(rx);
    processor.clear_runtime_references();
    manager.shutdown_all_threads_bounded(DEADLINE).await;
    processor.shutdown_threads().await;
    Ok(())
}

#[tokio::test]
async fn passive_blocked_response_releases_registry_and_unsubscribe_orders_after_replay()
-> Result<()> {
    use codex_app_server_protocol::{DynamicToolCallParams, ServerRequestPayload};
    let server = create_mock_responses_server_sequence_unchecked(Vec::new()).await;
    let (_home, config, processor, mut rx) = setup(&server.uri()).await?;
    let owner = ConnectionId(1);
    let observer = ConnectionId(2);
    let sa = initialize(&processor, &mut rx, owner).await;
    let sb = initialize(&processor, &mut rx, observer).await;
    let thread = start(&processor, &mut rx, owner, &sa).await.thread;
    let id = ThreadId::from_string(&thread.id)?;
    let (ordinary, _waiter) = processor
        .outgoing
        .send_request_to_connections(
            Some(&[owner]),
            ServerRequestPayload::DynamicToolCall(DynamicToolCallParams {
                thread_id: thread.id.clone(),
                turn_id: "turn-1".into(),
                call_id: "call-1".into(),
                namespace: None,
                tool: "pending".into(),
                arguments: json!({}),
            }),
            Some(id),
        )
        .await;
    while rx.try_recv().is_ok() {}
    // The actual outgoing channel has capacity 16. Fill it before the listener
    // responds, so the subscription stays blocked in bounded mpsc::send.
    for n in 0..16 {
        processor
            .outgoing
            .send_error(
                ConnectionRequestId {
                    connection_id: owner,
                    request_id: RequestId::Integer(100 + n),
                },
                crate::error_code::invalid_request("fill bounded outgoing"),
            )
            .await;
    }
    send(
        &processor,
        observer,
        &sb,
        json!({"id":60,"method":"thread/subscribePassive","params":{"threadId":thread.id}}),
    )
    .await;
    let states = processor.thread_processor.subscription_state_for_test();
    timeout(DEADLINE, async {
        while !states
            .subscribed_connection_ids(id)
            .await
            .contains(&observer)
        {
            tokio::task::yield_now().await;
        }
    })
    .await?;
    // Publication and raw removal both need the real global registry writer.
    // Neither may be held hostage by the blocked response to this connection.
    let unrelated = timeout(
        DEADLINE,
        processor
            .thread_processor
            .thread_manager
            .start_thread(codex_core::StartThreadOptions::new((*config).clone())),
    )
    .await??;
    timeout(
        DEADLINE,
        processor
            .thread_processor
            .thread_manager
            .remove_thread(&unrelated.thread_id),
    )
    .await?
    .expect("unrelated runtime removed");
    unrelated.thread.shutdown_and_wait().await?;
    send(
        &processor,
        observer,
        &sb,
        json!({"id":61,"method":"thread/unsubscribe","params":{"threadId":thread.id}}),
    )
    .await;
    let _: ThreadResumeResponse = read_response_from(&mut rx, observer, 60).await;
    let mut replayed = false;
    loop {
        if let Some(OutgoingEnvelope::ToConnection {
            connection_id,
            message,
            ..
        }) = timeout(DEADLINE, rx.recv()).await?
        {
            if connection_id != observer {
                continue;
            }
            match message {
                OutgoingMessage::Request(request) if request.id() == &ordinary => replayed = true,
                OutgoingMessage::Response(response) if response.id == RequestId::Integer(61) => {
                    break;
                }
                _ => {}
            }
        }
    }
    assert!(
        replayed,
        "ordinary pending replay must precede unsubscribe acknowledgement"
    );
    assert_eq!(states.subscribed_connection_ids(id).await, vec![owner]);
    assert!(
        rx.try_recv().is_err(),
        "no replay may be queued after unsubscribe response"
    );
    finish(&processor, rx).await;
    Ok(())
}

#[tokio::test]
async fn passive_refuses_removed_runtime_or_listener_absence() -> Result<()> {
    let server = create_mock_responses_server_sequence_unchecked(Vec::new()).await;
    let (_home, config, processor, mut rx) = setup(&server.uri()).await?;
    let owner = ConnectionId(1);
    let observer = ConnectionId(2);
    let sa = initialize(&processor, &mut rx, owner).await;
    let sb = initialize(&processor, &mut rx, observer).await;
    let thread = start(&processor, &mut rx, owner, &sa).await.thread;
    let id = ThreadId::from_string(&thread.id)?;
    let states = processor.thread_processor.subscription_state_for_test();
    {
        let mut pending = processor
            .thread_processor
            .pending_thread_unloads
            .lock()
            .await;
        pending.insert(id, tokio::sync::watch::channel(()).0);
    }
    send(
        &processor,
        observer,
        &sb,
        json!({"id":69,"method":"thread/subscribePassive","params":{"threadId":thread.id}}),
    )
    .await;
    assert_passive_unavailable(error_details(&mut rx, observer, 69).await, "closing");
    assert!(
        !states
            .subscribed_connection_ids(id)
            .await
            .contains(&observer)
    );
    processor
        .thread_processor
        .pending_thread_unloads
        .lock()
        .await
        .remove(&id);
    let (snapshot_entered, release_snapshot) = states.pause_passive_snapshot_for_test(id).await;
    send(
        &processor,
        observer,
        &sb,
        json!({"id":70,"method":"thread/subscribePassive","params":{"threadId":thread.id}}),
    )
    .await;
    // The real listener has taken its snapshot but has not committed membership.
    timeout(DEADLINE, snapshot_entered).await??;
    // Remove the actual registered runtime at that exact boundary.
    let old = processor
        .thread_processor
        .thread_manager
        .remove_thread(&id)
        .await
        .expect("old runtime");
    release_snapshot.send(()).unwrap();
    assert_passive_unavailable(error_details(&mut rx, observer, 70).await, "notLoaded");
    assert!(
        !states
            .subscribed_connection_ids(id)
            .await
            .contains(&observer)
    );
    old.shutdown_and_wait().await?;
    let created = processor
        .thread_processor
        .thread_manager
        .start_thread(codex_core::StartThreadOptions::new((*config).clone()))
        .await?;
    send(&processor,observer,&sb,json!({"id":71,"method":"thread/subscribePassive","params":{"threadId":created.thread_id.to_string()}})).await;
    assert_passive_unavailable(
        error_details(&mut rx, observer, 71).await,
        "listenerNotReady",
    );
    assert!(
        states
            .existing_thread_state(created.thread_id)
            .await
            .is_none()
    );
    created.thread.shutdown_and_wait().await?;
    send(&processor,observer,&sb,json!({"id":72,"method":"thread/subscribePassive","params":{"threadId":created.thread_id.to_string()}})).await;
    assert_passive_unavailable(error_details(&mut rx, observer, 72).await, "closing");
    send(
        &processor,
        observer,
        &sb,
        json!({"id":73,"method":"thread/subscribePassive","params":{"threadId":"invalid"}}),
    )
    .await;
    let invalid = error_details(&mut rx, observer, 73).await;
    assert_eq!(
        invalid.data, None,
        "invalid identifiers are not retryable availability failures"
    );
    send(&processor,observer,&sb,json!({"id":74,"method":"thread/subscribePassiveUnsupported","params":{"threadId":thread.id}})).await;
    let unsupported = error_details(&mut rx, observer, 74).await;
    // Older servers decode an unknown method as InvalidRequest, not necessarily
    // MethodNotFound. Neither a missing kind/reason nor its message is retryable.
    assert_eq!(
        unsupported.code,
        crate::error_code::INVALID_REQUEST_ERROR_CODE
    );
    assert_eq!(unsupported.data, None);
    finish(&processor, rx).await;
    Ok(())
}

#[tokio::test]
async fn new_methods_queued_before_connection_gate_closes_have_no_effects() -> Result<()> {
    use crate::request_serialization::{RequestSerializationAccess, RequestSerializationQueueKey};
    let server = create_mock_responses_server_sequence_unchecked(Vec::new()).await;
    let (_home, _, processor, mut rx) = setup(&server.uri()).await?;
    let owner = ConnectionId(1);
    let observer = ConnectionId(2);
    let sa = initialize(&processor, &mut rx, owner).await;
    let sb = initialize(&processor, &mut rx, observer).await;
    let thread = start(&processor, &mut rx, owner, &sa).await.thread;
    let id = ThreadId::from_string(&thread.id)?;
    let key = RequestSerializationQueueKey::Thread {
        thread_id: thread.id.clone(),
    };
    let (entered, waiting) = oneshot::channel();
    let (release, blocked) = oneshot::channel();
    processor
        .request_serialization_queues
        .enqueue_background(
            key.clone(),
            RequestSerializationAccess::Exclusive,
            async move {
                let _ = entered.send(());
                let _ = blocked.await;
            },
        )
        .await;
    timeout(DEADLINE, waiting).await??;
    send(
        &processor,
        observer,
        &sb,
        json!({"id":80,"method":"thread/subscribePassive","params":{"threadId":thread.id}}),
    )
    .await;
    send(&processor,observer,&sb,json!({"id":81,"method":"turn/startUserIfIdle","params":{
        "threadId":thread.id,"input":[{"type":"text","text":"closed owner must not run","text_elements":[]}],"model":"must-not-apply"
    }})).await;
    sb.rpc_gate.close().await;
    release.send(()).unwrap();
    let (done, finished) = oneshot::channel();
    processor
        .request_serialization_queues
        .enqueue_background(key, RequestSerializationAccess::Exclusive, async move {
            let _ = done.send(());
        })
        .await;
    timeout(DEADLINE, finished).await??;
    assert_eq!(
        processor
            .thread_processor
            .subscription_state_for_test()
            .subscribed_connection_ids(id)
            .await,
        vec![owner]
    );
    assert_eq!(
        processor
            .thread_processor
            .thread_manager
            .get_thread(id)
            .await?
            .config_snapshot()
            .await
            .model,
        "mock-model"
    );
    finish(&processor, rx).await;
    Ok(())
}

#[tokio::test]
async fn strict_user_idle_lost_response_keeps_accepted_owner_and_never_resubmits() -> Result<()> {
    let (release, gate) = oneshot::channel();
    let (server, _) = start_streaming_sse_server(vec![vec![
        StreamingSseChunk {
            gate: None,
            body: responses::sse(vec![responses::ev_response_created("lost-response")]),
        },
        StreamingSseChunk {
            gate: Some(gate),
            body: responses::sse(vec![responses::ev_completed("lost-response")]),
        },
    ]])
    .await;
    let (_home, _, processor, mut rx) = setup(&server.uri()).await?;
    let owner = ConnectionId(1);
    let observer = ConnectionId(2);
    let sa = initialize(&processor, &mut rx, owner).await;
    let sb = initialize(&processor, &mut rx, observer).await;
    let thread = start(&processor, &mut rx, owner, &sa).await.thread;
    send(&processor,owner,&sa,json!({"id":90,"method":"turn/startUserIfIdle","params":{
        "threadId":thread.id,"input":[{"type":"text","text":"accepted once before disconnect","text_elements":[]}],"clientUserMessageId":"correlation-without-dedupe"
    }})).await;
    // The real model request proves admission has committed. Deliberately do
    // not read the Started response, then close that transport connection.
    timeout(DEADLINE, server.wait_for_request_count(1)).await?;
    processor.connection_closed(owner, &sa).await;
    send(
        &processor,
        observer,
        &sb,
        json!({"id":91,"method":"thread/subscribePassive","params":{"threadId":thread.id}}),
    )
    .await;
    let _: ThreadResumeResponse = read_response_from(&mut rx, observer, 91).await;
    release.send(()).unwrap();
    loop {
        if let Some(OutgoingEnvelope::ToConnection {
            connection_id,
            message: OutgoingMessage::AppServerNotification(notification),
            ..
        }) = timeout(DEADLINE, rx.recv()).await?
        {
            if connection_id == observer
                && matches!(
                    notification.notification,
                    ServerNotification::TurnCompleted(_)
                )
            {
                break;
            }
        }
    }
    assert_eq!(
        server.requests().await.len(),
        1,
        "lost acknowledgement must not cause automatic resubmission"
    );
    finish(&processor, rx).await;
    server.shutdown().await;
    Ok(())
}

#[tokio::test]
async fn passive_snapshot_gap_delivers_committed_model_and_effort_after_its_response() -> Result<()>
{
    let server = create_mock_responses_server_sequence_unchecked(Vec::new()).await;
    let (_home, _, processor, mut rx) = setup(&server.uri()).await?;
    let owner = ConnectionId(1);
    let observer = ConnectionId(2);
    let sa = initialize(&processor, &mut rx, owner).await;
    let sb = initialize(&processor, &mut rx, observer).await;
    let thread = start(&processor, &mut rx, owner, &sa).await.thread;
    let id = ThreadId::from_string(&thread.id)?;
    let runtime = processor
        .thread_processor
        .thread_manager
        .get_thread(id)
        .await?;
    let original = runtime.config_snapshot().await;
    let (entered, release) = processor
        .thread_processor
        .subscription_state_for_test()
        .pause_passive_snapshot_for_test(id)
        .await;
    send(
        &processor,
        observer,
        &sb,
        json!({"id":95,"method":"thread/subscribePassive","params":{"threadId":thread.id}}),
    )
    .await;
    // Pause the real listener after it captured metadata and before membership.
    timeout(DEADLINE, entered).await??;
    runtime
        .update_thread_settings(codex_protocol::protocol::ThreadSettingsOverrides {
            model: Some("gpt-5.4".into()),
            effort: Some(Some(codex_protocol::openai_models::ReasoningEffort::High)),
            ..Default::default()
        })
        .await?;
    assert_eq!(runtime.config_snapshot().await.model, "gpt-5.4");
    assert_eq!(
        runtime.config_snapshot().await.reasoning_effort,
        Some(codex_protocol::openai_models::ReasoningEffort::High)
    );
    release.send(()).unwrap();
    let mut response_seen = false;
    loop {
        if let Some(OutgoingEnvelope::ToConnection {
            connection_id,
            message,
            ..
        }) = timeout(DEADLINE, rx.recv()).await?
        {
            if connection_id != observer {
                continue;
            }
            match message {
                OutgoingMessage::Response(response) if response.id == RequestId::Integer(95) => {
                    let response: ThreadResumeResponse =
                        serde_json::from_value(serde_json::to_value(response.result)?)?;
                    assert_eq!(response.model, original.model);
                    assert_eq!(response.reasoning_effort, original.reasoning_effort);
                    response_seen = true;
                }
                OutgoingMessage::AppServerNotification(ServerNotificationEnvelope {
                    notification: ServerNotification::ThreadSettingsUpdated(update),
                    ..
                }) if update.thread_id == thread.id => {
                    assert!(
                        response_seen,
                        "same-thread settings must follow the passive snapshot response"
                    );
                    let updated = serde_json::to_string(&update.thread_settings)?;
                    assert!(updated.contains("gpt-5.4"));
                    assert!(updated.contains("high"));
                    break;
                }
                _ => {}
            }
        }
    }
    finish(&processor, rx).await;
    Ok(())
}
