// Modified for OpenDIY Codex R31 (2026-11-01); see MODIFICATIONS.md.
//! 인증된 Desktop 요청도 기존 예약 상태의 단일 소유 루프에서만 변경한다.
use super::cron_jobs::CronTarget;
use super::*;
use crate::cron::{Phase, Schedule};
use crate::desktop_cron::*;
use std::time::{Instant as MonotonicInstant, SystemTime, UNIX_EPOCH};
use tokio::sync::oneshot;

const MAX_RECEIPTS: usize = 1_024;
const MAX_SAFE_INTEGER: u64 = 9_007_199_254_740_991;

struct Receipt {
    scope: Scope,
    fingerprint: [u8; 32],
    result: MutationResult,
}

/// backend 교체·통로 종료·turn 완료는 이 owner 수명 영수증을 퇴거하지 않는다.
pub(super) struct DesktopOwner {
    pub(super) owner: UuidText,
    receipts: HashMap<String, Receipt>,
}

impl Default for DesktopOwner {
    fn default() -> Self {
        Self {
            owner: UuidText::parse(&uuid::Uuid::new_v4().to_string()).expect("새 소유 UUID"),
            receipts: HashMap::new(),
        }
    }
}

fn mutation_id(mutation: &CronRequest) -> Option<&str> {
    match mutation {
        CronRequest::List {} => None,
        CronRequest::Add { request_id, .. }
        | CronRequest::Pause { request_id, .. }
        | CronRequest::Resume { request_id, .. }
        | CronRequest::Remove { request_id, .. }
        | CronRequest::Cancel { request_id, .. } => Some(request_id),
    }
}

fn outcome(status: MutationStatus, reason: Option<&str>, job_id: Option<u64>) -> MutationResult {
    MutationResult {
        status,
        reason: reason.map(str::to_owned),
        job_id: job_id.map(|id| id.to_string()),
    }
}

enum Admission {
    Reserved,
    Existing(MutationResult),
}

impl DesktopOwner {
    /// 새로운 변경 전에 자리부터 예약한다. 기존 UUID는 포화 상태에서도 조회한다.
    fn admit(&mut self, scope: &Scope, mutation: &CronRequest) -> Result<Admission, &'static str> {
        let id = mutation_id(mutation).ok_or("invalidRequest")?;
        let fingerprint = receipt_fingerprint(scope, mutation).map_err(|_| "internal")?;
        if let Some(receipt) = self.receipts.get(id) {
            return if &receipt.scope == scope && receipt.fingerprint == fingerprint {
                Ok(Admission::Existing(receipt.result.clone()))
            } else {
                Err("requestIdConflict")
            };
        }
        if self.receipts.len() >= MAX_RECEIPTS {
            return Err("receiptsFull");
        }
        if self.receipts.values().any(|receipt| {
            &receipt.scope == scope && matches!(receipt.result.status, MutationStatus::Unknown)
        }) {
            return Err("unresolvedMutation");
        }
        mutation
            .validate_new_schedule()
            .map_err(|_| "invalidSchedule")?;
        self.receipts.insert(
            id.to_owned(),
            Receipt {
                scope: scope.clone(),
                fingerprint,
                result: outcome(MutationStatus::Unknown, None, None),
            },
        );
        Ok(Admission::Reserved)
    }

    fn resolve(&mut self, id: &str, result: MutationResult) {
        if let Some(receipt) = self.receipts.get_mut(id) {
            receipt.result = result;
        }
    }

    fn reconciliation(&self, scope: &Scope, mutation: &CronRequest) -> bool {
        let Some(id) = mutation_id(mutation) else {
            return false;
        };
        let Ok(fingerprint) = receipt_fingerprint(scope, mutation) else {
            return false;
        };
        self.receipts.get(id).is_some_and(|receipt| {
            &receipt.scope == scope
                && receipt.fingerprint == fingerprint
                && !matches!(receipt.result.status, MutationStatus::Unknown)
        })
    }
}

pub(crate) struct DesktopCancelReply {
    target: CronTarget,
    job_id: u64,
    request_id: String,
    result: MutationResult,
    response: Response,
    reply: oneshot::Sender<Response>,
}

