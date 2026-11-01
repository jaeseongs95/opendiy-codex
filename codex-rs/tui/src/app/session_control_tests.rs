// Modified for OpenDIY Codex R31 (2026-11-01); see MODIFICATIONS.md.
use super::*;
use crate::backend_id::BackendId;
use crate::session_control;
use codex_app_server_protocol::{DynamicToolCallParams, DynamicToolCallResponse};
use codex_features::Feature;
use codex_utils_absolute_path::test_support::PathExt;
use std::sync::{Arc, atomic::AtomicU8};
use tokio::sync::oneshot;
use tokio_util::sync::CancellationToken;

fn request(
    session: &AppServerSession,
    thread: ThreadId,
    call: &str,
    arguments: Value,
) -> (
    crate::cron_tools::Request,
    oneshot::Receiver<DynamicToolCallResponse>,
) {
    let (reply, result) = oneshot::channel();
    (
        crate::cron_tools::Request {
            connection: session.cron_tool_connection(),
            params: DynamicToolCallParams {
                thread_id: thread.to_string(),
                turn_id: "control-turn".into(),
                call_id: call.into(),
                namespace: Some(crate::dynamic_tools::NAMESPACE.into()),
                tool: session_control::NAME.into(),
                arguments: arguments.clone(),
            },
            command: session_control::command(arguments).unwrap(),
            claim: Arc::new(AtomicU8::new(0)),
            cancellation: CancellationToken::new(),
            reply,
        },
        result,
    )
}

#[tokio::test]
async fn model_control_call_preserves_origin_cancellation_and_replay_boundary()
-> color_eyre::Result<()> {
    let (mut app, _events, _) = crate::app::tests::make_test_app_with_channels().await;
    let (mut server, mut observed, worker) =
        super::super::app_server_requests::retained_reply_tests::connection().await?;
    let owner = ThreadId::new();
    app.active_thread_id = Some(owner);
    app.ensure_thread_channel(owner)
        .store
        .lock()
        .await
        .set_active_turn_id("control-turn".into());
    for case in ["cancelled", "stale", "other-screen", "valid", "valid"] {
        let (mut call, response) = request(&server, owner, case, json!({"action":"status"}));
        match case {
            "cancelled" => call.cancellation.cancel(),
            "stale" => call.connection.backend = BackendId::new(),
            "other-screen" => app.active_thread_id = Some(ThreadId::new()),
            _ => app.active_thread_id = Some(owner),
        }
        app.handle_session_control_tool_call(&mut server, call)
            .await;
        if case == "cancelled" {
            assert!(response.await.is_err());
        } else {
            assert_eq!(response.await?.success, case == "valid");
        }
    }
    let (call, response) = request(&server, owner, "valid", json!({"action":"context_reset"}));
    app.handle_session_control_tool_call(&mut server, call)
        .await;
    assert!(
        !response.await?.success,
        "같은 식별자의 다른 요청을 실행하지 않음"
    );
    assert!(
        observed.try_recv().is_err(),
        "거절·상태 조회·동일 호출 재시도는 서버 쓰기를 하지 않음"
    );
    server.shutdown().await?;
    worker.await??;

    let thread = owner;
    let (mut server, mut requests, worker) =
        control_server(json!({"summary":"x".repeat(1024 * 1024)})).await?;
    app.active_thread_id = Some(thread);
    app.ensure_thread_channel(thread)
        .store
        .lock()
        .await
        .set_active_turn_id("control-turn".into());
    for _ in 0..2 {
        let (call, response) = request(
            &server,
            thread,
            "large-read",
            json!({"action":"memory_view"}),
        );
        let key = call.key(thread);
        app.handle_session_control_tool_call(&mut server, call)
            .await;
        assert!(response.await?.success);
        assert_eq!(next_request(&mut requests).await.0, "thread/memory/read");
        assert!(
            serde_json::to_vec(&app.cron.model_calls[&key].1)?.len() < 1024,
            "읽기 재시도는 요약 전체를 호출 기록에 누적하지 않음"
        );
    }
    server.shutdown().await?;
    worker.await??;
    Ok(())
}

