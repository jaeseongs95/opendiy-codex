// Modified for OpenDIY Codex R31 (2026-11-01); see MODIFICATIONS.md.
//! Model-facing transport into the TUI-owned, CLI-lifetime scheduler.
use crate::app_event::AppEvent;
use crate::app_event_sender::AppEventSender;
use crate::backend_id::BackendId;
use crate::dynamic_tools;
use crate::i18n::text;
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

pub(crate) fn is_owned_tool(name: &str) -> bool {
    matches!(name, NAME | crate::session_control::NAME)
}

pub(crate) type CallKey = (
    BackendId,
    uuid::Uuid,
    codex_protocol::ThreadId,
    String,
    String,
);

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

impl Request {
    pub(crate) fn key(&self, thread: codex_protocol::ThreadId) -> CallKey {
        (
            self.connection.backend,
            self.connection.id,
            thread,
            self.params.turn_id.clone(),
            self.params.call_id.clone(),
        )
    }
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
                return Err(text("The schedule must be an interval without whitespace or an RFC3339 timestamp with an offset.").into());
            }
            let prompt = args.prompt.filter(|prompt| !prompt.trim().is_empty())
                .ok_or(text("A prompt is required to add a cron job."))?;
            Ok(format!("add {flag} {value} -- {prompt}"))
        }
        "list" if args.every.is_none() && args.at.is_none() && args.prompt.is_none() && args.id.is_none() => Ok("list".into()),
        "pause" | "resume" | "remove" | "cancel" if args.every.is_none() && args.at.is_none() && args.prompt.is_none() => {
            let id = args.id.filter(|id| *id > 0).ok_or(text("A cron job id is required."))?;
            Ok(format!("{} {id}", args.action))
        }
        _ => Err(text("Use arguments for the selected action. add requires exactly one of every or at and a prompt; management actions require only an id.").into()),
    }
}

pub(crate) fn specs() -> Vec<DynamicToolSpec> {
    vec![DynamicToolSpec::Namespace(DynamicToolNamespaceSpec {
        name: dynamic_tools::NAMESPACE.into(),
        description: "현재 대화의 CLI 수명 예약과 사용자가 요청한 로컬 설정·기능을 제어합니다.".into(),
        tools: vec![DynamicToolNamespaceTool::Function(DynamicToolFunctionSpec {
            name: NAME.into(),
            description: "사용자가 CLI에서 자연어로 예약 등록·조회·중지·재개·삭제를 요청하면 이 도구를 사용하세요. '너한테 걸려있던 예약작업 다시 걸어줘', '예약 다시 켜줘' 같은 기존 예약 재개 요청은 먼저 list로 현재 대화의 예약과 id를 확인한 뒤 대상 id에 resume을 호출하세요. 여러 예약 중 대상이 불명확하면 목록을 바탕으로 질문하고, 임의로 하나나 전부를 재개하지 마세요. 기존 예약의 일정·prompt를 다시 요구하거나 add로 중복 등록하지 마세요. 목록이 비었거나 제거·CLI 종료로 예약이 사라졌다면 재개할 수 없다고 알리고, 임의 복원하거나 성공했다고 말하지 마세요. 새 등록에 필요한 일정·prompt가 확인된 경우에만 add를 사용하세요. '5분마다 확인해줘', 'cron 걸어줘', '반복 실행해줘'는 새 등록 요청의 예입니다. 사용자가 CLI 예약이라고 정정하면 그 해석을 유지하세요. ChatGPT Tasks·Windows 작업 스케줄러·시스템 crontab·데스크톱 heartbeat로 임의 해석하거나 웹검색·다른 스케줄러 등록으로 대체하지 마세요. /cron을 직접 입력하게 떠넘기지 마세요. add는 every(예: 10m, 30s, 2h) 또는 at(offset 포함 RFC3339) 하나와 prompt가 필요하며, 새 등록의 일정이나 실행할 일이 불명확하면 먼저 질문하세요. list와 pause/resume/remove/cancel(id)을 지원합니다. 실제 성공 응답 전 등록·재개·변경됐다고 말하지 마세요. 시간초과·응답 유실은 결과 미확인입니다. 자동 재전송·재등록하지 말고 list로 확인하세요. 현재 호출 대화만 대상이며 CLI 종료 시 사라집니다. 바쁜 회차는 건너뜁니다. remove는 실행 중 turn을 취소하지 않습니다.".into(),
            input_schema: json!({"type":"object", "additionalProperties":false,
                "properties": {
                    "action":{"type":"string","enum":["add","list","pause","resume","remove","cancel"]},
                    "every":{"type":"string","description":"양의 정수와 s, m, h 단위"},
                    "at":{"type":"string","description":"미래 시각, offset 포함 RFC3339"},
                    "prompt":{"type":"string","minLength":1},
                    "id":{"type":"integer","minimum":1}
                }, "required":["action"]}),
            defer_loading: false,
        }), DynamicToolNamespaceTool::Function(crate::session_control::spec())],
    })]
}

