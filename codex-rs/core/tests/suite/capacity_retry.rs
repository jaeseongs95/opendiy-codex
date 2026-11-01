// Modified for OpenDIY Codex R32 (2026-11-01); see MODIFICATIONS.md.
//! Loopback-only coverage of the same-turn capacity recovery boundary.
use codex_core::PromptCacheKey;
use codex_core::StartThreadOptions;
use codex_features::Feature;
use codex_protocol::dynamic_tools::DynamicToolCallOutputContentItem;
use codex_protocol::dynamic_tools::DynamicToolFunctionSpec;
use codex_protocol::dynamic_tools::DynamicToolResponse;
use codex_protocol::dynamic_tools::DynamicToolSpec;
use codex_protocol::protocol::CodexErrorInfo;
use codex_protocol::protocol::EventMsg;
use codex_protocol::protocol::Op;
use codex_protocol::turn_input::TurnInputRequest;
use codex_protocol::user_input::UserInput;
use core_test_support::responses;
use core_test_support::streaming_sse::StreamingSseChunk;
use core_test_support::streaming_sse::start_streaming_sse_server;
use core_test_support::test_codex::test_codex;
use core_test_support::wait_for_event;
use serde_json::json;
use std::time::Duration;

fn input(text: &str) -> TurnInputRequest {
    TurnInputRequest::user_input(vec![UserInput::Text {
        text: text.into(),
        text_elements: Vec::new(),
    }])
}

fn overload() -> serde_json::Value {
    json!({"type":"response.failed", "response":{
        "id":"capacity", "error":{"code":"server_is_overloaded"}
    }})
}

