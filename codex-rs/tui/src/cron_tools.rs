// Modified for OpenDIY Codex R29 (2026-10-30); see MODIFICATIONS.md.
//! Model-facing transport into the TUI-owned, CLI-lifetime scheduler.
use crate::app_event::AppEvent;
use crate::app_event_sender::AppEventSender;
use crate::backend_id::BackendId;
use crate::dynamic_tools;
use codex_app_server_protocol::{
    DynamicToolCallParams, DynamicToolCallResponse, DynamicToolFunctionSpec,
    DynamicToolNamespaceSpec, DynamicToolNamespaceTool, DynamicToolSpec,
};
use serde::Deserialize;
use serde_json::{Value, json};
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU8, Ordering},
};
use tokio::sync::oneshot;
use tokio_util::sync::CancellationToken;

pub(crate) const NAME: &str = "cron";

#[derive(Clone, Debug)]
pub(crate) struct Connection {
    pub(crate) backend: BackendId,
    pub(crate) id: uuid::Uuid,
    pub(crate) live: Arc<AtomicBool>,
}

#[derive(Debug)]
pub(crate) struct Request {
    pub(crate) connection: Connection,
    pub(crate) params: DynamicToolCallParams,
    pub(crate) command: String,
    pub(crate) claim: Arc<AtomicU8>,
    pub(crate) cancellation: CancellationToken,
    pub(crate) reply: oneshot::Sender<DynamicToolCallResponse>,
}