impl std::fmt::Debug for DesktopCancelReply {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("DesktopCancelReply { .. }")
    }
}

fn response(request: &Request, scope: Option<Scope>) -> Response {
    Response {
        v: VersionOne,
        kind: ResponseType::Value,
        id: request.id.clone(),
        owner_instance_id: request.owner_instance_id.clone(),
        scope,
        result: None,
        error: None,
    }
}

fn error_response(request: &Request, code: ErrorCode) -> Response {
    let mut response = response(request, request.scope.clone());
    response.error = Some(ResponseError { code });
    response
}

fn schedule(schedule: &CronSchedule, now: MonotonicInstant) -> Result<Schedule, &'static str> {
    match schedule {
        CronSchedule::Every { seconds } => Ok(Schedule::Every(Duration::from_secs(*seconds))),
        CronSchedule::At { at_unix_ms } => {
            let unix_now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_err(|_| "invalidSchedule")?
                .as_millis();
            let delay = u128::from(*at_unix_ms)
                .checked_sub(unix_now)
                .filter(|delay| *delay > 0)
                .ok_or("invalidSchedule")?;
            let deadline = now
                .checked_add(Duration::from_millis(delay as u64))
                .ok_or("invalidSchedule")?;
            Ok(Schedule::Once {
                deadline,
                at_unix_ms: *at_unix_ms,
            })
        }
    }
}

impl App {
    fn desktop_cron_scope(&self, app_server: &AppServerSession, thread: ThreadId) -> Scope {
        Scope {
            backend_id: UuidText::parse(&app_server.backend_id().desktop_cron_uuid().to_string())
                .expect("현재 backend UUID"),
            backend_epoch: DecimalU64::new(self.cron.epoch(app_server.backend_id())),
            thread_id: UuidText::parse(&thread.to_string()).expect("현재 대화 UUID"),
        }
    }

    fn desktop_cron_list(
        &self,
        app_server: &AppServerSession,
        scope: &Scope,
        request: &Request,
    ) -> ListResult {
        let now = MonotonicInstant::now();
        let unix_now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .ok()
            .and_then(|time| u64::try_from(time.as_millis()).ok());
        let jobs = self
            .cron
            .scheduler
            .jobs
            .iter()
            .filter(|job| {
                job.target.backend == app_server.backend_id()
                    && job.target.epoch == scope.backend_epoch.get()
                    && job.target.thread.to_string() == scope.thread_id.as_str()
            })
            .map(|job| Job {
                id: job.id.to_string(),
                prompt_preview: job.prompt.chars().take(512).collect(),
                schedule: match job.schedule {
                    Schedule::Every(interval) => CronSchedule::Every {
                        seconds: interval.as_secs(),
                    },
                    Schedule::Once { at_unix_ms, .. } => CronSchedule::At { at_unix_ms },
                },
                state: match &job.phase {
                    Phase::Ready => JobState::Ready,
                    Phase::Pending { .. } => JobState::Pending,
                    Phase::Running { .. } => JobState::Running,
                    Phase::Finished(_) => JobState::Finished,
                    Phase::Unknown(_) => JobState::Unknown,
                    Phase::BlockedUnsupported(_) => JobState::BlockedUnsupported,
                    Phase::BlockedConnection(_) => JobState::BlockedConnection,
                },
                paused: job.paused,
                removed: job.removed,
                next_run_at: job
                    .next_due
                    .and_then(|due| {
                        unix_now?.checked_add(
                            u64::try_from(due.saturating_duration_since(now).as_millis()).ok()?,
                        )
                    })
                    .filter(|time| (1..=MAX_SAFE_INTEGER).contains(time)),
                skipped: Some(job.skipped.min(MAX_SAFE_INTEGER)),
            })
            .collect();
        let reconciled = request
            .reconcile
            .as_ref()
            .filter(|reconcile| self.cron.desktop.reconciliation(scope, &reconcile.request));
        ListResult {
            supported: true,
            connected: !self.reconnect.offline
                && app_server
                    .cron_tool_connection()
                    .live
                    .load(std::sync::atomic::Ordering::Acquire),
            scope: LifetimeScope::CliLifetime,
            jobs,
            has_more: false,
            supports_expected_turn_interrupt: if self.reconnect.offline {
                None
            } else {
                app_server.supports_expected_turn_interrupt()
            },
            mutation_state: request.reconcile.as_ref().map(|_| {
                if reconciled.is_some() {
                    MutationState::Clear
                } else {
                    MutationState::Unknown
                }
            }),
            reconciled_request_id: reconciled
                .and_then(|reconcile| mutation_id(&reconcile.request))
                .and_then(|id| UuidText::parse(id).ok()),
            reason: None,
        }
    }