#[test_case::test_case(PromptCacheKey::SessionDerived; "session_derived")]
#[test_case::test_case(PromptCacheKey::Custom("capacity-custom-prefix".into()); "custom")]
#[test_case::test_case(PromptCacheKey::Omit; "omitted")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn capacity_recovers_in_same_turn_with_same_request_identity(
    policy: PromptCacheKey,
) -> anyhow::Result<()> {
    let server = responses::start_mock_server().await;
    let mock = responses::mount_sse_sequence(
        &server,
        vec![
            responses::sse(vec![overload()]),
            responses::sse_completed("recovered"),
        ],
    )
    .await;
    let mut test = test_codex()
        .with_auth(codex_login::CodexAuth::from_api_key("fixture-identity"))
        .with_config(|config| {
            config.service_tier = Some("flex".into());
            config.model_provider.request_max_retries = Some(0);
            config.model_provider.stream_max_retries = Some(1);
        })
        .build_with_auto_env(&server)
        .await?;
    let started = test
        .thread_manager
        .start_thread(StartThreadOptions {
            prompt_cache_key: policy.clone(),
            ..test.start_thread_options().await
        })
        .await?;
    test.codex = started.thread;
    test.session_configured = started.session_configured;
    test.codex
        .start_or_steer_turn(input("capacity recovery"))
        .await?;
    let mut starts = 0;
    wait_for_event(&test.codex, |event| {
        assert!(!matches!(event, EventMsg::Error(_)), "{event:?}");
        if matches!(event, EventMsg::TurnStarted(_)) {
            starts += 1;
        }
        matches!(event, EventMsg::TurnComplete(done) if done.error.is_none())
    })
    .await;
    assert_eq!(starts, 1, "recovery must not create another turn");
    let requests = mock.requests();
    assert_eq!(requests.len(), 2);
    let first = requests[0].body_json();
    let second = requests[1].body_json();
    assert_eq!(first["service_tier"], "flex");
    let expected_key = match policy {
        PromptCacheKey::SessionDerived => {
            Some(json!(test.session_configured.session_id.to_string()))
        }
        PromptCacheKey::Custom(key) => Some(json!(key)),
        PromptCacheKey::Omit => None,
    };
    assert_eq!(first.get("prompt_cache_key"), expected_key.as_ref());
    for key in ["model", "service_tier", "prompt_cache_key", "input"] {
        assert_eq!(first[key], second[key], "changed {key}");
    }
    for header in ["session-id", "thread-id", "authorization"] {
        assert!(requests[0].header(header).is_some(), "missing {header}");
        assert_eq!(requests[0].header(header), requests[1].header(header));
    }
    assert_eq!(
        requests[0].header("authorization").as_deref(),
        Some("Bearer fixture-identity")
    );
    test.codex.shutdown_and_wait().await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn capacity_new_input_interrupts_wait_with_instant_interrupt() -> anyhow::Result<()> {
    let server = responses::start_mock_server().await;
    let mock = responses::mount_sse_sequence(
        &server,
        vec![
            responses::sse(vec![overload()]),
            responses::sse_completed("steered"),
        ],
    )
    .await;
    let test = test_codex()
        .with_config(|config| {
            config
                .features
                .enable(Feature::InstantInterrupt)
                .expect("test config must enable InstantInterrupt");
            config.model_provider.request_max_retries = Some(0);
            config.model_provider.stream_max_retries = Some(1);
        })
        .build_with_auto_env(&server)
        .await?;
    test.codex
        .start_or_steer_turn(input("original capacity prompt"))
        .await?;
    wait_for_event(&test.codex, |event| {
        matches!(event, EventMsg::StreamError(_))
    })
    .await;
    test.codex
        .start_or_steer_turn(input("new capacity prompt"))
        .await?;
    tokio::time::timeout(
        Duration::from_secs(3),
        wait_for_event(&test.codex, |event| {
            assert!(!matches!(event, EventMsg::Error(_)), "{event:?}");
            matches!(event, EventMsg::TurnComplete(done) if done.error.is_none())
        }),
    )
    .await
    .expect("new input must skip the minimum 4.5-second capacity backoff");
    let requests = mock.requests();
    assert_eq!(requests.len(), 2);
    let second = requests[1].body_json().to_string();
    assert!(second.contains("original capacity prompt"));
    assert!(second.contains("new capacity prompt"));
    test.codex.shutdown_and_wait().await?;
    Ok(())
}

#[test_case::test_case(true; "enabled preempts resumed stream")]
#[test_case::test_case(false; "disabled retains queued input through retry and stream")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn capacity_new_input_respects_instant_interrupt(
    instant_interrupt: bool,
) -> anyhow::Result<()> {
    let (release, gate) = tokio::sync::oneshot::channel();
    let mut release = Some(release);
    let (server, _) = start_streaming_sse_server(vec![
        vec![StreamingSseChunk {
            gate: None,
            body: responses::sse(vec![overload()]),
        }],
        vec![
            StreamingSseChunk {
                gate: None,
                body: responses::sse(vec![
                    responses::ev_response_created("active"),
                    responses::ev_message_item_added("active", ""),
                    responses::ev_output_text_delta("capacity active stream"),
                ]),
            },
            StreamingSseChunk {
                gate: Some(gate),
                body: responses::sse(vec![responses::ev_completed("active")]),
            },
        ],
        vec![StreamingSseChunk {
            gate: None,
            body: responses::sse_completed("steered-active"),
        }],
    ])
    .await;
    let test = test_codex()
        .with_config(move |config| {
            if instant_interrupt {
                config.features.enable(Feature::InstantInterrupt).unwrap();
            } else {
                config.features.disable(Feature::InstantInterrupt).unwrap();
            }
            config.model_provider.request_max_retries = Some(0);
            config.model_provider.stream_max_retries = Some(1);
        })
        .build_with_streaming_server(&server)
        .await?;
    test.codex
        .start_or_steer_turn(input("capacity active original"))
        .await?;
    if !instant_interrupt {
        wait_for_event(&test.codex, |event| {
            matches!(event, EventMsg::StreamError(_))
        })
        .await;
        assert_eq!(server.requests().await.len(), 1);
        test.codex
            .start_or_steer_turn(input("capacity active replacement"))
            .await?;
    }
    wait_for_event(&test.codex, |event| matches!(event, EventMsg::AgentMessageContentDelta(delta) if delta.delta.contains("capacity active stream"))).await;
    if instant_interrupt {
        test.codex
            .start_or_steer_turn(input("capacity active replacement"))
            .await?;
    } else {
        let requests = server.requests().await;
        assert_eq!(requests.len(), 2);
        let retry: serde_json::Value = serde_json::from_slice(&requests[1])?;
        assert!(retry.to_string().contains("capacity active original"));
        assert!(!retry.to_string().contains("capacity active replacement"));
        assert!(
            tokio::time::timeout(
                Duration::from_secs(3),
                wait_for_event(&test.codex, |event| {
                    assert!(!matches!(event, EventMsg::Error(_)), "{event:?}");
                    matches!(event, EventMsg::TurnComplete(_))
                }),
            )
            .await
            .is_err(),
            "disabled InstantInterrupt must keep the retried stream running"
        );
        assert_eq!(server.requests().await.len(), 2);
        release
            .take()
            .unwrap()
            .send(())
            .expect("release retained stream");
    }
    tokio::time::timeout(
        Duration::from_secs(3),
        wait_for_event(
            &test.codex,
            |event| matches!(event, EventMsg::TurnComplete(done) if done.error.is_none()),
        ),
    )
    .await
    .expect("queued input must be handled after preemption or normal response completion");
    let requests = server.requests().await;
    assert_eq!(requests.len(), 3);
    let third: serde_json::Value = serde_json::from_slice(&requests[2])?;
    assert!(third.to_string().contains("capacity active replacement"));
    drop(release);
    test.codex.shutdown_and_wait().await?;
    server.shutdown().await;
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn capacity_user_cancel_drops_wait_without_resending() -> anyhow::Result<()> {
    let server = responses::start_mock_server().await;
    let mock = responses::mount_sse_once(&server, responses::sse(vec![overload()])).await;
    let test = test_codex()
        .with_config(|config| {
            config.model_provider.request_max_retries = Some(0);
        })
        .build_with_auto_env(&server)
        .await?;
    test.codex
        .start_or_steer_turn(input("cancel capacity wait"))
        .await?;
    wait_for_event(&test.codex, |event| {
        matches!(event, EventMsg::StreamError(_))
    })
    .await;
    test.codex.submit(Op::Interrupt).await?;
    wait_for_event(&test.codex, |event| {
        matches!(event, EventMsg::TurnAborted(_))
    })
    .await;
    test.codex.shutdown_and_wait().await?;
    assert_eq!(mock.requests().len(), 1);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn capacity_after_partial_output_is_terminal() -> anyhow::Result<()> {
    let server = responses::start_mock_server().await;
    let mock = responses::mount_sse_once(
        &server,
        responses::sse(vec![
            responses::ev_response_created("partial"),
            responses::ev_message_item_added("partial", "already visible"),
            overload(),
        ]),
    )
    .await;
    let test = test_codex()
        .with_config(|config| {
            config.model_provider.request_max_retries = Some(0);
        })
        .build_with_auto_env(&server)
        .await?;
    test.codex
        .start_or_steer_turn(input("partial capacity response"))
        .await?;
    wait_for_event(&test.codex, |event| {
        assert!(!matches!(event, EventMsg::StreamError(_)), "partial response retried");
        matches!(event, EventMsg::TurnComplete(done) if
            done.error.as_ref().and_then(|error| error.codex_error_info.clone()) == Some(CodexErrorInfo::ServerOverloaded))
    }).await;
    assert_eq!(mock.requests().len(), 1);
    test.codex.shutdown_and_wait().await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn capacity_after_tool_dispatch_never_replays_tool() -> anyhow::Result<()> {
    let server = responses::start_mock_server().await;
    let mock = responses::mount_sse_once(
        &server,
        responses::sse(vec![
            responses::ev_response_created("tool-response"),
            responses::ev_function_call("capacity-tool", "capacity_counter", "{}"),
            overload(),
        ]),
    )
    .await;
    let test = test_codex()
        .with_config(|config| {
            config.model_provider.request_max_retries = Some(0);
        })
        .build_with_auto_env(&server)
        .await?;
    let codex = test
        .thread_manager
        .start_thread(StartThreadOptions {
            dynamic_tools: vec![DynamicToolSpec::Function(DynamicToolFunctionSpec {
                name: "capacity_counter".into(),
                description: "Local effect counter".into(),
                input_schema: json!({"type":"object", "properties":{}}),
                defer_loading: false,
            })],
            ..StartThreadOptions::new(test.config.clone())
        })
        .await?
        .thread;
    codex
        .start_or_steer_turn(input("count capacity dispatches"))
        .await?;
    let EventMsg::DynamicToolCallRequest(request) = wait_for_event(&codex, |event| {
        matches!(event, EventMsg::DynamicToolCallRequest(_))
    })
    .await
    else {
        unreachable!()
    };
    let mut effects = 1;
    codex
        .submit(Op::DynamicToolResponse {
            id: request.call_id,
            response: DynamicToolResponse {
                content_items: vec![DynamicToolCallOutputContentItem::InputText {
                    text: "effect applied once".into(),
                }],
                success: true,
            },
        })
        .await?;
    wait_for_event(&codex, |event| {
        if matches!(event, EventMsg::DynamicToolCallRequest(_)) { effects += 1; }
        assert!(!matches!(event, EventMsg::StreamError(_)), "tool-bearing response retried");
        matches!(event, EventMsg::TurnComplete(done) if
            done.error.as_ref().and_then(|error| error.codex_error_info.clone()) == Some(CodexErrorInfo::ServerOverloaded))
    }).await;
    assert_eq!(effects, 1);
    assert_eq!(mock.requests().len(), 1);
    codex.shutdown_and_wait().await?;
    test.codex.shutdown_and_wait().await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn capacity_completed_tool_output_is_retained_across_retry() -> anyhow::Result<()> {
    let server = responses::start_mock_server().await;
    let mock = responses::mount_sse_sequence(
        &server,
        vec![
            responses::sse(vec![
                responses::ev_response_created("tool-ok"),
                responses::ev_function_call("once", "capacity_counter", "{}"),
                responses::ev_completed("tool-ok"),
            ]),
            responses::sse(vec![overload()]),
            responses::sse_completed("continued"),
        ],
    )
    .await;
    let test = test_codex()
        .with_config(|config| {
            config.model_provider.request_max_retries = Some(0);
        })
        .build_with_auto_env(&server)
        .await?;
    let mut options = test.start_thread_options().await;
    options.dynamic_tools = vec![DynamicToolSpec::Function(DynamicToolFunctionSpec {
        name: "capacity_counter".into(),
        description: "Count a local host effect".into(),
        input_schema: json!({"type":"object", "properties":{}}),
        defer_loading: false,
    })];
    let codex = test.thread_manager.start_thread(options).await?.thread;
    codex
        .start_or_steer_turn(input("retain the completed effect"))
        .await?;
    let mut effects = 0;
    let EventMsg::DynamicToolCallRequest(request) = wait_for_event(&codex, |event| {
        matches!(event, EventMsg::DynamicToolCallRequest(_))
    })
    .await
    else {
        unreachable!()
    };
    effects += 1;
    codex
        .submit(Op::DynamicToolResponse {
            id: request.call_id,
            response: DynamicToolResponse {
                content_items: vec![DynamicToolCallOutputContentItem::InputText {
                    text: "effect result 1".into(),
                }],
                success: true,
            },
        })
        .await?;
    wait_for_event(&codex, |event| {
        if matches!(event, EventMsg::DynamicToolCallRequest(_)) {
            effects += 1;
        }
        assert!(!matches!(event, EventMsg::Error(_)), "{event:?}");
        matches!(event, EventMsg::TurnComplete(done) if done.error.is_none())
    })
    .await;
    assert_eq!(effects, 1);
    let requests = mock.requests();
    assert_eq!(requests.len(), 3);
    for request in &requests[1..] {
        let body = request.body_json();
        let history = body["input"].as_array().expect("history");
        for kind in ["function_call", "function_call_output"] {
            assert_eq!(
                history
                    .iter()
                    .filter(|item| item["type"] == kind && item["call_id"] == "once")
                    .count(),
                1,
                "{kind}"
            );
        }
        assert!(body.to_string().contains("effect result 1"));
    }
    codex.shutdown_and_wait().await?;
    test.codex.shutdown_and_wait().await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn capacity_budget_survives_real_generic_transport_fallback() -> anyhow::Result<()> {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, ResponseTemplate};
    let server = responses::start_mock_server().await;
    let gets = Arc::new(AtomicUsize::new(0));
    let warmup = Arc::new(tokio::sync::Notify::new());
    let count = gets.clone();
    let notify = warmup.clone();
    Mock::given(method("GET"))
        .and(path("/v1/responses"))
        .respond_with(
            move |_: &wiremock::Request| match count.fetch_add(1, Ordering::SeqCst) {
                0 => {
                    notify.notify_one();
                    ResponseTemplate::new(500)
                }
                1 => ResponseTemplate::new(503)
                    .set_body_json(json!({"error":{"code":"server_is_overloaded"}})),
                _ => ResponseTemplate::new(500)
                    .set_body_json(json!({"error":{"code":"internal_server_error"}})),
            },
        )
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/v1/responses"))
        .respond_with(
            ResponseTemplate::new(503)
                .set_body_json(json!({"error":{"code":"server_is_overloaded"}})),
        )
        .mount(&server)
        .await;
    let test = test_codex()
        .with_config(|config| {
            config.model_catalog =
                Some(codex_models_manager::bundled_models_response().expect("bundled models"));
            config.model_provider.supports_websockets = true;
            config.model_provider.request_max_retries = Some(0);
            config.model_provider.stream_max_retries = Some(1);
        })
        .build_with_auto_env(&server)
        .await?;
    tokio::time::timeout(Duration::from_secs(10), warmup.notified()).await?;
    test.codex
        .start_or_steer_turn(input("capacity then ordinary transport failure"))
        .await?;
    let mut fallbacks = 0;
    let mut capacity_retries = 0;
    wait_for_event(&test.codex, |event| {
        if matches!(event, EventMsg::Warning(w) if w.message.contains("Falling back from WebSockets")) { fallbacks += 1; }
        if matches!(event, EventMsg::StreamError(e) if e.message.starts_with("Model at capacity.")) { capacity_retries += 1; }
        matches!(event, EventMsg::TurnComplete(done) if done.error.as_ref().and_then(|error| error.codex_error_info.clone()) == Some(CodexErrorInfo::ServerOverloaded))
    }).await;
    assert_eq!(fallbacks, 1);
    assert_eq!(
        capacity_retries, 1,
        "generic fallback must not replenish capacity budget"
    );
    assert_eq!(
        gets.load(Ordering::SeqCst),
        3,
        "one warmup and two sampling GETs"
    );
    assert_eq!(
        server
            .received_requests()
            .await
            .unwrap()
            .iter()
            .filter(|request| request.method == "POST")
            .count(),
        1
    );
    test.codex.shutdown_and_wait().await?;
    Ok(())
}
