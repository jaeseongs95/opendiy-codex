// Modified for OpenDIY Codex R32 (2026-11-01); see MODIFICATIONS.md.
use super::*;
use async_channel::bounded;
use codex_extension_api::ExtensionFuture;
use codex_extension_api::ExtensionRegistryBuilder;
use codex_extension_api::ThreadLifecycleContributor;
use codex_extension_api::ThreadStartInput;
use codex_protocol::capabilities::CapabilityRootLocation;
use codex_protocol::capabilities::SelectedCapabilityRoot;
use codex_protocol::error::CodexErrorDetails;
use codex_protocol::models::ResponseItem;
use codex_protocol::protocol::AgentStatus;
use codex_protocol::protocol::EventMsg;
use codex_protocol::protocol::McpStartupCompleteEvent;
use codex_protocol::protocol::McpStartupStatus;
use codex_protocol::protocol::McpStartupUpdateEvent;
use codex_protocol::protocol::RawResponseItemEvent;
use codex_protocol::protocol::TurnAbortReason;
use codex_protocol::protocol::TurnAbortedEvent;
use codex_protocol::protocol::TurnEnvironmentSelection;
use futures::FutureExt;
use pretty_assertions::assert_eq;
use std::sync::Arc;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use tokio::sync::watch;
use tokio::time::timeout;

struct ThreadStartRecorder(Arc<AtomicUsize>);

impl ThreadLifecycleContributor<Config> for ThreadStartRecorder {
    fn on_thread_start<'a>(
        &'a self,
        _input: ThreadStartInput<'a, Config>,
    ) -> ExtensionFuture<'a, ()> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Box::pin(std::future::ready(()))
    }
}

#[tokio::test]
async fn forward_events_filters_private_events_before_blocked_send_is_cancelled() {
    let (tx_events, rx_events) = bounded(SUBMISSION_CHANNEL_CAPACITY);
    let (tx_sub, rx_sub) = bounded(SUBMISSION_CHANNEL_CAPACITY);
    let (_agent_status_tx, agent_status) = watch::channel(AgentStatus::PendingInit);
    let io = Arc::new(SessionIo {
        admission: Arc::default(),
        tx_sub,
        rx_event: rx_events,
        agent_status,
        session_loop_termination: completed_session_loop_termination(),
    });

    let (tx_out, rx_out) = bounded(1);
    tx_out
        .send(Event {
            id: "full".to_string(),
            msg: EventMsg::TurnAborted(TurnAbortedEvent {
                root_turn_id: None,
                turn_id: Some("turn-1".to_string()),
                started_at: None,
                reason: TurnAbortReason::Interrupted,
                error: None,
                completed_at: None,
                duration_ms: None,
            }),
        })
        .await
        .unwrap();

    let cancel = CancellationToken::new();
    let forward = tokio::spawn(forward_events(
        Arc::clone(&io),
        tx_out.clone(),
        cancel.clone(),
    ));

    for msg in [
        EventMsg::McpStartupUpdate(McpStartupUpdateEvent {
            server: "pending".to_string(),
            status: McpStartupStatus::Starting,
        }),
        EventMsg::McpStartupComplete(McpStartupCompleteEvent::default()),
    ] {
        tx_events
            .send(Event {
                id: "delegate-startup".to_string(),
                msg,
            })
            .await
            .unwrap();
    }
    let visible_msg = EventMsg::RawResponseItem(RawResponseItemEvent {
        item: ResponseItem::CustomToolCall {
            encrypted_content: None,
            id: None,
            status: None,
            call_id: "call-1".to_string(),
            name: "tool".to_string(),
            namespace: None,
            input: "{}".to_string(),
            internal_chat_message_metadata_passthrough: None,
        },
    });
    for id in ["visible-1", "visible-2", "blocked"] {
        tx_events
            .send(Event {
                id: id.to_string(),
                msg: visible_msg.clone(),
            })
            .await
            .unwrap();
    }

    drop(tx_events);
    let received = rx_out.recv().await.expect("prefilled event missing");
    assert_eq!(received.id, "full");
    let received = rx_out.recv().await.expect("visible event missing");
    assert_eq!(received.id, "visible-1");
    cancel.cancel();
    timeout(std::time::Duration::from_millis(1000), forward)
        .await
        .expect("forward_events hung")
        .expect("forward_events join error");

    let mut ops = Vec::new();
    while let Ok(sub) = rx_sub.try_recv() {
        ops.push(sub.op);
    }
    assert!(
        ops.iter().any(|op| matches!(op, Op::Interrupt)),
        "expected Interrupt op after cancellation"
    );
    assert!(
        ops.iter().any(|op| matches!(op, Op::Shutdown)),
        "expected Shutdown op after cancellation"
    );
}