    pub(super) async fn handle_desktop_cron_request(
        &mut self,
        app_server: &AppServerSession,
        pending: PendingRequest,
    ) {
        if !pending.claim() {
            let response = error_response(&pending.request, ErrorCode::RequestTimeout);
            let _ = pending.reply.send(response);
            return;
        }
        let request = &pending.request;
        if request.validate().is_err() || request.owner_instance_id != self.cron.desktop.owner {
            let _ = pending
                .reply
                .send(error_response(request, ErrorCode::InvalidRequest));
            return;
        }
        let thread = ThreadId::from_string(request.target_thread_id.as_str());
        let Ok(thread) = thread else {
            let _ = pending
                .reply
                .send(error_response(request, ErrorCode::InvalidRequest));
            return;
        };
        let scope = self.desktop_cron_scope(app_server, thread);
        // 최초 조회도 현재 TUI의 실제 대화와 결합한다. 과거 scope로 이동하지 않는다.
        if self.current_displayed_thread_id() != Some(thread)
            || request
                .scope
                .as_ref()
                .is_some_and(|expected| expected != &scope)
            || self
                .cron
                .closed_threads
                .contains(&(app_server.backend_id(), thread))
        {
            let _ = pending
                .reply
                .send(error_response(request, ErrorCode::StaleTarget));
            return;
        }
        let mut response = response(request, Some(scope.clone()));
        if matches!(request.body, CronRequest::List {}) {
            response.result = Some(ResultBody::List(
                self.desktop_cron_list(app_server, &scope, request),
            ));
            let _ = pending.reply.send(response);
            return;
        }
        let request_id = mutation_id(&request.body)
            .expect("검증한 변경 UUID")
            .to_owned();
        let result = match self.cron.desktop.admit(&scope, &request.body) {
            Ok(Admission::Existing(result)) => {
                response.result = Some(ResultBody::Mutation(result));
                let _ = pending.reply.send(response);
                return;
            }
            Err(reason) => {
                response.result = Some(ResultBody::Mutation(outcome(
                    MutationStatus::NotApplied,
                    Some(reason),
                    None,
                )));
                let _ = pending.reply.send(response);
                return;
            }
            Ok(Admission::Reserved) => {
                // claim 이후 이 접수 구간에는 await가 없다. 늦은 미접수 작업을 적용하지 않는다.
                if tokio::time::Instant::now() >= pending.received_deadline {
                    outcome(MutationStatus::NotApplied, Some("requestTimeout"), None)
                } else if self.reconnect.offline
                    || !app_server
                        .cron_tool_connection()
                        .live
                        .load(std::sync::atomic::Ordering::Acquire)
                {
                    outcome(
                        MutationStatus::NotApplied,
                        Some("backendDisconnected"),
                        None,
                    )
                } else {
                    let target = CronTarget {
                        backend: app_server.backend_id(),
                        epoch: scope.backend_epoch.get(),
                        thread,
                    };
                    match &request.body {
                        CronRequest::Add {
                            schedule: wire_schedule,
                            prompt,
                            ..
                        } => {
                            if self.thread_unavailable(thread)
                                || self.chat_widget.has_misalignment_policy_violation()
                            {
                                outcome(MutationStatus::NotApplied, Some("targetUnavailable"), None)
                            } else {
                                let now = MonotonicInstant::now();
                                let prompt = prompt.clone();
                                match schedule(wire_schedule, now).and_then(|schedule| {
                                    if tokio::time::Instant::now() >= pending.received_deadline {
                                        return Err("requestTimeout");
                                    }
                                    self.cron
                                        .scheduler
                                        .add(target, prompt, schedule, now)
                                        .map_err(|_| "scheduleRejected")
                                }) {
                                    Ok(id) => outcome(MutationStatus::Applied, None, Some(id)),
                                    Err(reason) => {
                                        outcome(MutationStatus::NotApplied, Some(reason), None)
                                    }
                                }
                            }
                        }
                        mutation => {
                            let job_id = match mutation {
                                CronRequest::Pause { job_id, .. }
                                | CronRequest::Resume { job_id, .. }
                                | CronRequest::Remove { job_id, .. }
                                | CronRequest::Cancel { job_id, .. } => job_id,
                                _ => unreachable!("등록은 앞에서 처리합니다."),
                            }
                            .parse::<u64>()
                            .expect("검증한 예약 ID");
                            let owned = self
                                .cron
                                .scheduler
                                .jobs
                                .iter()
                                .any(|job| job.id == job_id && job.target == target);
                            if !owned {
                                outcome(MutationStatus::NotApplied, Some("jobNotOwned"), None)
                            } else if tokio::time::Instant::now() >= pending.received_deadline {
                                outcome(MutationStatus::NotApplied, Some("requestTimeout"), None)
                            } else if matches!(mutation, CronRequest::Cancel { .. }) {
                                match self.claim_cron_cancel(app_server, &target, job_id) {
                                    Err(reason) => {
                                        if reason == "expectedTurnInterruptUnsupported" {
                                            outcome(
                                                MutationStatus::Unsupported,
                                                Some("expectedTurnInterruptUnsupported"),
                                                None,
                                            )
                                        } else {
                                            outcome(
                                                MutationStatus::NotApplied,
                                                Some("cancelRejected"),
                                                None,
                                            )
                                        }
                                    }
                                    Ok(params) => {
                                        let sender = self.app_event_tx.clone();
                                        let handle = app_server.request_handle();
                                        let connection = app_server.cron_tool_connection();
                                        let receipt_id = request_id.clone();
                                        let task = tokio::spawn(async move {
                                            let result = if !connection
                                                .live
                                                .load(std::sync::atomic::Ordering::Acquire)
                                            {
                                                outcome(
                                                    MutationStatus::NotApplied,
                                                    Some("backendDisconnected"),
                                                    None,
                                                )
                                            } else {
                                                match tokio::time::timeout(Duration::from_secs(30), handle.request_typed::<codex_app_server_protocol::TurnInterruptResponse>(
                                                    ClientRequest::TurnInterrupt {
                                                        request_id: codex_app_server_protocol::RequestId::String(uuid::Uuid::new_v4().to_string()), params,
                                                    }
                                                )).await {
                                                    Ok(Ok(_)) => outcome(MutationStatus::Applied, None, Some(job_id)),
                                                    Ok(Err(codex_app_server_client::TypedRequestError::Server { .. })) => outcome(MutationStatus::NotApplied, Some("cancelRejected"), None),
                                                    _ => outcome(MutationStatus::Unknown, Some("cancelResultUnknown"), None),
                                                }
                                            };
                                            let _ = sender.app_event_tx.send(
                                                AppEvent::DesktopCronCancelReply(Box::new(
                                                    DesktopCancelReply {
                                                        target,
                                                        job_id,
                                                        request_id: receipt_id,
                                                        result,
                                                        response,
                                                        reply: pending.reply,
                                                    },
                                                )),
                                            );
                                        });
                                        self.cron
                                            .cancel_tasks
                                            .insert(job_id, (app_server.backend_id(), task));
                                        return;
                                    }
                                }
                            } else {
                                let result = match mutation {
                                    CronRequest::Pause { .. } => self.cron.scheduler.pause(job_id),
                                    CronRequest::Resume { .. } => {
                                        self.cron.scheduler.resume(job_id, MonotonicInstant::now())
                                    }
                                    CronRequest::Remove { .. } => {
                                        self.cron.scheduler.remove(job_id)
                                    }
                                    _ => unreachable!("취소는 앞에서 처리합니다."),
                                };
                                match result {
                                    Ok(()) => outcome(MutationStatus::Applied, None, Some(job_id)),
                                    Err(_) => outcome(
                                        MutationStatus::NotApplied,
                                        Some("scheduleRejected"),
                                        None,
                                    ),
                                }
                            }
                        }
                    }
                }
            }
        };
        // 수신자 소실은 확정 변경을 취소하거나 영수증을 제거하지 않는다.
        self.cron.desktop.resolve(&request_id, result.clone());
        let removed = matches!(request.body, CronRequest::Remove { .. })
            && matches!(result.status, MutationStatus::Applied);
        response.result = Some(ResultBody::Mutation(result));
        let _ = pending.reply.send(response);
        if removed {
            self.release_cron_thread_if_unused(app_server, thread).await;
        }
    }

