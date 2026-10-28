// Modified for OpenDIY Codex 0.165.0 local patch (2026-10-28); see MODIFICATIONS.md.
//! Public-boundary capacity retry tests (CONC-FENCE-002 / CONC-CANCEL-004).
//!
//! Plan: stop/account changes during PreModelRequest admit zero stale HTTP requests;
//! with InstantInterrupt enabled, steering during that hook or delayed headers admits fresh input;
//! an owner change during a resumed stream dispatches zero tools. Local HTTP mocks remove
//! the remote model only; native hooks, auth notifications, turn scheduling and dispatch stay real.
use anyhow::Context;
use codex_core::CodexThread;
use codex_features::Feature;
use codex_login::AuthManager;
use codex_login::CodexAuth;
use codex_protocol::dynamic_tools::DynamicToolFunctionSpec;
use codex_protocol::dynamic_tools::DynamicToolSpec;
use codex_protocol::models::ResponseItem;
use codex_protocol::protocol::EventMsg;
use codex_protocol::protocol::HookRunStatus;
use codex_protocol::protocol::Op;
use codex_protocol::turn_input::TurnInputRequest;
use codex_protocol::user_input::UserInput;
use core_test_support::fs_wait;
use core_test_support::hooks::trust_discovered_hooks;
use core_test_support::responses;
use core_test_support::streaming_sse::StreamingSseChunk;
use core_test_support::streaming_sse::start_streaming_sse_server;
use core_test_support::test_codex::test_codex;
use serde_json::Value;
use serde_json::json;
use std::fs;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Notify;
use tokio::time::timeout;

fn input(text: &str) -> TurnInputRequest {
    TurnInputRequest::user_input(vec![UserInput::Text {
        text: text.into(),
        text_elements: Vec::new(),
    }])
}

fn capacity_response() -> String {
    responses::sse_failed("capacity", "server_is_overloaded", "at capacity")
}

// Bare interpreter names execute under PowerShell, cmd.exe and Unix shells alike.
// Only the second pre-request hook blocks; the follow-up request must remain runnable.
const BARRIER_HOOK: &str = r#"import json
import pathlib
import sys
import time

home = pathlib.Path(__file__).parent
payload = json.load(sys.stdin)
event = payload["hook_event_name"]
log = home / (event + ".jsonl")
with log.open("a", encoding="utf-8") as output:
    output.write(json.dumps(payload) + "\n")
barrier = None
if len(log.read_text(encoding="utf-8").splitlines()) == 2:
    if event == "PreModelRequest" and "--pre-barrier" in sys.argv:
        barrier = "pre"
    if event == "PostModelRequest" and "--post-barrier" in sys.argv:
        barrier = "post"
if barrier is not None:
    (home / ("capacity-" + barrier + "-request-blocked")).touch()
    deadline = time.monotonic() + 20
    while not (home / ("release-capacity-" + barrier + "-request")).exists():
        if time.monotonic() >= deadline:
            raise TimeoutError("capacity model-request barrier was not released")
        time.sleep(0.01)
"#;

fn write_barrier_hooks(home: &Path, mode: &str) -> anyhow::Result<()> {
    let python = ["python", "python3"]
        .into_iter()
        .find_map(|candidate| {
            let result = Command::new(candidate)
                .args(["-c", "import sys; print(sys.executable)"])
                .output()
                .ok()?;
            result.status.success().then_some(candidate)
        })
        .context("capacity hook boundary fixture requires Python on PATH")?;
    let script = home.join("capacity_barrier.py");
    fs::write(&script, BARRIER_HOOK)?;
    let command = format!("{python} \"{}\" {mode}", script.display());
    let hook = json!([{"hooks": [{"type": "command", "command": command, "async": false}]}]);
    fs::write(
        home.join("hooks.json"),
        json!({"hooks": {"PreModelRequest": hook, "PostModelRequest": hook}}).to_string(),
    )?;
    Ok(())
}

fn assert_hook_succeeded(event: &EventMsg) {
    if let EventMsg::HookCompleted(completed) = event {
        assert_ne!(
            completed.run.status,
            HookRunStatus::Failed,
            "native fixture hook failed (including stderr): {:?}",
            completed.run
        );
    }
}