#[tokio::test]
async fn forward_ops_preserves_submission_trace_context() {
    let (tx_sub, rx_sub) = bounded(SUBMISSION_CHANNEL_CAPACITY);
    let (_tx_events, rx_events) = bounded(SUBMISSION_CHANNEL_CAPACITY);
    let (_agent_status_tx, agent_status) = watch::channel(AgentStatus::PendingInit);
    let io = Arc::new(SessionIo {
        admission: Arc::default(),
        tx_sub,
        rx_event: rx_events,
        agent_status,
        session_loop_termination: completed_session_loop_termination(),
    });
    let (tx_ops, rx_ops) = bounded(1);
    let cancel = CancellationToken::new();
    let forward = tokio::spawn(forward_ops(Arc::clone(&io), rx_ops, cancel));

    let submission = Submission {
        id: "sub-1".to_string(),
        op: Op::Interrupt,
        turn_extension_init: None,
        trace: Some(codex_protocol::protocol::W3cTraceContext {
            traceparent: Some(
                "00-1234567890abcdef1234567890abcdef-1234567890abcdef-01".to_string(),
            ),
            tracestate: Some("vendor=state".to_string()),
        }),
        parent_turn_id: Some("parent-turn".to_string()),
        root_turn_id: Some("root-turn".to_string()),
        residency_guard: None,
        message_delivery: None,
    };
    tx_ops.send(submission).await.unwrap();
    drop(tx_ops);

    let forwarded = timeout(Duration::from_secs(1), rx_sub.recv())
        .await
        .expect("forward_ops hung")
        .expect("forwarded submission missing");
    assert_eq!("sub-1", forwarded.id);
    assert!(matches!(forwarded.op, Op::Interrupt));
    assert_eq!(
        forwarded.trace,
        Some(codex_protocol::protocol::W3cTraceContext {
            traceparent: Some(
                "00-1234567890abcdef1234567890abcdef-1234567890abcdef-01".to_string(),
            ),
            tracestate: Some("vendor=state".to_string()),
        })
    );
    assert_eq!(Some("parent-turn".to_string()), forwarded.parent_turn_id);

    timeout(Duration::from_secs(1), forward)
        .await
        .expect("forward_ops did not exit")
        .expect("forward_ops join error");
}

#[tokio::test]
async fn one_shot_wrapper_shares_live_session_admission_and_rejects_closed_input() {
    assert_one_shot_owned_shutdown(true, false, OneShotOutput::Drain).await;
}

#[tokio::test]
async fn one_shot_completion_waits_for_owned_shutdown() {
    assert_one_shot_owned_shutdown(false, false, OneShotOutput::Drain).await;
}

#[tokio::test]
async fn one_shot_cancellation_during_owned_shutdown_preserves_teardown() {
    assert_one_shot_owned_shutdown(true, true, OneShotOutput::Drain).await;
}

#[tokio::test]
async fn one_shot_cancel_with_closed_input_and_full_output_finishes_shutdown() {
    assert_one_shot_owned_shutdown(true, false, OneShotOutput::CancelFull).await;
}

#[tokio::test]
async fn one_shot_disconnect_with_closed_input_and_full_output_finishes_shutdown() {
    assert_one_shot_owned_shutdown(true, false, OneShotOutput::DisconnectFull).await;
}

#[tokio::test]
async fn one_shot_disconnect_with_closed_input_while_waiting_finishes_shutdown() {
    assert_one_shot_owned_shutdown(true, false, OneShotOutput::DisconnectWaiting).await;
}

#[derive(Clone, Copy)]
enum OneShotOutput {
    Drain,
    CancelFull,
    DisconnectFull,
    DisconnectWaiting,
}