    pub(super) fn handle_desktop_cron_cancel_reply(&mut self, mut reply: DesktopCancelReply) {
        self.cron.cancel_tasks.remove(&reply.job_id);
        // 과거 scope의 영수증도 owner 수명 끝까지 보존한다. 새 backend로 재송신하지 않는다.
        self.cron
            .desktop
            .resolve(&reply.request_id, reply.result.clone());
        if self.cron.epoch(reply.target.backend) == reply.target.epoch
            && matches!(reply.result.status, MutationStatus::Unknown)
        {
            self.chat_widget.add_error_message(
                "예약 취소 결과를 확인하지 못했습니다. 자동 재전송하지 않습니다.".into(),
            );
        }
        reply.response.result = Some(ResultBody::Mutation(reply.result));
        let _ = reply.reply.send(reply.response);
        // applied는 정확 취소 접수이며 실제 turn 종료·소유권 정리는 종료 알림이 맡는다.
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn scope() -> Scope {
        Scope {
            backend_id: UuidText::parse("11111111-2222-4333-8444-555555555555").unwrap(),
            backend_epoch: DecimalU64::new(7),
            thread_id: UuidText::parse("12345678-1234-4567-8123-123456789abc").unwrap(),
        }
    }

    fn pause(number: u128) -> CronRequest {
        CronRequest::Pause {
            request_id: uuid::Uuid::from_u128(number).to_string(),
            job_id: "1".into(),
        }
    }

    #[test]
    fn owner_receipts_never_evict_and_existing_uuid_survives_capacity() {
        let mut owner = DesktopOwner::default();
        let scope = scope();
        for number in 1..=MAX_RECEIPTS as u128 {
            let mutation = pause(number);
            assert!(matches!(
                owner.admit(&scope, &mutation),
                Ok(Admission::Reserved)
            ));
            owner.resolve(
                mutation_id(&mutation).unwrap(),
                outcome(MutationStatus::Applied, None, Some(1)),
            );
        }
        assert_eq!(owner.receipts.len(), MAX_RECEIPTS);
        assert!(matches!(
            owner.admit(&scope, &pause(1_025)),
            Err("receiptsFull")
        ));
        assert!(matches!(
            owner.admit(&scope, &pause(1)),
            Ok(Admission::Existing(_))
        ));
        assert!(owner.reconciliation(&scope, &pause(1)));
        let changed = CronRequest::Remove {
            request_id: mutation_id(&pause(1)).unwrap().into(),
            job_id: "1".into(),
        };
        assert!(matches!(
            owner.admit(&scope, &changed),
            Err("requestIdConflict")
        ));
        assert!(!owner.reconciliation(&scope, &changed));
        assert_eq!(owner.receipts.len(), MAX_RECEIPTS);
    }

    #[test]
    fn unknown_receipt_blocks_only_its_scope_and_plain_list_cannot_resolve_it() {
        let mut owner = DesktopOwner::default();
        let original_scope = scope();
        let mutation = pause(1);
        assert!(matches!(
            owner.admit(&original_scope, &mutation),
            Ok(Admission::Reserved)
        ));
        assert!(!owner.reconciliation(&original_scope, &mutation));
        assert!(matches!(
            owner.admit(&original_scope, &pause(2)),
            Err("unresolvedMutation")
        ));
        let mut next_scope = original_scope.clone();
        next_scope.backend_epoch = DecimalU64::new(8);
        assert!(matches!(
            owner.admit(&next_scope, &pause(1)),
            Err("requestIdConflict")
        ));
        assert!(matches!(
            owner.admit(&next_scope, &pause(2)),
            Ok(Admission::Reserved)
        ));
        assert!(!owner.reconciliation(&next_scope, &mutation));
        owner.resolve(
            mutation_id(&mutation).unwrap(),
            outcome(
                MutationStatus::Unsupported,
                Some("expectedTurnInterruptUnsupported"),
                None,
            ),
        );
        assert!(owner.reconciliation(&original_scope, &mutation));
        assert!(!owner.reconciliation(&next_scope, &mutation));
        assert!(matches!(
            owner.admit(&original_scope, &pause(3)),
            Ok(Admission::Reserved)
        ));
    }

    fn request(
        app: &App,
        session: &AppServerSession,
        thread: ThreadId,
        body: serde_json::Value,
    ) -> Request {
        let id = body
            .get("requestId")
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned)
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        from_json(
            &serde_json::to_vec(&json!({
                "v":1, "type":"request", "id":id,
                "ownerInstanceId":app.cron.desktop.owner.as_str(),
                "targetThreadId":thread.to_string(),
                "scope":app.desktop_cron_scope(session, thread), "body":body,
            }))
            .unwrap(),
        )
        .unwrap()
    }