async fn wait_for_event<F>(codex: &CodexThread, mut predicate: F) -> EventMsg
where
    F: FnMut(&EventMsg) -> bool,
{
    core_test_support::wait_for_event(codex, |event| {
        assert_hook_succeeded(event);
        predicate(event)
    })
    .await
}

async fn watch_hook_failures(codex: &CodexThread) {
    loop {
        let event = codex
            .next_event()
            .await
            .expect("native hook event stream ended");
        assert_hook_succeeded(&event.msg);
    }
}

async fn wait_for_hook_path(
    codex: &CodexThread,
    path: PathBuf,
    duration: Duration,
) -> anyhow::Result<()> {
    tokio::select! {
        result = fs_wait::wait_for_path_exists(path, duration) => result.map(|_| ()),
        _ = watch_hook_failures(codex) => unreachable!("hook failure watcher returned"),
    }
}

async fn hook_inputs(
    codex: &CodexThread,
    home: &Path,
    event: &str,
    count: usize,
) -> anyhow::Result<Vec<Value>> {
    let logs = timeout(Duration::from_secs(10), async {
        loop {
            if let Ok(contents) = fs::read_to_string(home.join(format!("{event}.jsonl"))) {
                let values: Result<Vec<Value>, _> =
                    contents.lines().map(serde_json::from_str).collect();
                if let Ok(values) = values
                    && values.len() >= count
                {
                    return values;
                }
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    });
    tokio::select! {
        result = logs => result.context("native model-request hook did not write the expected inputs"),
        _ = watch_hook_failures(codex) => unreachable!("hook failure watcher returned"),
    }
}

#[test_case::test_case(false; "stop")]
#[test_case::test_case(true; "account owner change")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn capacity_pre_request_barrier_blocks_stale_http_after_cancellation(
    change_owner: bool,
) -> anyhow::Result<()> {
    let server = responses::start_mock_server().await;
    let mock = responses::mount_sse_once(&server, capacity_response()).await;
    let test = test_codex()
        .with_auth_manager(AuthManager::from_auth_for_testing(CodexAuth::from_api_key(
            "fixture-owner",
        )))
        .with_pre_build_hook(|home| {
            write_barrier_hooks(home, "--pre-barrier").expect("write native hook fixture")
        })
        .with_config(|config| {
            trust_discovered_hooks(config);
            config.model_provider.request_max_retries = Some(0);
            config.model_provider.stream_max_retries = Some(1);
        })
        .build_with_auto_env(&server)
        .await?;
    test.codex
        .start_or_steer_turn(input("stale capacity prompt"))
        .await?;
    wait_for_event(&test.codex, |event| {
        matches!(event, EventMsg::StreamError(_))
    })
    .await;
    wait_for_hook_path(
        &test.codex,
        test.codex_home_path().join("capacity-pre-request-blocked"),
        Duration::from_secs(15),
    )
    .await?;
    assert_eq!(
        mock.requests().len(),
        1,
        "second HTTP request escaped the hook barrier"
    );
    if change_owner {
        assert!(test.thread_manager.auth_manager().reload().await);
    } else {
        test.codex.submit(Op::Interrupt).await?;
    }
    timeout(
        Duration::from_secs(3),
        wait_for_event(&test.codex, |event| {
            assert!(!matches!(event, EventMsg::DynamicToolCallRequest(_)));
            matches!(event, EventMsg::TurnAborted(_) | EventMsg::TurnComplete(_))
        }),
    )
    .await
    .context("cancellation waited for the blocked native hook")?;
    let posts = hook_inputs(&test.codex, test.codex_home_path(), "PostModelRequest", 2).await?;
    assert_eq!(posts[1]["status"], "cancelled");
    // Release after cancellation to expose a late stale continuation before shutdown joins it.
    fs::write(
        test.codex_home_path().join("release-capacity-pre-request"),
        "",
    )?;
    test.codex.shutdown_and_wait().await?;
    let requests = server
        .received_requests()
        .await
        .context("record HTTP requests")?;
    assert_eq!(
        requests.iter().filter(|request| request.method == "POST" && request.url.path().ends_with("/responses")).count(),
        1,
        "cancelled capacity attempt sent the stale prompt"
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn capacity_steering_pre_request_barrier_sends_only_fresh_prompt() -> anyhow::Result<()> {
    let server = responses::start_mock_server().await;
    let mock = responses::mount_sse_sequence(
        &server,
        vec![capacity_response(), responses::sse_completed("fresh")],
    )
    .await;
    let test = test_codex()
        .with_pre_build_hook(|home| {
            write_barrier_hooks(home, "--pre-barrier").expect("write native hook fixture")
        })
        .with_config(|config| {
            trust_discovered_hooks(config);
            config
                .features
                .enable(Feature::InstantInterrupt)
                .expect("enable InstantInterrupt for capacity steering fixture");
            config.model_provider.request_max_retries = Some(0);
            config.model_provider.stream_max_retries = Some(1);
        })
        .build_with_auto_env(&server)
        .await?;
    test.codex.start_or_steer_turn(input("old input")).await?;
    wait_for_event(&test.codex, |event| {
        matches!(event, EventMsg::StreamError(_))
    })
    .await;
    wait_for_hook_path(
        &test.codex,
        test.codex_home_path().join("capacity-pre-request-blocked"),
        Duration::from_secs(15),
    )
    .await?;
    assert_eq!(mock.requests().len(), 1);
    test.codex
        .start_or_steer_turn(input("replacement input"))
        .await?;
    timeout(
        Duration::from_secs(3),
        wait_for_event(&test.codex, |event| {
            assert!(!matches!(event, EventMsg::Error(_)), "{event:?}");
            matches!(event, EventMsg::TurnComplete(done) if done.error.is_none())
        }),
    )
    .await
    .context("steering waited for the stale PreModelRequest hook")?;
    assert!(
        !test
            .codex_home_path()
            .join("release-capacity-pre-request")
            .exists()
    );
    let requests = mock.requests();
    assert_eq!(requests.len(), 2);
    assert!(
        requests[1]
            .body_json()
            .to_string()
            .contains("replacement input")
    );
    let posts = hook_inputs(&test.codex, test.codex_home_path(), "PostModelRequest", 3).await?;
    assert_eq!(
        posts
            .iter()
            .filter(|post| post["status"] == "cancelled")
            .count(),
        1
    );
    fs::write(
        test.codex_home_path().join("release-capacity-pre-request"),
        "",
    )?;
    test.codex.shutdown_and_wait().await?;
    assert_eq!(mock.requests().len(), 2);
    Ok(())
}

#[test_case::test_case("steer"; "new input")]
#[test_case::test_case("owner"; "account owner change")]
#[test_case::test_case("stop"; "stop")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn capacity_cancellation_drops_delayed_headers(action: &str) -> anyhow::Result<()> {
    let server = responses::start_mock_server().await;
    responses::mount_sse_once(&server, capacity_response()).await;
    let admitted = Arc::new(Notify::new());
    let notify = Arc::clone(&admitted);
    let steering = action == "steer";
    let test = test_codex()
        .with_auth_manager(AuthManager::from_auth_for_testing(CodexAuth::from_api_key(
            "fixture-owner",
        )))
        .with_pre_build_hook(|home| {
            write_barrier_hooks(home, "--no-barrier").expect("write native hook fixture")
        })
        .with_config(move |config| {
            trust_discovered_hooks(config);
            if steering {
                config.features.enable(Feature::InstantInterrupt).unwrap();
            } else {
                config.features.disable(Feature::InstantInterrupt).unwrap();
            }
            config.model_provider.request_max_retries = Some(0);
            config.model_provider.stream_max_retries = Some(1);
        })
        .build_with_auto_env(&server)
        .await?;
    test.codex
        .start_or_steer_turn(input("header wait original"))
        .await?;
    wait_for_event(&test.codex, |event| {
        matches!(event, EventMsg::StreamError(_))
    })
    .await;
    // Mount each reply after the preceding request: newest wiremock matches take precedence.
    let delayed = responses::mount_response_once_match(
        &server,
        move |_: &wiremock::Request| {
            notify.notify_one();
            true
        },
        responses::sse_response(responses::sse_completed("stale-headers"))
            .set_delay(Duration::from_secs(60)),
    )
    .await;
    timeout(Duration::from_secs(15), admitted.notified()).await?;
    let fresh = if action == "steer" {
        Some(responses::mount_sse_once(&server, responses::sse_completed("fresh-headers")).await)
    } else {
        None
    };
    match action {
        "steer" => {
            test.codex
                .start_or_steer_turn(input("header wait replacement"))
                .await?;
        }
        "owner" => {
            assert!(test.thread_manager.auth_manager().reload().await);
        }
        "stop" => {
            test.codex.submit(Op::Interrupt).await?;
        }
        _ => unreachable!("unknown fixture action"),
    }
    timeout(
        Duration::from_secs(3),
        wait_for_event(&test.codex, |event| {
            assert!(!matches!(event, EventMsg::Error(_)), "{event:?}");
            if action == "steer" {
                matches!(event, EventMsg::TurnComplete(done) if done.error.is_none())
            } else {
                matches!(event, EventMsg::TurnAborted(_) | EventMsg::TurnComplete(_))
            }
        }),
    )
    .await
    .context("cancellation waited for the 60-second stale response headers")?;
    assert_eq!(delayed.requests().len(), 1);
    let posts = hook_inputs(
        &test.codex,
        test.codex_home_path(),
        "PostModelRequest",
        if action == "steer" { 3 } else { 2 },
    )
    .await?;
    assert_eq!(posts[1]["status"], "cancelled");
    assert_eq!(posts[1]["error"], Value::Null);
    if let Some(fresh) = fresh {
        let fresh_requests = fresh.requests();
        assert_eq!(fresh_requests.len(), 1);
        assert!(
            fresh_requests[0]
                .body_json()
                .to_string()
                .contains("header wait replacement")
        );
    }
    test.codex.shutdown_and_wait().await?;
    let requests = server
        .received_requests()
        .await
        .context("record HTTP requests")?;
    assert_eq!(requests.iter().filter(|request| request.method == "POST" && request.url.path().ends_with("/responses")).count(), if action == "steer" { 3 } else { 2 });
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn capacity_resumed_stream_owner_change_blocks_tool_dispatch() -> anyhow::Result<()> {
    let (release, gate) = tokio::sync::oneshot::channel();
    let (server, _) = start_streaming_sse_server(vec![
        vec![StreamingSseChunk {
            gate: None,
            body: capacity_response(),
        }],
        vec![
            StreamingSseChunk {
                gate: None,
                body: responses::sse(vec![
                    responses::ev_response_created("resumed"),
                    responses::ev_message_item_added("resumed", ""),
                    responses::ev_output_text_delta("resumed owner fence armed"),
                ]),
            },
            StreamingSseChunk {
                gate: Some(gate),
                body: responses::sse(vec![
                    responses::ev_function_call("stale-tool", "capacity_effect", "{}"),
                    responses::ev_completed("resumed"),
                ]),
            },
        ],
    ])
    .await;
    let test = test_codex()
        .with_auth_manager(AuthManager::from_auth_for_testing(CodexAuth::from_api_key(
            "fixture-owner",
        )))
        .with_config(|config| {
            config.model_provider.request_max_retries = Some(0);
            config.model_provider.stream_max_retries = Some(1);
        })
        .build_with_streaming_server(&server)
        .await?;
    let mut options = test.start_thread_options().await;
    options.dynamic_tools = vec![DynamicToolSpec::Function(DynamicToolFunctionSpec {
        name: "capacity_effect".into(),
        description: "Observe stale dispatch without applying an effect".into(),
        input_schema: json!({"type": "object", "properties": {}}),
        defer_loading: false,
    })];
    let codex = test.thread_manager.start_thread(options).await?.thread;
    codex
        .start_or_steer_turn(input("owner fence tool boundary"))
        .await?;
    wait_for_event(&codex, |event| {
        matches!(event,
            EventMsg::AgentMessageContentDelta(delta) if delta.delta.contains("resumed owner fence armed")
        )
    })
    .await;
    assert!(test.thread_manager.auth_manager().reload().await);
    // The socket is still gated: owner change itself must end the active attempt promptly.
    timeout(
        Duration::from_secs(3),
        wait_for_event(&codex, |event| {
            assert!(
                !matches!(event, EventMsg::DynamicToolCallRequest(_)),
                "stale owner dispatched a tool"
            );
            matches!(event, EventMsg::TurnAborted(_) | EventMsg::TurnComplete(_))
        }),
    )
    .await
    .context("resumed stream did not observe owner cancellation")?;
    let _ = release.send(());
    codex.shutdown_and_wait().await?;
    test.codex.shutdown_and_wait().await?;
    assert_eq!(server.requests().await.len(), 2);
    server.shutdown().await;
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn capacity_owner_change_aborts_tool_queued_before_sync_post_hook() -> anyhow::Result<()> {
    let (release_stream, gate) = tokio::sync::oneshot::channel();
    let (server, _) = start_streaming_sse_server(vec![
        vec![StreamingSseChunk {
            gate: None,
            body: capacity_response(),
        }],
        vec![
            StreamingSseChunk {
                gate: None,
                body: responses::sse(vec![
                    responses::ev_response_created("queued-tool"),
                    responses::ev_function_call("queued-capacity-tool", "capacity_effect", "{}"),
                    responses::ev_assistant_message("after-tool", "pending tool queued"),
                ]),
            },
            StreamingSseChunk {
                gate: Some(gate),
                body: responses::sse(vec![responses::ev_completed("queued-tool")]),
            },
        ],
    ])
    .await;
    let test = test_codex()
        .with_auth_manager(AuthManager::from_auth_for_testing(CodexAuth::from_api_key(
            "fixture-owner",
        )))
        .with_pre_build_hook(|home| {
            write_barrier_hooks(home, "--post-barrier").expect("write native hook fixture")
        })
        .with_config(|config| {
            trust_discovered_hooks(config);
            config.model_provider.request_max_retries = Some(0);
            config.model_provider.stream_max_retries = Some(1);
        })
        .build_with_streaming_server(&server)
        .await?;
    let mut options = test.start_thread_options().await;
    options.dynamic_tools = vec![DynamicToolSpec::Function(DynamicToolFunctionSpec {
        name: "capacity_effect".into(),
        description: "Observe deferred dispatch without applying an effect".into(),
        input_schema: json!({"type": "object", "properties": {}}),
        defer_loading: false,
    })];
    let codex = test.thread_manager.start_thread(options).await?.thread;
    codex
        .start_or_steer_turn(input("queue capacity tool before owner change"))
        .await?;
    // This message follows OutputItemDone for the tool, so its future is already queued.
    wait_for_event(&codex, |event| {
        assert!(!matches!(event, EventMsg::DynamicToolCallRequest(_)), "sync post hook did not defer dispatch");
        matches!(event, EventMsg::AgentMessage(message) if message.message.contains("pending tool queued"))
    }).await;
    assert!(test.thread_manager.auth_manager().reload().await);
    timeout(
        Duration::from_secs(3),
        wait_for_event(&codex, |event| {
            assert!(
                !matches!(event, EventMsg::DynamicToolCallRequest(_)),
                "owner change admitted the queued tool"
            );
            matches!(event, EventMsg::TurnAborted(_) | EventMsg::TurnComplete(_))
        }),
    )
    .await
    .context("owner cancellation waited for the sync PostModelRequest hook or queued tool")?;
    wait_for_hook_path(
        &codex,
        test.codex_home_path().join("capacity-post-request-blocked"),
        Duration::from_secs(10),
    )
    .await?;
    let posts = hook_inputs(&codex, test.codex_home_path(), "PostModelRequest", 2).await?;
    assert_eq!(posts[1]["status"], "cancelled");
    assert_eq!(posts[1]["error"], Value::Null);
    assert!(
        !test
            .codex_home_path()
            .join("release-capacity-post-request")
            .exists()
    );
    let history = codex.conversation_history_snapshot().await;
    let history = serde_json::to_value(history.items().collect::<Vec<_>>())?;
    let outputs = history
        .as_array()
        .expect("serialized history items")
        .iter()
        .filter(|item| {
            item["type"] == "function_call_output" && item["call_id"] == "queued-capacity-tool"
        })
        .collect::<Vec<_>>();
    assert_eq!(
        outputs.len(),
        1,
        "queued call needs exactly one terminal history output"
    );
    assert_eq!(outputs[0]["output"], "aborted before dispatch");
    fs::write(
        test.codex_home_path().join("release-capacity-post-request"),
        "",
    )?;
    let _ = release_stream.send(());
    codex.shutdown_and_wait().await?;
    test.codex.shutdown_and_wait().await?;
    assert_eq!(server.requests().await.len(), 2);
    server.shutdown().await;
    Ok(())
}

#[test_case::test_case(true; "release hook after owner switch")]
#[test_case::test_case(false; "hold hook after owner switch")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn capacity_owner_change_during_completed_sync_post_hook_blocks_tool(
    release_after_switch: bool,
) -> anyhow::Result<()> {
    let server = responses::start_mock_server().await;
    let requests = responses::mount_sse_sequence(
        &server,
        vec![
            capacity_response(),
            responses::sse(vec![
                responses::ev_response_created("completed-before-owner-switch"),
                responses::ev_function_call("completed-capacity-tool", "capacity_effect", "{}"),
                responses::ev_completed("completed-before-owner-switch"),
            ]),
        ],
    )
    .await;
    let test = test_codex()
        .with_auth_manager(AuthManager::from_auth_for_testing(CodexAuth::from_api_key(
            "fixture-owner",
        )))
        .with_pre_build_hook(|home| {
            write_barrier_hooks(home, "--post-barrier").expect("write native hook fixture")
        })
        .with_config(|config| {
            trust_discovered_hooks(config);
            config.model_provider.request_max_retries = Some(0);
            config.model_provider.stream_max_retries = Some(1);
        })
        .build_with_auto_env(&server)
        .await?;
    let mut options = test.start_thread_options().await;
    options.dynamic_tools = vec![DynamicToolSpec::Function(DynamicToolFunctionSpec {
        name: "capacity_effect".into(),
        description: "Observe admission after a completed response and account change".into(),
        input_schema: json!({"type": "object", "properties": {}}),
        defer_loading: false,
    })];
    let codex = test.thread_manager.start_thread(options).await?.thread;
    codex
        .start_or_steer_turn(input("complete capacity recovery before owner switch"))
        .await?;
    // The server sends response.completed without a gate. Only the native sync post hook blocks.
    tokio::select! {
        result = fs_wait::wait_for_path_exists(
            test.codex_home_path().join("capacity-post-request-blocked"),
            Duration::from_secs(15),
        ) => { result?; }
        _ = wait_for_event(&codex, |event| {
            assert!(!matches!(event, EventMsg::DynamicToolCallRequest(_)), "tool dispatched before the sync post hook was released");
            false
        }) => unreachable!("event observer returned before the hook barrier"),
    }
    let posts = hook_inputs(&codex, test.codex_home_path(), "PostModelRequest", 2).await?;
    assert_eq!(posts[1]["status"], "completed");
    assert_eq!(posts[1]["error"], Value::Null);
    assert_eq!(requests.requests().len(), 2);
    let release = test.codex_home_path().join("release-capacity-post-request");
    assert!(!release.exists());
    assert!(test.thread_manager.auth_manager().reload().await);
    if release_after_switch {
        fs::write(&release, "")?;
    }
    let ended = timeout(
        Duration::from_secs(3),
        wait_for_event(&codex, |event| {
            assert!(
                !matches!(event, EventMsg::DynamicToolCallRequest(_)),
                "owner change admitted a tool queued before the completed post hook"
            );
            matches!(event, EventMsg::TurnAborted(_) | EventMsg::TurnComplete(_))
        }),
    )
    .await;
    // Release even on timeout so a failed held-hook case leaves no waiting native process.
    fs::write(&release, "")?;
    ended.context("owner change did not interrupt the completed sync PostModelRequest wait")?;
    let history = codex.conversation_history_snapshot().await;
    let history = serde_json::to_value(history.items().collect::<Vec<_>>())?;
    let outputs = history
        .as_array()
        .expect("serialized history items")
        .iter()
        .filter(|item| {
            item["type"] == "function_call_output" && item["call_id"] == "completed-capacity-tool"
        })
        .collect::<Vec<_>>();
    assert_eq!(
        outputs.len(),
        1,
        "completed call needs one aborted history output"
    );
    assert_eq!(outputs[0]["output"], "aborted before dispatch");
    // An account switch cannot change the fact that the model response already completed.
    assert_eq!(posts[1]["status"], "completed");
    codex.shutdown_and_wait().await?;
    test.codex.shutdown_and_wait().await?;
    assert_eq!(requests.requests().len(), 2);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn capacity_owner_change_cancels_admitted_dynamic_tool_drain() -> anyhow::Result<()> {
    let server = responses::start_mock_server().await;
    let requests = responses::mount_sse_sequence(
        &server,
        vec![
            capacity_response(),
            responses::sse(vec![
                responses::ev_response_created("admitted-drain"),
                responses::ev_function_call("admitted-capacity-tool", "capacity_effect", "{}"),
                responses::ev_completed_with_tokens("admitted-drain", 42),
            ]),
        ],
    )
    .await;
    let test = test_codex()
        .with_auth_manager(AuthManager::from_auth_for_testing(CodexAuth::from_api_key(
            "fixture-owner",
        )))
        .with_pre_build_hook(|home| {
            write_barrier_hooks(home, "--no-barrier").expect("write native hook fixture")
        })
        .with_config(|config| {
            trust_discovered_hooks(config);
            config.model_provider.request_max_retries = Some(0);
            config.model_provider.stream_max_retries = Some(1);
        })
        .build_with_auto_env(&server)
        .await?;
    let mut options = test.start_thread_options().await;
    options.dynamic_tools = vec![DynamicToolSpec::Function(DynamicToolFunctionSpec {
        name: "capacity_effect".into(),
        description: "Wait for a host reply after admission without applying an external effect"
            .into(),
        input_schema: json!({"type": "object", "properties": {}}),
        defer_loading: false,
    })];
    let codex = test.thread_manager.start_thread(options).await?.thread;
    codex
        .start_or_steer_turn(input("admit capacity tool before owner switch"))
        .await?;
    // Both the response and sync post hook finish before the live tool asks its host for a reply.
    let EventMsg::DynamicToolCallRequest(request) = wait_for_event(&codex, |event| {
        matches!(event, EventMsg::DynamicToolCallRequest(_))
    })
    .await
    else {
        unreachable!("dynamic tool request predicate returned another event")
    };
    assert_eq!(request.call_id, "admitted-capacity-tool");
    let posts = hook_inputs(&codex, test.codex_home_path(), "PostModelRequest", 2).await?;
    assert_eq!(posts[1]["status"], "completed");
    assert_eq!(posts[1]["error"], Value::Null);
    assert_eq!(posts[1]["token_usage"]["total_tokens"], 42);
    assert_eq!(posts[1]["token_usage"]["input_tokens"], 42);
    assert_eq!(requests.requests().len(), 2);
    assert!(test.thread_manager.auth_manager().reload().await);
    // Do not answer the admitted call: account cancellation must end its drain itself.
    let ended = timeout(
        Duration::from_secs(3),
        wait_for_event(&codex, |event| {
            assert!(
                !matches!(event, EventMsg::DynamicToolCallRequest(_)),
                "owner change admitted an additional dynamic tool"
            );
            matches!(event, EventMsg::TurnAborted(_) | EventMsg::TurnComplete(_))
        }),
    )
    .await;
    if let Err(error) = ended {
        // Cleanup cannot turn the failed owner-cancellation deadline into a passing test.
        let _ = codex.submit(Op::Interrupt).await;
        let _ = codex.shutdown_and_wait().await;
        let _ = test.codex.shutdown_and_wait().await;
        return Err(error).context("owner change did not cancel the admitted dynamic tool drain");
    }
    let history = codex.conversation_history_snapshot().await;
    let outputs = history
        .items()
        .filter_map(|item| match item {
            ResponseItem::FunctionCallOutput {
                call_id, output, ..
            } if call_id.as_deref() == Some("admitted-capacity-tool") => Some(output),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        outputs.len(),
        1,
        "admitted call needs exactly one terminal output"
    );
    // DynamicToolHandler finishes cancellation itself (tools/handlers/dynamic.rs), and
    // tools/parallel.rs::failure_response retains success=false. Inspect the typed payload:
    // protocol/models.rs::FunctionCallOutputPayload serializes only body, omitting success.
    assert_eq!(outputs[0].success, Some(false));
    let output = outputs[0].text_content().expect("text cancellation output");
    assert!(
        output.contains("dynamic tool call was cancelled before receiving a response"),
        "admitted dynamic tool must record its handler cancellation, got {output:?}"
    );
    assert_ne!(output, "aborted before dispatch");
    codex.shutdown_and_wait().await?;
    test.codex.shutdown_and_wait().await?;
    assert_eq!(requests.requests().len(), 2);
    Ok(())
}