struct PendingCall(Arc<AtomicU8>);
impl Drop for PendingCall {
    fn drop(&mut self) {
        let _ = self
            .0
            .compare_exchange(0, 2, Ordering::AcqRel, Ordering::Acquire);
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Arguments {
    action: String,
    every: Option<String>,
    at: Option<String>,
    prompt: Option<String>,
    id: Option<u64>,
}

fn command(value: Value) -> Result<String, String> {
    let args: Arguments = serde_json::from_value(value).map_err(|error| error.to_string())?;
    match args.action.as_str() {
        "add" if args.id.is_none() && args.every.is_some() != args.at.is_some() => {
            let (flag, value) = match (args.every, args.at) {
                (Some(value), None) => ("--every", value),
                (None, Some(value)) => ("--at", value),
                _ => unreachable!(),
            };
            if value.is_empty() || value.chars().any(char::is_whitespace) {
                return Err("일정은 공백 없는 간격 또는 offset이 포함된 RFC3339 시각이어야 합니다.".into());
            }
            let prompt = args.prompt.filter(|prompt| !prompt.trim().is_empty())
                .ok_or("등록에는 실행할 prompt가 필요합니다.")?;
            Ok(format!("add {flag} {value} -- {prompt}"))
        }
        "list" if args.every.is_none() && args.at.is_none() && args.prompt.is_none() && args.id.is_none() => Ok("list".into()),
        "pause" | "resume" | "remove" | "cancel" if args.every.is_none() && args.at.is_none() && args.prompt.is_none() => {
            let id = args.id.filter(|id| *id > 0).ok_or("관리할 예약 id가 필요합니다.")?;
            Ok(format!("{} {id}", args.action))
        }
        _ => Err("action에 맞는 인자를 사용하세요. add에는 every 또는 at 하나와 prompt, 관리에는 id만 필요합니다.".into()),
    }
}

pub(crate) fn specs() -> Vec<DynamicToolSpec> {
    vec![DynamicToolSpec::Namespace(DynamicToolNamespaceSpec {
        name: dynamic_tools::NAMESPACE.into(),
        description: "현재 대화의 CLI 수명 예약을 관리합니다.".into(),
        tools: vec![DynamicToolNamespaceTool::Function(DynamicToolFunctionSpec {
            name: NAME.into(),
            description: "사용자가 예약·주기적 확인을 요청하면 호출하세요. 일정이나 실행할 일이 불명확하면 먼저 질문하세요. add는 every(예: 10m, 30s, 2h) 또는 at(offset 포함 RFC3339) 하나와 prompt가 필요합니다. list와 pause/resume/remove/cancel(id)을 지원합니다. 실제 성공 응답 전 등록됐다고 말하지 마세요. 시간초과·응답 유실은 결과 미확인입니다. 재등록하지 말고 list로 확인하세요. 현재 호출 대화만 대상이며 CLI 종료 시 사라집니다. 바쁜 회차는 건너뜁니다. remove는 실행 중 turn을 취소하지 않습니다.".into(),
            input_schema: json!({"type":"object", "additionalProperties":false,
                "properties": {
                    "action":{"type":"string","enum":["add","list","pause","resume","remove","cancel"]},
                    "every":{"type":"string","description":"양의 정수와 s, m, h 단위"},
                    "at":{"type":"string","description":"미래 시각, offset 포함 RFC3339"},
                    "prompt":{"type":"string","minLength":1},
                    "id":{"type":"integer","minimum":1}
                }, "required":["action"]}),
            defer_loading: false,
        })],
    })]
}

pub(crate) async fn execute(
    connection: Connection,
    handle: codex_app_server_client::AppServerRequestHandle,
    params: DynamicToolCallParams,
    events: &AppEventSender,
    cancellation: CancellationToken,
) -> DynamicToolCallResponse {
    let command = match command(params.arguments.clone()) {
        Ok(command) => command,
        Err(error) => return dynamic_tools::failure_response(error),
    };
    if params.thread_id.is_empty() || params.turn_id.is_empty() || params.call_id.is_empty() {
        return dynamic_tools::failure_response("실제 호출 대화·turn·call 정보가 필요합니다.");
    }
    // Verify source policy on the originating connection, never from visible UI state.
    let verified = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        handle.request_typed::<codex_app_server_protocol::ThreadReadResponse>(
            codex_app_server_protocol::ClientRequest::ThreadRead {
                request_id: codex_app_server_protocol::RequestId::String(
                    uuid::Uuid::new_v4().to_string(),
                ),
                params: codex_app_server_protocol::ThreadReadParams {
                    thread_id: params.thread_id.clone(),
                    include_turns: false,
                },
            },
        ),
    )
    .await;
    match verified {
        Ok(Ok(response))
            if response.thread.id == params.thread_id
                && !crate::app_server_session::thread_blocks_direct_input(&response.thread)
                && !matches!(
                    response.thread.source,
                    codex_app_server_protocol::SessionSource::SubAgent(_)
                ) => {}
        Ok(Ok(_)) => {
            return dynamic_tools::failure_response("이 대화는 예약 도구를 사용할 수 없습니다.");
        }
        Ok(Err(error)) => {
            return dynamic_tools::failure_response(format!("호출 대화 확인 실패: {error}"));
        }
        Err(_) => {
            return dynamic_tools::failure_response(
                "호출 대화 확인 시간초과: 예약 요청을 보내지 않았습니다.",
            );
        }
    }
    let (reply, result) = oneshot::channel();
    let pending = PendingCall(Arc::new(AtomicU8::new(0)));
    events.send(AppEvent::CronToolCall(Box::new(Request {
        connection,
        params,
        command,
        claim: Arc::clone(&pending.0),
        cancellation,
        reply,
    })));
    match tokio::time::timeout(std::time::Duration::from_secs(30), result).await {
        Ok(Ok(response)) => response,
        _ => dynamic_tools::failure_response(
            "예약 처리 결과 미확인: 자동 재전송하지 않습니다. 재등록하지 말고 list로 확인하세요.",
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn cron_execute_verifies_real_thread_read_policy_before_queueing()
    -> color_eyre::Result<()> {
        use codex_app_server_protocol::{JSONRPCMessage, SessionSource};
        use codex_protocol::ThreadId;
        use futures::{SinkExt, StreamExt};
        use tokio_tungstenite::tungstenite::Message;
        for case in ["wrong-id", "direct-input-denied", "subagent", "valid"] {
            let caller = ThreadId::new();
            let returned = if case == "wrong-id" {
                ThreadId::new()
            } else {
                caller
            };
            let source = if case == "subagent" {
                SessionSource::SubAgent(codex_protocol::protocol::SubAgentSource::Review)
            } else {
                SessionSource::Cli
            };
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
            let endpoint = crate::resolve_remote_addr(&format!("ws://{}", listener.local_addr()?))?;
            let reads = Arc::new(AtomicU8::new(0));
            let observed = Arc::clone(&reads);
            let server = tokio::spawn(async move {
                let (stream, _) = listener.accept().await?;
                let mut socket = tokio_tungstenite::accept_async(stream).await?;
                while let Some(frame) = socket.next().await {
                    let Message::Text(text) = frame? else {
                        continue;
                    };
                    let JSONRPCMessage::Request(request) = serde_json::from_str(&text)? else {
                        continue;
                    };
                    let result = match request.method.as_str() {
                        "initialize" => json!({
                            "userAgent":"cron-policy-test", "codexHome":std::env::temp_dir(),
                            "platformFamily":std::env::consts::FAMILY, "platformOs":std::env::consts::OS
                        }),
                        "thread/read" => {
                            let params: codex_app_server_protocol::ThreadReadParams =
                                serde_json::from_value(
                                    request.params.as_ref().expect("read params").clone(),
                                )?;
                            assert_eq!(params.thread_id, caller.to_string());
                            assert!(!params.include_turns);
                            observed.fetch_add(1, Ordering::AcqRel);
                            json!({"thread": {
                                "id":returned, "sessionId":returned, "preview":"caller", "ephemeral":false,
                                "modelProvider":"test-provider", "createdAt":1, "updatedAt":2,
                                "status":{"type":"idle"}, "cwd":std::env::temp_dir(), "cliVersion":"0.0.0",
                                "source":source, "canAcceptDirectInput":case != "direct-input-denied", "turns":[]
                            }})
                        }
                        method => panic!("unexpected policy fixture request: {method}"),
                    };
                    if request.method == "initialize" {
                        let _: codex_app_server_protocol::InitializeResponse =
                            serde_json::from_value(result.clone())?;
                    } else {
                        let _: codex_app_server_protocol::ThreadReadResponse =
                            serde_json::from_value(result.clone())?;
                    }
                    socket
                        .send(Message::Text(
                            json!({"id":request.id,"result":result}).to_string().into(),
                        ))
                        .await?;
                }
                Ok::<_, color_eyre::Report>(())
            });
            let session = crate::app_server_session::AppServerSession::new(
                BackendId::new(),
                crate::connect_remote_app_server(endpoint).await?,
                crate::app_server_session::ThreadParamsMode::Remote,
            );
            let (tx, mut events) = tokio::sync::mpsc::unbounded_channel();
            let sender = AppEventSender::new(tx);
            let connection = session.cron_tool_connection();
            let handle = session.request_handle();
            let event_sender = sender.clone();
            let pending = tokio::spawn(async move {
                execute(
                    connection,
                    handle,
                    DynamicToolCallParams {
                        thread_id: caller.to_string(),
                        turn_id: "caller-turn".into(),
                        call_id: case.into(),
                        namespace: Some(dynamic_tools::NAMESPACE.into()),
                        tool: NAME.into(),
                        arguments: json!({"action":"add","every":"1m","prompt":"권한 확인"}),
                    },
                    &event_sender,
                    CancellationToken::new(),
                )
                .await
            });
            if case == "valid" {
                let Some(AppEvent::CronToolCall(request)) =
                    tokio::time::timeout(std::time::Duration::from_secs(5), events.recv()).await?
                else {
                    panic!("valid source must reach the app queue")
                };
                assert_eq!(request.params.thread_id, caller.to_string());
                assert_eq!(request.command, "add --every 1m -- 권한 확인");
                let _ = request.reply.send(DynamicToolCallResponse {
                    success: true,
                    content_items: Vec::new(),
                });
            }
            let response =
                tokio::time::timeout(std::time::Duration::from_secs(5), pending).await??;
            assert_eq!(response.success, case == "valid", "{case}: {response:?}");
            assert_eq!(reads.load(Ordering::Acquire), 1);
            assert!(
                matches!(
                    events.try_recv(),
                    Err(tokio::sync::mpsc::error::TryRecvError::Empty)
                ),
                "{case}"
            );
            session.shutdown().await?;
            server.await??;
        }
        Ok(())
    }

    #[test]
    fn cron_arguments_reject_target_injection_and_ambiguous_schedules() {
        assert_eq!(
            command(json!({"action":"add","every":"10m","prompt":"확인 -- 그대로"})).unwrap(),
            "add --every 10m -- 확인 -- 그대로"
        );
        for value in [
            json!({"action":"add","every":"10m","at":"2030-01-01T00:00:00+09:00","prompt":"확인"}),
            json!({"action":"add","every":"10m -- 주입","prompt":"확인"}),
            json!({"action":"list","threadId":"다른 대화"}),
            json!({"action":"remove","id":0}),
            json!({"action":"list","prompt":"불필요"}),
        ] {
            assert!(command(value).is_err());
        }
    }
}
