// Modified for OpenDIY Codex R28 (2026-10-29); see MODIFICATIONS.md.
//! CLI-lifetime cron integration: immutable targets, normal protocol input, one loop deadline.
use super::*;
use crate::backend_id::BackendId;
use crate::cron::{Phase, Schedule, Scheduler, Submission};
use codex_app_server_protocol::{
    RequestId as AppServerRequestId, TurnInterruptParams, TurnInterruptResponse,
    TurnStartIfIdleParams, TurnStartIfIdleResponse,
};
use std::time::Instant as MonotonicInstant;

const MAX_MODEL_CALLS: usize = 1_024;

const USAGE: &str = "/cron add --every 10m -- <prompt> | add --at <RFC3339+offset> -- <prompt> | list | pause/resume/remove/cancel <id>";

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct CronTarget {
    backend: BackendId,
    epoch: u64,
    thread: ThreadId,
}

#[derive(Debug)]
pub(crate) enum CronResult {
    Response(TurnStartIfIdleResponse),
    Rejected(String),
    Unknown(String),
    Unsupported,
}

#[derive(Debug)]
pub(crate) struct CronReply {
    target: CronTarget,
    id: u64,
    attempt: u64,
    result: CronResult,
}

#[derive(Default)]
pub(crate) struct CronState {
    scheduler: Scheduler<CronTarget>,
    epochs: HashMap<BackendId, u64>,
    unsupported: HashSet<BackendId>,
    tasks: HashMap<(u64, u64), (BackendId, JoinHandle<()>)>,
    cancel_tasks: HashMap<u64, (BackendId, JoinHandle<()>)>,
    pub(super) tool_tasks:
        HashMap<(BackendId, AppServerRequestId), (ThreadId, CancellationToken, JoinHandle<()>)>,
    closed_threads: HashSet<(BackendId, ThreadId)>,
    model_calls: HashMap<
        (BackendId, uuid::Uuid, ThreadId, String, String),
        (String, codex_app_server_protocol::DynamicToolCallResponse),
    >,
}

impl Drop for CronState {
    fn drop(&mut self) {
        self.stop();
    }
}

impl CronState {
    pub(super) fn stop(&mut self) {
        let owners: HashSet<_> = self
            .scheduler
            .jobs
            .iter()
            .map(|job| job.target.backend)
            .collect();
        for owner in owners {
            *self.epochs.entry(owner).or_default() += 1;
        }
        self.unsupported.clear();
        // Pausing execution for reconnect does not end other live connections' turns.
        // Accepted call identities live until their turn or owning connection ends.
        for (_, (_, cancellation, task)) in self.tool_tasks.drain() {
            cancellation.cancel();
            task.abort();
        }
        for (_, (_, task)) in self.tasks.drain() {
            task.abort();
        }
        for (_, (_, task)) in self.cancel_tasks.drain() {
            task.abort();
        }
        self.scheduler.invalidate(
            |_| true,
            "실행기 중지: CLI 종료 또는 연결 교체, 자동 재전송 없음",
        );
    }

    pub(super) fn invalidate_backend(&mut self, backend: BackendId, reason: &str) {
        self.invalidate_schedules(backend, reason);
        self.model_calls
            .retain(|(owner, _, _, _, _), _| *owner != backend);
        self.tool_tasks
            .retain(|(owner, _), (_, cancellation, task)| {
                if *owner == backend {
                    cancellation.cancel();
                    task.abort();
                    false
                } else {
                    true
                }
            });
    }

    // Scheduler support can end while model calls on the same connection remain valid.
    fn invalidate_schedules(&mut self, backend: BackendId, reason: &str) {
        *self.epochs.entry(backend).or_default() += 1;
        self.unsupported.remove(&backend);
        self.scheduler
            .invalidate(|target| target.backend == backend, reason);
        self.cancel_tasks.retain(|_, (owner, task)| {
            if *owner == backend {
                task.abort();
                false
            } else {
                true
            }
        });
        self.tasks.retain(|_, (owner, task)| {
            if *owner == backend {
                task.abort();
                false
            } else {
                true
            }
        });
    }

    fn epoch(&self, backend: BackendId) -> u64 {
        self.epochs.get(&backend).copied().unwrap_or_default()
    }

    pub(super) fn holds_thread(&self, thread: ThreadId) -> bool {
        self.scheduler
            .jobs
            .iter()
            .any(|job| job.target.thread == thread && job.holds_target())
    }

    pub(super) fn next_deadline(&self) -> Option<tokio::time::Instant> {
        self.scheduler
            .next_deadline()
            .map(tokio::time::Instant::from_std)
    }
}

fn idle_method_unsupported(error: &codex_app_server_protocol::JSONRPCErrorError) -> bool {
    error.code == -32601
        || (error.code == -32600
            && error.message.starts_with(
                "Invalid request: unknown variant `turn/startIfIdle`, expected one of ",
            ))
}

#[derive(Debug)]
pub(crate) struct CronCancelReply {
    target: CronTarget,
    id: u64,
    turn_id: String,
    result: Result<(), String>,
}

fn parse_add(args: &str, now: MonotonicInstant) -> Result<(Schedule, String), String> {
    let (options, prompt) = args.split_once(" -- ").ok_or(USAGE)?;
    let options: Vec<_> = options.split_whitespace().collect();
    let schedule = match options.as_slice() {
        ["add", "--every", interval] => {
            let (number, suffix) =
                interval.split_at(interval.char_indices().next_back().ok_or(USAGE)?.0);
            let multiplier = match suffix {
                "s" => 1,
                "m" => 60,
                "h" => 3600,
                _ => return Err("간격 단위는 s, m, h입니다.".into()),
            };
            let seconds = number
                .parse::<u64>()
                .ok()
                .and_then(|value| value.checked_mul(multiplier))
                .filter(|value| *value > 0)
                .ok_or("간격은 0보다 큰 정수여야 합니다.")?;
            let interval = Duration::from_secs(seconds);
            now.checked_add(interval)
                .ok_or("예약 간격이 너무 큽니다.")?;
            Schedule::Every(interval)
        }
        ["add", "--at", timestamp] => {
            let at = chrono::DateTime::parse_from_rfc3339(timestamp)
                .map_err(|_| "시각은 offset이 포함된 RFC3339 형식이어야 합니다.")?;
            let delay = (at.with_timezone(&chrono::Utc) - chrono::Utc::now())
                .to_std()
                .map_err(|_| "예약 시각은 미래여야 합니다.")?;
            Schedule::Once(now.checked_add(delay).ok_or("예약 시각이 너무 멉니다.")?)
        }
        _ => return Err(USAGE.into()),
    };
    if prompt.trim().is_empty() {
        return Err("예약 프롬프트가 비어 있습니다.".into());
    }
    Ok((schedule, prompt.to_string()))
}

impl App {
    pub(super) async fn handle_cron_tool_call(
        &mut self,
        app_server: &AppServerSession,
        request: crate::cron_tools::Request,
    ) {
        use std::sync::atomic::Ordering;
        let fail = |message| crate::dynamic_tools::failure_response(message);
        let Ok(thread) = ThreadId::from_string(&request.params.thread_id) else {
            let _ = request
                .reply
                .send(fail("호출 대화 식별자가 올바르지 않습니다."));
            return;
        };
        if request.reply.is_closed() {
            return;
        }
        let current_connection = app_server.cron_tool_connection();
        let available = request.connection.backend == current_connection.backend
            && request.connection.id == current_connection.id
            && request.connection.live.load(Ordering::Acquire)
            && !self.reconnect.offline
            && !self
                .cron
                .closed_threads
                .contains(&(request.connection.backend, thread))
            && !self.abandoned_side_threads.contains(&thread)
            && !self.temporary_structured_requests.contains_key(&thread)
            && !self.thread_unavailable(thread)
            && self.active_turn_id_for_thread(thread).await.as_deref()
                == Some(request.params.turn_id.as_str());
        if !available {
            let _ = request.reply.send(fail("원래 호출 대화·활성 turn·연결이 종료되거나 바뀌었습니다. 예약을 변경하지 않았습니다."));
            return;
        }
        let key = (
            request.connection.backend,
            request.connection.id,
            thread,
            request.params.turn_id.clone(),
            request.params.call_id.clone(),
        );
        if let Some((command, response)) = self.cron.model_calls.get(&key) {
            let response = if command == &request.command {
                response.clone()
            } else {
                fail("동일 호출 식별자에 다른 요청을 사용할 수 없습니다.")
            };
            let _ = request.reply.send(response);
            return;
        }
        if self.cron.model_calls.len() >= MAX_MODEL_CALLS {
            let _ = request.reply.send(fail("동시 활성 호출 기록 한도에 도달했습니다. 이전 요청을 다시 등록하지 말고 turn 종료 후 확인하세요."));
            return;
        }
        // Cancellation before this claim wins. After it, a lost reply is an unknown
        // outcome, not permission to replay the same mutation.
        if request.reply.is_closed()
            || request.cancellation.is_cancelled()
            || request
                .claim
                .compare_exchange(0, 1, Ordering::AcqRel, Ordering::Acquire)
                .is_err()
            || request.cancellation.is_cancelled()
            || !request.connection.live.load(Ordering::Acquire)
        {
            return;
        }
        let response = match self
            .cron_command_scoped(app_server, Some(thread), &request.command, true)
            .await
        {
            Ok(message) => codex_app_server_protocol::DynamicToolCallResponse {
                success: true,
                content_items: vec![
                    codex_app_server_protocol::DynamicToolCallOutputContentItem::InputText {
                        text: message,
                    },
                ],
            },
            Err(error) => crate::dynamic_tools::failure_response(error),
        };
        self.cron
            .model_calls
            .insert(key, (request.command, response.clone()));
        let _ = request.reply.send(response);
    }