    async fn apply(
        app: &mut App,
        session: &AppServerSession,
        request: Request,
    ) -> serde_json::Value {
        let (pending, reply) = PendingRequest::for_test(request, Duration::from_secs(5));
        app.handle_desktop_cron_request(session, pending).await;
        serde_json::to_value(reply.await.unwrap()).unwrap()
    }

    #[tokio::test]
    async fn desktop_actor_duplicate_conflict_expired_and_stale_requests_do_not_mutate()
    -> color_eyre::Result<()> {
        let (mut app, _events, _ops) = crate::app::tests::make_test_app_with_channels().await;
        let (session, _observed, worker) =
            super::super::app_server_requests::retained_reply_tests::connection().await?;
        let thread = ThreadId::new();
        app.active_thread_id = Some(thread);
        app.primary_thread_id = Some(thread);
        app.ensure_thread_channel(thread);
        let id = uuid::Uuid::new_v4().to_string();
        let body = json!({"type":"add", "requestId":id, "schedule":{"type":"every","seconds":60}, "prompt":"원문  프롬프트\n보존"});
        let first = request(&app, &session, thread, body.clone());
        assert_eq!(
            apply(&mut app, &session, first).await["result"]["status"],
            "applied"
        );
        let duplicate = request(&app, &session, thread, body.clone());
        assert_eq!(
            apply(&mut app, &session, duplicate).await["result"]["jobId"],
            "1"
        );
        assert_eq!(app.cron.scheduler.jobs.len(), 1);
        assert_eq!(app.cron.scheduler.jobs[0].prompt, "원문  프롬프트\n보존");
        let mut changed = body.clone();
        changed["prompt"] = json!("다른 본문");
        let conflict = request(&app, &session, thread, changed);
        assert_eq!(
            apply(&mut app, &session, conflict).await["result"]["reason"],
            "requestIdConflict"
        );
        let new_id = uuid::Uuid::new_v4().to_string();
        let mut expired_body = body.clone();
        expired_body["requestId"] = json!(new_id);
        let expired = request(&app, &session, thread, expired_body);
        let (pending, reply) = PendingRequest::for_test(expired, Duration::ZERO);
        app.handle_desktop_cron_request(&session, pending).await;
        assert_eq!(
            serde_json::to_value(reply.await?)?["error"]["code"],
            "requestTimeout"
        );
        let mut stale = request(&app, &session, thread, json!({"type":"list"}));
        stale.scope.as_mut().unwrap().backend_epoch = DecimalU64::new(1);
        assert_eq!(
            apply(&mut app, &session, stale).await["error"]["code"],
            "staleTarget"
        );
        assert_eq!(app.cron.scheduler.jobs.len(), 1);
        assert_eq!(app.cron.desktop.receipts.len(), 1);
        app.cron
            .invalidate_backend(session.backend_id(), "연결 교체");
        assert_eq!(app.cron.desktop.receipts.len(), 1);
        session.shutdown().await?;
        worker.await??;
        Ok(())
    }