/// Exact R28/R30 cron definition. Only this known predecessor may be replaced.
pub(crate) fn previous_specs() -> Vec<DynamicToolSpec> {
    let mut previous = specs();
    let DynamicToolSpec::Namespace(namespace) = &mut previous[0] else {
        unreachable!()
    };
    namespace.tools.retain(|tool| {
        let DynamicToolNamespaceTool::Function(tool) = tool;
        tool.name == NAME
    });
    let DynamicToolNamespaceTool::Function(tool) = &mut namespace.tools[0];
    tool.description = "사용자가 CLI에서 자연어로 '5분마다 확인해줘', 'cron 걸어줘', '반복 실행해줘'처럼 예약·주기적 확인을 요청하면 이 도구로 등록하세요. /cron을 직접 입력하게 떠넘기지 마세요. 이 도구는 Windows 작업 스케줄러·시스템 crontab·데스크톱 heartbeat와 별개입니다. CLI 예약 요청을 다른 스케줄러 등록으로 대체하지 마세요. 일정이나 실행할 일이 불명확하면 먼저 질문하세요. add는 every(예: 10m, 30s, 2h) 또는 at(offset 포함 RFC3339) 하나와 prompt가 필요합니다. list와 pause/resume/remove/cancel(id)을 지원합니다. 실제 성공 응답 전 등록됐다고 말하지 마세요. 시간초과·응답 유실은 결과 미확인입니다. 재등록하지 말고 list로 확인하세요. 현재 호출 대화만 대상이며 CLI 종료 시 사라집니다. 바쁜 회차는 건너뜁니다. remove는 실행 중 turn을 취소하지 않습니다.".into();
    previous
}

pub(crate) async fn execute(
    connection: Connection,
    handle: codex_app_server_client::AppServerRequestHandle,
    params: DynamicToolCallParams,
    events: &AppEventSender,
    cancellation: CancellationToken,
) -> DynamicToolCallResponse {
    let parsed = if params.tool == crate::session_control::NAME {
        crate::session_control::command(params.arguments.clone())
    } else if params.tool == NAME {
        command(params.arguments.clone())
    } else {
        Err(text("This is not a local control tool for this connection.").into())
    };
    let command = match parsed {
        Ok(command) => command,
        Err(error) => return dynamic_tools::failure_response(error),
    };
    if params.thread_id.is_empty() || params.turn_id.is_empty() || params.call_id.is_empty() {
        return dynamic_tools::failure_response(text(
            "The actual calling thread, turn, and call are required.",
        ));
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
            return dynamic_tools::failure_response(text(
                "This thread cannot use local control tools.",
            ));
        }
        Ok(Err(error)) => {
            return dynamic_tools::failure_response(format!(
                "{} {error}",
                text("Calling thread verification failed:")
            ));
        }
        Err(_) => {
            return dynamic_tools::failure_response(text(
                "Calling thread verification timed out: no cron request was sent.",
            ));
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
        _ => dynamic_tools::failure_response(text(
            "Local control outcome unknown: do not retry automatically. Use list for cron jobs or the relevant query for other settings.",
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cron_argument_errors_follow_locale_but_commands_and_specs_do_not() {
        use crate::i18n::{Locale, with_locale};
        let english_specs = with_locale(Locale::English, || serde_json::to_value(specs()).unwrap());
        let korean_specs = with_locale(Locale::Korean, || serde_json::to_value(specs()).unwrap());
        assert_eq!(english_specs, korean_specs);
        for locale in [Locale::English, Locale::Korean] {
            with_locale(locale, || {
                let korean = locale == Locale::Korean;
                let prompt =
                    "대기 -- original {id} /cron list pause resume remove delete cancel at every";
                assert_eq!(
                    command(json!({"action":"add","every":"10m","prompt":prompt})).unwrap(),
                    format!("add --every 10m -- {prompt}")
                );
                let at = "2099-01-01T00:00:00+09:00";
                assert_eq!(
                    command(json!({"action":"add","at":at,"prompt":prompt})).unwrap(),
                    format!("add --at {at} -- {prompt}")
                );
                assert_eq!(command(json!({"action":"list"})).unwrap(), "list");
                for action in ["pause", "resume", "remove", "cancel"] {
                    assert_eq!(
                        command(json!({"action":action,"id":42})).unwrap(),
                        format!("{action} 42")
                    );
                }
                for (arguments, expected) in [
                    (
                        json!({"action":"add","every":"1m -- injected","prompt":"unchanged"}),
                        if korean {
                            "일정은 공백 없는 간격 또는 offset이 포함된 RFC3339 시각이어야 합니다."
                        } else {
                            "The schedule must be an interval without whitespace or an RFC3339 timestamp with an offset."
                        },
                    ),
                    (
                        json!({"action":"add","every":"1m"}),
                        if korean {
                            "등록에는 실행할 prompt가 필요합니다."
                        } else {
                            "A prompt is required to add a cron job."
                        },
                    ),
                    (
                        json!({"action":"pause","id":0}),
                        if korean {
                            "관리할 예약 id가 필요합니다."
                        } else {
                            "A cron job id is required."
                        },
                    ),
                    (
                        json!({"action":"add","every":"1m","at":at,"prompt":"unchanged"}),
                        if korean {
                            "action에 맞는 인자를 사용하세요. add에는 every 또는 at 하나와 prompt, 관리에는 id만 필요합니다."
                        } else {
                            "Use arguments for the selected action. add requires exactly one of every or at and a prompt; management actions require only an id."
                        },
                    ),
                ] {
                    assert_eq!(command(arguments).unwrap_err(), expected);
                }
            });
        }
    }

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