    pub(super) async fn handle_cron_command(
        &mut self,
        app_server: &AppServerSession,
        thread: Option<ThreadId>,
        args: String,
    ) {
        let outcome = self.cron_command(app_server, thread, &args).await;
        match outcome {
            Ok(message) => self.chat_widget.add_info_message(message, None),
            Err(error) => self.chat_widget.add_error_message(error),
        }
    }

    async fn cron_command(
        &mut self,
        app_server: &AppServerSession,
        thread: Option<ThreadId>,
        args: &str,
    ) -> Result<String, String> {
        self.cron_command_scoped(app_server, thread, args, false)
            .await
    }

    async fn cron_command_scoped(
        &mut self,
        app_server: &AppServerSession,
        thread: Option<ThreadId>,
        args: &str,
        model_scoped: bool,
    ) -> Result<String, String> {
        let now = MonotonicInstant::now();
        let words: Vec<_> = args.split_whitespace().collect();
        match words.as_slice() {
            [] | ["list"] => {
                if self.cron.scheduler.jobs.iter().all(|job| {
                    model_scoped
                        && (job.target.backend != app_server.backend_id()
                            || Some(job.target.thread) != thread)
                }) {
                    return Ok("등록된 예약이 없습니다. CLI를 종료하면 예약도 사라집니다.".into());
                }
                Ok(self
                    .cron
                    .scheduler
                    .jobs
                    .iter()
                    .filter(|job| {
                        !model_scoped
                            || (job.target.backend == app_server.backend_id()
                                && Some(job.target.thread) == thread)
                    })
                    .map(|job| {
                        let state = match &job.phase {
                            Phase::Ready => "대기".to_string(),
                            Phase::Pending { .. } => "입장 응답 대기".to_string(),
                            Phase::Running {
                                turn_id,
                                cancelling,
                            } => format!(
                                "{} turn={turn_id}",
                                if *cancelling {
                                    "취소 요청·종료 대기"
                                } else {
                                    "실행 중"
                                }
                            ),
                            Phase::Finished(reason) => reason.clone(),
                            Phase::BlockedConnection(reason) => {
                                format!("연결 변경·실행 차단: {reason}")
                            }
                            Phase::BlockedUnsupported(reason) => {
                                format!("미지원·실행 차단: {reason}")
                            }
                            Phase::Unknown(reason) => format!("미확인·실행 중지: {reason}"),
                        };
                        let schedule = job
                            .interval
                            .map(|interval| format!("every={}s", interval.as_secs()))
                            .unwrap_or_else(|| "일회성".into());
                        let next = job
                            .next_due
                            .map(|deadline| {
                                format!("{}s", deadline.saturating_duration_since(now).as_secs())
                            })
                            .unwrap_or_else(|| "없음".into());
                        let mut prompt = String::new();
                        for ch in serde_json::json!(job.prompt).to_string().chars() {
                            if ch.is_control() {
                                prompt.push_str(&format!("\\u{:04x}", u32::from(ch)));
                            } else {
                                prompt.push(ch);
                            }
                        }
                        format!(
                            "#{} thread={} {} next={} {}{}{} skipped={} prompt={}",
                            job.id,
                            job.target.thread,
                            schedule,
                            next,
                            state,
                            if job.paused { " (일시중지)" } else { "" },
                            if job.removed {
                                " (예약 제거됨; 실행 소유권 유지)"
                            } else {
                                ""
                            },
                            job.skipped,
                            prompt
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("\n"))
            }
            ["add", ..] => {
                let thread = thread.ok_or("등록 대상 대화가 없습니다.")?;
                if (!model_scoped && self.current_displayed_thread_id() != Some(thread))
                    || self.thread_unavailable(thread)
                {
                    return Err("등록 대상 대화가 바뀌었거나 사용할 수 없습니다.".into());
                }
                if !model_scoped && self.chat_widget.has_misalignment_policy_violation() {
                    return Err("현재 대화는 직접 입력을 허용하지 않습니다.".into());
                }
                if self.cron.unsupported.contains(&app_server.backend_id()) {
                    return Err("이 backend는 turn/startIfIdle을 지원하지 않아 예약이 비활성화되어 있습니다.".into());
                }
                if self.reconnect.offline {
                    return Err("backend 연결이 끊겨 예약을 등록할 수 없습니다.".into());
                }
                let (schedule, prompt) = parse_add(args, now)?;
                let target = CronTarget {
                    backend: app_server.backend_id(),
                    epoch: self.cron.epoch(app_server.backend_id()),
                    thread,
                };
                let id = self.cron.scheduler.add(target, prompt, schedule, now)?;
                Ok(format!(
                    "예약 #{id} 등록: thread={thread}. busy 회차는 건너뜁니다. backend 지원은 첫 전용 요청에서 확인합니다."
                ))
            }
            [action @ ("pause" | "resume" | "remove" | "cancel"), id] => {
                let id = id.parse::<u64>().map_err(|_| USAGE)?;
                let job = self.cron.scheduler.job_mut(id)?;
                if job.target.backend != app_server.backend_id()
                    || (model_scoped && Some(job.target.thread) != thread)
                {
                    return Err("예약이 다른 backend에 속합니다.".into());
                }
                let target = job.target.clone();
                match *action {
                    "pause" => self.cron.scheduler.pause(id)?,
                    "resume" => {
                        if target.epoch != self.cron.epoch(target.backend) {
                            return Err("등록 당시 backend 연결 세대가 끝났습니다. 실행 상태를 확인한 뒤 새로 등록하세요.".into());
                        }
                        if self.reconnect.offline {
                            return Err("backend 연결이 끊겨 예약을 재개할 수 없습니다.".into());
                        }
                        self.cron.scheduler.resume(id, now)?;
                    }
                    "remove" => {
                        self.cron.scheduler.remove(id)?;
                        self.release_cron_thread_if_unused(app_server, target.thread)
                            .await;
                    }
                    "cancel" => {
                        let job = self.cron.scheduler.job_mut(id)?;
                        let Phase::Running {
                            turn_id,
                            cancelling,
                        } = &mut job.phase
                        else {
                            return Err(
                                "정확한 소유 turn이 확인된 실행만 취소할 수 있습니다.".into()
                            );
                        };
                        if *cancelling {
                            return Err("이미 이 turn의 취소 종료를 기다리고 있습니다.".into());
                        }
                        // Never substitute active_turn_id: it may belong to manual input.
                        let params = TurnInterruptParams {
                            thread_id: target.thread.to_string(),
                            turn_id: turn_id.clone(),
                        };
                        *cancelling = true;
                        let turn_id = params.turn_id.clone();
                        let sender = self.app_event_tx.clone();
                        let request = app_server.request_handle();
                        let owner = target.backend;
                        let task = tokio::spawn(async move {
                            let result = tokio::time::timeout(
                                Duration::from_secs(30),
                                request.request_typed::<TurnInterruptResponse>(
                                    ClientRequest::TurnInterrupt {
                                        request_id: AppServerRequestId::String(
                                            uuid::Uuid::new_v4().to_string(),
                                        ),
                                        params,
                                    },
                                ),
                            )
                            .await
                            .map_err(|_| "취소 응답 timeout: 종료 여부 미확인".to_string())
                            .and_then(|result| {
                                result
                                    .map(|_| ())
                                    .map_err(|error| format!("취소 결과 미확인: {error}"))
                            });
                            sender.send(AppEvent::CronCancelReply(Box::new(CronCancelReply {
                                target,
                                id,
                                turn_id,
                                result,
                            })));
                        });
                        self.cron.cancel_tasks.insert(id, (owner, task));
                    }
                    _ => unreachable!(),
                }
                Ok(format!(
                    "예약 #{id}: {action}. remove는 이미 수락한 turn을 취소하지 않습니다."
                ))
            }
            _ => Err(USAGE.into()),
        }
    }

    pub(super) fn dispatch_due_cron(&mut self, backends: &backend_sessions::BackendSessions) {
        for dispatch in self.cron.scheduler.due(MonotonicInstant::now()) {
            let target = dispatch.target;
            let Some(session) = backends
                .session(target.backend)
                .filter(|_| backends.is_listening(target.backend))
            else {
                self.cron.scheduler.submitted(
                    dispatch.id,
                    dispatch.attempt,
                    Submission::Unknown("backend 연결이 없습니다.".into()),
                );
                continue;
            };
            if self.cron.epoch(target.backend) != target.epoch
                || self.cron.unsupported.contains(&target.backend)
            {
                self.cron.scheduler.submitted(
                    dispatch.id,
                    dispatch.attempt,
                    Submission::Unknown("backend 세대 또는 지원 상태가 바뀌었습니다.".into()),
                );
                continue;
            }
            let request = session.request_handle();
            let sender = self.app_event_tx.clone();
            let id = dispatch.id;
            let attempt = dispatch.attempt;
            let owner = target.backend;
            let task = tokio::spawn(async move {
                let response = tokio::time::timeout(
                    Duration::from_secs(30),
                    request.request_typed::<TurnStartIfIdleResponse>(
                        ClientRequest::TurnStartIfIdle {
                            request_id: AppServerRequestId::String(
                                uuid::Uuid::new_v4().to_string(),
                            ),
                            params: TurnStartIfIdleParams {
                                thread_id: target.thread.to_string(),
                                input: vec![codex_app_server_protocol::UserInput::Text {
                                    text: dispatch.prompt,
                                    text_elements: vec![],
                                }],
                            },
                        },
                    ),
                )
                .await;
                let result = match response {
                    Ok(Ok(response)) => CronResult::Response(response),
                    Ok(Err(TypedRequestError::Server { source, .. }))
                        if idle_method_unsupported(&source) =>
                    {
                        CronResult::Unsupported
                    }
                    Ok(Err(TypedRequestError::Server { source, .. }))
                        if source.code == -32600 || source.code == -32602 =>
                    {
                        CronResult::Rejected(source.message)
                    }
                    Ok(Err(error)) => CronResult::Unknown(error.to_string()),
                    Err(_) => CronResult::Unknown(
                        "입장 응답 timeout: 실행 여부 미확인, 재전송 없음".into(),
                    ),
                };
                sender.send(AppEvent::CronReply(Box::new(CronReply {
                    target,
                    id,
                    attempt,
                    result,
                })));
            });
            self.cron.tasks.insert((id, attempt), (owner, task));
        }
    }

    pub(super) async fn handle_cron_reply(
        &mut self,
        app_server: &AppServerSession,
        reply: CronReply,
    ) {
        self.cron.tasks.remove(&(reply.id, reply.attempt));
        if self.cron.epoch(reply.target.backend) != reply.target.epoch {
            return;
        }
        let notice = match &reply.result {
            CronResult::Unknown(reason) => Some(format!(
                "예약 #{} 실행 여부 미확인: {reason}. 자동 재전송을 중지했습니다.",
                reply.id
            )),
            CronResult::Rejected(reason)
            | CronResult::Response(TurnStartIfIdleResponse::NotStarted { reason }) => Some(
                format!("예약 #{} 미입장: {reason}. 일시중지했습니다.", reply.id),
            ),
            _ => None,
        };
        let submission = match reply.result {
            CronResult::Response(TurnStartIfIdleResponse::Started { turn }) => {
                Submission::Started(turn.id)
            }
            CronResult::Response(TurnStartIfIdleResponse::SkippedBusy) => Submission::Busy,
            CronResult::Response(TurnStartIfIdleResponse::NotStarted { reason })
            | CronResult::Rejected(reason) => Submission::NotStarted(reason),
            CronResult::Unknown(reason) => Submission::Unknown(reason),
            CronResult::Unsupported => {
                let threads: HashSet<_> = self
                    .cron
                    .scheduler
                    .jobs
                    .iter()
                    .filter(|job| job.target.backend == reply.target.backend)
                    .map(|job| job.target.thread)
                    .collect();
                self.cron
                    .invalidate_schedules(reply.target.backend, "turn/startIfIdle 미지원");
                self.cron.unsupported.insert(reply.target.backend);
                self.cron
                    .scheduler
                    .block_unsupported(|target| target.backend == reply.target.backend);
                self.chat_widget.add_error_message(
                    "이 backend는 안전한 예약 실행을 지원하지 않습니다. 예약을 비활성화했습니다."
                        .into(),
                );
                if app_server.backend_id() == reply.target.backend {
                    for thread in threads {
                        self.release_cron_thread_if_unused(app_server, thread).await;
                    }
                }
                return;
            }
        };
        if self
            .cron
            .scheduler
            .submitted(reply.id, reply.attempt, submission)
            && app_server.backend_id() == reply.target.backend
        {
            if let Some(notice) = notice {
                self.chat_widget.add_error_message(notice);
            }
            self.release_cron_thread_if_unused(app_server, reply.target.thread)
                .await;
        }
    }

    pub(super) fn handle_cron_cancel_reply(&mut self, reply: CronCancelReply) {
        self.cron.cancel_tasks.remove(&reply.id);
        if self.cron.epoch(reply.target.backend) != reply.target.epoch {
            return;
        }
        if let Ok(job) = self.cron.scheduler.job_mut(reply.id)
            && job.target == reply.target
            && matches!(&job.phase, Phase::Running { turn_id, cancelling: true } if turn_id == &reply.turn_id)
            && let Err(error) = reply.result
        {
            self.chat_widget.add_error_message(error);
        }
        // A successful interrupt reply is not turn completion. Terminal notification owns cleanup.
    }

    pub(super) async fn note_cron_notification(
        &mut self,
        app_server: &AppServerSession,
        notification: &ServerNotification,
    ) {
        let closed_thread = match notification {
            ServerNotification::ThreadClosed(closed) => Some(closed.thread_id.as_str()),
            ServerNotification::ThreadArchived(archived) => Some(archived.thread_id.as_str()),
            ServerNotification::ThreadDeleted(deleted) => Some(deleted.thread_id.as_str()),
            _ => None,
        };
        if let Some(thread) = closed_thread.and_then(|id| ThreadId::from_string(id).ok()) {
            let owner = app_server.backend_id();
            self.cron.closed_threads.insert((owner, thread));
            self.cron.scheduler.invalidate(
                |target| target.backend == owner && target.thread == thread,
                "대화 종료·삭제·보관: 실행 상태를 자동 추정하지 않음",
            );
        }
        if let ServerNotification::TurnStarted(started) = notification
            && let Ok(thread) = ThreadId::from_string(&started.thread_id)
        {
            self.cron
                .closed_threads
                .remove(&(app_server.backend_id(), thread));
        }
        if let ServerNotification::TurnCompleted(completed) = notification {
            let thread = ThreadId::from_string(&completed.thread_id).ok();
            self.cron
                .model_calls
                .retain(|(backend, _, caller, turn, _), _| {
                    *backend != app_server.backend_id()
                        || Some(*caller) != thread
                        || turn != &completed.turn.id
                });
        }
        if let ServerNotification::TurnCompleted(completed) = notification
            && let Ok(thread) = ThreadId::from_string(&completed.thread_id)
            && self.cron.holds_thread(thread)
        {
            let target = CronTarget {
                backend: app_server.backend_id(),
                epoch: self.cron.epoch(app_server.backend_id()),
                thread,
            };
            self.cron.scheduler.terminal(&target, &completed.turn.id);
            self.release_cron_thread_if_unused(app_server, thread).await;
        }
    }

    async fn release_cron_thread_if_unused(
        &mut self,
        app_server: &AppServerSession,
        thread: ThreadId,
    ) {
        if self.cron.holds_thread(thread)
            || self.current_displayed_thread_id() == Some(thread)
            || self.voice_owner_thread_id() == Some(thread)
            || self.side_threads.contains_key(&thread)
            || self
                .agents_overview
                .dispatched_requests
                .contains_key(&thread)
            || self.agents_overview.blank_sessions.contains_key(&thread)
        {
            return;
        }
        // The cached transcript remains available, as in ordinary navigation detach.
        let request = app_server.request_handle();
        let result = tokio::time::timeout(
            Duration::from_secs(5),
            request.request_typed::<codex_app_server_protocol::ThreadUnsubscribeResponse>(
                ClientRequest::ThreadUnsubscribe {
                    request_id: AppServerRequestId::String(uuid::Uuid::new_v4().to_string()),
                    params: codex_app_server_protocol::ThreadUnsubscribeParams {
                        thread_id: thread.to_string(),
                    },
                },
            ),
        )
        .await;
        match result {
            Ok(Ok(_)) => {
                self.abort_thread_event_listener(thread);
                self.pending_server_profiles.remove(&thread);
            }
            Ok(Err(error)) => self
                .chat_widget
                .add_error_message(format!("예약 대화 구독 정리 실패: {error}")),
            Err(_) => self.chat_widget.add_error_message(
                "예약 대화 구독 정리 timeout: 해제 여부를 확인하지 못했습니다.".into(),
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn model_request(
        connection: crate::cron_tools::Connection,
        thread: ThreadId,
        call: &str,
        command: &str,
    ) -> (
        crate::cron_tools::Request,
        tokio::sync::oneshot::Receiver<codex_app_server_protocol::DynamicToolCallResponse>,
    ) {
        let (reply, result) = tokio::sync::oneshot::channel();
        (
            crate::cron_tools::Request {
                connection,
                params: codex_app_server_protocol::DynamicToolCallParams {
                    thread_id: thread.to_string(),
                    turn_id: "caller-turn".into(),
                    call_id: call.into(),
                    namespace: Some(crate::dynamic_tools::NAMESPACE.into()),
                    tool: crate::cron_tools::NAME.into(),
                    arguments: serde_json::json!({}),
                },
                command: command.into(),
                claim: Arc::new(std::sync::atomic::AtomicU8::new(0)),
                cancellation: CancellationToken::new(),
                reply,
            },
            result,
        )
    }

    #[tokio::test]
    async fn cron_list_identifies_same_interval_prompts_and_escapes_control_characters()
    -> color_eyre::Result<()> {
        let (mut app, _events, _ops) = crate::app::tests::make_test_app_with_channels().await;
        let (session, _observed, worker) =
            super::super::app_server_requests::retained_reply_tests::connection().await?;
        let caller = ThreadId::new();
        let other = ThreadId::new();
        let prompts = [
            format!(
                "첫 요청 \"인용\" \\ 경로\n다음 줄\t탭\0널\u{1b}ESC\u{7f}DEL\u{85}C1 {}",
                "긴 요청".repeat(1_500)
            ),
            "둘째 요청: 같은 주기, 다른 작업".to_string(),
            "다른 대화의 요청".to_string(),
        ];
        for thread in [caller, other] {
            app.ensure_thread_channel(thread)
                .store
                .lock()
                .await
                .set_active_turn_id("caller-turn".into());
        }
        for (thread, prompt) in [caller, caller, other].into_iter().zip(&prompts) {
            app.active_thread_id = Some(thread);
            app.cron_command(
                &session,
                Some(thread),
                &format!("add --every 1m -- {prompt}"),
            )
            .await
            .expect("valid manual scheduler fixture");
        }
        let manual = app
            .cron_command(&session, None, "list")
            .await
            .expect("manual list");
        assert_eq!(manual.lines().count(), 3);
        assert!(!manual.chars().any(|ch| ch.is_control() && ch != '\n'));
        let listed: Vec<String> = manual
            .lines()
            .map(|line| {
                assert!(line.contains("every=60s"));
                serde_json::from_str(line.split_once(" prompt=").expect("quoted prompt").1)
                    .expect("valid JSON prompt")
            })
            .collect();
        assert_eq!(listed, prompts);
        for (thread, expected) in [(caller, &prompts[..2]), (other, &prompts[2..])] {
            let (request, response) =
                model_request(session.cron_tool_connection(), thread, "list", "list");
            app.handle_cron_tool_call(&session, request).await;
            let response = response.await?;
            assert!(response.success);
            let [codex_app_server_protocol::DynamicToolCallOutputContentItem::InputText { text }] =
                response.content_items.as_slice()
            else {
                panic!("expected list text");
            };
            let listed: Vec<String> = text
                .lines()
                .map(|line| {
                    serde_json::from_str(line.split_once(" prompt=").expect("quoted prompt").1)
                        .expect("valid JSON prompt")
                })
                .collect();
            assert_eq!(listed, expected);
        }
        session.shutdown().await?;
        worker.await??;
        Ok(())
    }

    #[tokio::test]
    async fn model_cron_is_caller_scoped_and_same_call_does_not_register_twice()
    -> color_eyre::Result<()> {
        let (mut app, _events, _ops) = crate::app::tests::make_test_app_with_channels().await;
        let (session, _observed, worker) =
            super::super::app_server_requests::retained_reply_tests::connection().await?;
        let caller = ThreadId::new();
        let displayed = ThreadId::new();
        app.active_thread_id = Some(displayed);
        app.chat_widget
            .restore_user_message_to_composer("다른 대화 초안".into());
        app.ensure_thread_channel(caller)
            .store
            .lock()
            .await
            .set_active_turn_id("caller-turn".into());
        app.ensure_thread_channel(displayed)
            .store
            .lock()
            .await
            .set_active_turn_id("caller-turn".into());
        let connection = session.cron_tool_connection();
        for attempt in 0..2 {
            let (request, response) = model_request(
                connection.clone(),
                caller,
                "same-add",
                "add --every 10m -- 원래 대화 확인",
            );
            app.handle_cron_tool_call(&session, request).await;
            if attempt == 0 {
                // The mutation completed, but its consumer never receives the reply.
                drop(response);
                app.cron
                    .invalidate_backend(BackendId::new(), "다른 backend 종료");
            } else {
                assert!(response.await?.success);
            }
        }
        let (request, response) = model_request(
            connection.clone(),
            caller,
            "same-add",
            "add --every 2m -- 다른 내용",
        );
        app.handle_cron_tool_call(&session, request).await;
        assert!(!response.await?.success);
        assert_eq!(app.cron.scheduler.jobs.len(), 1);
        assert_eq!(app.cron.scheduler.jobs[0].target.thread, caller);
        let id = app.cron.scheduler.jobs[0].id;
        for command in ["list".to_string(), format!("pause {id}")] {
            let (request, response) =
                model_request(connection.clone(), displayed, &command, &command);
            app.handle_cron_tool_call(&session, request).await;
            let response = response.await?;
            if command == "list" {
                assert!(response.success);
                assert!(
                    matches!(&response.content_items[0], codex_app_server_protocol::DynamicToolCallOutputContentItem::InputText { text } if !text.contains(&format!("#{id}")))
                );
            } else {
                assert!(!response.success);
            }
        }
        for (action, paused) in [("pause", true), ("resume", false)] {
            let command = format!("{action} {id}");
            let (request, response) = model_request(connection.clone(), caller, action, &command);
            app.handle_cron_tool_call(&session, request).await;
            assert!(response.await?.success);
            assert_eq!(app.cron.scheduler.jobs[0].paused, paused);
        }
        assert_eq!(
            app.chat_widget.composer_text_with_pending(),
            "다른 대화 초안"
        );
        app.active_thread_id = Some(caller);
        let (request, response) = model_request(
            connection.clone(),
            caller,
            "remove",
            &format!("remove {id}"),
        );
        app.handle_cron_tool_call(&session, request).await;
        assert!(response.await?.success);
        assert!(app.cron.scheduler.jobs.is_empty());
        let (request, response) = model_request(
            connection.clone(),
            caller,
            "same-add",
            "add --every 10m -- 원래 대화 확인",
        );
        app.handle_cron_tool_call(&session, request).await;
        assert!(response.await?.success);
        assert!(
            app.cron.scheduler.jobs.is_empty(),
            "삭제한 예약을 같은 호출 재전송으로 되살리지 않음"
        );
        let other_caller = ThreadId::new();
        while app.cron.model_calls.len() < MAX_MODEL_CALLS {
            app.cron.model_calls.insert(
                (
                    connection.backend,
                    connection.id,
                    other_caller,
                    "other-turn".into(),
                    uuid::Uuid::new_v4().to_string(),
                ),
                (
                    "list".into(),
                    crate::dynamic_tools::failure_response("시험 기록"),
                ),
            );
        }
        let (request, response) = model_request(
            connection.clone(),
            caller,
            "over-limit",
            "add --every 1m -- 한도 초과",
        );
        app.handle_cron_tool_call(&session, request).await;
        assert!(!response.await?.success);
        assert!(app.cron.scheduler.jobs.is_empty());
        assert_eq!(app.cron.model_calls.len(), MAX_MODEL_CALLS);
        let (request, response) =
            model_request(connection.clone(), caller, "capacity-list", "list");
        app.handle_cron_tool_call(&session, request).await;
        assert!(
            !response.await?.success,
            "한도에서는 성공을 기록할 수 없는 신규 조회도 거부"
        );
        app.note_cron_notification(
            &session,
            &ServerNotification::TurnCompleted(
                codex_app_server_protocol::TurnCompletedNotification {
                    thread_id: other_caller.to_string(),
                    turn: codex_app_server_protocol::Turn {
                        id: "other-turn".into(),
                        root_turn_id: Some("other-turn".into()),
                        items: vec![],
                        items_view: codex_app_server_protocol::TurnItemsView::NotLoaded,
                        error: None,
                        status: codex_app_server_protocol::TurnStatus::Completed,
                        started_at: None,
                        completed_at: None,
                        duration_ms: None,
                    },
                },
            ),
        )
        .await;
        assert!(app.cron.model_calls.len() < MAX_MODEL_CALLS);
        // The rejected list never succeeded. A later explicit call can now be admitted,
        // and that successful payload remains bound to the key.
        let (request, response) = model_request(
            connection.clone(),
            caller,
            "capacity-list",
            "add --every 1m -- 명시적 새 요청",
        );
        app.handle_cron_tool_call(&session, request).await;
        assert!(response.await?.success);
        let (request, response) = model_request(connection, caller, "capacity-list", "list");
        app.handle_cron_tool_call(&session, request).await;
        assert!(!response.await?.success);
        assert_eq!(app.cron.scheduler.jobs.len(), 1);
        app.cron.stop();
        session.shutdown().await?;
        worker.await??;
        Ok(())
    }

    #[tokio::test]
    async fn model_cron_cancelled_stale_and_closed_requests_never_mutate() -> color_eyre::Result<()>
    {
        use std::sync::atomic::Ordering;
        let (mut app, _events, _ops) = crate::app::tests::make_test_app_with_channels().await;
        let (session, _observed, worker) =
            super::super::app_server_requests::retained_reply_tests::connection().await?;
        let caller = ThreadId::new();
        app.ensure_thread_channel(caller)
            .store
            .lock()
            .await
            .set_active_turn_id("caller-turn".into());
        let connection = session.cron_tool_connection();
        let (request, response) = model_request(
            connection.clone(),
            caller,
            "dropped",
            "add --every 1m -- 확인",
        );
        drop(response);
        app.handle_cron_tool_call(&session, request).await;
        for case in [
            "cancelled",
            "other-turn",
            "other-connection",
            "closed",
            "disconnected",
        ] {
            let (mut request, response) =
                model_request(connection.clone(), caller, case, "add --every 1m -- 확인");
            match case {
                "cancelled" => request.claim.store(2, Ordering::Release),
                "other-turn" => request.params.turn_id = "old-turn".into(),
                "other-connection" => request.connection.id = uuid::Uuid::new_v4(),
                "closed" => {
                    app.cron.closed_threads.insert((connection.backend, caller));
                }
                "disconnected" => {
                    app.cron.closed_threads.clear();
                    connection.live.store(false, Ordering::Release);
                }
                _ => unreachable!(),
            }
            app.handle_cron_tool_call(&session, request).await;
            assert!(response.await.map_or(true, |response| !response.success));
            assert!(app.cron.scheduler.jobs.is_empty(), "{case}");
        }
        session.shutdown().await?;
        worker.await??;
        Ok(())
    }

    #[tokio::test]
    async fn model_cron_unsupported_scheduler_preserves_live_call_arguments_and_reply()
    -> color_eyre::Result<()> {
        let (mut app, _events, _ops) = crate::app::tests::make_test_app_with_channels().await;
        let (session, _observed, worker) =
            super::super::app_server_requests::retained_reply_tests::connection().await?;
        let caller = ThreadId::new();
        app.active_thread_id = Some(caller);
        app.ensure_thread_channel(caller)
            .store
            .lock()
            .await
            .set_active_turn_id("caller-turn".into());
        for prompt in ["보존할 수동 예약", "미지원 응답 대상"] {
            app.cron_command(
                &session,
                Some(caller),
                &format!("add --every 1m -- {prompt}"),
            )
            .await
            .expect("valid manual scheduler fixture");
        }
        let preserved = app.cron.scheduler.jobs[0].id;
        let unsupported = app.cron.scheduler.jobs[1].id;
        let connection = session.cron_tool_connection();
        let (request, response) =
            model_request(connection.clone(), caller, "live-call-key", "list");
        app.handle_cron_tool_call(&session, request).await;
        let original = response.await?;
        assert!(original.success);
        let now = MonotonicInstant::now();
        app.cron.scheduler.jobs[1].next_due = Some(now);
        let dispatch = app
            .cron
            .scheduler
            .due(now)
            .into_iter()
            .find(|dispatch| dispatch.id == unsupported)
            .expect("pending unsupported scheduler attempt");
        app.handle_cron_reply(
            &session,
            CronReply {
                target: dispatch.target,
                id: dispatch.id,
                attempt: dispatch.attempt,
                result: CronResult::Unsupported,
            },
        )
        .await;
        assert!(connection.live.load(std::sync::atomic::Ordering::Acquire));
        assert_eq!(
            app.active_turn_id_for_thread(caller).await.as_deref(),
            Some("caller-turn")
        );
        assert!(app.cron.unsupported.contains(&connection.backend));
        assert!(
            app.cron
                .scheduler
                .jobs
                .iter()
                .all(|job| matches!(job.phase, Phase::BlockedUnsupported(_)))
        );
        let (request, response) = model_request(
            connection.clone(),
            caller,
            "live-call-key",
            &format!("remove {preserved}"),
        );
        app.handle_cron_tool_call(&session, request).await;
        assert!(!response.await?.success);
        assert_eq!(app.cron.scheduler.jobs.len(), 2);
        assert!(
            app.cron
                .scheduler
                .jobs
                .iter()
                .any(|job| job.id == preserved && !job.removed)
        );
        let (request, response) = model_request(connection, caller, "live-call-key", "list");
        app.handle_cron_tool_call(&session, request).await;
        assert_eq!(
            serde_json::to_value(response.await?)?,
            serde_json::to_value(original)?
        );
        app.cron.stop();
        session.shutdown().await?;
        worker.await??;
        Ok(())
    }

    #[tokio::test]
    async fn model_cron_reconnect_stop_preserves_other_live_connection_calls()
    -> color_eyre::Result<()> {
        let (mut app, _events, _ops) = crate::app::tests::make_test_app_with_channels().await;
        let (replaced, _observed_a, worker_a) =
            super::super::app_server_requests::retained_reply_tests::connection().await?;
        let (surviving, _observed_b, worker_b) =
            super::super::app_server_requests::retained_reply_tests::connection().await?;
        let caller_a = ThreadId::new();
        let caller_b = ThreadId::new();
        app.active_thread_id = Some(caller_b);
        for caller in [caller_a, caller_b] {
            app.ensure_thread_channel(caller)
                .store
                .lock()
                .await
                .set_active_turn_id("caller-turn".into());
        }
        let connection_a = replaced.cron_tool_connection();
        let connection_b = surviving.cron_tool_connection();
        app.cron_command(
            &surviving,
            Some(caller_b),
            "add --every 1m -- 보존할 수동 예약",
        )
        .await
        .expect("valid manual scheduler fixture");
        let preserved = app.cron.scheduler.jobs[0].id;
        let (request, response) = model_request(connection_a.clone(), caller_a, "same-key", "list");
        app.handle_cron_tool_call(&replaced, request).await;
        assert!(response.await?.success);
        let (request, response) = model_request(connection_b.clone(), caller_b, "same-key", "list");
        app.handle_cron_tool_call(&surviving, request).await;
        let original = response.await?;
        assert!(original.success);
        app.active_thread_id = Some(caller_a);
        app.app_server_target = AppServerTarget::Remote {
            endpoint: crate::resolve_remote_addr("ws://127.0.0.1:9")?,
        };
        assert!(app.begin_reconnect());
        assert!(app.reconnect.offline);
        assert_eq!(app.cron.model_calls.len(), 2);
        assert!(connection_b.live.load(std::sync::atomic::Ordering::Acquire));
        // Match finish_reconnect: retire A while preserving the surviving B.
        connection_a
            .live
            .store(false, std::sync::atomic::Ordering::Release);
        app.cron
            .invalidate_backend(connection_a.backend, "원래 연결 교체");
        assert_eq!(app.cron.model_calls.len(), 1);
        // Selection/attachment may restore B after reconnect without ending its active turn.
        app.reconnect.offline = false;
        app.active_thread_id = Some(caller_b);
        assert_eq!(
            app.active_turn_id_for_thread(caller_b).await.as_deref(),
            Some("caller-turn")
        );
        let (request, response) = model_request(
            connection_b.clone(),
            caller_b,
            "same-key",
            &format!("remove {preserved}"),
        );
        app.handle_cron_tool_call(&surviving, request).await;
        assert!(!response.await?.success);
        assert!(!app.cron.scheduler.jobs[0].removed);
        let (request, response) = model_request(connection_b, caller_b, "same-key", "list");
        app.handle_cron_tool_call(&surviving, request).await;
        assert_eq!(
            serde_json::to_value(response.await?)?,
            serde_json::to_value(original)?
        );
        replaced.shutdown().await?;
        surviving.shutdown().await?;
        worker_a.await??;
        worker_b.await??;
        Ok(())
    }

    #[tokio::test]
    async fn model_cron_real_embedded_worker_end_fences_queued_call_and_preserves_other_backend()
    -> color_eyre::Result<()> {
        use crate::app::backend_sessions::BackendSessions;
        use codex_app_server_client::AppServerEvent;
        use codex_utils_absolute_path::test_support::PathExt;
        let (mut app, mut events, _ops) = crate::app::tests::make_test_app_with_channels().await;
        let home = tempfile::tempdir()?;
        app.config.codex_home = home.path().to_path_buf().abs();
        app.config.sqlite = codex_state::SqliteConfig::new_for_testing(home.path().abs());
        let config = app.config.clone();
        let (ready_tx, ready_rx) = tokio::sync::oneshot::channel();
        let (stop_tx, stop_rx) = tokio::sync::oneshot::channel();
        let runtime_thread = std::thread::spawn(move || -> color_eyre::Result<()> {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()?;
            let result = runtime.block_on(async {
                let mut session = crate::start_embedded_app_server_for_picker(&config).await?;
                let started = session.start_thread(&config).await?;
                ready_tx
                    .send((session, started))
                    .map_err(|_| color_eyre::eyre::eyre!("fixture consumer ended"))?;
                stop_rx.await?;
                Ok::<_, color_eyre::Report>(())
            });
            // Ending the owning runtime really drops the in-process worker/event sender.
            runtime.shutdown_timeout(Duration::from_secs(5));
            result
        });
        let (origin, started) = ready_rx.await?;
        let caller = started.session.thread_id;
        app.ensure_thread_channel(caller)
            .store
            .lock()
            .await
            .set_session(started.session, started.turns);
        app.ensure_thread_channel(caller)
            .store
            .lock()
            .await
            .set_active_turn_id("caller-turn".into());
        let (surviving, _observed, survivor_worker) =
            super::super::app_server_requests::retained_reply_tests::connection().await?;
        let other = ThreadId::new();
        app.active_thread_id = Some(other);
        app.ensure_thread_channel(other)
            .store
            .lock()
            .await
            .set_active_turn_id("caller-turn".into());
        app.cron_command(
            &surviving,
            Some(other),
            "add --every 1m -- 살아 있는 backend 예약",
        )
        .await
        .expect("valid manual scheduler fixture");
        for (session, thread) in [(&origin, caller), (&surviving, other)] {
            let (request, response) = model_request(
                session.cron_tool_connection(),
                thread,
                "accepted-list",
                "list",
            );
            app.handle_cron_tool_call(session, request).await;
            assert!(response.await?.success);
        }
        app.active_thread_id = Some(caller);
        let connection = origin.cron_tool_connection();
        let owner = connection.backend;
        let other_connection = surviving.cron_tool_connection();
        let handle = origin.request_handle();
        let sender = app.app_event_tx.clone();
        let pending_connection = connection.clone();
        let pending = tokio::spawn(async move {
            crate::cron_tools::execute(pending_connection, handle,
                codex_app_server_protocol::DynamicToolCallParams {
                    thread_id:caller.to_string(), turn_id:"caller-turn".into(), call_id:"queued-before-worker-end".into(),
                    namespace:Some(crate::dynamic_tools::NAMESPACE.into()), tool:crate::cron_tools::NAME.into(),
                    arguments:serde_json::json!({"action":"add","every":"1m","prompt":"종료 후 등록 금지"}),
                }, &sender, CancellationToken::new()).await
        });
        let queued = tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                if let Some(AppEvent::CronToolCall(request)) = events.recv().await {
                    break request;
                }
            }
        })
        .await?;
        assert!(connection.live.load(std::sync::atomic::Ordering::Acquire));
        let mut backends = BackendSessions::from_initial(origin, vec![surviving]);
        stop_tx.send(()).expect("owned fixture runtime is waiting");
        tokio::task::spawn_blocking(move || {
            runtime_thread.join().expect("fixture runtime panicked")
        })
        .await??;
        loop {
            let (polled_owner, event) = tokio::time::timeout(
                Duration::from_secs(10),
                backends.next_event(Some(other_connection.backend)),
            )
            .await?;
            assert_eq!(polled_owner, owner);
            if let Some(event) = event {
                assert!(!matches!(
                    &event.event,
                    AppServerEvent::Disconnected { .. } | AppServerEvent::Lagged { .. }
                ));
                app.handle_polled_backend_event(backends.session(owner).unwrap(), owner, event)
                    .await;
            } else {
                assert!(connection.live.load(std::sync::atomic::Ordering::Acquire));
                assert_eq!(
                    app.active_turn_id_for_thread(caller).await.as_deref(),
                    Some("caller-turn")
                );
                app.handle_backend_stream_closed(backends.session(owner).unwrap());
                break;
            }
        }
        assert!(!backends.is_listening(owner));
        assert!(matches!(app.app_server_target, AppServerTarget::Embedded));
        assert!(!app.begin_reconnect());
        assert!(!app.reconnect.offline);
        assert!(!connection.live.load(std::sync::atomic::Ordering::Acquire));
        let mut tui = crate::tui::test_support::make_test_tui()?;
        let slot = backends.slot(owner).unwrap();
        app.handle_event(
            &mut tui,
            backends.session_at_mut(slot),
            AppEvent::CronToolCall(queued),
        )
        .await?;
        assert!(!pending.await?.success);
        assert_eq!(app.cron.scheduler.jobs.len(), 1);
        assert_eq!(
            app.cron.scheduler.jobs[0].target.backend,
            other_connection.backend
        );
        assert!(matches!(app.cron.scheduler.jobs[0].phase, Phase::Ready));
        assert_eq!(app.cron.model_calls.len(), 1);
        assert!(
            other_connection
                .live
                .load(std::sync::atomic::Ordering::Acquire)
        );
        let (request, response) = model_request(
            other_connection,
            other,
            "accepted-list",
            "add --every 2m -- 변경 거부",
        );
        app.handle_cron_tool_call(
            backends.session_at(backends.slot(request.connection.backend).unwrap()),
            request,
        )
        .await;
        assert!(!response.await?.success);
        backends.shutdown().await?;
        survivor_worker.await??;
        Ok(())
    }

    #[tokio::test]
    async fn model_cron_background_transport_loss_fences_queued_call_only_for_its_owner()
    -> color_eyre::Result<()> {
        use crate::app_server_session::AppServerSessionEvent;
        use codex_app_server_client::AppServerEvent;
        for lost in [
            AppServerEvent::Disconnected {
                message: "background lost".into(),
            },
            AppServerEvent::Lagged { skipped: 1 },
        ] {
            let (mut app, mut events, _ops) =
                crate::app::tests::make_test_app_with_channels().await;
            let (session, _observed, worker) =
                super::super::app_server_requests::retained_reply_tests::connection().await?;
            let caller = ThreadId::new();
            let displayed = ThreadId::new();
            let displayed_owner = BackendId::new();
            app.active_thread_id = Some(displayed);
            app.ensure_thread_channel(caller)
                .store
                .lock()
                .await
                .set_active_turn_id("caller-turn".into());
            app.chat_widget
                .restore_user_message_to_composer("다른 backend 초안".into());
            let now = MonotonicInstant::now();
            let manual = app
                .cron
                .scheduler
                .add(
                    CronTarget {
                        backend: displayed_owner,
                        epoch: 0,
                        thread: displayed,
                    },
                    "다른 backend 수동 예약".into(),
                    Schedule::Every(Duration::from_secs(60)),
                    now,
                )
                .expect("valid manual scheduler fixture");
            let connection = session.cron_tool_connection();
            let (request, response) = model_request(
                connection.clone(),
                caller,
                "queued-before-loss",
                "add --every 1m -- 중단 대상",
            );
            app.app_event_tx
                .send(AppEvent::CronToolCall(Box::new(request)));
            let queued = loop {
                if let Some(AppEvent::CronToolCall(request)) = events.recv().await {
                    break request;
                }
            };
            app.handle_polled_backend_event(
                &session,
                displayed_owner,
                AppServerSessionEvent {
                    backend_id: displayed_owner,
                    event: AppServerEvent::Lagged { skipped: 1 },
                },
            )
            .await;
            assert!(
                connection.live.load(std::sync::atomic::Ordering::Acquire),
                "외부 소유 사건은 원연결을 바꾸지 않음"
            );
            app.handle_polled_backend_event(
                &session,
                displayed_owner,
                AppServerSessionEvent {
                    backend_id: session.backend_id(),
                    event: lost,
                },
            )
            .await;
            app.handle_cron_tool_call(&session, *queued).await;
            assert!(!response.await?.success);
            assert!(!connection.live.load(std::sync::atomic::Ordering::Acquire));
            assert_eq!(app.cron.scheduler.jobs.len(), 1);
            assert_eq!(app.cron.scheduler.jobs[0].id, manual);
            assert!(matches!(app.cron.scheduler.jobs[0].phase, Phase::Ready));
            assert!(!app.reconnect.offline);
            assert_eq!(
                app.chat_widget.composer_text_with_pending(),
                "다른 backend 초안"
            );
            app.cron.stop();
            session.shutdown().await?;
            worker.await??;
        }
        Ok(())
    }

    #[tokio::test]
    async fn model_cron_native_resolved_cancels_queued_call_before_worker_drop()
    -> color_eyre::Result<()> {
        use codex_app_server_client::AppServerEvent;
        use codex_utils_absolute_path::test_support::PathExt;
        let (mut app, mut events, _ops) = crate::app::tests::make_test_app_with_channels().await;
        let home = tempfile::tempdir()?;
        app.config.codex_home = home.path().to_path_buf().abs();
        app.config.sqlite = codex_state::SqliteConfig::new_for_testing(home.path().abs());
        let mut session =
            Box::pin(crate::start_embedded_app_server_for_picker(&app.config)).await?;
        let caller = session.start_thread(&app.config).await?.session.thread_id;
        app.ensure_thread_channel(caller)
            .store
            .lock()
            .await
            .set_active_turn_id("caller-turn".into());
        let (request, _response) = model_request(
            session.cron_tool_connection(),
            caller,
            "native-resolved",
            "",
        );
        let mut params = request.params;
        params.arguments =
            serde_json::json!({"action":"add","every":"1m","prompt":"중단된 native 요청"});
        let request_id = AppServerRequestId::Integer(9001);
        app.handle_app_server_event(
            &session,
            AppServerEvent::ServerRequest(Box::new(ServerRequest::DynamicToolCall {
                request_id: request_id.clone(),
                params,
            })),
        )
        .await;
        let queued = tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                if let Some(AppEvent::CronToolCall(request)) = events.recv().await {
                    break request;
                }
            }
        })
        .await?;
        assert!(!queued.cancellation.is_cancelled());
        app.handle_app_server_event(
            &session,
            AppServerEvent::ServerNotification(Box::new(
                ServerNotification::ServerRequestResolved(
                    codex_app_server_protocol::ServerRequestResolvedNotification {
                        thread_id: caller.to_string(),
                        request_id,
                    },
                ),
            )),
        )
        .await;
        // No yield or JoinHandle await here: the native worker has not dropped its future.
        assert!(queued.cancellation.is_cancelled());
        assert_eq!(queued.claim.load(std::sync::atomic::Ordering::Acquire), 0);
        app.handle_cron_tool_call(&session, *queued).await;
        assert!(app.cron.scheduler.jobs.is_empty());
        app.cron.stop();
        session.shutdown().await?;
        Ok(())
    }

