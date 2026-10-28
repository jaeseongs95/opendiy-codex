//! CLI-lifetime cron integration: immutable targets, normal protocol input, one loop deadline.
use super::*;
use crate::backend_id::BackendId;
use crate::cron::{Phase, Schedule, Scheduler, Submission};
use codex_app_server_protocol::{
    RequestId as AppServerRequestId, TurnInterruptParams, TurnInterruptResponse,
    TurnStartIfIdleParams, TurnStartIfIdleResponse,
};
use std::time::Instant as MonotonicInstant;

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
        let now = MonotonicInstant::now();
        let words: Vec<_> = args.split_whitespace().collect();
        match words.as_slice() {
            [] | ["list"] => {
                if self.cron.scheduler.jobs.is_empty() {
                    return Ok("등록된 예약이 없습니다. CLI를 종료하면 예약도 사라집니다.".into());
                }
                Ok(self
                    .cron
                    .scheduler
                    .jobs
                    .iter()
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
                        format!(
                            "#{} thread={} {} next={} {}{}{} skipped={}",
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
                            job.skipped
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("\n"))
            }
            ["add", ..] => {
                let thread = thread.ok_or("등록 대상 대화가 없습니다.")?;
                if self.current_displayed_thread_id() != Some(thread)
                    || self.thread_unavailable(thread)
                {
                    return Err("등록 대상 대화가 바뀌었거나 사용할 수 없습니다.".into());
                }
                if self.chat_widget.has_misalignment_policy_violation() {
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
                if job.target.backend != app_server.backend_id() {
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
                    .invalidate_backend(reply.target.backend, "turn/startIfIdle 미지원");
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
            self.cron.scheduler.invalidate(
                |target| target.backend == owner && target.thread == thread,
                "대화 종료·삭제·보관: 실행 상태를 자동 추정하지 않음",
            );
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
            create_final_assistant_message_sse_response("cron result")?,
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
