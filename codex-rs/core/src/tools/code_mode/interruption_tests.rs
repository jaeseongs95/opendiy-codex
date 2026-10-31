use super::*;
use crate::session::TurnInput;
use crate::session::tests::make_session_and_context_with_rx;
use crate::state::TaskKind;
use crate::tasks::SessionTask;
use crate::tasks::SessionTaskResult;
use codex_features::Feature;
use codex_protocol::protocol::EventMsg;
use codex_protocol::protocol::TurnAbortReason;
use std::sync::Mutex;
use std::sync::atomic::AtomicUsize;

#[derive(Default)]
struct RecordingSession(Mutex<Vec<CellId>>);
impl CodeModeSession for RecordingSession {
    fn execute<'a>(
        &'a self,
        _request: codex_code_mode::ExecuteRequest,
        _delegate: Arc<dyn codex_code_mode::CodeModeSessionDelegate>,
        _preempt: Option<CancellationToken>,
    ) -> codex_code_mode::CodeModeSessionResultFuture<'a, codex_code_mode::StartedCell> {
        Box::pin(async { Err("not used by interruption test".into()) })
    }
    fn wait<'a>(
        &'a self,
        request: codex_code_mode::WaitRequest,
        _preempt: Option<CancellationToken>,
    ) -> codex_code_mode::CodeModeSessionResultFuture<'a, codex_code_mode::WaitOutcome> {
        self.terminate(request.cell_id)
    }
    fn terminate<'a>(
        &'a self,
        cell_id: CellId,
    ) -> codex_code_mode::CodeModeSessionResultFuture<'a, codex_code_mode::WaitOutcome> {
        self.0.lock().unwrap().push(cell_id.clone());
        Box::pin(async move {
            Ok(codex_code_mode::WaitOutcome::MissingCell(
                RuntimeResponse::Result {
                    code_mode_host_duration: None,
                    result_buffered_for: Duration::ZERO,
                    error_text: None,
                    cell_id,
                    content_items: Vec::new(),
                },
            ))
        })
    }
    fn shutdown<'a>(&'a self) -> codex_code_mode::CodeModeSessionResultFuture<'a, ()> {
        Box::pin(async { Ok(()) })
    }
}
struct CountingTask(Arc<AtomicUsize>);
impl SessionTask for CountingTask {
    fn kind(&self) -> TaskKind {
        TaskKind::Regular
    }
    fn span_name(&self) -> &'static str {
        "session_task.exact_abort_test"
    }
    async fn run(
        self: Arc<Self>,
        _session: Arc<Session>,
        _ctx: Arc<TurnContext>,
        _input: Vec<TurnInput>,
        cancellation: CancellationToken,
    ) -> SessionTaskResult {
        cancellation.cancelled().await;
        Ok(None)
    }
    async fn abort(&self, _session: Arc<Session>, _ctx: Arc<TurnContext>) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

#[tokio::test]
async fn exact_abort_terminates_snapshot_and_preserves_later_turn_cells_and_cleanup() {
    for managed in [false, true] {
        for old_cell_present in [false, true] {
            let (session, mut turn, rx) = make_session_and_context_with_rx().await;
            let _ = Arc::make_mut(&mut Arc::get_mut(&mut turn).unwrap().config)
                .features
                .enable(Feature::CodeModeInterrupt);
            let recorder = Arc::new(RecordingSession::default());
            assert!(
                session
                    .services
                    .code_mode_service
                    .session
                    .set(Arc::clone(&recorder) as Arc<dyn CodeModeSession>)
                    .is_ok()
            );
            let old_cell = CellId::new("old-cell".to_owned());
            if old_cell_present {
                session
                    .services
                    .code_mode_service
                    .mark_cell_ready_for_dispatch(&old_cell, None);
            }
            let aborts = Arc::new(AtomicUsize::new(0));
            let old_id = turn.sub_id.clone();
            session
                .spawn_task(
                    Arc::clone(&turn),
                    Vec::new(),
                    CountingTask(Arc::clone(&aborts)),
                )
                .await;
            assert!(
                session
                    .take_turn_if_active("stale-turn", &TurnAbortReason::Interrupted)
                    .await
                    .is_none()
            );
            let claimed = if managed {
                let mut active = session.active_turn.lock().await;
                session.cancel_exact_task(active.as_mut().unwrap().task.as_mut().unwrap());
                active.take().unwrap()
            } else {
                session
                    .take_turn_if_active(&old_id, &TurnAbortReason::Interrupted)
                    .await
                    .unwrap()
            };
            assert!(
                claimed
                    .task
                    .as_ref()
                    .unwrap()
                    .cancellation_token
                    .is_cancelled()
            );
            let new_cell = CellId::new("replacement-cell".to_owned());
            session
                .services
                .code_mode_service
                .mark_cell_ready_for_dispatch(&new_cell, None);
            let replacement = session
                .new_turn_with_default_settings("replacement".into(), Default::default())
                .await;
            session
                .spawn_task(
                    replacement,
                    Vec::new(),
                    CountingTask(Arc::new(AtomicUsize::new(0))),
                )
                .await;
            if managed {
                session.finish_managed_turn_abort(claimed).await;
            } else {
                session
                    .finish_turn_abort(claimed, TurnAbortReason::Interrupted, None)
                    .await;
            }
            let terminated = recorder.0.lock().unwrap().clone();
            assert_eq!(
                terminated,
                if old_cell_present {
                    vec![old_cell]
                } else {
                    Vec::new()
                }
            );
            let active = session.active_turn.lock().await;
            let task = active.as_ref().unwrap().task.as_ref().unwrap();
            assert_eq!(task.turn_context.sub_id, "replacement");
            assert!(!task.cancellation_token.is_cancelled());
            drop(active);
            assert_eq!(aborts.load(Ordering::SeqCst), 1);

            tokio::time::timeout(Duration::from_secs(5), async {
                loop {
                    let event = rx.recv().await.unwrap();
                    if let EventMsg::TurnAborted(aborted) = event.msg {
                        assert_eq!(aborted.turn_id.as_deref(), Some(old_id.as_str()));
                        break;
                    }
                }
            })
            .await
            .unwrap();
            assert!(session.clone_history().await.raw_items().any(|item| {
                matches!(item, codex_protocol::models::ResponseItem::Message { content, .. }
                    if content.iter().any(|part| matches!(part, codex_protocol::models::ContentItem::InputText { text }
                        if text.contains("<turn_aborted>"))))
            }));
            session.abort_all_tasks(TurnAbortReason::Replaced).await;
        }
    }
}

#[tokio::test]
async fn ordinary_interrupt_still_enumerates_current_cells() {
    let (session, _, _) = make_session_and_context_with_rx().await;
    let recorder = Arc::new(RecordingSession::default());
    let service = &session.services.code_mode_service;
    assert!(
        service
            .session
            .set(Arc::clone(&recorder) as Arc<dyn CodeModeSession>)
            .is_ok()
    );
    let old_cell = CellId::new("old".into());
    let new_cell = CellId::new("new".into());
    service.mark_cell_ready_for_dispatch(&old_cell, None);
    let snapshot = service.active_cell_ids();
    service.mark_cell_ready_for_dispatch(&new_cell, None);
    service.interrupt_cells(snapshot).await;
    assert_eq!(*recorder.0.lock().unwrap(), vec![old_cell.clone()]);
    recorder.0.lock().unwrap().clear();
    service.interrupt_active_cells().await;
    let terminated = recorder.0.lock().unwrap();
    assert_eq!(terminated.len(), 2);
    assert!(terminated.contains(&old_cell));
    assert!(terminated.contains(&new_cell));
}