    #[tokio::test]
    async fn model_cron_mcp_metadata_and_cancelled_queue_fail_closed() -> color_eyre::Result<()> {
        use codex_utils_absolute_path::test_support::PathExt;
        use serde_json::json;
        let (mut app, mut events, _ops) = crate::app::tests::make_test_app_with_channels().await;
        let home = tempfile::tempdir()?;
        app.config.codex_home = home.path().to_path_buf().abs();
        app.config.sqlite = codex_state::SqliteConfig::new_for_testing(home.path().abs());
        let mut session =
            Box::pin(crate::start_embedded_app_server_for_picker(&app.config)).await?;
        let source = session.start_thread(&app.config).await?;
        let caller = source.session.thread_id;
        app.ensure_thread_channel(caller)
            .store
            .lock()
            .await
            .set_active_turn_id("caller-turn".into());
        let connection = session.cron_tool_connection();
        let (request, _unused) =
            model_request(connection.clone(), caller, "cancel-before-actor", "");
        let mut params = request.params;
        params.arguments = json!({"action":"add","every":"1m","prompt":"중단된 요청"});
        let handle = session.request_handle();
        let sender = app.app_event_tx.clone();
        let pending_connection = connection.clone();
        let pending = tokio::spawn(async move {
            crate::cron_tools::execute(
                pending_connection,
                handle,
                params,
                &sender,
                CancellationToken::new(),
            )
            .await
        });
        let queued = tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                if let Some(AppEvent::CronToolCall(request)) = events.recv().await {
                    break request;
                }
            }
        })
        .await?;
        pending.abort();
        assert!(pending.await.unwrap_err().is_cancelled());
        app.handle_cron_tool_call(&session, *queued).await;
        assert!(app.cron.scheduler.jobs.is_empty());

        let server = Arc::new(
            crate::dynamic_tools_mcp::DynamicToolMcpServer::start(
                session.request_handle(),
                codex_app_server_protocol::ThreadStartParams::default(),
                app.config.features.get().clone(),
                app.app_event_tx.clone(),
                app.dynamic_tool_status_updates.clone(),
                None,
                crate::dynamic_tools_mcp::ToolServices {
                    task_tools: true,
                    cron: Some(connection),
                    worktrees: None,
                },
            )
            .await?,
        );
        let transport = crate::dynamic_tools_mcp::ThreadToolTransport::Mcp(Arc::clone(&server));
        let mut overrides = None;
        transport.configure_mcp(&mut overrides);
        let config = overrides.unwrap().remove("mcp_servers.codex_tui").unwrap();
        let client = codex_http_client::HttpClientBuilder::new().build_direct()?;
        let call = |id, mut metadata: serde_json::Value| {
            let metadata = metadata.as_object_mut().unwrap();
            metadata.insert(
                "io.modelcontextprotocol/protocolVersion".into(),
                json!("2026-07-28"),
            );
            metadata.insert(
                "io.modelcontextprotocol/clientCapabilities".into(),
                json!({}),
            );
            client.post(config["url"].as_str().unwrap())
            .header("Authorization", config["http_headers"]["Authorization"].as_str().unwrap())
            .header("Accept", "application/json, text/event-stream")
            .header("MCP-Protocol-Version", "2026-07-28")
            .header("MCP-Method", "tools/call")
            .header("MCP-Name", crate::cron_tools::NAME)
            .json(&json!({"jsonrpc":"2.0","id":id,"method":"tools/call","params":{"name":"cron","arguments":{"action":"add","every":"1m","prompt":"MCP 확인"},"_meta":metadata}}))
        };
        for (id, metadata, expected_error) in [
            (
                1,
                json!({"threadId":caller,"turnId":"caller-turn"}),
                "예약에는 실제 turn·call 메타데이터가 필요합니다.",
            ),
            (
                2,
                json!({"threadId":caller,"turnId":"caller-turn","callId":"conflict","x-codex-turn-metadata":{"thread_id":ThreadId::new().to_string(),"turn_id":"caller-turn"}}),
                "호출 메타데이터가 서로 다릅니다.",
            ),
        ] {
            let body = call(id, metadata).send().await?.text().await?;
            let payload: serde_json::Value = serde_json::from_str(
                body.lines()
                    .find_map(|line| line.strip_prefix("data: "))
                    .unwrap_or(&body),
            )?;
            assert_eq!(payload["error"]["code"], json!(-32602), "{body}");
            assert_eq!(payload["error"]["message"], json!(expected_error), "{body}");
            assert!(app.cron.scheduler.jobs.is_empty());
        }
        let request = call(
            3,
            json!({"threadId":caller,"turnId":"caller-turn","callId":"cancelled-mcp"}),
        );
        let mut response = tokio::spawn(async move { request.send().await?.text().await });
        let queued = tokio::select! {
            queued = tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                if let Some(AppEvent::CronToolCall(request)) = events.recv().await {
                    break request;
                }
            }
            }) => queued?,
            early = &mut response => panic!("예약이 앱 대기열에 도달하기 전에 응답됨: {early:?}"),
        };
        // This is the actual RequestContext.ct clone held by the HTTP handler.
        // Do not yield to that handler after cancelling it and before actor admission.
        queued.cancellation.cancel();
        assert_eq!(queued.claim.load(std::sync::atomic::Ordering::Acquire), 0);
        app.handle_cron_tool_call(&session, *queued).await;
        assert!(app.cron.scheduler.jobs.is_empty());
        let body = response.await??;
        assert!(
            body.contains("중단") || body.contains("결과 미확인"),
            "{body}"
        );
        let request = call(
            4,
            json!({"threadId":caller,"turnId":"caller-turn","callId":"old-connection"}),
        );
        let mut response = tokio::spawn(async move { request.send().await?.text().await });
        let queued = tokio::select! {
            queued = tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                if let Some(AppEvent::CronToolCall(request)) = events.recv().await {
                    break request;
                }
            }
            }) => queued?,
            early = &mut response => panic!("예약이 앱 대기열에 도달하기 전에 응답됨: {early:?}"),
        };
        server.suspend();
        app.handle_cron_tool_call(&session, *queued).await;
        let body = response.await??;
        assert!(body.contains("원래 호출 대화"), "{body}");
        assert!(app.cron.scheduler.jobs.is_empty());
        drop(transport);
        drop(server);
        session.shutdown().await?;
        Ok(())
    }

    #[tokio::test]
    async fn model_cron_native_roundtrip_registers_and_survives_resume() -> color_eyre::Result<()> {
        use app_test_support::{
            MockResponsesConfig, create_final_assistant_message_sse_response,
            create_mock_responses_server_sequence_unchecked,
        };
        use codex_login::{AuthCredentialsStoreMode, AuthKeyringBackendKind};
        use codex_model_provider_info::ModelProviderInfo;
        use codex_utils_absolute_path::test_support::PathExt;
        use core_test_support::responses;
        let mut responses = Vec::new();
        for (id, arguments) in [
            (
                "model-cron-add",
                serde_json::json!({"action":"add","every":"10m","prompt":"주기 확인"}),
            ),
            ("model-cron-list", serde_json::json!({"action":"list"})),
        ] {
            responses.push(responses::sse(vec![
                responses::ev_response_created(id),
                responses::ev_function_call_with_namespace(
                    id,
                    crate::dynamic_tools::NAMESPACE,
                    crate::cron_tools::NAME,
                    &arguments.to_string(),
                ),
                responses::ev_completed(id),
            ]));
            responses.push(
                create_final_assistant_message_sse_response("예약 결과 확인")
                    .expect("valid final assistant message fixture"),
            );
        }
        let model_server = create_mock_responses_server_sequence_unchecked(responses).await;
        let (mut app, mut events, _ops) = crate::app::tests::make_test_app_with_channels().await;
        let home = tempfile::tempdir()?;
        MockResponsesConfig::new(&model_server.uri()).write(home.path())?;
        codex_login::login_with_api_key(
            home.path(),
            "sk-test-key",
            AuthCredentialsStoreMode::File,
            AuthKeyringBackendKind::default(),
        )?;
        app.config.codex_home = home.path().to_path_buf().abs();
        app.config.sqlite = codex_state::SqliteConfig::new_for_testing(home.path().abs());
        app.config.cli_auth_credentials_store_mode = AuthCredentialsStoreMode::File;
        app.config.model = Some("mock-model".into());
        app.config.model_provider_id = "test_provider".into();
        app.config.model_provider = ModelProviderInfo {
            name: "Test provider".into(),
            base_url: Some(format!("{}/v1", model_server.uri())),
            request_max_retries: Some(0),
            stream_max_retries: Some(0),
            ..Default::default()
        };
        let mut session =
            Box::pin(crate::start_embedded_app_server_for_picker(&app.config)).await?;
        let started = session.start_thread(&app.config).await?;
        assert!(
            !started.task_tools_available,
            "예약만으로 일반 작업 도구를 활성화하지 않음"
        );
        let caller = started.session.thread_id;
        app.enqueue_primary_thread_session(started.session, started.turns)
            .await?;
        let mut tui = crate::tui::test_support::make_test_tui()?;
        for round in 0..2 {
            if round == 1 {
                let resumed = session
                    .resume_thread(
                        &app.local_settings,
                        app.config.clone(),
                        caller,
                        crate::app_server_session::ResumeModelSettings::PreserveExistingThread,
                    )
                    .await?;
                assert!(!resumed.blocks_direct_input);
            }
            let _: codex_app_server_protocol::TurnStartResponse = session
                .request_handle()
                .request_typed(ClientRequest::TurnStart {
                    request_id: AppServerRequestId::String(uuid::Uuid::new_v4().to_string()),
                    params: codex_app_server_protocol::TurnStartParams {
                        thread_id: caller.to_string(),
                        input: vec![codex_app_server_protocol::UserInput::Text {
                            text: "예약을 확인해줘".into(),
                            text_elements: Vec::new(),
                        }],
                        ..Default::default()
                    },
                })
                .await?;
            tokio::time::timeout(Duration::from_secs(30), async {
                loop {
                    tokio::select! {
                        event = session.next_event() => {
                            let event = event.expect("내장 서버 이벤트");
                            let completed = matches!(&event.event, codex_app_server_client::AppServerEvent::ServerNotification(notification) if matches!(notification.as_ref(), ServerNotification::TurnCompleted(_)));
                            app.handle_backend_event(&session, event).await;
                            if completed { break; }
                        }
                        Some(event) = events.recv() => { app.handle_event(&mut tui, &mut session, event).await?; }
                    }
                }
                Ok::<_, color_eyre::Report>(())
            }).await??;
            assert_eq!(app.cron.scheduler.jobs.len(), 1);
            assert_eq!(app.cron.scheduler.jobs[0].target.thread, caller);
        }
        let requests = model_server.received_requests().await.unwrap();
        let requests: Vec<_> = requests
            .iter()
            .filter(|request| request.url.path().ends_with("/responses"))
            .collect();
        assert_eq!(requests.len(), 4);
        let first: serde_json::Value = serde_json::from_slice(&requests[0].body)?;
        let tools = first["tools"].to_string();
        assert!(tools.contains("cron"));
        assert!(!tools.contains("list_threads"));
        let registered: serde_json::Value = serde_json::from_slice(&requests[1].body)?;
        assert!(registered["input"].to_string().contains("예약 #1 등록"));
        let listed: serde_json::Value = serde_json::from_slice(&requests[3].body)?;
        assert!(listed["input"].to_string().contains("#1 thread="));
        app.cron.stop();
        session.shutdown().await?;
        Ok(())
    }

    #[tokio::test]
    async fn cron_a_survives_plain_b_navigation_and_preserves_b_draft() -> color_eyre::Result<()> {
        use app_test_support::{
            MockResponsesConfig, create_final_assistant_message_sse_response,
            create_mock_responses_server_sequence_unchecked,
        };
        use codex_login::{AuthCredentialsStoreMode, AuthKeyringBackendKind};
        use codex_model_provider_info::ModelProviderInfo;
        use codex_utils_absolute_path::test_support::PathExt;
        let model_server = create_mock_responses_server_sequence_unchecked(vec![
            create_final_assistant_message_sse_response("cron result")
                .expect("valid final assistant message fixture"),
        ])
        .await;
        let (mut app, mut events, _ops) = crate::app::tests::make_test_app_with_channels().await;
        let home = tempfile::tempdir()?;
        MockResponsesConfig::new(&model_server.uri()).write(home.path())?;
        codex_login::login_with_api_key(
            home.path(),
            "sk-test-key",
            AuthCredentialsStoreMode::File,
            AuthKeyringBackendKind::default(),
        )?;
        app.config.codex_home = home.path().to_path_buf().abs();
        app.config.sqlite = codex_state::SqliteConfig::new_for_testing(home.path().abs());
        app.config.cli_auth_credentials_store_mode = AuthCredentialsStoreMode::File;
        app.config.model = Some("mock-model".into());
        app.config.model_provider_id = "test_provider".into();
        app.config.model_provider = ModelProviderInfo {
            name: "Test provider".into(),
            base_url: Some(format!("{}/v1", model_server.uri())),
            request_max_retries: Some(0),
            stream_max_retries: Some(0),
            ..Default::default()
        };
        let mut session =
            Box::pin(crate::start_embedded_app_server_for_picker(&app.config)).await?;
        let started = session.start_thread(&app.config).await?;
        let registered = started.session.thread_id;
        app.enqueue_primary_thread_session(started.session, started.turns)
            .await?;
        app.handle_cron_command(
            &session,
            Some(registered),
            "add --every 10m -- saved cron prompt A".into(),
        )
        .await;
        assert_eq!(app.cron.scheduler.jobs.len(), 1);
        let other = session.start_thread(&app.config).await?;
        let displayed = other.session.thread_id;
        let mut tui = crate::tui::test_support::make_test_tui()?;
        app.detach_current_thread_for_navigation(&mut session, Some(displayed))
            .await;
        app.replace_chat_widget_with_app_server_thread(
            &mut tui,
            other,
            super::super::session_lifecycle::ThreadAttachPresentation::Fresh,
            None,
        )
        .await?;
        assert!(
            app.thread_event_channels.contains_key(&registered),
            "registration ownership must survive reset_thread_event_state"
        );
        app.chat_widget
            .restore_user_message_to_composer("manual draft B".into());
        app.chat_widget.set_model("screen-only-model");
        let owner = session.backend_id();
        let mut backends = backend_sessions::BackendSessions::new(session);
        app.cron.scheduler.jobs[0].next_due = Some(MonotonicInstant::now());
        app.dispatch_due_cron(&backends);
        tokio::time::timeout(Duration::from_secs(15), async {
            loop {
                if matches!(app.cron.scheduler.jobs[0].phase, Phase::Ready) { break Ok::<_, color_eyre::Report>(()); }
                tokio::select! {
                    (event_owner, event) = backends.next_event(None) => {
                        app.handle_backend_event(backends.session(event_owner).unwrap(), event.expect("embedded event stream")).await;
                    }
                    Some(event) = events.recv() => {
                        let slot = backends.slot(owner).unwrap();
                        app.handle_event(&mut tui, backends.session_at_mut(slot), event).await?;
                    }
                }
            }
        }).await??;
        assert_eq!(app.current_displayed_thread_id(), Some(displayed));
        assert_eq!(
            app.chat_widget.composer_text_with_pending(),
            "manual draft B"
        );
        let store = app.thread_event_channels[&registered].store.lock().await;
        assert!(store.buffer.iter().any(|event| matches!(event, ThreadBufferedEvent::Notification(notification) if matches!(notification.as_ref(), ServerNotification::TurnCompleted(completed) if completed.turn.status == codex_app_server_protocol::TurnStatus::Completed))));
        drop(store);
        let requests = model_server.received_requests().await.unwrap();
        let model_requests: Vec<_> = requests
            .iter()
            .filter(|request| request.url.path().ends_with("/responses"))
            .collect();
        assert_eq!(model_requests.len(), 1);
        let body: serde_json::Value = serde_json::from_slice(&model_requests[0].body)?;
        assert_eq!(body["model"], "mock-model");
        let input = body["input"].to_string();
        assert!(input.contains("saved cron prompt A"));
        assert!(!input.contains("manual draft B"));
        app.cron.stop();
        backends.shutdown().await?;
        Ok(())
    }

    #[tokio::test]
    async fn cron_remove_cancel_and_late_reply_keep_exact_turn_until_terminal()
    -> color_eyre::Result<()> {
        use codex_app_server_protocol::{
            JSONRPCMessage, Turn, TurnCompletedNotification, TurnItemsView, TurnStatus,
        };
        let (mut app, mut events, _ops) = crate::app::tests::make_test_app_with_channels().await;
        let (session, mut observed, worker) =
            super::super::app_server_requests::retained_reply_tests::connection().await?;
        let registered = ThreadId::new();
        let displayed = ThreadId::new();
        app.active_thread_id = Some(displayed);
        app.primary_thread_id = Some(displayed);
        app.ensure_thread_channel(registered);
        app.ensure_thread_channel(displayed);
        app.thread_event_channels[&registered]
            .store
            .lock()
            .await
            .active_turn_id = Some("manual-actual-turn-must-not-be-cancelled".into());
        app.chat_widget
            .restore_user_message_to_composer("draft B remains".into());
        let now = MonotonicInstant::now();
        let target = CronTarget {
            backend: session.backend_id(),
            epoch: 0,
            thread: registered,
        };
        let id = app
            .cron
            .scheduler
            .add(
                target.clone(),
                "saved".into(),
                Schedule::Every(Duration::from_secs(10)),
                now,
            )
            .unwrap();
        let dispatch = app
            .cron
            .scheduler
            .due(now + Duration::from_secs(10))
            .pop()
            .unwrap();
        app.cron.scheduler.submitted(
            id,
            dispatch.attempt,
            Submission::Started("owned-cron-turn".into()),
        );
        app.cron_command(&session, Some(displayed), &format!("remove {id}"))
            .await
            .unwrap();
        assert!(app.cron.holds_thread(registered));
        assert!(
            observed.try_recv().is_err(),
            "remove cannot interrupt or unsubscribe a live owner"
        );
        app.cron_command(&session, Some(displayed), &format!("cancel {id}"))
            .await
            .unwrap();
        let request = tokio::time::timeout(Duration::from_secs(5), observed.recv())
            .await?
            .unwrap();
        let JSONRPCMessage::Request(request) = request else {
            panic!("expected owned interrupt");
        };
        assert_eq!(request.method, "turn/interrupt");
        let params = request.params.unwrap();
        assert_eq!(params["threadId"], registered.to_string());
        assert_eq!(params["turnId"], "owned-cron-turn");
        let late_reply = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if let Some(AppEvent::CronCancelReply(reply)) = events.recv().await {
                    break reply;
                }
            }
        })
        .await?;
        assert!(
            app.cron.holds_thread(registered),
            "interrupt ACK cannot release a live turn"
        );
        app.note_cron_notification(
            &session,
            &ServerNotification::TurnCompleted(TurnCompletedNotification {
                thread_id: registered.to_string(),
                turn: Turn {
                    id: "owned-cron-turn".into(),
                    root_turn_id: Some("owned-cron-turn".into()),
                    items: vec![],
                    items_view: TurnItemsView::NotLoaded,
                    error: None,
                    status: TurnStatus::Interrupted,
                    started_at: None,
                    completed_at: None,
                    duration_ms: None,
                },
            }),
        )
        .await;
        assert!(app.cron.scheduler.jobs.is_empty());
        assert!(!app.cron.holds_thread(registered));
        let request = tokio::time::timeout(Duration::from_secs(5), observed.recv())
            .await?
            .unwrap();
        let JSONRPCMessage::Request(request) = request else {
            panic!("expected last-reference unsubscribe");
        };
        assert_eq!(request.method, "thread/unsubscribe");
        assert_eq!(request.params.unwrap()["threadId"], registered.to_string());
        app.handle_cron_cancel_reply(*late_reply);
        assert!(
            app.cron.scheduler.jobs.is_empty(),
            "late cancel reply must not resurrect terminal ownership"
        );
        assert_eq!(
            app.chat_widget.composer_text_with_pending(),
            "draft B remains"
        );
        app.cron.stop();
        session.shutdown().await?;
        worker.await??;
        Ok(())
    }

    #[test]
    fn compatibility_only_classifies_exact_unknown_method_as_unsupported() {
        let error = |code, message: &str| codex_app_server_protocol::JSONRPCErrorError {
            code,
            message: message.into(),
            data: None,
        };
        assert!(idle_method_unsupported(&error(-32601, "Method not found")));
        assert!(idle_method_unsupported(&error(
            -32600,
            "Invalid request: unknown variant `turn/startIfIdle`, expected one of `initialize`"
        )));
        assert!(!idle_method_unsupported(&error(
            -32600,
            "Invalid request: missing field `threadId`"
        )));
        assert!(!idle_method_unsupported(&error(
            -32600,
            "Invalid request: unknown variant `other/method`, expected one of `initialize`"
        )));
        assert!(!idle_method_unsupported(&error(-32603, "unknown outcome")));
    }

    #[test]
    fn parser_requires_explicit_schedule_and_preserves_saved_prompt() {
        let now = MonotonicInstant::now();
        let (schedule, prompt) = parse_add("add --every 10m -- saved -- prompt", now).unwrap();
        assert!(
            matches!(schedule, Schedule::Every(duration) if duration == Duration::from_secs(600))
        );
        assert_eq!(prompt, "saved -- prompt");
        for invalid in [
            "add --every 0m -- prompt",
            "add --every 2d -- prompt",
            "add --every 1초 -- prompt",
            "add --at 2099-01-01T00:00:00 -- prompt",
            "add --every 10m prompt",
        ] {
            assert!(parse_add(invalid, now).is_err());
        }
    }
}