async fn assert_one_shot_owned_shutdown(
    close_input: bool,
    cancel_during_shutdown: bool,
    output: OneShotOutput,
) {
    use core_test_support::responses;
    use core_test_support::streaming_sse::{StreamingSseChunk, start_streaming_sse_server};

    struct StopGate {
        entered: tokio::sync::Mutex<Option<tokio::sync::oneshot::Sender<()>>>,
        release: tokio::sync::Mutex<Option<tokio::sync::oneshot::Receiver<()>>>,
        calls: Arc<AtomicUsize>,
    }

    impl ThreadLifecycleContributor<Config> for StopGate {
        fn on_thread_stop<'a>(
            &'a self,
            _input: codex_extension_api::ThreadStopInput<'a>,
        ) -> ExtensionFuture<'a, ()> {
            Box::pin(async move {
                self.calls.fetch_add(1, Ordering::SeqCst);
                self.entered.lock().await.take().unwrap().send(()).unwrap();
                let release = self.release.lock().await.take().unwrap();
                release.await.unwrap();
            })
        }
    }

    let mut initial_events = vec![responses::ev_response_created("one-shot")];
    if matches!(
        output,
        OneShotOutput::CancelFull | OneShotOutput::DisconnectFull
    ) {
        initial_events.push(responses::ev_message_item_added("message", ""));
        initial_events.extend(
            (0..SUBMISSION_CHANNEL_CAPACITY + 1)
                .map(|_| responses::ev_output_text_delta("bounded output ")),
        );
    }
    let (release, gate) = tokio::sync::oneshot::channel();
    let mut release = Some(release);
    let (server, _) = start_streaming_sse_server(vec![vec![
        StreamingSseChunk {
            gate: None,
            body: responses::sse(initial_events),
        },
        StreamingSseChunk {
            gate: Some(gate),
            body: responses::sse(vec![responses::ev_completed("one-shot")]),
        },
    ]])
    .await;
    let mut provider = codex_model_provider_info::built_in_model_providers(None)["openai"].clone();
    provider.base_url = Some(format!("{}/v1", server.uri()));
    provider.supports_websockets = false;
    let home = tempfile::tempdir().expect("isolated one-shot fixture home");
    let (mut parent, context, _events) =
        crate::session::tests::make_session_and_context_with_auth_config_home_and_rx(
            codex_login::CodexAuth::from_api_key("test"),
            Vec::new(),
            home.path(),
            move |config| {
                config.model = Some("gpt-5.2".to_string());
                config.model_provider = provider;
            },
        )
        .await;
    let (stop_entered_tx, stop_entered) = tokio::sync::oneshot::channel();
    let (stop_release, stop_release_rx) = tokio::sync::oneshot::channel();
    let stop_calls = Arc::new(AtomicUsize::new(0));
    let mut extensions = ExtensionRegistryBuilder::<Config>::new();
    extensions.thread_lifecycle_contributor(Arc::new(StopGate {
        entered: tokio::sync::Mutex::new(Some(stop_entered_tx)),
        release: tokio::sync::Mutex::new(Some(stop_release_rx)),
        calls: Arc::clone(&stop_calls),
    }));
    Arc::get_mut(&mut parent)
        .expect("parent session should be uniquely owned")
        .services
        .extensions = Arc::new(extensions.build());
    let cancel = CancellationToken::new();
    let mut config = context.config.as_ref().clone();
    config.permissions.approval_policy = Constrained::allow_only(AskForApproval::Never);
    let (session, io) = timeout(
        Duration::from_secs(10),
        run_codex_thread_one_shot(
            config,
            Arc::clone(&parent.services.auth_manager),
            Arc::clone(&parent.services.models_manager),
            vec![UserInput::Text {
                text: "one-shot admission fixture".into(),
                text_elements: Vec::new(),
            }],
            parent,
            context,
            cancel.clone(),
            SubAgentSource::Review,
            None,
            None,
        ),
    )
    .await
    .expect("one-shot startup deadline")
    .expect("one-shot should accept its initial input");
    assert!(Arc::ptr_eq(&session.cleanup_admission, &io.admission));
    let termination = io.session_loop_termination.clone();
    let agent_status = io.agent_status.clone();
    timeout(Duration::from_secs(10), server.wait_for_request_count(1))
        .await
        .expect("initial input must reach only the local mock provider");
    if close_input {
        session.cleanup_admission.close();
        let revision = session.cleanup_admission.activity_revision();
        let error = io
            .submit(Op::Interrupt)
            .await
            .expect_err("the returned wrapper must honor its live session's closed admission");
        assert!(
            matches!(error.details(), CodexErrorDetails::InvalidRequest(message)
            if message == "thread cleanup has closed input admission")
        );
        assert_eq!(session.cleanup_admission.activity_revision(), revision);
    }
    match output {
        OneShotOutput::Drain => {
            release.take().unwrap().send(()).unwrap();
            timeout(Duration::from_secs(10), async {
                loop {
                    let event = io.next_event().await.expect("one-shot completion event");
                    assert!(!is_private_delegate_event(&event));
                    if matches!(event.msg, EventMsg::TurnComplete(_)) {
                        break;
                    }
                }
            })
            .await
            .expect("the one-shot completion event must remain observable");
        }
        OneShotOutput::CancelFull
        | OneShotOutput::DisconnectFull
        | OneShotOutput::DisconnectWaiting => {
            if !matches!(output, OneShotOutput::DisconnectWaiting) {
                timeout(Duration::from_secs(10), async {
                    while !io.rx_event.is_full() {
                        tokio::task::yield_now().await;
                    }
                })
                .await
                .expect("real streamed events must fill the bounded one-shot output");
            }
            assert!(termination.clone().now_or_never().is_none());
            if matches!(output, OneShotOutput::CancelFull) {
                cancel.cancel();
            } else {
                // DisconnectFull cancels neither the parent nor its model stream.
                drop(io);
            }
            // Keep the local SSE completion withheld: only owned shutdown can
            // abort this active turn and reach the lifecycle barrier below.
        }
    }
    timeout(Duration::from_secs(10), stop_entered)
        .await
        .expect("shutdown must reach the real thread-stop lifecycle")
        .unwrap();
    assert!(termination.clone().now_or_never().is_none());
    assert!(session.cleanup_admission.is_closing());
    assert!(session.cleanup_admission.gate.try_read().is_err());
    assert_eq!(
        session
            .cleanup_admission
            .shutdown_result
            .load(Ordering::Acquire),
        0
    );
    if cancel_during_shutdown {
        cancel.cancel();
    }
    stop_release.send(()).unwrap();
    timeout(Duration::from_secs(10), termination)
        .await
        .expect("the real one-shot bridge must finish its owned shutdown")
        .expect("the real one-shot bridge must confirm cleanup");
    assert_eq!(*agent_status.borrow(), AgentStatus::Shutdown);
    assert_eq!(
        session
            .cleanup_admission
            .shutdown_result
            .load(Ordering::Acquire),
        1
    );
    assert!(
        session
            .cleanup_admission
            .loop_completed
            .load(Ordering::Acquire)
    );
    assert_eq!(stop_calls.load(Ordering::SeqCst), 1);
    assert_eq!(server.requests().await.len(), 1);
    drop(release);
    server.shutdown().await;
}