    #[tokio::test]
    async fn desktop_actor_cancel_uses_own_connection_cap_and_authoritative_receipt()
    -> color_eyre::Result<()> {
        use codex_app_server_protocol::JSONRPCMessage;
        for capability in [None, Some(false), Some(true)] {
            let (mut app, mut events, _ops) =
                crate::app::tests::make_test_app_with_channels().await;
            let (session, mut observed, worker) = super::super::app_server_requests::retained_reply_tests::connection_with_expected_turn_interrupt(capability).await?;
            let thread = ThreadId::new();
            app.active_thread_id = Some(thread);
            app.primary_thread_id = Some(thread);
            app.ensure_thread_channel(thread);
            let now = MonotonicInstant::now();
            let target = CronTarget {
                backend: session.backend_id(),
                epoch: 0,
                thread,
            };
            let id = app
                .cron
                .scheduler
                .add(
                    target.clone(),
                    "saved".into(),
                    Schedule::Every(Duration::from_secs(1)),
                    now,
                )
                .unwrap();
            let attempt = app
                .cron
                .scheduler
                .due(now + Duration::from_secs(1))
                .pop()
                .unwrap()
                .attempt;
            app.cron.scheduler.submitted(
                id,
                attempt,
                crate::cron::Submission::Started("owned-cron".into()),
            );
            let request_id = uuid::Uuid::new_v4().to_string();
            let body = json!({"type":"cancel", "requestId":request_id, "jobId":id.to_string()});
            let cancel = request(&app, &session, thread, body.clone());
            let (pending, reply) = PendingRequest::for_test(cancel, Duration::from_secs(5));
            app.handle_desktop_cron_request(&session, pending).await;
            if capability == Some(true) {
                let message = tokio::time::timeout(Duration::from_secs(5), observed.recv())
                    .await?
                    .unwrap();
                let JSONRPCMessage::Request(message) = message else {
                    panic!("정확 취소 요청");
                };
                assert_eq!(message.method, "turn/interrupt");
                assert_eq!(message.params.unwrap()["turnId"], "owned-cron");
                let event = tokio::time::timeout(Duration::from_secs(5), async {
                    loop {
                        if let Some(AppEvent::DesktopCronCancelReply(reply)) = events.recv().await {
                            break *reply;
                        }
                    }
                })
                .await?;
                app.handle_desktop_cron_cancel_reply(event);
                assert_eq!(
                    serde_json::to_value(reply.await?)?["result"]["status"],
                    "applied"
                );
                assert!(matches!(
                    app.cron.scheduler.jobs[0].phase,
                    Phase::Running { .. }
                ));
            } else {
                assert_eq!(
                    serde_json::to_value(reply.await?)?["result"]["status"],
                    "unsupported"
                );
                assert!(observed.try_recv().is_err());
                assert!(matches!(
                    app.cron.scheduler.jobs[0].phase,
                    Phase::Running {
                        cancelling: false,
                        ..
                    }
                ));
            }
            let mut list = request(&app, &session, thread, json!({"type":"list"}));
            list.reconcile = Some(Reconcile {
                request: from_json(&serde_json::to_vec(&body)?).unwrap(),
            });
            let result = apply(&mut app, &session, list).await;
            assert_eq!(result["result"]["mutationState"], "clear");
            assert_eq!(result["result"]["reconciledRequestId"], request_id);
            assert_eq!(
                result["result"]["supportsExpectedTurnInterrupt"],
                capability.map_or(serde_json::Value::Null, serde_json::Value::Bool)
            );
            assert_eq!(app.cron.desktop.receipts.len(), 1);
            session.shutdown().await?;
            worker.await??;
        }
        Ok(())
    }
}