#[tokio::test]
async fn native_control_handlers_keep_memory_corrections_and_thread_settings_scoped()
-> color_eyre::Result<()> {
    let (mut app, _events, _) = crate::app::tests::make_test_app_with_channels().await;
    let home = tempfile::tempdir()?;
    app.config.codex_home = home.path().to_path_buf().abs();
    app.config.sqlite = codex_state::SqliteConfig::new_for_testing(home.path().abs());
    app.config.features.enable(Feature::MemoryTool)?;
    app.config.memories.scope = codex_config::types::MemoryScope::Global;
    app.config.memories.use_memories = false;
    app.config.memories.generate_memories = false;
    std::fs::write(
        home.path().join("config.toml"),
        "[features]\nmemories = true\n[memories]\nscope = 'global'\nuse_memories = false\ngenerate_memories = false\n",
    )?;
    let mut server = crate::start_embedded_app_server_for_picker(&app.config).await?;
    let started = server.start_thread(&app.config).await?;
    let thread = started.session.thread_id;
    app.enqueue_primary_thread_session(started.session, started.turns)
        .await?;
    server.thread_inject_items(thread, vec![serde_json::from_value(json!({"type":"message","role":"assistant","content":[{"type":"output_text","text":"isolated control fixture"}]}))?]).await?;
    let stored = app
        .session_control_command(&mut server, thread, Command::MemoryView)
        .await
        .map_err(|error| color_eyre::eyre::eyre!(error))?;
    let root = std::path::PathBuf::from(
        stored["memory"]["root"]
            .as_str()
            .expect("local memory root"),
    );
    assert!(
        root.starts_with(home.path()),
        "시험 메모리는 임시 홈 안에 있어야 함"
    );
    std::fs::create_dir_all(&root)?;
    std::fs::write(root.join("memory_summary.md"), "기존 메모리 요약\n")?;
    for command in [
        Command::MemoryNote {
            note: "정정할 내용".into(),
        },
        Command::MemoryForget {
            note: "이전 선호".into(),
        },
    ] {
        let result = app
            .session_control_command(&mut server, thread, command)
            .await;
        assert!(result.is_ok(), "{result:?}");
    }
    assert_eq!(
        std::fs::read_to_string(root.join("memory_summary.md"))?,
        "기존 메모리 요약\n"
    );
    let notes = std::fs::read_dir(root.join("extensions/ad_hoc/notes"))?
        .map(|entry| std::fs::read_to_string(entry.unwrap().path()).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(notes.len(), 2);
    assert!(
        notes
            .iter()
            .any(|note| note.contains("Explicit user forget request: 이전 선호"))
    );
    let defaults = std::fs::read(home.path().join("config.toml"))?;
    for command in [
        Command::ContextSet { tokens: 120000 },
        Command::ContextGet,
        Command::ContextReset,
        Command::ServiceTier {
            tier: "default".into(),
        },
        Command::ReloadPlugins,
    ] {
        let result = app
            .session_control_command(&mut server, thread, command)
            .await;
        assert!(result.is_ok(), "{result:?}");
    }
    assert_eq!(
        std::fs::read(home.path().join("config.toml"))?,
        defaults,
        "문맥·속도·다시 읽기는 전역 기본값을 쓰지 않음"
    );
    server.shutdown().await?;
    Ok(())
}

async fn control_server(
    response: Value,
) -> color_eyre::Result<(
    AppServerSession,
    tokio::sync::mpsc::UnboundedReceiver<(String, Value)>,
    tokio::task::JoinHandle<color_eyre::Result<()>>,
)> {
    use codex_app_server_protocol::JSONRPCMessage;
    use futures::{SinkExt, StreamExt};
    use tokio_tungstenite::tungstenite::Message;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let endpoint = crate::resolve_remote_addr(&format!("ws://{}", listener.local_addr()?))?;
    let (observed, requests) = tokio::sync::mpsc::unbounded_channel();
    let worker = tokio::spawn(async move {
        let (stream, _) = listener.accept().await?;
        let mut socket = tokio_tungstenite::accept_async(stream).await?;
        while let Some(frame) = socket.next().await {
            let Message::Text(text) = frame? else {
                continue;
            };
            let JSONRPCMessage::Request(request) = serde_json::from_str(&text)? else {
                continue;
            };
            let result = if request.method == "initialize" {
                json!({"userAgent":"control-test","codexHome":std::env::temp_dir(),"platformFamily":std::env::consts::FAMILY,"platformOs":std::env::consts::OS})
            } else {
                observed.send((request.method, request.params.unwrap_or_default()))?;
                response.clone()
            };
            socket
                .send(Message::Text(
                    json!({"id":request.id,"result":result}).to_string().into(),
                ))
                .await?;
        }
        Ok(())
    });
    let server = AppServerSession::new(
        BackendId::new(),
        crate::connect_remote_app_server(endpoint).await?,
        crate::app_server_session::ThreadParamsMode::Remote,
    );
    Ok((server, requests, worker))
}

async fn next_request(
    requests: &mut tokio::sync::mpsc::UnboundedReceiver<(String, Value)>,
) -> (String, Value) {
    tokio::time::timeout(std::time::Duration::from_secs(5), requests.recv())
        .await
        .expect("제어 요청 관측 시간초과")
        .expect("제어 요청 관측 채널 종료")
}

#[tokio::test]
async fn control_preview_never_deletes_and_queue_only_settings_do_not_report_success()
-> color_eyre::Result<()> {
    let (mut app, _events, _) = crate::app::tests::make_test_app_with_channels().await;
    let thread = ThreadId::new();
    let (mut server, mut requests, worker) = control_server(json!({"results":[]})).await?;
    let preview = app
        .session_control_command(&mut server, thread, Command::CleanupAgentsPreview)
        .await
        .map_err(|error| color_eyre::eyre::eyre!(error))?;
    assert_eq!(preview["deleted"], false);
    let (method, params) = next_request(&mut requests).await;
    assert_eq!(method, "thread/cleanupCompleted");
    assert_eq!(params["confirmed"], false);
    assert_eq!(params["currentThreadId"], thread.to_string());
    assert_eq!(params["parentThreadId"], thread.to_string());
    assert_eq!(params["selection"], json!([]));
    assert!(requests.try_recv().is_err());
    server.shutdown().await?;
    worker.await??;

    let before = app.chat_widget.configured_service_tier();
    let (mut server, mut requests, worker) = control_server(json!({})).await?;
    for command in [
        Command::ContextSet { tokens: 120000 },
        Command::ServiceTier {
            tier: "default".into(),
        },
    ] {
        assert!(
            app.session_control_command(&mut server, thread, command)
                .await
                .is_err()
        );
        let (method, params) = next_request(&mut requests).await;
        assert_eq!(method, "thread/settings/update");
        assert_eq!(params["threadId"], thread.to_string());
        assert_eq!(app.chat_widget.configured_service_tier(), before);
    }
    server.shutdown().await?;
    worker.await??;
    Ok(())
}