#[tokio::test]
async fn run_codex_thread_interactive_respects_pre_cancelled_spawn() {
    for shutdown_requested in [false, true] {
        let (parent_session, parent_ctx, _rx_events) =
            crate::session::tests::make_session_and_context_with_rx().await;
        if shutdown_requested {
            parent_session
                .services
                .local_agent_runtime
                .request_shutdown();
        }
        let mut config = parent_ctx.config.as_ref().clone();
        config.permissions.approval_policy = Constrained::allow_only(AskForApproval::Never);
        let cancel_token = CancellationToken::new();
        cancel_token.cancel();
        let parent_environments = parent_ctx.initial_environments.clone();

        let result = timeout(
            Duration::from_secs(/*secs*/ 1),
            run_codex_thread_interactive(
                config,
                Arc::clone(&parent_session.services.auth_manager),
                Arc::clone(&parent_session.services.models_manager),
                parent_session,
                parent_ctx,
                parent_environments,
                cancel_token,
                SubAgentSource::Review,
                codex_extension_api::SessionIsolation::Inherit,
                /*initial_history*/ None,
                crate::session::GitEnrichmentPolicy::Fresh,
                codex_sandboxing::WindowsSandboxProxySettingsMode::Reconcile,
            ),
        )
        .await
        .expect("cancelled delegate spawn should not hang");

        let error = result.err().expect("cancelled delegate spawn should fail");
        assert!(
            matches!(error.details(), CodexErrorDetails::TurnAborted),
            "unexpected cancelled delegate spawn error (shutdown_requested={shutdown_requested}): {error}"
        );
    }
}

#[tokio::test]
async fn delegate_start_analytics_honors_child_opt_out_with_enabled_parent() {
    use codex_analytics::AnalyticsEventsClient;
    use codex_login::CodexAuth;
    use wiremock::Mock;
    use wiremock::MockServer;
    use wiremock::ResponseTemplate;
    use wiremock::matchers::path;

    let server = MockServer::start().await;
    Mock::given(path("/codex/analytics-events/events"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&server)
        .await;
    let client = AnalyticsEventsClient::new(
        AuthManager::from_auth_for_testing(CodexAuth::create_dummy_chatgpt_auth_for_testing()),
        server.uri(),
        /*analytics_enabled*/ Some(true),
    );
    let (mut parent_session, parent_ctx, _rx_events) =
        crate::session::tests::make_session_and_context_with_rx().await;
    Arc::get_mut(&mut parent_session)
        .expect("parent session should be uniquely owned")
        .services
        .analytics_events_client = client.clone();
    parent_session
        .set_app_server_client_info(
            Some("codex-test".to_string()),
            Some("1.0.0".to_string()),
            /*mcp_elicitations_auto_deny*/ false,
        )
        .await
        .expect("set parent client metadata");

    let mut expected_events = Vec::new();
    for analytics_enabled in [false, true] {
        let mut config = parent_ctx.config.as_ref().clone();
        config.permissions.approval_policy = Constrained::allow_only(AskForApproval::Never);
        config.analytics_enabled = Some(analytics_enabled);
        let (session, io) = run_codex_thread_interactive(
            config,
            Arc::clone(&parent_session.services.auth_manager),
            Arc::clone(&parent_session.services.models_manager),
            Arc::clone(&parent_session),
            Arc::clone(&parent_ctx),
            parent_ctx.initial_environments.clone(),
            CancellationToken::new(),
            SubAgentSource::Review,
            codex_extension_api::SessionIsolation::Inherit,
            /*initial_history*/ None,
            crate::session::GitEnrichmentPolicy::Fresh,
            codex_sandboxing::WindowsSandboxProxySettingsMode::Reconcile,
        )
        .await
        .expect("delegate session should start");
        if analytics_enabled {
            expected_events.push(serde_json::json!([
                "codex_thread_initialized",
                session.thread_id().to_string(),
            ]));
        }
        io.shutdown_and_wait()
            .await
            .expect("delegate session should shut down");
    }
    client.flush().await;
    let events = server
        .received_requests()
        .await
        .expect("analytics requests")
        .into_iter()
        .flat_map(|request| {
            let payload: Value = serde_json::from_slice(&request.body).expect("analytics payload");
            payload["events"].as_array().expect("events array").clone()
        })
        .map(|event| serde_json::json!([event["event_type"], event["event_params"]["thread_id"]]))
        .collect::<Vec<_>>();
    assert_eq!(events, expected_events);
}

#[tokio::test]
async fn delegate_isolation_does_not_depend_on_attribution() {
    let (mut parent_session, mut parent_ctx, _rx_events) =
        crate::session::tests::make_session_and_context_with_rx().await;
    Arc::get_mut(&mut parent_ctx)
        .expect("parent turn should be uniquely owned")
        .disabled_plugin_ids = vec!["parent-root".to_string()];
    let thread_starts = Arc::new(AtomicUsize::new(0));
    let mut extensions = ExtensionRegistryBuilder::<Config>::new();
    extensions
        .thread_lifecycle_contributor(Arc::new(ThreadStartRecorder(Arc::clone(&thread_starts))));
    Arc::get_mut(&mut parent_session)
        .expect("parent session should be uniquely owned")
        .services
        .extensions = Arc::new(extensions.build());

    let request = parent_ctx
        .initial_environments
        .inheritable_requests()
        .into_iter()
        .next()
        .expect("parent environment");
    let root = SelectedCapabilityRoot {
        id: "parent-root".to_string(),
        location: CapabilityRootLocation::Environment {
            environment_id: request.environment_id.clone(),
            path: request.cwd.clone(),
        },
    };
    parent_session
        .services
        .turn_environments
        .update_selections(&[TurnEnvironmentSelection::new(
            request,
            std::slice::from_ref(&root),
        )]);
    let parent_environments = parent_session.services.turn_environments.snapshot().await;

    for (subagent_source, isolation, expected_thread_starts, expected_thread_source) in [
        (
            SubAgentSource::Other(crate::guardian::GUARDIAN_REVIEWER_NAME.to_string()),
            codex_extension_api::SessionIsolation::Isolated,
            0,
            ThreadSource::GuardianReview,
        ),
        (
            SubAgentSource::Review,
            codex_extension_api::SessionIsolation::Isolated,
            0,
            ThreadSource::Subagent,
        ),
        (
            SubAgentSource::Review,
            codex_extension_api::SessionIsolation::Inherit,
            1,
            ThreadSource::Subagent,
        ),
    ] {
        let mut config = parent_ctx.config.as_ref().clone();
        config.permissions.approval_policy = Constrained::allow_only(AskForApproval::Never);
        let (session, io) = run_codex_thread_interactive(
            config,
            Arc::clone(&parent_session.services.auth_manager),
            Arc::clone(&parent_session.services.models_manager),
            Arc::clone(&parent_session),
            Arc::clone(&parent_ctx),
            parent_environments.clone(),
            CancellationToken::new(),
            subagent_source,
            isolation,
            /*initial_history*/ None,
            crate::session::GitEnrichmentPolicy::Fresh,
            codex_sandboxing::WindowsSandboxProxySettingsMode::Reconcile,
        )
        .await
        .expect("delegate session should start");

        assert_eq!(
            session
                .services
                .extensions
                .thread_lifecycle_contributors()
                .len(),
            expected_thread_starts
        );
        assert_eq!(thread_starts.load(Ordering::SeqCst), expected_thread_starts);
        assert_eq!(
            session.thread_config_snapshot().await.thread_source,
            Some(expected_thread_source)
        );
        let expected_roots = match isolation {
            codex_extension_api::SessionIsolation::Inherit => vec![root.clone()],
            codex_extension_api::SessionIsolation::Isolated => Vec::new(),
        };
        assert_eq!(session.services.selected_capability_roots, expected_roots);
        let expected_disabled_plugins = match isolation {
            codex_extension_api::SessionIsolation::Inherit => vec!["parent-root".to_string()],
            codex_extension_api::SessionIsolation::Isolated => Vec::new(),
        };
        assert_eq!(
            session.thread_settings_snapshot().await.disabled_plugin_ids,
            expected_disabled_plugins,
        );
        assert_eq!(
            session
                .services
                .turn_environments
                .snapshot()
                .await
                .selected_capability_roots(),
            expected_roots,
        );
        io.shutdown_and_wait()
            .await
            .expect("delegate session should shut down");
    }
}

#[tokio::test]
async fn run_codex_thread_interactive_rejects_approval_policy_that_can_prompt() {
    let (parent_session, parent_ctx, _rx_events) =
        crate::session::tests::make_session_and_context_with_rx().await;
    let mut config = parent_ctx.config.as_ref().clone();
    config.permissions.approval_policy = Constrained::allow_any(AskForApproval::OnRequest);
    let parent_environments = parent_ctx.initial_environments.clone();

    let result = run_codex_thread_interactive(
        config,
        Arc::clone(&parent_session.services.auth_manager),
        Arc::clone(&parent_session.services.models_manager),
        parent_session,
        parent_ctx,
        parent_environments,
        CancellationToken::new(),
        SubAgentSource::Review,
        codex_extension_api::SessionIsolation::Inherit,
        /*initial_history*/ None,
        crate::session::GitEnrichmentPolicy::Fresh,
        codex_sandboxing::WindowsSandboxProxySettingsMode::Reconcile,
    )
    .await;

    assert!(matches!(
        result,
        Err(err)
            if matches!(
                err.details(),
                CodexErrorDetails::InvalidRequest(message)
                    if message == "Codex delegates require approval policy `never`"
            )
    ));
}

/// Private delegates participate in tree shutdown even though they are not manager-visible.
#[tokio::test]
async fn tree_shutdown_waits_for_private_delegate() {
    let (parent, context, _events) =
        crate::session::tests::make_session_and_context_with_rx().await;
    let mut config = context.config.as_ref().clone();
    config.permissions.approval_policy = Constrained::allow_only(AskForApproval::Never);
    let (_, io) = run_codex_thread_interactive(
        config,
        Arc::clone(&parent.services.auth_manager),
        Arc::clone(&parent.services.models_manager),
        Arc::clone(&parent),
        Arc::clone(&context),
        context.initial_environments.clone(),
        CancellationToken::new(),
        SubAgentSource::Review,
        codex_extension_api::SessionIsolation::Isolated,
        /*initial_history*/ None,
        crate::session::GitEnrichmentPolicy::Fresh,
        codex_sandboxing::WindowsSandboxProxySettingsMode::Reconcile,
    )
    .await
    .expect("start private delegate");

    let shutdown = parent.services.local_agent_runtime.request_shutdown();
    timeout(Duration::from_secs(/*secs*/ 10), shutdown.wait())
        .await
        .expect("tree shutdown should wait for the private delegate")
        .expect("private delegate should shut down cleanly");
    assert!(io.session_loop_termination.now_or_never().is_some());
}
